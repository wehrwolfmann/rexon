// Background tasks of the "Developer" page of the "Wallpaper Engine" tab —
// a port of three `QThread`s from `rexon/qt_wallpaper_engine/window.py`
// together with their execution layer from
// `rexon/qt_wallpaper_engine/engine_update.py`.
//
// The original kept three QThread subclasses:
//
//   1. `_UpstreamCheckWorker` — checking both upstream targets (waywallen
//      and OWE): our version ↔ GitHub Releases, the changelog of the range,
//      writing the report into the state → the `done(statuses)` signal;
//   2. `_EngineInstallWorker` — download → verify → install the engine
//      AppImage (with a `.bak` rollback) → `progress(str)` and
//      `done(ok, msg)` signals;
//   3. `_OwePluginWorker`     — download → verify (→ install) the OWE plugin
//      zip → `progress(str)` and `done(ok, msg, zip_path)` signals.
//
// Here these are three tasks of one runtime, and the Qt signals are
// replaced with one event queue ([`UpdateEvent`]): the page owner takes
// them from its own thread ([`WeUpdateTasks::poll_event`]). A queue rather
// than callbacks — so that data crosses the thread boundary in exactly one
// place, as already done for the "Steam" tab (`steam_workers.rs`).
//
// What is NOT here: decisions. The API addresses, parsing the answers,
// comparing versions, the AppImage validation verdict, parsing the plugin
// archive, the installation and rollback plans, the state format and ALL
// labels for the user live in `rexon_core::engine_update` and
// `rexon_core::we_window`. Only execution moved to Rust: the network,
// unpacking, file operations and launching subprocesses.
//
// Warning: THE LIVE DAEMON VERSION. The original had a
// `_daemon_version_via_qt` path: it created a `DaemonControl` RIGHT IN THE
// BACKGROUND THREAD and already crashed the process (fixed in commit
// a38d22f — the callbacks are no longer set in the constructor). Here this
// path does not exist at all and cannot: not a single task of this module
// touches Qt. The live daemon version, if known, is supplied by the page
// OWNER from its own thread — as a parameter of
// [`WeUpdateTasks::start_check`]. The other sources (native to the
// background thread) are computed by the core: `--version` of the binary
// that will actually be launched, then `busctl`, then the state.
//
// Warning: fixes of the original (rule #16), each marked `Fix:` in place:
//   1. THE COMPATIBILITY GATE FROZE THE INTERFACE. `_start_owe_install`
//      called `waywallen_meets_owe_min()` in the GUI thread, and with the
//      daemon not running it goes to `busctl` (a 5 s ceiling) and to the
//      `--version` probe (another 5 s). Ten seconds of a frozen window per
//      button press. The gate moved inside the task, and the refusal arrives
//      as the [`UpdateEvent::OweGateRefused`] event with text assembled by
//      the core — the refusal looks the same as before;
//   2. THE UPDATE CHECK HAD NO CANCELLATION. `_UpstreamCheckWorker` could
//      not be interrupted at all: `_wait_workers` waited for it up to a
//      minute (`WORKER_WAIT_MS`) until the network timeouts expired. Here the
//      tab has a cancellation tree, and the token aborts an already sent
//      request;
//   3. TWO WORKERS WROTE THE STATE INDEPENDENTLY. The check
//      (`_store_report`) and the installation (`mark_installed`)
//      read-modified-wrote one file from different threads without any
//      synchronization: a simultaneous write lost either the report or the
//      version. Here editing the state goes under a shared process lock.
//
// Never: do NOT run the engine installation live: it is ~200 MiB and system
// paths. The checks of this module work on local files and temporary
// directories, and the comparison is with a recorded reference of the live
// Python (`rust/rexon-gui/tests/golden/engine_update_reference.json`).

use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use rexon_core::engine_update as eu;
use rexon_core::net::{self, NetIo, UreqNet};
use rexon_core::we_window as ww;
use serde_json::{Map, Value};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::steam_workers::blocking_cancellable;
use crate::we_env;

/// How long we wait for the tasks to finish when the tab closes. This is a
/// CEILING, not a planned wait: the token is already triggered, the tasks
/// abort their requests themselves. The original waited up to
/// `WORKER_WAIT_MS` (a minute) AND WITHOUT cancellation for the update
/// check.
pub const SHUTDOWN_GRACE_MS: u64 = 2000;

/// Worker threads of the runtime. There are three tasks, all of them wait
/// for the network or a subprocess, so a couple of threads is enough: the
/// heavy work goes to the blocking pool.
const WORKER_THREADS: usize = 2;

// ============================================================================
// Events out
// ============================================================================

/// What the tasks hand to the page owner. A replacement for the signals of
/// the three QThreads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateEvent {
    /// `_UpstreamCheckWorker.done(statuses)`. An empty list means, as in the
    /// original, "could not check".
    CheckDone(Vec<eu::TargetStatus>),
    /// `progress(str)` of both installing workers: a ready status line from the
    /// core.
    Progress(String),
    /// `_EngineInstallWorker.done(ok, msg)`.
    EngineDone { ok: bool, message: String },
    /// `_OwePluginWorker.done(ok, msg, zip_path)`. A non-empty `zip_path` means
    /// the archive was verified and handed to the GUI thread for hot-install
    /// through the daemon IPC (a QWebSocket must not be touched from another
    /// thread).
    OweDone {
        ok: bool,
        message: String,
        zip_path: String,
    },
    /// The waywallen ≥ 0.2.3 compatibility gate was not passed: the text is
    /// already assembled by the core (`we_window::owe_gate_refused_text`). Fix
    /// 1 — the gate is computed by the task, not by the GUI thread.
    OweGateRefused(String),
    /// The live engine version for the "waywallen version: …" label.
    ///
    /// Warning: fix 4 (rule #16): the original had no background path for the
    /// version label at all — `_refresh_version` called `current_version()`
    /// RIGHT IN THE GUI THREAD, and that goes to the binary's `--version` (a 5
    /// s ceiling) and to `busctl` (another 5 s). The page opened and froze for
    /// those seconds on every visit and after every installation. Here the
    /// version is computed by a task, and the label arrives as an event.
    EngineVersion(String),
}

// ============================================================================
// Environment
// ============================================================================

/// Paths the state, the plugins directory and the managed AppImage depend
/// on. Taken once on construction: there is no reason to read `$HOME` from a
/// worker task, and the checks substitute their own.
#[derive(Debug, Clone, Default)]
pub struct UpdateEnv {
    pub home: String,
    pub xdg_config_home: Option<String>,
    pub xdg_data_home: Option<String>,
    /// Engine directory from the package (`we_client::ENGINE_DIR`); the checks
    /// substitute their own sandbox.
    pub engine_dir: String,
    /// The engine daemon from the distribution package
    /// (`we_client::SYSTEM_DAEMON`).
    pub system_daemon: String,
    /// Data directory of the engine from distribution packages
    /// (`we_client::SYSTEM_ENGINE_DIR`, with `plugins/` in it).
    pub system_engine_dir: String,
}

impl UpdateEnv {
    /// The live process environment.
    pub fn from_process() -> UpdateEnv {
        UpdateEnv {
            home: we_env::home(),
            xdg_config_home: we_env::xdg_config_home(),
            xdg_data_home: we_env::xdg_data_home(),
            engine_dir: rexon_core::we_client::ENGINE_DIR.to_string(),
            system_daemon: rexon_core::we_client::SYSTEM_DAEMON.to_string(),
            system_engine_dir: rexon_core::we_client::SYSTEM_ENGINE_DIR.to_string(),
        }
    }

