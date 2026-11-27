//! Port of `rexon/qt_wallpaper_engine/engine_update.py` — auto-updating the
//! upstream projects of the Wallpaper Engine tab (dev mode only).
//!
//! The original mixes PURE decisions (which repo to query, how to parse
//! a GitHub API response, who is newer, what to show in the report, where to put a file,
//! what argv the probes have) with side effects (HTTP via `urllib`, reading and
//! writing `~/.config/rexon/dev_upstream_state.json`, `subprocess.run` of
//! busctl/AppImage, unpacking a zip, `shutil`/`os.replace`). Here is only
//! the pure part: input → output. There is NO network in the core at all; environment observations
//! (the API response body, busctl stdout, the contents of `plugin.toml`, presence of
//! files) are passed as parameters, and executing the plans stays with the Python shim
//! `engine_update.py` (both paths are checked against each other by `tests/engine_update_roundtrip.py`).
//!
//! The only exception is [`validate_plugin_zip`]: it READS a local
//! archive (parsing, not mutation), and it lives in the core just like parsing `.rxn`
//! in `manifest_v4`/`ext_utils`. The pure check core is also available
//! separately — [`validate_plugin_entries`].
//!
//! Two tracked targets (see [`targets`]):
//!   * `waywallen` — the interface application, the managed artifact
//!     `~/.local/share/waywallen/waywallen.AppImage`;
//!   * `owe` — open-wallpaper-engine, a plugin zip for waywallen ≥ 0.2.3,
//!     the directory `$XDG_DATA_HOME/waywallen/plugins/<id>/`.
//!
//! ## Bugs of the original fixed during the port (the auto-fix rule)
//!
//! 1. **Losing the installed plugin when unpacking fails.** In
//!    `install_plugin_zip_fs` the `except` branch unconditionally did
//!    `_rmtree_quiet(target)`. If the error happened BEFORE the previous
//!    version was moved to `.bak` (a broken zip, no space, a permission denial on `extract`),
//!    the WORKING installed plugin was deleted, and there was nothing to restore it from
//!    (`moved_to_bak == False`). Here the rollback is controlled by
//!    [`plugin_rollback_plan`]: `target` is deleted only if we created it
//!    ourselves.
//! 2. **`AttributeError` on a corrupted state.** `load_state()` checked
//!    only the top level; `state["targets"]` or `targets[key]` being a non-object
//!    (for example a list after a manual edit of the file) crashed `_target_state` and
//!    `installed_version` with an unhandled `AttributeError`. Here any
//!    non-objects are normalized into an empty object ([`normalize_state`]).
//! 3. **`AttributeError` on `null` in a GitHub response.** `r.get("tag_name", "")`
//!    returns `None` if the key IS there and equals `null` (rather than being absent) —
//!    then `a["name"].endswith(...)` crashed. Here `null` of any field
//!    is treated as an empty string ([`json_str`]).
//! 4. **`version.lstrip("v")`** strips ALL leading `v` (the tag `vv1` → `1`,
//!    the string `version` → `ersion`). Here — [`version_tag_to_state`] with
//!    `removeprefix("v")` semantics.
//! 5. **The exec bit on a downloaded zip.** `download_asset` set `0o755` on any
//!    temporary file, including the plugin archive. An AppImage without `+x` does not pass
//!    validation, and a zip has no reason to be executable — [`download_spec`] gives `0o755`
//!    only for an AppImage, `0o644` for a zip.
//! 6. **Garbage when AppImage installation fails.** `install_appimage` did not clean up
//!    `dest + ".new-update"` after itself if it failed between `copy2` and `os.replace`.
//!    The plan [`appimage_install_plan`] names this file explicitly, and the shim deletes
//!    it in the rollback branch.
//!
//! 7. **The installed version was "remembered" instead of read.** Our waywallen
//!    version was taken from the LIVE daemon (D-Bus), and if it was silent — from
//!    the state, where `mark_installed` writes it. An engine updated bypassing
//!    Rexon (or updated from Rexon but not restarted yet) made both
//!    sources wrong: 0.3.7 on disk, the daemon reports 0.3.5, the state has
//!    0.2.6 from 26.07 (a live measurement of 27.08.2026). Here a first
//!    source is added — `--version` of the binary that will actually be started
//!    ([`binary_version_argv`], [`parse_binary_version`],
//!    [`current_version`]).
//! 8. **The state kept two different "our versions".** `store_report` wrote
//!    `latest`/`update_available`/`note` computed from the LIVE version, but
//!    did not touch the `installed_version` field — and the file said "installed
//!    0.2.6" next to "up to date (0.3.5)". Now the check puts into the state the
//!    same version it compared against.
//! 9. **Choosing the release asset as "the first whose name ends with the suffix".**
//!    While upstream put exactly one file into a release, the rule worked; as
//!    soon as neighbors appeared, the choice broke for BOTH targets (a live
//!    check of 31.08.2026):
//!      * OWE v0.2.8 — four assets, and under `-linux-x86_64.zip` the first
//!        match is `debug-symbols-linux-x86_64.zip`, i.e. debug symbols were downloaded
//!        instead of the plugin. Installation then honestly failed with
//!        "no plugin.toml in the archive root" — the data was intact, but the plugin
//!        stopped installing and updating altogether;
//!      * waywallen v0.3.1 and newer — two AppImages, and under `.AppImage` the first
//!        is `waywallen-<v>-aarch64.AppImage`, i.e. a binary of a
//!        FOREIGN architecture was downloaded.
//!    The fix is [`asset_matches`]: we recognize the artifact by what it
//!    is (`<asset_prefix>-<version>…<asset_suffix>`), rather than crossing out
//!    known extras by name.
//!
//! A debatable quirk left as is (report compatibility):
//! `check_target` in the "no releases" branch sets `update_available = True` on
//! ANY new commit of the default branch — it is not a release, but such repos have no other
//! sign of freshness.

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

// ============================================================================
// Constants
// ============================================================================

/// The OWE plugin id — from the `plugin.toml` of the real release zip; the install directory =
/// `plugins_dir()/<id>`.
pub const OWE_PLUGIN_ID: &str = "org.waywallen.open-wallpaper-engine";

/// The minimum waywallen version for OWE as a plugin (release notes v0.2.3).
pub const OWE_MIN_WAYWALLEN: [u128; 3] = [0, 2, 3];

/// The maximum number of releases/commits to fetch from the API at once.
pub const FETCH_LIMIT: usize = 30;

/// How many entries to show if the base version is unknown.
pub const UNKNOWN_BASE_LIMIT: usize = 10;

/// The marker of the waywallen usage banner — it confirms that the AppImage
/// unpacked and the binary inside runs.
pub const USAGE_MARKER: &str = "usage: waywallen";

/// A dummy flag used to check that the AppImage starts.
pub const VALIDATE_PROBE_FLAG: &str = "--rexon-validate-probe";

/// The flag asking the engine binary for its version.
///
/// ⚠️ The original's comment "the binary does NOT support `--version` — it prints
/// usage (checked live 2026-07-08)" is outdated: waywallen 0.3.x answers
/// `waywallen <version>`. Old builds still print the usage banner —
/// [`parse_binary_version`] recognizes it and returns "", after which the
/// old sources work (daemon/busctl/state).
pub const VERSION_FLAG: &str = "--version";

