//! Port of `rexon/waywallen_kde_plugin.py` — managing the Plasma wallpaper plugin
//! `org.waywallen.kde` for KDE Plasma 6 (install/upgrade/detect/auto-select).
//!
//! Here is the PURE logic (deterministic input→output), execution is NOT here:
//!   * package paths (`bundle_dir` / `install_dir` / `version_marker`);
//!   * `is_kde` (parsing `XDG_CURRENT_DESKTOP`);
//!   * `file_sha` — version of the runtime `.so` (sha256 → first 16 hex), the executor
//!     reads the file, the hashing happens HERE (`hashlib.sha256(...).hexdigest()[:16]`);
//!   * `resolve_installed_version` — the marker text or the hash of the installed `.so`;
//!   * `install_or_upgrade` — the whole idempotent kpackagetool6 pipeline
//!     (no-op / `-i` / `-u` / fallback `-r`+`-i`) as a DRIVER over a `run` closure
//!     (the rc/stderr of every run is supplied by the executor — no subprocess
//!     runs here), with the same decision logic, the same argv/timeouts and
//!     message texts byte for byte;
//!   * the Plasma scripting JS scripts (`select_plugin` / `current_plugins`) and the argv
//!     for delivering them (`dbus-send` / `qdbus6`), the parser of the `current_plugins` output,
//!     `all_selected`;
//!   * argv of the plasmashell restart (`systemctl --user`).
//!
//! NOT ported (execution stays with the executor/bridge): actually
//! running kpackagetool6/dbus-send/qdbus6/systemctl and reading their output, `which`
//! (the presence of the tool is passed as a parameter), reading/writing the FS (marker, `.so`,
//! metadata.json — their CONTENTS/PRESENCE are passed as parameters), catching the OSError
//! of a subprocess launch (the executor forms the "launch error" itself),
//! the orchestration of `ensure_waywallen_plugin` around live side effects —
//! the corresponding PURE decisions (`should_reselect`, building the final
//! message) are moved here, the calls themselves are not.

use sha2::{Digest, Sha256};

pub const PLUGIN_ID: &str = "org.waywallen.kde";
pub const PLUGIN_TYPE: &str = "Plasma/Wallpaper";

/// Name of the runtime library of the QML module inside the package; its hash = the package version.
///
/// ⚠️ What is stable is the NAME, not the path: upstream has already moved the file inside
/// the package (0.3.3 — from `contents/ui/WaywallenDisplayEmbed/` to
/// `contents/ui/Plugin/WaywallenDisplayEmbed/`). A hard-coded path would then silently
/// break version detection — the "installed" version is not visible, the update does not
/// trigger, the user keeps the old plugin. So the runtime is SEARCHED for by
/// name (`pick_runtime_so_rel` below), and the path below is only a fallback.
pub const RUNTIME_SO_NAME: &str = "libwaywallen-display-qml.so";

/// Runtime layout in the current bundled package. Used as the
/// fallback answer when the name search found nothing (no package, the directory
/// is unreadable) — so that the path in messages/logs stays defined.
pub const RUNTIME_SO_REL: &str =
    "contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so";

/// Name of the marker file with the version of the package we installed.
pub const VERSION_MARKER_NAME: &str = ".rexon-installed-version";

/// kpackagetool candidates in priority order — port of `_kpackagetool`
/// (`shutil.which("kpackagetool6") or shutil.which("kpackagetool5")`).
/// The executor runs `which` in order and passes the first one found.
pub const KPACKAGETOOL_CANDIDATES: &[&str] = &["kpackagetool6", "kpackagetool5"];

/// Timeout of kpackagetool6 runs (seconds) — like `timeout=30` in Python.
pub const KPACKAGETOOL_TIMEOUT: u64 = 30;

// ── paths ───────────────────────────────────────────────────────────────

fn join(parts: &[&str]) -> String {
    parts.join("/")
}

/// The kpackage bundled inside Rexon — port of `_BUNDLE_DIR`
/// (`join(dirname(__file__), "kpackage", PLUGIN_ID)`). `module_dir` — the directory
/// of the `rexon` package (the parent of `waywallen_kde_plugin.py`).
pub fn bundle_dir(module_dir: &str) -> String {
    join(&[module_dir, "kpackage", PLUGIN_ID])
}

/// Where kpackagetool6 installs Plasma/Wallpaper plugins (per user) — port of
/// `_INSTALL_DIR` (`~/.local/share/plasma/wallpapers/{PLUGIN_ID}`).
pub fn install_dir(home: &str) -> String {
    join(&[home, ".local/share/plasma/wallpapers", PLUGIN_ID])
}

/// Marker with the version of the installed package — port of `_VERSION_MARKER`.
pub fn version_marker(home: &str) -> String {
    join(&[&install_dir(home), VERSION_MARKER_NAME])
}

/// Fallback path of the runtime `.so` relative to the package base — `join(base,
/// RUNTIME_SO_REL)`. The main way is the search by name, see
/// `pick_runtime_so_rel`.
pub fn runtime_so_path(base: &str) -> String {
    join(&[base, RUNTIME_SO_REL])
}

/// Pick one runtime `.so` among the paths the executor found by the name
/// `RUNTIME_SO_NAME` inside the package (paths are RELATIVE to the package base, separated by
/// `/`). The choice rule is deterministic, so that both engines on one tree
/// always agree on one file:
///
/// 1. an exact match with `preferred_rel` (the layout of our bundled
///    package) — so the installed instance is compared with the bundle "as with
///    itself", not with an accidentally surviving copy from a previous version;
/// 2. otherwise — the shallowest path;
/// 3. at equal depth — the lexicographically smaller one.
///
/// An empty list → `None` (no package/runtime).
pub fn pick_runtime_so_rel(found_rel: &[String], preferred_rel: Option<&str>) -> Option<String> {
    if let Some(pref) = preferred_rel {
        if found_rel.iter().any(|p| p == pref) {
            return Some(pref.to_string());
        }
    }
    found_rel
        .iter()
        .min_by(|a, b| {
            let depth = |s: &str| s.matches('/').count();
            depth(a).cmp(&depth(b)).then_with(|| a.cmp(b))
        })
        .cloned()
}

/// Find the runtime `.so` inside package `base` BY NAME and pick one
/// by the [`pick_runtime_so_rel`] rule. The full path or `None`.
///
/// The walk lives here, not in each executor: it is called by the Rust wallpaper
/// tab (`rexon-gui::we_socket`) and the `rexon-py` bridge (`wkp_find_runtime_so`)
/// — the bridge no longer has its own copy of the walk.
/// Symlinks to directories are not followed: a loop in a foreign package would otherwise
/// lead the walk into infinity.
pub fn find_runtime_so(base: &str, preferred_rel: Option<&str>) -> Option<String> {
    let root = std::path::Path::new(base);
    let mut found: Vec<String> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if entry.file_name() == RUNTIME_SO_NAME {
                if let Some(rel) = path.strip_prefix(root).ok().and_then(|r| r.to_str()) {
                    found.push(rel.to_string());
                }
            }
        }
    }
    pick_runtime_so_rel(&found, preferred_rel)
        .map(|rel| root.join(rel).to_string_lossy().into_owned())
}

/// Name of the QML wrapper of the output element inside the package: it lies next to
/// the module directory (`…/Plugin/PluginDisplay.qml` and
/// `…/Plugin/WaywallenDisplayEmbed/`) and imports it by a relative
/// path. The `Waywallen.DisplayEmbed` module lies in a directory with a different name,
/// so it is not imported by the module name — only through this wrapper.
pub const DISPLAY_ITEM_QML_NAME: &str = "PluginDisplay.qml";

/// The output element wrapper for the found runtime library
/// ([`find_runtime_so`]): the directory above the module directory.
pub fn display_item_qml_of(runtime_so: &str) -> Option<String> {
    let module_dir = std::path::Path::new(runtime_so).parent()?;
    let ui_dir = module_dir.parent()?;
    Some(ui_dir.join(DISPLAY_ITEM_QML_NAME).to_string_lossy().into_owned())
}