    /// Refusal to install a second copy of target `key` that the package
    /// manager installed (`engine_update::system_update_refusal`); `None` means
    /// it is allowed.
    pub fn system_refusal(&self, key: &str) -> Option<String> {
        let system_engine = std::path::Path::new(&self.system_daemon).exists();
        let owe = eu::owe_manifest_path(&rexon_core::we_client::engine_plugins_path(&self.system_engine_dir));
        let system_owe = std::path::Path::new(&owe).is_file();
        eu::system_update_refusal(key, system_engine, system_owe)
    }

    /// File of the dev updates state.
    pub fn state_path(&self) -> String {
        eu::state_path_xdg(self.xdg_config_home.as_deref(), &self.home)
    }

    /// The user's waywallen plugins directory.
    pub fn plugins_dir(&self) -> String {
        eu::plugins_dir(self.xdg_data_home.as_deref(), &self.home)
    }

    /// The managed engine AppImage.
    pub fn managed_appimage(&self) -> String {
        eu::managed_appimage(&self.home)
    }
}

// ============================================================================
// Page runtime
// ============================================================================

/// All background tasks of the "Developer" page and their cancellation.
pub struct WeUpdateTasks {
    /// Warning: `Option` rather than just `Runtime`: the ordinary `Drop` of a
    /// tokio runtime waits for blocking tasks WITHOUT A LIMIT, and one stuck
    /// task keeps the process from exiting at all. The analysis is at this
    /// page's twin (`steam_workers::SteamTasks`), rule #12: the shutdown order
    /// is the same for both runtimes.
    rt: Option<Runtime>,
    /// Root of the cancellation tree: closing the tab cancels EVERYTHING.
    root: CancellationToken,
    /// Accounting of live tasks — so that closing waits for each one.
    tracker: TaskTracker,
    tx: UnboundedSender<UpdateEvent>,
    rx: Mutex<UnboundedReceiver<UpdateEvent>>,
    /// Whether a check is in progress (the original's
    /// `_check_worker.isRunning()`).
    checking: std::sync::Arc<AtomicBool>,
    /// Whether an engine installation is in progress
    /// (`_install_worker.isRunning()`).
    installing: std::sync::Arc<AtomicBool>,
    /// Whether plugin work is in progress (`_owe_worker.isRunning()`).
    owe_busy: std::sync::Arc<AtomicBool>,
    env: UpdateEnv,
}

impl WeUpdateTasks {
    /// Bring up the page runtime. `None` means the runtime was not created (in
    /// the live Rexon this means "there will be no background tasks", not a
    /// crash).
    pub fn new(env: UpdateEnv) -> Option<WeUpdateTasks> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .thread_name("rexon-we-update")
            // Warning: enable_io is mandatory, not "just in case": without it
            // `tokio::process` PANICS in a worker thread on the very first subprocess
            // ("A Tokio 1.x context was found, but IO is disabled"), and there are
            // three subprocesses here — the binary's `--version`, busctl and the
            // AppImage probe. Caught by a live run.
            .enable_io()
            .enable_time()
            .build()
            .ok()?;
        let (tx, rx) = unbounded_channel();
        let root = CancellationToken::new();
        // ⭐ The shared exit door: without this entry the background task drain
        // does not see the runtime's worker threads and answers "nobody is left"
        // while an update check is running. The same at the neighbour —
        // `steam_workers::SteamTasks::new`.
        {
            let root = root.clone();
            rexon_core::exit_guard::register_stopper(move || root.cancel());
        }
        Some(WeUpdateTasks {
            rt: Some(rt),
            root,
            tracker: TaskTracker::new(),
            tx,
            rx: Mutex::new(rx),
            checking: std::sync::Arc::new(AtomicBool::new(false)),
            installing: std::sync::Arc::new(AtomicBool::new(false)),
            owe_busy: std::sync::Arc::new(AtomicBool::new(false)),
            env,
        })
    }

    /// Take the next event; `None` means the queue is empty. Called from the
    /// interface thread (by the page's timer), never blocks.
    pub fn poll_event(&self) -> Option<UpdateEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// The environment the tasks work with (the owner needs it for path labels
    /// and for checks).
    pub fn env(&self) -> &UpdateEnv {
        &self.env
    }

    pub fn is_checking(&self) -> bool {
        self.checking.load(Ordering::SeqCst)
    }

    pub fn is_installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }

    pub fn is_owe_running(&self) -> bool {
        self.owe_busy.load(Ordering::SeqCst)
    }

    // ── Task 1: checking for updates ───────────────────────────────────

    /// A port of `_UpstreamCheckWorker`. `false` means a check is already in
    /// progress (the original's early return from `_start_upstream_check`).
    ///
    /// `daemon_version` is the `Version` property of the live daemon if the
    /// owner knows it; "" means the source is skipped. Asking the daemon FROM
    /// HERE is not allowed: that is Qt, and we are in another thread (see the
    /// module header).
    pub fn start_check(&self, daemon_version: String) -> bool {
        if self.checking.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        let busy = self.checking.clone();
        self.spawn(async move {
            let statuses = check_all_targets(&env, &token, &daemon_version).await;
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::CheckDone(statuses));
            }
        });
        true
    }

    /// Update the engine version label in the background (fix 4).
    ///
    /// The original had no separate task: it asked for the version right in the
    /// interface thread. It has no busy flag of its own — the task is short,
    /// idempotent and writes nothing.
    pub fn start_version_refresh(&self, daemon_version: String) {
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        self.spawn(async move {
            let version = engine_current_version(&env, &daemon_version).await;
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::EngineVersion(version));
            }
        });
    }

    // ── Task 2: installing the engine AppImage ─────────────────────────

    /// A port of `_EngineInstallWorker`. `false` means an installation is
    /// already in progress.
    pub fn start_engine_install(&self, asset_url: String, latest_tag: String) -> bool {
        if self.installing.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        let busy = self.installing.clone();
        self.spawn(async move {
            // The engine is installed as a distribution package — we do not download
            // a second copy.
            if let Some(message) = env.system_refusal("waywallen") {
                busy.store(false, Ordering::SeqCst);
                if !token.is_cancelled() {
                    let _ = tx.send(UpdateEvent::EngineDone { ok: false, message });
                }
                return;
            }
            let out = engine_install(&env, &token, &tx, &asset_url, &latest_tag).await;
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::EngineDone {
                    ok: out.ok,
                    message: out.message,
                });
            }
        });
        true
    }

    // ── Task 3: the OWE plugin ─────────────────────────────────────────

    /// A port of `_OwePluginWorker` together with the compatibility gate (fix
    /// 1).
    ///
    /// `fs_install` is decided by the owner BEFORE the start:
    /// `we_window::plugin_fs_install` from "whether the daemon is connected".
    /// `false` means plugin work is already in progress.
    pub fn start_owe_install(
        &self,
        asset_url: String,
        latest_tag: String,
        fs_install: bool,
        daemon_version: String,
    ) -> bool {
        if self.owe_busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        let busy = self.owe_busy.clone();
        self.spawn(async move {
            // The plugin is installed as a distribution package — we do not download a
            // second copy. The refusal goes as the same event as the gate: the button
            // comes back.
            if let Some(message) = env.system_refusal("owe") {
                busy.store(false, Ordering::SeqCst);
                if !token.is_cancelled() {
                    let _ = tx.send(UpdateEvent::OweGateRefused(message));
                }
                return;
            }
            // Fix 1: the "waywallen ≥ 0.2.3" gate is computed here and not in the
            // interface thread — it goes to busctl and to the `--version` probe.
            let version = engine_current_version(&env, &daemon_version).await;
            let (allowed, why) = eu::waywallen_meets_owe_min(&version);
            if !allowed {
                busy.store(false, Ordering::SeqCst);
                if !token.is_cancelled() {
                    let _ = tx.send(UpdateEvent::OweGateRefused(ww::owe_gate_refused_text(&why)));
                }
                return;
            }
            let out = owe_install(&env, &token, &tx, &asset_url, &latest_tag, fs_install).await;
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::OweDone {
                    ok: out.ok,
                    message: out.message,
                    zip_path: out.zip_path,
                });
            }
        });
        true
    }

    // ── Services for the interface thread ──────────────────────────────

    /// A port of `engine_update.mark_seen` — "read the changelog, will not
    /// install". The version is written as a tag, as in `_on_mark_owe_seen`.
    pub fn mark_seen(&self, key: &str, version: &str) {
        let now = now_iso();
        edit_state(&self.env.state_path(), |st| {
            eu::mark_seen(st, key, version, &now);
        });
    }

    /// A port of `mark_installed(key, version_tag_to_state(tag))` — that is how
    /// `_on_owe_ipc_resp` calls it after the daemon successfully installed the
    /// plugin.
    pub fn mark_installed_tag(&self, key: &str, tag: &str) {
        let version = eu::version_tag_to_state(tag);
        let now = now_iso();
        edit_state(&self.env.state_path(), |st| {
            eu::mark_installed(st, key, &version, &now);
        });
    }

    /// The FS fallback of the plugin installation RIGHT IN THE INTERFACE THREAD
    /// — a port of the `_on_owe_worker_done` branch where the daemon died while
    /// the archive was downloading. There is no network here, only unpacking an
    /// already downloaded file, so the original did it on the spot — and so do
    /// we.
    pub fn install_plugin_zip_now(&self, zip_path: &str, tag: &str) -> (bool, String) {
        install_plugin_zip_fs(&self.env, zip_path, tag, "")
    }

    // ── Closing ────────────────────────────────────────────────────────

    /// Cancel ALL tasks and wait for them — a port of `_wait_workers`.
    ///
    /// The original waited up to a minute because the update check had no
    /// cancellation at all (fix 2). Here the token is triggered first, the
    /// tasks abort their requests themselves, and the wait is a safety net with
    /// the ceiling [`SHUTDOWN_GRACE_MS`].
    pub fn shutdown(&self) {
        self.root.cancel();
        self.tracker.close();
        let Some(rt) = self.rt.as_ref() else {
            return;
        };
        let tracker = self.tracker.clone();
        rt.block_on(async move {
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(SHUTDOWN_GRACE_MS),
                tracker.wait(),
            )
            .await;
        });
    }

    /// Start a task under the tracker's accounting.
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if self.root.is_cancelled() {
            return;
        }
        let Some(rt) = self.rt.as_ref() else {
            return;
        };
        let _guard = rt.enter();
        self.tracker.spawn(fut);
    }
}

