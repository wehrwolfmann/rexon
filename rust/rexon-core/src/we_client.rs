//! Port of `rexon/qt_wallpaper_engine/client.py` — the waywallen daemon client
//! for the «Wallpaper Engine» tab.
//!
//! The original is two layers on top of Qt: `DaemonControl` (daemon
//! discovery/autostart over D-Bus) and `WaywallenClient` (control plane over
//! WebSocket+protobuf). ALL the Qt-free logic lives here:
//!
//! 1. **Finding the daemon binary** — [`daemon_binary_candidates`] /
//!    [`find_daemon_binary`]. The order of checks matters (bug fix 2026-06-24)
//!    and is pinned by a separate test.
//! 2. **Autostart at login** — contents of the systemd unit and the `.desktop`,
//!    paths, `systemctl --user` argv, enable/disable plans ([`enable_plan`],
//!    [`disable_plan`]). ⛔ The core starts NOTHING and writes NOTHING: the plan
//!    is executed by `rexon-gui::autostart_queries` (the Python executor has
//!    been gone since 07.09.2026), where it is also wired to the
//!    «Start at login» switch on the tab's settings page.
//! 3. **`DaemonControl` decisions** — [`probe`] on top of the already existing
//!    trait [`WaywallenIo`] (the `WsPort`/`Version`/`CurrentWallpaperId`
//!    properties are NOT duplicated — one mechanism, rule #12), the detached
//!    launch argv, [`ensure_running_decision`], [`poll_decision`], parsing
//!    `ListTasks` (QtDBus strings and the `busctl --json` fallback).
//! 4. **REQUEST codec** — handing out `request_id` ([`RequestIds`]) and
//!    assembling `Request` bytes on top of the prost structs [`crate::proto`]:
//!    typed wrappers (`health`, `wallpaper_list`, …) and the generic
//!    [`request_bytes`] for the `call(field, msg)` path from `qml_ui/bridge.py`.
//!
//! ⛔ What DELIBERATELY stays in Python (the stage 2 boundary):
//! * `QWebSocket` as a whole (open/close/state/`sendBinaryMessage`) — we do not
//!   drag a second event loop (tokio/tungstenite) into the process;
//! * `QtDBus`/`QProcess`/`QTimer` — executors of the core's decisions;
//! * the `_pending` map with QObject owners and `shiboken6.isValid` on delivery
//!   (crash fix `39617f2` for dead QML objects) — the owners are live Qt
//!   objects, they have no business in Rust;
//! * **PARSING the daemon's RESPONSES** in QML pages: `bridge.py` reads the
//!   `pb.Response` fields directly (parsing of ported requests is in
//!   [`crate::we_query`]). That is stage 3. The exception is responses that
//!   `client.py` ITSELF needs on its synchronous path: [`renderer_ids_from_frame`].
//!
//! ## Bugs of the original fixed during the port (auto-fix rule)
//!
//! 1. **`int(ws_port)` in `probe`** crashed with an unhandled `ValueError` if
//!    the daemon returned a non-numeric `WsPort` (an old/broken interface
//!    version) — the WE tab did not open at all. Here an unconvertible value
//!    gives `ws_port = 0`, which sends `ensure_running` into the autostart
//!    branch. The same class as already fixed in [`crate::waywallen_client`].
//! 2. **The systemd unit path ignored `XDG_CONFIG_HOME`.**
//!    `_autostart_unit_path` hard-coded `~/.config/systemd/user`, while
//!    `_autostart_desktop_path` next to it respected `XDG_CONFIG_HOME`. With a
//!    non-standard `XDG_CONFIG_HOME` the unit was written where systemd does
//!    NOT look for it, and then `systemctl --user enable` failed with «Unit file
//!    does not exist» — autostart silently stayed off. Here both paths are
//!    computed from one root ([`config_home`]).
//! 3. **The unit killed the daemon with SIGBUS on EVERY stop.** The unit had no
//!    `KillMode`, and the default is `control-group`: systemd sends SIGTERM to
//!    ALL processes of the cgroup at once. The daemon runs from an AppImage,
//!    and next to it in the same cgroup lives the AppImage wrapper process that
//!    holds the squashfs FUSE mount (`/tmp/.mount_*`) the binary itself
//!    executes from. On SIGTERM the wrapper unmounts the image immediately,
//!    while the daemon is still finishing its normal shutdown — the first code
//!    page not in memory gives SIGBUS/`BUS_ADRERR`. Result: every
//!    `stop`/`restart`/session logout (`PartOf=graphical-session.target`)
//!    ended in a core dump, and `Restart=on-failure` counted it as a failure
//!    and brought the daemon back up. Here the unit carries
//!    [`KillMode=mixed`](AUTOSTART_UNIT_TMPL): ONLY the main process gets
//!    SIGTERM, the mount lives until it exits. Measured: SIGTERM to the main
//!    PID alone — clean exit, SIGTERM to the whole cgroup — `status=7/BUS`
//!    (reproduced 2026-08-22).

use std::collections::HashMap;
use serde::Serialize;
use prost::Message;

use crate::proto::control as pb;
use crate::waywallen_client::{DbusValue, WaywallenIo};

// ============================================================================
// D-Bus and polling constants
// ============================================================================

/// Daemon service name (the same as [`crate::waywallen_client::BUS_NAME`] — a
/// re-export so the WE tab does not keep a second copy of the address).
pub use crate::waywallen_client::{
    BUS_NAME as DBUS_SERVICE, IFACE as DBUS_IFACE, OBJECT_PATH as DBUS_PATH,
};

/// D-Bus polling interval after autostarting the daemon, ms.
pub const POLL_MS: u64 = 500;

/// How many times to poll (≈12 seconds for a cold daemon start).
pub const POLL_LIMIT: i64 = 24;

/// `Properties.Get` timeout, ms.
pub const PROPERTY_TIMEOUT_MS: u64 = 1000;

/// `ListTasks` over D-Bus timeout, ms.
pub const LIST_TASKS_TIMEOUT_MS: u64 = 2000;

/// `busctl` fallback timeout, seconds.
pub const BUSCTL_TIMEOUT_SECS: u64 = 3;

/// Arguments the daemon starts with from Rexon: do not open its own UI and
/// tray; the daemon restores wallpapers itself from its config.
pub const DAEMON_ARGS: [&str; 2] = ["--no-ui", "--no-tray"];

// ============================================================================
// 1. Finding the daemon binary
// ============================================================================

/// Fallback candidates (by priority), port of `_DAEMON_FALLBACKS`.
///
/// ⚠️ The knowingly outdated extracted binary `owe-ref/install/bin/waywallen`
/// is deliberately NOT included: it does not know the DB migration rewritten
/// by the new AppImage → the daemon crashes at startup (bug 2026-06-24).
///
/// ⚠️ The former second candidate `~/Projects/waywallen/waywallen.AppImage`
/// (dev build) was removed: the file is gone, the symlink
/// `~/.local/bin/waywallen` points to the managed AppImage. The dead path only
/// made the search longer.
pub const DAEMON_FALLBACKS: [&str; 1] = [
    // symlink to the managed AppImage
    "~/.local/bin/waywallen",
];

/// Names of the bundled AppImage in `rexon/vendor/` (in check order).
pub const BUNDLED_NAMES: [&str; 2] = ["waywallen.AppImage", "waywallen"];

/// Engine directory installed by the Rexon package (`packaging/aur/PKGBUILD`):
/// the waywallen AppImage and next to it `plugins/<id>/` — the Open Wallpaper
/// Engine plugin.
///
/// Why `/usr/lib` and not `/usr/share`: it holds architecture-specific
/// executables (AppImage, plugin renderers, CEF). Why the directory has
/// EXACTLY this layout: the waywallen daemon accepts the `--plugin <dir>` flag
/// and looks for plugins in `<dir>/plugins/` (waywallen v0.4.2,
/// `daemon/bootstrap.rs`), so the engine directory is passed to the daemon as
/// is — no second layout is needed.
pub const ENGINE_DIR: &str = "/usr/lib/rexon/engine";

/// Name of the engine AppImage in [`ENGINE_DIR`].
pub const ENGINE_APPIMAGE_NAME: &str = "waywallen.AppImage";

/// Plugins subdirectory inside the directory passed to the daemon with the
/// [`PLUGIN_FLAG`] flag (the daemon's contract, see [`ENGINE_DIR`]).
pub const ENGINE_PLUGINS_SUBDIR: &str = "plugins";

/// The daemon flag «one more plugin root». waywallen has it since 0.3.x; 0.2.x
/// does not have it at all (an unknown flag means refusing to start), so the
/// flag is passed ONLY when the bundle's plugins directory really exists on
/// disk, i.e. the engine of the same bundle or newer sits next to it.
pub const PLUGIN_FLAG: &str = "--plugin";

/// Engine AppImage from the package bundle.
pub fn engine_appimage_path(engine_dir: &str) -> String {
    format!("{}/{ENGINE_APPIMAGE_NAME}", engine_dir.trim_end_matches('/'))
}

/// Bundle plugins directory (`<engine dir>/plugins`).
pub fn engine_plugins_path(engine_dir: &str) -> String {
    format!("{}/{ENGINE_PLUGINS_SUBDIR}", engine_dir.trim_end_matches('/'))
}

// ── Engine from distribution packages (since 30.10.2026) ────────────────────
//
// The Rexon package for distributions (`packaging/aur/PKGBUILD`) NO LONGER
// bundles the engine: it depends on the separate `waywallen` and
// `open-wallpaper-engine` packages. A foreign AppImage with statically
// bundled libraries and our own archive of GPL sources inside a pacman
// package is what distribution maintainers will not let through review;
// updating the engine is the package manager's job, not Rexon's. The bundled
// layout ([`ENGINE_DIR`]) stays in the core for deliveries without a system
// engine package; if both are installed, the system one goes first
// ([`engine_layouts`]).
//
// Where Arch/AUR packages put files (checked against their PKGBUILDs 30.10.2026):
//   * `waywallen` — `lito install --prefix /usr`: daemon `/usr/bin/waywallen`,
//     built-in plugins in `/usr/share/waywallen/plugins/`,
//     `libwaywallen-bridge.so` in `/usr/lib`; upstream installs no systemd
//     unit (Rexon writes it, [`AUTOSTART_UNIT`]);
//   * `waywallen-bin` — the unpacked AppImage in `/opt/waywallen-bin`,
//     `/usr/bin/waywallen` is a symlink to its `AppRun`;
//   * `open-wallpaper-engine` — the plugin in
//     `/usr/share/waywallen/plugins/org.waywallen.open-wallpaper-engine/`
//     (`install.lua` of tag v0.3.2), renderers `waywallen-wescene-renderer`
//     and `waywallen-weweb-renderer` in `/usr/bin` (CEF is the system `cef`
//     package).
//
// A daemon living in `<prefix>/bin` looks for plugins in
// `<prefix>/share/waywallen/plugins` ITSELF (`standard_plugin_roots`,
// waywallen v0.5.0) — by the real path of its file, after symlinks. So the
// `waywallen` daemon from the package does not need the `--plugin` flag,
// while `waywallen-bin` and the user's managed AppImage do: their file does
// not live under `/usr`. Passing the flag «just in case» is not allowed: the
// daemon scans the same directory twice and keeps two identical entries of
// every plugin.

/// Engine daemon from the distribution package.
pub const SYSTEM_DAEMON: &str = "/usr/bin/waywallen";

/// Engine data directory from distribution packages: holds `plugins/`.
pub const SYSTEM_ENGINE_DIR: &str = "/usr/share/waywallen";

/// Whose engine: bundled into the Rexon package or from distribution packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// Bundled into the Rexon delivery ([`ENGINE_DIR`]): AppImage and plugins side by side.
    Bundled,
    /// Separate distribution packages ([`SYSTEM_DAEMON`],
    /// [`SYSTEM_ENGINE_DIR`]): the package manager updates them.
    System,
}

/// Layout of an engine installed by a package rather than by the user: where
/// the daemon is and where the directory with `plugins/` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLayout {
    pub kind: EngineKind,
    /// Daemon binary.
    pub daemon: String,
    /// Directory holding `plugins/` (it is also what `--plugin` receives).
    pub dir: String,
}

impl EngineLayout {
    /// Bundled set in the `engine_dir` directory.
    pub fn bundled_in(engine_dir: &str) -> EngineLayout {
        EngineLayout {
            kind: EngineKind::Bundled,
            daemon: engine_appimage_path(engine_dir),
            dir: engine_dir.trim_end_matches('/').to_string(),
        }
    }

    /// Engine from distribution packages: daemon `daemon`, data in `dir`.
    pub fn system_in(daemon: &str, dir: &str) -> EngineLayout {
        EngineLayout {
            kind: EngineKind::System,
            daemon: daemon.to_string(),
            dir: dir.trim_end_matches('/').to_string(),
        }
    }

    /// `<dir>/plugins`.
    pub fn plugins_dir(&self) -> String {
        engine_plugins_path(&self.dir)
    }
}

/// Engine layouts on the customer's machine, by priority: distribution
/// packages, then the bundled set. One source of truth for finding the
/// daemon, the `--plugin` flag and the package watcher.
///
/// System package first: the package manager updates it together with the
/// system libraries, while the bundled set (Steam delivery, manual install)
/// is only a fallback where there is no engine package. If both are installed,
/// the system one is launched, it gets the bundle's plugins via the `--plugin`
/// flag ([`plugin_root_for`]), and of two plugin copies the daemon takes the
/// newer one.
pub fn engine_layouts() -> Vec<EngineLayout> {
    vec![
        EngineLayout::system_in(SYSTEM_DAEMON, SYSTEM_ENGINE_DIR),
        EngineLayout::bundled_in(ENGINE_DIR),
    ]
}

/// The layout whose daemon is `binary` (the path from the daemon search, before links).
pub fn layout_of<'a>(layouts: &'a [EngineLayout], binary: &str) -> Option<&'a EngineLayout> {
    layouts.iter().find(|l| l.daemon == binary)
}

/// The data directory the daemon sees on its own: `<prefix>/share/waywallen`
/// for the file `<prefix>/bin/<name>`. `resolved` is the REAL path of the file
/// (after symlinks): the daemon computes its prefix from it.
pub fn daemon_own_engine_dir(resolved: &str) -> Option<String> {
    let p = std::path::Path::new(resolved);
    let prefix = p.parent()?.parent()?;
    Some(prefix.join("share/waywallen").to_string_lossy().into_owned())
}

/// Which directory to pass to the daemon with the `--plugin` flag.
///
/// `present` — layouts whose `plugins/` really exists on disk (in
/// [`engine_layouts`] priority); `resolved_binary` — the real path of the
/// daemon binary that will be launched (`None` — unknown). The first layout
/// whose directory the daemon does not see on its own is taken
/// ([`daemon_own_engine_dir`]); `None` — the flag is not needed.
pub fn plugin_root_for(present: &[EngineLayout], resolved_binary: Option<&str>) -> Option<String> {
    let own = resolved_binary.and_then(daemon_own_engine_dir);
    present
        .iter()
        .map(|l| l.dir.clone())
        .find(|d| own.as_deref() != Some(d.as_str()))
}