/// Timeout of asking the binary for its version, seconds.
pub const BINARY_VERSION_TIMEOUT_SECS: u64 = 5;

/// Timeout of the AppImage test launch, seconds.
pub const PROBE_TIMEOUT_SECS: u64 = 30;

/// Timeout of the version query over D-Bus via busctl, seconds.
pub const BUSCTL_TIMEOUT_SECS: u64 = 5;

/// Timeout of a GitHub API request, seconds.
pub const API_TIMEOUT_SECS: u64 = 15;

/// Timeout of downloading an asset, seconds.
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 30;

/// The download chunk size (1 MiB) — cancellation is checked between chunks.
pub const DOWNLOAD_CHUNK: u64 = 1 << 20;

/// The `Accept` header of GitHub API requests.
pub const API_ACCEPT: &str = "application/vnd.github+json";

/// The `User-Agent` header of GitHub API requests.
pub const API_USER_AGENT: &str = "Rexon-dev";

/// The prefix of the plugin unpacking staging directory.
pub const PLUGIN_STAGING_PREFIX: &str = ".rexon-plugin-install-";

/// The staging subdirectory the archive is unpacked into.
pub const PLUGIN_EXTRACT_SUBDIR: &str = "archive";

/// The suffix of the previous version's directory/file (for rollback).
pub const BAK_SUFFIX: &str = ".bak-update";

/// The suffix of the temporary file of the new AppImage version (in the destination directory).
pub const NEW_SUFFIX: &str = ".new-update";

// ============================================================================
// Paths
// ============================================================================

/// The engine's managed AppImage — port of `MANAGED_APPIMAGE`.
pub fn managed_appimage(home: &str) -> String {
    format!("{home}/.local/share/waywallen/waywallen.AppImage")
}

/// The user directory of waywallen plugins — port of `plugins_dir()`.
/// Mirrors the daemon's `standard_plugin_roots()`: `$XDG_DATA_HOME/waywallen/plugins`,
/// fallback `~/.local/share`. An empty variable counts as unset (like
/// `os.environ.get(...) or ...` in Python).
pub fn plugins_dir(xdg_data_home: Option<&str>, home: &str) -> String {
    let base = match xdg_data_home {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => format!("{home}/.local/share"),
    };
    format!("{base}/waywallen/plugins")
}

/// The dev update state file — port of `STATE_PATH`.
pub fn state_path(home: &str) -> String {
    state_path_xdg(None, home)
}

/// The same, taking `$XDG_CONFIG_HOME` into account.
pub fn state_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/dev_upstream_state.json",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// The temporary file of the atomic state write (`tmp+rename`).
pub fn state_tmp_path(state_path: &str) -> String {
    format!("{state_path}.tmp")
}

/// The `plugin.toml` path of the installed OWE plugin.
pub fn owe_manifest_path(plugins_dir: &str) -> String {
    format!("{plugins_dir}/{OWE_PLUGIN_ID}/plugin.toml")
}

// ============================================================================
// Targets
// ============================================================================

/// The target's install method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum InstallKind {
    /// A managed AppImage.
    #[serde(rename = "appimage")]
    AppImage,
    /// A plugin zip for waywallen.
    #[serde(rename = "plugin_zip")]
    PluginZip,
    /// Installation is not supported (report-only).
    #[serde(rename = "")]
    None,
}

impl InstallKind {
    /// The string name, as in the original's `UpstreamTarget.kind` field.
    pub fn as_str(self) -> &'static str {
        match self {
            InstallKind::AppImage => "appimage",
            InstallKind::PluginZip => "plugin_zip",
            InstallKind::None => "",
        }
    }
}

/// One tracked upstream project — port of the `UpstreamTarget` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpstreamTarget {
    /// The id in the state/UI.
    pub key: String,
    /// A human-readable name.
    pub title: String,
    /// slug GitHub («owner/name»).
    pub repo: String,
    /// The start of the release asset name — the artifact's own name, without the version
    /// ("" = unknown, then only the suffix is checked). Several names an artifact has
    /// carried are separated by `|`.
    pub asset_prefix: String,
    /// The release asset suffix (for installable).
    pub asset_suffix: String,
    /// The managed local artifact ("" = none).
    pub managed_path: String,
    /// Whether we can download+install.
    pub installable: bool,
    /// The install method.
    pub kind: InstallKind,
    /// The mode explanation for the UI.
    pub note: String,
}

/// Both targets in the order of the original `TARGETS` tuple. `home` is needed for
/// waywallen's `managed_path` (in Python it is a module constant with `expanduser`).
pub fn targets(home: &str) -> Vec<UpstreamTarget> {
    vec![
        UpstreamTarget {
            key: "waywallen".into(),
            // The project's terminology: waywallen = the APPLICATION with the interface,
            // "the renderer engine" is OWE below. Do not mix them up in labels.
            title: crate::i18n::tr("waywallen (interface app)"),
            repo: "waywallen/waywallen".into(),
            // the asset name: waywallen-<v>[-<build>]-x86_64.AppImage
            // (in v0.1.3/v0.1.4 there was a commit hash between the version and the architecture,
            // in v0.1.7 — a rebuild number, so the middle of the name
            // is not part of the rule).
            asset_prefix: "waywallen".into(),
            // ⚠️ the architecture in the suffix is MANDATORY: since v0.3.1 upstream
            // also attaches an aarch64 build, and a bare ".AppImage"
            // matched it too (and matched it FIRST — see defect 9).
            asset_suffix: "-x86_64.AppImage".into(),
            managed_path: managed_appimage(home),
            installable: true,
            kind: InstallKind::AppImage,
            note: crate::i18n::tr("managed AppImage, full update cycle"),
        },
        UpstreamTarget {
            key: "owe".into(),
            title: crate::i18n::tr("open-wallpaper-engine (renderer engine)"),
            repo: "waywallen/open-wallpaper-engine".into(),
            // the asset name: open-wallpaper-engine-<v>-linux-x86_64.zip since
            // v0.4.0, org.waywallen.open-wallpaper-engine-<v>-linux-x86_64.zip before
            asset_prefix: format!("open-wallpaper-engine|{OWE_PLUGIN_ID}"),
            asset_suffix: "-linux-x86_64.zip".into(),
            managed_path: String::new(),
            installable: true,
            kind: InstallKind::PluginZip,
            note: crate::i18n::tr(
                "plugin zip for waywallen (v0.2.3+), installed through the daemon \
or into ~/.local/share/waywallen/plugins/",
            ),
        },
    ]
}

/// A target by key (`None` — the key is not ours).
pub fn target_by_key(home: &str, key: &str) -> Option<UpstreamTarget> {
    targets(home).into_iter().find(|t| t.key == key)
}

// ============================================================================
// Changelog entries and statuses
// ============================================================================

/// A release asset (name + direct download URL).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// A changelog entry: a release or (in the fallback) a commit of the default branch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ChangelogEntry {
    pub tag: String,
    pub name: String,
    pub published_at: String,
    pub body: String,
    pub assets: Vec<Asset>,
}