impl Drop for WeUpdateTasks {
    /// A ceiling on dismantling the runtime — the same order as at
    /// `steam_workers::SteamTasks` (rule #12): a user who closed the window
    /// must get a closed window, whatever the background task is busy with.
    fn drop(&mut self) {
        self.root.cancel();
        self.tracker.close();
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(std::time::Duration::from_millis(SHUTDOWN_GRACE_MS));
        }
    }
}

// ============================================================================
// Task outcomes
// ============================================================================

/// Outcome of the installation task — a port of the core's `WorkerOutcome`,
/// brought to an event.
struct TaskOutcome {
    ok: bool,
    message: String,
    /// Non-empty means the archive was handed to the GUI thread for IPC
    /// installation.
    zip_path: String,
}

// ============================================================================
// Task 1: checking both targets
// ============================================================================

/// A port of `check_all_targets` — both targets in a row, as in the
/// original.
async fn check_all_targets(
    env: &UpdateEnv,
    token: &CancellationToken,
    daemon_version: &str,
) -> Vec<eu::TargetStatus> {
    // The live engine version is needed only for the `waywallen` target, and
    // we ask for it ONCE: the original called `current_version()` inside the
    // loop over the targets.
    let engine_version = engine_current_version(env, daemon_version).await;
    let state_path = env.state_path();
    let mut out = Vec::new();
    for target in eu::targets(&env.home) {
        if token.is_cancelled() {
            break;
        }
        let state = eu::load_state(read_state_text(&state_path).as_deref());
        let installed = eu::installed_version(
            &target.key,
            if target.key == "waywallen" {
                &engine_version
            } else {
                ""
            },
            &owe_installed_version(env),
            &eu::state_field(&state, &target.key, "installed_version"),
            &eu::state_field(&state, &target.key, "seen_version"),
        );
        let since = eu::state_field(&state, &target.key, "last_check");
        let repo = target.repo.clone();
        let fetch = blocking_cancellable(token, move |cancel| fetch_outcome(&repo, &since, &cancel))
            .await;
        let Some(fetch) = fetch else { break };
        let st = eu::check_target(&target, &installed, &fetch);
        if st.ok {
            let now = now_iso();
            let key = target.key.clone();
            let snapshot = st.clone();
            edit_state(&state_path, |state| {
                eu::store_report(state, &key, &snapshot, &now);
            });
        }
        out.push(st);
    }
    out
}

/// A trip to the GitHub API for releases, with a fallback to the default
/// branch commits — a port of the network part of `check_target`.
fn fetch_outcome(repo: &str, since_iso: &str, cancel: &net::CancelToken) -> eu::FetchOutcome {
    match api_json(&eu::releases_url(repo), cancel) {
        Err(e) => eu::FetchOutcome::ReleasesFailed(e),
        Ok(body) => match eu::parse_releases(&body) {
            Err(e) => eu::FetchOutcome::ReleasesFailed(e),
            Ok(rels) if !rels.is_empty() => eu::FetchOutcome::Releases(rels),
            // The repo has no releases — fall back to the default branch commits.
            Ok(_) => match api_json(&eu::commits_url(repo, since_iso), cancel) {
                Err(e) => eu::FetchOutcome::CommitsFailed(e),
                Ok(body) => match eu::parse_commits(&body) {
                    Err(e) => eu::FetchOutcome::CommitsFailed(e),
                    Ok(commits) => eu::FetchOutcome::Commits(commits),
                },
            },
        },
    }
}

/// `GET` to the GitHub API — a port of `_api_json`. The body as is; the
/// core parses it.
fn api_json(url: &str, cancel: &net::CancelToken) -> Result<String, String> {
    let mut net = api_net(cancel.clone());
    let body = net
        .get(url, eu::API_USER_AGENT, eu::API_TIMEOUT_SECS)
        .map_err(|e| e.human())?;
    String::from_utf8(body).map_err(|e| e.to_string())
}

/// Transport for the GitHub API: exactly the headers the core named
/// (`api_headers` — `Accept` and `User-Agent`).
fn api_net(cancel: net::CancelToken) -> UreqNet {
    let mut extra = eu::api_headers();
    // The transport sets `User-Agent` itself as a separate argument of every
    // request — only what it does not know remains here.
    extra.retain(|(name, _)| !name.eq_ignore_ascii_case("user-agent"));
    UreqNet::new().with_cancel(cancel).with_headers(extra)
}

// ============================================================================
// Our current version
// ============================================================================

/// The waywallen engine version — a port of `current_version` with all its
/// sources.
///
/// The order and the verdict are entirely the core's
/// (`engine_update::current_version`): here there is only gathering
/// observations. `daemon_version` comes from the page owner because only
/// the Qt layer can ask the daemon over D-Bus.
async fn engine_current_version(env: &UpdateEnv, daemon_version: &str) -> String {
    let binary = binary_version().await;
    let busctl = run_capture(
        &eu::busctl_version_argv(),
        eu::BUSCTL_TIMEOUT_SECS,
    )
    .await
    .map(|(stdout, _)| stdout);
    let state = eu::load_state(read_state_text(&env.state_path()).as_deref());
    eu::current_version(
        Some(&binary),
        Some(daemon_version),
        busctl.as_deref(),
        &eu::state_field(&state, "waywallen", "installed_version"),
    )
}

