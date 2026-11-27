// Background tasks of the "Steam" tab — a port of five `QThread`s from
// `rexon/qt_steam/window.py` to async (tokio).
//
// The original kept five QThread subclasses, and each lived on its own:
//
//   1. `_SplashMetaWorker`  — the whole public inventory → video splash
//                             markers → disk cache → the `done(meta)` signal;
//   2. `_SplashPosterWorker`— clip preview frames (ffprobe + ffmpeg) →
//                             the `ready(id, png)` signal for each one ready;
//   3. `_InventoryWorker`   — owned backgrounds through WebAPI, otherwise the
//                             whole public inventory → `done(items, err)`;
//   4. `_ApplyBgWorker`     — download a background and apply it as a
//                             wallpaper → the `done(ok, msg)` signal;
//   5. `_ThumbLoader`       — card thumbnails of the current page →
//                             the `loaded(id, path)` signal for each one.
//
// Here these are five tasks of one runtime, and the Qt signals are replaced
// with one event queue ([`SteamEvent`]): the page owner takes them from its
// own thread ([`SteamTasks::poll_event`]) and lays them out into the model.
// A queue rather than callbacks — so that data crosses the thread boundary
// in exactly one place.
//
// Warning: CANCELLATION. In Qt a thread could be abandoned; async needs an
// explicit token, and here it is a TREE: the tab root → the showcase node →
// the task. Closing the tab cancels the root, changing the background page
// only the thumbnails node, rebuilding the showcase only the posters node.
// The token reaches the request itself too: the core's synchronous ureq can
// abort an already sent request (`rexon_core::net::CancelToken`), and our
// node triggers it.
//
// Warning: fixes of the original (rule #16), each marked `Fix:` in place:
//   1. DOUBLE INVENTORY DOWNLOAD. `_InventoryWorker` and `_SplashMetaWorker`
//      each pulled the same public inventory on their own (up to 20 pages of
//      2000 items — twice on every tab opening; the owner has 2029 items).
//      Now the pages are downloaded ONCE and feed both showcases;
//   2. ORPHANED THREADS. `_render_bg_page` called `stop()` on the previous
//      `_ThumbLoader` and immediately REPLACED the reference to it with a new
//      one — nobody waited for the previous thread any more. With fast paging
//      a bunch of live threads remained, while `closeEvent` waited only for
//      the last one: hence the "QThread: Destroyed while thread is still
//      running" crash. Here all tasks are counted by a `TaskTracker`, and
//      closing waits for each one;
//   3. CLOSING HUNG FOR UP TO 30 SECONDS. The original's `stop()` was a flag
//      checked BETWEEN requests, so `th.wait(30000)` in `closeEvent` honestly
//      waited for the end of the network timeout. The token aborts the
//      request on the spot;
//   4. `_ApplyBgWorker` had no cancellation AT ALL: `done` flew into a
//      closing window. Now applying sits on the tab's shared token, and the
//      result is not delivered into a closed tab;
//   5. THUMBNAILS WERE DOWNLOADED ONE AT A TIME. The page's 25 files went
//      strictly sequentially. Now up to `THUMB_CONCURRENCY` at once — the
//      order of appearance is no longer the list order, but the cards filled
//      in as they became ready before too, not in rows;
//   6. A PARTIALLY DOWNLOADED `.part` stayed in the cache on any failure
//      (`_download_community` created it and did not remove it). Now the
//      temporary file is removed on any outcome.
//
// What is NOT here: decisions. What to show, in what order and with what
// words is answered by `rexon_core::qt_steam`; side actions that do not
// exist in Rust yet (the wallpaper engine, extracting the WebAPI token from
// the Steam cookie) come as objects from outside.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rexon_core::i18n::tr;
use rexon_core::net::{self, NetIo, UreqNet};
use rexon_core::qt_steam as qs;
use rexon_core::session_splash as ss;
use rexon_core::steam_profile;
use rexon_core::steam_profile_items::{self as spi, ProfileBackgroundItem};
use rexon_core::steam_token;
use rexon_core::wallpaper_exec::{self, ExecSlot, LiveSys, WpSys};
use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// How many thumbnails we download at once. Fix #5: the original downloaded
/// one at a time, and a page of 25 cards filled with exactly 25 sequential
/// requests. Six is a compromise: noticeably faster and still not looking
/// like a raid on Steam's CDN.
pub const THUMB_CONCURRENCY: usize = 6;

/// How long we wait for the tasks to finish when the tab closes. This is a
/// CEILING, not a planned wait: the token is already triggered, the tasks
/// abort their requests themselves, and normally the wait takes
/// milliseconds. The original waited up to 30 seconds AND WITHOUT
/// cancellation.
pub const SHUTDOWN_GRACE_MS: u64 = 2000;

/// Worker threads of the runtime. There are few tasks and all of them wait
/// for the network or ffmpeg, so a couple of threads is enough: the heavy
/// work goes to the blocking pool anyway.
const WORKER_THREADS: usize = 2;

// ============================================================================
// Events out
// ============================================================================

/// What the tasks hand to the page owner. A replacement for the signals of
/// the five QThreads.
#[derive(Debug, Clone, PartialEq)]
pub enum SteamEvent {
    /// `_InventoryWorker.done(items, error)`. An empty error = all is well.
    InventoryDone {
        items: Vec<ProfileBackgroundItem>,
        error: String,
    },
    /// How the request for owned items ended — the only source of animated
    /// backgrounds. Arrives BEFORE `InventoryDone` on every list load;
    /// `AnimatedGap::None` clears the previous reason.
    OwnedAnswer(qs::AnimatedGap),
    /// `_SplashMetaWorker.done(meta)` — video splash markers from the inventory.
    SplashMeta(qs::MetaMap),
    /// `_SplashPosterWorker.ready(id, png)` — a clip's preview frame.
    Poster { id: String, png: String },
    /// `_ThumbLoader.loaded(id, path)` — a background card's thumbnail.
    Thumb { id: String, path: String },
    /// `_ApplyBgWorker.done(ok, msg)`.
    ApplyDone { ok: bool, message: String },
    /// The full-size background for the large viewer was obtained (or not):
    /// `still` is the picture, `movie` the clip of an animated background; an
    /// empty string means this part is missing or did not download.
    Full { id: String, still: String, movie: String },
    /// Adding the person's own clip: the current stage for the progress line.
    MovieImportStage(String),
    /// Adding (or keeping a copy of the old selection) ended: the line for the
    /// status, `ok` — the showcase must be re-read.
    MovieImportDone { ok: bool, message: String },
}