/// The check result of one target — port of the `TargetStatus` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TargetStatus {
    pub key: String,
    pub title: String,
    /// Our version ("" = unknown).
    pub installed: String,
    /// The latest upstream tag.
    pub latest: String,
    pub update_available: bool,
    /// The asset URL for installation (installable).
    pub asset_url: String,
    pub installable: bool,
    /// Entries newer than our version, from old to new.
    pub changelog: Vec<ChangelogEntry>,
    /// A human-readable status/error.
    pub note: String,
    /// `false` — "could not check".
    pub ok: bool,
}

// ============================================================================
// Parsing GitHub API responses
// ============================================================================

/// The URL of the repository's release list.
pub fn releases_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases?per_page={FETCH_LIMIT}")
}

/// The URL of the default branch's commit list (`since` — an ISO time or "").
pub fn commits_url(repo: &str, since_iso: &str) -> String {
    let base = format!("https://api.github.com/repos/{repo}/commits?per_page={FETCH_LIMIT}");
    if since_iso.is_empty() {
        base
    } else {
        format!("{base}&since={since_iso}")
    }
}

/// API request headers — (name, value) pairs in the original's order.
pub fn api_headers() -> Vec<(String, String)> {
    vec![
        ("Accept".to_string(), API_ACCEPT.to_string()),
        ("User-Agent".to_string(), API_USER_AGENT.to_string()),
    ]
}

/// A string field of a JSON object. `null` and a missing key give "" —
/// a fix of the original's bug (`dict.get(k, "")` returns `None` on an explicit `null`,
/// and then `.endswith`/`.strip` crashed with `AttributeError`).
fn json_str(obj: &Map<String, Value>, key: &str) -> String {
    match obj.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Python truthiness of a JSON value (for `draft`/`prerelease`).
fn json_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Parse the response of `GET /repos/{repo}/releases` — port of `_fetch_releases`.
///
/// Drops draft/prerelease, the order is kept (the API returns from new to
/// old). A non-array at the top level — an empty list (as in the original).
/// `Err` — the body did not parse as JSON (in Python this is a `json.loads` exception,
/// which `check_target` catches).
pub fn parse_releases(body: &str) -> Result<Vec<ChangelogEntry>, String> {
    let data: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let Some(items) = data.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else { continue };
        if json_truthy(obj.get("draft")) || json_truthy(obj.get("prerelease")) {
            continue;
        }
        let tag = json_str(obj, "tag_name");
        let name = json_str(obj, "name");
        let assets = obj
            .get("assets")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_object)
                    .map(|a| Asset {
                        name: json_str(a, "name"),
                        url: json_str(a, "browser_download_url"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(ChangelogEntry {
            name: if name.is_empty() { tag.clone() } else { name },
            tag,
            published_at: json_str(obj, "published_at"),
            body: json_str(obj, "body").trim().to_string(),
            assets,
        });
    }
    Ok(out)
}

/// Parse the response of `GET /repos/{repo}/commits` — port of `_fetch_commits`.
/// Commits are brought to the changelog format: `tag` = the first 9 characters of the sha,
/// `name` = the first line of the message, `body` = the rest.
pub fn parse_commits(body: &str) -> Result<Vec<ChangelogEntry>, String> {
    let data: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let Some(items) = data.as_array() else {
        return Ok(Vec::new());
    };
    let empty = Map::new();
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else { continue };
        let commit = obj.get("commit").and_then(Value::as_object).unwrap_or(&empty);
        let message = json_str(commit, "message");
        let (head, tail) = match message.split_once('\n') {
            Some((h, t)) => (h.to_string(), t.trim().to_string()),
            None => (message.clone(), String::new()),
        };
        let sha = json_str(obj, "sha");
        let committer = commit
            .get("committer")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        out.push(ChangelogEntry {
            tag: sha.chars().take(9).collect(),
            name: head,
            published_at: json_str(committer, "date"),
            body: tail,
            assets: Vec::new(),
        });
    }
    Ok(out)
}

// ============================================================================
// Versions
// ============================================================================

/// `'v0.2.4'` / `'0.2.4-x86_64'` → `[0, 2, 4]` — port of `_ver_tuple`.
///
/// ⚠️ A deliberate narrowing: Python's `re.findall(r"\d+")` also matches non-ASCII
/// Unicode digits; waywallen tags/versions are ASCII, so only ASCII digits
/// are taken here. Numbers wider than `u128` saturate at its maximum
/// (in Python integers are unbounded, but such "versions" do not occur).
pub fn ver_tuple(v: &str) -> Vec<u128> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in v.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(cur.parse::<u128>().unwrap_or(u128::MAX));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.parse::<u128>().unwrap_or(u128::MAX));
    }
    out
}

/// A release tag → the string for the state. Port of `version.lstrip("v")` with fixed
/// semantics: ONE leading `v` is stripped (`removeprefix`), not all of them in a row.
pub fn version_tag_to_state(version: &str) -> String {
    version.strip_prefix('v').unwrap_or(version).to_string()
}

/// argv asking the engine binary for its version: `<binary> --version`.
///
/// The binary is found with the same search as for starting the daemon
/// ([`crate::we_client::find_daemon_binary`]) — we ask the version of EXACTLY
/// the candidate that will actually be started.
pub fn binary_version_argv(bin: &str) -> Vec<String> {
    vec![bin.to_string(), VERSION_FLAG.to_string()]
}

/// Parse the output of `waywallen --version` (`waywallen 0.3.7`) → `0.3.7`.
///
/// The rules are deliberately narrow, so that old builds' usage banner is not taken
/// for a version:
///   * output containing [`USAGE_MARKER`] does not count as a version at all;
///   * the first token of the form `[v]N.N[.N…]` is accepted — necessarily with a dot (otherwise
///     any number from the help, like `--fps 60`, would become a "version");
///   * a tail after the numbers is allowed only via `-`/`+`
///     (`0.3.7-x86_64`) and is not part of the result.
///
/// Nothing resembling a version → "" (the source counts as unavailable).
pub fn parse_binary_version(stdout: &str) -> String {
    let text = stdout.trim();
    if text.is_empty() || text.to_lowercase().contains(USAGE_MARKER) {
        return String::new();
    }
    for tok in text.split_whitespace() {
        let cand = tok.strip_prefix('v').unwrap_or(tok);
        let head: String = cand
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let tail = &cand[head.len()..];
        let plausible = head.contains('.')
            && head.starts_with(|c: char| c.is_ascii_digit())
            && !head.ends_with('.')
            && (tail.is_empty() || tail.starts_with('-') || tail.starts_with('+'));
        if plausible {
            return head;
        }
    }
    String::new()
}

/// argv of the daemon version query over D-Bus via busctl — port of `current_version`.
pub fn busctl_version_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "get-property",
        crate::waywallen_client::BUS_NAME,
        crate::waywallen_client::OBJECT_PATH,
        crate::waywallen_client::IFACE,
        "Version",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Parse the busctl output (`s "0.2.2"`) — port of the parsing in `current_version`.
/// No quotes — the version is unknown ("").
pub fn parse_busctl_version(stdout: &str) -> String {
    let out = stdout.trim();
    let mut parts = out.split('"');
    match (parts.next(), parts.next()) {
        (Some(_), Some(v)) => v.to_string(),
        _ => String::new(),
    }
}