/// `waywallen --version` of THE binary that will actually be launched.
/// Different builds print the version to stdout or to stderr — we look at
/// both.
async fn binary_version() -> String {
    let Some(bin) = we_env::daemon_binary() else {
        return String::new();
    };
    let Some((stdout, stderr)) = run_capture(
        &eu::binary_version_argv(&bin),
        eu::BINARY_VERSION_TIMEOUT_SECS,
    )
    .await
    else {
        return String::new();
    };
    let v = eu::parse_binary_version(&stdout);
    if v.is_empty() {
        eu::parse_binary_version(&stderr)
    } else {
        v
    }
}

/// The version of the OWE plugin that the daemon will ACTUALLY load — the
/// truth from the FS (`plugin.toml`), a port of `owe_installed_version`.
///
/// Since 28.09.2026 there may be two copies: the user's
/// (`$XDG_DATA_HOME/waywallen/plugins`) and in the Rexon package
/// (`/usr/lib/rexon/engine/plugins`). The daemon takes the newer of the two
/// (on a tie, the user's copy); we compute the version by the same rule
/// (`we_client::active_plugin_source`), otherwise the "Developer" page would
/// offer to update a plugin that is already installed in the package.
pub fn owe_installed_version(env: &UpdateEnv) -> String {
    let read = |plugins: &str| {
        std::fs::read_to_string(eu::owe_manifest_path(plugins))
            .map(|text| eu::owe_version_from_manifest(&text))
            .unwrap_or_default()
    };
    let user = read(&env.plugins_dir());
    // The package copy: from the distribution package, and without it, the one
    // built into Rexon (the order of `we_client::engine_layouts`).
    let bundled = Some(read(&rexon_core::we_client::engine_plugins_path(&env.system_engine_dir)))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| read(&rexon_core::we_client::engine_plugins_path(&env.engine_dir)));
    match (bundled.is_empty(), user.is_empty()) {
        (true, _) => user,
        (false, true) => bundled,
        (false, false) => match rexon_core::we_client::active_plugin_source(&bundled, &user) {
            rexon_core::we_client::PluginSource::Bundled => bundled,
            rexon_core::we_client::PluginSource::User => user,
        },
    }
}

// ============================================================================
// Task 2: download → verify → install the engine AppImage
// ============================================================================

async fn engine_install(
    env: &UpdateEnv,
    token: &CancellationToken,
    tx: &UnboundedSender<UpdateEvent>,
    url: &str,
    tag: &str,
) -> TaskOutcome {
    let prefix = ww::PREFIX_ENGINE;
    let _ = tx.send(UpdateEvent::Progress(ww::download_start_text(prefix)));
    let tmp = download_step(token, tx, url, eu::DownloadKind::AppImage, prefix).await;
    if let Some(out) = ww::after_download(prefix, tmp.is_some(), token.is_cancelled(), None) {
        if out.remove_tmp {
            drop_tmp(tmp.as_deref().unwrap_or_default());
        }
        return done(out);
    }
    let tmp = tmp.unwrap_or_default();

    let _ = tx.send(UpdateEvent::Progress(ww::text_validate_appimage().to_string()));
    let (ok, msg) = validate_appimage(&tmp).await;
    if let Some(out) = ww::after_validate(false, ok, &msg) {
        if out.remove_tmp {
            drop_tmp(&tmp);
        }
        return done(out);
    }

    // Stop the engine BEFORE the swap (pitfall of 27.08.2026: a swap under a
    // live daemon — a version mismatch and SIGBUS on stop). If it could not be
    // stopped, the file is not touched at all.
    let _ = tx.send(UpdateEvent::Progress(rexon_core::i18n::tr(
        "Stopping the wallpaper engine…",
    )));
    let swap = match stop_engine_for_swap().await {
        Ok(plan) => plan,
        Err(message) => {
            drop_tmp(&tmp);
            return done(ww::after_install_appimage(false, &message));
        }
    };

    let env2 = env.clone();
    let src = tmp.clone();
    let version = tag.to_string();
    let (ok, imsg) = tokio::task::spawn_blocking(move || install_appimage(&env2, &src, &version))
        .await
        .unwrap_or_else(|e| (false, eu::appimage_install_fail_message(&e.to_string())));
    // Bring the service up on ANY outcome: on rollback the previous version
    // came back, and the user must not be left without a wallpaper.
    let restarted = match &swap.start_argv {
        Some(argv) => run_status(argv, rexon_core::we_client::SYSTEMCTL_TIMEOUT_SECS).await == Some(0),
        None => false,
    };
    let imsg = match (ok, restarted, swap.quit_daemon) {
        (true, true, _) => eu::appimage_install_restarted_message(&env.managed_appimage()),
        (true, false, true) => eu::appimage_install_quit_message(&env.managed_appimage()),
        _ => imsg,
    };
    // A new engine — and the KDE wallpaper plugin, if already installed, in
    // step with the bundle (defect of 12.11.2026). One not installed is not
    // installed.
    if ok {
        crate::we_socket::refresh_installed_kde_plugin("обновление движка");
    }
    let out = ww::after_install_appimage(ok, &imsg);
    if out.remove_tmp {
        drop_tmp(&tmp);
    }
    done(out)
}

// ============================================================================
// Task 3: download → verify (→ install) the OWE plugin zip
// ============================================================================

async fn owe_install(
    env: &UpdateEnv,
    token: &CancellationToken,
    tx: &UnboundedSender<UpdateEvent>,
    url: &str,
    tag: &str,
    fs_install: bool,
) -> TaskOutcome {
    let prefix_text = ww::prefix_owe();
    let prefix = prefix_text.as_str();
    let _ = tx.send(UpdateEvent::Progress(ww::download_start_text(prefix)));
    let tmp = download_step(token, tx, url, eu::DownloadKind::PluginZip, prefix).await;
    if let Some(out) = ww::after_download(prefix, tmp.is_some(), token.is_cancelled(), None) {
        if out.remove_tmp {
            drop_tmp(tmp.as_deref().unwrap_or_default());
        }
        return done(out);
    }
    let tmp = tmp.unwrap_or_default();

    let _ = tx.send(UpdateEvent::Progress(ww::text_validate_plugin().to_string()));
    let path = tmp.clone();
    let validation = tokio::task::spawn_blocking(move || eu::validate_plugin_zip(Path::new(&path)))
        .await
        .unwrap_or_default();
    if let Some(out) = ww::after_validate(true, validation.ok, &validation.msg) {
        if out.remove_tmp {
            drop_tmp(&tmp);
        }
        return done(out);
    }

    let out = ww::after_validate_plugin_route(fs_install, &validation.msg);
    if out.hand_over_tmp {
        // The installation will be done by the GUI thread through the daemon IPC:
        // a QWebSocket must not be touched from another thread, the worker only
        // delivers the path.
        return TaskOutcome {
            ok: out.ok,
            message: out.message,
            zip_path: tmp,
        };
    }

    let _ = tx.send(UpdateEvent::Progress(
        ww::text_install_plugin_fs().to_string(),
    ));
    let env2 = env.clone();
    let src = tmp.clone();
    let version = tag.to_string();
    let (ok, imsg) =
        tokio::task::spawn_blocking(move || install_plugin_zip_fs(&env2, &src, &version, ""))
            .await
            .unwrap_or_else(|e| (false, eu::plugin_install_fail_message(&e.to_string())));
    let out = ww::after_install_plugin_fs(ok, &imsg);
    if out.remove_tmp {
        drop_tmp(&tmp);
    }
    done(out)
}