// ============================================================================
// Side actions that do not exist in Rust yet
// ============================================================================

/// The wallpaper engine. The live implementation is [`ExecApplier`] (the
/// core's executor), substitutes are in the reconciliation tests.
///
/// The method is synchronous and returns "whether it worked" together with
/// the text for the status line — exactly what the original took from
/// `res.message`.
pub trait WallpaperApply: Send + Sync {
    fn apply(&self, path: &str, format: &str, audio: bool, name: &str) -> (bool, String);
}

/// The tab's live wallpaper engine: the core's shared executor, with no
/// rules of its own.
///
/// What and how to apply is decided by the core
/// ([`qs::apply_background_with`] → `WallpaperExec::apply_with_audio`); the
/// executor slot is one per process and shared with "Settings"
/// (`wallpaper_exec::live_slot`); applying and supervising the load go
/// through the core's shared door (`wallpaper_exec::run_in_slot`).
///
/// WHY ONE PER PROCESS and not per page: the "Steam" page lives in the main
/// window's `Loader` and is recreated, while the executor remembers which
/// process draws the wallpaper and what to restore after stopping. A page's
/// own executor would not know about a video started by its previous copy
/// or by "Settings", and "Stop" there would not remove what was set here.
pub struct ExecApplier<S: WpSys = LiveSys> {
    slot: ExecSlot<S>,
}

impl ExecApplier {
    /// The engine on the process executor.
    pub fn live() -> ExecApplier {
        ExecApplier { slot: wallpaper_exec::live_slot() }
    }
}

impl<S: WpSys> ExecApplier<S> {
    /// The engine on an explicitly given slot — this is how the reconciliation
    /// tests substitute an executor with a fake seam.
    pub fn on(slot: ExecSlot<S>) -> ExecApplier<S> {
        ExecApplier { slot }
    }
}

impl<S: WpSys + Default + Send + 'static> WallpaperApply for ExecApplier<S> {
    // `format` is not needed: the core derives it from the path with the same
    // function (`qs::apply_format`) that computed it here too. `audio` is the
    // "🔊 Video wallpaper sound" checkbox, passed to the executor as is.
    fn apply(&self, path: &str, _format: &str, audio: bool, name: &str) -> (bool, String) {
        wallpaper_exec::run_in_slot(&self.slot, |exec| {
            qs::apply_background_with(exec, path, audio, name)
        })
    }
}

// ============================================================================
// Environment
// ============================================================================

/// Paths the caches depend on. Taken once on construction: there is no
/// reason to read `$HOME` from a worker task, and the tests substitute their
/// own.
#[derive(Debug, Clone, Default)]
pub struct SteamEnv {
    pub home: String,
    pub xdg_cache_home: Option<String>,
    pub xdg_config_home: Option<String>,
}

impl SteamEnv {
    /// The live process environment.
    pub fn from_process() -> SteamEnv {
        let nonempty = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        SteamEnv {
            home: std::env::var("HOME").unwrap_or_default(),
            xdg_cache_home: nonempty("XDG_CACHE_HOME"),
            xdg_config_home: nonempty("XDG_CONFIG_HOME"),
        }
    }

    fn cache(&self) -> Option<&str> {
        self.xdg_cache_home.as_deref()
    }

    fn config(&self) -> Option<&str> {
        self.xdg_config_home.as_deref()
    }
}

/// An order to load the inventory.
#[derive(Debug, Clone, Default)]
pub struct InventoryRequest {
    /// SteamID64 of the account.
    pub sid: String,
    /// The background equipped in the profile (parsed from `localconfig.vdf` in
    /// advance).
    pub equipped: Option<ProfileBackgroundItem>,
    /// WebAPI token for owned backgrounds, if the page host brought it itself
    /// (reconciliation tests). Empty — the task obtains it itself, in the
    /// blocking pool: `rexon_core::steam_token::access_token` goes to the
    /// network, and it has no place in the interface thread.
    ///
    /// Warning: until 13.10.2026 the token had to COME from the page
    /// (`actions.webapiToken()`), and since 08.09.2026 the method was not
    /// visible from QML (`#[qinvokable]` was lost): the page always started with
    /// an empty token, owned backgrounds were not requested, and there was not a
    /// single animated one in the list. Now the page does not carry the token at
    /// all.
    pub webapi_token: String,
}

// ============================================================================
// Tab runtime
// ============================================================================

/// All background tasks of the "Steam" tab and their cancellation.
pub struct SteamTasks {
    /// Warning: `Option` rather than just `Runtime`, and it is not decoration.
    /// The ordinary `Drop` of a tokio runtime waits for blocking tasks WITHOUT A
    /// LIMIT: one stuck task and the process never exits. Here the runtime is
    /// taken out of the field and dropped with [`Runtime::shutdown_timeout`] —
    /// with a ceiling.
    rt: Option<Runtime>,
    /// Root of the cancellation tree: closing the tab cancels EVERYTHING.
    root: CancellationToken,
    /// Accounting of live tasks — so that closing waits for each one (fix #2).
    tracker: TaskTracker,
    tx: UnboundedSender<SteamEvent>,
    rx: Mutex<UnboundedReceiver<SteamEvent>>,
    /// Cancellation node of the thumbnails: lives exactly until the next page.
    thumbs: Mutex<Option<CancellationToken>>,
    /// Cancellation node of the preview frames: lives exactly until the next
    /// showcase rebuild.
    posters: Mutex<Option<CancellationToken>>,
    /// Whether applying a wallpaper is in progress (the original's
    /// `_wp_worker.isRunning()`).
    applying: Arc<AtomicBool>,
    /// Cancellation node of adding the person's own clip; `Some` while it runs.
    import: Arc<Mutex<Option<CancellationToken>>>,
    env: SteamEnv,
}