/// The waywallen engine version — port of `current_version` without side effects.
///
/// The order of sources — from the most truthful to the oldest:
/// * `binary_version` — the `--version` output of THE binary that will actually
///   be started (parsed by [`parse_binary_version`]). It is the only
///   source that knows what lies on disk RIGHT NOW;
/// * `daemon_version` — the `Version` property of the live daemon over D-Bus (the Qt path);
///   passed as `Some(..)` only if the daemon answers and the version is non-empty.
///   ⚠️ This is the version of the RUNNING process: after an AppImage update it keeps
///   reporting the old number for a long time (a live measurement of 27.08.2026: 0.3.7 on disk,
///   the daemon over D-Bus — 0.3.5), so the binary comes BEFORE it;
/// * `busctl_stdout` — stdout of the fallback busctl call (`None` if the call
///   was not made/failed);
/// * `state_installed` — the version written at the last installation. The
///   weakest source: if the engine was updated not from Rexon, it lies indefinitely
///   (a live measurement: 0.2.6 from 26.07 in the state with 0.3.7 on disk).
pub fn current_version(
    binary_version: Option<&str>,
    daemon_version: Option<&str>,
    busctl_stdout: Option<&str>,
    state_installed: &str,
) -> String {
    if let Some(v) = binary_version {
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Some(v) = daemon_version {
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Some(out) = busctl_stdout {
        let v = parse_busctl_version(out);
        if !v.is_empty() {
            return v;
        }
    }
    state_installed.to_string()
}

/// The OWE plugin version from the `plugin.toml` text — port of `owe_installed_version`.
/// Broken TOML/no `[plugin]` section/no `version` → "".
pub fn owe_version_from_manifest(toml_text: &str) -> String {
    let Ok(v) = toml_text.parse::<toml::Value>() else {
        return String::new();
    };
    v.get("plugin")
        .and_then(|p| p.get("version"))
        .map(toml_scalar_str)
        .unwrap_or_default()
}

/// `str(value)` in Python terms for scalar TOML values.
fn toml_scalar_str(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => format_py_float(*f),
        toml::Value::Boolean(true) => "True".to_string(),
        toml::Value::Boolean(false) => "False".to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        // Lists/tables do not occur in these fields; we do not reproduce Python's
        // string representation for them — we return empty.
        _ => String::new(),
    }
}

/// `str(float)` as in Python (the shortest `repr` representation).
fn format_py_float(f: f64) -> String {
    if f == f.trunc() && f.is_finite() && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Our version of a target — port of `installed_version`.
///
/// * `waywallen` → the live engine version (`current_version`, where the binary's
///   `--version` is the first source);
/// * `owe` → the truth from the FS (`plugin.toml`) → the install state → the seen one;
/// * others → the seen one → the installed one.
pub fn installed_version(
    key: &str,
    current_version: &str,
    owe_fs_version: &str,
    state_installed: &str,
    state_seen: &str,
) -> String {
    match key {
        "waywallen" => current_version.to_string(),
        "owe" => first_non_empty(&[owe_fs_version, state_installed, state_seen]),
        _ => first_non_empty(&[state_seen, state_installed]),
    }
}

fn first_non_empty(candidates: &[&str]) -> String {
    candidates
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

// ============================================================================
// The state ~/.config/rexon/dev_upstream_state.json
// ============================================================================

/// Parse the state — port of `load_state`. A missing file, broken JSON, a non-object
/// at the top level give an empty state (the original did not crash only at the top
/// level; nested non-objects are fixed in [`normalize_state`]).
pub fn load_state(text: Option<&str>) -> Map<String, Value> {
    let Some(text) = text else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Bring `state["targets"]` and its records to objects — a fix of the original's
/// bug: `state.setdefault("targets", {}).setdefault(key, {})` crashed with
/// `AttributeError` if the file held a list/string.
pub fn normalize_state(state: &mut Map<String, Value>) {
    if !state.get("targets").map(Value::is_object).unwrap_or(false) {
        state.insert("targets".into(), Value::Object(Map::new()));
    }
    let targets = state
        .get_mut("targets")
        .and_then(Value::as_object_mut)
        .expect("targets только что нормализован в объект");
    let broken: Vec<String> = targets
        .iter()
        .filter(|(_, v)| !v.is_object())
        .map(|(k, _)| k.clone())
        .collect();
    for k in broken {
        targets.insert(k, Value::Object(Map::new()));
    }
}

/// The target's record in the state (created if missing) — port of `_target_state`.
fn target_state<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    normalize_state(state);
    let targets = state
        .get_mut("targets")
        .and_then(Value::as_object_mut)
        .expect("targets нормализован");
    targets
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    targets
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("запись цели нормализована")
}

/// Read a string field of the target's record (without creating the record).
pub fn state_field(state: &Map<String, Value>, key: &str, field: &str) -> String {
    state
        .get("targets")
        .and_then(Value::as_object)
        .and_then(|t| t.get(key))
        .and_then(Value::as_object)
        .map(|e| match e.get(field) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        })
        .unwrap_or_default()
}

/// Record the target's installed version — port of `mark_installed`.
pub fn mark_installed(state: &mut Map<String, Value>, key: &str, version: &str, now_iso: &str) {
    let ts = target_state(state, key);
    ts.insert("installed_version".into(), Value::from(version));
    ts.insert("installed_at".into(), Value::from(now_iso));
}

/// Mark a version as seen — port of `mark_seen`.
pub fn mark_seen(state: &mut Map<String, Value>, key: &str, version: &str, now_iso: &str) {
    let ts = target_state(state, key);
    ts.insert("seen_version".into(), Value::from(version));
    ts.insert("seen_at".into(), Value::from(now_iso));
}

/// Save the target's report into the state — port of `_store_report`. Assets do not get
/// into the report (noise), release bodies do.
pub fn store_report(
    state: &mut Map<String, Value>,
    key: &str,
    st: &TargetStatus,
    now_iso: &str,
) {
    let report: Vec<Value> = st
        .changelog
        .iter()
        .map(|r| {
            let mut m = Map::new();
            m.insert("tag".into(), Value::from(r.tag.clone()));
            m.insert("name".into(), Value::from(r.name.clone()));
            m.insert("published_at".into(), Value::from(r.published_at.clone()));
            m.insert("body".into(), Value::from(r.body.clone()));
            Value::Object(m)
        })
        .collect();
    let ts = target_state(state, key);
    ts.insert("last_check".into(), Value::from(now_iso));
    // The installed version that was COMPARED against goes into the same file: otherwise
    // `installed_version` stays from the moment of the last installation from Rexon and
    // diverges from `note`/`update_available` computed from the live version
    // (measured 27.08.2026: 0.2.6 in the file, while the note said "up to date (0.3.5)").
    // We do not write an empty version — "could not determine" must not overwrite
    // what `mark_installed` wrote.
    if !st.installed.is_empty() {
        ts.insert("installed_version".into(), Value::from(st.installed.clone()));
    }
    ts.insert("latest".into(), Value::from(st.latest.clone()));
    ts.insert("update_available".into(), Value::from(st.update_available));
    ts.insert("note".into(), Value::from(st.note.clone()));
    ts.insert("report".into(), Value::Array(report));
}

/// The state file text — like `json.dump(..., ensure_ascii=False, indent=1)`:
/// a one-space indent, Cyrillic is not escaped, no trailing
/// newline.
pub fn serialize_state(state: &Map<String, Value>) -> String {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    if serde::Serialize::serialize(&Value::Object(state.clone()), &mut ser).is_err() {
        return "{}".to_string();
    }
    String::from_utf8(buf).unwrap_or_else(|_| "{}".to_string())
}

/// The state timestamp — port of `_now_iso` (`%Y-%m-%dT%H:%M:%SZ`, UTC).
pub fn now_iso(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let rest = unix_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Days since the Unix epoch → (year, month, day) in the Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ============================================================================
// Checking targets
// ============================================================================

/// How the network fetch of a target's data ended (executed outside the core).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// The releases request failed (the Python exception text).
    ReleasesFailed(String),
    /// Releases received. An empty list = "no releases", and the shim SHOULD have
    /// gone for commits; here an empty list is treated the same as
    /// `Commits(vec![])` (neither releases nor new commits).
    Releases(Vec<ChangelogEntry>),
    /// No releases, the default branch's commits were received.
    Commits(Vec<ChangelogEntry>),
    /// No releases, the commits request failed (the Python exception text).
    CommitsFailed(String),
}

/// Whether a release asset name fits our target.
///
/// ⚠️ A fix of the ORIGINAL's defect 9 (see the module header). The original took the first
/// asset whose name `endswith(asset_suffix)`, and broke as soon as upstream
/// added more files to a release: for OWE `debug-symbols-…` was chosen, for
/// waywallen — the aarch64 build.
///
/// Why the rule is exactly this. Crossing out known extras by name
/// ("do not take debug-symbols") is pointless: tomorrow `sources.zip`,
/// `sha256sums.txt` or a `-musl-` variant will appear, and we will be back here. So
/// we recognize not "the foreign" but OUR OWN: for both targets in ALL releases (checked
/// live on all 18 OWE releases and 23 waywallen ones, 31.08.2026) the real
/// artifact is named by one scheme
///
/// ```text
/// <artifact name>-<version>[-<anything>]-<architecture><extension>
///     org.waywallen.open-wallpaper-engine-0.2.8-linux-x86_64.zip
///     waywallen-0.3.7-x86_64.AppImage
///     waywallen-0.1.4-g8233394-x86_64.AppImage
/// ```
///
/// that is, the name STARTS with the artifact name, right after it comes the version (a digit)
/// after a hyphen, and the name ENDS with the architecture and extension. Both ends
/// are pinned — the middle (a commit hash, a rebuild number) is free.
/// The release's side files fall outside this scheme: `debug-symbols-…` does not start
/// with the artifact name, `…-aarch64.zip` does not end with our suffix.
///
/// An empty `asset_prefix` = the original's behavior (suffix only): this way
/// targets whose naming scheme we do not know keep working.
///
/// `prefix` may list several artifact names separated by `|` (upstream renamed the
/// OWE archive in v0.4.0); any of them fits.
pub fn asset_matches(name: &str, prefix: &str, suffix: &str) -> bool {
    if !name.ends_with(suffix) {
        return false;
    }
    if prefix.is_empty() {
        return true;
    }
    prefix.split('|').any(|p| {
        let Some(rest) = name.strip_prefix(p).and_then(|r| r.strip_prefix('-')) else {
            return false;
        };
        // the version starts right after the artifact name, and the suffix is a separate
        // tail, not a piece of this same version (`rest` is longer than the suffix)
        rest.starts_with(|c: char| c.is_ascii_digit()) && rest.len() > suffix.len()
    })
}

/// Check one target — port of `check_target` without the network.
///
/// `installed` — the result of [`installed_version`]; `fetch` — what the trip
/// to the API returned. No exceptions escape: any error becomes `ok = false`
/// with a clear `note`.
pub fn check_target(
    target: &UpstreamTarget,
    installed: &str,
    fetch: &FetchOutcome,
) -> TargetStatus {
    let mut st = TargetStatus {
        key: target.key.clone(),
        title: target.title.clone(),
        installable: target.installable,
        installed: installed.to_string(),
        ..Default::default()
    };

    let releases: &[ChangelogEntry] = match fetch {
        FetchOutcome::ReleasesFailed(e) => {
            st.note = crate::i18n::trf("could not check: %1", &[&e]);
            return st;
        }
        FetchOutcome::CommitsFailed(e) => {
            st.note = crate::i18n::trf("no releases, and commits are unavailable: %1", &[&e]);
            return st;
        }
        FetchOutcome::Commits(commits) => {
            st.ok = true;
            st.changelog = commits.iter().rev().cloned().collect();
            st.latest = commits.first().map(|c| c.tag.clone()).unwrap_or_default();
            st.update_available = !commits.is_empty();
            st.note = if commits.is_empty() {
                crate::i18n::tr("no releases and no new commits")
            } else {
                crate::i18n::tr("no releases — showing commits of the default branch")
            };
            return st;
        }
        FetchOutcome::Releases(rels) if rels.is_empty() => {
            // The shim does not get here (an empty list = a trip for commits),
            // but we guard against it: emptiness is equivalent to "no commits".
            st.ok = true;
            st.update_available = false;
            st.note = crate::i18n::tr("no releases and no new commits");
            return st;
        }
        FetchOutcome::Releases(rels) => rels,
    };

    st.ok = true;
    st.latest = releases[0].tag.clone();
    let ours = ver_tuple(&st.installed);
    let newer: Vec<ChangelogEntry> = if ours.is_empty() {
        // the base version is unknown — we honestly show the latest releases
        let shown: Vec<ChangelogEntry> =
            releases.iter().take(UNKNOWN_BASE_LIMIT).cloned().collect();
        st.update_available = true;
        st.note = crate::i18n::trf(
            "our version is unknown — showing the last %1 release(s), newest %2",
            &[&shown.len(), &st.latest],
        );
        shown
    } else {
        let shown: Vec<ChangelogEntry> = releases
            .iter()
            .filter(|r| ver_tuple(&r.tag) > ours)
            .cloned()
            .collect();
        st.update_available = !shown.is_empty();
        st.note = if shown.is_empty() {
            crate::i18n::trf("up to date (%1)", &[&st.installed])
        } else {
            crate::i18n::trf("%1 available (we have %2)", &[&st.latest, &st.installed])
        };
        shown
    };
    // from old to new — easier to read/adopt in order
    st.changelog = newer.into_iter().rev().collect();

    // the asset URL for installation — from THE newest release
    if target.installable && st.update_available {
        for a in &releases[0].assets {
            if asset_matches(&a.name, &target.asset_prefix, &target.asset_suffix) {
                st.asset_url = a.url.clone();
                break;
            }
        }
        if st.asset_url.is_empty() {
            st.note = crate::i18n::trf("%1; the release has no matching asset", &[&st.note]);
        }
    }
    st
}

/// The plain report text — port of `render_changelog_text` (the «Copy
/// all» button). The format is stable: per target a range header, per release — the tag,
/// name, date and full release notes.
pub fn render_changelog_text(statuses: &[TargetStatus]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for st in statuses {
        let base = if st.installed.is_empty() {
            crate::i18n::tr("(unknown)")
        } else {
            st.installed.clone()
        };
        let latest = if st.latest.is_empty() { "?" } else { &st.latest };
        lines.push(format!("═══ {} — {base} → {latest} ═══", st.title));
        lines.push(crate::i18n::trf("Status: %1", &[&st.note]));
        for rel in &st.changelog {
            let date: String = rel.published_at.chars().take(10).collect();
            lines.push(String::new());
            lines.push(format!("— {} ({}) {date}", rel.tag, rel.name));
            let body = rel.body.trim();
            lines.push(if body.is_empty() {
                crate::i18n::tr("(no description)")
            } else {
                body.to_string()
            });
        }
        lines.push(String::new());
    }
    lines.join("\n").trim().to_string()
}

// ============================================================================
// Downloading
// ============================================================================

/// What we download — determines the temporary file name and its permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadKind {
    /// The engine AppImage (`download_asset` with default arguments).
    AppImage,
    /// The OWE plugin zip (`download_plugin_zip`).
    PluginZip,
}

/// Parameters of the download's temporary file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSpec {
    pub prefix: String,
    pub suffix: String,
    /// Permissions of the finished file.
    ///
    /// ⚠️ A fix of the original: `download_asset` set `0o755` on ANY file,
    /// including the plugin archive. An AppImage without `+x` does not pass `validate_appimage`,
    /// and a zip has no reason to be executable — it gets `0o644`.
    pub mode: u32,
    pub chunk: u64,
    pub timeout_secs: u64,
}