/// The core's verdict → the task outcome (the archive is handed to nobody).
fn done(out: ww::WorkerOutcome) -> TaskOutcome {
    TaskOutcome {
        ok: out.ok,
        message: out.message,
        zip_path: String::new(),
    }
}

// ============================================================================
// Downloading
// ============================================================================

/// The download step with progress messages — shared by both tasks.
async fn download_step(
    token: &CancellationToken,
    tx: &UnboundedSender<UpdateEvent>,
    url: &str,
    kind: eu::DownloadKind,
    prefix: &str,
) -> Option<String> {
    let spec = eu::download_spec(kind);
    let url = url.to_string();
    let prefix = prefix.to_string();
    let tx = tx.clone();
    blocking_cancellable(token, move |cancel| {
        download_asset(&url, &spec, &cancel, |done_b, total_b| {
            if let Some(text) = ww::download_progress(&prefix, done_b, total_b) {
                let _ = tx.send(UpdateEvent::Progress(text));
            }
        })
    })
    .await
    .flatten()
}

/// A port of `download_asset`/`download_plugin_zip`: a release asset into a
/// temporary file. A path or `None`. A local path (for checks) is accepted
/// too — as in the original.
fn download_asset(
    url: &str,
    spec: &eu::DownloadSpec,
    cancel: &net::CancelToken,
    on_progress: impl FnMut(u64, u64),
) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let tmp = mktemp_file(&spec.prefix, &spec.suffix)?;
    let fetched = if Path::new(url).is_file() {
        std::fs::copy(url, &tmp).map(|_| ()).map_err(|_| ())
    } else {
        stream_to_file(url, &tmp, spec, cancel, on_progress)
    };
    if fetched.is_err()
        || std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(spec.mode)).is_err()
    {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    Some(tmp)
}

/// The response body into a file in `spec.chunk` pieces with cooperative
/// cancellation between them — a port of the
/// `while True: chunk = r.read(...)` loop.
fn stream_to_file(
    url: &str,
    dest: &str,
    spec: &eu::DownloadSpec,
    cancel: &net::CancelToken,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), ()> {
    let mut net = UreqNet::new().with_cancel(cancel.clone());
    let total = net
        .open(url, eu::API_USER_AGENT, spec.timeout_secs)
        .map_err(|_| ())?
        .max(0) as u64;
    let mut file = std::fs::File::create(dest).map_err(|_| ())?;
    let mut got: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(());
        }
        let chunk = net.read(spec.chunk as usize).map_err(|_| ())?;
        if chunk.is_empty() {
            break;
        }
        file.write_all(&chunk).map_err(|_| ())?;
        got += chunk.len() as u64;
        on_progress(got, total);
    }
    net.close();
    file.flush().map_err(|_| ())
}

// ============================================================================
// AppImage validation and installation
// ============================================================================

/// A port of `validate_appimage`: we take the observations here, the core
/// gives the verdict.
async fn validate_appimage(path: &str) -> (bool, String) {
    let meta = std::fs::metadata(path);
    let is_file = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
    let is_exec = is_file
        && meta
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    if !(is_file && is_exec) {
        return eu::validate_appimage_verdict(is_file, is_exec, None, "");
    }
    match run_capture(&eu::appimage_probe_argv(path), eu::PROBE_TIMEOUT_SECS).await {
        Some((stdout, stderr)) => {
            eu::validate_appimage_verdict(true, true, None, &format!("{stdout}{stderr}"))
        }
        // Did not start or did not fit into the ceiling — for the core this is the
        // same "spawn_error" as the `subprocess.run` exception in the original.
        None => eu::validate_appimage_verdict(true, true, Some(&probe_failed()), ""),
    }
}

/// An explanation for the failed probe launch: the core inserts it into its
/// phrase "does not start: …", which the user sees on the "Developer" page
/// — so it too comes from the translation catalogue.
fn probe_failed() -> String {
    rexon_core::i18n::tr("the process did not start or did not respond in time")
}

/// A port of `install_appimage`: an atomic swap of the managed AppImage
/// with a `.bak` rollback. The plan (paths, permissions, version for the
/// state) is built by the core.
fn install_appimage(env: &UpdateEnv, new_path: &str, tag: &str) -> (bool, String) {
    let plan = eu::appimage_install_plan_for_dest(&env.managed_appimage(), tag);
    let _ = std::fs::create_dir_all(&plan.dest_dir);
    match place_appimage(&plan, new_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(new_path);
            if !plan.state_version.is_empty() {
                let now = now_iso();
                edit_state(&env.state_path(), |st| {
                    eu::mark_installed(st, "waywallen", &plan.state_version, &now);
                });
            }
            (true, eu::appimage_install_ok_message(&plan.dest))
        }
        Err(e) => {
            // An unfinished `.new-update` does not stay as garbage in the engine
            // directory.
            let _ = std::fs::remove_file(&plan.tmp);
            if Path::new(&plan.bak).is_file() {
                let _ = std::fs::rename(&plan.bak, &plan.dest);
            }
            (false, eu::appimage_install_fail_message(&e))
        }
    }
}

/// File operations of the swap: a copy of the previous version into
/// `.bak`, the new one through a temporary name IN THE SAME folder and
/// `rename` (writing over a running binary is not allowed — "Text file
/// busy").
fn place_appimage(plan: &eu::AppimageInstallPlan, new_path: &str) -> Result<(), String> {
    if Path::new(&plan.dest).is_file() {
        std::fs::copy(&plan.dest, &plan.bak).map_err(|e| e.to_string())?;
    }
    std::fs::copy(new_path, &plan.tmp).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&plan.tmp, std::fs::Permissions::from_mode(plan.mode))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&plan.tmp, &plan.dest).map_err(|e| e.to_string())
}

// ============================================================================
// Installing the plugin zip into the file system
// ============================================================================

/// A port of `install_plugin_zip_fs`: staging → an atomic move into
/// `plugins_dir()/<id>`, the previous version into `.bak-update`, a rollback
/// by the core's plan.
///
/// `plugins_root` overrides the directory ("" = the standard one).
pub fn install_plugin_zip_fs(
    env: &UpdateEnv,
    zip_path: &str,
    tag: &str,
    plugins_root: &str,
) -> (bool, String) {
    let validation = eu::validate_plugin_zip(Path::new(zip_path));
    let plan = eu::plugin_install_plan(&validation, &env.plugins_dir(), plugins_root, tag);
    if !plan.ok {
        return (false, plan.message);
    }
    let Some(staging) = mktemp_dir(&plan.staging_prefix) else {
        return (
            false,
            eu::plugin_install_fail_message(&rexon_core::i18n::tr("could not create the staging directory")),
        );
    };
    let extract_root = format!("{staging}/{}", plan.extract_subdir);
    let mut moved_to_bak = false;
    let mut target_created = false;
    let outcome = unpack_into_place(
        zip_path,
        &plan,
        &extract_root,
        &mut moved_to_bak,
        &mut target_created,
    );
    rmtree_quiet(&staging);
    match outcome {
        Ok(()) => {
            if !plan.state_version.is_empty() {
                let now = now_iso();
                edit_state(&env.state_path(), |st| {
                    eu::mark_installed(st, "owe", &plan.state_version, &now);
                });
            }
            (
                true,
                eu::plugin_install_ok_message(&plan.info.id, &plan.target),
            )
        }
        Err(e) => {
            let roll = eu::plugin_rollback_plan(moved_to_bak, target_created);
            if roll.remove_target {
                rmtree_quiet(&plan.target);
            }
            if roll.restore_bak && Path::new(&plan.bak).exists() {
                let _ = std::fs::rename(&plan.bak, &plan.target);
            }
            (false, eu::plugin_install_fail_message(&e))
        }
    }
}