impl SteamTasks {
    /// Bring up the tab runtime. `None` means the runtime was not created (in
    /// the live Rexon this means "there will be no background tasks", not a
    /// crash).
    pub fn new(env: SteamEnv) -> Option<SteamTasks> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .thread_name("rexon-steam")
            // Warning: fix: without enable_io `tokio::process` PANICS in a worker
            // thread ("A Tokio 1.x context was found, but IO is disabled"), and the
            // ffprobe and ffmpeg of the preview frame tasks go through it. Caught by a
            // live run of the neighbouring tab, where the same runtime launches
            // subprocesses.
            .enable_io()
            .enable_time()
            .build()
            .ok()?;
        let (tx, rx) = unbounded_channel();
        let root = CancellationToken::new();
        // ⭐ THE SHARED EXIT DOOR (rule #12). The background task drain
        // (`exit_guard`) counts only what it was shown: it does not see the
        // runtime's worker threads and without this entry honestly answered
        // "nobody is left" while the inventory download was running. The request
        // is triggered BEFORE the window is torn down — so the network is aborted
        // before objects the task could touch start being destroyed.
        {
            let root = root.clone();
            rexon_core::exit_guard::register_stopper(move || root.cancel());
        }
        Some(SteamTasks {
            rt: Some(rt),
            root,
            tracker: TaskTracker::new(),
            tx,
            rx: Mutex::new(rx),
            thumbs: Mutex::new(None),
            posters: Mutex::new(None),
            applying: Arc::new(AtomicBool::new(false)),
            import: Arc::new(Mutex::new(None)),
            env,
        })
    }

    // ── Adding the person's own startup clip ──────────────────────────

    /// Whether a clip is being added right now.
    pub fn is_importing(&self) -> bool {
        self.import.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// Add the clip `src` (`migrate` — keep a copy of the OLD selection) in
    /// the blocking pool: ffprobe/ffmpeg and a password window for installing
    /// ffmpeg must not freeze the window. All decisions are the core's
    /// (`Splash::import_movie` / `migrate_movie`); here only the thread, the
    /// cancellation and the events. `false` — something is already being added.
    pub fn start_movie_import(&self, src: String, migrate: bool) -> bool {
        let token = self.root.child_token();
        {
            let Ok(mut slot) = self.import.lock() else {
                return false;
            };
            if slot.is_some() {
                return false;
            }
            *slot = Some(token.clone());
        }
        let tx = self.tx.clone();
        let slot = self.import.clone();
        let home = self.env.home.clone();
        self.spawn(async move {
            let t = token.clone();
            let tx2 = tx.clone();
            let res = tokio::task::spawn_blocking(move || {
                let mut io = rexon_core::splash_exec::LiveSplashIo::new(&home, "");
                let mut splash = ss::Splash::new(&mut io, &home);
                if migrate {
                    splash.migrate_movie(&src)
                } else {
                    let cancelled = || t.is_cancelled();
                    let mut progress = |stage: ss::ImportStage| {
                        let _ = tx2.send(SteamEvent::MovieImportStage(ss::import_stage_text(stage)));
                    };
                    splash.import_movie(&src, &cancelled, &mut progress)
                }
            })
            .await;
            if let Ok(mut g) = slot.lock() {
                *g = None;
            }
            let outcome = match res {
                Ok(o) => o,
                Err(e) => ss::ImportOutcome::failed(ss::msg_import_failed(&e.to_string())),
            };
            // A cancelled addition says so too; into a closed tab the event
            // simply does not arrive.
            let _ = tx.send(SteamEvent::MovieImportDone { ok: outcome.ok, message: outcome.message });
        });
        true
    }

    /// Stop the running addition (the conversion is killed, the partial file removed).
    pub fn cancel_movie_import(&self) {
        if let Ok(g) = self.import.lock() {
            if let Some(t) = g.as_ref() {
                t.cancel();
            }
        }
    }

    /// Take the next event; `None` means the queue is empty. Called from the
    /// interface thread (by the page's timer), never blocks.
    pub fn poll_event(&self) -> Option<SteamEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// Whether applying a wallpaper is in progress right now.
    pub fn is_applying(&self) -> bool {
        self.applying.load(Ordering::SeqCst)
    }

    /// Whether the whole tab is cancelled (closing has already started).
    pub fn is_cancelled(&self) -> bool {
        self.root.is_cancelled()
    }

    // ── Task 1+3: the inventory and the splash markers ─────────────────

    /// A port of `_InventoryWorker` AND `_SplashMetaWorker` as one task.
    ///
    /// Fix #1: the public inventory pages are downloaded ONCE. Then the same
    /// answer is parsed twice — into profile backgrounds
    /// ([`spi::parse_inventory_backgrounds`]) and into video splash markers
    /// ([`qs::splash_item_meta`]), and both events go out as they become ready,
    /// as before with two signals.
    ///
    /// The owned backgrounds path (WebAPI) is kept: only it gives links to
    /// ANIMATION. But it does not give the splash markers, so the public
    /// inventory is pulled in this case too — as in the original, just no
    /// longer twice.
    pub fn start_inventory(&self, req: InventoryRequest) {
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        self.spawn(async move {
            // Owned backgrounds: the only source of animation links.
            // The order of steps and the verdict are the core's
            // (`owned_backgrounds_with`), from here only the key and the network.
            let mut answered = false;
            let given = req.webapi_token.clone();
            let home = env.home.clone();
            let owned = blocking_cancellable(&token, move |cancel| {
                qs::owned_backgrounds_with(
                    |force| {
                        if !given.is_empty() {
                            return (given.clone(), String::new());
                        }
                        if force {
                            steam_token::forget_token();
                        }
                        steam_token::access_token(&home, force)
                    },
                    |key| fetch_owned(key, cancel.clone()),
                )
            })
            .await;
            let Some((owned, gap)) = owned else { return };
            let _ = tx.send(SteamEvent::OwnedAnswer(gap));
            if !owned.is_empty() {
                let items = spi::finalize_items(owned, req.equipped.clone());
                let _ = tx.send(SteamEvent::InventoryDone {
                    items,
                    error: String::new(),
                });
                answered = true;
            }

            // The public inventory: both backgrounds (if the owned ones did not work
            // out) and splash markers — from ONE download.
            let sid = req.sid.clone();
            let pages = blocking_cancellable(&token, move |cancel| {
                fetch_inventory_pages(&sid, cancel)
            })
            .await;
            let Some(pages) = pages else { return };

            let data = match pages {
                Ok(v) => v,
                Err(e) => {
                    if !answered {
                        let _ = tx.send(SteamEvent::InventoryDone {
                            items: Vec::new(),
                            error: e,
                        });
                    }
                    return;
                }
            };
            if token.is_cancelled() {
                return;
            }

            if !answered {
                let eq_id = req.equipped.as_ref().map(|e| e.id.clone());
                let parsed = spi::parse_inventory_backgrounds(&data, eq_id.as_deref());
                let items = spi::finalize_items(parsed, req.equipped.clone());
                let _ = tx.send(SteamEvent::InventoryDone {
                    items,
                    error: String::new(),
                });
            }

            // Splash markers + their disk cache.
            let descs = data
                .get("descriptions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let meta = qs::splash_item_meta(&descs);
            if meta.is_empty() || token.is_cancelled() {
                return;
            }
            let path = qs::splash_meta_cache_path(env.cache(), &env.home, &req.sid);
            let text = qs::meta_cache_json(&meta);
            let _ = tokio::task::spawn_blocking(move || write_atomic(&path, text.as_bytes())).await;
            if !token.is_cancelled() {
                let _ = tx.send(SteamEvent::SplashMeta(meta));
            }
        });
    }

    // ── Task 2: clip preview frames ────────────────────────────────────

    /// A port of `_SplashPosterWorker`. `jobs` are the paths of clips that have
    /// no frame yet. The previous batch is cancelled: the showcase was rebuilt,
    /// nobody needs the old frames any more (fix #2 — the previous task is not
    /// orphaned but cancelled).
    pub fn start_posters(&self, jobs: Vec<String>) {
        let token = self.swap_node(&self.posters);
        if jobs.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let env = self.env.clone();
        self.spawn(async move {
            for path in jobs {
                if token.is_cancelled() {
                    return;
                }
                let env2 = env.clone();
                let p = path.clone();
                let png = tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    out = preview_frame(env2, p) => out,
                };
                if let Some(png) = png {
                    if token.is_cancelled() {
                        return;
                    }
                    let _ = tx.send(SteamEvent::Poster { id: path, png });
                }
            }
        });
    }

    // ── Task 5: background card thumbnails ─────────────────────────────

    /// A port of `_ThumbLoader`. `jobs` are "background id → preview address"
    /// pairs of ONLY the current page. The previous batch is cancelled together
    /// with its node.
    pub fn start_thumbs(&self, jobs: Vec<(String, String)>) {
        let token = self.swap_node(&self.thumbs);
        let jobs: Vec<(String, String)> = jobs.into_iter().filter(|(_, u)| !u.is_empty()).collect();
        if jobs.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let dir = qs::thumb_cache_dir(self.env.cache(), &self.env.home);
        self.spawn(async move {
            let _ = tokio::fs::create_dir_all(&dir).await;
            // Fix #5: up to THUMB_CONCURRENCY at once instead of strictly one at a
            // time.
            let gate = Arc::new(Semaphore::new(THUMB_CONCURRENCY));
            let mut set = tokio::task::JoinSet::new();
            for (id, url) in jobs {
                let gate = gate.clone();
                let token = token.clone();
                let tx = tx.clone();
                let dir = dir.clone();
                set.spawn(async move {
                    let Ok(_permit) = gate.acquire().await else {
                        return;
                    };
                    if token.is_cancelled() {
                        return;
                    }
                    let dest = format!("{dir}/{}", qs::thumb_cache_file_name(&id, &url));
                    let out = blocking_cancellable(&token, move |cancel| {
                        download_cached(&url, &dest, qs::THUMB_DOWNLOAD_TIMEOUT_S, cancel)
                    })
                    .await;
                    // One misfire does not break the list — as in the original.
                    if let Some(Ok(path)) = out {
                        if !token.is_cancelled() {
                            let _ = tx.send(SteamEvent::Thumb { id, path });
                        }
                    }
                });
            }
            // We wait for our subtasks here: otherwise the `TaskTracker` would
            // consider the parent finished while the grandchildren kept downloading.
            while set.join_next().await.is_some() {
                if token.is_cancelled() {
                    set.abort_all();
                }
            }
        });
    }

    // ── Task 6: the full-size background for the large viewer ──────────

    /// Obtain the whole background for the large viewer: the picture
    /// (`still_url`) and, for an animated one, the clip (`movie_url`; empty
    /// means there is no clip).
    ///
    /// The download layer and the caches are the same as for thumbnails and
    /// "Apply": the picture lands in the thumbnail cache (Steam gives it in full
    /// size right away, the background has no separate reduced file), the clip
    /// in the backgrounds cache, from where "Apply" will later take it without
    /// downloading a second time.
    ///
    /// The task sits on the root token, not on the page node: paging the viewer
    /// must not be aborted because the grid moved to another page. The answer
    /// ALWAYS comes — on failure too, otherwise the viewer would show "loading"
    /// forever.
    pub fn start_full(&self, id: String, still_url: String, movie_url: String) {
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let thumb_dir = qs::thumb_cache_dir(self.env.cache(), &self.env.home);
        let bg_dir = qs::bg_cache_dir(self.env.cache(), &self.env.home);
        self.spawn(async move {
            let mut still = String::new();
            let mut movie = String::new();
            if !still_url.is_empty() {
                let dest = format!("{thumb_dir}/{}", qs::thumb_cache_file_name(&id, &still_url));
                let out = blocking_cancellable(&token, move |cancel| {
                    download_cached(&still_url, &dest, qs::THUMB_DOWNLOAD_TIMEOUT_S, cancel)
                })
                .await;
                if let Some(Ok(path)) = out {
                    still = path;
                }
            }
            if !movie_url.is_empty() && !token.is_cancelled() {
                let dest = format!("{bg_dir}/{}", qs::bg_cache_file_name(&id, &movie_url));
                let out = blocking_cancellable(&token, move |cancel| {
                    download_cached(&movie_url, &dest, qs::BG_DOWNLOAD_TIMEOUT_S, cancel)
                })
                .await;
                if let Some(Ok(path)) = out {
                    movie = path;
                }
            }
            if !token.is_cancelled() {
                let _ = tx.send(SteamEvent::Full { id, still, movie });
            }
        });
    }

    // ── Task 4: download a background and apply it as a wallpaper ──────

    /// A port of `_ApplyBgWorker`. `false` means we are already applying (the
    /// original's answer "⏳ Already applying the wallpaper — please wait").
    ///
    /// Fix #4: the task has cancellation. It sits on the tab's root token —
    /// applying a wallpaper must not be aborted by a page change, but when the
    /// tab closes nobody needs the result any more.
    pub fn apply_background(
        &self,
        item: ProfileBackgroundItem,
        audio: bool,
        applier: Arc<dyn WallpaperApply>,
    ) -> bool {
        if self.applying.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let busy = self.applying.clone();
        let dir = qs::bg_cache_dir(self.env.cache(), &self.env.home);
        self.spawn(async move {
            let _ = tokio::fs::create_dir_all(&dir).await;
            let dest = format!("{dir}/{}", qs::bg_cache_file_name(&item.id, &item.url));
            let url = item.url.clone();
            let dest2 = dest.clone();
            let downloaded = blocking_cancellable(&token, move |cancel| {
                download_cached(&url, &dest2, qs::BG_DOWNLOAD_TIMEOUT_S, cancel)
            })
            .await;

            let (ok, message) = match downloaded {
                None => {
                    // The tab was closed — leave silently without bothering anyone.
                    busy.store(false, Ordering::SeqCst);
                    return;
                }
                Some(Err(e)) => (false, e),
                Some(Ok(path)) => {
                    let fmt = qs::apply_format(&path);
                    let name = item.name.clone();
                    let res = tokio::task::spawn_blocking(move || {
                        applier.apply(&path, fmt, audio, &name)
                    })
                    .await;
                    match res {
                        Ok(v) => v,
                        Err(e) => (false, e.to_string()),
                    }
                }
            };
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(SteamEvent::ApplyDone { ok, message });
            }
        });
    true
    }

    // ── Closing ────────────────────────────────────────────────────────

    /// Cancel ALL tasks and wait for them — a port of `closeEvent`.
    ///
    /// The original waited up to 30 seconds because it had no cancellation.
    /// Here the token is triggered first, the tasks abort their requests
    /// themselves, and the wait is a safety net with the ceiling
    /// [`SHUTDOWN_GRACE_MS`].
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

    // ── Internals ──────────────────────────────────────────────────────

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

    /// Replace the showcase's cancellation node: cancel the previous batch,
    /// start a new one.
    fn swap_node(&self, slot: &Mutex<Option<CancellationToken>>) -> CancellationToken {
        let fresh = self.root.child_token();
        if let Ok(mut cur) = slot.lock() {
            if let Some(old) = cur.replace(fresh.clone()) {
                old.cancel();
            }
        }
        fresh
    }
}