/// Download parameters by artifact type — port of the defaults of `download_asset`
/// and `download_plugin_zip`.
pub fn download_spec(kind: DownloadKind) -> DownloadSpec {
    match kind {
        DownloadKind::AppImage => DownloadSpec {
            prefix: "waywallen-new-".into(),
            suffix: ".AppImage".into(),
            mode: 0o755,
            chunk: DOWNLOAD_CHUNK,
            timeout_secs: DOWNLOAD_TIMEOUT_SECS,
        },
        DownloadKind::PluginZip => DownloadSpec {
            prefix: "owe-plugin-".into(),
            suffix: ".zip".into(),
            mode: 0o644,
            chunk: DOWNLOAD_CHUNK,
            timeout_secs: DOWNLOAD_TIMEOUT_SECS,
        },
    }
}

/// The download progress text in MiB — port of the window workers' `done_b >> 20 / total_b >> 20`
/// (a shift, not a division: it rounds down the same way).
pub fn progress_mib(done_bytes: u64, total_bytes: u64) -> (u64, u64) {
    (done_bytes >> 20, total_bytes >> 20)
}

// ============================================================================
// Validating and installing the AppImage
// ============================================================================

/// argv of the AppImage test launch — port of `validate_appimage`.
pub fn appimage_probe_argv(path: &str) -> Vec<String> {
    vec![path.to_string(), VALIDATE_PROBE_FLAG.to_string()]
}