/// Unpacking and moving into place. The rollback flags are set exactly
/// where the original set them: until the previous version is moved into
/// `.bak`, the plugin directory is NOT ours yet, and it must not be touched
/// on failure.
fn unpack_into_place(
    zip_path: &str,
    plan: &eu::PluginInstallPlan,
    extract_root: &str,
    moved_to_bak: &mut bool,
    target_created: &mut bool,
) -> Result<(), String> {
    std::fs::create_dir_all(&plan.root).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(extract_root).map_err(|e| e.to_string())?;
    extract_zip(zip_path, extract_root)?;
    if Path::new(&plan.bak).exists() {
        rmtree_quiet(&plan.bak);
    }
    if Path::new(&plan.target).exists() {
        std::fs::rename(&plan.target, &plan.bak).map_err(|e| e.to_string())?;
        *moved_to_bak = true;
    }
    // From this moment the target is OURS: it may be removed on rollback.
    *target_created = true;
    if std::fs::rename(extract_root, &plan.target).is_err() {
        // Staging on another file system (usually this is the case: the temporary
        // directory is in /tmp, the plugins in $HOME) — we copy the tree.
        copy_tree(Path::new(extract_root), Path::new(&plan.target))?;
    }
    Ok(())
}

/// Unpack the archive, restoring the unix permissions of the entries.
///
/// The permissions are needed by the renderer binaries (the daemon does the
/// same in `apply_zip_mode`), and the mask is set by the core
/// (`zip_entry_mode`). Warning: for archives built not on unix, the `zip`
/// crate infers permissions where the original's `ZipFile` saw zero;
/// waywallen plugins are built on Linux, where the value is the same.
fn extract_zip(zip_path: &str, root: &str) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(rexon_core::i18n::trf("unsafe path in the archive: %1", &[&entry.name()]));
        };
        let out = Path::new(root).join(rel);
        let mode = entry
            .unix_mode()
            .map(|m| eu::zip_entry_mode(m << 16))
            .unwrap_or(0);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut dest = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            // Reading to the end also verifies the entry's CRC: a broken archive fails
            // here, BEFORE the installed version is touched.
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            dest.write_all(&buf).map_err(|e| e.to_string())?;
        }
        if mode != 0 {
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

/// A recursive copy of a tree with permissions — a replacement for
/// `shutil.copytree` for the "staging on another file system" case.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if meta.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
        let _ = std::fs::set_permissions(&to, meta.permissions());
    }
    Ok(())
}

/// A port of `_rmtree_quiet`: remove a file or a tree without interfering
/// with the step's outcome.
fn rmtree_quiet(path: &str) {
    let p = Path::new(path);
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else if p.exists() {
        let _ = std::fs::remove_file(p);
    }
}

/// A port of `_drop_tmp`: remove the worker's temporary file.
pub fn drop_tmp(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = std::fs::remove_file(path);
}

// ============================================================================
// State ~/.config/rexon/dev_upstream_state.json
// ============================================================================

/// Lock for editing the state. Fix 3: the check and the installation
/// changed one file from different threads without any synchronization,
/// and a simultaneous write lost now the report, now the recorded version.
fn state_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// A port of `_read_state_text`: the raw file text (`None` — missing or
/// unreadable).
fn read_state_text(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Read-modify-write the state under the shared lock.
fn edit_state(path: &str, edit: impl FnOnce(&mut Map<String, Value>)) {
    let _guard = state_lock().lock();
    let mut state = eu::load_state(read_state_text(path).as_deref());
    edit(&mut state);
    write_state_text(path, &eu::serialize_state(&state));
}

/// A port of `_write_state_text`: an atomic write through a temporary name
/// from the core.
fn write_state_text(path: &str, text: &str) {
    if let Some(dir) = Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // ⭐ The profile's single atomic door: a temporary file NEXT TO the
    // target, `fsync`, rename. There is no longer a "write + rename" pair of
    // our own here — without `fsync`, after a power failure an intact new state
    // of zeros ended up in place of the intact previous one, and the page
    // would show "the engine was not checked" for a checked engine. The guard
    // is `rexon-core/tests/profile_writes_atomic.rs`.
    let _ = rexon_core::utils::atomic_write(path, text);
}

/// A port of `_now_iso`: the state timestamp in the core's format.
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    eu::now_iso(secs)
}

// ============================================================================
// Temporary files and subprocesses
// ============================================================================

/// Sequence number of the temporary name — so that two tasks of one process
/// do not aim at the same path within a nanosecond.
fn temp_counter() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Tail of a unique name: the time, the process identifier and the counter.
fn temp_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 + d.as_secs())
        .unwrap_or(0);
    format!("{:x}{:x}{:x}", std::process::id(), nanos, temp_counter())
}