impl Drop for SteamTasks {
    /// Warning: THE CEILING HERE IS MANDATORY, otherwise Rexon does not close.
    ///
    /// The ordinary `Drop` of a tokio runtime waits for blocking tasks WITHOUT A
    /// LIMIT — that is exactly why on 19.09.2026 the main window self-test did
    /// not finish on its own in 9 runs out of 12: the Steam inventory download
    /// got stuck in a network read, the window teardown ran into it, and the
    /// watchdog finished the process off. The root cause of that hang was cured
    /// in the core (`rexon_core::net`), but the dependency "one stuck task = a
    /// process alive forever" did not go anywhere, and it has to be closed
    /// here: a user who closed the window must get a closed window, whatever
    /// the background task is busy with.
    ///
    /// This abandons the remaining task as is — it spoils nothing: tasks reach
    /// the outside only through the event queue, and sending into a closed
    /// queue simply does not arrive.
    fn drop(&mut self) {
        self.root.cancel();
        self.tracker.close();
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(std::time::Duration::from_millis(SHUTDOWN_GRACE_MS));
        }
    }
}

// ============================================================================
// Cancellation: a bridge from the tokio token to the core network layer token
// ============================================================================

/// Run synchronous work in the blocking pool so that it CAN be aborted: a
/// [`net::CancelToken`] is handed out, triggered as soon as `token` is
/// cancelled.
///
/// `None` means the tab is cancelled (nobody needs the result) or the task
/// crashed.
///
/// There is nothing Steam-specific here: the same bridge is used by the
/// background tasks of the wallpaper tab's "Developer" page
/// (`we_update_workers.rs`). There must not be a second copy of this
/// linkage in the crate (rule #12), so it is open to the whole crate rather
/// than private to the module.
pub(crate) async fn blocking_cancellable<T, F>(token: &CancellationToken, f: F) -> Option<T>
where
    F: FnOnce(net::CancelToken) -> T + Send + 'static,
    T: Send + 'static,
{
    if token.is_cancelled() {
        return None;
    }
    let net_cancel = net::CancelToken::new();
    // The watcher lives exactly as long as the work itself: as soon as it
    // finishes, `done_tx` is dropped and the watcher leaves. Without this
    // watchers would pile up, one per request.
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let nc = net_cancel.clone();
        let t = token.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = t.cancelled() => nc.cancel(),
                _ = done_rx => {}
            }
        });
    }
    let out = tokio::task::spawn_blocking(move || f(net_cancel)).await.ok();
    drop(done_tx);
    if token.is_cancelled() {
        None
    } else {
        out
    }
}