/// Daemon launch arguments: [`DAEMON_ARGS`] and, if the bundle has a plugins
/// directory, `--plugin <engine dir>`.
///
/// `plugin_root` — the engine directory if it holds `plugins/` (the executor
/// makes the observation), `None` — no bundle.
///
/// Which plugin wins when the same `id` lies both in the bundle and in the
/// user's `~/.local/share/waywallen/plugins` is decided by the daemon ITSELF,
/// and its decision is exactly what we need: newer by `version` from
/// `plugin.toml` (semantic comparison), on a tie — the user's copy
/// (`select_active_plugins`/`plugin_priority`, waywallen v0.4.2). So:
///   * the package got updated while the user still has an old manual copy —
///     the new one from the package runs, the old one does not get in the way;
///   * the user installed a plugin newer than the bundle — their copy runs,
///     and a package update will not «roll it back».
/// We did not duplicate this choice on our side: a second decision next to
/// the daemon's would silently drift from it. The mirror of the rule for
/// reports and checks is [`active_plugin_source`].
pub fn engine_launch_args(plugin_root: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = DAEMON_ARGS.iter().map(|a| a.to_string()).collect();
    if let Some(root) = plugin_root {
        args.push(PLUGIN_FLAG.to_string());
        args.push(root.trim_end_matches('/').to_string());
    }
    args
}

/// Where the daemon takes a plugin from when it lies in two places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    /// The copy from the package bundle ([`ENGINE_DIR`]).
    Bundled,
    /// The user's copy (`$XDG_DATA_HOME/waywallen/plugins`).
    User,
}

/// Mirror of the daemon's choice between copies of one plugin (see
/// [`engine_launch_args`]): newer by version, on a tie — the user's copy. A
/// version that does not parse as `X.Y.Z` is compared as a string — like the
/// daemon does.
pub fn active_plugin_source(bundled_version: &str, user_version: &str) -> PluginSource {
    let norm = |v: &str| v.trim().trim_start_matches(['v', 'V']).to_string();
    let (b, u) = (norm(bundled_version), norm(user_version));
    let semver = |v: &str| -> Option<Vec<u128>> {
        let core = v.split(['-', '+']).next().unwrap_or("");
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        parts.iter().map(|p| p.parse::<u128>().ok()).collect()
    };
    let order = match (semver(&b), semver(&u)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => b.cmp(&u),
    };
    if order.is_gt() {
        PluginSource::Bundled
    } else {
        PluginSource::User
    }
}

// ── The live daemon did not pick up the plugin from the package ─────────────
//
// Defect from the live check 24.10.2026: the waywallen plugin list showed the
// user's copy 0.3.0 and an offer to update to 0.3.1, although the Rexon
// package had already put 0.3.1 into [`ENGINE_DIR`]. The daemon's choice of
// copy itself is correct (newer by version, on a tie — the user's copy, see
// [`engine_launch_args`]), but it makes it ONCE — when it scans plugin roots
// at startup (`daemon/bootstrap.rs`, `scan_plugin_roots`) and after a plugin
// is installed through its API. The daemon has no «rescan» command. A daemon
// started BEFORE the package update (or by someone without the `--plugin`
// flag) keeps holding the old copy until it is restarted.
//
// So the cure is restarting the daemon, not editing the user's files:
//   * their copy need not be deleted — after a restart the daemon will not
//     take it as long as the package has a newer one;
//   * «updating» it with the regular waywallen button means installing the
//     freshest version from GitHub bypassing the package, and from then on it
//     would override the packaged one;
//   * the daemon has no way to disable one copy — there is no such request.

/// A plugin copy: from the bundle's `plugin.toml` or active in the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCopy {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// `id`, `name` and `version` from the text of `plugin.toml` (the `[plugin]`
/// section). `None` — the manifest is broken or lacks `id`/`version`.
pub fn plugin_manifest_copy(toml_text: &str) -> Option<PluginCopy> {
    let value = toml_text.parse::<toml::Value>().ok()?;
    let plugin = value.get("plugin")?;
    let field = |key: &str| plugin.get(key).and_then(|v| v.as_str()).map(str::trim);
    let id = field("id").filter(|s| !s.is_empty())?;
    let version = field("version").filter(|s| !s.is_empty())?;
    Some(PluginCopy {
        id: id.to_string(),
        name: field("name").unwrap_or_default().to_string(),
        version: version.to_string(),
    })
}

/// Active plugin copies from the daemon's response to `plugin_list`. `None` —
/// the frame did not parse, it is not a `plugin_list` response, or the daemon
/// answered with an error.
pub fn active_plugins_from_frame(frame_bytes: &[u8]) -> Option<Vec<PluginCopy>> {
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let pb::server_frame::Kind::Response(rsp) = frame.kind? else {
        return None;
    };
    if rsp.status != pb::Status::Ok as i32 {
        return None;
    }
    let pb::response::Payload::PluginList(list) = rsp.payload? else {
        return None;
    };
    Some(
        list.plugins
            .into_iter()
            .map(|p| PluginCopy { id: p.id, name: p.name, version: p.version })
            .collect(),
    )
}

/// A bundle plugin the live daemon did not pick up, although by its own rule
/// it should have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePlugin {
    pub id: String,
    pub name: String,
    pub bundled_version: String,
    /// The version the daemon holds; empty — the daemon has no such plugin at all
    /// (it was started without `--plugin`, and the user has no copy).
    pub active_version: String,
}