/// Where to look for the package with the output element for the output host client on X11:
/// first the one bundled in Rexon (`<data>/kpackage/…`, a version tested with
/// our daemon), then the installed Plasma plugin (`~/.local/share/…`).
pub fn display_package_candidates(data_dir: Option<&str>, home: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(dir) = data_dir.filter(|d| !d.is_empty()) {
        out.push(bundle_dir(dir.trim_end_matches('/')));
    }
    if home.starts_with('/') {
        out.push(install_dir(home.trim_end_matches('/')));
    }
    out
}

/// The package's metadata.json — presence is determined by it (bundle/installed).
pub fn metadata_json_path(base: &str) -> String {
    join(&[base, "metadata.json"])
}

// ── environment ─────────────────────────────────────────────────────────

/// Port of `_is_kde` — "kde"/"plasma" in `XDG_CURRENT_DESKTOP` (case-insensitive).
pub fn is_kde(xdg_current_desktop: &str) -> bool {
    let d = xdg_current_desktop.to_lowercase();
    d.contains("kde") || d.contains("plasma")
}

// ── package version (hash of the runtime .so) ───────────────────────────

/// Port of `_file_sha` — sha256 of the file contents, the first 16 hex characters.
/// The executor reads the file (in Python — in 64 KiB chunks, the result is the same).
pub fn file_sha(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(16);
    for b in digest.iter() {
        if hex.len() >= 16 {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(16);
    hex
}

/// Port of `_installed_version`: if the marker is non-empty — its text (after strip),
/// otherwise the hash of the installed `.so` (`installed_so_sha`, `None` if there is no file).
pub fn resolve_installed_version(
    marker_text: Option<&str>,
    installed_so_sha: Option<&str>,
) -> Option<String> {
    if let Some(t) = marker_text {
        let v = t.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    installed_so_sha.map(str::to_string)
}

// ── kpackage install / upgrade ──────────────────────────────────────────

/// Result of one kpackagetool6 run, supplied by the executor.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub rc: i32,
    pub stderr: String,
    pub stdout: String,
}

/// One kpackagetool6 command the executor must run.
#[derive(Debug, Clone, PartialEq)]
pub struct KpCall {
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

/// Result of `_install_or_upgrade` — the plan of the calls actually made + the decision.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallOutcome {
    /// kpackagetool6 runs in execution order (argv + timeout).
    pub calls: Vec<KpCall>,
    pub ok: bool,
    pub message: String,
    /// Whether the package was (re)installed this time.
    pub installed_now: bool,
    /// Version to write into the marker (only on a successful install and when
    /// `bundle_ver` is present); `None` — the marker is not touched.
    pub marker_to_write: Option<String>,
}

fn kp_argv(tool: &str, op: &str, target: &str) -> Vec<String> {
    vec![
        tool.to_string(),
        "--type".to_string(),
        PLUGIN_TYPE.to_string(),
        op.to_string(),
        target.to_string(),
    ]
}

/// Port of `_install_or_upgrade`. A driver: no subprocess runs here —
/// the executor runs the given argv via `run` and returns rc/output.
///
/// `tool` — the kpackagetool path (`None` = not found); `bundle_valid` = the package
/// directory exists and contains metadata.json; `installed` / `installed_ver` /
/// `bundle_ver` — the state computed by the executor from the FS.
///
/// The catch-OSError branch of the Python original's subprocess launch is NOT modelled here
/// (it is about the process itself, not about the logic) — on a launch failure the executor itself
/// forms «kpackagetool6 ошибка запуска: …» (the literal of `rexon-gui/src/we_socket.rs`).
pub fn install_or_upgrade(
    tool: Option<&str>,
    bundle_dir: &str,
    bundle_valid: bool,
    installed: bool,
    installed_ver: Option<&str>,
    bundle_ver: Option<&str>,
    run: &mut dyn FnMut(&[String]) -> RunOutput,
) -> InstallOutcome {
    let calls: Vec<KpCall> = Vec::new();
    let fail = |calls: Vec<KpCall>, msg: String| InstallOutcome {
        calls,
        ok: false,
        message: msg,
        installed_now: false,
        marker_to_write: None,
    };

    let Some(tool) = tool else {
        return fail(calls, crate::i18n::tr("kpackagetool6 was not found — KDE Plasma 6 is required"));
    };
    if !bundle_valid {
        return fail(calls, crate::i18n::trf("the bundled plugin package was not found: %1", &[&bundle_dir]));
    }

    // No-op: the same plugin of the same version is already installed.
    if installed && bundle_ver.is_some() && installed_ver == bundle_ver {
        return InstallOutcome {
            calls,
            ok: true,
            message: "плагин уже установлен (актуальная версия)".to_string(),
            installed_now: false,
            marker_to_write: None,
        };
    }

    let mut calls = calls;
    let op = if installed { "-u" } else { "-i" };
    let mut r = {
        let argv = kp_argv(tool, op, bundle_dir);
        let out = run(&argv);
        calls.push(KpCall { argv, timeout_secs: KPACKAGETOOL_TIMEOUT });
        out
    };

    if r.rc != 0 {
        // -u could have failed (waywallen has no version in metadata) → remove + install.
        if installed {
            let rm = kp_argv(tool, "-r", PLUGIN_ID);
            run(&rm); // the result of -r is ignored (as in Python)
            calls.push(KpCall { argv: rm, timeout_secs: KPACKAGETOOL_TIMEOUT });
            let ins = kp_argv(tool, "-i", bundle_dir);
            r = run(&ins);
            calls.push(KpCall { argv: ins, timeout_secs: KPACKAGETOOL_TIMEOUT });
        }
        if r.rc != 0 {
            // As in Python: op stays the original one here ("-u" on fallback);
            // (stderr if non-empty, otherwise stdout).strip().
            let detail = if r.stderr.is_empty() { &r.stdout } else { &r.stderr };
            return fail(
                calls,
                format!("kpackagetool6 {op} rc={}: {}", r.rc, detail.trim()),
            );
        }
    }

    let verb = if installed { "обновлён" } else { "установлен" };
    InstallOutcome {
        calls,
        ok: true,
        message: format!("плагин {verb}"),
        installed_now: true,
        marker_to_write: bundle_ver.map(str::to_string),
    }
}

// ── auto-selecting the plugin on desktop containments ───────────────────

/// JS for assigning the plugin as the wallpaper of all desktops — port of `select_plugin`.
/// Spaces matter (two spaces before `ds[i]`) — checked by the golden reference;
/// do NOT use a `\` line continuation (it eats leading spaces).
pub fn select_plugin_js() -> String {
    format!(
        "var ds = desktops();for (var i = 0; i < ds.length; i++) {{  ds[i].wallpaperPlugin = '{PLUGIN_ID}';}}"
    )
}

/// JS for reading the wallpaperPlugin of each desktop — port of `current_plugins`.
/// The space before `out.push` matters — checked by the golden reference.
pub fn current_plugins_js() -> String {
    "var ds = desktops(); var out = [];for (var i = 0; i < ds.length; i++) out.push(ds[i].wallpaperPlugin);print(out.join(','));".to_string()
}

/// argv for delivering JS to plasmashell via D-Bus — port of `_plasma_evaluate_script`
/// (`dbus-send`, timeout 15). The argument string is `string:{js}`.
pub fn evaluate_script_argv(js: &str) -> Vec<String> {
    vec![
        "dbus-send".to_string(),
        "--session".to_string(),
        "--print-reply".to_string(),
        "--dest=org.kde.plasmashell".to_string(),
        "--type=method_call".to_string(),
        "/PlasmaShell".to_string(),
        "org.kde.PlasmaShell.evaluateScript".to_string(),
        format!("string:{js}"),
    ]
}

/// argv for reading the current plugins — port of `current_plugins` (`qdbus6`, timeout 10).
pub fn current_plugins_argv() -> Vec<String> {
    vec![
        "qdbus6".to_string(),
        "org.kde.plasmashell".to_string(),
        "/PlasmaShell".to_string(),
        "org.kde.PlasmaShell.evaluateScript".to_string(),
        current_plugins_js(),
    ]
}

/// Parsing the stdout of `current_plugins` — port of `[p for p in
/// out.strip().split(",") if p]` (elements are NOT trimmed individually).
/// Called only when rc==0; when rc!=0 the executor returns an empty list.
pub fn parse_current_plugins(stdout: &str) -> Vec<String> {
    stdout
        .trim()
        .split(',')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Port of `_all_selected` — non-empty AND all equal to PLUGIN_ID.
pub fn all_selected(plugins: &[String]) -> bool {
    !plugins.is_empty() && plugins.iter().all(|p| p == PLUGIN_ID)
}

// ── plasmashell restart (only when the selected plugin is updated) ──────

/// argv of the plasmashell restart — port of `_restart_plasmashell`: reset-failed
/// (timeout 5), then restart (timeout 15). The executor runs them in order.
pub fn restart_plasmashell_cmds() -> Vec<KpCall> {
    vec![
        KpCall {
            argv: vec![
                "systemctl".to_string(),
                "--user".to_string(),
                "reset-failed".to_string(),
                "plasma-plasmashell.service".to_string(),
            ],
            timeout_secs: 5,
        },
        KpCall {
            argv: vec![
                "systemctl".to_string(),
                "--user".to_string(),
                "restart".to_string(),
                "plasma-plasmashell.service".to_string(),
            ],
            timeout_secs: 15,
        },
    ]
}

// ── ensure orchestration (pure decisions) ───────────────────────────────

/// Whether to (re)select the plugin on the desktops — port of the condition in
/// `ensure_waywallen_plugin`: if we just (re)installed it OR it is not yet selected on
/// all desktops (`not _all_selected()`), do select; otherwise no-op.
pub fn should_reselect(installed_now: bool, all_selected_now: bool) -> bool {
    installed_now || !all_selected_now
}

/// The final message of `ensure_waywallen_plugin` — a "; "-joined concatenation of
/// the base message and optional notes (order as in Python).
pub fn ensure_message(base: &str, selected: bool, restarted: bool) -> String {
    let mut parts = vec![base.to_string()];
    if selected {
        parts.push("выбран на десктопах".to_string());
    }
    if restarted {
        parts.push("plasmashell перезапущен".to_string());
    }
    parts.join("; ")
}

// ── when to assign the plugin as wallpaper: only on apply ──────────────
//
// ⛔ Defect of 30.10.2026 (acceptance in a VM, Plasma 6.8.2, new user):
// the FIRST opening of the Wallpaper Engine tab installed the plugin, assigned it as
// the wallpaper of ALL desktops and restarted plasmashell. The library was
// empty — before the first frame the plugin draws black (upstream `main.qml`), and
// a person who merely looked into the tab was left with a black desktop instead of
// their wallpaper (rule #13).
//
// How upstream does it: on KDE the daemon does not start showing by itself
// (`display_registry.rs`: `spawn = External`, "plasmashell loads the
// kpackage"), and the person chooses the plugin in the Plasma settings — waywallen itself
// never touches `wallpaperPlugin`. At the same time the daemon rejects `wallpaper_apply` with zero
// displays (`apply.rs`: `require_display` →
// `NoDisplayRegistered`). So the plugin must be SELECTED by the time of
// the apply — but not earlier.
//
// Hence the order:
//   * opening the tab: the plugin may be installed (kpackagetool6, without root) —
//     this does not affect the desktop; it must not be assigned as wallpaper;
//     plasmashell is restarted only if a plugin that is already set as wallpaper
//     was UPDATED (otherwise the new `.so` will not get into the shell's memory);
//   * applying (`wallpaper_apply`, starting a playlist, moving through it):
//     no displays — the frame is held back, the plugin is assigned, the frame goes out
//     as soon as plasmashell registers a display (or on timeout — then
//     the daemon honestly answers with an error rather than silence).

/// Why the plugin preparation is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsurePurpose {
    /// The tab was opened (or «Install Rexon's Version» was pressed): only
    /// installation, the desktop is not touched.
    Prepare,
    /// The person applied a wallpaper from the library — the plugin is assigned as wallpaper.
    Apply,
}

impl EnsurePurpose {
    /// Whether to assign the plugin as the desktops' wallpaper.
    pub fn selects(self) -> bool {
        self == EnsurePurpose::Apply
    }
}

/// Request fields after which the daemon shows wallpaper on the displays.
///
/// `wallpaper_apply_via_portal` is NOT included: it sets a picture through
/// the desktop portal, not through a waywallen display.
pub const SHOWS_WALLPAPER_FIELDS: &[&str] =
    &["wallpaper_apply", "playlist_activate", "playlist_jump_to"];

/// Whether a request with this field shows wallpaper on the displays.
pub fn request_shows_wallpaper(field: &str) -> bool {
    SHOWS_WALLPAPER_FIELDS.contains(&field)
}

/// Port of the reverse side of `_all_selected`: whether the plugin is set as wallpaper on at least
/// one desktop (that is, its `.so` is already in the shell's memory).
pub fn any_selected(plugins: &[String]) -> bool {
    plugins.iter().any(|p| p == PLUGIN_ID)
}

/// Whether to restart plasmashell after installation.
///
/// Only when a plugin that is currently set as wallpaper was UPDATED: the old `.so`
/// is already loaded, and without a restart the shell keeps drawing with it. A fresh
/// install and an update of a non-selected plugin do not touch the shell — it
/// will load the new package by itself when it is assigned.
pub fn restart_after_install(installed_now: bool, was_present: bool, in_use: bool) -> bool {
    installed_now && was_present && in_use
}

// ── quiet refresh of an already installed plugin ────────────────────────
//
// Defect of 12.11.2026: Rexon updated the bundled engine to waywallen 0.6.0 and
// the bundle plugin to 0.4.5, but the person's `~/.local/share/plasma/…` still
// had the plugin from 06.10. The plugin was refreshed only when applying a wallpaper in
// KDE; the person worked in GNOME/MATE, and on logging into Plasma the old plugin with
// the new daemon hung plasmashell (windows exist, but no panel and no desktop).
// So an already INSTALLED plugin is refreshed both at Rexon startup and after
// an engine update — on any desktop (kpackagetool6 also works outside
// Plasma). One NOT installed is not installed: that is the job of applying a wallpaper in KDE.

/// What to do with the installed plugin at startup and after an engine update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    /// The person has no plugin — we do not install it (applying a wallpaper in KDE installs it).
    NotInstalled,
    /// The runtime matches the bundle (or the version guard considers them the same) — nothing.
    UpToDate,
    /// No kpackagetool6 — skip, write to the log.
    NoTool,
    /// No bundle or it lacks metadata.json — nothing to update with.
    NoBundle,
    /// Update: `kpackagetool6 -u` with the same [`install_or_upgrade`] pipeline.
    Upgrade,
    /// The version guard is against it (the installed one is newer, a foreign one of unknown version,
    /// the bundle does not suit the daemon) — we do not touch it silently.
    Skip(PluginAction),
}

impl RefreshDecision {
    /// Whether running kpackagetool6 is needed.
    pub fn upgrades(self) -> bool {
        self == RefreshDecision::Upgrade
    }
}

/// Decide whether to refresh the installed plugin. A pure function: the FS state
/// (`check`), the presence of the tool and the fitness of the bundle are passed in by the executor.
///
/// The order is cheap: first "is there a plugin" and comparing the runtime hashes —
/// at a normal Rexon startup it goes no further. The "newer /
/// older / foreign" decision belongs to the shared version guard [`plan_plugin`] (without the person's
/// consent: we silently install only what would also be installed on apply).
pub fn refresh_decision(check: &PluginCheck, tool_found: bool, bundle_valid: bool) -> RefreshDecision {
    if !check.installed.present {
        return RefreshDecision::NotInstalled;
    }
    if check.bundle_sha.is_some() && check.installed.runtime_sha == check.bundle_sha {
        return RefreshDecision::UpToDate;
    }
    if !tool_found {
        return RefreshDecision::NoTool;
    }
    if !bundle_valid {
        return RefreshDecision::NoBundle;
    }
    let silent = PluginCheck { consent: false, ..check.clone() };
    match plan_plugin(&silent).action {
        PluginAction::Upgrade | PluginAction::Install => RefreshDecision::Upgrade,
        PluginAction::Keep => RefreshDecision::UpToDate,
        other => RefreshDecision::Skip(other),
    }
}

/// Whether to restart plasmashell after a quiet refresh: only on KDE and
/// only by the shared rule [`restart_after_install`] (a plugin that is currently
/// set as wallpaper was updated). Outside Plasma there is no shell — nothing to touch.
pub fn refresh_restarts(is_kde_now: bool, installed_now: bool, in_use: bool) -> bool {
    is_kde_now && restart_after_install(installed_now, true, in_use)
}

/// How to send a request frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyGate {
    /// Right away, as usual.
    Send,
    /// Hold it back until a display appears, and meanwhile bring up the display
    /// (KDE — assign the plugin, X11 — start the output host client).
    HoldAndSelect,
}