/// The core's network transport with cancellation already attached.
fn net_with(cancel: net::CancelToken) -> UreqNet {
    UreqNet::new().with_cancel(cancel)
}

// ============================================================================
// Network: the inventory, owned backgrounds, downloading assets
// ============================================================================

/// A port of `_fetch_inventory_pages`: the WHOLE public inventory as one
/// answer.
///
/// Pages go on while Steam says `more_items` (a fix of the original of
/// 2026-07-28 — the former code took one page and lost everything recent).
fn fetch_inventory_pages(sid: &str, cancel: net::CancelToken) -> Result<Value, String> {
    let mut net = net_with(cancel.clone());
    let mut pages: Vec<Value> = Vec::new();
    let mut start = String::new();
    for _ in 0..qs::INVENTORY_MAX_PAGES {
        if cancel.is_cancelled() {
            return Err(String::new());
        }
        let url = qs::inventory_page_url(sid, &start);
        let body = net
            .get(&url, qs::USER_AGENT, qs::INVENTORY_TIMEOUT_S)
            .map_err(|e| e.human())?;
        let page: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let next = qs::inventory_next_start(&page);
        pages.push(page);
        match next {
            Some(v) => start = v,
            None => break,
        }
    }
    Ok(qs::merge_inventory_pages(&pages))
}