/// The validation verdict of a downloaded AppImage — port of `validate_appimage`.
///
/// * `is_file` / `is_executable` — observations of `os.path.isfile` / `os.access`;
/// * `spawn_error` — the exception text if the launch did not happen at all;
/// * `combined_output` — `stdout + stderr` of the test launch.
pub fn validate_appimage_verdict(
    is_file: bool,
    is_executable: bool,
    spawn_error: Option<&str>,
    combined_output: &str,
) -> (bool, String) {
    if !(is_file && is_executable) {
        return (false, crate::i18n::tr("file not found or not executable"));
    }
    if let Some(e) = spawn_error {
        return (false, crate::i18n::trf("does not start: %1", &[&e]));
    }
    if combined_output.contains(USAGE_MARKER) {
        return (true, crate::i18n::tr("AppImage starts (usage banner received)"));
    }
    (
        false,
        crate::i18n::tr("AppImage did not print the expected usage banner — the file may be corrupt"),
    )
}

/// The plan for replacing the managed AppImage — port of `install_appimage`.
///
/// ⚠️ Writing over `dest` is not allowed: the daemon executes this file, writing gives
/// `[Errno 26] Text file busy`. So the copy is put under a temporary name IN THE SAME
/// folder and atomically renamed — a rename over a running binary
/// is legal (the old inode finishes executing, the new one is picked up on restart).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppimageInstallPlan {
    /// The destination directory (create if missing).
    pub dest_dir: String,
    /// The managed AppImage.
    pub dest: String,
    /// A copy of the previous version for rollback.
    pub bak: String,
    /// The temporary name of the new version next to `dest`.
    pub tmp: String,
    /// Permissions of the new file.
    pub mode: u32,
    /// The version for the state ("" — do not write).
    pub state_version: String,
}

/// Build the AppImage install plan. `version` — the release tag ("" = do not write
/// to the state).
pub fn appimage_install_plan(home: &str, version: &str) -> AppimageInstallPlan {
    appimage_install_plan_for_dest(&managed_appimage(home), version)
}

/// The same from an explicit path of the managed artifact — needed by the shim, where
/// `MANAGED_APPIMAGE` may be overridden (dry-run/tests).
pub fn appimage_install_plan_for_dest(dest: &str, version: &str) -> AppimageInstallPlan {
    let dest_dir = match dest.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => dest[..i].to_string(),
        None => String::new(),
    };
    AppimageInstallPlan {
        dest_dir,
        bak: format!("{dest}{BAK_SUFFIX}"),
        tmp: format!("{dest}{NEW_SUFFIX}"),
        mode: 0o755,
        state_version: if version.is_empty() {
            String::new()
        } else {
            version_tag_to_state(version)
        },
        dest: dest.to_string(),
    }
}

/// The message about a successful AppImage installation.
pub fn appimage_install_ok_message(dest: &str) -> String {
    crate::i18n::trf(
        "installed to %1; the running daemon keeps the old version until it stops — \
restart the wallpaper engine to pick up the new one",
        &[&dest],
    )
}

/// How to replace the engine AppImage without bringing down the running daemon.
///
/// ⛔ Replacing under a live daemon is a known pitfall (27.08.2026): the new version's
/// interface attaches to the old daemon, and when stopped the AppImage wrapper
/// unmounts the image from under the still-living process — SIGBUS. So the order is
/// one: stop → replace → bring up.
///
/// We stop only by HONEST means:
///   * the autostart service [`crate::we_client::AUTOSTART_UNIT`] is active —
///     `systemctl --user stop`, after the replacement `start`. The unit carries
///     `KillMode=mixed`, and systemd stops the chain in the right order;
///   * the daemon is alive but not under our service (Rexon brought it up when opening
///     the tab) — we ask it to exit with its own `Quit` method over D-Bus: it
///     exits by itself, and the wrapper unmounts the image after it. The tab will bring it
///     up again — with its usual autostart on the next access.
/// ⛔ There is no `kill` and no process search by name here, and there must not be
/// (the wrapper and the daemon are two links of one chain, killing the wrapper = SIGBUS).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineSwapPlan {
    /// argv stopping the service BEFORE the replacement.
    pub stop_argv: Option<Vec<String>>,
    /// Ask the daemon to exit with the `Quit` method (the service is not ours or there is none).
    pub quit_daemon: bool,
    /// argv bringing up the service AFTER the replacement (and after a rollback too: the person
    /// must not be left without wallpaper in any outcome).
    pub start_argv: Option<Vec<String>>,
}

/// argv of `systemctl --user is-active --quiet <autostart service>`: code 0 —
/// the service is running.
pub fn engine_unit_active_argv() -> Vec<String> {
    crate::we_client::systemctl_user_argv(&[
        "is-active",
        "--quiet",
        crate::we_client::AUTOSTART_UNIT,
    ])
}