/// Bundle plugins the daemon holds in an old version (or does not see).
///
/// The rule is the same as the daemon's ([`active_plugin_source`]): the bundle
/// copy must be running if it is STRICTLY newer than the active one. The
/// active one is newer or equal — that is intended (the user installed their
/// own), not a defect.
pub fn stale_bundled_plugins(bundled: &[PluginCopy], active: &[PluginCopy]) -> Vec<StalePlugin> {
    let mut out: Vec<StalePlugin> = bundled
        .iter()
        .filter_map(|b| {
            let active_version = active
                .iter()
                .find(|a| a.id == b.id)
                .map(|a| a.version.clone())
                .unwrap_or_default();
            let behind = active_version.is_empty()
                || active_plugin_source(&b.version, &active_version) == PluginSource::Bundled;
            behind.then(|| StalePlugin {
                id: b.id.clone(),
                name: if b.name.is_empty() { b.id.clone() } else { b.name.clone() },
                bundled_version: b.version.clone(),
                active_version,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Attempt key: which plugins and up to which versions the restart should
/// bring up. The same key a second time — the restart did not help. The key is
/// printed to the log, so it reads left to right: «in the daemon → in the
/// package» (`id=0.3.1→0.3.2`; no plugin in the daemon — `id=нет→0.3.2`). The
/// former form `id=0.3.2<0.3.1` read as «0.3.2 is less than 0.3.1».
pub fn stale_plugins_key(stale: &[StalePlugin]) -> String {
    stale
        .iter()
        .map(|s| {
            let active = if s.active_version.is_empty() { "нет" } else { s.active_version.as_str() };
            format!("{}={active}→{}", s.id, s.bundled_version)
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// What to do after comparing the live daemon's plugins with the bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRefresh {
    /// The daemon holds what it should.
    Fine,
    /// Restart the daemon; remember `key` ([`stale_plugins_key`]).
    Restart { key: String, stale: Vec<StalePlugin> },
    /// A restart for exactly this was already done — it did not help. We do not
    /// touch it a second time: restarting in a loop would kill the wallpaper
    /// every few seconds.
    GiveUp { stale: Vec<StalePlugin> },
}

/// The comparison decision. `tried_key` — the key of the previous attempt in this process.
pub fn plugin_refresh_decision(
    bundled: &[PluginCopy],
    active: &[PluginCopy],
    tried_key: Option<&str>,
) -> PluginRefresh {
    let stale = stale_bundled_plugins(bundled, active);
    if stale.is_empty() {
        return PluginRefresh::Fine;
    }
    let key = stale_plugins_key(&stale);
    if tried_key == Some(key.as_str()) {
        PluginRefresh::GiveUp { stale }
    } else {
        PluginRefresh::Restart { key, stale }
    }
}

/// What to do when the live daemon and the bundle engine on disk diverge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineRefresh {
    /// The daemon is the same engine as on disk (or there is nothing to compare with).
    Fine,
    /// Restart the daemon; remember `key` ([`engine_refresh_key`]).
    Restart { key: String },
    /// A restart for exactly this pair of versions was already done — it did not help.
    GiveUp,
}

/// Attempt key: from which version to which the restart should move.
pub fn engine_refresh_key(live: &str, on_disk: &str) -> String {
    format!("engine={live}→{on_disk}")
}

/// Comparing the live daemon with the bundle AppImage after the package
/// replaced the file.
///
/// `runs_bundled` — the daemon is launched from the bundle (the binary search
/// chose it, not the user's AppImage: the user updates that one, and a
/// restart for someone else's file would be overreach). `live` — the live
/// daemon's `Version` property, `on_disk` — `--version` of the file on disk.
/// Parsed numbers are compared ([`crate::engine_update::parse_binary_version`]):
/// a build suffix on one of the sources (`0.5.0-x86_64`) is no reason. If the
/// version cannot be learned on either side — do not touch: a blind restart
/// would kill the wallpaper for nothing.
pub fn engine_refresh_decision(runs_bundled: bool, live: &str, on_disk: &str, tried_key: Option<&str>) -> EngineRefresh {
    let live = crate::engine_update::parse_binary_version(live);
    let on_disk = crate::engine_update::parse_binary_version(on_disk);
    if !runs_bundled || live.is_empty() || on_disk.is_empty() || live == on_disk {
        return EngineRefresh::Fine;
    }
    let key = engine_refresh_key(&live, &on_disk);
    if tried_key == Some(key.as_str()) {
        EngineRefresh::GiveUp
    } else {
        EngineRefresh::Restart { key }
    }
}

/// Text of the notification about a restart for a new engine from the package
/// (the title is shared, [`plugin_refresh_title`]).
pub fn engine_refresh_body(live: &str, on_disk: &str) -> String {
    engine_refresh_body_for(EngineKind::Bundled, live, on_disk)
}

/// [`engine_refresh_body`] for the engine of the layout that installed it: the
/// bundled one comes with a Rexon update, the system one with a system update.
pub fn engine_refresh_body_for(kind: EngineKind, live: &str, on_disk: &str) -> String {
    let (live, on_disk) = (
        crate::engine_update::parse_binary_version(live),
        crate::engine_update::parse_binary_version(on_disk),
    );
    match kind {
        EngineKind::Bundled => crate::i18n::trf(
            // TRANSLATORS: Body of the "The wallpaper engine was restarted" notification. %1 is the old engine version, %2 the new one from the Rexon update.
            "It was still running version %1; the Rexon update brought version %2.",
            &[&live, &on_disk],
        ),
        EngineKind::System => crate::i18n::trf(
            // TRANSLATORS: Body of the "The wallpaper engine was restarted" notification. %1 is the old engine version, %2 the new one that a system update installed.
            "It was still running version %1; a system update installed version %2.",
            &[&live, &on_disk],
        ),
    }
}

/// Daemon task kind «service»: a long-lived loop (output socket, sound, MPRIS,
/// power watcher…). Such tasks stay `running` for the daemon's whole life.
pub const TASK_KIND_SERVICE: &str = "service";

/// Tasks that WAIT for an event rather than work: a restart cuts nothing off
/// for them. `boot/display-watcher` waits for the first display and stays
/// `running` while there is none — on a desktop without an output (MATE, XFCE
/// on X11) that is forever.
pub const WAITING_TASK_NAMES: &[&str] = &["boot/display-watcher"];

/// Whether the daemon is busy with work a restart would cut off (downloading,
/// scanning, installing a plugin, applying a wallpaper): `ListTasks` has such
/// a task in the `running` state. Then the restart is postponed until the next
/// connection.
///
/// ⛔ Defect 30.10.2026 (MATE, daemon 0.5.0): ANY task in `running` was
/// counted, and daemon 0.5.0 always has more than a dozen of them — services
/// ([`TASK_KIND_SERVICE`]) and the waiting `boot/display-watcher`
/// ([`WAITING_TASK_NAMES`]). The daemon came out «busy» forever, and the
/// restart for the plugin from the package was postponed at every comparison.
pub fn daemon_busy(tasks: &[DaemonTask]) -> bool {
    tasks.iter().any(|t| {
        t.state == "running"
            && t.kind != TASK_KIND_SERVICE
            && !WAITING_TASK_NAMES.contains(&t.name.as_str())
    })
}

/// Title of the notification about a restart for a plugin from the package.
pub fn plugin_refresh_title() -> String {
    crate::i18n::tr("The wallpaper engine was restarted")
}

/// Notification text: what the daemon will now take from the package and instead of what.
pub fn plugin_refresh_body(stale: &[StalePlugin]) -> String {
    plugin_refresh_body_for(EngineKind::Bundled, stale)
}

/// [`plugin_refresh_body`] for plugins of the layout that installed them.
pub fn plugin_refresh_body_for(kind: EngineKind, stale: &[StalePlugin]) -> String {
    let mut lines = vec![match kind {
        EngineKind::Bundled => crate::i18n::tr(
            "It was still running an older plugin; Rexon has a newer one in its package:",
        ),
        EngineKind::System => crate::i18n::tr(
            "It was still running an older plugin; a system update installed a newer one:",
        ),
    }];
    for s in stale {
        lines.push(if s.active_version.is_empty() {
            crate::i18n::trf("%1 %2 (was not loaded)", &[&s.name, &s.bundled_version])
        } else {
            crate::i18n::trf("%1 %2 instead of %3", &[&s.name, &s.bundled_version, &s.active_version])
        });
    }
    lines.join("\n")
}

// ── Package watcher: comparison right after an update ───────────────────────
//
// The comparison above runs when the tab connects to the daemon. But Rexon
// usually lives in the tray, nobody opens the tab, and after `pacman -Syu`
// the daemon would keep the old plugin until the next session login. So the
// tray watches the engine directory (inotify, `fs_watch`) and after things
// settle down runs the same comparison.

/// Quiet pause after the package manager's last write, ms: pacman writes
/// files in a batch, and plugins must be looked at once it is over.
pub const ENGINE_WATCH_QUIET_MS: i32 = 8000;
/// Cap on waiting for the end of the batch, ms: even if writing does not
/// settle (a big system update), compare two minutes after the first write.
pub const ENGINE_WATCH_MAX_MS: u64 = 120_000;
/// Repeated comparison while the daemon is busy with tasks (downloading, scanning), ms.
pub const ENGINE_WATCH_BUSY_RETRY_MS: i32 = 60_000;
/// How many times to repeat the comparison while the daemon is busy (an hour
/// at a one-minute pause); after that — until the next package change or tab
/// connection.
pub const ENGINE_WATCH_BUSY_RETRIES: u32 = 60;
/// Deadline of the watcher's one-off `plugin_list` request, ms.
pub const ENGINE_WATCH_QUERY_MS: i32 = 5000;

/// What to watch to notice a new plugin in the package.
///
/// * the parent of the engine directory — the name of the directory itself
///   (the package may have replaced it entirely, or it appeared for the first
///   time);
/// * the engine directory — `plugins/` and the engine AppImage itself: the
///   live daemon will not pick up an engine replaced by the package until it
///   is restarted ([`engine_refresh_decision`]);
/// * `plugins/` — any name: a plugin folder appeared, vanished or was replaced;
/// * every `plugins/<folder>/` — only `plugin.toml`: the daemon takes the
///   version from there, and replacing libraries without a version change does
///   not change the comparison.
///
/// `plugin_dirs` — folder names in `plugins/` at the time the watch was set
/// up; new ones are picked up when the watcher re-arms the watch after a batch.
///
/// A daemon outside the engine directory (distribution packages:
/// `/usr/bin/waywallen`) is a separate target: its directory, only the
/// daemon's name.
pub fn engine_watch_targets(layout: &EngineLayout, plugin_dirs: &[String]) -> Vec<crate::fs_watch::WatchTarget> {
    use crate::fs_watch::WatchTarget;
    let engine = layout.dir.trim_end_matches('/');
    let plugins = engine_plugins_path(engine);
    let mut out = Vec::new();
    if let Some((parent, name)) = engine.rsplit_once('/') {
        if !parent.is_empty() && !name.is_empty() {
            out.push(WatchTarget { dir: parent.to_string(), names: vec![name.to_string()] });
        }
    }
    let mut names = vec![ENGINE_PLUGINS_SUBDIR.to_string()];
    match layout.daemon.rsplit_once('/') {
        Some((dir, name)) if dir == engine => names.push(name.to_string()),
        Some((dir, name)) if !dir.is_empty() && !name.is_empty() => {
            out.push(WatchTarget { dir: dir.to_string(), names: vec![name.to_string()] });
        }
        _ => {}
    }
    out.push(WatchTarget { dir: engine.to_string(), names });
    out.push(WatchTarget { dir: plugins.clone(), names: Vec::new() });
    for d in plugin_dirs {
        if d.is_empty() || d.contains('/') {
            continue;
        }
        out.push(WatchTarget { dir: format!("{plugins}/{d}"), names: vec!["plugin.toml".to_string()] });
    }
    out
}

/// One step of the binary search. List order = priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonCandidate {
    /// Valid if it is an existing executable FILE
    /// (`os.path.isfile && os.access(X_OK)`).
    File(String),
    /// `shutil.which(name)` — search for an executable in `PATH`.
    PathLookup(String),
}

/// Live FS checks for [`find_daemon_binary`]. The core does not touch the FS
/// itself — the same trick as `WaywallenIo`: transport outside, order inside.
pub trait DaemonFs {
    /// `os.path.isfile(path) and os.access(path, os.X_OK)`.
    fn is_exec_file(&mut self, path: &str) -> bool;
    /// `shutil.which(name)` → full path or `None`.
    fn which(&mut self, name: &str) -> Option<String>;
    /// Engine version at the path (`X.Y.Z`) or `None` if it cannot be learned.
    ///
    /// Asked ONLY when the bundle engine and the managed AppImage both lie on
    /// disk at the same time (see [`find_daemon_binary_in`]) — the executor is
    /// allowed to run `--version`. Defaults to `None`: then the list order
    /// stands.
    fn binary_version(&mut self, _path: &str) -> Option<String> {
        None
    }
}

/// `~` at the start of a path → `home` (port of `os.path.expanduser`; the
/// `~/...` form is enough, the module has no others). The core's shared
/// expander, the `~/.config` branch respects `$XDG_CONFIG_HOME` — see
/// [`crate::utils::expand_user`].
pub fn expanduser(path: &str, home: &str) -> String {
    crate::utils::expand_user(path, home, None)
}

/// The full ordered list of daemon binary candidates.
///
/// `rexon_pkg_dir` — the `rexon/` package directory (computed from `__file__`
/// in Python), `home` — the home directory. The order is INTENTIONAL (bug fix
/// 2026-06-24, covered by the test `порядок кандидатов`):
///   1. the AppImage bundled into the tree (`rexon/vendor/`) — a developer
///      override: put an image next to the sources, and that is what runs;
///   2. the engine from the distribution package ([`SYSTEM_DAEMON`]);
///   3. the engine bundled into the Rexon delivery ([`ENGINE_DIR`]);
///   4. the managed AppImage in `~/.local/share/waywallen/`;
///   5. `waywallen` in `PATH` (usually a symlink to the managed one);
///   6. known fallback paths ([`DAEMON_FALLBACKS`]).
///
/// ⚠️ The order between 2–3 and 4 is not final: see [`find_daemon_binary_in`].
pub fn daemon_binary_candidates(
    rexon_pkg_dir: &str,
    home: &str,
) -> Vec<DaemonCandidate> {
    daemon_binary_candidates_in(rexon_pkg_dir, home, ENGINE_DIR)
}

/// [`daemon_binary_candidates`] with an explicit bundle engine directory (for
/// checks — a real `/usr/lib/rexon/engine` may well exist on the test bench).
pub fn daemon_binary_candidates_in(
    rexon_pkg_dir: &str,
    home: &str,
    engine_dir: &str,
) -> Vec<DaemonCandidate> {
    daemon_binary_candidates_for(rexon_pkg_dir, home, &layouts_with(engine_dir))
}

/// [`engine_layouts`] layouts with a substituted bundle directory (for checks:
/// a real `/usr/lib/rexon/engine` may well exist on the test bench).
fn layouts_with(engine_dir: &str) -> Vec<EngineLayout> {
    engine_layouts()
        .into_iter()
        .map(|l| match l.kind {
            EngineKind::Bundled => EngineLayout::bundled_in(engine_dir),
            EngineKind::System => l,
        })
        .collect()
}

/// [`daemon_binary_candidates`] with an explicit set of package engine layouts.
pub fn daemon_binary_candidates_for(
    rexon_pkg_dir: &str,
    home: &str,
    layouts: &[EngineLayout],
) -> Vec<DaemonCandidate> {
    let pkg = rexon_pkg_dir.trim_end_matches('/');
    let mut out: Vec<DaemonCandidate> = BUNDLED_NAMES
        .iter()
        .map(|name| DaemonCandidate::File(format!("{pkg}/vendor/{name}")))
        .collect();
    out.extend(layouts.iter().map(|l| DaemonCandidate::File(l.daemon.clone())));
    out.push(DaemonCandidate::File(managed_appimage_path(home)));
    out.push(DaemonCandidate::PathLookup("waywallen".to_string()));
    out.extend(
        DAEMON_FALLBACKS
            .iter()
            .map(|c| DaemonCandidate::File(expanduser(c, home))),
    );
    out
}

/// The managed AppImage in the user data dir — port of `_managed_appimage`.
/// Dev mode installs new engine versions here, and the
/// `~/.local/bin/waywallen` symlink points here too; this file migrates the
/// engine DB to its own schema → it must be preferred over extracted outdated
/// binaries.
///
/// The path is NOT duplicated: the single source is [`crate::engine_update`],
/// which installs this very file (completeness rule — one place to edit when
/// the directory changes).
pub fn managed_appimage_path(home: &str) -> String {
    crate::engine_update::managed_appimage(home)
}

/// Paths of the bundled AppImage — port of `_bundled_appimage` (candidates, no checks).
pub fn bundled_appimage_paths(rexon_pkg_dir: &str) -> Vec<String> {
    let pkg = rexon_pkg_dir.trim_end_matches('/');
    BUNDLED_NAMES
        .iter()
        .map(|name| format!("{pkg}/vendor/{name}"))
        .collect()
}

/// Port of `find_daemon_binary` — the first valid candidate from
/// [`daemon_binary_candidates`].
pub fn find_daemon_binary<FS: DaemonFs>(
    fs: &mut FS,
    rexon_pkg_dir: &str,
    home: &str,
) -> Option<String> {
    find_daemon_binary_in(fs, rexon_pkg_dir, home, ENGINE_DIR)
}

/// The package engine (bundled set or distribution package) or the managed
/// AppImage — which one to run when both lie on disk.
///
/// The package engine is higher in the list: the package installs it so that
/// wallpapers work right away and without a network. But the managed
/// AppImage is installed by the user (the update button on the «Developer»
/// page), and if it is NEWER than the bundle, it has already rewritten the
/// engine database to its schema. Running the older bundle after that ends in
/// a refusal to start («migration … is missing») — exactly the 2026-06-24
/// defect. So when it is newer, it wins.
///
/// The version cannot be learned (an old build without `--version`, a launch
/// failure) — the list order stands: the bundle. Equal versions — the bundle
/// too: its file will not be swapped under a live daemon by either the button
/// or the user's package manager.
pub fn prefer_managed_over_bundled(
    bundled_version: Option<&str>,
    managed_version: Option<&str>,
) -> bool {
    match (bundled_version, managed_version) {
        (Some(b), Some(m)) if !b.is_empty() && !m.is_empty() => {
            crate::engine_update::ver_tuple(m) > crate::engine_update::ver_tuple(b)
        }
        _ => false,
    }
}

/// [`find_daemon_binary`] with an explicit bundle engine directory.
pub fn find_daemon_binary_in<FS: DaemonFs>(
    fs: &mut FS,
    rexon_pkg_dir: &str,
    home: &str,
    engine_dir: &str,
) -> Option<String> {
    find_daemon_binary_for(fs, rexon_pkg_dir, home, &layouts_with(engine_dir))
}

/// [`find_daemon_binary`] with an explicit set of package engine layouts.
///
/// The rule «the managed AppImage wins only if strictly newer»
/// ([`prefer_managed_over_bundled`]) is one for both layouts: any newer engine
/// rewrites the engine database to its schema, whoever installed it.
pub fn find_daemon_binary_for<FS: DaemonFs>(
    fs: &mut FS,
    rexon_pkg_dir: &str,
    home: &str,
    layouts: &[EngineLayout],
) -> Option<String> {
    let managed = managed_appimage_path(home);
    for cand in daemon_binary_candidates_for(rexon_pkg_dir, home, layouts) {
        match cand {
            DaemonCandidate::File(p) => {
                if fs.is_exec_file(&p) {
                    if layout_of(layouts, &p).is_some() && fs.is_exec_file(&managed) {
                        let b = fs.binary_version(&p);
                        let m = fs.binary_version(&managed);
                        if prefer_managed_over_bundled(b.as_deref(), m.as_deref()) {
                            return Some(managed);
                        }
                    }
                    return Some(p);
                }
            }
            DaemonCandidate::PathLookup(name) => {
                if let Some(p) = fs.which(&name) {
                    return Some(p);
                }
            }
        }
    }
    None
}

// ============================================================================
// 2. Engine autostart at login
// ============================================================================

/// Name of the autostart systemd user unit.
pub const AUTOSTART_UNIT: &str = "rexon-wallpaper-engine.service";

/// Name of the XDG autostart `.desktop` (fallback for systems without systemd).
pub const AUTOSTART_DESKTOP: &str = "rexon-wallpaper-engine.desktop";

/// systemd user unit template. `{binary}` — absolute path of the daemon binary.
///
/// ⚠️ `KillMode=mixed` is mandatory and must not be removed (fix of bug #3 in
/// the module header). The daemon executes from an AppImage, i.e. from a FUSE
/// mount held by a neighbouring process in the SAME cgroup. With the default
/// `control-group` SIGTERM hits both at once, the image falls away under a
/// still-living daemon, and the normal shutdown ends in SIGBUS instead of
/// code 0.
///
/// `Description` is in English: systemd has no localized keys, and the line
/// is only visible in `systemctl`/the journal (see `session_splash`).
const AUTOSTART_UNIT_TMPL: &str = "\
[Unit]
Description=Rexon: wallpaper engine (waywallen)
# The file is managed by Rexon (WE tab → «Start at login»); edits will be overwritten.
After=graphical-session.target
PartOf=graphical-session.target

[Service]
# The daemon is already alive outside the service (started by the waywallen window or by hand) — there will be
# no second copy: the service is honestly skipped with the reason in the journal rather than
# exiting «successfully» with the words «already running».
ExecCondition=/bin/sh -c \"if busctl --user status {bus} >/dev/null 2>&1; then echo 'waywallen is already running outside this service; not starting a second copy' >&2; exit 1; fi\"
ExecStart=\"{binary}\" --no-ui --no-tray{plugin}
# The daemon lives in an AppImage: the FUSE mount is held by a neighbouring process of the same
# cgroup. SIGTERM only to the main one — otherwise the image is unmounted under the running
# daemon and shutdown fails with SIGBUS.
KillMode=mixed
Restart=on-failure
RestartSec=3

[Install]
WantedBy=graphical-session.target
";

// TRANSLATORS: Name of Rexon's wallpaper engine autostart entry (shown in the desktop's autostart settings).
const AUTOSTART_NAME: &str = crate::i18n::n_("Rexon Wallpaper Engine");
// TRANSLATORS: Description of the wallpaper engine autostart entry. waywallen is a program name.
const AUTOSTART_COMMENT: &str = crate::i18n::n_("Start the wallpaper engine (waywallen) at login");

/// XDG autostart `.desktop` template. `{name}` and `{comment}` are
/// `Key=` + `Key[<code>]=` lines in all catalog languages
/// ([`crate::system_l10n::desktop_key`]): previously `Comment` was Russian
/// only, without a language key, and `Name[ru]` was a pair in code.
const AUTOSTART_DESKTOP_TMPL: &str = "\
[Desktop Entry]
Type=Application
{name}{comment}Exec=\"{binary}\" --no-ui --no-tray{plugin}
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
";

/// systemd user unit contents for the given binary (without bundle plugins).
pub fn autostart_unit_content(binary: &str) -> String {
    autostart_unit_content_with(binary, None)
}

/// XDG autostart `.desktop` contents for the given binary.
pub fn autostart_desktop_content(binary: &str) -> String {
    autostart_desktop_content_with(binary, None)
}

/// `ExecStart`/`Exec` tail with the bundle plugins directory (see
/// [`engine_launch_args`]); without a bundle — empty, the unit line is the
/// same as before the bundle existed.
fn plugin_tail(plugin_root: Option<&str>) -> String {
    match plugin_root {
        Some(root) => format!(" {PLUGIN_FLAG} \"{}\"", root.trim_end_matches('/')),
        None => String::new(),
    }
}

/// [`autostart_unit_content`] with the bundle plugins directory: the service
/// brought up at login must see the Open Wallpaper Engine plugin from the
/// package just like a daemon started from the window.
pub fn autostart_unit_content_with(binary: &str, plugin_root: Option<&str>) -> String {
    AUTOSTART_UNIT_TMPL
        .replace("{bus}", DBUS_SERVICE)
        .replace("{binary}", binary)
        .replace("{plugin}", &plugin_tail(plugin_root))
}

/// [`autostart_desktop_content`] with the bundle plugins directory.
pub fn autostart_desktop_content_with(binary: &str, plugin_root: Option<&str>) -> String {
    use crate::system_l10n::desktop_key as key;
    // Language lines first (they have no `{…}`), then the path — a path with
    // curly braces must not hit the template's placeholders.
    AUTOSTART_DESKTOP_TMPL
        .replace("{name}", &key("Name", AUTOSTART_NAME))
        .replace("{comment}", &key("Comment", AUTOSTART_COMMENT))
        .replace("{binary}", binary)
        .replace("{plugin}", &plugin_tail(plugin_root))
}

/// Root of user configs: `$XDG_CONFIG_HOME` or `~/.config`.
/// Re-export of the core's single point — [`crate::utils::config_home`].
pub fn config_home(xdg_config_home: Option<&str>, home: &str) -> String {
    crate::utils::config_home(xdg_config_home, home)
}

/// Path of the systemd user unit.
///
/// ⚠️ Intentional divergence from Python (a fix, see the module header):
/// `XDG_CONFIG_HOME` is respected — otherwise with a non-standard value the
/// unit is written where systemd does not look for it, and `enable` fails.
pub fn autostart_unit_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/systemd/user/{AUTOSTART_UNIT}")
}

/// Path of the XDG autostart `.desktop`.
pub fn autostart_desktop_path(
    xdg_config_home: Option<&str>,
    home: &str,
) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/autostart/{AUTOSTART_DESKTOP}")
}

/// Directory of the live systemd user manager — port of the condition
/// `os.path.isdir(f"/run/user/{uid}/systemd")`.
pub fn systemd_runtime_dir(uid: u32) -> String {
    format!("/run/user/{uid}/systemd")
}

/// Whether a live systemd user manager exists (otherwise — the XDG fallback).
/// Observations: `shutil.which("systemctl") is not None` and the presence of
/// [`systemd_runtime_dir`].
pub fn systemd_user_available(
    systemctl_present: bool,
    runtime_dir_exists: bool,
) -> bool {
    systemctl_present && runtime_dir_exists
}

/// argv `systemctl --user <args…>` — port of `_systemctl_user`.
pub fn systemctl_user_argv(args: &[&str]) -> Vec<String> {
    let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    argv
}

/// Timeout of `systemctl --user` calls, seconds.
pub const SYSTEMCTL_TIMEOUT_SECS: u64 = 15;

/// argv of the «is the unit enabled» check.
pub fn autostart_is_enabled_argv() -> Vec<String> {
    systemctl_user_argv(&["is-enabled", AUTOSTART_UNIT])
}

/// argv `daemon-reload`.
pub fn autostart_daemon_reload_argv() -> Vec<String> {
    systemctl_user_argv(&["daemon-reload"])
}

/// argv for enabling the unit.
pub fn autostart_enable_argv() -> Vec<String> {
    systemctl_user_argv(&["enable", AUTOSTART_UNIT])
}

/// Command for configs of compositors that do not bring up
/// `graphical-session.target` (sway without sway-systemd, Hyprland without
/// uwsm, river, labwc, Wayfire): an enabled unit does not start by itself at
/// login there, and the config line starts it explicitly
/// (`crate::compositor_autostart`). A repeated `start` of an already running
/// service does nothing — there will not be two engines. Session variables
/// are passed to the manager before the start: without `WAYLAND_DISPLAY` the
/// engine has nothing to connect to, and compositors import them on a line of
/// their own, racing with ours.
pub fn autostart_unit_start_argv() -> Vec<String> {
    vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP 2>/dev/null; \
             exec systemctl --user start {AUTOSTART_UNIT}"
        ),
    ]
}

/// argv for disabling the unit.
pub fn autostart_disable_argv() -> Vec<String> {
    systemctl_user_argv(&["disable", AUTOSTART_UNIT])
}

/// Port of `autostart_enabled`: a verdict from observations.
///
/// `is_enabled` — result of `systemctl --user is-enabled` (exit code and
/// stdout) or `None` if systemd is unavailable or the call failed (in Python —
/// a caught `OSError`/`SubprocessError`). `desktop_exists` — whether the XDG
/// autostart `.desktop` exists.
pub fn autostart_enabled(
    is_enabled: Option<(i32, &str)>,
    desktop_exists: bool,
) -> bool {
    if let Some((code, stdout)) = is_enabled {
        if code == 0 && stdout.trim() == "enabled" {
            return true;
        }
    }
    desktop_exists
}

/// Plan for disabling autostart — port of the `not enabled` branch in
/// `_set_autostart_inner`. BOTH mechanisms are removed (leave no leftovers
/// after the system changes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisablePlan {
    /// argv `systemctl --user disable` — only if systemd is alive AND the unit exists.
    pub disable_argv: Option<Vec<String>>,
    /// Unit file to delete (absence is not an error).
    pub remove_unit: String,
    /// argv `daemon-reload` — run only if the unit was REALLY deleted.
    pub daemon_reload_argv: Option<Vec<String>>,
    /// `.desktop` to delete (absence is not an error).
    pub remove_desktop: String,
    /// Final message (`ok = true`).
    pub message: String,
}