/// A port of `_fetch_owned` from `rexon/steam_profile_items.py`: owned
/// backgrounds with links to the animation.
///
/// The refusal is distinguished by kind: "key not accepted" (the core will
/// re-read the key and retry) and "did not answer". The network error text
/// does NOT go out: it contains the request address, and the address
/// contains the key.
fn fetch_owned(
    token: &str,
    cancel: net::CancelToken,
) -> Result<Vec<ProfileBackgroundItem>, qs::OwnedError> {
    let url = format!(
        "https://api.steampowered.com/IPlayerService/GetProfileItemsOwned/v1/\
?access_token={}&language={}",
        percent_encode(token),
        // The language of names is the interface language (previously it was
        // hard-wired to `russian`).
        qs::steam_language()
    );
    let mut net = net_with(cancel);
    let body = net
        .get(&url, qs::USER_AGENT, qs::INVENTORY_TIMEOUT_S)
        .map_err(|e| {
            qs::owned_error_of_status(match e {
                net::NetError::Status { code, .. } => Some(code),
                net::NetError::Transport { .. } => None,
            })
        })?;
    let data = serde_json::from_slice::<Value>(&body).map_err(|_| qs::OwnedError::NoAnswer)?;
    Ok(steam_profile::parse_owned_backgrounds(&data)
        .into_iter()
        .map(|b| ProfileBackgroundItem {
            id: b.id,
            name: b.name,
            game: b.game,
            url: b.url,
            preview: b.preview,
            animated: b.animated,
            equipped: false,
        })
        .collect())
}

/// A port of `_download_community` + the cache checks from
/// `_fetch`/`_download`.
///
/// The file is already in place and non-empty — return it without touching
/// the network. Otherwise download, trying the CDN host variants (Steam rot:
/// the same path may exist on only one of the two).
///
/// Fix #6: the temporary `.part` is removed on ANY outcome.
pub(crate) fn download_cached(
    url: &str,
    dest: &str,
    timeout: u64,
    cancel: net::CancelToken,
) -> Result<String, String> {
    if std::fs::metadata(dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(dest.to_string());
    }
    if let Some(dir) = dest.rsplit_once('/').map(|(d, _)| d) {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = qs::part_path(dest);
    let mut net = net_with(cancel.clone());
    // TRANSLATORS: why a Steam background could not be downloaded: Steam
    // gave no download address for it.
    let mut last = tr("no download address");
    for candidate in spi::community_url_variants(url) {
        if cancel.is_cancelled() {
            let _ = std::fs::remove_file(&tmp);
            return Err(String::new());
        }
        match net.get(&candidate, qs::USER_AGENT, timeout) {
            Ok(body) => {
                if let Err(e) = std::fs::write(&tmp, &body) {
                    last = e.to_string();
                    continue;
                }
                if let Err(e) = std::fs::rename(&tmp, dest) {
                    last = e.to_string();
                    continue;
                }
                return Ok(dest.to_string());
            }
            Err(e) => last = e.human(),
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last)
}

/// `urllib.parse.quote` for the token in the address.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ============================================================================
// Clip preview frame (ffprobe + ffmpeg)
// ============================================================================

/// A port of `session_splash.preview_frame` for an async task.
///
/// The decisions come entirely from the core: the cache directory, the file
/// name by "path + modification time", the frame moment, both command lines
/// and the "first frame" retry for short/broken clips. Here there is only
/// launching the processes.
async fn preview_frame(env: SteamEnv, video: String) -> Option<String> {
    if video.is_empty() || !tokio::fs::try_exists(&video).await.unwrap_or(false) {
        return None;
    }
    let stamp = tokio::fs::metadata(&video)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let abs = std::fs::canonicalize(&video)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| video.clone());
    let cache_dir = ss::preview_cache_dir(env.cache(), &env.home);
    let out = ss::preview_cache_path(&cache_dir, &abs, stamp);
    if tokio::fs::metadata(&out)
        .await
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        return Some(out);
    }

    let ffmpeg = which("ffmpeg")?;
    tokio::fs::create_dir_all(&cache_dir).await.ok()?;

    let seek = video_duration(&video).await * ss::PREVIEW_AT;
    let tmp = format!("{out}{}", ss::PREVIEW_TMP_SUFFIX);
    let ok = run_argv(
        &ss::preview_ffmpeg_argv(&ffmpeg, &video, &tmp, seek),
        ss::FFMPEG_TIMEOUT_SEC,
    )
    .await
    .map(|(rc, _)| rc == 0)
    .unwrap_or(false)
        && tokio::fs::try_exists(&tmp).await.unwrap_or(false);
    if !ok {
        // The clip is shorter than expected or broken — take the very first frame.
        let second = run_argv(
            &ss::preview_ffmpeg_argv(&ffmpeg, &video, &tmp, 0.0),
            ss::FFMPEG_TIMEOUT_SEC,
        )
        .await
        .map(|(rc, _)| rc == 0)
        .unwrap_or(false)
            && tokio::fs::try_exists(&tmp).await.unwrap_or(false);
        if !second {
            let _ = tokio::fs::remove_file(&tmp).await;
            return None;
        }
    }
    tokio::fs::rename(&tmp, &out).await.ok()?;
    Some(out)
}

/// Clip duration via ffprobe; without ffprobe it is 0 (the frame is taken
/// from the start, as in the original).
async fn video_duration(path: &str) -> f64 {
    let Some(exe) = which("ffprobe") else {
        return 0.0;
    };
    match run_argv(
        &ss::ffprobe_duration_argv(&exe, path),
        ss::FFPROBE_TIMEOUT_SEC,
    )
    .await
    {
        Some((_, stdout)) => ss::parse_duration(&stdout),
        None => 0.0,
    }
}