/// The decision for a request frame: whether to hold it back while Rexon brings up
/// the daemon's display.
///
/// Only a request that shows wallpaper is held back, only where Rexon itself
/// brings up the display (`rexon_brings_display`: KDE — by assigning the plugin,
/// X11 — by the output host client, `wallpaper_backend::DisplaySetup`), and
/// only when the daemon has no displays: if there is a display, output already works, and
/// there is no reason to decide for the person where else to show the wallpaper.
pub fn apply_gate(rexon_brings_display: bool, field: &str, displays: usize) -> ApplyGate {
    if rexon_brings_display && displays == 0 && request_shows_wallpaper(field) {
        ApplyGate::HoldAndSelect
    } else {
        ApplyGate::Send
    }
}

/// How long to wait for a display after assigning the plugin, ms. Longer — the frame
/// goes out as is, and the daemon answers with its own "no display" error.
pub const HOLD_TIMEOUT_MS: u64 = 15_000;

// ── «Apply» with zero displays: the target "all desktops" ──────────────
//
// ⛔ Defect of 30.10.2026 (VM, Plasma 6.8.2, fresh user, r1887):
// after assigning the plugin was moved to the moment of applying,
// the vendored wallpaper card kept «Apply» grey — it is enabled by
// `PresentationTargetState.hasSelection`, and that needs at least one
// display. And a display appears only when the plugin is ALREADY assigned:
// a vicious circle, there was no way to apply a wallpaper on a fresh system. Meanwhile the
// «Displays» page sent the person to install the extension by hand from GitHub/the KDE
// Store and select it in the Plasma settings (rule #13).
//
// The way out: while there are no displays but the apply frame will be held back and the plugin
// assigned (`apply_gate` → `HoldAndSelect`), the button has a target — "all
// desktops" (an empty target list in the request, like the vendored "all").
// The decision is taken from the same `apply_gate`, so that the button does not diverge from
// what the transport will do with the frame.