/// Build a [`DisablePlan`].
pub fn disable_plan(
    xdg_config_home: Option<&str>,
    home: &str,
    systemd_available: bool,
    unit_exists: bool,
) -> DisablePlan {
    DisablePlan {
        disable_argv: if systemd_available && unit_exists {
            Some(autostart_disable_argv())
        } else {
            None
        },
        remove_unit: autostart_unit_path(xdg_config_home, home),
        daemon_reload_argv: if systemd_available {
            Some(autostart_daemon_reload_argv())
        } else {
            None
        },
        remove_desktop: autostart_desktop_path(xdg_config_home, home),
        message: crate::i18n::tr("Autostart disabled"),
    }
}

/// Plan for enabling autostart — port of the `enabled` branch in `_set_autostart_inner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnablePlan {
    /// Engine binary not found — nothing to configure.
    NoBinary { message: String },
    /// Main path: systemd user unit.
    Systemd {
        unit_path: String,
        unit_content: String,
        daemon_reload_argv: Vec<String>,
        enable_argv: Vec<String>,
        /// We do not keep the second mechanism in parallel.
        remove_desktop: String,
        message: String,
    },
    /// Fallback for systems without systemd.
    Xdg { desktop_path: String, desktop_content: String, message: String },
}

/// Build an [`EnablePlan`]. `binary` — result of [`find_daemon_binary`]
/// (an ABSOLUTE path; enabling again simply overwrites the file).
pub fn enable_plan(
    binary: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
    systemd_available: bool,
) -> EnablePlan {
    enable_plan_with(binary, xdg_config_home, home, systemd_available, None)
}

/// [`enable_plan`] with the bundle plugins directory (see [`engine_launch_args`]).
pub fn enable_plan_with(
    binary: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
    systemd_available: bool,
    plugin_root: Option<&str>,
) -> EnablePlan {
    let Some(binary) = binary else {
        return EnablePlan::NoBinary {
            message: crate::i18n::tr("The waywallen engine was not found — autostart is not set up."),
        };
    };
    if systemd_available {
        return EnablePlan::Systemd {
            unit_path: autostart_unit_path(xdg_config_home, home),
            unit_content: autostart_unit_content_with(binary, plugin_root),
            daemon_reload_argv: autostart_daemon_reload_argv(),
            enable_argv: autostart_enable_argv(),
            remove_desktop: autostart_desktop_path(xdg_config_home, home),
            message: crate::i18n::tr("Autostart enabled (systemd user unit)"),
        };
    }
    EnablePlan::Xdg {
        desktop_path: autostart_desktop_path(xdg_config_home, home),
        desktop_content: autostart_desktop_content_with(binary, plugin_root),
        message: crate::i18n::tr("Autostart enabled (XDG autostart)"),
    }
}

/// Whether Rexon can set up engine autostart on this machine →
/// `(supported, unavailable_reason)`.
///
/// The answer for the `supported`/`unavailableReason` properties of the
/// `W.AutostartGetQuery` request: the vendor settings page (waywallen since
/// #380) keeps the switch greyed out while `supported` is false, and shows the
/// reason as a hint next to the label. Rexon's autostart is local: a systemd
/// user unit, and without systemd — XDG autostart, which can always be
/// written. So only one thing can get in the way — the engine itself being
/// absent: then [`enable_plan_with`] answers `NoBinary`, and there is nothing
/// to enable. The reason is empty when `supported` is true — as upstream does.
pub fn autostart_availability(binary: Option<&str>) -> (bool, String) {
    match binary {
        Some(_) => (true, String::new()),
        None => (
            false,
            crate::i18n::tr(
                "The waywallen engine is not installed, so there is nothing to start at login.",
            ),
        ),
    }
}

/// Message about a failed `systemctl enable` — `stderr`, or `stdout` if it is
/// empty (both trimmed, port of `r.stderr.strip() or r.stdout.strip()`).
pub fn enable_failed_message(stderr: &str, stdout: &str) -> String {
    let s = stderr.trim();
    let text = if s.is_empty() { stdout.trim() } else { s };
    format!("systemctl enable: {text}")
}

/// Message of the outer `set_autostart` catch.
pub fn set_autostart_error_message(err: &str) -> String {
    crate::i18n::trf("Could not set up autostart: %1", &[&err])
}

/// The binary from the `ExecStart="…" --no-ui --no-tray` line of an already
/// written unit.
///
/// Needed for refreshing: the engine path was chosen by the user (or an
/// earlier install), and rewriting the unit must not replace it.
pub fn unit_binary(existing: &str) -> Option<String> {
    let line = existing
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("ExecStart="))?;
    let value = line["ExecStart=".len()..].trim();
    let path = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => value.split_whitespace().next()?,
    };
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// What to do with an autostart unit already on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitRefresh {
    /// No unit (autostart is not enabled) or `ExecStart` did not parse —
    /// nothing to touch.
    Skip,
    /// The contents match the current template.
    UpToDate,
    /// The unit is outdated: rewrite it and reload the systemd configuration.
    Rewrite {
        unit_path: String,
        unit_content: String,
        daemon_reload_argv: Vec<String>,
    },
}

/// Plan for refreshing a previously written autostart unit.
///
/// Why: the unit is written ONCE — when «Start at login» is switched on, and
/// then lives at the user's for years. Without this mechanism any template
/// fix (e.g. `KillMode=mixed`, bug #3 of the module header) only reaches those
/// who flipped the switch again. Here the template is rebuilt on the OLD
/// binary path ([`unit_binary`]) — the engine choice is kept, only the wiring
/// gets fixed.
///
/// `existing` — contents of the unit file (`None` if there is no file).
pub fn refresh_unit_plan(
    existing: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
) -> UnitRefresh {
    refresh_unit_plan_with(existing, xdg_config_home, home, None)
}

/// [`refresh_unit_plan`] with the bundle plugins directory.
///
/// The unit's binary is still not touched (the user or an earlier install
/// chose it), while the `--plugin` tail follows the bundle: the package got
/// installed — the service starts seeing its plugin; removed — the flag goes
/// away, and the daemon does not look for a directory that no longer exists.
pub fn refresh_unit_plan_with(
    existing: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
    plugin_root: Option<&str>,
) -> UnitRefresh {
    let Some(binary) = existing.and_then(unit_binary) else {
        return UnitRefresh::Skip;
    };
    refresh_unit_plan_on(existing, &binary, xdg_config_home, home, plugin_root)
}

/// The binary to rebuild the unit on: the one Rexon would launch now
/// ([`find_daemon_binary`]); nothing found — the previous one (a unit without
/// an engine will not come up anyway, and we do not overwrite someone else's
/// choice).
///
/// Why: on 30.10.2026 the Rexon package stopped bundling the engine
/// (`/usr/lib/rexon/engine/waywallen.AppImage` went away with the update), and
/// a unit written to that path would point into nowhere after the package
/// update — wallpapers would vanish from the next login.
///
/// ⚠️ «The recorded one still lies on disk» does NOT justify the unit: on
/// 08.11.2026 the live unit pointed at the managed AppImage
/// `~/.local/share/waywallen/` from 23.10 (byte-identical to the packaged one
/// of the time), while Rexon was installed as a package with the engine in
/// `/usr/lib/rexon/engine/`. The window launches the package engine, the
/// login service — the old copy, and after the next package update the old
/// daemon would have come up at login. A managed AppImage that is NEWER than
/// the packaged one is chosen by the daemon search itself
/// ([`prefer_managed_over_bundled`]) — so the unit keeps it.
pub fn unit_refresh_binary(written: &str, found: Option<&str>) -> String {
    match found {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => written.to_string(),
    }
}

/// [`refresh_unit_plan_with`] on an explicitly chosen binary
/// ([`unit_refresh_binary`]); `existing` — contents of the unit.
pub fn refresh_unit_plan_on(
    existing: Option<&str>,
    binary: &str,
    xdg_config_home: Option<&str>,
    home: &str,
    plugin_root: Option<&str>,
) -> UnitRefresh {
    let Some(existing) = existing else {
        return UnitRefresh::Skip;
    };
    let unit_content = autostart_unit_content_with(binary, plugin_root);
    if unit_content == existing {
        return UnitRefresh::UpToDate;
    }
    UnitRefresh::Rewrite {
        unit_path: autostart_unit_path(xdg_config_home, home),
        unit_content,
        daemon_reload_argv: autostart_daemon_reload_argv(),
    }
}

// ============================================================================
// 3. DaemonControl decisions
// ============================================================================

/// Daemon status — port of the `DaemonStatus` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub version: String,
    pub ws_port: i64,
    pub current_wallpaper_id: String,
}

/// Port of `DaemonControl.probe` — a synchronous poll of the daemon state.
///
/// Properties are read through the already existing trait [`WaywallenIo`]
/// (`WsPort` / `Version` / `CurrentWallpaperId` are not duplicated — rule #12).
/// A missing `WsPort` ([`DbusValue::None`], in Python —
/// `reply.type() != ReplyMessage` or empty arguments) means «the daemon does
/// not answer».
///
/// ⚠️ Intentional divergence: a non-numeric `WsPort` gives `0` (Python crashed
/// with an unhandled `ValueError`).
pub fn probe<IO: WaywallenIo>(io: &mut IO) -> DaemonStatus {
    let ws_port = io.get_property("WsPort");
    if ws_port == DbusValue::None {
        return DaemonStatus::default();
    }
    DaemonStatus {
        running: true,
        ws_port: ws_port.py_int().unwrap_or(0),
        version: str_or_empty(io.get_property("Version")),
        current_wallpaper_id: str_or_empty(
            io.get_property("CurrentWallpaperId"),
        ),
    }
}

/// Port of `str(value or "")`.
fn str_or_empty(v: DbusValue) -> String {
    if v.is_truthy() { v.py_str() } else { String::new() }
}

/// argv of the detached daemon launch: `(program, args)`. The daemon must
/// survive Rexon closing — the window launches it as a separate session
/// (`we_socket::spawn_engine`).
pub fn daemon_launch_argv(binary: &str) -> (String, Vec<String>) {
    daemon_launch_argv_with(binary, None)
}

/// [`daemon_launch_argv`] with the bundle plugins directory.
pub fn daemon_launch_argv_with(binary: &str, plugin_root: Option<&str>) -> (String, Vec<String>) {
    (binary.to_string(), engine_launch_args(plugin_root))
}

/// The «binary not found» message from `ensure_running`.
pub fn daemon_not_found_message() -> String {
    format!(
        "Движок waywallen не найден. Ожидается вшитый AppImage (rexon/vendor/), \
         пакет дистрибутива ({SYSTEM_DAEMON}), движок пакета ({ENGINE_DIR}/{ENGINE_APPIMAGE_NAME}), \
         ~/.local/share/waywallen/waywallen.AppImage, в PATH или \
         ~/.local/bin/waywallen."
    )
}

/// Message about a failed detached launch.
pub fn daemon_launch_failed_message(binary: &str) -> String {
    format!("Не удалось запустить {binary}")
}

/// Polling timeout message.
pub fn daemon_poll_timeout_message() -> String {
    "Демон waywallen не поднялся за 12 секунд.".to_string()
}