/// `subprocess.run(argv, capture_output=True, timeout=…)` — the exit code
/// and standard output. `None` means it did not start or did not fit into
/// the timeout.
async fn run_argv(argv: &[String], timeout: f64) -> Option<(i32, String)> {
    let (exe, args) = argv.split_first()?;
    // `kill_on_drop`: the frame order is cancelled on every showcase update,
    // and the timeout drops the future — without this ffmpeg stayed alive as an
    // orphan and wrote into a TEMPORARY file that the next order is already
    // writing.
    let fut = tokio::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs_f64(timeout), fut)
        .await
        .ok()?
        .ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// `shutil.which` — the first executable file with this name in `$PATH`.
fn which(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate = format!("{dir}/{name}");
        if std::fs::metadata(&candidate).map(|m| m.is_file()).unwrap_or(false) {
            return Some(candidate);
        }
    }
    None
}

// ============================================================================
// Live file system reads (the executor side)
// ============================================================================

/// A port of `_steam_id64`: SteamID64 of the account whose
/// `localconfig.vdf` is newer.
pub fn detect_account(steam_root: &str) -> Option<String> {
    let userdata = format!("{steam_root}/userdata");
    let mut entries: Vec<(String, f64)> = Vec::new();
    for e in std::fs::read_dir(&userdata).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let cfg = format!("{userdata}/{name}/config/localconfig.vdf");
        let Ok(meta) = std::fs::metadata(&cfg) else {
            continue;
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        entries.push((name, mtime));
    }
    qs::pick_account(&entries)
}

/// A port of `_equipped_profile_background`: the background equipped in the
/// profile from `localconfig.vdf` (may be animated).
pub fn read_equipped_background(steam_root: &str) -> Option<ProfileBackgroundItem> {
    let userdata = format!("{steam_root}/userdata");
    for e in std::fs::read_dir(&userdata).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let cfg = format!("{userdata}/{name}/config/localconfig.vdf");
        let Ok(bytes) = std::fs::read(&cfg) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let Some(raw) = qs::extract_equipped_json(&text) else {
            continue;
        };
        let Ok(data) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if let Some(item) = spi::parse_equipped_background(&data) {
            return Some(item);
        }
    }
    None
}

/// Disk cache of the inventory LIST: show the showcase INSTANTLY while the
/// network quietly updates it in the background (the original's
/// stale-while-revalidate).
pub fn read_inventory_cache(env: &SteamEnv, sid: &str) -> Option<Vec<ProfileBackgroundItem>> {
    let path = qs::inventory_cache_path(env.cache(), &env.home, sid);
    let text = std::fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    qs::parse_inventory_cache(&data, sid)
}

/// Atomic write of the inventory list cache (`.tmp` + `rename`).
pub fn write_inventory_cache(env: &SteamEnv, sid: &str, items: &[ProfileBackgroundItem]) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let path = qs::inventory_cache_path(env.cache(), &env.home, sid);
    write_atomic(&path, qs::inventory_cache_json(sid, items, ts).as_bytes());
}

/// Video splash markers from the disk cache — instantly, without the
/// network.
pub fn read_meta_cache(env: &SteamEnv, sid: &str) -> qs::MetaMap {
    let path = qs::splash_meta_cache_path(env.cache(), &env.home, sid);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| qs::parse_meta_cache(&v))
        .unwrap_or_default()
}

/// id of the background currently applied as a wallpaper (an empty string
/// means there is none).
pub fn read_applied_id(env: &SteamEnv) -> String {
    let path = qs::applied_config_path(env.config(), &env.home);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| qs::parse_applied_config(&v))
        .unwrap_or_default()
}

/// Remember which background is applied as a wallpaper (the wallpaper engine does not know the Steam item).
pub fn write_applied(env: &SteamEnv, id: &str, name: &str) {
    let path = qs::applied_config_path(env.config(), &env.home);
    write_atomic(&path, qs::applied_config_json(id, name).as_bytes());
}