/// argv calling `Quit` on the daemon over D-Bus.
pub fn busctl_quit_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "call",
        crate::waywallen_client::BUS_NAME,
        crate::waywallen_client::OBJECT_PATH,
        crate::waywallen_client::IFACE,
        "Quit",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// How many times and with what step to wait for the daemon to leave after `Quit`, ms.
pub const QUIT_WAIT_STEPS: u32 = 20;
/// The step of waiting for the daemon to leave after `Quit`, ms.
pub const QUIT_WAIT_STEP_MS: u64 = 250;

/// Build an [`EngineSwapPlan`]. `unit_active` — the answer of
/// [`engine_unit_active_argv`], `daemon_running` — the daemon answers on the bus.
pub fn engine_swap_plan(unit_active: bool, daemon_running: bool) -> EngineSwapPlan {
    let unit = crate::we_client::AUTOSTART_UNIT;
    if unit_active {
        return EngineSwapPlan {
            stop_argv: Some(crate::we_client::systemctl_user_argv(&["stop", unit])),
            quit_daemon: false,
            start_argv: Some(crate::we_client::systemctl_user_argv(&["start", unit])),
        };
    }
    EngineSwapPlan {
        stop_argv: None,
        quit_daemon: daemon_running,
        start_argv: None,
    }
}

/// The message about a successful installation when the engine was restarted by the service.
pub fn appimage_install_restarted_message(dest: &str) -> String {
    crate::i18n::trf(
        "Installed to %1; the wallpaper engine was restarted with the new version.",
        &[&dest],
    )
}

/// The message about a successful installation when the engine was stopped and will come up
/// on the tab's next access.
pub fn appimage_install_quit_message(dest: &str) -> String {
    crate::i18n::trf(
        "Installed to %1; the wallpaper engine was stopped and will start with \
the new version the next time the Wallpaper Engine tab needs it.",
        &[&dest],
    )
}

/// The message: the daemon did not leave when asked — the replacement was NOT done.
pub fn appimage_install_busy_message() -> String {
    crate::i18n::tr(
        "The wallpaper engine did not stop, so its file was not replaced. \
Close Rexon, stop the engine and try again.",
    )
}

/// The message about an AppImage installation error (after the rollback).
pub fn appimage_install_fail_message(err: &str) -> String {
    crate::i18n::trf("installation failed, rolled back: %1", &[&err])
}

// ============================================================================
// An engine from distribution packages: the system updates it, not Rexon
// ============================================================================

/// A refusal to install a second copy of a target installed by the package manager.
///
/// Since 30.10.2026 the Rexon package takes the engine as dependencies (`waywallen`,
/// `open-wallpaper-engine`). A copy downloaded by this page would land next to it
/// (`~/.local/share/waywallen/…`) and, being newer, would shadow the packaged one:
/// the daemon takes the newest copy of the plugin, Rexon — the newest engine
/// ([`crate::we_client::prefer_managed_over_bundled`]). Then the person
/// gets stuck on this copy — system updates do not touch it, and it has already rewritten the engine's
/// database for itself. So for targets installed as a distribution package,
/// the page downloads nothing and says who updates them.
///
/// `key` — the target key ([`targets`]); `system_engine` — the distribution package's
/// daemon is on disk ([`crate::we_client::SYSTEM_DAEMON`]), `system_owe` — its
/// OWE plugin (`/usr/share/waywallen/plugins/<OWE_PLUGIN_ID>`). `None` —
/// installing is allowed.
pub fn system_update_refusal(key: &str, system_engine: bool, system_owe: bool) -> Option<String> {
    match key {
        "waywallen" if system_engine => Some(crate::i18n::tr(
            "The waywallen wallpaper engine on this computer is installed as a system \
package (waywallen). It is updated together with the rest of the system, so Rexon \
does not download a second copy of it. Install system updates, and the engine will \
be updated too.",
        )),
        "owe" if system_owe => Some(crate::i18n::tr(
            "Open Wallpaper Engine on this computer is installed as a system package \
(open-wallpaper-engine). It is updated together with the rest of the system, so \
Rexon does not download a second copy of it. Install system updates, and the plugin \
will be updated too.",
        )),
        _ => None,
    }
}

// ============================================================================
// The OWE plugin: compatibility gate, validation, installation
// ============================================================================

/// Whether the installed waywallen is compatible with OWE as a plugin — port of
/// `waywallen_meets_owe_min`. An empty version → `false` with an explanation.
pub fn waywallen_meets_owe_min(version: &str) -> (bool, String) {
    if version.is_empty() {
        return (
            false,
            crate::i18n::tr(
                "waywallen version unknown (the daemon is not running) — \
cannot check plugin compatibility",
            ),
        );
    }
    if ver_tuple(version).as_slice() < OWE_MIN_WAYWALLEN.as_slice() {
        let need = OWE_MIN_WAYWALLEN
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(".");
        return (
            false,
            crate::i18n::trf(
                "waywallen %1 is too old for the OWE plugin (needs ≥ %2: before \
that OWE is built into the AppImage, and a user copy would duplicate it) — \
update waywallen first",
                &[&version, &need],
            ),
        );
    }
    (true, crate::i18n::trf("waywallen %1 is compatible", &[&version]))
}

/// A parsed plugin manifest — port of the `info` dictionary of `validate_plugin_zip`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// The result of checking a plugin archive — port of the `(ok, msg, info)` tuple.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginValidation {
    pub ok: bool,
    pub msg: String,
    pub info: PluginInfo,
}

impl PluginValidation {
    fn fail(msg: impl Into<String>) -> Self {
        PluginValidation {
            ok: false,
            msg: msg.into(),
            info: PluginInfo::default(),
        }
    }
}

/// `repr()` of a string as in CPython: single quotes, but double ones if it contains
/// `'` and no `"` (needed for the message "unsafe plugin id: …").
fn py_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// The pure core of the plugin archive check — port of the body of `validate_plugin_zip`.
///
/// The requirements mirror the daemon's `inspect_zip()`/`install_zip()`
/// (`src/plugin/installer.rs`): `plugin.toml` IN THE ROOT, a non-empty `[plugin].id`
/// without path separators, safe paths inside the archive. The daemon only
/// warns about `files.txt` — here it is not a blocker either, it goes into the message.
///
/// `names` — `ZipFile.namelist()`; `manifest_toml` — the contents of `plugin.toml`
/// (`None` if the archive has no such file).
pub fn validate_plugin_entries(
    names: &[String],
    manifest_toml: Option<&str>,
) -> PluginValidation {
    for n in names {
        let p = n.replace('\\', "/");
        if p.starts_with('/') || p.split('/').any(|seg| seg == "..") {
            return PluginValidation::fail(crate::i18n::trf("unsafe path in the archive: %1", &[n]));
        }
    }
    if !names.iter().any(|n| n == "plugin.toml") {
        return PluginValidation::fail(crate::i18n::tr("no plugin.toml at the archive root"));
    }
    let Some(text) = manifest_toml else {
        return PluginValidation::fail(crate::i18n::tr("no plugin.toml at the archive root"));
    };
    let manifest = match text.parse::<toml::Value>() {
        Ok(v) => v,
        Err(e) => return PluginValidation::fail(crate::i18n::trf("the archive cannot be read: %1", &[&e])),
    };
    let get = |k: &str| {
        manifest
            .get("plugin")
            .and_then(|p| p.get(k))
            .map(toml_scalar_str)
            .unwrap_or_default()
    };
    let info = PluginInfo {
        id: get("id"),
        name: get("name"),
        version: get("version"),
    };
    let pid = info.id.clone();
    if pid.is_empty() || pid.contains('/') || pid.contains('\\') || pid == "." || pid == ".." {
        return PluginValidation {
            ok: false,
            msg: crate::i18n::trf("unsafe plugin id: %1", &[&py_repr(&pid)]),
            info,
        };
    }
    let args: [&dyn std::fmt::Display; 4] = [&info.name, &info.version, &pid, &names.len()];
    let msg = if names.iter().any(|n| n == "files.txt") {
        crate::i18n::trf("plugin “%1” %2 (id %3), files: %4", &args)
    } else {
        crate::i18n::trf(
            "plugin “%1” %2 (id %3), files: %4; ⚠️ no files.txt (the daemon will warn)",
            &args,
        )
    };
    PluginValidation {
        ok: true,
        msg,
        info,
    }
}