/// The «transport layer is not built» message from `ensure_running`.
///
/// The text lived as a literal in `client.py` — the only client message that
/// was not asked of the core. One label — one place to edit (rule #12),
/// otherwise the next rename of the build script would drift apart from the
/// other such strings of the tab.
pub fn transport_missing_message() -> String {
    crate::i18n::tr(
        "The wallpaper engine module is not built: build it with tools/build_qml_control.sh",
    )
}

/// Message about a server frame that did not parse (protocol version mismatch).
pub fn frame_parse_error_message(err: &str) -> String {
    crate::i18n::trf("Could not parse a server frame: %1", &[&err])
}

/// Whether to show the next transport error (socket, broken frame) as a toast.
///
/// While there is no daemon, the transport tries to reconnect again and
/// again, and every attempt gives the same socket error. A toast for each is
/// a wall of identical messages a person can neither read nor close (rule
/// #13). So one toast per «connection lost»; the next one — only when the
/// connection was restored and lost again, or when the error became
/// DIFFERENT (a new cause is new knowledge for the person).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TransportErrorToast {
    /// Text of the last toast shown during the current outage; `None` — no
    /// toasts since the last connection.
    shown: Option<String>,
}

impl TransportErrorToast {
    /// An error arrived. `true` — show it as a toast.
    pub fn on_error(&mut self, text: &str) -> bool {
        if self.shown.as_deref() == Some(text) {
            return false;
        }
        self.shown = Some(text.to_owned());
        true
    }

    /// Connection established: the next error is already a new outage.
    pub fn on_connected(&mut self) {
        self.shown = None;
    }
}

/// The `ensure_running` decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnsureAction {
    /// The daemon is alive → `ready(status)` right away.
    Ready { status: DaemonStatus },
    /// Binary not found → `failed(message)`.
    NoBinary { message: String },
    /// Autostart + D-Bus polling.
    Launch {
        program: String,
        args: Vec<String>,
        polls_left: i64,
        poll_interval_ms: u64,
    },
}

impl EnsureAction {
    /// Whether to quietly install+select the Plasma plugin `org.waywallen.kde`
    /// before the action (best-effort, KDE-only).
    ///
    /// ONLY with a live/launchable engine: without it the containment plugin
    /// would replace the user's wallpaper with a black screen.
    pub fn ensure_kde_plugin(&self) -> bool {
        !matches!(self, EnsureAction::NoBinary { .. })
    }
}

/// Port of `DaemonControl.ensure_running` (without side effects).
///
/// The daemon counts as alive when `running && ws_port != 0` — like
/// `st.running and st.ws_port` in Python.
pub fn ensure_running_decision(
    status: &DaemonStatus,
    binary: Option<&str>,
) -> EnsureAction {
    ensure_running_decision_with(status, binary, None)
}

/// [`ensure_running_decision`] with the bundle plugins directory.
pub fn ensure_running_decision_with(
    status: &DaemonStatus,
    binary: Option<&str>,
    plugin_root: Option<&str>,
) -> EnsureAction {
    if status.running && status.ws_port != 0 {
        return EnsureAction::Ready { status: status.clone() };
    }
    match binary {
        None => EnsureAction::NoBinary { message: daemon_not_found_message() },
        Some(bin) => {
            let (program, args) = daemon_launch_argv_with(bin, plugin_root);
            EnsureAction::Launch {
                program,
                args,
                polls_left: POLL_LIMIT,
                poll_interval_ms: POLL_MS,
            }
        }
    }
}

/// [`ensure_running_decision_with`] when it is known whether an autostart
/// service exists (`service` — the [`AUTOSTART_UNIT`] unit is written and the
/// user's systemd is alive).
///
/// If there is a service, the daemon is brought up THROUGH it
/// (`systemctl --user start`): that way it is the only one and supervised by
/// the service, with its `--plugin` and `KillMode`. Previously the tab
/// launched a second, separate process bypassing the service, and the
/// service then ran into someone else's daemon at startup (19.10.2026). An
/// already running daemon — whoever's — is not touched.
pub fn ensure_running_decision_service(
    status: &DaemonStatus,
    binary: Option<&str>,
    plugin_root: Option<&str>,
    service: bool,
) -> EnsureAction {
    let action = ensure_running_decision_with(status, binary, plugin_root);
    match action {
        // Session variables go to the manager before the start (like the
        // compositors' autostart line): without `WAYLAND_DISPLAY` the service's
        // daemon has nothing to connect to, if the session did not pass them itself.
        EnsureAction::Launch { polls_left, poll_interval_ms, .. } if service => {
            let mut argv = autostart_unit_start_argv();
            let program = argv.remove(0);
            EnsureAction::Launch { program, args: argv, polls_left, poll_interval_ms }
        }
        other => other,
    }
}

/// Decision of one polling tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PollAction {
    /// The daemon came up → stop the timer, `ready(status)`.
    Ready { status: DaemonStatus },
    /// Keep waiting; how many attempts are left.
    Continue { polls_left: i64 },
    /// Attempts ran out → stop the timer, `failed(message)`.
    Timeout { message: String },
}

/// Port of `DaemonControl._poll_once`.
pub fn poll_decision(status: &DaemonStatus, polls_left: i64) -> PollAction {
    if status.running && status.ws_port != 0 {
        return PollAction::Ready { status: status.clone() };
    }
    let left = polls_left - 1;
    if left <= 0 {
        return PollAction::Timeout { message: daemon_poll_timeout_message() };
    }
    PollAction::Continue { polls_left: left }
}

// ============================================================================
// Engine launch watcher: wait while the process is alive and working
// ============================================================================
//
// ⛔ Defect 30.10.2026 (VM, package r1895): the first opening of the tab
// right after installing the package ended in the «the engine is taking too
// long to start» screen — [`poll_decision`] gave up exactly after
// 24 × 500 ms, although the engine was honestly starting: the first AppImage
// launch on a slow machine (mounting and unpacking the image, DB migrations)
// takes longer than 12 s. «Retry launch» opened everything right away. The
// customer saw an error where there was none (rule #13).
//
// Now there are three deadlines instead of one, and the watcher judges by
// the PROCESS, not by the clock:
//   * for the first [`START_QUIET_MS`] — simply wait for the bus, as before;
//   * then wait while the process is alive and shows signs of life (CPU time
//     or I/O counters grow for it or its descendants); the screen shows
//     «starting…» with an explanation that the first launch can be long;
//   * failure — only if the process died, went quiet for [`START_STALL_MS`]
//     or the hard limit [`START_HARD_LIMIT_MS`] expired (it is looping).
// The process died — fail right away, not after 12 s: there is nothing to
// wait for.

/// Quiet launch window, ms: this long we wait for the bus without asking the
/// process anything. It is the former hard deadline ([`POLL_LIMIT`] ×
/// [`POLL_MS`]): a normal start fits in 3–5 s (measured in the header of
/// `we_window::CONNECT_TIMEOUT_MS`).
pub const START_QUIET_MS: u64 = POLL_LIMIT as u64 * POLL_MS;

/// How long the process may show no signs of life after the quiet window, ms,
/// before it is considered hung. If signs of life are not visible at all (no
/// `/proc`, the service does not answer) — the same deadline counts from the
/// launch.
pub const START_STALL_MS: u64 = 30_000;

/// Hard launch limit, ms: we do not wait longer even for a working process —
/// this catches a loop where the CPU is busy but there is no bus.
pub const START_HARD_LIMIT_MS: u64 = 180_000;

/// Snapshot of the launched engine's signs of life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineLife {
    /// The process is alive (for a service — the unit is starting or running).
    pub alive: bool,
    /// Work counter of the process tree: CPU time in ticks plus I/O bytes. What
    /// matters is not the value but whether it changes.
    pub activity: u64,
    /// How many times the service restarted the engine (`NRestarts`); for a
    /// process outside a service — 0. Growth during the launch means the engine
    /// crashed.
    pub restarts: u32,
}

/// Decision of one launch watcher tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartStep {
    /// The engine is on the bus and listening on the port — the launch is over.
    Ready { status: DaemonStatus },
    /// Keep waiting. `slow` — the quiet window is behind: time to tell the person
    /// that the first launch can be long.
    Wait { slow: bool },
    /// The launch failed; `message` — the RAW cause for the log (the label for
    /// the person is assembled by `we_window::no_daemon_text_in`).
    Failed { message: String },
}

/// Engine launch watcher. Ticks at the bus polling step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartWatch {
    elapsed_ms: u64,
    idle_ms: u64,
    last_activity: Option<u64>,
    seen_alive: bool,
    first_restarts: Option<u32>,
}

impl StartWatch {
    /// `started` — the process was definitely launched by us (its PID is known).
    /// A service (`systemctl --user start`) does not bring the engine up right
    /// away, and until the unit has moved to «activating», «dead» does not mean
    /// «died».
    pub fn new(started: bool) -> Self {
        Self { seen_alive: started, ..Self::default() }
    }

    /// How long we have been waiting already, ms.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// Next tick: `status` — the bus, `life` — the process's signs of life
    /// (`None` — could not be learned), `step_ms` — the step since the last tick.
    pub fn tick(&mut self, status: &DaemonStatus, life: Option<EngineLife>, step_ms: u64) -> StartStep {
        if status.running && status.ws_port != 0 {
            return StartStep::Ready { status: status.clone() };
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(step_ms);
        match life {
            Some(life) => {
                let first = *self.first_restarts.get_or_insert(life.restarts);
                let restarted = life.restarts > first;
                if life.alive && !restarted {
                    self.seen_alive = true;
                    if self.last_activity == Some(life.activity) {
                        self.idle_ms = self.idle_ms.saturating_add(step_ms);
                    } else {
                        self.last_activity = Some(life.activity);
                        self.idle_ms = 0;
                    }
                } else {
                    if self.seen_alive || restarted || self.elapsed_ms >= START_QUIET_MS {
                        return StartStep::Failed { message: daemon_exited_message() };
                    }
                    self.idle_ms = self.idle_ms.saturating_add(step_ms);
                }
            }
            None => self.idle_ms = self.idle_ms.saturating_add(step_ms),
        }
        let secs = self.elapsed_ms / 1000;
        if self.elapsed_ms >= START_HARD_LIMIT_MS {
            return StartStep::Failed { message: daemon_start_timeout_message(secs, false) };
        }
        if self.elapsed_ms >= START_QUIET_MS && self.idle_ms >= START_STALL_MS {
            return StartStep::Failed { message: daemon_start_timeout_message(secs, true) };
        }
        StartStep::Wait { slow: self.elapsed_ms >= START_QUIET_MS }
    }
}

/// Raw cause: the engine process exited without getting in touch.
pub fn daemon_exited_message() -> String {
    "Процесс движка waywallen завершился, не выйдя на связь.".to_string()
}

/// Start of the raw cause «did not get in touch in time» — by it
/// `we_window::daemon_failure` recognizes the failure.
pub const START_TIMEOUT_HEAD: &str = "Демон waywallen не вышел на связь за";

/// Raw cause of the launch watcher timeout. `stalled` — the process went
/// quiet; otherwise the hard limit expired.
pub fn daemon_start_timeout_message(secs: u64, stalled: bool) -> String {
    let why = if stalled {
        "процесс перестал подавать признаки жизни"
    } else {
        "истёк предельный срок запуска"
    };
    format!("{START_TIMEOUT_HEAD} {secs} с: {why}.")
}

// ── What to tell the log when the engine died at launch ─────────────────────
//
// ⏳ Case 30.10.2026 (VM, clean profile, engine from the system package): the
// first launch ended in «the process exited without getting in touch»,
// repeated ones worked. No logs were left, and ours would have had nothing
// to read: the daemon was launched with `QProcess::startDetached` — the exit
// code of a foreign descendant cannot be learned, and its stderr silently
// went into rexon.log mixed with our lines (the window redirects its
// descriptor 2 there, the daemon inherited it).
//
// What is known from the daemon v0.5.0 source (`daemon/bootstrap.rs`,
// `system/dbus.rs`): it grabs the bus name BEFORE the database, the plugin
// scan and publishing the object; a second instance with the name taken and
// `--no-ui` prints «already running, no UI to launch» and exits with code 0
// before opening the port. The bus probe (`WsPort`) answers «not running»
// all that time. The other early exits are code 1 and an `Error: …` line in
// stderr (output socket, database, plugin translations). Hence three facts
// that tell the causes apart: the exit code, the tail of the daemon's stderr
// and who holds the bus name at the moment of failure.

/// Output file of the daemon launched by Rexon (stdout and stderr) — next to
/// rexon.log. Overwritten on every launch: the daemon's history is in its own
/// log anyway (`~/.local/state/waywallen/logs`).
pub const DAEMON_OUTPUT_LOG: &str = "waywallen-daemon.log";

/// How many last lines of daemon output to put into the log on failure.
pub const DAEMON_OUTPUT_TAIL_LINES: usize = 15;

/// Path of the daemon output file in the log directory `log_dir`.
pub fn daemon_output_log_path(log_dir: &str) -> String {
    format!("{}/{DAEMON_OUTPUT_LOG}", log_dir.trim_end_matches('/'))
}

/// The last `max` non-empty lines of text (without trailing whitespace).
pub fn text_tail(text: &str, max: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
    let from = lines.len().saturating_sub(max);
    lines[from..].iter().map(|l| l.to_string()).collect()
}

/// argv «whose process holds the daemon's bus name» (`GetConnectionUnixProcessID`).
pub fn daemon_name_owner_pid_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "call",
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixProcessID",
        "s",
        crate::waywallen_client::BUS_NAME,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Answer of `busctl call … GetConnectionUnixProcessID`: `u 1234` → `1234`.
pub fn parse_name_owner_pid(stdout: &str) -> Option<u32> {
    let mut it = stdout.split_whitespace();
    (it.next()? == "u").then_some(())?;
    it.next()?.parse().ok().filter(|p| *p != 0)
}

/// How the engine process ended: a code or a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineExit {
    /// Still alive (failure by deadline) or could not be learned.
    Unknown,
    Code(i32),
    Signal(i32),
}

/// Owner of the daemon's bus name at the moment of failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusNameOwner {
    /// The name is free.
    Free,
    /// Held by process `pid` with command line `cmdline` (empty — could not be read).
    Process { pid: u32, cmdline: String },
}

/// Log lines about a launch failure: exit code, bus name owner and the tail
/// of the daemon's output. Log only — this is not shown to the person.
///
/// Code 0 with a foreign name owner is almost certainly a second instance:
/// someone else was bringing the engine up (a service, another window, a
/// desktop plugin), and our process yielded the name to it.
pub fn daemon_exit_report(
    pid: u32,
    exit: EngineExit,
    owner: Option<&BusNameOwner>,
    output_path: &str,
    tail: &[String],
) -> Vec<String> {
    let mut out = Vec::new();
    let how = match exit {
        EngineExit::Unknown => "процесс жив или код выхода неизвестен".to_string(),
        EngineExit::Code(c) => format!("код выхода {c}"),
        EngineExit::Signal(s) => format!("убит сигналом {s}"),
    };
    out.push(format!("движок pid {pid}: {how}"));
    match owner {
        None => out.push("имя шины демона: узнать не удалось".to_string()),
        Some(BusNameOwner::Free) => out.push("имя шины демона свободно".to_string()),
        Some(BusNameOwner::Process { pid: owner_pid, cmdline }) => {
            let who = if cmdline.is_empty() { "?" } else { cmdline.as_str() };
            let note = if *owner_pid != pid && exit == EngineExit::Code(0) {
                " — похоже, движок уже поднимал другой процесс, а наш уступил ему имя"
            } else {
                ""
            };
            out.push(format!("имя шины демона держит pid {owner_pid} ({who}){note}"));
        }
    }
    if tail.is_empty() {
        out.push(format!("вывод демона пуст ({output_path})"));
    } else {
        out.push(format!("хвост вывода демона ({output_path}):"));
        out.extend(tail.iter().map(|l| format!("  | {l}")));
    }
    out
}