/// Whether «Apply» can be pressed while the daemon has not a single display.
///
/// Yes exactly when the transport will hold back such a frame and bring up the
/// display itself: then the wallpaper is applied to all desktops.
pub fn apply_without_displays(rexon_brings_display: bool) -> bool {
    apply_gate(rexon_brings_display, "wallpaper_apply", 0) == ApplyGate::HoldAndSelect
}

/// Whether a playlist can be started while the daemon has not a single display.
///
/// ⛔ The same circle as with «Apply» (defect of 30.10.2026, after 66a2dd5b):
/// the vendored playlist list selects targets one by one and without a display did not
/// enable the start button, and with an empty target list the "already
/// playing" check (`every()` over an empty set) answered "yes" and turned
/// the start into a pause. The solution is the same `apply_gate` for `playlist_activate`:
/// the frame is held back, the plugin is assigned, the target is all desktops with
/// new displays attached (`auto_attach`), like the vendored "all".
pub fn play_without_displays(rexon_brings_display: bool) -> bool {
    apply_gate(rexon_brings_display, "playlist_activate", 0) == ApplyGate::HoldAndSelect
}

/// Hint of the «Displays» page with zero displays instead of the vendored
/// "install the extension by hand" (KDE) or a bare «No displays
/// registered» (X11: MATE, XFCE, Cinnamon X11 — defect of 30.10.2026).
/// An empty string — no hint (the display is not brought up by Rexon), the page
/// shows its own. Names both paths that work without displays:
/// «Apply» on a wallpaper and «Play Playlist» ([`play_without_displays`]).
pub fn no_displays_hint(setup: crate::wallpaper_backend::DisplaySetup) -> String {
    use crate::wallpaper_backend::DisplaySetup;
    if !apply_without_displays(setup.rexon_brings_display()) {
        return String::new();
    }
    match setup {
        DisplaySetup::KdePlugin => crate::i18n::tr(
            "Wallpaper Engine wallpapers are not on the desktop yet. Pick a wallpaper or a playlist \
             and press “Apply” or “Play playlist” — Rexon will put them on the Plasma desktop by \
             itself.",
        ),
        // GNOME: Rexon enables the waywallen extension itself — the same words.
        DisplaySetup::RexonHost | DisplaySetup::GnomeExtension => crate::i18n::tr(
            "Wallpaper Engine wallpapers are not on the desktop yet. Pick a wallpaper or a playlist \
             and press “Apply” or “Play playlist” — Rexon will put them on the desktop by itself.",
        ),
        DisplaySetup::None => String::new(),
    }
}

/// Hint of the «Displays» page taking the session backend into account: where Rexon
/// brings up the display — [`no_displays_hint`]; where there is no output for scenes at all and
/// «Apply» is therefore grey — an explanation why, not a bare «No displays
/// registered» (rule #13). Where the engine itself brings up the display
/// (wlr compositors) — empty: the page shows its own.
pub fn no_displays_hint_in(
    setup: crate::wallpaper_backend::DisplaySetup,
    backend: crate::wallpaper_backend::WallpaperBackend,
) -> String {
    use crate::wallpaper_backend::{DisplaySetup, WallpaperBackend};
    if setup != DisplaySetup::None {
        return no_displays_hint(setup);
    }
    match backend {
        WallpaperBackend::ShellExtension => crate::i18n::tr(
            "Wallpaper Engine wallpapers can't be shown on this desktop: the shell doesn't let \
             programs draw on the wallpaper layer, so there is no monitor to show them on and \
             “Apply” stays unavailable. Video, web and Flash wallpapers from Rexon still work.",
        ),
        WallpaperBackend::Unsupported => crate::i18n::tr(
            "Wallpaper Engine wallpapers can't be shown in this session: its window manager \
             doesn't support live wallpapers, so “Apply” stays unavailable.",
        ),
        WallpaperBackend::KdePlugin | WallpaperBackend::DaemonLayerShell | WallpaperBackend::X11Subprocess => String::new(),
    }
}

/// Whether to release the held-back frames.
///
/// `select_done` — the preparation has finished; `select_ok` — the plugin is assigned.
/// Assigning failed — there is nothing to wait for, the daemon's answer will tell the person the truth.
pub fn release_held(select_done: bool, select_ok: bool, displays: usize, waited_ms: u64) -> bool {
    displays > 0 || (select_done && !select_ok) || waited_ms >= HOLD_TIMEOUT_MS
}

/// Update the daemon's set of displays from its event.
///
/// Returns `true` if the set changed. Other events do not touch the set.
pub fn track_displays(
    ids: &mut std::collections::BTreeSet<u64>,
    event: &crate::proto::control::Event,
) -> bool {
    use crate::proto::control::event::Payload;
    match event.payload.as_ref() {
        Some(Payload::DisplaySnapshot(snap)) => {
            let next: std::collections::BTreeSet<u64> =
                snap.displays.iter().map(|d| d.display_id).collect();
            let changed = *ids != next;
            *ids = next;
            changed
        }
        Some(Payload::DisplayChanged(changed)) => {
            changed.display.as_ref().is_some_and(|d| ids.insert(d.display_id))
        }
        Some(Payload::DisplayRemoved(removed)) => ids.remove(&removed.display_id),
        _ => false,
    }
}

/// What to write to the log about assigning the plugin at the moment the
/// held-back frames are released. The former line "plugin assigned: false" was
/// misleading: frames are most often released when a display appears, while
/// the assigning thread has not yet reported, although the plugin is already assigned.
pub fn select_state_for_log(select_done: bool, select_ok: bool) -> &'static str {
    match (select_done, select_ok) {
        // Shared words for both ways of bringing up output: on KDE it is assigning the
        // plugin, on X11 — starting the output host client.
        (false, _) => "подъём вывода ещё не отчитался",
        (true, true) => "вывод поднят только что",
        (true, false) => "не поднимался (уже был) или поднять не вышло",
    }
}