/// A port of `tempfile.mkstemp(prefix, suffix)`: create an empty file under
/// our own name. `create_new` — someone else's file with this name will not
/// be hijacked.
fn mktemp_file(prefix: &str, suffix: &str) -> Option<String> {
    let dir = std::env::temp_dir();
    for _ in 0..64 {
        let path = dir.join(format!("{prefix}{}{suffix}", temp_token()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => return Some(path.to_string_lossy().into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// A port of `tempfile.mkdtemp(prefix)`.
fn mktemp_dir(prefix: &str) -> Option<String> {
    let dir = std::env::temp_dir();
    for _ in 0..64 {
        let path = dir.join(format!("{prefix}{}", temp_token()));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
                return Some(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// `subprocess.run(argv, capture_output=True, timeout=…)` — standard output
/// and the error stream. `None` means it did not start or did not fit into
/// the ceiling.
/// Exit code of the process (`None` — did not start or did not finish in
/// time).
async fn run_status(argv: &[String], timeout_secs: u64) -> Option<i32> {
    let (exe, args) = argv.split_first()?;
    let fut = tokio::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut)
        .await
        .ok()?
        .ok()?
        .code()
}

/// Whether the daemon answers on the bus (the `Version` property is
/// readable).
async fn daemon_on_bus() -> bool {
    run_capture(&eu::busctl_version_argv(), eu::BUSCTL_TIMEOUT_SECS)
        .await
        .map(|(stdout, _)| !eu::parse_busctl_version(&stdout).is_empty())
        .unwrap_or(false)
}

/// Stop the engine before swapping its file — by the core's plan
/// (`engine_update::engine_swap_plan`): a service gets
/// `systemctl --user stop`, a daemon outside a service gets its own `Quit`
/// method. `Err` means it could not be stopped, and the file MUST NOT be
/// swapped.
async fn stop_engine_for_swap() -> Result<eu::EngineSwapPlan, String> {
    let unit_active = run_status(
        &eu::engine_unit_active_argv(),
        rexon_core::we_client::SYSTEMCTL_TIMEOUT_SECS,
    )
    .await
        == Some(0);
    let plan = eu::engine_swap_plan(unit_active, daemon_on_bus().await);
    if let Some(argv) = &plan.stop_argv {
        if run_status(argv, rexon_core::we_client::SYSTEMCTL_TIMEOUT_SECS).await != Some(0) {
            return Err(eu::appimage_install_busy_message());
        }
    }
    if plan.quit_daemon {
        let _ = run_status(&eu::busctl_quit_argv(), eu::BUSCTL_TIMEOUT_SECS).await;
        let mut gone = false;
        for _ in 0..eu::QUIT_WAIT_STEPS {
            if !daemon_on_bus().await {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(eu::QUIT_WAIT_STEP_MS)).await;
        }
        if !gone {
            return Err(eu::appimage_install_busy_message());
        }
    }
    Ok(plan)
}

async fn run_capture(argv: &[String], timeout_secs: u64) -> Option<(String, String)> {
    let (exe, args) = argv.split_first()?;
    let fut = tokio::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut)
        .await
        .ok()?
        .ok()?;
    Some((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded reference of the live Python (the `rexon-py` bridge went to
    /// the archive).
    const REFERENCE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/engine_update_reference.json"
    ));

    fn reference() -> Value {
        serde_json::from_str(REFERENCE).expect("эталон engine_update обязан быть валидным JSON")
    }

    /// Messages are compared in the SOURCE language. Since 30.09 (65e9b32) the
    /// texts are English msgids of the catalogue, and Russian comes as a
    /// translation; the Python-era reference was Russian, and the comparison was
    /// green only in a Russian locale with the catalogue installed — and even
    /// then only until a neighbouring comparison in the process switched the
    /// language to English. The process language is shared, so we pin the same
    /// one the crate's neighbours set (`SOURCE_LANGUAGE`).
    fn source_language() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    }

    /// A reference value with the real home directory substituted for the
    /// `<H>` placeholder with which Python recorded it.
    fn expect_str(key: &str, home: &str) -> String {
        reference()[key]
            .as_str()
            .unwrap_or_else(|| panic!("в эталоне нет строки {key}"))
            .replace("<H>", home)
    }

    fn expect_pair(key: &str, home: &str) -> (bool, String) {
        let v = reference();
        let arr = v[key]
            .as_array()
            .unwrap_or_else(|| panic!("в эталоне нет пары {key}"));
        (
            arr[0].as_bool().unwrap_or(false),
            arr[1].as_str().unwrap_or_default().replace("<H>", home),
        )
    }

    struct Sandbox(String);

    impl Sandbox {
        fn new(tag: &str) -> Sandbox {
            let path = std::env::temp_dir().join(format!("rexon-we-upd-{tag}-{}", temp_token()));
            std::fs::create_dir_all(&path).expect("песочница обязана создаться");
            Sandbox(path.to_string_lossy().into_owned())
        }

        fn env(&self) -> UpdateEnv {
            UpdateEnv {
                home: self.0.clone(),
                xdg_config_home: None,
                xdg_data_home: None,
                engine_dir: format!("{}/engine", self.0),
                system_daemon: format!("{}/usr/bin/waywallen", self.0),
                system_engine_dir: format!("{}/usr/share/waywallen", self.0),
            }
        }

        fn join(&self, rel: &str) -> String {
            format!("{}/{rel}", self.0)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            rmtree_quiet(&self.0);
        }
    }

    fn write_exec(path: &str, body: &[u8], mode: u32) {
        if let Some(dir) = Path::new(path).parent() {
            std::fs::create_dir_all(dir).expect("каталог обязан создаться");
        }
        std::fs::write(path, body).expect("файл обязан записаться");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .expect("права обязаны выставиться");
    }

    fn mode_of(path: &str) -> u32 {
        std::fs::metadata(path)
            .expect("файл обязан существовать")
            .permissions()
            .mode()
            & 0o7777
    }

    /// State with a substituted timestamp: the live Python froze it, while we
    /// take the real one — everything else is compared byte for byte.
    fn state_with_frozen_stamp(path: &str) -> String {
        let text = std::fs::read_to_string(path).expect("стейт обязан быть записан");
        let mut out = String::new();
        for line in text.lines() {
            if let Some(head) = line.split_once("_at\": \"") {
                out.push_str(head.0);
                out.push_str("_at\": \"2026-07-25T17:34:56Z\"");
                if line.ends_with(',') {
                    out.push(',');
                }
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        out.trim_end_matches('\n').to_string()
    }

    // ── Downloading from a local path (reference dl_*) ──────────────────

    #[test]
    fn download_local_path_matches_reference() {
        let sb = Sandbox::new("dl");
        let src = sb.join("local.bin");
        std::fs::write(&src, b"LOCAL").expect("исходник обязан записаться");
        let cancel = net::CancelToken::new();

        let spec = eu::download_spec(eu::DownloadKind::AppImage);
        let p = download_asset(&src, &spec, &cancel, |_, _| {}).expect("копия обязана получиться");
        let name = Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let dl_app = reference()["dl_app"].clone();
        assert!(name.starts_with(dl_app[0].as_str().unwrap()));
        assert!(name.ends_with(dl_app[1].as_str().unwrap()));
        assert_eq!(format!("{:#o}", mode_of(&p)).replace("0o", "0o"), "0o755");
        assert_eq!(std::fs::read(&p).unwrap(), b"LOCAL");
        drop_tmp(&p);

        let spec = eu::download_spec(eu::DownloadKind::PluginZip);
        let p = download_asset(&src, &spec, &cancel, |_, _| {}).expect("копия обязана получиться");
        let name = Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let dl_zip = reference()["dl_zip"].clone();
        assert!(name.starts_with(dl_zip[0].as_str().unwrap()));
        assert!(name.ends_with(dl_zip[1].as_str().unwrap()));
        assert_eq!(mode_of(&p), 0o644);
        drop_tmp(&p);

        // dl_empty: an empty address — we create nothing.
        assert!(download_asset("", &spec, &cancel, |_, _| {}).is_none());
    }

    // ── Installing the AppImage (reference ia_*) ────────────────────────

    #[test]
    fn install_appimage_matches_reference() {
        source_language();
        let sb = Sandbox::new("ia");
        let env = sb.env();
        let dest = env.managed_appimage();

        // ia_fresh: a clean installation.
        let src = sb.join("new.AppImage");
        write_exec(&src, b"NEW", 0o755);
        let got = install_appimage(&env, &src, "v0.2.5");
        assert_eq!(got, expect_pair("ia_fresh", &sb.0));
        assert_eq!(mode_of(&dest), 0o755);
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEW");
        assert!(!Path::new(&src).exists(), "ia_src_removed");
        assert_eq!(
            state_with_frozen_stamp(&env.state_path()),
            expect_str("ia_fresh_state", &sb.0)
        );

        // ia_over: over the previous one — the previous one goes to .bak.
        let src2 = sb.join("new2.AppImage");
        write_exec(&src2, b"NEWER", 0o755);
        let got = install_appimage(&env, &src2, "0.2.6");
        assert_eq!(got, expect_pair("ia_over", &sb.0));
        assert_eq!(std::fs::read(format!("{dest}.bak-update")).unwrap(), b"NEW");
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEWER");
        assert_eq!(
            state_with_frozen_stamp(&env.state_path()),
            expect_str("ia_over_state", &sb.0)
        );

        // ia_missing: there is no source — a rollback, no garbage remains.
        let (ok, msg) = install_appimage(&env, &sb.join("nope"), "v1");
        assert!(!ok);
        assert_eq!(msg.split(':').next().unwrap(), "installation failed, rolled back");
        assert!(
            !Path::new(&format!("{dest}.new-update")).exists(),
            "ia_no_tmp_left"
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEWER", "ia_rollback_dest");
    }

    // ── AppImage validation without launching (reference va_missing/va_noexec) ─

    #[test]
    fn validate_appimage_file_checks_match_reference() {
        source_language();
        let sb = Sandbox::new("va");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("рантайм проверки обязан создаться");

        let missing = sb.join("absent");
        assert_eq!(
            rt.block_on(validate_appimage(&missing)),
            expect_pair("va_missing", &sb.0)
        );

        let noexec = sb.join("noexec");
        write_exec(&noexec, b"x", 0o644);
        assert_eq!(
            rt.block_on(validate_appimage(&noexec)),
            expect_pair("va_noexec", &sb.0)
        );
    }

    // ── Installing the plugin zip (reference ipz_*) ─────────────────────

    const MANIFEST_OK: &str = "[plugin]\n\
id = \"org.waywallen.open-wallpaper-engine\"\n\
name = \"Open Wallpaper Engine\"\n\
version = \"0.1.10\"\n";

    fn make_zip(path: &str, entries: &[(&str, &str, u32)]) -> String {
        let file = std::fs::File::create(path).expect("архив обязан создаться");
        let mut zw = zip::ZipWriter::new(file);
        for (name, body, mode) in entries {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(*mode);
            zw.start_file(*name, opts).expect("запись обязана начаться");
            zw.write_all(body.as_bytes()).expect("тело обязано лечь");
        }
        zw.finish().expect("архив обязан закрыться");
        path.to_string()
    }

    #[test]
    fn install_plugin_zip_fs_matches_reference() {
        source_language();
        let sb = Sandbox::new("ipz");
        let env = sb.env();
        // The reference was captured with plugins_root = <H>/work/plugins.
        let proot = sb.join("work/plugins");
        let zp = make_zip(
            &sb.join("install.zip"),
            &[
                ("plugin.toml", MANIFEST_OK, 0o644),
                ("files.txt", "bin/renderer\n", 0o644),
                ("bin/renderer", "#!/bin/sh\necho hi\n", 0o755),
            ],
        );

        let got = install_plugin_zip_fs(&env, &zp, "v0.2.0", &proot);
        assert_eq!(got, expect_pair("ipz_fresh", &sb.0));

        let tgt = format!("{proot}/{}", eu::OWE_PLUGIN_ID);
        let mut tree: Vec<String> = Vec::new();
        collect_files(Path::new(&tgt), Path::new(&tgt), &mut tree);
        tree.sort();
        let want: Vec<String> = reference()["ipz_tree"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(tree, want, "ipz_tree");
        assert_eq!(mode_of(&format!("{tgt}/bin/renderer")), 0o755, "ipz_mode");
        assert_eq!(
            state_with_frozen_stamp(&env.state_path()),
            expect_str("ipz_state", &sb.0)
        );

        // ipz_over: a repeated installation — the previous version into
        // .bak-update.
        let got = install_plugin_zip_fs(&env, &zp, "0.2.1", &proot);
        assert_eq!(got, expect_pair("ipz_over", &sb.0));
        assert!(
            Path::new(&format!("{tgt}.bak-update")).is_dir(),
            "ipz_bak"
        );

        // ipz_invalid: an archive without plugin.toml.
        let bad = make_zip(&sb.join("bad.zip"), &[("files.txt", "x", 0o644)]);
        assert_eq!(
            install_plugin_zip_fs(&env, &bad, "1", &proot),
            expect_pair("ipz_invalid", &sb.0)
        );

        // ipz_broken: validation passes, but unpacking fails on the CRC — the
        // working installed plugin must SURVIVE.
        let broken = sb.join("corrupt.zip");
        make_stored_zip(
            &broken,
            &[
                ("plugin.toml", MANIFEST_OK),
                ("files.txt", "bin/renderer\n"),
                ("bin/renderer", "RENDERER-PAYLOAD-XYZ\n"),
            ],
        );
        corrupt_payload(&broken, b"RENDERER-PAYLOAD-XYZ", b"ZZZZ");
        assert!(
            eu::validate_plugin_zip(Path::new(&broken)).ok,
            "ipz_broken_valid"
        );
        let (ok, _) = install_plugin_zip_fs(&env, &broken, "0.2.2", &proot);
        assert!(!ok, "ipz_broken");
        assert!(
            Path::new(&format!("{tgt}/plugin.toml")).is_file(),
            "ipz_survived"
        );
    }

    /// A tree copy survives a move between file systems — the branch that in a
    /// live installation is enabled by `/tmp` in RAM.
    #[test]
    fn copy_tree_keeps_exec_bit() {
        let sb = Sandbox::new("tree");
        write_exec(&sb.join("src/bin/renderer"), b"#!/bin/sh\n", 0o755);
        write_exec(&sb.join("src/plugin.toml"), MANIFEST_OK.as_bytes(), 0o644);
        copy_tree(Path::new(&sb.join("src")), Path::new(&sb.join("dst")))
            .expect("копия дерева обязана удаться");
        assert_eq!(mode_of(&sb.join("dst/bin/renderer")), 0o755);
        assert_eq!(mode_of(&sb.join("dst/plugin.toml")), 0o644);
    }

    /// State marks — the same keys and the same format as the original's.
    #[test]
    fn state_marks_roundtrip() {
        let sb = Sandbox::new("state");
        let env = sb.env();
        let tasks = WeUpdateTasks::new(env.clone()).expect("рантайм обязан подняться");
        tasks.mark_seen("owe", "v0.1.11");
        tasks.mark_installed_tag("owe", "v0.1.12");
        let state = eu::load_state(read_state_text(&env.state_path()).as_deref());
        assert_eq!(eu::state_field(&state, "owe", "seen_version"), "v0.1.11");
        assert_eq!(
            eu::state_field(&state, "owe", "installed_version"),
            "0.1.12"
        );
        tasks.shutdown();
    }

    fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_files(root, &p, out);
            } else if let Ok(rel) = p.strip_prefix(root) {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }

    fn make_stored_zip(path: &str, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).expect("архив обязан создаться");
        let mut zw = zip::ZipWriter::new(file);
        for (name, body) in entries {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644);
            zw.start_file(*name, opts).expect("запись обязана начаться");
            zw.write_all(body.as_bytes()).expect("тело обязано лечь");
        }
        zw.finish().expect("архив обязан закрыться");
    }

    fn corrupt_payload(path: &str, needle: &[u8], patch: &[u8]) {
        let mut raw = std::fs::read(path).expect("архив обязан читаться");
        let at = raw
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("подпись обязана найтись в архиве без сжатия");
        raw[at..at + patch.len()].copy_from_slice(patch);
        std::fs::write(path, raw).expect("архив обязан переписаться");
    }

    // ── Live release check ──────────────────────────────────────────────

    /// A real trip to GitHub for the releases of both targets.
    ///
    /// Run: `cargo test -p rexon-gui --lib upstream_check_live -- --ignored
    /// --nocapture`. Separate from the ordinary run — the network is not
    /// mandatory in tests; this test cannot install the engine and does not
    /// try.
    #[test]
    #[ignore = "needs the internet"]
    fn upstream_check_live() {
        let sb = Sandbox::new("live");
        let env = sb.env();
        let tasks = WeUpdateTasks::new(env).expect("рантайм обязан подняться");
        assert!(tasks.start_check(String::new()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        let mut statuses = None;
        while std::time::Instant::now() < deadline {
            if let Some(UpdateEvent::CheckDone(v)) = tasks.poll_event() {
                statuses = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        tasks.shutdown();
        let statuses = statuses.expect("проверка обязана ответить");
        assert_eq!(statuses.len(), 2, "целей ровно две");
        for st in &statuses {
            println!(
                "{}: ok={} наша={:?} последняя={:?} обновление={} ассет={:?}\n  {}",
                st.key, st.ok, st.installed, st.latest, st.update_available, st.asset_url, st.note
            );
            assert!(st.ok, "цель {} не проверилась: {}", st.key, st.note);
            assert!(!st.latest.is_empty(), "у {} нет последнего тега", st.key);
        }
    }
}