/// One line of `/proc/<pid>/stat`: `(pid, ppid, state, utime+stime)`.
///
/// The process name in parentheses may contain spaces and parentheses — we
/// cut at the last `)`. Then fields from the third: state (3), ppid (4) …
/// utime (14), stime (15).
pub fn parse_proc_stat(text: &str) -> Option<(u32, u32, char, u64)> {
    let open = text.find('(')?;
    let pid = text[..open].trim().parse().ok()?;
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let state = f.first()?.chars().next()?;
    let ppid = f.get(1)?.parse().ok()?;
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    Some((pid, ppid, state, utime.saturating_add(stime)))
}

/// I/O bytes from `/proc/<pid>/io` (`rchar` + `wchar`). They are also counted
/// when reading from the image mount — that is, they grow while the AppImage
/// unpacks, even if the CPU is waiting for the disk.
pub fn parse_proc_io(text: &str) -> u64 {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            matches!(key.trim(), "rchar" | "wchar").then(|| value.trim().parse::<u64>().ok())?
        })
        .fold(0u64, u64::saturating_add)
}

/// A process from `/proc` for [`tree_life`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    /// State from `stat` (`R`, `S`, `Z` …).
    pub state: char,
    /// CPU time + I/O bytes.
    pub activity: u64,
}

/// Signs of life of the process tree rooted at `root`: whether the root is
/// alive (a zombie no longer is) and the sum of work of the root and all its
/// descendants. Descendants are needed because with an AppImage a separate
/// process serves the image.
pub fn tree_life(root: u32, entries: &[ProcEntry]) -> EngineLife {
    let alive = entries
        .iter()
        .any(|e| e.pid == root && e.state != 'Z' && e.state != 'X');
    let mut tree = vec![root];
    let mut activity = 0u64;
    let mut i = 0;
    while i < tree.len() {
        let pid = tree[i];
        for e in entries {
            if e.pid == pid {
                activity = activity.saturating_add(e.activity);
            }
            if e.ppid == pid && e.pid != pid && !tree.contains(&e.pid) {
                tree.push(e.pid);
            }
        }
        i += 1;
    }
    EngineLife { alive, activity, restarts: 0 }
}

/// Collect the user's processes from `/proc` (one's own `io` is readable without privileges).
pub fn read_proc_entries() -> Vec<ProcEntry> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let pid: u32 = name.to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (pid, ppid, state, cpu) = parse_proc_stat(&stat)?;
            let io = std::fs::read_to_string(format!("/proc/{pid}/io"))
                .map(|t| parse_proc_io(&t))
                .unwrap_or(0);
            Some(ProcEntry { pid, ppid, state, activity: cpu.saturating_add(io) })
        })
        .collect()
}

/// Signs of life of process `pid` launched outside a service. `None` —
/// `/proc` is not available at all, and there is nothing to judge by.
pub fn read_process_life(pid: u32) -> Option<EngineLife> {
    let entries = read_proc_entries();
    if entries.is_empty() || pid == 0 {
        return None;
    }
    Some(tree_life(pid, &entries))
}

/// argv for querying the engine service: state, main PID and restart count.
pub fn engine_unit_show_argv() -> Vec<String> {
    systemctl_user_argv(&[
        "show",
        "--property=ActiveState,MainPID,NRestarts",
        AUTOSTART_UNIT,
    ])
}

/// Service state from the [`engine_unit_show_argv`] output: `(alive, main
/// PID, restarts)`. «Alive» — the unit is starting, running or waiting for a
/// restart (`activating`), but not `failed`/`inactive`.
pub fn parse_engine_unit_show(stdout: &str) -> Option<(bool, u32, u32)> {
    let mut state = None;
    let mut pid = 0;
    let mut restarts = 0;
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "ActiveState" => state = Some(value.to_string()),
            "MainPID" => pid = value.parse().unwrap_or(0),
            "NRestarts" => restarts = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    let state = state?;
    let alive = matches!(state.as_str(), "activating" | "active" | "reloading");
    Some((alive, pid, restarts))
}

/// Signs of life of the engine under a service: the unit state plus the work
/// of its main process tree. `unit_show` — output of [`engine_unit_show_argv`].
pub fn service_life(unit_show: &str, entries: &[ProcEntry]) -> Option<EngineLife> {
    let (alive, pid, restarts) = parse_engine_unit_show(unit_show)?;
    let activity = if pid != 0 { tree_life(pid, entries).activity } else { 0 };
    Some(EngineLife { alive, activity, restarts })
}

/// Port of `dbus_quick`: `str(args[0]) if args else ""`.
pub fn dbus_quick_result(arguments: &[DbusValue]) -> String {
    match arguments.first() {
        Some(v) => v.py_str(),
        None => String::new(),
    }
}

/// A `ListTasks` entry — port of the tuple
/// `(id, kind, name, started_at_ms, state)`; D-Bus signature `a(tssxs)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonTask {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub started_at_ms: i64,
    pub state: String,
}

/// Port of parsing the demarshalled QtDBus `ListTasks` reply.
///
/// `None` — the row did not parse (not 5 elements / non-numeric id), in
/// Python that is `TypeError`/`ValueError` → fallback to `busctl`
/// ([`busctl_tasks_argv`] + [`parse_busctl_tasks`]).
pub fn tasks_from_dbus_rows(rows: &[Vec<DbusValue>]) -> Option<Vec<DaemonTask>> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() != 5 {
            return None;
        }
        out.push(DaemonTask {
            id: row[0].py_int()?,
            kind: row[1].py_str(),
            name: row[2].py_str(),
            started_at_ms: row[3].py_int()?,
            state: row[4].py_str(),
        });
    }
    Some(out)
}

/// argv of the `busctl --json=short` fallback (systemd exists on all target Arch systems).
pub fn busctl_tasks_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "--json=short",
        "call",
        DBUS_SERVICE,
        DBUS_PATH,
        DBUS_IFACE,
        "ListTasks",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Port of `_list_tasks_busctl`: parsing `json.loads(stdout)["data"][0]`.
/// Any format error → an empty list (the original swallows everything with
/// `except Exception: return []`).
pub fn parse_busctl_tasks(stdout: &str) -> Vec<DaemonTask> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    let Some(rows) = v.get("data").and_then(|d| d.get(0)).and_then(|d| d.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(r) = r.as_array() else { return Vec::new() };
        if r.len() < 5 {
            return Vec::new();
        }
        let (Some(id), Some(started)) = (json_int(&r[0]), json_int(&r[3]))
        else {
            return Vec::new();
        };
        out.push(DaemonTask {
            id,
            kind: json_str(&r[1]),
            name: json_str(&r[2]),
            started_at_ms: started,
            state: json_str(&r[4]),
        });
    }
    out
}

/// `int(x)` over a JSON value: a number or a numeric string.
fn json_int(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        serde_json::Value::Bool(b) => Some(i64::from(*b)),
        serde_json::Value::String(s) => {
            DbusValue::Str(s.clone()).py_int()
        }
        _ => None,
    }
}

/// `str(x)` over a JSON value (in the busctl answer it is always a string).
fn json_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "None".to_string(),
        serde_json::Value::Bool(true) => "True".to_string(),
        serde_json::Value::Bool(false) => "False".to_string(),
        other => other.to_string(),
    }
}

// ── synchronous stop of active WE wallpapers (for the static installer) ─────

/// Result of `stop_active_wallpapers`: `n` active renderers were stopped.
pub const STOP_STOPPED: &str = "stopped";
/// The waywallen daemon is not running → no WE wallpapers, nothing to do.
pub const STOP_NOT_RUNNING: &str = "not_running";
/// No `QApplication` (headless/CLI) → cannot stop synchronously.
pub const STOP_NO_QT: &str = "no_qt";
/// The daemon exists, but stopping failed (timeout/WS drop).
pub const STOP_ERROR: &str = "error";

/// Default timeout of the whole operation, ms.
pub const STOP_TIMEOUT_MS: u64 = 4000;

/// Port of the early branches of `stop_active_wallpapers`: whether work is
/// possible at all. `None` — yes, go to the WS part; `Some((code, 0))` — this
/// result right away.
pub fn stop_active_gate(
    status: &DaemonStatus,
    qt_available: bool,
) -> Option<(&'static str, i64)> {
    if !status.running || status.ws_port == 0 {
        return Some((STOP_NOT_RUNNING, 0));
    }
    if !qt_available {
        return Some((STOP_NO_QT, 0));
    }
    None
}

/// Port of the final branch of `stop_active_wallpapers`.
pub fn stop_active_result(ok: bool, killed: i64) -> (&'static str, i64) {
    if ok { (STOP_STOPPED, killed) } else { (STOP_ERROR, 0) }
}

// ============================================================================
// 4. REQUEST codec
// ============================================================================

/// Handing out `request_id` — port of the `_next_id` field (numbering from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIds {
    next: u64,
}

impl Default for RequestIds {
    fn default() -> Self {
        RequestIds { next: 1 }
    }
}

impl RequestIds {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next `request_id` (and advancing the counter).
    pub fn alloc(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Current counter value (without advancing) — for comparison with the shim.
    pub fn peek(&self) -> u64 {
        self.next
    }
}

// Table of `oneof payload` field numbers of the `Request` message — needed
// by the generic [`request_bytes`] (the `call(field, msg)` path from
// `qml_ui/bridge.py`, where the payload arrives already serialized).
//
// ⚠️ The file is GENERATED by build.rs from the same `control.proto` as the
// [`crate::proto`] structs. The hand-written list here already lagged behind
// the vendor: upstream added fields 69–74 (`plugin_action`,
// `subscription_status`, `subscription_set`, `qr_login_cancel`,
// `remote_settings_patch`, `wallpaper_unsubscribe`), and the table did not
// know about them — those requests silently went to the Python fallback.
// Now a new field in `.proto` is picked up by itself.
include!(concat!(env!("OUT_DIR"), "/request_payload_fields.rs"));

/// Number of a `oneof payload` field by its name.
pub fn request_payload_field_number(field: &str) -> Option<u32> {
    REQUEST_PAYLOAD_FIELDS
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, num)| *num)
}

/// Assembling `Request{request_id, <field>: msg}` from an ALREADY serialized
/// payload — the generic path of `WaywallenClient.call(field, msg)`.
///
/// The bytes are assembled directly: `request_id` (field 1, varint, omitted
/// at 0 as the proto3 default), then a length-delimited payload with the
/// field number from [`REQUEST_PAYLOAD_FIELDS`]. Ascending order — the same as
/// `google.protobuf`.
pub fn request_bytes(
    request_id: u64,
    field: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let num = request_payload_field_number(field)
        .ok_or_else(|| format!("неизвестное поле Request: {field}"))?;
    let mut out = Vec::with_capacity(payload.len() + 16);
    if request_id != 0 {
        out.push(0x08); // field 1, varint
        put_varint(&mut out, request_id);
    }
    put_varint(&mut out, (u64::from(num) << 3) | 2);
    put_varint(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Writing a varint per the protobuf protocol.
fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Assemble a `Request` from a ready payload of a prost type.
fn encode(request_id: u64, payload: pb::request::Payload) -> Vec<u8> {
    pb::Request { request_id, payload: Some(payload) }.encode_to_vec()
}

// ── wrappers used by the tab's pages ────────────────────────────────────────

/// `health`.
pub fn health(request_id: u64) -> Vec<u8> {
    encode(request_id, pb::request::Payload::Health(pb::HealthRequest {}))
}

/// `wallpaper_list` parameters — quick filters of the Filters dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperListParams {
    pub search_text: String,
    /// `filter.proto`: `WallpaperSortKey`; defaults to
    /// `WALLPAPER_SORT_KEY_NAME` (1).
    pub sort_key: i32,
    pub sort_desc: bool,
    /// Exclude types.
    pub skip_types: Vec<String>,
    /// Only wallpapers with any of the tags (empty = all).
    pub filter_tags: Vec<String>,
    /// Exclude ratings.
    pub skip_content_ratings: Vec<String>,
}

impl Default for WallpaperListParams {
    fn default() -> Self {
        WallpaperListParams {
            search_text: String::new(),
            sort_key: pb::WallpaperSortKey::Name as i32,
            sort_desc: false,
            skip_types: Vec::new(),
            filter_tags: Vec::new(),
            skip_content_ratings: Vec::new(),
        }
    }
}

/// `wallpaper_list`. `page_size = 0` — all entries, no pagination.
pub fn wallpaper_list(request_id: u64, p: &WallpaperListParams) -> Vec<u8> {
    let direction = if p.sort_desc {
        pb::SortDirection::Desc
    } else {
        pb::SortDirection::Asc
    } as i32;
    let req = pb::WallpaperListRequest {
        page_size: 0,
        search_text: p.search_text.clone(),
        skip_types: p.skip_types.clone(),
        filter_tags: p.filter_tags.clone(),
        skip_content_ratings: p.skip_content_ratings.clone(),
        sorts: vec![pb::WallpaperSortRule { key: p.sort_key, direction }],
        ..Default::default()
    };
    encode(request_id, pb::request::Payload::WallpaperList(req))
}

/// `display_list`.
pub fn display_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::DisplayList(pb::DisplayListRequest {}),
    )
}

/// Apply targets from display numbers — `PresentationTarget` v0.3.5.
///
/// Since v0.3.5 field 5 of `WallpaperApplyRequest` (and fields 2/1 of playlist
/// activation and deactivation) changed type: it was
/// `repeated uint64 display_ids`, it became `repeated PresentationTarget
/// targets`. The FIELD NUMBER IS THE SAME, but the wire type differs (packed
/// varints vs nested messages), so frames of the two versions are
/// incompatible both ways and the daemon silently drops a foreign frame.
/// Hence the single point for building targets is here, so that display
/// numbers do not turn into a frame somewhere else in the old form.
///
/// Zeros are dropped: in the vendor a display id is a non-zero key, and `0`
/// comes from QML as «no value» (`presentationTargetsFromVariant`).
pub fn display_targets(display_ids: &[u64]) -> Vec<pb::PresentationTarget> {
    display_ids
        .iter()
        .filter(|id| **id != 0)
        .map(|id| pb::PresentationTarget {
            target: Some(pb::presentation_target::Target::DisplayId(*id)),
        })
        .collect()
}

/// A canvas target (`PresentationTarget.canvas_id`). An empty id is not a target.
pub fn canvas_target(canvas_id: &str) -> Option<pb::PresentationTarget> {
    if canvas_id.is_empty() {
        return None;
    }
    Some(pb::PresentationTarget {
        target: Some(pb::presentation_target::Target::CanvasId(
            canvas_id.to_string(),
        )),
    })
}

/// `wallpaper_apply`.
pub fn wallpaper_apply(
    request_id: u64,
    wallpaper_id: &str,
    targets: &[pb::PresentationTarget],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperApply(pb::WallpaperApplyRequest {
            wallpaper_id: wallpaper_id.to_string(),
            targets: targets.to_vec(),
            ..Default::default()
        }),
    )
}