// ── rollback: the live wallpaper did not start — bring back the desktop's previous wallpaper ─
//
// ⛔ A risk found during the acceptance of 66a2dd5b (VM, llvmpipe): the plugin was assigned
// for the sake of an apply, but the waywallen renderer did not start
// (`ww_bridge_pool_create failed: -22`) — the person is left with a BLACK desktop
// instead of their wallpaper (rule #13). The same will happen on any machine where
// the renderer does not come up (no Vulkan with DMA-BUF, an old graphics card).
//
// Rule: if the held-back apply failed — the daemon answered with an error
// or the renderer of this apply vanished in the first seconds — and the plugin
// was assigned PRECISELY for it, the desktops get back the plugin that stood before
// the assignment (usually `org.kde.image`; Plasma keeps its settings, so
// the picture itself comes back too). If the plugin stood there before «Apply» — we do not touch it.

/// The wallpaper plugin of one Plasma desktop (a `desktops()` container).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPlugin {
    /// Container `id` — by it the rollback finds the same desktop.
    pub desktop_id: i64,
    pub plugin: String,
}

/// JS: the wallpaper plugin of each desktop together with the container number —
/// `id=plugin` separated by `;`. Taken before the assignment, so that there is something to
/// bring back.
pub fn plugins_by_desktop_js() -> String {
    "var ds = desktops(); var res = '';for (var i = 0; i < ds.length; i++) \
     res += (i ? ';' : '') + ds[i].id + '=' + ds[i].wallpaperPlugin;print(res);"
        .to_string()
}

/// Parsing the answer of `plugins_by_desktop_js`. Garbage is skipped.
pub fn parse_plugins_by_desktop(stdout: &str) -> Vec<DesktopPlugin> {
    stdout
        .trim()
        .split(';')
        .filter_map(|item| {
            let (id, plugin) = item.trim().split_once('=')?;
            let desktop_id = id.trim().parse().ok()?;
            let plugin = plugin.trim();
            (!plugin.is_empty()).then(|| DesktopPlugin { desktop_id, plugin: plugin.to_string() })
        })
        .collect()
}

/// A plugin name that is safe to insert into JS (KPackage id: Latin letters,
/// digits, `.`, `_`, `-`). Anything else does not get into the rollback.
fn plugin_id_is_safe(plugin: &str) -> bool {
    !plugin.is_empty()
        && plugin.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Rollback JS: for desktops where our plugin currently stands, bring back the previous one.
///
/// `None` — nothing to roll back: the plugin was not assigned by us (`selected_now`
/// is false), there is no snapshot of the previous one, or all desktops already stood on our
/// plugin before. Desktops that appeared after the snapshot, and desktops where the person has already
/// chosen something else, are not touched.
pub fn revert_plugins_js(selected_now: bool, previous: Option<&[DesktopPlugin]>) -> Option<String> {
    if !selected_now {
        return None;
    }
    let entries: Vec<String> = previous?
        .iter()
        .filter(|d| d.plugin != PLUGIN_ID && plugin_id_is_safe(&d.plugin))
        .map(|d| format!("'{}': '{}'", d.desktop_id, d.plugin))
        .collect();
    if entries.is_empty() {
        return None;
    }
    Some(format!(
        "var prev = {{{}}}; var ds = desktops();for (var i = 0; i < ds.length; i++) {{  \
         var p = prev[String(ds[i].id)];  if (p && ds[i].wallpaperPlugin == '{PLUGIN_ID}') \
         ds[i].wallpaperPlugin = p;}}",
        entries.join(", ")
    ))
}

/// How long after sending the held-back apply to watch whether
/// its renderer vanished (crashed at startup, on the first frame), ms.
pub const FIRST_FRAME_WATCH_MS: u64 = 10_000;

/// What to do on the next event of the apply guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchVerdict {
    /// Keep watching.
    Keep,
    /// The apply happened (or there is no longer any point in watching it).
    Done,
    /// The apply failed — bring back the previous wallpaper.
    Revert,
}

/// Guard of a held-back apply: watches the daemon's answer to the LAST
/// released frame (it is what determines what ends up on the desktop) and its
/// renderer during the first `FIRST_FRAME_WATCH_MS`.
///
/// ⛔ A playlist (`playlist_activate`) and a deferred apply do not name the renderer in
/// the answer — previously the guard gave up on such an answer, and a renderer
/// crash after a successful answer (VM, llvmpipe:
/// `ww_bridge_pool_create failed: -22`) left the desktop black. Now the
/// renderer is recognised by the daemon's events: which renderers are shown on
/// the displays (`DisplayInfo.links`) since the frame was sent. Before the hold there were
/// no displays at all, so everything that appeared on them afterwards comes from
/// this apply. The daemon, removing a crashed renderer, sends
/// `RendererRemoved` before the updated display list
/// (`on_renderer_process_exit`), but the order does not matter: the guard remembers everyone
/// who was ever shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyWatch {
    request_id: u64,
    answered: bool,
    /// The renderer from the `wallpaper_apply` answer — we watch exactly it.
    renderer_id: Option<String>,
    /// Current "display → renderers" links from the daemon's events.
    links: std::collections::BTreeMap<u64, std::collections::BTreeSet<String>>,
    /// Renderers ever shown on the displays since the frame was sent.
    shown: std::collections::BTreeSet<String>,
    /// Renderers the daemon reported (`RendererChanged`) since sending:
    /// live candidates, for example the next renderer of a playlist.
    announced: std::collections::BTreeSet<String>,
    /// Shown renderers that the daemon removed.
    lost: std::collections::BTreeSet<String>,
}

impl ApplyWatch {
    pub fn new(request_id: u64) -> ApplyWatch {
        ApplyWatch {
            request_id,
            answered: false,
            renderer_id: None,
            links: Default::default(),
            shown: Default::default(),
            announced: Default::default(),
            lost: Default::default(),
        }
    }

    /// Whether this is an answer to the watched frame.
    pub fn watches(&self, request_id: u64) -> bool {
        request_id != 0 && request_id == self.request_id
    }

    /// The daemon's answer. `ok` — status `OK`; `renderer_id` — from the
    /// `wallpaper_apply` answer (empty for playlists and a deferred apply —
    /// then the renderer is recognised by display events, `on_event`).
    pub fn on_response(&mut self, request_id: u64, ok: bool, renderer_id: &str) -> WatchVerdict {
        if !self.watches(request_id) {
            return WatchVerdict::Keep;
        }
        if !ok {
            return WatchVerdict::Revert;
        }
        self.answered = true;
        if !renderer_id.is_empty() {
            self.renderer_id = Some(renderer_id.to_string());
            return WatchVerdict::Keep;
        }
        // The renderer could have crashed even before the answer — the events have already arrived.
        self.verdict_by_displays()
    }

    /// A daemon event: display links, renderers appearing and being removed.
    pub fn on_event(&mut self, event: &crate::proto::control::Event) -> WatchVerdict {
        use crate::proto::control::event::Payload;
        match event.payload.as_ref() {
            Some(Payload::DisplaySnapshot(snap)) => {
                self.links.clear();
                for display in &snap.displays {
                    self.set_links(display);
                }
                WatchVerdict::Keep
            }
            Some(Payload::DisplayChanged(changed)) => {
                if let Some(display) = &changed.display {
                    self.set_links(display);
                }
                WatchVerdict::Keep
            }
            Some(Payload::DisplayRemoved(removed)) => {
                self.links.remove(&removed.display_id);
                WatchVerdict::Keep
            }
            Some(Payload::RendererChanged(changed)) => {
                if let Some(renderer) = &changed.renderer {
                    self.lost.remove(&renderer.renderer_id);
                    self.announced.insert(renderer.renderer_id.clone());
                }
                WatchVerdict::Keep
            }
            Some(Payload::RendererRemoved(removed)) => self.on_renderer_removed(&removed.renderer_id),
            _ => WatchVerdict::Keep,
        }
    }