/// Check a downloaded plugin zip BEFORE installation — port of `validate_plugin_zip`
/// together with reading the archive (read-only, like parsing `.rxn` in `manifest_v4`).
pub fn validate_plugin_zip(path: &Path) -> PluginValidation {
    if !path.is_file() {
        return PluginValidation::fail(crate::i18n::tr("file not found"));
    }
    let Ok(file) = std::fs::File::open(path) else {
        return PluginValidation::fail(crate::i18n::tr("not a zip archive"));
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return PluginValidation::fail(crate::i18n::tr("not a zip archive")),
    };
    // The order and duplicates — as in `ZipFile.namelist()` (the order of entries in
    // the archive): both "files: N" and which exact path the safety check
    // reports depend on it.
    let mut names: Vec<String> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        match archive.by_index_raw(i) {
            Ok(f) => names.push(f.name().to_string()),
            Err(e) => return PluginValidation::fail(crate::i18n::trf("the archive cannot be read: %1", &[&e])),
        }
    }
    let manifest = archive.by_name("plugin.toml").ok().and_then(|mut f| {
        use std::io::Read;
        let mut buf = String::new();
        f.read_to_string(&mut buf).ok().map(|_| buf)
    });
    validate_plugin_entries(&names, manifest.as_deref())
}

/// The plan for installing the plugin by unpacking into the plugins directory — port of
/// `install_plugin_zip_fs` (the shim performs the file operations themselves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallPlan {
    /// The archive validation passed.
    pub ok: bool,
    /// With `ok == false` — a ready refusal message.
    pub message: String,
    pub info: PluginInfo,
    /// The root of user plugins.
    pub root: String,
    /// The plugin directory `<root>/<id>`.
    pub target: String,
    /// The previous version's directory for rollback.
    pub bak: String,
    /// The prefix of the temporary staging directory.
    pub staging_prefix: String,
    /// The staging subdirectory the archive is unpacked into.
    pub extract_subdir: String,
    /// The version for the state ("" — do not write).
    pub state_version: String,
}

/// Build the plugin install plan. `plugins_root` — a directory override
/// (for dry-run/tests), "" = the default directory from `default_root`.
pub fn plugin_install_plan(
    validation: &PluginValidation,
    default_root: &str,
    plugins_root: &str,
    version: &str,
) -> PluginInstallPlan {
    let root = if plugins_root.is_empty() {
        default_root.to_string()
    } else {
        plugins_root.to_string()
    };
    if !validation.ok {
        return PluginInstallPlan {
            ok: false,
            message: crate::i18n::trf("validation failed: %1", &[&validation.msg]),
            info: validation.info.clone(),
            root,
            target: String::new(),
            bak: String::new(),
            staging_prefix: PLUGIN_STAGING_PREFIX.to_string(),
            extract_subdir: PLUGIN_EXTRACT_SUBDIR.to_string(),
            state_version: String::new(),
        };
    }
    let target = format!("{root}/{}", validation.info.id);
    PluginInstallPlan {
        ok: true,
        message: String::new(),
        info: validation.info.clone(),
        root,
        bak: format!("{target}{BAK_SUFFIX}"),
        target,
        staging_prefix: PLUGIN_STAGING_PREFIX.to_string(),
        extract_subdir: PLUGIN_EXTRACT_SUBDIR.to_string(),
        state_version: if version.is_empty() {
            String::new()
        } else {
            version_tag_to_state(version)
        },
    }
}

/// The file permissions from a zip entry — port of `(zi.external_attr >> 16) & 0o7777`.
/// `ZipFile.extract` does NOT restore unix permissions, while renderer binaries
/// need the exec bit (the daemon does the same in `apply_zip_mode`). `0` = do not touch.
pub fn zip_entry_mode(external_attr: u32) -> u32 {
    (external_attr >> 16) & 0o7777
}

/// What to do when plugin installation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginRollbackPlan {
    /// Delete `target` (only if it appeared through our fault).
    pub remove_target: bool,
    /// Put `.bak` back in place of `target`.
    pub restore_bak: bool,
}

/// The rollback plan of a plugin installation.
///
/// ⚠️ A fix of the original's bug: the `except` branch of `install_plugin_zip_fs`
/// unconditionally did `_rmtree_quiet(target)`. On a failure BEFORE the previous
/// version was moved to `.bak` (a broken archive, no space, a permission denial) this erased the WORKING
/// installed plugin with no way to restore it. Here `target`
/// is deleted only if we created it ourselves.
///
/// * `moved_to_bak` — the previous version has already moved to `.bak`;
/// * `target_created` — we have already managed to create a new `target`.
pub fn plugin_rollback_plan(moved_to_bak: bool, target_created: bool) -> PluginRollbackPlan {
    PluginRollbackPlan {
        remove_target: target_created,
        restore_bak: moved_to_bak,
    }
}

/// The message about a successful FS installation of the plugin.
pub fn plugin_install_ok_message(plugin_id: &str, target: &str) -> String {
    crate::i18n::trf(
        "plugin %1 installed to %2 (previous version in .bak-update); restart \
the waywallen daemon so the renderers pick it up",
        &[&plugin_id, &target],
    )
}

/// The message about an FS installation error of the plugin (after the rollback).
pub fn plugin_install_fail_message(err: &str) -> String {
    crate::i18n::trf("plugin installation failed, rolled back: %1", &[&err])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ver_tuple_basics() {
        assert_eq!(ver_tuple("v0.2.4"), vec![0, 2, 4]);
        assert_eq!(ver_tuple("0.2.4-x86_64"), vec![0, 2, 4, 86, 64]);
        assert!(ver_tuple("").is_empty());
    }

    #[test]
    fn now_iso_epoch() {
        assert_eq!(now_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(now_iso(1_752_969_600), "2025-07-20T00:00:00Z");
    }

    #[test]
    fn version_tag_removes_single_v() {
        assert_eq!(version_tag_to_state("v0.2.5"), "0.2.5");
        assert_eq!(version_tag_to_state("vv1"), "v1");
        assert_eq!(version_tag_to_state("version"), "ersion");
    }
}