/// `wallpaper_scan`.
pub fn wallpaper_scan(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperScan(pb::WallpaperScanRequest {}),
    )
}

/// `wallpaper_get` — a fresh snapshot of one wallpaper (only here do
/// `user_properties_schema` + `user_property_overrides` arrive).
pub fn wallpaper_get(request_id: u64, wallpaper_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperGet(pb::WallpaperGetRequest {
            wallpaper_id: wallpaper_id.to_string(),
        }),
    )
}

/// `wallpaper_property_set` — one user-property override. An empty `value` =
/// reset to the `project.json` default (a valid oneof value for the daemon).
pub fn wallpaper_property_set(
    request_id: u64,
    wallpaper_id: &str,
    key: &str,
    value: &str,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperPropertySet(
            pb::WallpaperPropertySetRequest {
                wallpaper_id: wallpaper_id.to_string(),
                key: key.to_string(),
                operation: Some(
                    pb::wallpaper_property_set_request::Operation::Value(
                        value.to_string(),
                    ),
                ),
            },
        ),
    )
}

// ── playlists ───────────────────────────────────────────────────────────────

/// `playlist_list`.
pub fn playlist_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistList(pb::PlaylistListRequest {}),
    )
}

/// `playlist_status`.
pub fn playlist_status(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistStatus(pb::PlaylistStatusRequest {}),
    )
}

/// `playlist_create`.
pub fn playlist_create(
    request_id: u64,
    name: &str,
    mode: i32,
    interval_secs: u32,
    entry_ids: &[String],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistCreate(pb::PlaylistCreateRequest {
            name: name.to_string(),
            mode,
            interval_secs,
            entry_ids: entry_ids.to_vec(),
            // The field appeared in protocol 0.3.7 (a shared wallpaper choice
            // for the playlist). `false` is exactly the current behaviour: in
            // proto3 a zero bool is not written to the wire, the frame stays
            // the same byte for byte. The value will come from the form once
            // the vendor QML 0.3.7 moves to it.
            synchronized_selection: false,
            // waywallen 0.5.0: playlist source (a curated list or a smart
            // selection). The daemon reads `None` as curated from
            // `entry_ids` — exactly the previous behaviour; Rexon does not
            // create smart playlists yet.
            source: None,
        }),
    )
}

/// `playlist_delete`.
pub fn playlist_delete(request_id: u64, playlist_id: i64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistDelete(pb::PlaylistDeleteRequest {
            id: playlist_id,
        }),
    )
}

/// `playlist_rename`.
pub fn playlist_rename(request_id: u64, playlist_id: i64, name: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistRename(pb::PlaylistRenameRequest {
            id: playlist_id,
            name: name.to_string(),
        }),
    )
}

/// `playlist_set_items`.
pub fn playlist_set_items(
    request_id: u64,
    playlist_id: i64,
    entry_ids: &[String],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistSetItems(pb::PlaylistSetItemsRequest {
            id: playlist_id,
            entry_ids: entry_ids.to_vec(),
            // Edit race guard from protocol 0.3.7. Zero = «do not check»,
            // i.e. the current behaviour; a zero field does not appear on
            // the wire at all.
            expected_revision: 0,
        }),
    )
}

/// `playlist_set_mode`.
pub fn playlist_set_mode(request_id: u64, playlist_id: i64, mode: i32) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistSetMode(pb::PlaylistSetModeRequest {
            id: playlist_id,
            mode,
        }),
    )
}

/// `playlist_set_interval`.
pub fn playlist_set_interval(
    request_id: u64,
    playlist_id: i64,
    interval_secs: u32,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistSetInterval(
            pb::PlaylistSetIntervalRequest { id: playlist_id, interval_secs },
        ),
    )
}

/// `playlist_activate`.
pub fn playlist_activate(
    request_id: u64,
    playlist_id: i64,
    targets: &[pb::PresentationTarget],
    auto_attach: bool,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistActivate(pb::PlaylistActivateRequest {
            id: playlist_id,
            targets: targets.to_vec(),
            auto_attach,
        }),
    )
}

/// `playlist_deactivate`.
pub fn playlist_deactivate(
    request_id: u64,
    targets: &[pb::PresentationTarget],
    clear_auto_attach: i64,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistDeactivate(
            pb::PlaylistDeactivateRequest {
                targets: targets.to_vec(),
                clear_auto_attach,
            },
        ),
    )
}

/// `playlist_jump_to`.
pub fn playlist_jump_to(
    request_id: u64,
    playlist_id: i64,
    entry_id: &str,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistJumpTo(pb::PlaylistJumpToRequest {
            id: playlist_id,
            entry_id: entry_id.to_string(),
        }),
    )
}

// ⚠️ playlist_export / playlist_import were REMOVED by the daemon in v0.2.3
// (Request fields 55/56 are reserved). There are no wrappers on purpose.

// ── displays ────────────────────────────────────────────────────────────────

/// `display_rename`.
///
/// ⚠️ `display_id` is mandatory for exactly the same reason as in
/// `display_layout_set` below: since v0.2.4 the daemon resolves the target by
/// number, and keeps `name` only for old clients. The previous wrapper did not
/// send the number at all (`..Default::default()`), and the rename would have
/// gone nowhere — fixed when moving the request to Rust (rule #16).
pub fn display_rename(
    request_id: u64,
    name: &str,
    display_id: u64,
    alias: &str,
    clear: bool,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::DisplayRename(pb::DisplayRenameRequest {
            name: name.to_string(),
            display_id,
            alias: alias.to_string(),
            clear,
        }),
    )
}

/// Sparse-override per-display layout — `display_layout_set` parameters.
///
/// Since v0.2.4 the target is resolved by `display_id` («name is retained for
/// older clients but is not used for target resolution», fix #79) — passing it
/// is MANDATORY; we send `name` too, for old daemons.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayLayoutParams {
    pub name: String,
    pub display_id: u64,
    pub fillmode: Option<i32>,
    pub align: Option<i32>,
    pub rotation: Option<i32>,
    /// `(x, y)` in percent 0..100, 50 = center.
    pub location: Option<(u32, u32)>,
    pub clear_fillmode: bool,
    pub clear_align: bool,
    pub clear_rotation: bool,
    pub clear_location: bool,
}

/// `display_layout_set`. The `override` submessage appears in the frame only
/// if at least one of `fillmode`/`align`/`rotation`/`location` is set (in
/// Python it is created by the very access `req.override.*`).
pub fn display_layout_set(request_id: u64, p: &DisplayLayoutParams) -> Vec<u8> {
    let touched = p.fillmode.is_some()
        || p.align.is_some()
        || p.rotation.is_some()
        || p.location.is_some();
    let over = touched.then(|| {
        let mut o = pb::LayoutOverride::default();
        if let Some(v) = p.fillmode {
            o.fillmode_set = true;
            o.fillmode = v;
        }
        if let Some(v) = p.align {
            o.align_set = true;
            o.align = v;
        }
        if let Some(v) = p.rotation {
            o.rotation_set = true;
            o.rotation = v;
        }
        if let Some((x, y)) = p.location {
            o.location_set = true;
            o.location_x = x;
            o.location_y = y;
        }
        o
    });
    encode(
        request_id,
        pb::request::Payload::DisplayLayoutSet(pb::DisplayLayoutSetRequest {
            name: p.name.clone(),
            r#override: over,
            clear_fillmode: p.clear_fillmode,
            clear_align: p.clear_align,
            clear_rotation: p.clear_rotation,
            display_id: p.display_id,
            clear_location: p.clear_location,
            // waywallen 0.4.4: mirroring (`flip`). We have no control for
            // it: `flip_set` in the override is false, we do not ask for a
            // reset — the daemon keeps whatever mirroring the display has.
            clear_flip: false,
        }),
    )
}

// ── status ──────────────────────────────────────────────────────────────────

/// `renderer_list`.
pub fn renderer_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RendererList(pb::RendererListRequest {}),
    )
}

/// `renderer_plugin_list`.
pub fn renderer_plugin_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RendererPluginList(
            pb::RendererPluginListRequest {},
        ),
    )
}

/// `renderer_kill`.
pub fn renderer_kill(request_id: u64, renderer_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RendererKill(pb::RendererKillRequest {
            renderer_id: renderer_id.to_string(),
        }),
    )
}

/// `gpu_list`.
pub fn gpu_list(request_id: u64) -> Vec<u8> {
    encode(request_id, pb::request::Payload::GpuList(pb::GpuListRequest {}))
}

/// `wallpaper_lookup` — wallpapers by known identifiers (the «Current
/// wallpaper» card). The daemon answers from its database, it does not query
/// sources.
pub fn wallpaper_lookup(request_id: u64, wallpaper_ids: &[String]) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperLookup(pb::WallpaperLookupRequest {
            wallpaper_ids: wallpaper_ids.to_vec(),
        }),
    )
}

// ── filters (tag/rating references) ─────────────────────────────────────────

/// `tag_list`.
pub fn tag_list(request_id: u64) -> Vec<u8> {
    encode(request_id, pb::request::Payload::TagList(pb::TagListRequest {}))
}

/// `content_rating_list`.
pub fn content_rating_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::ContentRatingList(pb::ContentRatingListRequest {}),
    )
}

// ── sources/libraries ───────────────────────────────────────────────────────

/// `source_list`.
pub fn source_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::SourceList(pb::SourceListRequest {}),
    )
}

/// `library_list`.
pub fn library_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryList(pb::LibraryListRequest {}),
    )
}

/// The library name as the waywallen 0.5.0 vendor interface sends it: spaces
/// at the edges trimmed, an empty one is not sent at all (`None` — the daemon
/// picks the default name). One rule for adding and renaming.
fn library_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Library settings values as the waywallen 0.6.0 vendor interface sends
/// them: the page's map (`QVariantMap`) → the request's
/// `map<string, string>`, each value is `QVariant::toString()`. An integer
/// that came from QML as a fraction (`3.0`) is written without the tail — like
/// Qt does; `null` and nested values give an empty string (`QVariant` prints
/// nothing for them either). Not a map — no settings.
pub fn library_settings(value: &serde_json::Value) -> HashMap<String, String> {
    use serde_json::Value;
    let Value::Object(map) = value else {
        return HashMap::new();
    };
    map.iter()
        .map(|(key, value)| {
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
                    (Some(i), _, _) => i.to_string(),
                    (_, Some(u), _) => u.to_string(),
                    (_, _, Some(f)) if f.fract() == 0.0 && f.abs() < 1e15 => {
                        (f as i64).to_string()
                    }
                    (_, _, Some(f)) => f.to_string(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            (key.clone(), text)
        })
        .collect()
}

/// Body of `library_add` — or `None` if the request must not go out: without
/// a path or without a plugin the original silently returns. The name is per
/// [`library_name`] (waywallen 0.5.0, optional field
/// `LibraryAddRequest.name`); library settings (0.6.0, the plugin's
/// `librarySettings`) — as is.
pub fn library_add_request(
    path: &str,
    plugin_name: &str,
    name: &str,
    settings: HashMap<String, String>,
) -> Option<pb::LibraryAddRequest> {
    if path.is_empty() || plugin_name.is_empty() {
        return None;
    }
    Some(pb::LibraryAddRequest {
        path: path.to_string(),
        plugin_name: plugin_name.to_string(),
        name: library_name(name),
        settings,
    })
}

/// `library_add`. An empty `name` — the default name from the daemon, as
/// before the field appeared in 0.5.0.
pub fn library_add(request_id: u64, path: &str, plugin_name: &str, name: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryAdd(pb::LibraryAddRequest {
            path: path.to_string(),
            plugin_name: plugin_name.to_string(),
            name: library_name(name),
            settings: HashMap::new(),
        }),
    )
}

/// Body of `library_edit` (waywallen 0.6.0: the library name and settings in
/// one request; the vendor interface no longer sends the former
/// `library_rename`) — or `None`: the number is not positive or the name is
/// empty after trimming spaces, then the original silently returns.
pub fn library_edit_request(
    library_id: i64,
    name: &str,
    settings: HashMap<String, String>,
) -> Option<pb::LibraryEditRequest> {
    if library_id <= 0 {
        return None;
    }
    Some(pb::LibraryEditRequest {
        id: library_id,
        name: library_name(name)?,
        settings,
    })
}

/// `library_edit` — the new name and settings of the library.
pub fn library_edit(
    request_id: u64,
    library_id: i64,
    name: &str,
    settings: HashMap<String, String>,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryEdit(pb::LibraryEditRequest {
            id: library_id,
            name: name.trim().to_string(),
            settings,
        }),
    )
}

/// `library_remove`.
pub fn library_remove(request_id: u64, library_id: i64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryRemove(pb::LibraryRemoveRequest {
            id: library_id,
        }),
    )
}

/// `library_auto_detect` — source plugins themselves look for libraries in
/// typical places (Pictures, Steam workshop) and register what they find.
pub fn library_auto_detect(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryAutoDetect(pb::LibraryAutoDetectRequest {}),
    )
}

// ── daemon settings ─────────────────────────────────────────────────────────

/// `settings_get`.
pub fn settings_get(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::SettingsGet(pb::SettingsGetRequest {}),
    )
}

/// `settings_set`.
///
/// ⚠️ Full-replace: the daemon overwrites `global` AND the plugin map with
/// what arrived — plugins missing from the request are DELETED. Always pass
/// `plugins` from the last `settings_get`.
///
/// Submessages arrive already serialized (the Python settings pages form
/// them) and are decoded into prost types here.
///
/// ⚠️ A byte comparison with Python does NOT apply to this request: the order
/// of `map` entries on the wire is undefined in either implementation.
pub fn settings_set(
    request_id: u64,
    global_settings: &[u8],
    plugins: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, String> {
    let global = pb::GlobalSettings::decode(global_settings)
        .map_err(|e| format!("settings_set: global не разобран: {e}"))?;
    let mut map = std::collections::HashMap::with_capacity(plugins.len());
    for (name, raw) in plugins {
        let ps = pb::PluginSettings::decode(&raw[..]).map_err(|e| {
            format!("settings_set: плагин {name} не разобран: {e}")
        })?;
        map.insert(name.clone(), ps);
    }
    Ok(encode(
        request_id,
        pb::request::Payload::SettingsSet(pb::SettingsSetRequest {
            global: Some(global),
            plugins: map,
        }),
    ))
}

// ── installable plugins ─────────────────────────────────────────────────────

/// `plugin_list`.
pub fn plugin_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PluginList(pb::PluginListRequest {}),
    )
}

/// `plugin_install` — installing a plugin from a LOCAL `.zip`
/// (`$XDG_DATA_HOME/waywallen/plugins/`, the daemon downloads nothing).
pub fn plugin_install(request_id: u64, zip_path: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PluginInstall(pb::PluginInstallRequest {
            zip_path: zip_path.to_string(),
        }),
    )
}

// ── Workshop (the «Discover» page) ──────────────────────────────────────────

/// `remote_availability`.
pub fn remote_availability(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteAvailability(pb::RemoteAvailabilityRequest {}),
    )
}