    fn set_links(&mut self, display: &crate::proto::control::DisplayInfo) {
        let ids: std::collections::BTreeSet<String> = display
            .links
            .iter()
            .map(|l| l.renderer_id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        self.shown.extend(ids.iter().cloned());
        self.links.insert(display.display_id, ids);
    }

    /// The daemon removed a renderer. If it is ours, it crashed without starting to show.
    pub fn on_renderer_removed(&mut self, renderer_id: &str) -> WatchVerdict {
        if let Some(ours) = &self.renderer_id {
            return if ours == renderer_id { WatchVerdict::Revert } else { WatchVerdict::Keep };
        }
        self.announced.remove(renderer_id);
        if !self.shown.contains(renderer_id) {
            // It was not shown on the displays after sending — not ours.
            return WatchVerdict::Keep;
        }
        self.lost.insert(renderer_id.to_string());
        if self.answered {
            self.verdict_by_displays()
        } else {
            // No answer yet: it will report an error, and success — taking the loss into account.
            WatchVerdict::Keep
        }
    }

    /// The renderer is not named: the apply failed if the shown renderer
    /// was removed and no live one came to replace it (a wallpaper change by a playlist
    /// removes the previous one, but the new one has already been announced by then).
    fn verdict_by_displays(&self) -> WatchVerdict {
        if self.lost.is_empty() {
            return WatchVerdict::Keep;
        }
        let alive = self
            .links
            .values()
            .flatten()
            .chain(self.announced.iter())
            .any(|id| !self.lost.contains(id));
        if alive {
            WatchVerdict::Keep
        } else {
            WatchVerdict::Revert
        }
    }

    /// The person applied something else themselves — their new action takes precedence.
    pub fn on_new_apply(&self) -> WatchVerdict {
        WatchVerdict::Done
    }

    /// Clock tick: `elapsed_ms` — since the frame was sent.
    pub fn on_tick(&self, elapsed_ms: u64) -> WatchVerdict {
        let limit = if self.answered { FIRST_FRAME_WATCH_MS } else { HOLD_TIMEOUT_MS + FIRST_FRAME_WATCH_MS };
        if elapsed_ms >= limit {
            WatchVerdict::Done
        } else {
            WatchVerdict::Keep
        }
    }
}

/// Message to the person: the live wallpaper did not start, the previous wallpaper was brought back.
pub fn revert_done_text() -> String {
    crate::i18n::tr(
        "The live wallpaper could not start on this computer, so Rexon put your previous \
         desktop wallpaper back.",
    )
}

/// Message to the person: the live wallpaper did not start, and bringing back the previous one
/// failed — what to do themselves.
pub fn revert_failed_text() -> String {
    crate::i18n::tr(
        "The live wallpaper could not start, and Rexon could not put your previous wallpaper \
         back. Right-click the desktop, choose “Configure Desktop and Wallpaper…” and pick \
         your wallpaper again.",
    )
}

// ── version guard: do not install old over new ─────────────────────────
//
// ⛔ Defect of 02.10.2026 (live at Wehrwolfmann's). The former rule was "version
// = hash of the `.so`, on ANY mismatch with the bundle — `kpackagetool6 -u`".
// It did not know direction: Rexon r1062 with the bundled plugin 0.3.3
// (display protocol v8), when the wallpaper tab was opened, reinstalled it ON TOP of
// the newer 0.4.0, while the waywallen 0.4.3 daemon accepts only v9 —
// "client protocol v8 not supported; daemon accepts [9..=9]", no monitors,
// the wallpaper vanished. Now REAL versions are compared, old is never installed
// over new, a foreign plugin of unknown version is not touched without the person's
// consent, and incompatibility with the daemon is stated out loud.
//
// Where the versions come from:
//   * for the plugin — `vendor.toml` (`version`), it travels INSIDE our package and
//     kpackagetool6 copies it into the installed directory: if it is present, we installed
//     the plugin, and the version is exact;
//   * a foreign installed one (an upstream release) does not carry `vendor.toml`,
//     the upstream `metadata.json` has no `Version`, and there is no version string in the `.so`
//     (`waywallen_display_version()` is a function, not text: it can be learned
//     only by loading the library into the process, which we do not do). So for
//     a foreign one — the table of known runtime hashes [`KNOWN_RUNTIME_SHA`];
//   * the `.rexon-installed-version` marker — a fallback sign of "we installed it"
//     (installs were marked this way before `vendor.toml` travelled in the package).
//
// Display protocol and the daemon. The daemon can be asked about the protocol only through
// the display socket (`display.sock`, the `hello` request): on refusal it sends all
// control clients a `DisplayConnectionFailed` event, and a warning pops up for the person
// in the waywallen window. That is interfering with a live daemon, and
// so the protocol is taken by the daemon VERSION (the `Version` property on the bus, or
// the binary's `--version`) from the [`DAEMON_DISPLAY_PROTOCOLS`] table, taken from the
// upstream tags. A version outside the table — no verdict, the "not older" rule remains.

/// Name of the file describing the vendored piece (inside the plugin package).
pub const VENDOR_TOML_NAME: &str = "vendor.toml";

/// `vendor.toml` of package `base` (the bundle or an installed instance).
pub fn vendor_toml_path(base: &str) -> String {
    join(&[base, VENDOR_TOML_NAME])
}

/// Version from the `vendor.toml` text (the `version` field); no field, empty or the file
/// does not parse — `None`.
pub fn parse_vendor_version(text: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let v = table.get("version")?.as_str()?.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Known runtimes of `libwaywallen-display-qml.so` → plugin version (by
/// [`file_sha`]). Needed for a FOREIGN install that has no `vendor.toml`.
///
/// ⚠️ The hash changes not only with the version: Rexon packages up to and including r1278
/// stripped debug symbols from the `.so`, and one installed from them has a different hash
/// than the release. Since 09.10.2026 the plasmoid travels from the release archive bit for bit
/// (PKGBUILD excludes it from `_strip_own_binaries`), but the stripped forms
/// remain for those who installed the earlier packages. An unknown hash means the version is
/// unknown, not "old".
pub const KNOWN_RUNTIME_SHA: &[(&str, &str)] = &[
    // 0.4.5 — the upstream release `waywallen-kde-0.4.5-x86_64.zip` (travels in the package
    // since 08.11.2026; from this release on the archive has no "-embed" suffix).
    ("4d90629e44055113", "0.4.5"),
    // 0.4.3 — the upstream release `waywallen-kde-0.4.3-x86_64-embed.zip` (travels in the
    // package since 23.10.2026; computed from the release archive).
    ("5981949590228ee7", "0.4.3"),
    // 0.4.2 — the upstream release `waywallen-kde-0.4.2-x86_64-embed.zip` (since
    // 19.10.2026 it is exactly what travels in the package; the same build is installed by those who
    // installed the release themselves). There is no stripped form: since 09.10.2026 the package does not
    // strip the plasmoid.
    ("19c1613463252bae", "0.4.2"),
    // 0.4.1 — the upstream release `waywallen-kde-0.4.1-x86_64-embed.zip` (travelled in the
    // package from 09.10 to 19.10.2026; the same build is installed by those who installed the
    // release themselves).
    ("684c8ff280549b10", "0.4.1"),
    // 0.4.1 — the same runtime after `strip --strip-unneeded` (computed with the same
    // strip; in case of a package built before 09.10.2026).
    ("b70e45b8c5d7dbe3", "0.4.1"),
    // 0.4.0 — the upstream release `waywallen-kde-0.4.0-x86_64-embed.zip` (the tree).
    ("bc9e0fbd268e448a", "0.4.0"),
    // 0.4.0 — the same runtime after strip in the Rexon r1278 package.
    ("cbfc755c8c3ad9fd", "0.4.0"),
    // 0.3.3 — the upstream release (the tree before 26.09.2026).
    ("e9c240a1da4c9327", "0.3.3"),
    // 0.3.3 — the same runtime after strip in the Rexon r1062 package.
    ("da1b8e3007238009", "0.3.3"),
];

/// Plugin version by the hash of its runtime (see [`KNOWN_RUNTIME_SHA`]).
pub fn version_from_runtime_sha(sha: &str) -> Option<&'static str> {
    KNOWN_RUNTIME_SHA.iter().find(|(s, _)| *s == sha).map(|(_, v)| *v)
}

/// The display protocol spoken by a plugin of a given version: lines "from this
/// version — this protocol", in descending order. Taken from the upstream tags of
/// `waywallen-display` (`WAYWALLEN_DISPLAY_PROTOCOL_VERSION`, 02.10.2026;
/// v0.4.1 checked 09.10.2026 — the same v9, only optional
/// "tagged" extensions added, the line "from 0.4.0 — v9" covers it; v0.4.2 checked
/// 19.10.2026 — the same constant, the wire did not change, changes only inside the
/// library: EGL frame synchronisation, #76; v0.4.3 checked 23.10.2026 —
/// the same constant, only an optional "tagged" display role field
/// added; v0.4.4 and v0.4.5 checked 08.11.2026 — the same constant, v9).
pub const PLUGIN_DISPLAY_PROTOCOLS: &[(&str, u32)] = &[
    ("0.4.0", 9),
    ("0.3.0", 8),
    ("0.2.1", 7),
    ("0.2.0", 6),
    ("0.1.3", 5),
    ("0.1.2", 4),
    ("0.1.1", 3),
];

/// Up to which plugin version the table above is verified. For newer ones we do not
/// guess the protocol (a new release could have changed it).
pub const PLUGIN_PROTOCOLS_CHECKED_UP_TO: &str = "0.4.5";

/// Which display protocols a waywallen daemon of a given version accepts
/// (`MIN/MAX_SUPPORTED_CLIENT_VERSION`), in descending version order. Taken from the
/// upstream `waywallen` tags on 02.10.2026, v0.4.4 checked 09.10.2026, v0.5.0 —
/// 23.10.2026, v0.6.0 — 08.11.2026 (still `[9..=9]`); v0.6.1 checked 2026-11-23,
/// still `[9..=9]`. Below 0.3.0 nothing is recorded (the range is wider there and of no
/// interest to us) — no verdict.
pub const DAEMON_DISPLAY_PROTOCOLS: &[(&str, u32, u32)] = &[("0.4.0", 9, 9), ("0.3.0", 8, 8)];

/// Up to which daemon version the table above is verified.
pub const DAEMON_PROTOCOLS_CHECKED_UP_TO: &str = "0.6.1";

fn version_key(v: &str) -> Vec<u128> {
    let mut t = crate::engine_update::ver_tuple(v);
    while t.last() == Some(&0) {
        t.pop();
    }
    t
}

/// Compare versions numerically (`0.4.0` = `v0.4`). Empty — `None`.
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let (ka, kb) = (version_key(a), version_key(b));
    if ka.is_empty() || kb.is_empty() {
        return None;
    }
    Some(ka.cmp(&kb))
}

fn within_checked(version: &str, checked_up_to: &str) -> bool {
    compare_versions(version, checked_up_to).is_some_and(|o| o != std::cmp::Ordering::Greater)
}

/// Display protocol of a plugin of a given version; outside the verified table — `None`.
pub fn plugin_protocol(version: &str) -> Option<u32> {
    if !within_checked(version, PLUGIN_PROTOCOLS_CHECKED_UP_TO) {
        return None;
    }
    PLUGIN_DISPLAY_PROTOCOLS
        .iter()
        .find(|(from, _)| compare_versions(version, from).is_some_and(|o| o.is_ge()))
        .map(|(_, p)| *p)
}

/// The range of protocols a daemon of a given version accepts; outside the
/// verified table — `None`.
pub fn daemon_protocols(version: &str) -> Option<(u32, u32)> {
    if !within_checked(version, DAEMON_PROTOCOLS_CHECKED_UP_TO) {
        return None;
    }
    DAEMON_DISPLAY_PROTOCOLS
        .iter()
        .find(|(from, _, _)| compare_versions(version, from).is_some_and(|o| o.is_ge()))
        .map(|(_, lo, hi)| (*lo, *hi))
}

/// Whether plugin version `plugin` suits daemon version `daemon`: `None` —
/// we do not know (one of the versions is missing or outside the tables).
pub fn compatible_with_daemon(plugin: Option<&str>, daemon: Option<&str>) -> Option<bool> {
    let p = plugin_protocol(plugin?)?;
    let (lo, hi) = daemon_protocols(daemon?)?;
    Some((lo..=hi).contains(&p))
}

/// What is visible about the installed instance (the executor reads everything).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstalledPlugin {
    /// `~/.local/share/plasma/wallpapers/` has a `metadata.json`.
    pub present: bool,
    /// `version` from its `vendor.toml` (only one installed by us has it).
    pub vendor_version: Option<String>,
    /// A non-empty `.rexon-installed-version` marker — we installed it.
    pub marker: bool,
    /// [`file_sha`] of its runtime.
    pub runtime_sha: Option<String>,
}