/// Writing through the profile's SINGLE atomic door
/// ([`rexon_core::utils::atomic_write_bytes`]).
///
/// There is no longer a "temporary file + rename" pair of our own here:
/// without `fsync` the name swap may reach the disk before the contents,
/// and after a power failure an intact new record of zeros ends up in place
/// of the intact previous one. The guard of the shared door is
/// `rexon-core/tests/profile_writes_atomic.rs`.
fn write_atomic(path: &str, data: &[u8]) {
    if let Some((dir, _)) = path.rsplit_once('/') {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let _ = rexon_core::utils::atomic_write_bytes(path, data);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_at(dir: &std::path::Path) -> SteamEnv {
        SteamEnv {
            home: dir.to_string_lossy().into_owned(),
            xdg_cache_home: Some(format!("{}/cache", dir.to_string_lossy())),
            xdg_config_home: Some(format!("{}/config", dir.to_string_lossy())),
        }
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rexon-steam-workers-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Closing the tab must not hang: the token is triggered first, and the
    /// wait hits the ceiling. The original waited up to 30 seconds.
    #[test]
    fn shutdown_is_prompt_and_idempotent() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        let started = std::time::Instant::now();
        tasks.shutdown();
        assert!(tasks.is_cancelled());
        assert!(started.elapsed().as_millis() < SHUTDOWN_GRACE_MS as u128 + 500);
        // A repeated call is safe (closing the tab may come twice).
        tasks.shutdown();
    }

    /// After closing no new tasks are started: nobody would receive the result.
    #[test]
    fn no_tasks_after_shutdown() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.shutdown();
        tasks.start_posters(vec!["/несуществующий.webm".into()]);
        tasks.start_thumbs(vec![("1".into(), "https://x/y.jpg".into())]);
        assert_eq!(tasks.poll_event(), None);
    }

    /// A page change cancels the previous thumbnails node rather than orphaning
    /// it (fix #2).
    #[test]
    fn thumb_node_is_swapped_and_cancelled() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.start_thumbs(vec![("1".into(), "https://invalid.invalid/a.jpg".into())]);
        let first = tasks.thumbs.lock().unwrap().clone().expect("узел заведён");
        tasks.start_thumbs(vec![("2".into(), "https://invalid.invalid/b.jpg".into())]);
        assert!(first.is_cancelled(), "прежний узел обязан быть погашен");
        let second = tasks.thumbs.lock().unwrap().clone().expect("узел заведён");
        assert!(!second.is_cancelled());
        tasks.shutdown();
        assert!(second.is_cancelled(), "закрытие гасит и текущий узел");
    }

    /// The wallpaper executor is one per process: a recreated page gets the
    /// same slot rather than an empty one of its own. The test does not touch
    /// the live machine — the executor in the slot is started only by applying.
    #[test]
    fn wallpaper_slot_is_shared_by_the_process() {
        let a = wallpaper_exec::live_slot();
        let b = ExecApplier::live();
        assert!(Arc::ptr_eq(&a, &b.slot), "страница обязана получить общий слот");
        assert!(Arc::ptr_eq(&a, &ExecApplier::live().slot));
    }

    /// Applying a wallpaper does not start a second time while the first is in
    /// progress.
    #[test]
    fn apply_is_single_flight() {
        struct Slow;
        impl WallpaperApply for Slow {
            fn apply(&self, _p: &str, _f: &str, _a: bool, _n: &str) -> (bool, String) {
                std::thread::sleep(std::time::Duration::from_millis(300));
                (true, String::new())
            }
        }
        let dir = tmpdir("apply");
        let tasks = SteamTasks::new(env_at(&dir)).expect("рантайм");
        let item = ProfileBackgroundItem {
            id: "1".into(),
            name: "Ночь".into(),
            game: String::new(),
            url: "https://invalid.invalid/a.jpg".into(),
            preview: String::new(),
            animated: false,
            equipped: false,
        };
        let a: Arc<dyn WallpaperApply> = Arc::new(Slow);
        assert!(tasks.apply_background(item.clone(), false, a.clone()));
        assert!(
            !tasks.apply_background(item, false, a),
            "второе применение обязано получить отказ"
        );
        tasks.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ The most likely place for defects: the work is ALREADY running, and
    /// the tab is being closed. We check the mechanism itself: the blocking work
    /// gets a network token, cancelling the node triggers it on the spot, and
    /// the result does not get out — even if the work managed to finish
    /// computing.
    #[test]
    fn inflight_work_is_cut_by_token() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        let node = tasks.root.child_token();
        let saw_cancel = Arc::new(AtomicBool::new(false));
        let seen = saw_cancel.clone();
        let node2 = node.clone();
        let out = tasks.rt.as_ref().expect("рантайм жив").block_on(async move {
            let handle = tokio::spawn(async move {
                blocking_cancellable(&node2, move |cancel| {
                    // Wait until the watcher triggers the network token: that is exactly what
                    // aborts an already sent ureq request.
                    for _ in 0..500 {
                        if cancel.is_cancelled() {
                            seen.store(true, Ordering::SeqCst);
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    42u32
                })
                .await
            });
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            node.cancel();
            handle.await.expect("задача не падает")
        });
        assert!(
            saw_cancel.load(Ordering::SeqCst),
            "работа обязана УВИДЕТЬ отмену, а не досчитать до конца"
        );
        assert_eq!(out, None, "результат отменённой работы наружу не идёт");
        tasks.shutdown();
    }

    /// Rebuilding the showcase cancels the previous batch of preview frames —
    /// the same class as for thumbnails (fix #2).
    #[test]
    fn poster_node_is_swapped_and_cancelled() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.start_posters(vec!["/нет-такого-1.webm".into()]);
        let first = tasks.posters.lock().unwrap().clone().expect("узел заведён");
        tasks.start_posters(vec!["/нет-такого-2.webm".into()]);
        assert!(first.is_cancelled(), "прежний узел обязан быть погашен");
        tasks.shutdown();
    }

    /// Closing the tab must not leave events in the queue that nobody will
    /// receive: it was exactly here that the original's `done` flew into a
    /// window being destroyed (fix #4).
    #[test]
    fn no_events_after_shutdown() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.start_thumbs(vec![
            ("1".into(), "https://invalid.invalid/a.jpg".into()),
            ("2".into(), "https://invalid.invalid/b.jpg".into()),
        ]);
        tasks.shutdown();
        // Until the tasks are cancelled, they may have managed to put an event;
        // after closing not a single new one appears.
        while tasks.poll_event().is_some() {}
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert_eq!(tasks.poll_event(), None);
    }

    /// A "wrote the cache → read it" round trip on a live file system.
    #[test]
    fn caches_roundtrip_on_disk() {
        let dir = tmpdir("cache");
        let env = env_at(&dir);
        let items = vec![ProfileBackgroundItem {
            id: "1".into(),
            name: "Ночь".into(),
            game: "Portal 2".into(),
            url: "https://cdn/1.jpg".into(),
            preview: String::new(),
            animated: false,
            equipped: true,
        }];
        write_inventory_cache(&env, "765", &items);
        assert_eq!(read_inventory_cache(&env, "765"), Some(items));
        assert_eq!(read_inventory_cache(&env, "999"), None);

        write_applied(&env, "1", "Ночь");
        assert_eq!(read_applied_id(&env), "1");

        assert!(read_meta_cache(&env, "765").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An already downloaded non-empty file does not touch the network.
    #[test]
    fn cached_file_short_circuits_download() {
        let dir = tmpdir("dl");
        let dest = format!("{}/a.jpg", dir.to_string_lossy());
        std::fs::write(&dest, b"x").unwrap();
        let out = download_cached("https://invalid.invalid/a.jpg", &dest, 1, net::CancelToken::new());
        assert_eq!(out.as_deref(), Ok(dest.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Fix #6: a partially downloaded `.part` does not stay in the cache.
    #[test]
    fn failed_download_leaves_no_part_file() {
        let dir = tmpdir("part");
        let dest = format!("{}/b.jpg", dir.to_string_lossy());
        let cancel = net::CancelToken::new();
        cancel.cancel(); // abort even before the first access
        let out = download_cached("https://invalid.invalid/b.jpg", &dest, 1, cancel);
        assert!(out.is_err());
        assert!(!std::path::Path::new(&qs::part_path(&dest)).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