/// Conditions of the `remote_search` request. v0.2.3+: the source is given by
/// `source_id` (from `RemoteAvailability.sources`), `sort_key` is a string key
/// from `RemoteSourceInfo.sorts`. waywallen 0.6.0: excluded tags, the filter
/// match mode and a continuation cursor (empty — the first page, or the
/// source pages by numbers).
pub struct RemoteSearch<'a> {
    pub query: &'a str,
    pub sort_key: &'a str,
    pub page: u32,
    pub source_id: &'a str,
    pub tags: &'a [String],
    pub excluded_tags: &'a [String],
    pub filter_match_modes: &'a std::collections::BTreeMap<String, String>,
    pub cursor: &'a str,
    /// waywallen 0.6.1: limit the search to one author (empty = everyone).
    pub artist_id: &'a str,
}

/// `remote_search`.
pub fn remote_search(request_id: u64, search: &RemoteSearch<'_>) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteSearch(pb::RemoteSearchRequest {
            source_id: search.source_id.to_string(),
            query: search.query.to_string(),
            sort_key: search.sort_key.to_string(),
            page: search.page,
            required_tags: search.tags.to_vec(),
            cursor: search.cursor.to_string(),
            filter_match_modes: search
                .filter_match_modes
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            excluded_tags: search.excluded_tags.to_vec(),
            artist_id: search.artist_id.to_string(),
        }),
    )
}

/// `remote_details`.
pub fn remote_details(request_id: u64, item_id: &str, source_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteDetails(pb::RemoteDetailsRequest {
            source_id: source_id.to_string(),
            id: item_id.to_string(),
        }),
    )
}

/// `remote_download`.
pub fn remote_download(request_id: u64, item_id: &str, source_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteDownload(pb::RemoteDownloadRequest {
            source_id: source_id.to_string(),
            id: item_id.to_string(),
        }),
    )
}

/// `remote_uninstall`.
pub fn remote_uninstall(
    request_id: u64,
    item_id: &str,
    source_id: &str,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteUninstall(pb::RemoteUninstallRequest {
            source_id: source_id.to_string(),
            id: item_id.to_string(),
        }),
    )
}

// ============================================================================
// Parsing RESPONSES the module itself needs
// ============================================================================
//
// Parsing of the tab's responses lives in [`crate::we_query`] (along with the
// `Query` state and the Qt signals). Here is only what the SYNCHRONOUS path
// of `client.py` reads, which has no `Query` at all: `stop_active_wallpapers`
// stops active renderers before installing static wallpapers.

/// Identifiers of active renderers from a `renderer_list` response frame.
///
/// Input — RAW `ServerFrame` bytes (as the WebSocket delivers them). `None` —
/// the frame did not parse or it is not a `renderer_list` response: the caller
/// then assumes there are no renderers (port of the original's
/// `except → ids = []`, where «could not read» and «nothing to stop» behave
/// the same).
///
/// Empty `renderer_id`s are dropped: there is nothing to kill a renderer
/// without an address with, and it would get into the stopped counter and lie
/// to the user about the result.
pub fn renderer_ids_from_frame(frame_bytes: &[u8]) -> Option<Vec<String>> {
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let pb::server_frame::Kind::Response(rsp) = frame.kind? else {
        return None;
    };
    let pb::response::Payload::RendererList(rl) = rsp.payload? else {
        return None;
    };
    Some(
        rl.instances
            .into_iter()
            .map(|i| i.renderer_id)
            .filter(|id| !id.is_empty())
            .collect(),
    )
}

// ============================================================================
// Server frame for Python callbacks
// ============================================================================
//
// The tab's Python callbacks read `pb.Response` fields THEMSELVES — for that
// the customer delivery carried a generated `control_pb2.py` and the
// `python-protobuf` dependency. Parsing is the same for all three callbacks,
// so it is here ONCE (rule #12), not a copy for each.
//
// ⚠️ Payloads that Python does NOT read are deliberately not laid out here:
// the core parses them on its side (`crate::we_query`), and a second copy of
// the parsing next to it would silently go stale. A new Python consumer
// appeared — add a branch for it to `payload_fields`, not its own in-place
// parsing.

/// Name of a payload for which there is no parsing here.
///
/// Not an empty string and not «ok»: a callback that sees this name must
/// understand that nobody parsed the fields for it, and not take emptiness
/// for the daemon's answer.
pub const REPLY_PAYLOAD_UNPARSED: &str = "unparsed";

/// A `ServerFrame` parsed for Python code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReplyFields {
    /// `"response"`, `"event"` or `""` — a frame without contents.
    pub kind: String,
    pub request_id: u64,
    pub status: i32,
    pub error_code: i32,
    pub message: String,
    /// Name of the `oneof payload` field — `""` (no payload) or
    /// [`REPLY_PAYLOAD_UNPARSED`] if no parsing is set up for it here.
    pub payload: String,
    /// String fields of the payload.
    pub text: Vec<(String, String)>,
    /// Boolean fields of the payload.
    pub flags: Vec<(String, bool)>,
}

/// Response payload fields that the Python callbacks read.
fn payload_fields(payload: Option<pb::response::Payload>) -> (String, Vec<(String, String)>, Vec<(String, bool)>) {
    match payload {
        None => (String::new(), Vec::new(), Vec::new()),
        // Tab window: whether the engine is healthy after the socket connects.
        Some(pb::response::Payload::Health(h)) => (
            "health".to_owned(),
            vec![
                ("service".to_owned(), h.service),
                ("state".to_owned(), h.state),
                ("os_name".to_owned(), h.os_name),
            ],
            Vec::new(),
        ),
        // «Developer» page: hot install of an engine plugin.
        Some(pb::response::Payload::PluginInstall(p)) => (
            "plugin_install".to_owned(),
            vec![("plugin_id".to_owned(), p.plugin_id)],
            vec![("needs_restart".to_owned(), p.needs_restart)],
        ),
        Some(_) => (REPLY_PAYLOAD_UNPARSED.to_owned(), Vec::new(), Vec::new()),
    }
}

/// Parse RAW `ServerFrame` bytes for a Python callback.
///
/// A parse error is returned to the caller: a broken frame must give a clear
/// error and be dropped, not be taken for an empty answer (the daemon may be
/// newer than us).
pub fn reply_fields(frame_bytes: &[u8]) -> Result<ReplyFields, prost::DecodeError> {
    let frame = pb::ServerFrame::decode(frame_bytes)?;
    match frame.kind {
        Some(pb::server_frame::Kind::Response(rsp)) => {
            let (payload, text, flags) = payload_fields(rsp.payload);
            Ok(ReplyFields {
                kind: "response".to_owned(),
                request_id: rsp.request_id,
                status: rsp.status,
                error_code: rsp.error_code,
                message: rsp.message,
                payload,
                text,
                flags,
            })
        }
        Some(pb::server_frame::Kind::Event(_)) => Ok(ReplyFields {
            kind: "event".to_owned(),
            ..Default::default()
        }),
        None => Ok(ReplyFields::default()),
    }
}

#[cfg(test)]
mod start_watch_tests {
    use super::*;

    #[test]
    fn exit_report_tells_causes_apart() {
        assert_eq!(daemon_output_log_path("/s/rexon/"), "/s/rexon/waywallen-daemon.log");
        assert_eq!(text_tail("a\n\nb  \nc\n", 2), ["b", "c"]);
        assert_eq!(text_tail("", 5), Vec::<String>::new());
        assert_eq!(parse_name_owner_pid("u 4242\n"), Some(4242));
        assert_eq!(parse_name_owner_pid("u 0"), None);
        assert_eq!(parse_name_owner_pid("Call failed: name has no owner"), None);
        assert_eq!(daemon_name_owner_pid_argv().last().map(String::as_str), Some(crate::waywallen_client::BUS_NAME));

        // Second instance: code 0, another process holds the name.
        let owner = BusNameOwner::Process { pid: 77, cmdline: "/usr/bin/waywallen --no-ui".into() };
        let tail = vec!["waywallen: already running, no UI to launch".to_string()];
        let r = daemon_exit_report(55, EngineExit::Code(0), Some(&owner), "/s/w.log", &tail);
        assert_eq!(r[0], "движок pid 55: код выхода 0");
        assert!(r[1].contains("pid 77") && r[1].contains("уступил"), "{r:?}");
        assert_eq!(r[3], "  | waywallen: already running, no UI to launch");

        // Own failure: code 1, the name is free, the error is in the tail.
        let r = daemon_exit_report(55, EngineExit::Code(1), Some(&BusNameOwner::Free), "/s/w.log", &[]);
        assert_eq!(r, ["движок pid 55: код выхода 1", "имя шины демона свободно", "вывод демона пуст (/s/w.log)"]);
        let r = daemon_exit_report(55, EngineExit::Signal(9), None, "/s/w.log", &[]);
        assert!(r[0].ends_with("убит сигналом 9") && r[1].contains("не удалось"));
        // Own name on a deadline failure — no guess about a second instance.
        let me = BusNameOwner::Process { pid: 55, cmdline: String::new() };
        let r = daemon_exit_report(55, EngineExit::Unknown, Some(&me), "/s/w.log", &[]);
        assert_eq!(r[1], "имя шины демона держит pid 55 (?)");
    }

    fn dead_bus() -> DaemonStatus {
        DaemonStatus::default()
    }

    fn life(alive: bool, activity: u64) -> Option<EngineLife> {
        Some(EngineLife { alive, activity, restarts: 0 })
    }

    /// Defect 30.10 (r1895): a live, working process after 12 s is not a failure
    /// but «waiting, the first launch can be long».
    #[test]
    fn working_process_is_waited_past_the_old_12_seconds() {
        let mut w = StartWatch::new(true);
        let mut activity = 0;
        let mut last = StartStep::Wait { slow: false };
        for _ in 0..(60_000 / POLL_MS) {
            activity += 7;
            last = w.tick(&dead_bus(), life(true, activity), POLL_MS);
            assert!(!matches!(last, StartStep::Failed { .. }), "живой процесс сдан: {last:?}");
        }
        assert_eq!(last, StartStep::Wait { slow: true });
        let live = DaemonStatus { running: true, ws_port: 5, ..Default::default() };
        assert_eq!(w.tick(&live, life(true, activity), POLL_MS), StartStep::Ready { status: live });
    }

    #[test]
    fn first_ticks_are_quiet_not_slow() {
        let mut w = StartWatch::new(true);
        assert_eq!(w.tick(&dead_bus(), life(true, 1), POLL_MS), StartStep::Wait { slow: false });
    }

    /// Died — fail right away, there is nothing to wait 12 s for.
    #[test]
    fn dead_process_fails_at_once() {
        let mut w = StartWatch::new(true);
        assert_eq!(
            w.tick(&dead_bus(), life(false, 0), POLL_MS),
            StartStep::Failed { message: daemon_exited_message() }
        );
    }

    /// The service has not moved to «activating» yet — that is not death while
    /// the quiet window has not expired.
    #[test]
    fn service_not_yet_started_is_not_dead_until_quiet_window_ends() {
        let mut w = StartWatch::new(false);
        let mut n = 0;
        loop {
            n += 1;
            match w.tick(&dead_bus(), life(false, 0), POLL_MS) {
                StartStep::Wait { .. } => {}
                StartStep::Failed { message } => {
                    assert_eq!(message, daemon_exited_message());
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(n as u64 * POLL_MS, START_QUIET_MS);
    }

    #[test]
    fn service_restart_means_crash() {
        let mut w = StartWatch::new(false);
        let at = |r| Some(EngineLife { alive: true, activity: r as u64, restarts: r });
        assert!(matches!(w.tick(&dead_bus(), at(0), POLL_MS), StartStep::Wait { .. }));
        assert_eq!(
            w.tick(&dead_bus(), at(1), POLL_MS),
            StartStep::Failed { message: daemon_exited_message() }
        );
    }

    /// A live but quiet process is hung: failure after the quiet window and the
    /// stall deadline, not earlier.
    #[test]
    fn silent_process_fails_after_stall() {
        let mut w = StartWatch::new(true);
        let mut t = 0;
        let message = loop {
            t += POLL_MS;
            if let StartStep::Failed { message } = w.tick(&dead_bus(), life(true, 42), POLL_MS) {
                break message;
            }
        };
        assert!(t >= START_QUIET_MS && t >= START_STALL_MS, "сдался на {t} мс");
        assert!(t < START_QUIET_MS + START_STALL_MS);
        assert!(message.starts_with(START_TIMEOUT_HEAD), "{message}");
    }

    #[test]
    fn unknown_life_fails_after_stall_from_start() {
        let mut w = StartWatch::new(true);
        let mut t = 0;
        while !matches!(w.tick(&dead_bus(), None, POLL_MS), StartStep::Failed { .. }) {
            t += POLL_MS;
        }
        assert_eq!(t + POLL_MS, START_STALL_MS.max(START_QUIET_MS));
    }

    /// Works endlessly but there is no bus — the hard limit.
    #[test]
    fn busy_forever_hits_hard_limit() {
        let mut w = StartWatch::new(true);
        let mut activity = 0;
        let mut t = 0;
        let message = loop {
            activity += 1;
            t += POLL_MS;
            if let StartStep::Failed { message } = w.tick(&dead_bus(), life(true, activity), POLL_MS) {
                break message;
            }
        };
        assert_eq!(t, START_HARD_LIMIT_MS);
        assert!(message.starts_with(START_TIMEOUT_HEAD));
    }

    #[test]
    fn proc_stat_parsing_survives_odd_names() {
        let line = "1234 (way wal) (len)) S 77 1 1 0 -1 4194560 100 0 0 0 150 25 0 0 20 0 4 0 999 1 2";
        assert_eq!(parse_proc_stat(line), Some((1234, 77, 'S', 175)));
        assert_eq!(parse_proc_stat("garbage"), None);
    }

    #[test]
    fn proc_io_sums_read_and_write_chars() {
        let io = "rchar: 1000\nwchar: 24\nsyscr: 5\nread_bytes: 4096\n";
        assert_eq!(parse_proc_io(io), 1024);
        assert_eq!(parse_proc_io(""), 0);
    }

    #[test]
    fn tree_life_counts_descendants_and_ignores_zombies() {
        let e = |pid, ppid, state, activity| ProcEntry { pid, ppid, state, activity };
        let entries = [e(10, 1, 'S', 5), e(11, 10, 'R', 7), e(12, 11, 'S', 1), e(20, 1, 'R', 100)];
        assert_eq!(tree_life(10, &entries), EngineLife { alive: true, activity: 13, restarts: 0 });
        assert!(!tree_life(30, &entries).alive);
        assert!(!tree_life(10, &[e(10, 1, 'Z', 5)]).alive);
    }

    #[test]
    fn unit_show_parsing() {
        let out = "ActiveState=activating\nMainPID=4321\nNRestarts=2\n";
        assert_eq!(parse_engine_unit_show(out), Some((true, 4321, 2)));
        assert_eq!(parse_engine_unit_show("ActiveState=failed\nMainPID=0\nNRestarts=5"), Some((false, 0, 5)));
        assert_eq!(parse_engine_unit_show(""), None);
        let entries = [ProcEntry { pid: 4321, ppid: 1, state: 'S', activity: 9 }];
        assert_eq!(
            service_life(out, &entries),
            Some(EngineLife { alive: true, activity: 9, restarts: 2 })
        );
        assert_eq!(engine_unit_show_argv().last().map(String::as_str), Some(AUTOSTART_UNIT));
    }
}