/// Input of the "whether to install the plugin" decision.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginCheck {
    /// `version` from the `vendor.toml` of our bundle.
    pub bundle_version: Option<String>,
    /// [`file_sha`] of the bundle runtime.
    pub bundle_sha: Option<String>,
    pub installed: InstalledPlugin,
    /// Version of the waywallen daemon (bus or `--version`); `None` — cannot be learned.
    pub daemon_version: Option<String>,
    /// The person pressed «Install Rexon's Version» themselves.
    pub consent: bool,
}

/// What to do with the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginAction {
    /// No plugin — `kpackagetool6 -i`.
    Install,
    /// The installed one is older (or ours without a version, or consent) — `-u`.
    Upgrade,
    /// The same one — do nothing.
    Keep,
    /// The installed one is NEWER than the bundle — do not touch.
    KeepNewer,
    /// A foreign one, the version cannot be determined — do not touch, ask the person.
    AskForeign,
    /// The bundle does not suit the daemon — do not install (do not break what works).
    Blocked,
}

impl PluginAction {
    /// Whether running kpackagetool6 is needed.
    pub fn installs(self) -> bool {
        matches!(self, PluginAction::Install | PluginAction::Upgrade)
    }

    /// Name for the log and checks.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginAction::Install => "install",
            PluginAction::Upgrade => "upgrade",
            PluginAction::Keep => "keep",
            PluginAction::KeepNewer => "keep_newer",
            PluginAction::AskForeign => "ask_foreign",
            PluginAction::Blocked => "blocked",
        }
    }
}

/// Message to the person about the wallpaper plugin (the decision is here, showing it is the tab's job).
#[derive(Debug, Clone, PartialEq)]
pub enum PluginNotice {
    /// The plugin is not from Rexon, the version is unknown — offer ours.
    ForeignUnknown,
    /// The installed (kept) plugin does not suit the daemon.
    InstalledIncompatible {
        plugin_version: String,
        daemon_version: String,
        /// Our bundle suits the daemon — we can offer to install it.
        offer_install: bool,
        /// The plugin's protocol is older than what the daemon accepts.
        plugin_older: bool,
    },
    /// Our bundle does not suit the daemon, and there is no working plugin.
    BundleIncompatible {
        bundle_version: String,
        daemon_version: String,
        /// The bundle is older than the daemon — update Rexon; otherwise — update waywallen.
        bundle_older: bool,
    },
}

impl PluginNotice {
    /// Whether to offer the «Install Rexon's Version» button.
    pub fn offers_install(&self) -> bool {
        match self {
            PluginNotice::ForeignUnknown => true,
            PluginNotice::InstalledIncompatible { offer_install, .. } => *offer_install,
            PluginNotice::BundleIncompatible { .. } => false,
        }
    }

    /// A sign for the log and checks.
    pub fn kind(&self) -> &'static str {
        match self {
            PluginNotice::ForeignUnknown => "foreign_unknown",
            PluginNotice::InstalledIncompatible { .. } => "installed_incompatible",
            PluginNotice::BundleIncompatible { .. } => "bundle_incompatible",
        }
    }

    /// Text for the person (through the translation catalog).
    pub fn text(&self) -> String {
        use crate::i18n::{tr, trf};
        match self {
            PluginNotice::ForeignUnknown => tr(
                "The Plasma wallpaper plugin for waywallen was installed by someone \
                 other than Rexon, and its version could not be determined. Rexon left \
                 it as is. If live wallpapers do not appear, install the version that \
                 comes with Rexon.",
            ),
            PluginNotice::InstalledIncompatible {
                plugin_version,
                daemon_version,
                offer_install,
                plugin_older,
            } => {
                let head = trf(
                    "The installed Plasma wallpaper plugin (version %1) does not work \
                     with the waywallen engine %2, so live wallpapers are not shown.",
                    &[plugin_version, daemon_version],
                );
                let tail = if *offer_install {
                    tr("Rexon can install a version that fits.")
                } else if *plugin_older {
                    tr("Update Rexon to get a newer plugin.")
                } else {
                    tr("Update waywallen.")
                };
                format!("{head} {tail}")
            }
            PluginNotice::BundleIncompatible { bundle_version, daemon_version, bundle_older } => {
                if *bundle_older {
                    trf(
                        "The waywallen engine %1 is newer than the Plasma wallpaper \
                         plugin in this version of Rexon (%2). Update Rexon, otherwise \
                         live wallpapers will not appear on KDE.",
                        &[daemon_version, bundle_version],
                    )
                } else {
                    trf(
                        "The Plasma wallpaper plugin in Rexon (%2) needs a newer waywallen \
                         engine than the installed one (%1). Update waywallen.",
                        &[daemon_version, bundle_version],
                    )
                }
            }
        }
    }
}

/// Label of the consent button.
pub fn install_rexon_version_label() -> String {
    crate::i18n::tr("Install Rexon's Version")
}

/// Installing by consent failed; `reason` — the raw answer of kpackagetool6.
pub fn install_failed_text(reason: &str) -> String {
    crate::i18n::trf("Could not install the Plasma wallpaper plugin: %1", &[&reason])
}

/// The decision about the plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginPlan {
    pub action: PluginAction,
    /// The recognised version of the installed one (`vendor.toml` or the hash).
    pub installed_version: Option<String>,
    pub notice: Option<PluginNotice>,
    /// Whether to assign the plugin as the desktops' wallpaper. No, if the kept plugin
    /// certainly does not suit the daemon (otherwise a black screen instead of the person's
    /// wallpaper) or there is nothing to install.
    pub select: bool,
    /// Why so — for the log (not for the person, not translated).
    pub reason: String,
}

/// Decide what to do with the plugin. A pure function: all the state is passed in.
///
/// Rules:
///   * one installed that is NEWER than the bundle is never touched (even with consent —
///     in that case the button is offered only for compatibility with the daemon);
///   * the same runtime — do nothing; the same version of ours with different
///     contents — refresh (our patches on top of upstream could have changed);
///   * a foreign one of unknown version — only with consent;
///   * a bundle the daemon certainly will not accept is not installed: we do not break a working
///     plugin, and if there is none — we name what to update.
pub fn plan_plugin(c: &PluginCheck) -> PluginPlan {
    use std::cmp::Ordering;

    let inst = &c.installed;
    let installed_version = inst.vendor_version.clone().or_else(|| {
        inst.runtime_sha.as_deref().and_then(version_from_runtime_sha).map(str::to_string)
    });
    let ours = inst.vendor_version.is_some() || inst.marker;
    let daemon = c.daemon_version.as_deref();
    let bundle_ok = compatible_with_daemon(c.bundle_version.as_deref(), daemon);
    let installed_ok = compatible_with_daemon(installed_version.as_deref(), daemon);

    let plan = |action: PluginAction, notice, reason: &str| PluginPlan {
        action,
        installed_version: installed_version.clone(),
        notice,
        select: match action {
            PluginAction::Install | PluginAction::Upgrade => true,
            PluginAction::Blocked => inst.present && installed_ok == Some(true),
            _ => installed_ok != Some(false),
        },
        reason: reason.to_string(),
    };
    // The bundle does not suit the daemon: if there is nothing better to install — explain.
    let blocked = |reason: &str| {
        let notice = (installed_ok != Some(true)).then(|| PluginNotice::BundleIncompatible {
            bundle_version: c.bundle_version.clone().unwrap_or_default(),
            daemon_version: c.daemon_version.clone().unwrap_or_default(),
            bundle_older: bundle_protocol_older(c),
        });
        plan(PluginAction::Blocked, notice, reason)
    };
    // Keep the installed one: if it does not suit the daemon — say so.
    let keep = |action, reason: &str| {
        let notice = (installed_ok == Some(false)).then(|| {
            let iv = installed_version.clone().unwrap_or_default();
            let older = plugin_protocol(&iv)
                .zip(daemon.and_then(daemon_protocols))
                .is_some_and(|(p, (lo, _))| p < lo);
            PluginNotice::InstalledIncompatible {
                plugin_version: iv,
                daemon_version: c.daemon_version.clone().unwrap_or_default(),
                offer_install: bundle_ok == Some(true),
                plugin_older: older,
            }
        });
        plan(action, notice, reason)
    };
    let upgrade = |reason: &str| {
        if bundle_ok == Some(false) {
            blocked("бандл не подходит к демону — не ставим")
        } else {
            plan(PluginAction::Upgrade, None, reason)
        }
    };

    if !inst.present {
        if bundle_ok == Some(false) {
            return blocked("плагина нет, а бандл не подходит к демону");
        }
        return plan(PluginAction::Install, None, "плагина нет — ставим");
    }

    let same_runtime = c.bundle_sha.is_some() && inst.runtime_sha == c.bundle_sha;
    let cmp = installed_version
        .as_deref()
        .zip(c.bundle_version.as_deref())
        .and_then(|(i, b)| compare_versions(i, b));

    match cmp {
        Some(Ordering::Greater) => {
            if c.consent && installed_ok == Some(false) && bundle_ok == Some(true) {
                return plan(
                    PluginAction::Upgrade,
                    None,
                    "по согласию: установленный новее, но не подходит к демону",
                );
            }
            keep(PluginAction::KeepNewer, "установленный новее бандла — не трогаем")
        }
        Some(Ordering::Equal) if same_runtime || !ours => {
            keep(PluginAction::Keep, "та же версия — ничего не делаем")
        }
        Some(Ordering::Equal) => upgrade("та же версия, другое содержимое (наши правки) — освежаем"),
        Some(Ordering::Less) => upgrade("установленный старее — обновляем"),
        None if same_runtime => keep(PluginAction::Keep, "тот же рантайм — ничего не делаем"),
        None if c.bundle_version.is_none() => {
            keep(PluginAction::Keep, "версия бандла неизвестна — не трогаем")
        }
        None if c.consent => upgrade("по согласию человека — ставим версию Rexon"),
        None if ours => upgrade("наш прежний без vendor.toml — обновляем"),
        None => {
            let mut p = keep(PluginAction::AskForeign, "чужой, версия неизвестна — спрашиваем");
            if p.notice.is_none() {
                p.notice = Some(PluginNotice::ForeignUnknown);
            }
            p
        }
    }
}

fn bundle_protocol_older(c: &PluginCheck) -> bool {
    c.bundle_version
        .as_deref()
        .and_then(plugin_protocol)
        .zip(c.daemon_version.as_deref().and_then(daemon_protocols))
        .is_some_and(|(p, (lo, _))| p < lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk finds the runtime by name at any depth and returns the FULL path;
    /// of two finds without a preference the shallowest one wins.
    #[test]
    fn find_runtime_so_walks_package_by_name() {
        let base = std::env::temp_dir().join(format!("rexon-wkp-find-{}", std::process::id()));
        let deep = base.join("contents/ui/Plugin/WaywallenDisplayEmbed");
        let shallow = base.join("contents/ui/WaywallenDisplayEmbed");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(&shallow).unwrap();
        std::fs::write(deep.join(RUNTIME_SO_NAME), b"a").unwrap();
        std::fs::write(shallow.join(RUNTIME_SO_NAME), b"b").unwrap();
        let base_s = base.to_str().unwrap();

        let got = find_runtime_so(base_s, None).expect("рантайм найден");
        assert_eq!(got, shallow.join(RUNTIME_SO_NAME).to_string_lossy());

        let pref = "contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so";
        let got = find_runtime_so(base_s, Some(pref)).expect("рантайм найден");
        assert_eq!(got, deep.join(RUNTIME_SO_NAME).to_string_lossy());

        std::fs::remove_dir_all(&base).unwrap();
        assert_eq!(find_runtime_so(base_s, None), None, "нет пакета — нет рантайма");
    }
}
