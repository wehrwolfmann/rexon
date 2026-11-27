//! Live executor of wallpaper plans: where the engine's PLAN becomes action.
//!
//! WHY. Wallpaper decisions have long been entirely in Rust — [`wallpaper_engine`]
//! can build a plan for any operation ([`Step`]s of `static_plan`,
//! `restore_static_plan`, `animated_plan`, `video_native_plan`,
//! `youtube_plan`, `web_plan`), knows what to restore the desktop with after
//! a stop, and knows where live wallpapers are drawn not by our process but
//! by a shell extension. But it cannot DO anything: a plan is data. Until now
//! the only plan executor was Python (`WallpaperApplier` in
//! `rexon/wallpaper_engine.py`), and because of that the side-effect object
//! of the «Settings» tab in Rust honestly answered "no engine" to all twelve
//! wallpaper buttons (`settings_actions.rs`, boundary 1).
//!
//! This is where the executor appears. The split is the same as before and it
//! is fundamental — **the engine decides, the executor acts**:
//!   * WHAT to launch, in which order, with which words to answer — [`wpe`];
//!   * WITH WHAT to do it — the [`WpSys`] seam (subprocesses, files,
//!     environment, clock, bridge) and its live implementation [`LiveSys`];
//!   * HOW to run the plan — [`WallpaperExec::run_plan`].
//!
//! There is not a single rule of its own here. Even the timings — the "is the
//! launched process alive" grace period, the poll step, waiting for death by
//! signal, the plasmashell and swww timeouts — live in [`wpe`] next to their
//! justification, not as numbers in the code: we already got burned exactly
//! on this when the shim grew its own copy of the grace period.
//!
//! ⚠️ WHY A SEAM AND NOT DIRECT CALLS. The wallpaper executor changes the
//! machine owner's DESKTOP. A check that called it directly would strip the
//! person's wallpaper on every `cargo test` — so all side effects go through
//! [`WpSys`]: live it is [`LiveSys`], in the checks it is a fake `FakeSys`
//! that only records what it was asked to do. This also makes it possible to
//! check the order of steps and plan rollbacks without launching a single
//! process.
//!
//! The model is the live executors already in the core: [`crate::splash_exec`]
//! (subprocesses, sockets, lock, journal) and [`crate::ext_exec`].

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::session_splash::RunOut;
use crate::system::DesktopShell;
use crate::utils;
use crate::wallpaper_engine as wpe;

// ============================================================================
// Journal
// ============================================================================

/// Wallpaper journal sink: `(level, message)`.
///
/// Its own, not shared with the splash: in Python these lines went to
/// DIFFERENT loggers (`rexon.wallpaper_engine` and `rexon.session_splash`),
/// and a bridge with one sink on both would glue two journals into one.
type LogSink = Box<dyn Fn(&str, &str) + Send + Sync>;

fn log_sink() -> &'static Mutex<Option<LogSink>> {
    static SINK: OnceLock<Mutex<Option<LogSink>>> = OnceLock::new();
    SINK.get_or_init(|| Mutex::new(None))
}

/// Install the journal sink (the bridge routes it to `rexon.wallpaper_engine`).
pub fn set_log_sink(sink: impl Fn(&str, &str) + Send + Sync + 'static) {
    if let Ok(mut slot) = log_sink().lock() {
        *slot = Some(Box::new(sink));
    }
}

fn log_line(level: &str, message: &str) {
    if let Ok(slot) = log_sink().lock() {
        if let Some(sink) = slot.as_ref() {
            sink(level, message);
            return;
        }
    }
    crate::applog::write(crate::applog::level_from_name(level), "wallpaper", message);
}

// ============================================================================
// Operation result
// ============================================================================

/// Result of a wallpaper operation — port of `WallpaperResult`.
///
/// The message is never empty: a customer reads a silent refusal as a broken
/// button (rule #13), so the executor has no empty outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperResult {
    /// Whether it worked.
    pub ok: bool,
    /// What to tell the person.
    pub message: String,
    /// PID of the started wallpaper process, if one was started.
    pub pid: Option<i64>,
    /// The wallpaper is set, but there is something the person should know (live
    /// Wallpaper Engine wallpapers stayed on top). Separate from `message`: a
    /// result with a warning is shown with its own icon rather than a green
    /// check mark, and is not truncated along with a long wallpaper name (rule #13).
    pub warning: Option<String>,
}

impl WallpaperResult {
    /// Success with a message.
    pub fn ok(message: impl Into<String>) -> Self {
        WallpaperResult { ok: true, message: message.into(), pid: None, warning: None }
    }

    /// Failure with a message.
    pub fn fail(message: impl Into<String>) -> Self {
        WallpaperResult { ok: false, message: message.into(), pid: None, warning: None }
    }
}

/// ⚠️ THE EXECUTOR'S BOUNDARY, STATED OUT LOUD.
///
/// One format has not been carried over — Flash (SWF): the executor has no
/// Ruffle path (a separate host is being built for it). Keeping quiet about it
/// is not allowed (rule #13): a customer reads a silent "nothing happened" as
/// a broken button, so the executor ANSWERS with this line instead of
/// pretending the wallpaper was set.
pub fn live_formats_not_carried() -> String {
    crate::i18n::tr("this kind of live wallpaper is still shown by the old engine")
}

/// Whether the executor carries this wallpaper format itself — that is,
/// whether [`WallpaperExec::apply`] applies it rather than answering with the
/// [`live_formats_not_carried`] boundary.
///
/// There is one rule per product and it lives here, next to the boundary
/// itself: the window that decides whether to call the executor or send the
/// person to the classic window asks this function instead of keeping its own
/// copy of the list. Otherwise moving the next format here would silently
/// never reach the button.
///
/// Today the executor carries ALL formats: picture, GIF, video, YouTube, web
/// and — the last to move over — Flash (SWF), shown by the
/// `rexon-wallpaper-host --mode swf` mode (Ruffle as the `rexon-swf` library).
/// "Carries" also means "answers definitively": the executor rejects scenes,
/// application wallpapers and an unknown format with the same words as the
/// classic window. The function remains the decision point: if a format ever
/// ends up only in the classic window again, that is declared here, and the
/// buttons find out by themselves.
pub fn carries_format(_fmt: &str) -> bool {
    true
}

/// The final refusal for a format Rexon does not have at all — the same words
/// as in Python (`apply`: `scene_unsupported`, `application_unsupported`,
/// `unknown_format`). `None` — the format is known and gets applied.
fn retired_format_refusal(fmt: &str) -> Option<String> {
    match fmt {
        "static" | "animated" | "video" | "youtube" | "web" | "swf" => None,
        "scene" => Some(wpe::scene_unsupported()),
        "application" => Some(wpe::application_unsupported()),
        other => Some(wpe::unknown_format_message(other)),
    }
}

/// YouTube stream volume, % — Python's default `volume` is 50: the stream is
/// played by mpv/mpvpaper, which the host's sound supervisor cannot reach.
const YOUTUBE_STREAM_VOLUME: i64 = 50;

/// Video and YouTube sound when the caller gives no choice — the Python
/// default (`video_has_audio = True`). Volume and "sound on/off" meanwhile
/// live in `settings.json` (`wallpaper_audio`) and take effect on the fly.
pub const VIDEO_AUDIO_DEFAULT: bool = true;

/// Whether live wallpapers of this format need load supervision: video and
/// YouTube (their host decodes frames the whole time it runs) and Flash
/// (Ruffle draws frame after frame). Flash pauses the same way as video —
/// SIGSTOP/SIGCONT to the host process; after resuming, the movie does not
/// fast-forward: its time step is capped in `rexon-swf` (`Settings::dt_cap`).
pub fn format_wants_load_monitor(fmt: Option<&str>) -> bool {
    matches!(fmt, Some("video") | Some("youtube") | Some("swf"))
}

/// File system checks for finding our hosts — the same as Python's
/// `ExecFsProbe`: the executable is in place, and a lookup in `PATH`.
struct ExecProbe;

impl crate::we_client::DaemonFs for ExecProbe {
    fn is_exec_file(&mut self, path: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    fn which(&mut self, name: &str) -> Option<String> {
        utils::which(name, None)
    }
}

/// One background sound: a separate mpv process for each.
#[derive(Debug, Clone)]
struct Ambient {
    id: String,
    path: String,
    name: String,
    volume: i64,
    /// Looped. A one-shot sound is a legitimate case, not a failure: the extension
    /// sets the mode itself, and the mode decides how launch success is measured.
    looped: bool,
    proc: ProcHandle,
}

/// A playing background sound — what the tab sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientInfo {
    /// Sound identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Volume, percent.
    pub volume: i64,
    /// Whether it is playing.
    pub active: bool,
}

/// Snapshot of the wallpaper engine state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Whether live wallpapers are running right now.
    pub running: bool,
    /// Format of what is playing.
    pub format: Option<String>,
    /// Path of what is playing.
    pub path: Option<String>,
    /// Detected environment.
    pub de: String,
    /// Detected session type.
    pub session_type: String,
    /// PID of the wallpaper process while it is alive.
    pub pid: Option<i64>,
    /// Whether a backdrop (a movie frame as the desktop picture) or a blurred
    /// copy of the wallpaper is set.
    pub blur_active: bool,
    /// Playing background sounds.
    pub ambient: Vec<AmbientInfo>,
}

// ============================================================================
// Side-effect seam
// ============================================================================

/// Handle of a started process — not a PID: the system reuses PIDs, and the
/// executor must stop ONLY its own process and never someone else's by name.
pub type ProcHandle = i64;

/// Everything the executor touches the live machine with.
///
/// Not a single decision has moved here: implementations only do what they are
/// told. The live one is [`LiveSys`], the fake one is in this module's checks.
pub trait WpSys {
    /// Run a command and wait for it. `None` — the command is missing or timed
    /// out: the same safe default Python gave on `OSError`.
    fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut>;

    /// Start a process. `new_session` — its own session (`start_new_session`),
    /// so that grandchildren (mpv/ffmpeg) are stopped together with the parent.
    fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String>;

    /// Exit code if the process has already finished; `None` — alive.
    fn poll(&mut self, proc: ProcHandle) -> Option<i64>;

    /// Process PID — for the `{pid}` message and the PID file.
    fn pid(&mut self, proc: ProcHandle) -> Option<i64>;

    /// Stop the process: SIGTERM to the group → wait → SIGKILL to the group → wait.
    fn kill_process(&mut self, proc: ProcHandle);

    /// Whether such a program exists on the system.
    fn has_tool(&mut self, tool: &str) -> bool;

    /// Package name hint for the current distribution (or empty).
    fn package_hint(&mut self, tool: &str) -> String;

    /// The file or directory is in place.
    fn exists(&mut self, path: &str) -> bool;

    /// It is a directory.
    fn is_dir(&mut self, path: &str) -> bool;

    /// Sorted directory contents (names without the path).
    fn listdir(&mut self, path: &str) -> Vec<String>;

    /// File text or `None`.
    fn read_text(&mut self, path: &str) -> Option<String>;

    /// Write text, creating directories. A failure is logged, not fatal.
    fn write_text(&mut self, path: &str, content: &str);

    /// Delete the file if it exists.
    fn remove_file(&mut self, path: &str);

    /// Environment variable or an empty string.
    fn env(&mut self, name: &str) -> String;

    /// Monotonic clock, seconds.
    fn monotonic(&mut self) -> f64;

    /// Wait.
    fn sleep(&mut self, seconds: f64);

    /// Tell the bridge whether live wallpapers are running (`announce` — also as an event).
    fn bridge_notify(&mut self, live: bool, name: &str, announce: bool);

    /// Blur the `source` picture and write a JPEG to `output`. By default it
    /// refuses: fake seams are not obliged to blur.
    fn blur_image(&mut self, _source: &str, _output: &str, _radius: i64) -> Result<(), String> {
        Err("размытие этим швом не поддерживается".to_string())
    }

    /// Freeze (`pause`) or unfreeze the wallpaper process — SIGSTOP/SIGCONT to
    /// the process itself, as in Python. By default nothing.
    fn pause_process(&mut self, _proc: ProcHandle, _pause: bool) {}

    /// The built wallpaper host (`rexon-wallpaper-host`, video and web modes)
    /// or `None` — it is nowhere. The search order is kept by the engine
    /// (`wallpaper_engine::find_wallpaper_host_binary`).
    fn wallpaper_host(&mut self) -> Option<String> {
        self.video_host()
    }

    /// ⚠️ A transitional name for [`WpSys::wallpaper_host`] from the time when
    /// there were two hosts. The test substitute in
    /// `rexon-gui/src/settings_actions.rs` still answers to it (that file is
    /// being edited by parallel work); there is no search of its own behind it,
    /// the executor asks only `wallpaper_host`. Remove together with migrating
    /// that substitute.
    fn video_host(&mut self) -> Option<String> {
        None
    }

    /// File size in bytes or `None`.
    fn file_size(&mut self, _path: &str) -> Option<u64> {
        None
    }

    /// Path to the faststart version of a video (moov at the start) — port of
    /// `_ensure_faststart`. By default the original path.
    fn ensure_faststart(&mut self, path: &str) -> String {
        path.to_string()
    }

    /// Extract a frame from a video (copy a picture) into `output`. True — the
    /// frame is in place. By default no.
    fn extract_frame(&mut self, _source: &str, _output: &str, _timestamp: f64) -> bool {
        false
    }

    /// A sharp movie frame for the backdrop and for the frame shown instead of a
    /// crashing wallpaper, scaled to the `screen` ([`wpe::video_still_argv`]).
    /// By default the same frame as [`WpSys::extract_frame`].
    fn extract_still(&mut self, source: &str, output: &str, timestamp: f64, _screen: Option<(u64, u64)>) -> bool {
        self.extract_frame(source, output, timestamp)
    }

    /// Time of the file's last modification, seconds since the epoch, or `None`.
    fn file_mtime(&mut self, _path: &str) -> Option<i64> {
        None
    }

    /// The frame was taken from the frame cache: refresh its time so that pruning
    /// ([`WpSys::prune_video_stills`]) removes long-unneeded ones, not this one.
    fn touch_file(&mut self, _path: &str) {}

    /// Shrink the frame cache `dir` to the limit ([`wpe::video_still_evictions`]),
    /// without touching `keep`. By default nothing.
    fn prune_video_stills(&mut self, _dir: &str, _keep: &str) {}

    /// Download a YouTube video into the cache: the file path or `None`. By
    /// default no.
    fn download_youtube(&mut self, _url: &str, _quality: &str) -> Option<String> {
        None
    }

    /// A foreign shell (Noctalia, DankMaterialShell) currently holding the
    /// wallpaper layer in this environment: its surface would lie ON TOP of our
    /// wallpaper. By default nobody: fake seams do not see a live session. The
    /// "whose background" decision is made by the core
    /// (`wallpaper_engine::wallpaper_layer_owner`), the live implementation only
    /// gathers observations.
    fn wallpaper_shell_owner(&mut self, _de: &str, _session_type: &str) -> Option<DesktopShell> {
        None
    }

    /// Synchronously stop live Wallpaper Engine wallpapers (the waywallen daemon's
    /// renderers): `(status, how many stopped)` in the words of
    /// [`crate::we_client::stop_active_result`]. `None` — there is nobody to stop
    /// them: a process without Qt (core, CLI) did not install the hook.
    ///
    /// By default — the process hook ([`set_we_stopper`]): the socket to the
    /// daemon lives in Qt, and the window (`rexon-gui`) installs it. Fake seams
    /// do not see it unless they override the method.
    fn stop_we_wallpapers(&mut self, timeout_ms: u64) -> Option<(String, i64)> {
        we_stopper().map(|stop| stop(timeout_ms))
    }

    /// Remove the waywallen output that, without live wallpapers, covers the
    /// desktop with its placeholder (GNOME: the `org.waywallen.gnome` extension
    /// paints the desktop black, [`crate::wallpaper_backend::set_waywallen_gnome_display`]).
    /// By default nothing: fake seams do not touch the live session.
    fn release_waywallen_display(&mut self) {}

    /// Our own live wallpaper went in as a background-layer surface (video, web,
    /// Flash, GIF on a compositor with layer-shell). On Cinnamon with layer-shell
    /// the shell's background service is older and would cover it — it has to be
    /// put under our surface ([`crate::wallpaper_backend::restack_cinnamon_background_soon`]).
    /// By default nothing: fake seams do not touch the live session.
    fn own_layer_surface_up(&mut self) {}

    /// Cinnamon with layer-shell in this session
    /// ([`crate::wallpaper_backend::cinnamon_layer_shell_fact`]). By default
    /// no: fake seams do not read the live process fact.
    fn cinnamon_layer_shell(&self) -> bool {
        false
    }

    /// Remove an abandoned copy of our Cinnamon extension from the home
    /// directory — the same function as for screen effects
    /// ([`crate::screen_effects::remove_orphan_user_copy`]). `Some` — the path of
    /// the removed copy. By default nothing: fake seams do not touch the home
    /// directory.
    fn remove_orphan_shell_ext_copy(&mut self, _de: &str) -> Option<String> {
        None
    }

    /// Applying has started (the journal's «applying …» line): the next host
    /// process gets this moment in [`wpe::APPLY_STARTED_ENV`] and tells the
    /// journal how long the person waited for the first frame. By default
    /// nothing.
    fn note_apply_started(&mut self) {}

    /// Rexon set the picture `path` as the desktop wallpaper — next, recompute
    /// «Accent color from wallpaper» outside Plasma ([`crate::accent::follow_wallpaper`]).
    /// By default nothing: fake seams do not touch real settings.
    fn static_wallpaper_changed(&mut self, _path: &str) {}

    /// Live wallpapers are applied — make sure the crash watcher
    /// ([`crate::renderer_watch::ensure_running`]) runs in this process.
    /// By default nothing: fake seams do not start a live watcher.
    fn start_watch(&mut self) {}
}

/// What the person applied with our own host — what the crash watcher will
/// bring back up with the same apply as the button does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostRequest {
    path: String,
    fmt: String,
    name: String,
    audio: bool,
}

/// Formats shown by our own wallpaper host and watched by
/// [`crate::renderer_watch::HostWatch`].
pub fn host_watched_format(fmt: &str) -> bool {
    matches!(fmt, "video" | "web" | "swf")
}

/// What the executor knows about its host for a watcher pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostSnapshot {
    /// What should be shown (`format:path`); `None` — no wallpaper of our own.
    pub subject: Option<String>,
    /// Our process (or a companion on another monitor) has exited by itself.
    pub exited: bool,
    /// PIDs of the live host processes — our children.
    pub pids: Vec<i64>,
    /// Frozen by load supervision.
    pub paused: bool,
    /// The host is a child of the shell extension (GNOME/Cinnamon-Wayland).
    pub via_extension: bool,
    /// Path of the host binary — extension processes are matched by it.
    pub host_binary: Option<String>,
}

// ============================================================================
// Live Wallpaper Engine wallpapers under our own
// ============================================================================
//
// WE wallpapers are drawn by the waywallen daemon with its own renderers — on
// top of the desktop, wherever it displays them (Plasma plugin, layer-shell,
// GNOME/Cinnamon extensions). Rexon's own wallpapers set while a WE
// wallpaper is live end up UNDER its surface: the person presses «Apply» but
// sees the previous picture (rule #13). So any wallpaper of our own first
// stops the WE wallpapers — with one request to the daemon (`renderer_list`
// → `renderer_kill` for each), without working out which desktop shows them.

/// The "stop live WE wallpapers" hook: timeout in ms → `(status, how many stopped)`.
pub type WeStopper = fn(u64) -> (String, i64);

fn we_stopper_slot() -> &'static Mutex<Option<WeStopper>> {
    static SLOT: Mutex<Option<WeStopper>> = Mutex::new(None);
    &SLOT
}

/// Install the WE-stopping hook (the Qt window calls it once at startup).
pub fn set_we_stopper(stop: WeStopper) {
    if let Ok(mut slot) = we_stopper_slot().lock() {
        *slot = Some(stop);
    }
}

fn we_stopper() -> Option<WeStopper> {
    we_stopper_slot().lock().ok().and_then(|slot| *slot)
}

/// Whether wallpapers of this format stop live WE wallpapers before themselves.
/// Yes — all of Rexon's own formats: each lands on the same desktop layer as
/// the WE wallpaper.
pub fn yields_we_wallpapers(fmt: &str) -> bool {
    matches!(fmt, "static" | "animated" | "video" | "youtube" | "web" | "swf")
}

/// How long to give the shell to enable the extension before asking again.
const SHELL_EXT_ENABLE_SETTLE_SEC: f64 = 0.5;

/// Whether wallpapers of this format put their own surface into the background
/// layer on Wayland (our host, swww, mpvpaper). A picture does not: the shell draws it itself.
pub fn own_layer_surface_format(fmt: &str) -> bool {
    matches!(fmt, "animated" | "video" | "youtube" | "web" | "swf")
}

/// What to tell the person after trying to stop WE wallpapers: `None` — nothing
/// (stopped, nothing to stop, no daemon); `Some` — a warning for the result:
/// our wallpaper is set, but the WE wallpaper may have stayed on top. The
/// warning icon is added by the result line (`qt_settings::wallpaper_done_warned`),
/// it is not in the text itself.
pub fn we_yield_note(status: &str) -> Option<String> {
    (status == crate::we_client::STOP_ERROR).then(|| {
        crate::i18n::tr(
            "Rexon could not reach Wallpaper Engine to stop its live wallpaper. If \
             you still see the old wallpaper, stop it on the Wallpaper Engine tab.",
        )
    })
}

// ============================================================================
// X11 desktop icons — executor seams for core decisions
// ============================================================================

/// Preparing and rolling back desktop icons (`wallpaper_backend::prepare_desktop_icons`,
/// `restore_desktop_icons`) through the same [`WpSys`] seam as the executor:
/// one live path and one substitute in the checks.
pub struct DesktopIconsSys<'a, S: WpSys> {
    sys: &'a mut S,
    state_path: String,
}

impl<'a, S: WpSys> DesktopIconsSys<'a, S> {
    pub fn new(sys: &'a mut S) -> Self {
        let state_path = crate::wallpaper_backend::desktop_icons_state_path(&sys.env("XDG_STATE_HOME"), &sys.env("HOME"));
        Self { sys, state_path }
    }
}

impl<S: WpSys> crate::wallpaper_backend::DesktopIconsIo for DesktopIconsSys<'_, S> {
    fn run(&mut self, argv: &[&str]) -> Option<String> {
        let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
        match self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
            Some(out) if out.rc == 0 => Some(out.stdout),
            _ => None,
        }
    }

    fn read_state(&mut self) -> Option<String> {
        self.sys.read_text(&self.state_path)
    }

    fn write_state(&mut self, content: Option<&str>) -> bool {
        match content {
            Some(text) => self.sys.write_text(&self.state_path, text),
            None => self.sys.remove_file(&self.state_path),
        }
        true
    }

    fn pause_ms(&mut self, ms: u64) {
        self.sys.sleep(ms as f64 / 1000.0);
    }
}

// ============================================================================
// Live implementation of the seam
// ============================================================================

/// Side effects on the real machine.
pub struct LiveSys {
    procs: HashMap<ProcHandle, Child>,
    next: ProcHandle,
    epoch: Instant,
    family: String,
    /// Start of the current apply (ms since the epoch) — goes to the next host
    /// in [`wpe::APPLY_STARTED_ENV`] ([`WpSys::note_apply_started`]).
    apply_started_ms: Option<u64>,
}

impl Default for LiveSys {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveSys {
    /// A new side-effect executor.
    pub fn new() -> LiveSys {
        LiveSys {
            procs: HashMap::new(),
            next: 1,
            epoch: Instant::now(),
            family: String::new(),
            apply_started_ms: None,
        }
    }

    fn handle(&mut self) -> ProcHandle {
        let h = self.next;
        self.next += 1;
        h
    }

    /// Package manager family — asked ONCE per process: the engine asks for the
    /// package hint on every plan, and `/etc/os-release` does not change while
    /// running.
    fn family(&mut self) -> String {
        if self.family.is_empty() {
            self.family = crate::system::DistroDetector::detect_live()
                .family_live()
                .as_str()
                .to_string();
        }
        self.family.clone()
    }

    /// Send a signal to the whole process group. `false` — there is no group or
    /// it is OUR group (suicide is not allowed).
    fn signal_group(pid: libc::pid_t, sig: libc::c_int) -> bool {
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid <= 0 {
            return false;
        }
        if pgid == unsafe { libc::getpgrp() } {
            return false;
        }
        unsafe { libc::killpg(pgid, sig) == 0 }
    }

    /// Wait for the child to finish, no longer than the deadline. `true` — it did.
    fn wait_child(child: &mut Child, seconds: f64) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_secs_f64(wpe::ALIVE_POLL_STEP_SEC));
        }
    }
}

impl WpSys for LiveSys {
    fn static_wallpaper_changed(&mut self, path: &str) {
        crate::accent::live_follow_wallpaper(path);
    }

    fn start_watch(&mut self) {
        crate::renderer_watch::ensure_running();
    }

    fn release_waywallen_display(&mut self) {
        crate::wallpaper_backend::set_waywallen_gnome_display_live(false);
        // Cinnamon with layer-shell: the scene output surface would cover both the
        // background service's picture and our layer window — stop the output client.
        if crate::wallpaper_backend::cinnamon_layer_shell_fact() {
            crate::wallpaper_backend::set_layer_shell_display_off_live(true);
        }
    }

    fn own_layer_surface_up(&mut self) {
        if crate::wallpaper_backend::cinnamon_layer_shell_fact() {
            crate::wallpaper_backend::restack_cinnamon_background_soon("свои живые обои слоем стола");
        }
    }

    fn cinnamon_layer_shell(&self) -> bool {
        crate::wallpaper_backend::cinnamon_layer_shell_fact()
    }

    fn remove_orphan_shell_ext_copy(&mut self, de: &str) -> Option<String> {
        let mut ctx = crate::animations::LookCtx::from_env();
        ctx.de = de.to_string();
        crate::screen_effects::remove_orphan_user_copy(&crate::animations::SystemIo, &ctx)
    }

    fn note_apply_started(&mut self) {
        self.apply_started_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as u64);
    }

    fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut> {
        crate::splash_exec::run_capture(argv, timeout)
    }

    fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String> {
        let Some((bin, rest)) = argv.split_first() else {
            return Err(crate::i18n::tr("empty command"));
        };
        let mut cmd = Command::new(bin);
        // The "applying" moment — for one process, the next one after it: the host
        // measures the wait for the first frame from it.
        if let Some(started) = self.apply_started_ms.take() {
            cmd.env(wpe::APPLY_STARTED_ENV, started.to_string());
        }
        cmd.args(rest)
            .stdin(Stdio::null())
            // The wallpaper process output is deliberately not read — why is explained
            // at `wpe::spawn_died_message`: the pipe would fill up on a live wallpaper
            // and choke the host.
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if new_session {
            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        match cmd.spawn() {
            Ok(child) => {
                let h = self.handle();
                self.procs.insert(h, child);
                Ok(h)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn poll(&mut self, proc: ProcHandle) -> Option<i64> {
        let child = self.procs.get_mut(&proc)?;
        match child.try_wait() {
            Ok(Some(st)) => Some(st.code().unwrap_or(-1) as i64),
            _ => None,
        }
    }

    fn pid(&mut self, proc: ProcHandle) -> Option<i64> {
        self.procs.get(&proc).map(|c| c.id() as i64)
    }

    fn kill_process(&mut self, proc: ProcHandle) {
        let Some(mut child) = self.procs.remove(&proc) else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return; // already died by itself — status reaped, nothing more needed
        }
        let pid = child.id() as libc::pid_t;
        if !LiveSys::signal_group(pid, libc::SIGTERM) {
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
        if LiveSys::wait_child(&mut child, wpe::KILL_WAIT_SEC) {
            return;
        }
        log_line("warning", &format!("процесс обоев PID={pid} не ушёл по SIGTERM — SIGKILL"));
        if !LiveSys::signal_group(pid, libc::SIGKILL) {
            let _ = child.kill();
        }
        if !LiveSys::wait_child(&mut child, wpe::KILL_WAIT_SEC) {
            log_line("error", &format!("процесс обоев PID={pid} не умирает даже по SIGKILL"));
        }
    }

    fn has_tool(&mut self, tool: &str) -> bool {
        utils::resolve_tool(tool, &[], std::env::var("PATH").ok().as_deref()).is_some()
    }

    fn package_hint(&mut self, tool: &str) -> String {
        let family = self.family();
        utils::tool_package_hint(tool, &family)
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        // Sorted order: with `os.listdir` it was file system order, and "the first
        // wallpaper package found" changed from machine to machine.
        let mut out: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        std::fs::read(path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn write_text(&mut self, path: &str, content: &str) {
        if let Some(dir) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = crate::utils::atomic_write(path, content) {
            log_line("debug", &format!("не записался {path}: {e}"));
        }
    }

    fn remove_file(&mut self, path: &str) {
        if std::path::Path::new(path).is_file() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn env(&mut self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn monotonic(&mut self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    fn sleep(&mut self, seconds: f64) {
        std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)));
    }

    fn blur_image(&mut self, source: &str, output: &str, radius: i64) -> Result<(), String> {
        wpe::blur_image_file(source, output, radius)
    }

    fn wallpaper_host(&mut self) -> Option<String> {
        wpe::find_wallpaper_host_binary(&mut ExecProbe, &crate::cli::repo_root())
    }

    fn file_size(&mut self, path: &str) -> Option<u64> {
        std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len())
    }

    fn ensure_faststart(&mut self, path: &str) -> String {
        use std::io::Read;
        let (_, ext) = wpe::split_ext(path);
        if !wpe::FASTSTART_EXTS.contains(&ext.to_lowercase().as_str()) {
            return path.to_string();
        }
        let mut header = vec![0u8; wpe::MOOV_HEADER_BYTES];
        let read = std::fs::File::open(path).and_then(|mut f| f.read(&mut header));
        let Ok(n) = read else {
            return path.to_string(); // could not be read — leave the file alone
        };
        header.truncate(n);
        if wpe::moov_at_front(&header) {
            return path.to_string();
        }
        if utils::which(wpe::FFMPEG_BIN, None).is_none() {
            log_line("warning", &format!("нет ffmpeg — видео без faststart может играть чёрным: {path}"));
            return path.to_string();
        }
        let Ok(meta) = std::fs::metadata(path) else {
            return path.to_string();
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let abs = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        let key = wpe::faststart_key(&abs, meta.len(), mtime);
        let dir = format!("{}/rexon/faststart", cache_home_live());
        if std::fs::create_dir_all(&dir).is_err() {
            return path.to_string();
        }
        let out = format!("{dir}/{key}.mp4");
        let ready = |p: &str| std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false);
        if ready(&out) {
            return out;
        }
        log_line("info", &format!("видео без faststart — переношу moov в начало: {path}"));
        let argv = wpe::faststart_argv(path, &out);
        match crate::splash_exec::run_capture(&argv, wpe::FASTSTART_TIMEOUT_SEC as f64) {
            Some(r) if r.rc == 0 && ready(&out) => out,
            _ => {
                log_line("warning", "faststart-ремукс не удался — играю исходный файл");
                path.to_string()
            }
        }
    }

    fn download_youtube(&mut self, url: &str, quality: &str) -> Option<String> {
        crate::youtube_wallpaper::download_live(url, quality)
    }

    fn wallpaper_shell_owner(&mut self, de: &str, session_type: &str) -> Option<DesktopShell> {
        use crate::desktop_shell::{shell_wallpaper_state, LiveFacts, ShellHome};
        let detection = LiveFacts::collect().detect();
        shell_wallpaper_state(de, session_type, detection, &ShellHome::live()).owner.shell()
    }

    fn extract_frame(&mut self, source: &str, output: &str, timestamp: f64) -> bool {
        if let Some(dir) = std::path::Path::new(output).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let (_, ext) = wpe::split_ext(source);
        if wpe::IMAGE_EXTS.contains(&ext.to_lowercase().as_str()) {
            // A picture is taken by copying — no ffmpeg needed here.
            return std::fs::copy(source, output).is_ok();
        }
        if utils::which(wpe::FFMPEG_BIN, None).is_none() {
            return false;
        }
        let argv = wpe::extract_frame_argv(source, output, timestamp);
        matches!(crate::splash_exec::run_capture(&argv, 15.0), Some(r) if r.rc == 0)
            && std::path::Path::new(output).is_file()
    }

    fn extract_still(&mut self, source: &str, output: &str, timestamp: f64, screen: Option<(u64, u64)>) -> bool {
        let (_, ext) = wpe::split_ext(source);
        if wpe::IMAGE_EXTS.contains(&ext.to_lowercase().as_str()) {
            return self.extract_frame(source, output, timestamp);
        }
        if let Some(dir) = std::path::Path::new(output).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if utils::which(wpe::FFMPEG_BIN, None).is_none() {
            return false;
        }
        // Write alongside and rename: an interrupted ffmpeg must not leave half a
        // frame in the cache that the next run would take as a finished one.
        let part = format!("{output}.part.jpg");
        let argv = wpe::video_still_argv(source, &part, timestamp, screen);
        let ok = matches!(crate::splash_exec::run_capture(&argv, 20.0), Some(r) if r.rc == 0)
            && std::path::Path::new(&part).is_file()
            && std::fs::rename(&part, output).is_ok();
        if !ok {
            let _ = std::fs::remove_file(&part);
        }
        ok
    }

    fn file_mtime(&mut self, path: &str) -> Option<i64> {
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        Some(modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
    }

    fn touch_file(&mut self, path: &str) {
        if let Ok(f) = std::fs::File::options().write(true).open(path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
    }

    fn prune_video_stills(&mut self, dir: &str, keep: &str) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let files: Vec<(String, i64, u64)> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                Some((e.path().to_string_lossy().into_owned(), mtime, meta.len()))
            })
            .collect();
        let gone = wpe::video_still_evictions(&files, keep);
        for path in &gone {
            let _ = std::fs::remove_file(path);
        }
        if !gone.is_empty() {
            log_line("info", &format!("кеш кадров подложки ужат: убрано {}", gone.len()));
        }
    }

    fn pause_process(&mut self, proc: ProcHandle, pause: bool) {
        let Some(child) = self.procs.get_mut(&proc) else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return; // already finished — nobody to signal
        }
        let pid = child.id() as libc::pid_t;
        let sig = if pause { libc::SIGSTOP } else { libc::SIGCONT };
        unsafe { libc::kill(pid, sig) };
    }

    fn bridge_notify(&mut self, live: bool, name: &str, announce: bool) {
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        // The timestamp is printed by the core (`engine_update::now_iso`) — there
        // must be no second copy of the calendar in the tree.
        let now = crate::engine_update::now_iso(since.as_secs() as i64);
        let epoch_ms = since.as_millis() as u64;
        if let Err(e) = bridge.notify_video_wallpaper(live, name, announce, &now, epoch_ms) {
            log_line("debug", &format!("мост не принял весть об обоях: {e}"));
        }
    }
}

// ============================================================================
// Outputs (monitors): one query for the whole plan run
// ============================================================================

/// One desktop output: what it is called, its resolution and where it
/// stands.
///
/// The name is needed by engines that can address an output (`swww --outputs`,
/// `mpvpaper <output>`), the resolution with position by those that ask for
/// window geometry (`xwinwrap -g WxH+X+Y`): without the offset, the second
/// monitor's window would land on top of the first. All three values come
/// from ONE query: previously the name came from `wlr-randr` and the
/// resolution from a separate `xrandr`, and on the same machine they could
/// be talking about DIFFERENT monitors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenOutput {
    /// Output name (`DP-1`, `eDP-1`, `HDMI-A-1`).
    pub name: String,
    /// Output resolution, `WxH`.
    pub resolution: String,
    /// Where the output stands on the desktop, `(X, Y)` in points. `None` — the
    /// query did not say: this happens with truncated output and foreign reply forms.
    pub position: Option<(i64, i64)>,
}

impl ScreenOutput {
    /// Window geometry on this output, `WxH+X+Y` — what `xwinwrap -g` asks for.
    ///
    /// ⚠️ Position unknown — the offset is zero: this gives exactly the geometry
    /// the plan's command had before the per-monitor layout, and the wallpaper
    /// gets set as before instead of no longer being set.
    pub fn geometry(&self) -> String {
        let (x, y) = self.position.unwrap_or((0, 0));
        format!("{}+{x}+{y}", self.resolution)
    }

    /// Output size in points. `None` — the resolution did not parse (truncated
    /// output, a foreign reply form): such an output does not take part in size
    /// comparison rather than being counted as zero.
    pub fn size(&self) -> Option<(u64, u64)> {
        let (w, h) = self.resolution.split_once(['x', 'X'])?;
        let w: u64 = w.trim().parse().ok()?;
        let h: u64 = h.trim().parse().ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some((w, h))
    }
}

/// Output name when the system could not be queried: "all" in the engines' language.
const ANY_OUTPUT: &str = "*";

/// Resolution when the system could not be queried.
const ANY_RESOLUTION: &str = "1920x1080";

/// Outputs from the JSON of `wlr-randr --json` or `hyprctl monitors -j`.
///
/// The forms differ, but both have what is needed: in `wlr-randr` the size
/// lies in the current mode (`modes[i].current`), in `hyprctl` directly in
/// the monitor (`width`/`height`). Disabled outputs are skipped: wallpapers
/// are not set on a switched-off monitor.
pub fn parse_outputs_json(stdout: &str) -> Vec<ScreenOutput> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        // `wlr-randr`: enabled=false; `hyprctl`: disabled=true.
        if item.get("enabled").and_then(|v| v.as_bool()) == Some(false)
            || item.get("disabled").and_then(|v| v.as_bool()) == Some(true)
        {
            continue;
        }
        let name = item
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let size = |v: &serde_json::Value, key: &str| -> Option<i64> {
            v.get(key).and_then(|n| n.as_i64())
        };
        let mut resolution = match (size(item, "width"), size(item, "height")) {
            (Some(w), Some(h)) if w > 0 && h > 0 => format!("{w}x{h}"),
            _ => String::new(),
        };
        if resolution.is_empty() {
            if let Some(modes) = item.get("modes").and_then(|m| m.as_array()) {
                let current = modes
                    .iter()
                    .find(|m| m.get("current").and_then(|c| c.as_bool()) == Some(true))
                    .or_else(|| modes.first());
                if let Some(m) = current {
                    if let (Some(w), Some(h)) = (size(m, "width"), size(m, "height")) {
                        if w > 0 && h > 0 {
                            resolution = format!("{w}x{h}");
                        }
                    }
                }
            }
        }
        if resolution.is_empty() {
            resolution = ANY_RESOLUTION.to_string();
        }
        // Position: in `wlr-randr` it is nested (`position.x`), in `hyprctl` it lies
        // directly in the monitor (`x`/`y`). Neither one — `None`, and the geometry
        // comes out with a zero offset.
        let at = |v: &serde_json::Value, key: &str| -> Option<i64> {
            v.get(key).and_then(|n| n.as_i64())
        };
        let position = match item.get("position") {
            Some(pos) if pos.is_object() => match (at(pos, "x"), at(pos, "y")) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
            _ => match (at(item, "x"), at(item, "y")) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
        };
        out.push(ScreenOutput { name, resolution, position });
    }
    out
}

/// Outputs from `xrandr --current`.
///
/// Take the lines of connected outputs (`DP-1 connected 1920x1080+0+0 …`):
/// the name is the first word, the resolution is the size up to the first
/// `+`. Modes are listed below as separate lines and contain no name at all —
/// so if not a single `connected` line is found (truncated output, a foreign
/// locale), fall back to the old "line with the active mode" rule and return
/// one nameless output.
pub fn parse_xrandr_outputs(stdout: &str) -> Vec<ScreenOutput> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line.starts_with(char::is_whitespace) {
            continue; // a mode line, not an output line
        }
        let mut words = line.split_whitespace();
        let Some(name) = words.next() else { continue };
        if words.next() != Some("connected") {
            continue;
        }
        let geometry = line
            .split_whitespace()
            .find(|w| w.contains('+') && w.contains('x'));
        let resolution = geometry
            .and_then(|g| g.split('+').next())
            .filter(|r| r.contains('x'))
            .unwrap_or(ANY_RESOLUTION)
            .to_string();
        // The output offset is printed on the same line as the size
        // (`1920x1080+1920+0`) — take it from there too, as the second and third
        // pieces.
        let position = geometry.and_then(|g| {
            let mut parts = g.split('+').skip(1);
            match (parts.next(), parts.next()) {
                (Some(x), Some(y)) => match (x.parse::<i64>(), y.parse::<i64>()) {
                    (Ok(x), Ok(y)) => Some((x, y)),
                    _ => None,
                },
                _ => None,
            }
        });
        out.push(ScreenOutput { name: name.to_string(), resolution, position });
    }
    if out.is_empty() {
        if let Some(res) = wpe::parse_xrandr_resolution(stdout) {
            out.push(ScreenOutput {
                name: ANY_OUTPUT.to_string(),
                resolution: res,
                position: None,
            });
        }
    }
    out
}

/// What to query outputs with: the command and the parser of its output.
///
/// The order depends on the session — under X11 `xrandr` answers first, under
/// Wayland it may be absent altogether (or show a single XWayland screen).
fn output_probes(session_type: &str) -> Vec<Vec<String>> {
    let wlr = vec!["wlr-randr".to_string(), "--json".to_string()];
    let hypr = vec![
        "hyprctl".to_string(),
        "monitors".to_string(),
        "-j".to_string(),
    ];
    let xr = vec!["xrandr".to_string(), "--current".to_string()];
    if session_type == "x11" {
        vec![xr, wlr, hypr]
    } else {
        vec![wlr, hypr, xr]
    }
}

/// The SINGLE source of the output list.
///
/// The rule used to be smeared around: the monitor name came from its own walk
/// over `wlr-randr`/`hyprctl`, the resolution from its own `xrandr` call, and
/// both took the FIRST thing they found. Now there is one query, it returns
/// all outputs with name and resolution, and everyone — the placeholders in
/// commands, the per-monitor layout, full-screen window supervision — asks it.
///
/// An empty answer is a legitimate outcome: the query tool may be missing
/// from the system. Then the caller must set the wallpaper the way it did
/// before any output enumeration.
fn screen_outputs<S: WpSys>(sys: &mut S, session_type: &str) -> Vec<ScreenOutput> {
    for cmd in output_probes(session_type) {
        let timeout = utils::default_timeout_secs(&cmd) as f64;
        let Some(out) = sys.run(&cmd, timeout) else {
            continue; // no such command or timed out
        };
        if out.rc != 0 || out.stdout.trim().is_empty() {
            continue;
        }
        let found = if cmd[0] == "xrandr" {
            parse_xrandr_outputs(&out.stdout)
        } else {
            parse_outputs_json(&out.stdout)
        };
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

// ============================================================================
// Observations the plan requests via placeholders
// ============================================================================

/// What has already been asked of the system during this plan run.
///
/// Observations are expensive and not every step needs them: the core requests
/// them with the `{monitor}`/`{resolution}` placeholders, and we ask the
/// system only when a step with a placeholder has ACTUALLY reached execution.
/// Otherwise setting a wallpaper via swww would poke `wlr-randr` for nothing.
#[derive(Debug, Default)]
struct Seen {
    /// Session type — it selects the order of output queries.
    session: String,
    /// Outputs, if they have already been asked for during this run (an empty
    /// list means asked and not found out; that is NOT a reason to ask again).
    outputs: Option<Vec<ScreenOutput>>,
    /// Which output fills the placeholders by default: `None` — the first one.
    pinned: Option<ScreenOutput>,
}

impl Seen {
    fn new(session_type: &str) -> Self {
        Seen { session: session_type.to_string(), ..Seen::default() }
    }

    /// Outputs of this run — the query is made at most once.
    fn outputs<S: WpSys>(&mut self, sys: &mut S) -> Vec<ScreenOutput> {
        if self.outputs.is_none() {
            let found = screen_outputs(sys, &self.session);
            if found.is_empty() {
                log_line(
                    "warning",
                    "не удалось перечислить мониторы — обои ставлю общей командой, \
                     как без перечисления",
                );
            }
            self.outputs = Some(found);
        }
        self.outputs.clone().unwrap_or_default()
    }

    /// What to fill the placeholders with now: the pinned output, otherwise the
    /// first in the list, otherwise "all"/the default.
    fn marks<S: WpSys>(&mut self, sys: &mut S) -> ScreenOutput {
        if let Some(one) = &self.pinned {
            return one.clone();
        }
        self.outputs(sys).first().cloned().unwrap_or(ScreenOutput {
            name: ANY_OUTPUT.to_string(),
            resolution: ANY_RESOLUTION.to_string(),
            position: None,
        })
    }
}

/// The outputs with the largest and smallest area — the extremes of those listed.
///
/// WHY BOTH. When ONE picture goes to the whole desktop (a wallpaper pack, a
/// `Whole` engine), measure by the LARGEST output: the big monitor gets its
/// own size, the small one shrinks the picture, and that is noticeably better
/// than the reverse — stretched up from the small one. When asked "does the
/// window occupy the whole screen", measure by the SMALLEST: a full-screen
/// game lives on its own monitor, and that is its yardstick, not the neighbor.
///
/// An output with an unparsed resolution does not take part in the
/// comparison; an empty list or no readable size at all gives `None`, and
/// the caller does the same as without any output enumeration. On equal
/// area the output with the smaller name wins: the same machine must give
/// the same answer.
fn extreme_output(outputs: &[ScreenOutput], biggest: bool) -> Option<&ScreenOutput> {
    let mut best: Option<(&ScreenOutput, u64)> = None;
    for o in outputs {
        let Some((w, h)) = o.size() else { continue };
        let area = w.saturating_mul(h);
        let better = match &best {
            None => true,
            Some((prev, prev_area)) => {
                if area == *prev_area {
                    o.name < prev.name
                } else if biggest {
                    area > *prev_area
                } else {
                    area < *prev_area
                }
            }
        };
        if better {
            best = Some((o, area));
        }
    }
    best.map(|(o, _)| o)
}

/// The largest output — the yardstick for ONE picture on the whole desktop.
pub fn largest_output(outputs: &[ScreenOutput]) -> Option<&ScreenOutput> {
    extreme_output(outputs, true)
}

/// The smallest output — the yardstick for "the window occupies its whole screen".
pub fn smallest_output(outputs: &[ScreenOutput]) -> Option<&ScreenOutput> {
    extreme_output(outputs, false)
}

/// Outputs of the live machine — the entry point for those without their own [`WpSys`] seam.
///
/// Needed by the «Appearance» tab: it sets a picture FROM A PACK as the
/// background and measures the screen to choose which one. It no longer has
/// its own query — the truth about outputs is one per product and lives here.
pub fn live_screen_outputs(session_type: &str) -> Vec<ScreenOutput> {
    screen_outputs(&mut LiveSys::new(), session_type)
}

/// Step placeholders with which the plan requests output observations.
///
/// `{geometry}` — the size TOGETHER with the position (`xwinwrap -g`),
/// `{resolution}` — the size alone, and the old placeholder stays for those
/// who do not need the offset.
fn asks_about_output(text: &str) -> bool {
    text.contains("{monitor}")
        || text.contains("{resolution}")
        || text.contains(wpe::GEOMETRY_MARK)
}

fn fill<S: WpSys>(sys: &mut S, text: &str, seen: &mut Seen) -> String {
    fill_marks(sys, text, seen, &|v| v.to_string())
}

/// [`fill`] for a message to the person: with a right-to-left language each
/// substitution is an isolate ([`crate::i18n::isolate`]). For commands — [`fill`].
fn fill_message<S: WpSys>(sys: &mut S, text: &str, seen: &mut Seen) -> String {
    fill_marks(sys, text, seen, &crate::i18n::isolate)
}

fn fill_marks<S: WpSys>(sys: &mut S, text: &str, seen: &mut Seen, wrap: &dyn Fn(&str) -> String) -> String {
    if !asks_about_output(text) {
        return text.to_string();
    }
    let one = seen.marks(sys);
    text.replace("{monitor}", &wrap(&one.name))
        .replace(wpe::GEOMETRY_MARK, &wrap(&one.geometry()))
        .replace("{resolution}", &wrap(&one.resolution))
}

fn fill_argv<S: WpSys>(sys: &mut S, argv: &[String], seen: &mut Seen) -> Vec<String> {
    argv.iter().map(|a| fill(sys, a, seen)).collect()
}

// ============================================================================
// Engine table: how to lay wallpapers out over outputs
// ============================================================================

/// How an engine covers several monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    /// The engine puts the wallpaper on all outputs ITSELF — one command, as it was.
    /// This also covers everything whose desktop belongs to the environment
    /// (gsettings, Plasma, xfconf), and daemons for which "all outputs" is the default.
    Whole,
    /// One command, but the outputs must be listed: a flag and a name separator
    /// (`swww img --outputs DP-1,HDMI-A-1`).
    List {
        /// The flag followed by the list.
        flag: &'static str,
        /// What joins the output names.
        sep: &'static str,
    },
    /// The command is repeated once per output, and the `{monitor}`,
    /// `{resolution}` and `{geometry}` placeholders in it mean a SPECIFIC output,
    /// not the first one.
    Each,
}

/// How to lay wallpapers out over outputs — a property of the engine, not a guess of the code.
///
/// The row name is `Step::backend`, the very one the engine calls its step by.
/// Anything not in the table is treated as `Whole`: an unfamiliar engine must
/// behave exactly as it did before the layout appeared.
const ENGINE_SPREAD: &[(&str, Spread)] = &[
    // `swww img` without the flag covers outputs by its own default, which has
    // changed from version to version, and the daemon remembers the previous
    // per-output layout. We list them ourselves — then the answer depends neither
    // on the version nor on the daemon's memory.
    ("swww", Spread::List { flag: "--outputs", sep: "," }),
    // `hyprctl hyprpaper wallpaper ,path` — its own form of "all monitors":
    // an empty monitor name before the comma means "all".
    ("hyprpaper", Spread::Whole),
    // swaybg and wbg without `-o` hold ALL outputs; pcmanfm-qt, feh, gsettings,
    // Plasma and xfconf-query own the whole desktop.
    ("swaybg", Spread::Whole),
    ("wbg", Spread::Whole),
    // `swaymsg output * bg` — all outputs with one command.
    ("sway", Spread::Whole),
    ("pcmanfm-qt", Spread::Whole),
    ("feh", Spread::Whole),
    ("gsettings", Spread::Whole),
    ("kde-dbus", Spread::Whole),
    ("plasma-apply-wallpaperimage", Spread::Whole),
    ("xfconf-query", Spread::Whole),
    // mpvpaper accepts EXACTLY ONE output as a positional argument — it reaches
    // the second monitor only with a second launch.
    ("mpvpaper", Spread::Each),
    // xwinwrap covers the X root window by the `-g WxH+X+Y` geometry: the offset
    // decides where the window goes. The plan gets it as the `{geometry}`
    // placeholder — its own for each output, so on two monitors xwinwrap is
    // started twice. Web wallpapers via surf go the same way (xwinwrap).
    ("xwinwrap", Spread::Each),
    ("surf", Spread::Each),
];

/// Table row for an engine; an unfamiliar one is `Whole`.
pub fn engine_spread(backend: &str) -> Spread {
    ENGINE_SPREAD
        .iter()
        .find(|(name, _)| *name == backend)
        .map(|(_, spread)| *spread)
        .unwrap_or(Spread::Whole)
}

// ============================================================================
// Executor
// ============================================================================

/// Live executor of wallpaper plans.
///
/// The state is exactly what `WallpaperApplier` had: what is playing now, by
/// which process, what to restore after a stop. The executor remembers
/// nothing beyond that and decides nothing by itself.
pub struct WallpaperExec<S: WpSys> {
    sys: S,
    de: String,
    session_type: String,

    current_format: Option<String>,
    current_path: Option<String>,
    current_name: String,

    /// The live wallpaper process.
    process: Option<ProcHandle>,
    /// The process HOLDING the static background (swaybg/wbg): it outlives the live ones.
    static_holder: Option<ProcHandle>,
    /// Wallpaper processes on the OTHER monitors: engines like `mpvpaper` accept
    /// exactly one output, so on two screens there are as many processes as
    /// screens. Companions live and die together with the main process —
    /// otherwise «Stop» would turn off the wallpaper on one monitor and leave it
    /// playing on the other.
    companions: Vec<ProcHandle>,

    /// The wallpaper that was there before us — stopping restores it.
    saved_wallpaper: Option<String>,
    /// The environment in which that wallpaper was saved.
    saved_de: Option<String>,
    /// Cinnamon 6.8+: the previous `picture-uri-list` and `background-mode` as
    /// they are (GVariant text) — stopping restores them in full, with fitting,
    /// colors and per-monitor pictures.
    saved_cinnamon_bg: Option<(String, String)>,
    /// KDE: a FOREIGN desktop wallpaper plugin, if the desktop was taken by one.
    saved_kde_plugin: Option<String>,
    /// The backdrop under live wallpapers (a sharp movie frame) or a blurred copy
    /// of the wallpaper — what is currently set as the desktop picture instead of the real one.
    blur_path: Option<String>,
    /// A backdrop deferred until after showing ([`wpe::BackdropTiming::AfterShow`]):
    /// the background door [`run_in_slot`] picks it up ([`spawn_backdrop`]).
    pending_backdrop: Option<BackdropJob>,
    /// Counter of applies and stops: a backdrop that is ready after the wallpaper
    /// changed is no longer in place.
    backdrop_generation: u64,
    /// The backdrop is being set: a successful plan step does not touch the shown
    /// format and path and sends no notice to the bridge — video is still what is shown.
    quiet_static: bool,
    /// Background sounds: a separate process for each, in the order they were started.
    ambient: Vec<Ambient>,

    /// What the bridge has already been told — not what is really going on with the wallpaper.
    bridge_live: bool,
    bridge_name: String,
    bridge_said_at: Option<f64>,
    bridge_flag: Option<bool>,

    /// The wallpaper process is frozen by load supervision (SIGSTOP).
    perf_paused: bool,
    /// The "WE wallpaper stayed on top" note for the current apply's result.
    we_note: Option<String>,
    /// The last successful apply with our own host — for the crash watcher.
    host_request: Option<HostRequest>,
    /// The Cinnamon extension has already been taken off the wallpaper (Cinnamon with layer-shell).
    shell_ext_retired: bool,
}

// ============================================================================
// Load supervision
// ============================================================================

/// What load supervision learned about the machine in one sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadObservation {
    /// The battery is below the threshold and not charging.
    pub battery_low: bool,
    /// A full-screen window is active (and the person allowed pausing on it).
    pub fullscreen: bool,
    /// The wallpaper process loads the CPU above the threshold.
    pub cpu_high: bool,
}

/// Supervision settings for one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorConfig {
    /// Whether to pause the wallpaper while a full-screen window is up.
    pub pause_on_fullscreen: bool,
    /// Battery threshold, %.
    pub battery_pause_pct: i64,
}

/// What supervision should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorVerdict {
    /// Keep watching.
    Continue,
    /// Nothing left to watch: the process changed, crashed or was stopped.
    Done,
}

/// Supervision settings from Rexon's settings map.
///
/// ⭐ A fix: in Python pausing on a full-screen window was hard-wired on
/// (`pause_on_fullscreen = True`) and did not listen to the person. If they
/// unchecked «Stop the wallpaper when it is not visible», the host stopped
/// pausing the player, but supervision froze the process anyway. Now there
/// is one decision — the same setting by which the host itself pauses.
pub fn monitor_config_from(settings: &serde_json::Map<String, serde_json::Value>) -> MonitorConfig {
    let audio = crate::wallpaper_audio::AudioSettings::from_map(settings);
    MonitorConfig {
        pause_on_fullscreen: audio.actions.fullscreen.pauses(),
        battery_pause_pct: wpe::BATTERY_PAUSE_PCT,
    }
}

/// Root of the live machine's user cache (`$XDG_CACHE_HOME`, otherwise
/// `~/.cache`).
fn cache_home_live() -> String {
    match std::env::var("XDG_CACHE_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.cache", std::env::var("HOME").unwrap_or_else(|_| ".".to_string())),
    }
}

/// Supervision settings from disk — re-read on every sample so that toggling
/// the check box takes effect immediately, without restarting the wallpaper.
pub fn monitor_config_live() -> MonitorConfig {
    monitor_config_from(&crate::settings::load_settings(&crate::settings::settings_file()))
}

/// Whether the battery is low — port of `_is_battery_low` (the first `BAT*` battery).
fn battery_low<S: WpSys>(sys: &mut S, threshold: i64) -> bool {
    if !sys.is_dir(wpe::POWER_SUPPLY_DIR) {
        return false;
    }
    for name in sys.listdir(wpe::POWER_SUPPLY_DIR) {
        if !name.starts_with("BAT") {
            continue;
        }
        let dir = format!("{}/{name}", wpe::POWER_SUPPLY_DIR);
        let Some(pct) = sys
            .read_text(&format!("{dir}/capacity"))
            .and_then(|t| t.trim().parse::<i64>().ok())
        else {
            continue;
        };
        let status = sys.read_text(&format!("{dir}/status")).unwrap_or_default();
        return wpe::battery_should_pause(pct, status.trim(), threshold);
    }
    false
}

/// A missing detection tool is logged ONCE: a sample is taken every 5
/// seconds, and the journal would be flooded with the same line.
fn probe_tool<S: WpSys>(sys: &mut S, name: &str) -> bool {
    if sys.has_tool(name) {
        return true;
    }
    static MISSING: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut said = MISSING
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !said.iter().any(|n| n == name) {
        said.push(name.to_string());
        log_line("info", &format!("нет {name} — пауза обоев при полноэкранном окне отключена"));
    }
    false
}

/// Whether a full-screen window is active — port of `_is_fullscreen_app_active`.
/// What to ask and how to parse the answer is up to the engine.
///
/// `wallpaper_pid` — the process drawing the wallpaper right now: its window
/// does not count as "a window over the wallpaper" (see [`wpe::WALLPAPER_SAFE_WINDOW_TYPES`]).
fn fullscreen_active<S: WpSys>(
    sys: &mut S,
    de: &str,
    session_type: &str,
    wallpaper_pid: i64,
) -> bool {
    let backend = wpe::fullscreen_backend(de, session_type);
    if backend.is_empty() || !probe_tool(sys, backend) {
        return false;
    }
    let ok_out = |out: Option<RunOut>| out.filter(|o| o.rc == 0 && !o.stdout.is_empty());
    match backend {
        "xdotool" => {
            let Some(out) = ok_out(sys.run(&wpe::xdotool_geometry_argv(), 5.0)) else {
                return false;
            };
            // ⚠️ The yardstick is the SMALLEST output, not the first one found: a
            // full-screen window occupies ITS OWN monitor, and on a smaller monitor it
            // is smaller than the first — the old "first resolution" yardstick simply
            // did not notice such a window, and the wallpaper did not pause. Outputs
            // not enumerated — the engine default stays, as before.
            let outputs = screen_outputs(sys, session_type);
            let screen = smallest_output(&outputs)
                .map(|o| o.resolution.clone())
                .unwrap_or_else(|| ANY_RESOLUTION.to_string());
            // ⚠️ Geometry alone is not enough: the window manager also calls the
            // desktop itself the active window, and the wallpaper is exactly a
            // full-screen desktop — otherwise the pause would always be on. The
            // decision is one for all supervisors and lives in the wallpaper sound
            // core (rule #12): the wallpaper host asks the same function.
            let has_xprop = sys.has_tool("xprop");
            crate::wallpaper_audio::x11_window_conditions(
                &out.stdout,
                &screen,
                &[wallpaper_pid],
                |id| {
                    if !has_xprop {
                        return None;
                    }
                    ok_out(sys.run(&wpe::xprop_window_info_argv(id), 5.0)).map(|o| o.stdout)
                },
            )
            .fullscreen
        }
        "hyprctl" => ok_out(sys.run(&wpe::hyprctl_active_argv(), 5.0))
            .map(|o| {
                !wpe::hyprctl_window_is_wallpaper_layer(&o.stdout, &[wallpaper_pid])
                    && wpe::parse_hyprctl_fullscreen(&o.stdout)
            })
            .unwrap_or(false),
        "gdbus" => ok_out(sys.run(&wpe::gdbus_fullscreen_argv(), 5.0))
            .map(|o| wpe::parse_gdbus_fullscreen(&o.stdout))
            .unwrap_or(false),
        "kdotool" => {
            let Some(out) = ok_out(sys.run(&wpe::kdotool_active_argv(), 5.0)) else {
                return false;
            };
            let id = out.stdout.trim().to_string();
            if id.is_empty() {
                return false;
            }
            // The window state is given by KWin itself (`getWindowInfo`); ask with
            // whichever qdbus exists — the parser understands its answer form.
            for tool in ["qdbus6", "qdbus"] {
                if !sys.has_tool(tool) {
                    continue;
                }
                let mut argv: Vec<String> = std::iter::once(tool.to_string())
                    .chain(wpe::KWIN_WINDOW_INFO.iter().map(|s| (*s).to_string()))
                    .collect();
                argv.push(id.clone());
                return ok_out(sys.run(&argv, 5.0))
                    .map(|o| {
                        !wpe::kwin_window_is_wallpaper_layer(&o.stdout, &[wallpaper_pid])
                            && wpe::parse_kwin_fullscreen(&o.stdout)
                    })
                    .unwrap_or(false);
            }
            false
        }
        _ => false,
    }
}

/// Whether the process loads the CPU above the threshold — port of `_is_process_cpu_high`.
fn cpu_high<S: WpSys>(sys: &mut S, pid: i64) -> bool {
    let Some(out) = sys.run(&wpe::ps_cpu_argv(pid), 5.0) else {
        return false;
    };
    if out.rc != 0 {
        return false;
    }
    match out.stdout.trim().parse::<f64>() {
        Ok(cpu) if cpu > wpe::CPU_HIGH_THRESHOLD => {
            log_line("debug", &format!("процесс обоев PID={pid} грузит процессор: {cpu:.1}%"));
            true
        }
        _ => false,
    }
}

/// One supervision sample. Runs WITHOUT the executor lock: querying the
/// window manager and `ps` takes tens of milliseconds, and the window must
/// not see the wallpaper as "busy" meanwhile.
pub fn observe_load<S: WpSys>(
    sys: &mut S,
    de: &str,
    session_type: &str,
    pid: i64,
    cfg: &MonitorConfig,
) -> LoadObservation {
    LoadObservation {
        battery_low: battery_low(sys, cfg.battery_pause_pct),
        fullscreen: cfg.pause_on_fullscreen && fullscreen_active(sys, de, session_type, pid),
        cpu_high: cpu_high(sys, pid),
    }
}

/// Start live supervision of the process currently drawing the wallpaper.
///
/// Port of `_start_performance_monitor` + `_performance_monitor_loop`: a
/// sample every [`wpe::MONITOR_INTERVAL_SEC`], decisions are the executor's
/// ([`WallpaperExec::monitor_apply`]). Supervision ends by itself as soon as
/// the process changed, crashed or was stopped — so it can be called after
/// every live wallpaper apply, without tracking the previous thread.
///
/// The window does not call it itself: applies go through [`run_in_slot`],
/// and supervision is started there — in one place for all pages. The seam
/// is generalized for the checks: samples go through the same seam as the
/// executor in the slot.
pub fn spawn_load_monitor<S: WpSys + Default + Send + 'static>(slot: ExecSlot<S>) {
    let target = {
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_mut().and_then(|exec| {
            // Not every live wallpaper needs watching — the core decides.
            if !format_wants_load_monitor(exec.current_format()) {
                return None;
            }
            exec.monitor_target()
                .map(|(handle, pid)| (handle, pid, exec.de.clone(), exec.session_type.clone()))
        })
    };
    let Some((watched, pid, de, session_type)) = target else {
        return;
    };
    log_line("info", &format!("надзор за нагрузкой начат: PID={pid}"));
    let spawned = std::thread::Builder::new()
        .name("rexon-perf-monitor".to_string())
        .spawn(move || {
            let mut probe = S::default();
            let mut strikes = 0i64;
            loop {
                let cfg = monitor_config_live();
                let obs = observe_load(&mut probe, &de, &session_type, pid, &cfg);
                let verdict = {
                    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                    match guard.as_mut() {
                        Some(exec) => exec.monitor_apply(watched, &mut strikes, &obs),
                        None => MonitorVerdict::Done,
                    }
                };
                if verdict == MonitorVerdict::Done {
                    break;
                }
                std::thread::sleep(Duration::from_secs(wpe::MONITOR_INTERVAL_SEC.max(1) as u64));
            }
        });
    if let Err(e) = spawned {
        log_line("error", &format!("надзор за нагрузкой не поднялся: {e}"));
    }
}

// ============================================================================
// Backdrop after showing
// ============================================================================

/// A deferred backdrop: what is shown and where to take the frame from.
#[derive(Debug, Clone, PartialEq)]
pub struct BackdropJob {
    /// The file to take the frame from.
    pub source: String,
    cache_home: String,
    session_type: String,
    generation: u64,
    /// Format and path of what was shown at the moment of queueing.
    shown: (Option<String>, Option<String>),
    /// When showing succeeded (the executor seam's clock) — for the journal line.
    shown_at: f64,
}

/// A sharp movie frame for the screen — from the cache or ffmpeg. One for the
/// backdrop and for the frame shown instead of a crashing wallpaper. The
/// frame is scaled to the largest output ([`wpe::still_screen_size`]); if
/// the outputs could not be queried — the movie's native size. Applying the
/// same wallpaper again on the same screen takes the ready frame
/// ([`wpe::video_still_key`]).
pub fn prepare_video_still<S: WpSys>(sys: &mut S, cache_home: &str, session_type: &str, source: &str) -> Option<String> {
    let sizes: Vec<(u64, u64)> = screen_outputs(sys, session_type).iter().filter_map(ScreenOutput::size).collect();
    let screen = wpe::still_screen_size(&sizes);
    let size = sys.file_size(source).unwrap_or(0);
    let mtime = sys.file_mtime(source).unwrap_or(0);
    let out = wpe::video_still_path(cache_home, &wpe::video_still_key(source, size, mtime, screen));
    if sys.exists(&out) && !sys.is_dir(&out) {
        sys.touch_file(&out);
        return Some(out);
    }
    let ok = sys.extract_still(source, &out, wpe::BACKDROP_FRAME_SEC, screen)
        || sys.extract_still(source, &out, wpe::BACKDROP_FALLBACK_SEC, screen);
    // After extracting — shrink the cache to the limit: this frame is untouched,
    // unfinished leftovers and long-unneeded frames go away.
    if let Some((dir, _)) = out.rsplit_once('/') {
        sys.prune_video_stills(dir, &out);
    }
    ok.then_some(out)
}

/// A deferred backdrop — in the background, after showing ([`wpe::BackdropTiming::AfterShow`]).
///
/// The frame (ffmpeg) is prepared WITHOUT the executor lock through its own
/// seam — the window and the next apply do not wait for it; it is set under
/// the lock, if the wallpaper has not changed in the meantime
/// ([`WallpaperExec::finish_backdrop`]). An error is a warning in the
/// journal: the apply result has already been reported for the video.
pub fn spawn_backdrop<S: WpSys + Default + Send + 'static>(slot: ExecSlot<S>) {
    let job = {
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_mut().and_then(WallpaperExec::take_pending_backdrop)
    };
    let Some(job) = job else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("rexon-backdrop".to_string())
        .spawn(move || {
            let mut probe = S::default();
            let still = prepare_video_still(&mut probe, &job.cache_home, &job.session_type, &job.source);
            let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(exec) = guard.as_mut() {
                exec.finish_backdrop(&job, still);
            }
        });
    if let Err(e) = spawned {
        log_line("warning", &format!("подложка не поднялась в фоне: {e}"));
    }
}

// ============================================================================
// The process executor: one for all window pages
// ============================================================================

/// Wallpaper executor slot: empty until first use.
pub type ExecSlot<S> = std::sync::Arc<Mutex<Option<WallpaperExec<S>>>>;

/// The live executor slot.
pub type LiveSlot = ExecSlot<LiveSys>;

/// The process's wallpaper executor slot — ONE for all window pages.
///
/// WHY ONE. The executor remembers which process is drawing the wallpaper
/// now, what was on the desktop before us and what to restore after a stop.
/// While «Settings» and «Steam» had their own executors, a video set by one
/// page was invisible to the other, which could not stop it, and an apply
/// from the second page put a second host on top of the first.
///
/// WHY IN THE CORE and not in a window module. Everything that must be one
/// per process is already here: the live seam [`LiveSys`] and load
/// supervision ([`spawn_load_monitor`]), which needs exactly this slot. A
/// slot in the module of a single page (as it was in `steam_workers`) would
/// make the other pages depend on it for shared state, and a separate window
/// module would be a second door to the same executor. Here is also the only
/// door for applying with supervision ([`run_in_slot`]).
///
/// ⚠️ Boundary: one slot per COPY of the core. All Rust pages of the window
/// live in one `rexon-main-window` process (the `rexon-gui` crate is linked
/// statically) and see the shared slot.
pub fn live_slot() -> LiveSlot {
    static SLOT: OnceLock<LiveSlot> = OnceLock::new();
    SLOT.get_or_init(|| std::sync::Arc::new(Mutex::new(None))).clone()
}

/// Do work with the slot's executor, creating it on first use (the
/// environment is detected by the seam). Waits for the lock — for background threads.
///
/// A poisoned lock means a background apply panicked; the executor's state
/// is not corrupted by that, it is fine to keep working with it.
pub fn with_slot<S: WpSys + Default, R>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
    work: impl FnOnce(&mut WallpaperExec<S>) -> R,
) -> R {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    let exec = guard.get_or_insert_with(|| WallpaperExec::detect(S::default()));
    work(exec)
}

/// The same without waiting: `None` — the executor is busy right now (with
/// an apply from any page or a supervision sample). For actions from the
/// window thread: waiting for the lock there would freeze the window for the
/// duration of shell commands.
pub fn try_with_slot<S: WpSys + Default, R>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
    work: impl FnOnce(&mut WallpaperExec<S>) -> R,
) -> Option<R> {
    let mut guard = match slot.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    let exec = guard.get_or_insert_with(|| WallpaperExec::detect(S::default()));
    Some(work(exec))
}

/// Applying a wallpaper with the slot's executor and then load supervision —
/// ONE door for all window pages. The lock is released BEFORE supervision:
/// supervision takes it itself on each sample. Whether this wallpaper needs
/// supervision is decided by [`spawn_load_monitor`] (video and YouTube — yes,
/// others — no).
pub fn run_in_slot<S: WpSys + Default + Send + 'static, R>(
    slot: &ExecSlot<S>,
    work: impl FnOnce(&mut WallpaperExec<S>) -> R,
) -> R {
    let out = with_slot(slot, |exec| {
        let out = work(exec);
        exec.sys.start_watch();
        out
    });
    spawn_backdrop(slot.clone());
    spawn_load_monitor(slot.clone());
    out
}

impl<S: WpSys> WallpaperExec<S> {
    /// An executor for an explicitly named environment — the way the checks call it.
    pub fn with_env(sys: S, de: &str, session_type: &str) -> WallpaperExec<S> {
        WallpaperExec {
            sys,
            de: de.to_string(),
            session_type: session_type.to_string(),
            current_format: None,
            current_path: None,
            current_name: String::new(),
            process: None,
            static_holder: None,
            companions: Vec::new(),
            saved_wallpaper: None,
            saved_de: None,
            saved_cinnamon_bg: None,
            saved_kde_plugin: None,
            blur_path: None,
            pending_backdrop: None,
            backdrop_generation: 0,
            quiet_static: false,
            ambient: Vec::new(),
            bridge_live: false,
            bridge_name: String::new(),
            bridge_said_at: None,
            bridge_flag: None,
            perf_paused: false,
            we_note: None,
            host_request: None,
            shell_ext_retired: false,
        }
    }

    /// An executor that detected the environment itself — by the same variables
    /// the engine detects it by.
    pub fn detect(mut sys: S) -> WallpaperExec<S> {
        let de = wpe::detect_de(&sys.env("XDG_CURRENT_DESKTOP"), &sys.env("DESKTOP_SESSION"));
        let session = wpe::detect_session_type(
            &sys.env("XDG_SESSION_TYPE"),
            &sys.env("WAYLAND_DISPLAY"),
            &sys.env("DISPLAY"),
        );
        WallpaperExec::with_env(sys, &de, &session)
    }

    /// Detected environment.
    pub fn de(&self) -> &str {
        &self.de
    }

    /// Detected session type.
    pub fn session_type(&self) -> &str {
        &self.session_type
    }

    /// Session type for the static wallpaper plan: for Deepin it is `treeland` if
    /// the session compositor really is Treeland ([`crate::dde::treeland_session_live`]);
    /// otherwise the Deepin plan is a refusal.
    fn static_session(&self) -> String {
        if self.de == "deepin" && crate::dde::treeland_session_live() {
            wpe::TREELAND_SESSION.to_string()
        } else {
            self.session_type.clone()
        }
    }

    /// Format of what is playing now.
    pub fn current_format(&self) -> Option<&str> {
        self.current_format.as_deref()
    }

    /// Path of what is playing now.
    pub fn current_path(&self) -> Option<&str> {
        self.current_path.as_deref()
    }

    /// Scene name for the bridge (only the one who applied the wallpaper knows it).
    pub fn set_current_name(&mut self, name: &str) {
        self.current_name = name.to_string();
    }

    /// Access to the seam — for the bridge and the checks, to look at the side-effect trail.
    pub fn sys_mut(&mut self) -> &mut S {
        &mut self.sys
    }

    /// Which of the programs the engine cares about exist on the system.
    pub fn plan_tools(&mut self) -> Vec<String> {
        wpe::PLAN_TOOLS
            .iter()
            .filter(|t| self.sys.has_tool(t))
            .map(|t| (*t).to_string())
            .collect()
    }

    /// Package name hints for the current distribution.
    pub fn hints(&mut self) -> wpe::Hints {
        wpe::Hints {
            mpvpaper: self.sys.package_hint("mpvpaper"),
            xwinwrap: self.sys.package_hint("xwinwrap"),
            cvlc: self.sys.package_hint("cvlc"),
            webkit: self.sys.package_hint("webkit2gtk-4.1"),
            swww: self.sys.package_hint("swww"),
            ytdlp: self.sys.package_hint("yt-dlp"),
        }
    }

    // ── plan run ──────────────────────────────────────────────────────────

    /// Execute an engine plan: the first step that produced an outcome is the answer.
    pub fn run_plan(&mut self, steps: &[wpe::Step], path: &str) -> WallpaperResult {
        let mut seen = Seen::new(&self.session_type);
        for step in steps {
            if let Some(res) = self.run_step(step, path, &mut seen) {
                return res;
            }
        }
        // We get here only if the plan ended without any step producing an outcome:
        // engine plans end with a terminal step or a refusal.
        log_line(
            "error",
            &format!("план обоев кончился без исхода ({} шагов)", steps.len()),
        );
        WallpaperResult::fail(crate::i18n::tr("Could not apply the wallpaper"))
    }

    /// One plan step. `None` — the step did not work, try the next one.
    fn run_step(
        &mut self,
        step: &wpe::Step,
        path: &str,
        seen: &mut Seen,
    ) -> Option<WallpaperResult> {
        if step.kind == wpe::step_kind::FAIL {
            let msg = fill_message(&mut self.sys, &step.err_message, seen);
            return Some(WallpaperResult::fail(msg));
        }

        if step.ensure_swww {
            self.ensure_swww_daemon();
        }
        for pre in &step.pre {
            let cmd = fill_argv(&mut self.sys, pre, seen);
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            self.sys.run(&cmd, timeout);
        }

        let cmds = self.spread_argv(step, seen);
        let argv = cmds
            .first()
            .map(|(_, a)| a.clone())
            .unwrap_or_default();

        let (ok, stderr) = match step.kind.as_str() {
            wpe::step_kind::RUN => {
                // There may be several commands — one per output. It counts as success
                // if the wallpaper got set on AT LEAST one: a failure on the second
                // monitor is no reason to redo everything with a fallback engine and
                // lose what is already set on the first.
                let mut any_ok = false;
                let mut first_err = String::new();
                for (output, cmd) in &cmds {
                    let timeout = step
                        .timeout
                        .unwrap_or_else(|| utils::default_timeout_secs(cmd))
                        as f64;
                    let out = self.sys.run(cmd, timeout);
                    let (step_ok, err) = match &out {
                        // No command or timed out: this counts as success only
                        // if the step itself asked for it (`ok_on_none`).
                        None => (step.ok_on_none, String::new()),
                        Some(r) => (r.rc == 0, r.stderr.clone()),
                    };
                    if step_ok {
                        any_ok = true;
                        continue;
                    }
                    if first_err.is_empty() {
                        first_err = err;
                    }
                    if cmds.len() > 1 {
                        log_line(
                            "warning",
                            &format!(
                                "{}: обои не встали на монитор {output}",
                                step.backend
                            ),
                        );
                    }
                }
                (any_ok, first_err)
            }
            wpe::step_kind::PLASMA_SCRIPT => {
                let script = argv.first().cloned().unwrap_or_default();
                (self.plasma_evaluate(&script).is_some(), String::new())
            }
            wpe::step_kind::XFCONF_ALL => {
                return Some(self.run_xfconf(step, &argv, path, seen));
            }
            wpe::step_kind::COSMIC_BG => {
                let ok = self.write_cosmic_bg(path);
                if ok {
                    // Our former swaybg/wbg would lie on top of cosmic-bg.
                    self.kill_static_holder();
                }
                (ok, String::new())
            }
            wpe::step_kind::SPAWN
            | wpe::step_kind::SPAWN_SESSION
            | wpe::step_kind::SPAWN_HOLD => {
                return Some(self.run_spawn(step, &cmds, path, seen));
            }
            other => {
                log_line("error", &format!("неизвестный вид шага плана обоев: {other}"));
                return None;
            }
        };

        let (ok, stderr) = match (&step.verify_ini, ok) {
            (Some(check), true) if !self.ini_written(check, path) => {
                log_line(
                    "warning",
                    &format!("{}: код возврата 0, но обои в {} не записаны", step.backend, check.file),
                );
                (false, crate::i18n::tr("the wallpaper was not saved in the desktop settings"))
            }
            _ => (ok, stderr),
        };

        if ok {
            if step.drops_holder {
                self.kill_static_holder();
            }
            for post in &step.post {
                let cmd = fill_argv(&mut self.sys, post, seen);
                let timeout = utils::default_timeout_secs(&cmd) as f64;
                self.sys.run(&cmd, timeout);
            }
            return Some(self.succeeded(step, path, seen, None));
        }
        self.failed(step, seen, &stderr)
    }

    /// The session's `$XDG_CONFIG_HOME` (or `~/.config`).
    fn config_home(&mut self) -> String {
        let home = self.home();
        let cfg = self.sys.env("XDG_CONFIG_HOME");
        utils::config_home(if cfg.is_empty() { None } else { Some(cfg.as_str()) }, &home)
    }

    /// COSMIC: the picture into cosmic-bg's native key ([`wpe::apply_cosmic_bg`]).
    fn write_cosmic_bg(&mut self, path: &str) -> bool {
        let cfg = self.config_home();
        let sys = std::cell::RefCell::new(&mut self.sys);
        wpe::apply_cosmic_bg(
            &cfg,
            path,
            &mut |p| sys.borrow_mut().read_text(p),
            &mut |p, text| {
                sys.borrow_mut().write_text(p, text);
                true
            },
        )
    }

    /// Whether the program wrote the wallpaper into its own settings (`Step::verify_ini`).
    ///
    /// With pcmanfm-qt the write is done by the already running instance, that
    /// is, not instantly: the file is checked several times with a short pause
    /// (up to ~2 s).
    fn ini_written(&mut self, check: &wpe::IniCheck, path: &str) -> bool {
        let home = self.home();
        let cfg = self.sys.env("XDG_CONFIG_HOME");
        let file = utils::expand_user(
            &check.file,
            &home,
            if cfg.is_empty() { None } else { Some(cfg.as_str()) },
        );
        for attempt in 0..8 {
            if attempt > 0 {
                self.sys.sleep(0.25);
            }
            let text = self.sys.read_text(&file);
            if wpe::ini_check_passes(text.as_deref(), check, path) {
                return true;
            }
        }
        false
    }

    /// The step's commands laid out over outputs: pairs of "output name, command".
    ///
    /// How many commands there will be is decided by [`ENGINE_SPREAD`] — a
    /// property of the engine itself, not a guess from the look of the command.
    /// The output name in the pair is only for the journal: an empty string
    /// means "the command is shared, it says nothing about outputs".
    ///
    /// ⚠️ One foolproofing rule for all branches: as soon as something is
    /// missing — outputs not enumerated, no query tool, no output placeholders
    /// in the command — return EXACTLY the command that was in the plan. The
    /// wallpaper gets set the same way as before the layout appeared, instead of
    /// not being set at all.
    fn spread_argv(&mut self, step: &wpe::Step, seen: &mut Seen) -> Vec<(String, Vec<String>)> {
        let whole = |sys: &mut S, seen: &mut Seen| {
            vec![(String::new(), fill_argv(sys, &step.argv, seen))]
        };
        match engine_spread(&step.backend) {
            Spread::Whole => whole(&mut self.sys, seen),
            Spread::List { flag, sep } => {
                let outputs = seen.outputs(&mut self.sys);
                let mut argv = fill_argv(&mut self.sys, &step.argv, seen);
                let names: Vec<&str> =
                    outputs.iter().map(|o| o.name.as_str()).collect();
                // Nothing to list; the engine already named the outputs itself; the
                // output name is unknown to us ("all") — in all three cases the
                // command goes out as it was.
                if names.is_empty()
                    || argv.iter().any(|a| a == flag)
                    || names.contains(&ANY_OUTPUT)
                {
                    return vec![(String::new(), argv)];
                }
                let list = names.join(sep);
                argv.push(flag.to_string());
                argv.push(list.clone());
                vec![(list, argv)]
            }
            Spread::Each => {
                let marked = step.argv.iter().any(|a| asks_about_output(a));
                if !marked {
                    // Repeating the same command adds nothing:
                    // it has no way to choose an output.
                    log_line(
                        "debug",
                        &format!(
                            "{}: в команде не названо, на какой монитор ставить;                              ставлю одной командой",
                            step.backend
                        ),
                    );
                    return whole(&mut self.sys, seen);
                }
                let outputs = seen.outputs(&mut self.sys);
                if outputs.len() < 2 {
                    // One output or none — one command, its placeholders
                    // are filled by the common path.
                    return whole(&mut self.sys, seen);
                }
                // ⚠️ Foolproofing: the command chooses the monitor by geometry,
                // and the query did not say where the monitors stand. A second
                // command would land on top of the first — one goes out, as before.
                let by_geometry = step
                    .argv
                    .iter()
                    .any(|a| a.contains(wpe::GEOMETRY_MARK));
                if by_geometry && outputs.iter().any(|o| o.position.is_none()) {
                    log_line(
                        "warning",
                        &format!(
                            "{}: опрос не сказал, где стоят мониторы — ставлю \
                             обои одной командой, на один экран",
                            step.backend
                        ),
                    );
                    return whole(&mut self.sys, seen);
                }
                let mut cmds = Vec::with_capacity(outputs.len());
                for one in outputs {
                    let name = one.name.clone();
                    seen.pinned = Some(one);
                    cmds.push((name, fill_argv(&mut self.sys, &step.argv, seen)));
                }
                seen.pinned = None;
                cmds
            }
        }
    }

    /// Xfce: list the background properties and write the path into each `/last-image`.
    fn run_xfconf(
        &mut self,
        step: &wpe::Step,
        argv: &[String],
        path: &str,
        seen: &mut Seen,
    ) -> WallpaperResult {
        let timeout = utils::default_timeout_secs(argv) as f64;
        let out = self.sys.run(argv, timeout);
        let stdout = match &out {
            Some(r) if !r.stdout.is_empty() => r.stdout.clone(),
            _ => return WallpaperResult::fail(step.err_message.clone()),
        };
        let Some(setter) = step.post.first() else {
            return WallpaperResult::fail(step.err_message.clone());
        };
        let mut set_any = false;
        for line in stdout.lines() {
            if !line.contains("/last-image") {
                continue;
            }
            let prop = line.trim();
            let cmd: Vec<String> = setter.iter().map(|a| a.replace("{prop}", prop)).collect();
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            self.sys.run(&cmd, timeout);
            set_any = true;
        }
        if set_any {
            return self.succeeded(step, path, seen, None);
        }
        WallpaperResult::fail(step.alt_message.clone())
    }

    /// Launching the wallpaper process (or the daemon holding the background).
    ///
    /// There may be several commands — one per monitor (engines like `mpvpaper`
    /// accept exactly one output). The first holds the desktop and lives as "the"
    /// wallpaper process, the rest go as companions: they are stopped, frozen
    /// and unfrozen together with it.
    fn run_spawn(
        &mut self,
        step: &wpe::Step,
        cmds: &[(String, Vec<String>)],
        path: &str,
        seen: &mut Seen,
    ) -> WallpaperResult {
        let kind = step.kind.as_str();
        if kind == wpe::step_kind::SPAWN_SESSION {
            // The previous live wallpaper is stopped without rolling back the background:
            // the new one goes up right away, and a rollback would flash the old picture.
            self.kill_running();
        }
        if kind == wpe::step_kind::SPAWN_HOLD {
            self.kill_static_holder();
        }
        self.kill_companions();
        let new_session = kind != wpe::step_kind::SPAWN;
        let empty: Vec<String> = Vec::new();
        let argv = cmds.first().map(|(_, a)| a).unwrap_or(&empty);
        let proc = match self.sys.spawn(argv, new_session) {
            Ok(p) => p,
            Err(e) => {
                log_line("error", &format!("не удалось запустить {}: {e}", step.backend));
                return self
                    .failed(step, seen, &e)
                    .unwrap_or_else(|| WallpaperResult::fail(e));
            }
        };

        // The launch succeeded — that does NOT yet mean the wallpaper is set: all
        // `spawn*` steps start a process that draws (or holds) the desktop while it
        // lives, and a dead one dies within milliseconds. The grace period and its
        // justification live in the engine, there is no copy of it here.
        if let Some(rc) = self.wait_alive(proc) {
            log_line(
                "error",
                &format!("процесс {} умер сразу после запуска (код {rc})", step.backend),
            );
            let died = wpe::spawn_died_message(Some(rc as i32));
            self.sys.kill_process(proc);
            return self
                .failed(step, seen, &died)
                .unwrap_or_else(|| WallpaperResult::fail(died));
        }

        // The other monitors. A failure on them does NOT bring down the whole step:
        // the wallpaper is already running on the first, and moving the plan to a
        // fallback engine would turn off what got set.
        for (output, cmd) in cmds.iter().skip(1) {
            let extra = match self.sys.spawn(cmd, new_session) {
                Ok(p) => p,
                Err(e) => {
                    log_line(
                        "warning",
                        &format!("{}: не запустился на мониторе {output}: {e}", step.backend),
                    );
                    continue;
                }
            };
            if let Some(rc) = self.wait_alive(extra) {
                log_line(
                    "warning",
                    &format!(
                        "{}: на мониторе {output} процесс умер сразу (код {rc})",
                        step.backend
                    ),
                );
                self.sys.kill_process(extra);
                continue;
            }
            self.companions.push(extra);
        }

        if kind == wpe::step_kind::SPAWN_HOLD {
            // swaybg/wbg do not "set and exit" — they HOLD the background while alive.
            self.static_holder = Some(proc);
            return self.succeeded(step, path, seen, None);
        }
        self.process = Some(proc);
        let pid = self.sys.pid(proc);
        self.succeeded(step, path, seen, pid)
    }

    /// Did the process survive the grace period? `None` — alive, otherwise the exit code.
    fn wait_alive(&mut self, proc: ProcHandle) -> Option<i64> {
        let deadline = self.sys.monotonic() + wpe::SPAWN_ALIVE_GRACE_SEC;
        loop {
            if let Some(rc) = self.sys.poll(proc) {
                return Some(rc);
            }
            let left = deadline - self.sys.monotonic();
            if left <= 0.0 {
                return None;
            }
            self.sys.sleep(wpe::ALIVE_POLL_STEP_SEC.min(left));
        }
    }

    fn succeeded(
        &mut self,
        step: &wpe::Step,
        path: &str,
        seen: &mut Seen,
        pid: Option<i64>,
    ) -> WallpaperResult {
        if self.quiet_static {
            log_line("info", &format!("подложка поставлена: путь {}", step.backend));
            let message = fill_message(&mut self.sys, &step.ok_message, seen);
            return WallpaperResult { ok: true, message, pid, warning: None };
        }
        self.current_format = Some(step.format.clone());
        self.current_path = Some(path.to_string());
        let fmt = step.format.clone();
        // Which path set the wallpaper — only for the journal: the internal path
        // name is not shown to the customer (`WALLPAPER_SET_MESSAGE`).
        log_line(
            "info",
            &format!("обои поставлены: путь {}, формат {fmt}", step.backend),
        );
        self.notify_live_wallpaper(Some(&fmt), true);
        let mut message = fill_message(&mut self.sys, &step.ok_message, seen);
        if let Some(pid) = pid {
            message = message.replace("{pid}", &crate::i18n::isolate(&pid.to_string()));
        }
        WallpaperResult { ok: true, message, pid, warning: None }
    }

    fn failed(
        &mut self,
        step: &wpe::Step,
        seen: &mut Seen,
        stderr: &str,
    ) -> Option<WallpaperResult> {
        if !step.terminal {
            return None;
        }
        let message = fill_message(&mut self.sys, &step.err_message, seen);
        Some(WallpaperResult::fail(message.replace("{stderr}", &crate::i18n::isolate(stderr))))
    }

    // ── shared run helpers ────────────────────────────────────────────────

    /// Start the swww/awww daemon if it is not answering yet.
    ///
    /// The `init` subcommand was removed in recent versions — there the daemon is
    /// started as a separate process, so we try both ways. Binary names (`swww`
    /// → `awww` after the upstream rename) are resolved by the shared tool
    /// resolver; there is no second name table here.
    fn ensure_swww_daemon(&mut self) {
        let query = vec!["swww".to_string(), "query".to_string()];
        if let Some(r) = self.sys.run(&query, wpe::SWWW_QUERY_TIMEOUT_SEC) {
            if r.rc == 0 {
                return; // the daemon is already answering
            }
        }
        let init = vec!["swww".to_string(), "init".to_string()];
        if let Some(r) = self.sys.run(&init, wpe::SWWW_INIT_TIMEOUT_SEC) {
            if r.rc == 0 {
                return;
            }
        }
        if !self.sys.has_tool("swww-daemon") {
            return;
        }
        if self
            .sys
            .spawn(&["swww-daemon".to_string()], true)
            .is_ok()
        {
            self.sys.sleep(wpe::SWWW_DAEMON_SETTLE_SEC);
        }
    }

    /// Run JS in plasmashell. `None` — it did not work; otherwise what was printed.
    fn plasma_evaluate(&mut self, script: &str) -> Option<String> {
        let argv = wpe::plasma_dbus_reply_argv(script);
        let out = self.sys.run(&argv, wpe::PLASMA_SCRIPT_TIMEOUT_SEC)?;
        if out.rc != 0 {
            return None;
        }
        Some(out.stdout)
    }

    /// Stop the previously started wallpaper process without touching anything else.
    ///
    /// The difference from a stop: that one also restores the static wallpaper,
    /// desktop icons and background sounds. Here the wallpaper is replaced by a
    /// new one RIGHT AWAY, so there is nothing to roll back — otherwise the old
    /// background would flash during the change.
    fn kill_running(&mut self) {
        self.resume_if_paused();
        self.kill_companions();
        let Some(proc) = self.process.take() else {
            return;
        };
        if let Some(pid) = self.sys.pid(proc) {
            log_line(
                "info",
                &format!("останавливаю прежние обои PID={pid} перед запуском новых"),
            );
        }
        self.sys.kill_process(proc);
    }

    /// Stop the wallpaper processes on the other monitors.
    fn kill_companions(&mut self) {
        for proc in std::mem::take(&mut self.companions) {
            self.sys.kill_process(proc);
        }
    }

    /// Unfreeze the wallpaper process if supervision froze it.
    fn resume_if_paused(&mut self) {
        if !self.perf_paused {
            return;
        }
        if let Some(proc) = self.process {
            self.sys.pause_process(proc, false);
        }
        self.pause_companions(false);
        self.perf_paused = false;
    }

    /// Freeze or unfreeze the wallpaper on the other monitors — the same thing
    /// supervision does with the main process.
    fn pause_companions(&mut self, pause: bool) {
        for proc in self.companions.clone() {
            self.sys.pause_process(proc, pause);
        }
    }

    // ── load supervision: decisions ───────────────────────────────────────

    /// Which process supervision should watch: handle and PID. `None` — there is
    /// no live wallpaper of our own process right now.
    pub fn monitor_target(&mut self) -> Option<(ProcHandle, i64)> {
        let proc = self.process?;
        if self.sys.poll(proc).is_some() {
            return None;
        }
        let pid = self.sys.pid(proc)?;
        Some((proc, pid))
    }

    /// Apply a supervision sample — the body of the `_performance_monitor_loop` loop.
    ///
    /// The order is as in Python: the process changed — nothing to watch; it
    /// crashed — restore the desktop; CPU above the threshold for
    /// [`wpe::CPU_HIGH_STRIKES`] samples in a row — forced stop; low battery or
    /// a full-screen window — freeze, the reason went away — unfreeze.
    pub fn monitor_apply(
        &mut self,
        watched: ProcHandle,
        cpu_strikes: &mut i64,
        obs: &LoadObservation,
    ) -> MonitorVerdict {
        if self.process != Some(watched) {
            return MonitorVerdict::Done;
        }
        if let Some(rc) = self.sys.poll(watched) {
            // Our own host is brought back by the crash watcher (`renderer_watch`):
            // restoring the desktop here would flash the previous picture before the
            // restart and leave the watcher with nothing to bring back.
            if self.host_request.is_some() {
                log_line("warning", &format!("хост обоев завершился сам (код {rc}) — решает сторож падений"));
                return MonitorVerdict::Done;
            }
            self.on_process_crashed(rc);
            return MonitorVerdict::Done;
        }
        *cpu_strikes = if obs.cpu_high { *cpu_strikes + 1 } else { 0 };
        if *cpu_strikes >= wpe::CPU_HIGH_STRIKES {
            log_line(
                "error",
                "процесс обоев слишком долго грузит процессор выше порога — принудительная остановка",
            );
            self.stop(true);
            return MonitorVerdict::Done;
        }
        let should_pause = obs.battery_low || obs.fullscreen;
        if should_pause && !self.perf_paused {
            self.sys.pause_process(watched, true);
            self.pause_companions(true);
            self.perf_paused = true;
            let reason = if obs.battery_low { "низкий заряд" } else { "полноэкранное окно" };
            log_line("info", &format!("надзор за нагрузкой: пауза ({reason})"));
        } else if !should_pause && self.perf_paused {
            self.sys.pause_process(watched, false);
            self.pause_companions(false);
            self.perf_paused = false;
            log_line("info", "надзор за нагрузкой: возобновление");
        }
        MonitorVerdict::Continue
    }

    /// The live wallpaper process crashed by itself — port of `_on_process_crashed`:
    /// return the desktop to the state a stop by the button would.
    fn on_process_crashed(&mut self, rc: i64) {
        log_line(
            "warning",
            &format!("процесс живых обоев упал (код {rc}) — возвращаю рабочий стол"),
        );
        if let Some(proc) = self.process.take() {
            // The process is already dead: stopping only reaps its status.
            self.sys.kill_process(proc);
        }
        self.kill_companions();
        self.perf_paused = false;
        self.set_desktop_icons(true);
        self.restore_wallpaper();
        self.blur_path = None;
        self.current_format = None;
        self.current_path = None;
        self.current_name.clear();
        self.remove_pidfile();
        // For the bridge listener a crash is the same stop as by the button.
        self.notify_live_wallpaper(None, true);
    }

    // ── host crash watcher: execution ─────────────────────────────────────
    //
    // `renderer_watch::HostWatch` decides; here is only what it sees and what
    // carries out its decisions.

    /// What is visible about our own host right now.
    pub fn host_snapshot(&mut self) -> HostSnapshot {
        let Some(req) = self.host_request.clone() else {
            return HostSnapshot::default();
        };
        // The format changed bypassing apply (a crash not of our own host, etc.)
        // — nothing to watch.
        if self.current_format.as_deref() != Some(req.fmt.as_str()) {
            return HostSnapshot::default();
        }
        let mut snap = HostSnapshot {
            subject: Some(format!("{}:{}", req.fmt, req.path)),
            paused: self.perf_paused,
            host_binary: self.sys.wallpaper_host(),
            ..HostSnapshot::default()
        };
        match self.process {
            Some(main) => {
                for proc in std::iter::once(main).chain(self.companions.clone()) {
                    if self.sys.poll(proc).is_some() {
                        snap.exited = true;
                    } else if let Some(pid) = self.sys.pid(proc) {
                        snap.pids.push(pid);
                    }
                }
            }
            None if wpe::needs_shell_extension(&self.de, &self.session_type) => {
                snap.via_extension = true;
            }
            // Neither a process nor an extension: it is not the host showing (the
            // plan's fallback path) — the watcher has nothing to bring back.
            None => snap.subject = None,
        }
        snap
    }

    /// Bring the same wallpaper back up — with the same apply as the button.
    pub fn restart_host(&mut self) -> WallpaperResult {
        let Some(req) = self.host_request.clone() else {
            // Only for the watcher's journal: this result is not shown to the person.
            return WallpaperResult::fail("nothing to restart".to_string());
        };
        if self.process.is_none() && wpe::needs_shell_extension(&self.de, &self.session_type) {
            // The extension restarts the host when its settings change, and the same
            // value gives no change: first turn it off (empty source), then the apply
            // writes the source again.
            self.shell_extension_stop();
        }
        self.apply_with_audio(&req.path, &req.fmt, &req.name, req.audio)
    }

    /// The wallpaper keeps crashing: stop it and set a still frame of the same
    /// wallpaper. True — the frame is set; false — there is no frame, the
    /// previous desktop picture was restored (as by «Stop»).
    pub fn show_still_frame(&mut self) -> bool {
        let req = self.host_request.take();
        self.resume_if_paused();
        if let Some(proc) = self.process.take() {
            self.sys.kill_process(proc);
        }
        self.kill_companions();
        if wpe::needs_shell_extension(&self.de, &self.session_type) {
            self.shell_extension_stop();
        }
        self.remove_pidfile();
        let still = req.as_ref().and_then(|r| self.still_frame_for(r));
        if let Some(still) = still {
            let tools = self.plan_tools();
            let hint = self.sys.package_hint("swww");
            let plan = wpe::static_plan(&self.de, &self.static_session(), &still, &tools, &hint);
            if self.run_plan(&plan, &still).ok {
                // The previous picture stays remembered: «Stop» and the next live
                // wallpaper will restore exactly it, not the frame.
                self.current_format = Some("static".to_string());
                self.current_path = Some(still);
                self.blur_path = None;
                self.notify_live_wallpaper(None, true);
                return true;
            }
        }
        log_line("warning", "кадра обоев нет — возвращаю прежнюю картинку стола");
        self.set_desktop_icons(true);
        self.restore_wallpaper();
        self.restore_kde_wallpaper_plugin();
        self.blur_path = None;
        self.current_format = None;
        self.current_path = None;
        self.current_name.clear();
        self.notify_live_wallpaper(None, true);
        false
    }

    /// A still frame of the wallpaper: for video a frame from the movie (into the
    /// cache), for web and Flash the preview next to the wallpaper. `None` — no frame.
    fn still_frame_for(&mut self, req: &HostRequest) -> Option<String> {
        if req.fmt == "video" {
            // The same frame and the same cache as the backdrop: usually it is already ready.
            let cache = self.cache_home();
            let session = self.session_type.clone();
            return prepare_video_still(&mut self.sys, &cache, &session, &req.path);
        }
        let project = crate::renderer_watch::still_frame_dir(&req.path)
            .and_then(|dir| self.sys.read_text(&format!("{dir}/project.json")));
        crate::renderer_watch::still_frame_candidates(&req.path, project.as_deref())
            .into_iter()
            .find(|c| self.sys.exists(c) && !self.sys.is_dir(c))
    }

    /// Turn off the shell extension's wallpaper (an empty source in its settings).
    fn shell_extension_stop(&mut self) {
        let Some(argv) = wpe::shell_ext_stop_argv(&self.de) else { return };
        let argv = match self.shell_extension_schema_dir() {
            Some(dir) => wpe::with_schemadir(&argv, &dir),
            None => argv,
        };
        let timeout = utils::default_timeout_secs(&argv) as f64;
        self.sys.run(&argv, timeout);
    }

    /// Cinnamon with layer-shell: our extension does not show the wallpaper, yet
    /// anything left in its keys from the old path (before 6.8, before the move
    /// to the desktop layer) wakes up the host in it as a SECOND process next to
    /// ours — live on 15.11.2026 two videos ran at once this way, the second from
    /// the `source` key of a morning apply. We disable the extension if there is
    /// no screen effect (foreign uuids are left alone) and clear the wallpaper in
    /// its keys. Once per process: after this the keys are written only by the
    /// extension path, and it does not run here. Bringing the extension back on
    /// Cinnamon without layer-shell needs nothing special — that path enables
    /// the disabled one itself.
    fn retire_shell_extension_wallpaper(&mut self) {
        if self.shell_ext_retired || wpe::shell_ext_schema(&self.de).is_none() || self.de == "gnome" {
            return;
        }
        self.shell_ext_retired = true;
        let Some(schema) = wpe::shell_ext_schema(&self.de) else { return };
        // The abandoned copy from an earlier version (live on 15.11.2026 it was the
        // one waking the second host) is removed BEFORE reading the keys: the
        // schema directory is then the system one. The same function as for screen effects.
        let de = self.de.clone();
        let orphan_removed = self.sys.remove_orphan_shell_ext_copy(&de).is_some();
        let schema_dir = self.shell_extension_schema_dir();
        let gsettings = |cmd: Vec<String>| match &schema_dir {
            Some(dir) => wpe::with_schemadir(&cmd, dir),
            None => cmd,
        };
        let ask = |sys: &mut S, cmd: Vec<String>| {
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            sys.run(&cmd, timeout).filter(|o| o.rc == 0).map(|o| o.stdout)
        };
        let source = ask(&mut self.sys, gsettings(["gsettings", "get", schema, "source"].map(String::from).to_vec()));
        let effect = ask(&mut self.sys, gsettings(["gsettings", "get", schema, "effect-shader"].map(String::from).to_vec()));
        let enabled = ask(&mut self.sys, wpe::shell_ext_state_argv(&self.de));
        let plan = wpe::shell_ext_retire_plan(&self.de, source.as_deref(), effect.as_deref(), enabled.as_deref());
        let disabling = plan.disable_argv.is_some();
        if let Some(cmd) = plan.disable_argv {
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            let ok = matches!(self.sys.run(&cmd, timeout), Some(ref o) if o.rc == 0);
            log_line(
                if ok { "info" } else { "warning" },
                &format!(
                    "расширение обоев Cinnamon выключено: обои слоем стола, эффекта экрана нет{}",
                    if ok { "" } else { " — не вышло" }
                ),
            );
        }
        // The copy was removed, but the extension stays (a screen effect is on) — let
        // the shell re-read it from disk, now the system one: in memory it has the
        // code of the removed copy. As in the screen effects fix.
        if orphan_removed && !disabling && enabled.as_deref().is_some_and(|o| o.contains(&format!("'{}'", wpe::SHELL_EXT_ID))) {
            let cmd = crate::screen_effects::cinnamon_reload_argv();
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            self.sys.run(&cmd, timeout);
        }
        if !plan.clear_argvs.is_empty() {
            for cmd in plan.clear_argvs {
                let cmd = gsettings(cmd);
                let timeout = utils::default_timeout_secs(&cmd) as f64;
                self.sys.run(&cmd, timeout);
            }
            log_line("info", "обои в ключах расширения Cinnamon очищены: их показывает Rexon слоем стола");
        }
    }

    /// Stop the background-holding process (swaybg/wbg) if it is ours.
    fn kill_static_holder(&mut self) {
        if let Some(proc) = self.static_holder.take() {
            self.sys.kill_process(proc);
        }
    }

    /// Tell the bridge whether live wallpapers are running now.
    ///
    /// Which format counts as live and when the notice is suppressed by the
    /// silence threshold is decided by the engine; here is only the delivery.
    fn notify_live_wallpaper(&mut self, fmt: Option<&str>, announce: bool) {
        let live = fmt.map(wpe::is_live_format).unwrap_or(false);
        let name = if live {
            self.current_name.trim().to_string()
        } else {
            String::new()
        };
        let now = self.sys.monotonic();
        let say = announce
            && wpe::bridge_should_announce(
                live,
                &name,
                self.bridge_live,
                &self.bridge_name,
                self.bridge_said_at,
                now,
            );
        // Neither a notice nor news in the flag — do not touch the bridge at all:
        // otherwise every playlist picture would rewrite the same thing for it.
        if !say && Some(live) == self.bridge_flag {
            return;
        }
        self.bridge_flag = Some(live);
        if say {
            self.bridge_live = live;
            self.bridge_name = name.clone();
            self.bridge_said_at = Some(now);
        }
        self.sys.bridge_notify(live, &name, say);
    }

    // ── apply and stop ────────────────────────────────────────────────────

    /// Apply a wallpaper: path and format.
    ///
    /// `name` — the scene name FOR A PERSON, if the caller knows it. Empty is an
    /// honest "I don't know": making up a name from the file name is not
    /// allowed, it would go straight into someone else's reply via the bridge.
    ///
    /// ⚠️ THE BOUNDARY, STATED OUT LOUD. Only Flash (SWF) has not been carried
    /// over — for it the executor ANSWERS with the [`live_formats_not_carried`]
    /// line rather than pretending the wallpaper was set (rule #13). Scenes,
    /// application wallpapers and an unknown format get a final refusal with the
    /// same words as in Python — see [`carries_format`].
    ///
    /// Sound for video and YouTube is the default [`VIDEO_AUDIO_DEFAULT`]; whoever
    /// gives the person a choice calls [`WallpaperExec::apply_with_audio`].
    /// Refusal if a foreign shell owns the wallpaper layer.
    ///
    /// ⚠️ A rule #12 junction: BOTH public apply entries call it
    /// ([`WallpaperExec::apply_with_audio`] — pictures, GIF, video, web, Flash;
    /// [`WallpaperExec::apply_youtube`]) and BEFORE doing anything: the previous
    /// wallpaper is not stopped, the desktop is not remembered. Putting a
    /// wallpaper under a foreign surface and reporting success is exactly what
    /// must not happen; the text tells the person what to press («Give the
    /// wallpaper to Rexon»).
    fn shell_owner_refusal(&mut self) -> Option<WallpaperResult> {
        let (de, session) = (self.de.clone(), self.session_type.clone());
        let shell = self.sys.wallpaper_shell_owner(&de, &session)?;
        log_line(
            "info",
            &format!("обои не применены: слой обоев держит шелл {}", shell.as_str()),
        );
        Some(WallpaperResult::fail(wpe::shell_owner_message(shell)))
    }

    pub fn apply(&mut self, path: &str, fmt: &str, name: &str) -> WallpaperResult {
        self.apply_with_audio(path, fmt, name, VIDEO_AUDIO_DEFAULT)
    }

    /// Apply a wallpaper with a sound choice — port of `apply(path, fmt, audio=…)`.
    ///
    /// One path for all callers: `apply` is the same thing with the default.
    /// `audio` affects video from a file and YouTube with default quality and
    /// mode (YouTube with a quality and mode choice has its own entry
    /// [`WallpaperExec::apply_youtube`], with sound as the same argument); other
    /// formats do not need sound and do not read it. `false` — the movie plays
    /// muted (`--no-audio` to the host or to the shell extension).
    ///
    /// ⚠️ Flash (`swf`) does NOT read this argument, and that is not an omission.
    /// `audio` here means "THIS wallpaper is meant to have sound" and comes from
    /// the video wallpaper's `project.json` or from the Steam background check
    /// box; SWF has neither — a Flash file is put as is. Flash wallpaper sound
    /// lives entirely in the shared settings (`wallpaper_audio_*`) and changes on
    /// the fly, and a one-off "mute this wallpaper" is given to the host itself
    /// with the `--no-audio` switch.
    pub fn apply_with_audio(
        &mut self,
        path: &str,
        fmt: &str,
        name: &str,
        audio: bool,
    ) -> WallpaperResult {
        self.we_note = None;
        self.forget_pending_backdrop();
        let mut res = self.apply_with_audio_inner(path, fmt, name, audio);
        if res.ok && fmt == "static" {
            self.sys.static_wallpaper_changed(path);
        }
        // For the crash watcher — what to bring back up if the host goes down. The
        // person's REQUEST is remembered (the original path, not the internal video
        // copy): the restart goes through the same apply as the button.
        if res.ok {
            self.host_request = host_watched_format(fmt).then(|| HostRequest {
                path: path.to_string(),
                fmt: fmt.to_string(),
                name: name.to_string(),
                audio,
            });
        }
        // Video, web and Flash are shown by our host, and it will report its surface
        // itself — when the first frame is on screen (`cinnamon_layer` in
        // rexon-gui). Ordering it from here, with a deadline from process launch,
        // only got in the way: on 15.11.2026 the window and the host restacked the
        // background service almost simultaneously, and the host caught it in the
        // middle of a restart as a foreign one.
        if res.ok
            && own_layer_surface_format(fmt)
            && !host_watched_format(fmt)
            && self.session_type == "wayland"
            && !wpe::needs_shell_extension(&self.de, &self.session_type)
        {
            self.sys.own_layer_surface_up();
        }
        // Cinnamon with layer-shell: the wallpaper is a desktop layer, the extension
        // is not needed for it — clear the old path's leftovers from it (a redundant second host).
        if res.ok && self.sys.cinnamon_layer_shell() {
            self.retire_shell_extension_wallpaper();
        }
        self.attach_we_note(&mut res);
        self.attach_desktop_icons_note(&mut res, fmt);
        res
    }

    /// A live wallpaper as a host window on X11 went above or below the desktop
    /// icons window — prepare the desktop and tell the person what happens to the
    /// icons (`wallpaper_backend::prepare_desktop_icons`). Only for host-window
    /// wallpapers: pictures create no window, and GNOME/Cinnamon-Wayland show the
    /// wallpaper with a shell extension.
    fn attach_desktop_icons_note(&mut self, res: &mut WallpaperResult, fmt: &str) {
        if !res.ok || !matches!(fmt, "video" | "web" | "swf") || wpe::needs_shell_extension(&self.de, &self.session_type) {
            return;
        }
        let Some(owner) = crate::wallpaper_backend::desktop_icons_owner(&self.de, &self.session_type) else {
            return;
        };
        let mut io = DesktopIconsSys::new(&mut self.sys);
        if let Some(note) = crate::wallpaper_backend::prepare_desktop_icons(&mut io, owner) {
            res.message = format!("{} {note}", res.message).trim().to_string();
        }
    }

    /// Stop live WE wallpapers before our own (see [`yields_we_wallpapers`]) and
    /// remember the note if stopping did not work.
    fn yield_we_wallpapers(&mut self, fmt: &str) {
        if !yields_we_wallpapers(fmt) {
            return;
        }
        let stopped = self.sys.stop_we_wallpapers(crate::we_client::STOP_TIMEOUT_MS);
        // The waywallen output — also when there was nobody to stop the renderers
        // (a process without Qt): its placeholder would cover our wallpaper even without them.
        self.sys.release_waywallen_display();
        let Some((status, killed)) = stopped else {
            return;
        };
        if status == crate::we_client::STOP_STOPPED && killed > 0 {
            log_line("info", &format!("сняты живые обои Wallpaper Engine: {killed}"));
        }
        self.we_note = we_yield_note(&status);
        if self.we_note.is_some() {
            log_line("warn", "живые обои Wallpaper Engine снять не удалось — свои лягут под них");
        }
    }

    /// Mark a successful result with the "the WE wallpaper may have stayed on top" warning.
    fn attach_we_note(&mut self, res: &mut WallpaperResult) {
        if let Some(note) = self.we_note.take() {
            if res.ok {
                res.warning = Some(note);
            }
        }
    }

    /// Whether the apply will get to work: without the file or the wallpaper
    /// host — a refusal, and the other wallpaper is not stopped for the sake of a refusal.
    fn apply_can_start(&mut self, path: &str, fmt: &str) -> bool {
        match fmt {
            "static" | "animated" => self.sys.exists(path),
            "video" => self.sys.exists(path) && self.sys.wallpaper_host().is_some(),
            "swf" => {
                self.sys.wallpaper_host().is_some() && self.sys.exists(path) && !self.sys.is_dir(path)
            }
            "web" => self.sys.wallpaper_host().is_some(),
            _ => true,
        }
    }

    fn apply_with_audio_inner(
        &mut self,
        path: &str,
        fmt: &str,
        name: &str,
        audio: bool,
    ) -> WallpaperResult {
        self.current_name = name.to_string();
        if host_watched_format(fmt) {
            self.sys.note_apply_started();
        }

        if !carries_format(fmt) {
            return WallpaperResult::fail(format!(
                "{}: {}",
                wpe::format_label(fmt),
                live_formats_not_carried()
            ));
        }
        // ⭐ A fix during the port: Python gave this refusal AFTER it had stopped
        // the previous live wallpaper and remembered the desktop — a scene tile from
        // the library silently turned off a running video. Refusal comes before doing anything.
        if let Some(refusal) = retired_format_refusal(fmt) {
            log_line("info", &format!("обои не применены: формат={fmt}: {refusal}"));
            return WallpaperResult::fail(refusal);
        }
        if let Some(refusal) = self.shell_owner_refusal() {
            return refusal;
        }
        // YouTube stops them itself (`apply_youtube`): it has its own checks.
        if fmt != "youtube" && self.apply_can_start(path, fmt) {
            self.yield_we_wallpapers(fmt);
        }
        if fmt == "web" {
            return self.apply_web(path);
        }
        if fmt == "youtube" {
            // Without a quality and mode choice — the Python defaults: 1080p, stream.
            return self.apply_youtube(path, "1080p", "stream", audio);
        }
        if fmt == "swf" {
            return self.apply_swf(path);
        }
        if fmt == "video" {
            if !self.sys.exists(path) {
                return WallpaperResult::fail(wpe::file_missing_message(path));
            }
            if self.process.is_some() {
                self.stop(false);
            }
            self.save_current_wallpaper();
            log_line(
                "info",
                &format!(
                    "применяю видеообои: окружение={}, сессия={}, путь={path}",
                    self.de, self.session_type
                ),
            );
            let res = self.apply_video(path, audio);
            if res.ok {
                self.write_pidfile();
            }
            return res;
        }
        if !self.sys.exists(path) {
            return WallpaperResult::fail(wpe::file_missing_message(path));
        }
        if fmt == "animated" {
            return self.apply_animated(path);
        }
        // The previous live wallpaper is stopped WITHOUT a notice to the bridge: the
        // wallpaper is about to be replaced, and a "stopped — started" pair would mislead the listener.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();

        log_line(
            "info",
            &format!(
                "применяю обои: формат={fmt}, окружение={}, сессия={}, путь={path}",
                self.de, self.session_type
            ),
        );
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::static_plan(&self.de, &self.static_session(), path, &tools, &hint);
        let res = self.run_plan(&plan, path);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// Root of the user cache (`$XDG_CACHE_HOME`, otherwise `~/.cache`).
    fn cache_home(&mut self) -> String {
        let c = self.sys.env("XDG_CACHE_HOME");
        if c.is_empty() {
            format!("{}/.cache", self.home())
        } else {
            c
        }
    }

    /// GIF/APNG animation — port of `_apply_animated`: a blurred backdrop, then
    /// the engine plan (`animated_plan`: swww, otherwise mpvpaper on Wayland;
    /// xwinwrap + mpv on X11). GIF has no load supervision — as in Python.
    ///
    /// ⭐ A fix during the port: where the plan is a single refusal (GNOME and
    /// Cinnamon-Wayland: there is no desktop layer for swww and mpvpaper),
    /// Python managed to stop the previous live wallpaper and set a blurred
    /// backdrop, and only then refused — the desktop stayed a blurred GIF frame.
    /// Here the refusal is given before doing anything.
    fn apply_animated(&mut self, path: &str) -> WallpaperResult {
        let tools = self.plan_tools();
        let hints = self.hints();
        let plan = wpe::animated_plan(
            &self.de,
            &self.session_type,
            path,
            &tools,
            "{monitor}",
            wpe::GEOMETRY_MARK,
            &hints,
        );
        if plan.first().is_some_and(|s| s.kind == wpe::step_kind::FAIL) {
            return self.run_plan(&plan, path);
        }
        // The previous live wallpaper — without a notice to the bridge: it is about to be replaced.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        log_line(
            "info",
            &format!(
                "применяю GIF-обои: окружение={}, сессия={}, путь={path}",
                self.de, self.session_type
            ),
        );
        let timing = self.backdrop_before_show(path);
        let res = self.run_plan(&plan, path);
        if res.ok {
            self.write_pidfile();
            self.queue_backdrop_after_show(timing, path);
        }
        res
    }

    /// Video wallpaper with our own host process — port of `_apply_video`.
    ///
    /// The host is looked up FIRST: if not found, there is nothing to show with,
    /// neither our own window nor a shell extension, and no point preparing the
    /// file for nothing. Then, as in Python: file check, faststart remux,
    /// remembering the KDE desktop picture, showing — with a shell extension
    /// where there is no wlr-layer-shell (GNOME, Cinnamon-Wayland), otherwise
    /// with our own window. The backdrop goes before or after showing, as
    /// [`wpe::backdrop_timing`] decides.
    /// Load supervision is started by the caller (`spawn_load_monitor`).
    fn apply_video(&mut self, path: &str, has_audio: bool) -> WallpaperResult {
        let Some(host) = self.sys.wallpaper_host() else {
            log_line("error", &wpe::video_host_missing_message());
            return WallpaperResult::fail(wpe::video_host_missing_message());
        };
        let exists = self.sys.exists(path) && !self.sys.is_dir(path);
        let size = self.sys.file_size(path).unwrap_or(0);
        let (ok, message) = wpe::validate_video(path, exists, size, wpe::MAX_VIDEO_FILE_MB);
        if !ok {
            return WallpaperResult::fail(message);
        }
        let original_name = path.rsplit('/').next().unwrap_or(path).to_string();
        // The backdrop frame comes from the person's file: the same cache key as the
        // frame shown instead of a crashing wallpaper (`still_frame_for`), and ffmpeg
        // does not need `moov` moved.
        let still_source = path.to_string();
        let path = self.sys.ensure_faststart(path);

        // KDE: remember the real desktop picture BEFORE the backdrop, so that a
        // stop restores it rather than the blurred frame.
        if wpe::uses_plasma_config(&self.de) && self.saved_wallpaper.is_none() {
            if let Some(image) = self.kde_current_static_image() {
                let exists = self.sys.exists(&image);
                if wpe::clean_image_candidate_ok(&image, exists) {
                    self.saved_wallpaper = Some(image);
                    self.saved_de = Some(self.de.clone());
                }
            }
        }

        let timing = self.backdrop_before_show(&still_source);

        let res = if wpe::needs_shell_extension(&self.de, &self.session_type) {
            self.video_shell_extension(&path, &host, has_audio)
        } else {
            let plan = wpe::video_native_plan(&host, &path, has_audio);
            self.run_plan(&plan, &path)
        };
        // What plays is the internal copy with the moved header (its name is a
        // fingerprint like `63f038e7….mp4`); to the person we name their own file.
        let mut res = res;
        let played_name = path.rsplit('/').next().unwrap_or(&path).to_string();
        if played_name != original_name {
            res.message = res.message.replace(&played_name, &original_name);
        }
        if res.ok {
            self.current_format = Some("video".to_string());
            self.current_path = Some(path);
            self.queue_backdrop_after_show(timing, &still_source);
        }
        res
    }

    /// YouTube wallpaper — port of `_apply_youtube`: quality, mode (stream or
    /// download) and sound are chosen by the person.
    ///
    /// The "stream" mode, where it cannot be carried out (GNOME, Cinnamon-Wayland —
    /// the wallpaper is shown by a shell extension, which needs a file), becomes
    /// a download, and the person is told why. A stream is played by mpv/mpvpaper
    /// through the engine plan; a downloaded video is set the same way as video
    /// from a file. Load supervision is started by the caller.
    pub fn apply_youtube(
        &mut self,
        url: &str,
        quality: &str,
        mode: &str,
        audio: bool,
    ) -> WallpaperResult {
        self.we_note = None;
        self.forget_pending_backdrop();
        let mut res = self.apply_youtube_inner(url, quality, mode, audio);
        self.attach_we_note(&mut res);
        res
    }

    fn apply_youtube_inner(
        &mut self,
        url: &str,
        quality: &str,
        mode: &str,
        audio: bool,
    ) -> WallpaperResult {
        let url = url.trim();
        if let Some(refusal) = self.shell_owner_refusal() {
            return refusal;
        }
        if !self.sys.has_tool("yt-dlp") {
            let hint = self.sys.package_hint("yt-dlp");
            return WallpaperResult::fail(wpe::youtube_no_ytdlp_message(&hint));
        }
        if !crate::youtube_wallpaper::is_supported_url(url) {
            return WallpaperResult::fail(wpe::youtube_unsupported_message(url));
        }
        self.yield_we_wallpapers("youtube");
        let quality = if quality.trim().is_empty() { "1080p" } else { quality };
        let effective = wpe::youtube_effective_mode(&self.de, &self.session_type, mode);
        let note = wpe::youtube_mode_downgraded_note(&self.de, &self.session_type, mode);
        if !note.is_empty() {
            log_line("info", &format!("YouTube: {note}"));
        }
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        // All environments: hide desktop icons (a stop restores them).
        self.set_desktop_icons(false);

        if effective == "download" {
            log_line("info", &format!("YouTube: скачиваю, качество {quality}"));
            let Some(local) = self.sys.download_youtube(url, quality) else {
                return WallpaperResult::fail(wpe::youtube_download_failed_message(
                    &self.de,
                    &self.session_type,
                ));
            };
            let mut res = self.apply_video(&local, audio);
            if res.ok {
                self.write_pidfile();
                // A stream was requested, but a download was obtained — say why.
                if !note.is_empty() {
                    res.message = format!("{} {note}", res.message);
                }
            }
            return res;
        }

        log_line("info", &format!("YouTube: поток, качество {quality}"));
        let tools = self.plan_tools();
        let hints = self.hints();
        let plan = wpe::youtube_plan(
            &self.de,
            &self.session_type,
            url,
            audio,
            YOUTUBE_STREAM_VOLUME,
            quality,
            &tools,
            "{monitor}",
            wpe::GEOMETRY_MARK,
            &hints,
        );
        let res = self.run_plan(&plan, url);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// The backdrop under live wallpapers: a sharp movie frame as the desktop
    /// picture — visible during a load pause, on the lock screen and in the
    /// overview, and when showing fails. If it did not work, no harm: the live
    /// wallpaper does not depend on it.
    ///
    /// This is the part BEFORE showing: where the backdrop goes before it
    /// ([`wpe::BackdropTiming::BeforeShow`]), it is set right away; where after
    /// — it is deferred by [`Self::queue_backdrop_after_show`].
    fn backdrop_before_show(&mut self, source: &str) -> wpe::BackdropTiming {
        let timing = wpe::backdrop_timing(&self.de, &self.session_type);
        match timing {
            wpe::BackdropTiming::BeforeShow => {
                let started = self.sys.monotonic();
                let cache = self.cache_home();
                let session = self.session_type.clone();
                let still = prepare_video_still(&mut self.sys, &cache, &session, source);
                match still {
                    Some(still) if self.set_backdrop_image(&still) => {
                        let ms = ((self.sys.monotonic() - started).max(0.0) * 1000.0) as u64;
                        log_line("info", &format!("подложка поставлена до показа за {ms} мс: картинку стола здесь держит своя поверхность слоя фона"));
                    }
                    Some(_) => log_line("warning", "подложка не встала — живые обои пойдут без неё"),
                    None => log_line("warning", "подложка не вышла: кадра из ролика нет — живые обои пойдут без неё"),
                }
            }
            wpe::BackdropTiming::AfterShow => {}
        }
        timing
    }

    /// Showing succeeded: defer the backdrop until after it. It is prepared and
    /// set by the background door ([`spawn_backdrop`]); whoever calls the
    /// executor bypassing it uses [`Self::run_pending_backdrop`].
    fn queue_backdrop_after_show(&mut self, timing: wpe::BackdropTiming, source: &str) {
        if timing != wpe::BackdropTiming::AfterShow {
            return;
        }
        let cache_home = self.cache_home();
        self.pending_backdrop = Some(BackdropJob {
            source: source.to_string(),
            cache_home,
            session_type: self.session_type.clone(),
            generation: self.backdrop_generation,
            shown: (self.current_format.clone(), self.current_path.clone()),
            shown_at: self.sys.monotonic(),
        });
    }

    /// Set the backdrop picture with the desktop's static plan — quietly: the
    /// shown format and path do not change, no notice goes to the bridge.
    fn set_backdrop_image(&mut self, still: &str) -> bool {
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::static_plan(&self.de, &self.static_session(), still, &tools, &hint);
        self.quiet_static = true;
        let res = self.run_plan(&plan, still);
        self.quiet_static = false;
        if res.ok {
            self.blur_path = Some(still.to_string());
        } else {
            log_line("warning", &format!("подложка не встала: {}", res.message));
        }
        res.ok
    }

    /// A new apply or a stop: the previous wallpaper's deferred backdrop, and the
    /// one already being prepared in the background, are out of place.
    fn forget_pending_backdrop(&mut self) {
        self.pending_backdrop = None;
        self.backdrop_generation = self.backdrop_generation.wrapping_add(1);
    }

    /// The deferred backdrop, if there is one — take it for execution.
    pub fn take_pending_backdrop(&mut self) -> Option<BackdropJob> {
        self.pending_backdrop.take()
    }

    /// Set the deferred backdrop whose frame has been prepared
    /// ([`prepare_video_still`]; `None` — no frame). True — the backdrop is set.
    /// If the wallpaper has since changed or been stopped, it does nothing: the
    /// backdrop is out of place, and the previous desktop picture was already restored.
    pub fn finish_backdrop(&mut self, job: &BackdropJob, still: Option<String>) -> bool {
        let shown = (self.current_format.clone(), self.current_path.clone());
        if job.generation != self.backdrop_generation || shown != job.shown {
            log_line("info", "подложку не ставлю: обои за это время сменились");
            return false;
        }
        let Some(still) = still else {
            log_line("warning", "подложка не вышла: кадра из ролика нет — живые обои идут без неё");
            return false;
        };
        if self.blur_path.as_deref() == Some(still.as_str()) {
            log_line("info", "подложка уже стоит — ставить заново незачем");
            return true;
        }
        if !self.set_backdrop_image(&still) {
            return false;
        }
        let ms = ((self.sys.monotonic() - job.shown_at).max(0.0) * 1000.0) as u64;
        log_line("info", &wpe::backdrop_ready_log_line(ms));
        true
    }

    /// The deferred backdrop — right now, with this same seam (for callers
    /// bypassing [`run_in_slot`]: checks, one-off applies without a window).
    pub fn run_pending_backdrop(&mut self) -> bool {
        let Some(job) = self.take_pending_backdrop() else {
            return false;
        };
        let still = prepare_video_still(&mut self.sys, &job.cache_home, &job.session_type, &job.source);
        self.finish_backdrop(&job, still)
    }

    /// Hand the video to the shell extension through its own settings — port of
    /// `_video_shell_extension`. There is no process of our own here: the
    /// extension itself launches the host from the ready command line.
    fn video_shell_extension(&mut self, path: &str, host: &str, has_audio: bool) -> WallpaperResult {
        let plan = wpe::shell_ext_plan(&self.de, path, host, has_audio);
        self.run_shell_extension(plan, path, "video")
    }

    /// Hand a wallpaper to the shell extension — ONE path for video and Flash:
    /// schema check, `engine`, the daemon command line, a notice to the bridge.
    fn run_shell_extension(
        &mut self,
        plan: Option<wpe::ShellExtPlan>,
        path: &str,
        fmt: &str,
    ) -> WallpaperResult {
        let Some(plan) = plan else {
            return WallpaperResult::fail(crate::i18n::tr(
                "This desktop environment does not need the shell extension",
            ));
        };
        let ran_ok = |sys: &mut S, argv: &[String]| {
            let timeout = utils::default_timeout_secs(argv) as f64;
            matches!(sys.run(argv, timeout), Some(ref out) if out.rc == 0)
        };
        // Write through the schema from the extension's own directory, if it
        // exists: it may be missing from the shared schema directory (see
        // `wpe::shell_ext_schema_dirs`).
        let schema_dir = self.shell_extension_schema_dir();
        let gsettings = |cmd: &[String]| match &schema_dir {
            Some(dir) => wpe::with_schemadir(cmd, dir),
            None => cmd.to_vec(),
        };
        // Only an installed extension has the schema — this is the check too.
        if !ran_ok(&mut self.sys, &gsettings(&plan.source_argv)) {
            return WallpaperResult::fail(plan.not_installed_message);
        }
        ran_ok(&mut self.sys, &gsettings(&plan.engine_argv));
        // The result of the command line write MUST be checked: an outdated extension
        // has no such key, and it no longer has a fallback path.
        if !ran_ok(&mut self.sys, &gsettings(&plan.daemon_argv)) {
            log_line("error", "расширение оболочки не приняло строку запуска хоста");
            return WallpaperResult::fail(plan.daemon_failed_message);
        }
        self.current_format = Some(fmt.to_string());
        self.current_path = Some(path.to_string());
        // No process of our own and no plan was executed — tell the bridge ourselves.
        self.notify_live_wallpaper(Some(fmt), true);
        let mut state = self.shell_extension_state();
        if state == wpe::ShellExtState::Disabled {
            state = self.enable_shell_extension();
        }
        match wpe::shell_ext_state_message(state, &plan.disabled_message) {
            None => WallpaperResult::ok(plan.ok_message),
            Some((ok, message)) => {
                log_line("warning", &format!("расширение оболочки не работает: {state:?}"));
                if ok {
                    WallpaperResult::ok(message)
                } else {
                    WallpaperResult::fail(message)
                }
            }
        }
    }

    /// Flash wallpaper (SWF) — the `rexon-wallpaper-host --mode swf` mode: Ruffle
    /// as a library (the `rexon-swf` crate), without an external `ruffle`,
    /// downloads or the classic window's HTML wrapper.
    ///
    /// Everything is checked BEFORE stopping the previous wallpaper: the host
    /// (none — nothing to show with) and the file (missing or a directory — a
    /// refusal with the name). Then as for video: a layer window or X11 in its
    /// own session, on GNOME and Cinnamon-Wayland — the shell extension, to which
    /// the host writes frames into a file. There is no blurred backdrop: no
    /// frame exists before Ruffle starts. Load supervision is started by the
    /// caller ([`format_wants_load_monitor`]).
    fn apply_swf(&mut self, path: &str) -> WallpaperResult {
        let Some(host) = self.sys.wallpaper_host() else {
            log_line("error", &wpe::video_host_missing_message());
            return WallpaperResult::fail(wpe::video_host_missing_message());
        };
        if !self.sys.exists(path) || self.sys.is_dir(path) {
            return WallpaperResult::fail(wpe::swf_missing_message(path));
        }
        // The previous live wallpaper — without a notice to the bridge: it is about to be replaced.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        log_line(
            "info",
            &format!(
                "применяю Flash-обои: окружение={}, сессия={}, путь={path}",
                self.de, self.session_type
            ),
        );
        let res = if wpe::needs_shell_extension(&self.de, &self.session_type) {
            let plan = wpe::swf_shell_ext_plan(&self.de, path, &host);
            self.run_shell_extension(plan, path, "swf")
        } else {
            let plan = wpe::swf_host_plan(&host, path);
            self.run_plan(&plan, path)
        };
        if res.ok {
            self.current_format = Some("swf".to_string());
            self.current_path = Some(path.to_string());
            self.write_pidfile();
        }
        res
    }

    /// Directory of our extension's compiled schema — the first of
    /// [`wpe::shell_ext_schema_dirs`] that holds `gschemas.compiled`.
    /// `None` — none: write with bare `gsettings` (the schema may be in the
    /// shared directory where the package puts it).
    fn shell_extension_schema_dir(&mut self) -> Option<String> {
        let home = self.sys.env("HOME");
        let data_home = self.sys.env("XDG_DATA_HOME");
        let data_dirs = self.sys.env("XDG_DATA_DIRS");
        wpe::shell_ext_schema_dirs(&self.de, &home, &data_home, &data_dirs)
            .into_iter()
            .find(|dir| self.sys.exists(&format!("{dir}/gschemas.compiled")))
    }

    /// What state our shell extension is in. If asking failed, we say exactly
    /// that ([`wpe::ShellExtState::Unknown`]): this used to say "nothing to check
    /// with — assume enabled", and the person got «done» with an empty desktop.
    /// Enable our own disabled extension and ask for its state again.
    /// The wallpaper is already written into its settings: once enabled, it
    /// reads them and starts the host by itself.
    fn enable_shell_extension(&mut self) -> wpe::ShellExtState {
        let probe = wpe::shell_ext_state_argv(&self.de);
        let timeout = utils::default_timeout_secs(&probe) as f64;
        let list = self.sys.run(&probe, timeout).filter(|o| o.rc == 0).map(|o| o.stdout);
        let Some(enable) = wpe::shell_ext_enable_argv(&self.de, list.as_deref()) else {
            return self.shell_extension_state();
        };
        let timeout = utils::default_timeout_secs(&enable) as f64;
        let ok = matches!(self.sys.run(&enable, timeout), Some(ref o) if o.rc == 0);
        log_line(
            if ok { "info" } else { "warning" },
            &format!("расширение оболочки было выключено — включаю: {}", if ok { "готово" } else { "не вышло" }),
        );
        if !ok {
            return wpe::ShellExtState::Disabled;
        }
        // The shell does not pick up the list instantly.
        self.sys.sleep(SHELL_EXT_ENABLE_SETTLE_SEC);
        self.shell_extension_state()
    }

    fn shell_extension_state(&mut self) -> wpe::ShellExtState {
        let argv = wpe::shell_ext_state_argv(&self.de);
        let timeout = utils::default_timeout_secs(&argv) as f64;
        let out = self.sys.run(&argv, timeout).filter(|o| o.rc == 0);
        wpe::parse_shell_ext_state(&self.de, out.as_ref().map(|o| o.stdout.as_str()))
    }

    /// Web wallpaper: an address (`http(s)://`, `file://`) or a local HTML file —
    /// port of `WallpaperApplier._apply_web` together with the web branch of `apply`.
    ///
    /// The `rexon-wallpaper-host --mode web` host is started in its own session
    /// (the engine's `web_plan`); without a host the plan itself answers with an
    /// honest refusal before launching.
    fn apply_web(&mut self, path: &str) -> WallpaperResult {
        // ⭐ A fix: the window turns a local path into `file://…`
        // (`qt_settings::web_wallpaper_url`), while Python's existence check looked
        // at that very string — there is no such file, and a local HTML file always
        // got «not found». The path without the scheme is checked.
        let local = path.strip_prefix("file://").unwrap_or(path);
        if !wpe::is_url(path) && (!self.sys.exists(local) || self.sys.is_dir(local)) {
            return WallpaperResult::fail(wpe::web_missing_message(local));
        }
        // The previous live wallpaper is stopped without a notice to the bridge: it is about to be replaced.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        log_line(
            "info",
            &format!("применяю веб-обои: сессия={}, адрес={path}", self.session_type),
        );
        let tools = self.plan_tools();
        let hints = self.hints();
        let host = self.sys.wallpaper_host();
        // The absolute path is needed only by the surf fallback for a local file;
        // the window passes a ready address here, the path goes as is.
        let plan = wpe::web_plan(
            &self.session_type,
            "",
            path,
            path,
            &tools,
            wpe::GEOMETRY_MARK,
            &hints,
            host.as_deref(),
        );
        let res = self.run_plan(&plan, path);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// Stop the live wallpaper and return the desktop to its original state.
    ///
    /// `announce` — whether to tell the bridge about the stop. False means "the
    /// wallpaper is being stopped for a replacement": the bridge's flag gets
    /// updated, there will be no notice.
    pub fn stop(&mut self, announce: bool) -> WallpaperResult {
        // The format is cleared FIRST, before any restore: the restore sets a static
        // picture with the same plan as a normal apply, and that one writes its own
        // format into the state — and the message assembled at the end would say
        // `static` even after a video.
        let fmt = self.current_format.clone();
        // The watcher does not bring back what was stopped on purpose.
        self.host_request = None;
        self.forget_pending_backdrop();

        // A process frozen by supervision is unfrozen first: SIGTERM does not reach
        // a stopped process, and the stop would wait for SIGKILL.
        self.resume_if_paused();
        if let Some(proc) = self.process.take() {
            self.sys.kill_process(proc);
        }
        // The wallpaper on the other monitors is part of the same wallpaper.
        self.kill_companions();

        // Background sounds are part of the same wallpaper: leaving them playing on
        // a bare desktop would mean stopping the wallpaper halfway.
        self.stop_all_ambient();

        // All environments: bring back the desktop icons.
        self.set_desktop_icons(true);
        // X11: return the desktop to how it was before the live wallpaper (Caja's
        // transparent background, the Marco compositor). Only on the person's
        // «Stop»: a stop before a wallpaper change (`announce == false`) would
        // switch Caja's desktop window back and forth on every change.
        if announce {
            crate::wallpaper_backend::restore_desktop_icons(&mut DesktopIconsSys::new(&mut self.sys));
        }

        // KDE: take Rexon's REMOVED wallpaper plugin off the desktop containments
        // and restore the stock one. For someone who upgraded from an old version it
        // may have stayed selected — then Plasma loads a non-existent plugin and the
        // desktop stays empty.
        if wpe::uses_plasma_config(&self.de) && self.kde_stale_rexon_plugin() {
            let clean = self.kde_clean_static_image();
            let script = wpe::kde_reset_script(&clean);
            let argv = wpe::plasma_dbus_argv(&script);
            self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC);
        }

        // GNOME/Cinnamon-Wayland: the daemon is owned by the shell extension, not
        // by us — it can only be stopped through its own settings, otherwise the
        // wallpaper would keep playing after «Stop».
        if wpe::needs_shell_extension(&self.de, &self.session_type) {
            self.shell_extension_stop();
        }

        self.restore_wallpaper();
        // …and if another wallpaper engine owned the desktop before us, give the
        // desktop back to it. AFTER restoring the picture: on KDE that one
        // unconditionally sets org.kde.image, and the reverse order would wipe out
        // the plugin restore.
        self.restore_kde_wallpaper_plugin();
        self.blur_path = None;

        self.current_format = None;
        self.current_path = None;
        if announce {
            // For a replacement the name already belongs to the NEW wallpaper: the apply
            // sets it before stopping the previous one. Erasing it here would mean
            // losing it by the time of the launch notice.
            self.current_name.clear();
        }
        self.remove_pidfile();

        // Bridge: there is no live wallpaper any more.
        self.notify_live_wallpaper(None, announce);

        WallpaperResult::ok(wpe::stopped_message(fmt.as_deref()))
    }

    /// Snapshot of the engine state — what the tab's buttons live on.
    pub fn status(&mut self) -> Status {
        let running = match self.process {
            Some(p) => self.sys.poll(p).is_none(),
            None => false,
        };
        let pid = if running {
            self.process.and_then(|p| self.sys.pid(p))
        } else {
            None
        };
        let ambient = self.active_ambient();
        Status {
            running,
            format: self.current_format.clone(),
            path: self.current_path.clone(),
            de: self.de.clone(),
            session_type: self.session_type.clone(),
            pid,
            blur_active: self.blur_path.is_some(),
            ambient,
        }
    }

    // ── background sounds ─────────────────────────────────────────────────

    /// Start a background sound. True only if it has STARTED SOUNDING.
    ///
    /// ⚠️ The success yardstick here is NOT the same as for wallpapers, and that
    /// is fundamental. A wallpaper process draws the desktop while alive — there
    /// the yardstick is "survived the grace period". Sound cannot be measured
    /// that way: a one-shot sound shorter than the grace period honestly ends by
    /// itself, and the "alive after half a second" rule would give a FALSE
    /// refusal on a working sound. So the yardstick is matching the expectation:
    ///
    ///   * a looped sound must keep playing — success only if "alive";
    ///   * a one-shot sound must play OR honestly finish — success if "alive or
    ///     exited with zero".
    ///
    /// The grace period itself is the same and comes from the engine: a broken
    /// file gives a refusal within its tenth of a second, only one that plays
    /// pays the full grace period.
    pub fn play_ambient(
        &mut self,
        id: &str,
        path: &str,
        name: &str,
        volume: i64,
        looped: bool,
    ) -> bool {
        if !self.sys.exists(path) || self.sys.is_dir(path) {
            log_line("error", &format!("звуковой файл не найден: {path}"));
            return false;
        }
        if !self.sys.has_tool("mpv") {
            log_line("error", "mpv не установлен — фоновые звуки недоступны");
            return false;
        }
        // Stop the previous instance of the same sound: otherwise two mpv processes
        // would play the same thing on top of each other.
        self.stop_ambient(id);

        let argv = wpe::ambient_argv(path, volume, looped);
        let proc = match self.sys.spawn(&argv, false) {
            Ok(p) => p,
            Err(e) => {
                log_line("error", &format!("фоновый звук не запустился: {path}: {e}"));
                return false;
            }
        };
        let rc = self.wait_alive(proc);
        if let Some(rc) = rc {
            if looped || rc != 0 {
                log_line(
                    "error",
                    &format!("фоновый звук не пошёл: {path} (зацикленный={looped}, код {rc})"),
                );
                self.sys.kill_process(proc);
                return false;
            }
        }
        let shown = if name.is_empty() {
            path.rsplit('/').next().unwrap_or(path).to_string()
        } else {
            name.to_string()
        };
        if rc.is_none() {
            log_line("info", &format!("фоновый звук запущен: {shown}"));
        } else {
            log_line("info", &format!("фоновый звук доиграл до конца: {shown}"));
        }
        self.ambient.push(Ambient {
            id: id.to_string(),
            path: path.to_string(),
            name: shown,
            volume,
            // The mode is needed for a volume change: mpv changes it only by
            // restarting, and it must be restarted in THE SAME mode — otherwise a
            // one-shot sound looped forever after a volume tweak.
            looped,
            proc,
        });
        true
    }

    /// Stop one background sound.
    pub fn stop_ambient(&mut self, id: &str) {
        let Some(idx) = self.ambient.iter().position(|a| a.id == id) else {
            return;
        };
        let sound = self.ambient.remove(idx);
        self.sys.kill_process(sound.proc);
        log_line("info", &format!("фоновый звук остановлен: {}", sound.name));
    }

    /// Stop all background sounds.
    pub fn stop_all_ambient(&mut self) {
        let ids: Vec<String> = self.ambient.iter().map(|a| a.id.clone()).collect();
        for id in ids {
            self.stop_ambient(&id);
        }
    }

    /// Change the sound volume — by restarting in the same mode.
    pub fn set_ambient_volume(&mut self, id: &str, volume: i64) -> bool {
        let Some(sound) = self.ambient.iter().find(|a| a.id == id) else {
            return false;
        };
        let (path, name, looped) = (sound.path.clone(), sound.name.clone(), sound.looped);
        self.stop_ambient(id);
        self.play_ambient(id, &path, &name, volume, looped)
    }

    /// Whether the sound is playing right now.
    pub fn ambient_playing(&mut self, id: &str) -> bool {
        let Some(proc) = self.ambient.iter().find(|a| a.id == id).map(|a| a.proc) else {
            return false;
        };
        self.sys.poll(proc).is_none()
    }

    /// List of playing sounds; finished ones are cleaned out of the state.
    pub fn active_ambient(&mut self) -> Vec<AmbientInfo> {
        let all: Vec<(String, String, i64, ProcHandle)> = self
            .ambient
            .iter()
            .map(|a| (a.id.clone(), a.name.clone(), a.volume, a.proc))
            .collect();
        let mut alive = Vec::new();
        let mut out = Vec::new();
        for (id, name, volume, proc) in all {
            if self.sys.poll(proc).is_some() {
                continue; // finished — it has no place in the list
            }
            alive.push(id.clone());
            out.push(AmbientInfo { id, name, volume, active: true });
        }
        self.ambient.retain(|a| alive.contains(&a.id));
        out
    }

    // ── what to restore after a stop ──────────────────────────────────────

    /// Remember the wallpaper that is set now — a stop restores it.
    fn save_current_wallpaper(&mut self) {
        // The KDE desktop plugin is remembered SEPARATELY from the picture: with a
        // foreign plugin there is no static picture in the config at all, and the
        // early return below would lose the desktop's owner.
        self.save_kde_wallpaper_plugin();

        if self.saved_wallpaper.is_some() {
            return; // already saved
        }
        self.saved_de = Some(self.de.clone());
        if self.de == "cosmic" {
            // The same key the COSMIC branch writes the picture into.
            let all = format!("{}/{}/all", self.config_home(), wpe::COSMIC_BG_DIR_REL);
            self.saved_wallpaper = self.sys.read_text(&all).and_then(|t| wpe::cosmic_bg_source_path(&t));
            return;
        }
        let Some((schema, key)) = wpe::gsettings_schema(&self.de) else {
            return;
        };
        let argv = wpe::gsettings_get_argv(schema, key);
        let timeout = utils::default_timeout_secs(&argv) as f64;
        if let Some(out) = self.sys.run(&argv, timeout) {
            let raw = out.stdout.trim().trim_matches(['\'', '"']).to_string();
            if !raw.is_empty() {
                self.saved_wallpaper = Some(raw.replace("file://", ""));
            }
        }
        if schema == wpe::CINNAMON_BG_SCHEMA {
            self.save_cinnamon_bg_list();
        }
    }

    /// Cinnamon 6.8+: remember the picture list and the mode as they are; take
    /// the "what was set" picture from the list if `picture-uri` is empty (on 6.8
    /// it is always empty — the shell clears it).
    fn save_cinnamon_bg_list(&mut self) {
        let Some(list) = self.cinnamon_bg_get(wpe::CINNAMON_BG_LIST_KEY) else {
            return; // old Cinnamon: no such key
        };
        let mode = self.cinnamon_bg_get(wpe::CINNAMON_BG_MODE_KEY).unwrap_or_else(|| "'mirror'".to_string());
        if self.saved_wallpaper.is_none() {
            self.saved_wallpaper = wpe::cinnamon_list_first_path(&list);
        }
        self.saved_cinnamon_bg = Some((list, mode));
    }

    /// Value of a Cinnamon background schema key (GVariant text); `None` — no
    /// such key or gsettings did not answer.
    fn cinnamon_bg_get(&mut self, key: &str) -> Option<String> {
        let argv = wpe::gsettings_get_argv(wpe::CINNAMON_BG_SCHEMA, key);
        let timeout = utils::default_timeout_secs(&argv) as f64;
        let out = self.sys.run(&argv, timeout).filter(|o| o.rc == 0)?;
        let raw = out.stdout.trim().to_string();
        (!raw.is_empty()).then_some(raw)
    }

    /// Cinnamon 6.8+: restore the remembered list and mode. True — restored.
    fn restore_cinnamon_bg_list(&mut self) -> bool {
        let Some((list, mode)) = self.saved_cinnamon_bg.take() else {
            return false;
        };
        let set = |key: &str, value: &str| {
            vec![
                "gsettings".to_string(),
                "set".to_string(),
                wpe::CINNAMON_BG_SCHEMA.to_string(),
                key.to_string(),
                value.to_string(),
            ]
        };
        let argv = set(wpe::CINNAMON_BG_LIST_KEY, &list);
        let timeout = utils::default_timeout_secs(&argv) as f64;
        let ok = matches!(self.sys.run(&argv, timeout), Some(ref o) if o.rc == 0);
        if ok {
            self.sys.run(&set(wpe::CINNAMON_BG_MODE_KEY, &mode), timeout);
            log_line("info", "фон Cinnamon: возвращён прежний список картинок");
        }
        ok
    }

    /// Restore the saved wallpaper.
    fn restore_wallpaper(&mut self) {
        // Cinnamon 6.8+: the whole list — more precise than a single picture.
        if self.restore_cinnamon_bg_list() {
            self.saved_wallpaper = None;
            self.saved_de = None;
            return;
        }
        let Some(path) = self.saved_wallpaper.take() else {
            return;
        };
        let de = self.saved_de.take().unwrap_or_else(|| self.de.clone());

        // KDE: reset the desktop plugin to the stock one (if ours was set).
        if wpe::uses_plasma_config(&de) {
            let argv = wpe::plasma_dbus_argv(wpe::KDE_PLAIN_RESET_SCRIPT);
            self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC);
        }

        if !self.sys.exists(&path) {
            return;
        }
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::restore_static_plan(&de, &path, &tools, &hint);
        if !plan.is_empty() {
            self.run_plan(&plan, &path);
        }
    }

    /// plasmashell's answer: which wallpaper plugins are set on ALL desktops.
    ///
    /// The only place where the executor asks about the desktop plugin: both
    /// "whose desktop is it, to whom to give it back" and "is the removed plugin
    /// still there" — the engine computes both answers from THIS string. We read
    /// it from Plasma itself, not from the config: the file on disk lags behind,
    /// plasmashell keeps settings in memory and writes them whenever it wants.
    /// Silence is returned as an empty string — nobody passes it off as "no plugins".
    fn kde_plugin_reply(&mut self) -> String {
        let argv = wpe::plasma_dbus_reply_argv(wpe::KDE_READ_PLUGIN_SCRIPT);
        match self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
            Some(out) if out.rc == 0 => out.stdout,
            _ => String::new(),
        }
    }

    /// Remember a FOREIGN desktop wallpaper plugin, if one was set there.
    fn save_kde_wallpaper_plugin(&mut self) {
        if !wpe::uses_plasma_config(&self.de) || self.saved_kde_plugin.is_some() {
            return;
        }
        let reply = self.kde_plugin_reply();
        if let Some(plugin) = wpe::kde_foreign_wallpaper_plugin(&reply) {
            log_line(
                "info",
                &format!("стол KDE занят плагином {plugin} — вернём его при остановке"),
            );
            self.saved_kde_plugin = Some(plugin);
        }
    }

    /// Give the KDE desktop back its previous foreign wallpaper plugin.
    fn restore_kde_wallpaper_plugin(&mut self) {
        let Some(plugin) = self.saved_kde_plugin.take() else {
            return;
        };
        let script = wpe::kde_restore_plugin_script(&plugin);
        let argv = wpe::plasma_dbus_argv(&script);
        match self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
            Some(out) if out.rc == 0 => {
                log_line("info", &format!("столу KDE возвращён плагин обоев {plugin}"));
            }
            other => {
                let why = other
                    .map(|o| o.stderr)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "нет ответа plasmashell".to_string());
                log_line(
                    "warning",
                    &format!("не удалось вернуть плагин обоев {plugin}: {why}"),
                );
            }
        }
    }

    /// Whether Rexon's REMOVED wallpaper plugin is still selected on the desktop.
    fn kde_stale_rexon_plugin(&mut self) -> bool {
        let reply = self.kde_plugin_reply();
        wpe::kde_removed_plugin_selected(&reply)
    }

    /// The current `org.kde.image` picture of the desktop.
    ///
    /// Read from the desktops config rather than via `kreadconfig6`: that one
    /// would need the containment number, and it differs on every machine — a
    /// query with someone else's number returned emptiness, and the stop
    /// restored not the person's wallpaper but the distribution default.
    fn kde_current_static_image(&mut self) -> Option<String> {
        let home = self.home();
        let cfg = format!("{home}/.config/plasma-org.kde.plasma.desktop-appletsrc");
        let text = self.sys.read_text(&cfg)?;
        wpe::kde_desktop_static_image(&text)
    }

    /// A clean wallpaper to restore after a stop: the saved real one → the
    /// current one (if it is not our frame from the cache) → the distribution default.
    fn kde_clean_static_image(&mut self) -> String {
        let saved = self.saved_wallpaper.clone();
        let current = self.kde_current_static_image();
        for cand in [saved, current].into_iter().flatten() {
            if cand.is_empty() {
                continue;
            }
            let exists = self.sys.exists(&cand.replace("file://", ""));
            if wpe::clean_image_candidate_ok(&cand, exists) {
                return cand;
            }
        }
        for dir in wpe::DISTRO_WALLPAPER_DIRS {
            if self.sys.is_dir(dir) {
                return format!("{dir}/");
            }
        }
        // Any installed Plasma wallpaper package. Sorted order — otherwise "the
        // first one found" would change from machine to machine.
        for name in self.sys.listdir(wpe::PLASMA_WALLPAPER_ROOT) {
            let pkg = format!("{}/{name}", wpe::PLASMA_WALLPAPER_ROOT);
            if self.sys.is_dir(&format!("{pkg}/contents/images")) {
                return format!("{pkg}/");
            }
        }
        String::new()
    }

    // ── desktop icons and the PID file ────────────────────────────────────

    /// Show or hide desktop icons with the engine's commands.
    fn set_desktop_icons(&mut self, visible: bool) {
        if let Some(probe) = wpe::desktop_icons_probe_argv(&self.de, visible) {
            let answered_yes = match self.sys.run(&probe, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
                Some(out) => out.stdout.trim().eq_ignore_ascii_case("true"),
                None => false,
            };
            if !answered_yes {
                return;
            }
        }
        for argv in wpe::desktop_icons_argvs(&self.de, visible) {
            self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC);
        }
    }


    // ── wallpaper blur ────────────────────────────────────────────────────

    /// Blur the current picture wallpaper and set the blurred copy.
    ///
    /// True — the copy is on the desktop. The real wallpaper is remembered the
    /// same way as before a live wallpaper — [`Self::unblur_wallpaper`] or a stop
    /// restores it.
    ///
    /// ⭐ A fix: Python learned the current wallpaper only via gsettings, and on
    /// KDE «Blur» always answered with a refusal. Here the KDE desktop picture is
    /// read from the desktops config (as when restoring after a stop); our frame
    /// from the cache is not taken for the real wallpaper.
    /// The picture currently set as the desktop wallpaper — read-only (a Conky
    /// widget set remembers it next to the widgets). Rexon's own picture
    /// wallpaper — from the executor's memory; otherwise from the environment's
    /// settings: the KDE desktops config, the COSMIC key, gsettings. `None` —
    /// could not find out, or what is set is not a picture.
    pub fn desktop_static_image(&mut self) -> Option<String> {
        if self.current_format.as_deref() == Some("static") {
            return self.current_path.clone();
        }
        if self.current_format.is_some() {
            return None; // a live wallpaper is playing — there is no picture
        }
        let found = if wpe::uses_plasma_config(&self.de) {
            self.kde_current_static_image()
        } else if self.de == "cosmic" {
            let all = format!("{}/{}/all", self.config_home(), wpe::COSMIC_BG_DIR_REL);
            self.sys.read_text(&all).and_then(|t| wpe::cosmic_bg_source_path(&t))
        } else if self.de == "xfce" {
            // xfdesktop keeps a picture for each monitor and workspace; the main
            // desktop is the first monitor, workspace 0.
            let list: Vec<String> = ["xfconf-query", "-c", "xfce4-desktop", "-l"].map(String::from).to_vec();
            let timeout = utils::default_timeout_secs(&list) as f64;
            let prop = wpe::xfce_last_image_prop(&self.sys.run(&list, timeout)?.stdout)?;
            let out = self.sys.run(&wpe::xfconf_get_argv(&prop), timeout)?;
            let raw = out.stdout.trim().to_string();
            (!raw.is_empty()).then_some(raw)
        } else {
            let (schema, key) = wpe::gsettings_schema(&self.de)?;
            let mut get = |schema: &str, key: &str| -> Option<String> {
                let argv = wpe::gsettings_get_argv(schema, key);
                let timeout = utils::default_timeout_secs(&argv) as f64;
                let raw = wpe::gsettings_path_value(&self.sys.run(&argv, timeout)?.stdout);
                (!raw.is_empty()).then_some(raw)
            };
            // With a dark style GNOME shows `picture-uri-dark`.
            let dark = wpe::gsettings_dark_key(&self.de).and_then(|(s, k)| {
                let (cs, ck) = crate::themes::COLOR_SCHEME_KEY;
                get(cs, ck).filter(|v| wpe::prefers_dark(v)).and_then(|_| get(s, k))
            });
            let first = dark.or_else(|| get(schema, key));
            let list = (first.is_none() && schema == wpe::CINNAMON_BG_SCHEMA)
                .then(|| self.cinnamon_bg_get(wpe::CINNAMON_BG_LIST_KEY))
                .flatten()
                .and_then(|l| wpe::cinnamon_list_first_path(&l));
            first.or(list)
        };
        found.filter(|p| self.sys.exists(p) && !self.sys.is_dir(p))
    }

    /// The picture to show a widget snapshot on (the Conky preview backdrop,
    /// `conky_preview`): the desktop picture wallpaper ([`Self::desktop_static_image`]),
    /// for video wallpapers a video frame (ffmpeg, into the cache), for a GIF
    /// animation the file itself (the decoder takes the first frame). `None` — a
    /// live wallpaper without a picture (scene, web, Flash) or could not find
    /// out: the backdrop is a flat fill.
    pub fn desktop_backdrop_image(&mut self) -> Option<String> {
        match self.current_format.clone().as_deref() {
            Some("video") => {
                let path = self.current_path.clone()?;
                let meta = std::fs::metadata(&path).ok();
                let stamp = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let key = crate::conky_overlay::key(&format!("{path}@{stamp}"));
                let out = format!("{}/rexon/conky-previews/backdrop/{key}.jpg", self.cache_home());
                if !self.sys.exists(&out) {
                    let _ = std::fs::create_dir_all(format!("{}/rexon/conky-previews/backdrop", self.cache_home()));
                    self.sys.run(&wpe::extract_frame_argv(&path, &out, 3.0), 30.0)?;
                }
                self.sys.exists(&out).then_some(out)
            }
            Some("animated") => self.current_path.clone().filter(|p| self.sys.exists(p)),
            _ => self.desktop_static_image(),
        }
    }

    pub fn blur_current_wallpaper(&mut self, radius: i64) -> bool {
        self.save_current_wallpaper();
        if self.saved_wallpaper.is_none() && wpe::uses_plasma_config(&self.de) {
            if let Some(image) = self.kde_current_static_image() {
                let exists = self.sys.exists(&image);
                if wpe::clean_image_candidate_ok(&image, exists) {
                    self.saved_wallpaper = Some(image);
                    self.saved_de = Some(self.de.clone());
                }
            }
        }
        let Some(source) = self.saved_wallpaper.clone() else {
            log_line("warning", "размытие: не удалось узнать текущие обои-картинку");
            return false;
        };
        if !self.sys.exists(&source) || self.sys.is_dir(&source) {
            log_line("warning", &format!("размытие: текущих обоев нет на диске: {source}"));
            return false;
        }
        let cache = self.cache_home();
        let output = wpe::blur_cache_output_path(&cache, &source);
        if let Err(e) = self.sys.blur_image(&source, &output, radius.clamp(1, 50)) {
            log_line("error", &format!("размытие не удалось: {e}"));
            return false;
        }
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::static_plan(&self.de, &self.static_session(), &output, &tools, &hint);
        let res = self.run_plan(&plan, &output);
        if !res.ok {
            log_line("error", &format!("размытая копия не встала: {}", res.message));
            return false;
        }
        self.blur_path = Some(output);
        true
    }

    /// Remove the blur: restore the remembered real wallpaper. False — nothing to
    /// restore (the same as in Python).
    pub fn unblur_wallpaper(&mut self) -> bool {
        if self.saved_wallpaper.is_none() {
            return false;
        }
        self.restore_wallpaper();
        self.blur_path = None;
        true
    }


    fn home(&mut self) -> String {
        let home = self.sys.env("HOME");
        if home.is_empty() {
            ".".to_string()
        } else {
            home
        }
    }

    /// Path of the wallpaper process PID file.
    fn pidfile(&mut self) -> String {
        let dir = self.sys.env("XDG_RUNTIME_DIR");
        let dir = if dir.is_empty() {
            format!("{}/.cache/rexon", self.home())
        } else {
            dir
        };
        format!("{dir}/{}", wpe::PIDFILE_NAME)
    }

    /// Write the PID of the live wallpaper process — orphans of the previous run
    /// are picked up by it.
    fn write_pidfile(&mut self) {
        let Some(proc) = self.process else {
            return;
        };
        if self.sys.poll(proc).is_some() {
            return; // the process is already dead — nothing to write
        }
        let Some(pid) = self.sys.pid(proc) else {
            return;
        };
        let path = self.pidfile();
        self.sys.write_text(&path, &pid.to_string());
    }

    /// Delete the PID file.
    fn remove_pidfile(&mut self) {
        let path = self.pidfile();
        self.sys.remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake seam: does nothing, records everything.
    ///
    /// The checks do not touch the live desktop at all — no `cargo test` has the
    /// right to take the wallpaper away from whoever ran it.
    #[derive(Default)]
    struct FakeSys {
        /// What was asked to be run and waited for.
        ran: Vec<Vec<String>>,
        /// What was asked to be started as a process (and whether in its own session).
        spawned: Vec<(Vec<String>, bool)>,
        /// Answers to `run` by the first word of the command.
        answers: HashMap<String, RunOut>,
        /// Commands that are "missing" from the system (`run` returns `None`).
        missing: Vec<String>,
        /// Programs that "exist".
        tools: Vec<String>,
        /// Exit codes of the started processes in order: `None` — alive.
        spawn_rc: Vec<Option<i64>>,
        /// Stopped processes.
        killed: Vec<ProcHandle>,
        /// Notices to the bridge.
        bridge: Vec<(bool, String, bool)>,
        /// Paths that are "missing" (everything else counts as existing).
        missing_paths: Vec<String>,
        /// Paths that count as directories.
        dirs: Vec<String>,
        /// Directory contents.
        listing: HashMap<String, Vec<String>>,
        /// Files that can be read.
        files: HashMap<String, String>,
        /// What was written and where.
        written: Vec<(String, String)>,
        /// What is written is immediately readable back (`files`).
        write_through: bool,
        /// What was deleted.
        removed: Vec<String>,
        /// Environment variables.
        envs: HashMap<String, String>,
        clock: f64,
        next: ProcHandle,
        /// What was asked to be blurred: (source, copy, strength).
        blurred: Vec<(String, String, i64)>,
        /// Blurring "fails".
        blur_fails: bool,
        /// Process freezes: (handle, whether to freeze).
        signals: Vec<(ProcHandle, bool)>,
        /// Where the wallpaper host "lives" (one for video and web).
        host: Option<String>,
        /// A video frame "gets extracted".
        frame_ok: bool,
        /// Which frames were asked to be extracted: (source, destination).
        frames: Vec<(String, String)>,
        /// Where the YouTube video "gets downloaded".
        yt_download: Option<String>,
        /// A foreign shell that "holds" the wallpaper layer.
        shell_owner: Option<DesktopShell>,
        /// The daemon's answer to "stop WE wallpapers"; `None` — no hook.
        we_stop: Option<(String, i64)>,
        /// When stopping WE wallpapers was requested: (commands run, processes started)
        /// at that moment — they show that stopping happened BEFORE our own wallpaper.
        we_stops: Vec<(usize, usize)>,
        /// When removing the waywallen output was requested — in the same measures.
        display_releases: Vec<(usize, usize)>,
        /// How many times the executor said "our own layer surface is up".
        layer_ups: usize,
        /// Answers to `run` by the end of the whole command line — take precedence over `answers`.
        tail_answers: Vec<(String, RunOut)>,
        /// The abandoned copy of the Cinnamon extension is "removed" at this path.
        orphan_copy: Option<String>,
        /// A "Cinnamon with layer-shell" session.
        cinnamon_layer: bool,
        /// How many times the executor marked the start of an apply.
        apply_marks: usize,
        /// Movie frames "are already" in the cache (`rexon/video-stills`); without
        /// this the cache is empty, and the frame is extracted anew.
        stills_cached: bool,
        /// How many commands had been run by each process start — they show what
        /// went before showing and what after.
        spawn_marks: Vec<usize>,
        /// Frames taken from the cache and refreshed.
        touched: Vec<String>,
        /// Frame cache prunes: (folder, what to leave alone).
        pruned: Vec<(String, String)>,
    }

    impl FakeSys {
        fn answer_tail(&mut self, tail: &str, stdout: &str) {
            self.tail_answers.push((
                tail.to_string(),
                RunOut { rc: 0, stdout: stdout.to_string(), stderr: String::new() },
            ));
        }
        fn answer(&mut self, bin: &str, rc: i64, stdout: &str) {
            self.answers.insert(
                bin.to_string(),
                RunOut { rc, stdout: stdout.to_string(), stderr: String::new() },
            );
        }
        fn stderr_answer(&mut self, bin: &str, rc: i64, stderr: &str) {
            self.answers.insert(
                bin.to_string(),
                RunOut { rc, stdout: String::new(), stderr: stderr.to_string() },
            );
        }
    }

    impl WpSys for FakeSys {
        fn run(&mut self, argv: &[String], _timeout: f64) -> Option<RunOut> {
            self.ran.push(argv.to_vec());
            let bin = argv.first().cloned().unwrap_or_default();
            if self.missing.contains(&bin) {
                return None;
            }
            let line = argv.join(" ");
            if let Some((_, out)) = self.tail_answers.iter().find(|(t, _)| line.ends_with(t.as_str())) {
                return Some(out.clone());
            }
            Some(self.answers.get(&bin).cloned().unwrap_or(RunOut {
                rc: 0,
                stdout: String::new(),
                stderr: String::new(),
            }))
        }
        fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String> {
            self.spawned.push((argv.to_vec(), new_session));
            self.spawn_marks.push(self.ran.len());
            let bin = argv.first().cloned().unwrap_or_default();
            if self.missing.contains(&bin) {
                return Err("нет такой команды".to_string());
            }
            self.next += 1;
            Ok(self.next)
        }
        fn poll(&mut self, proc: ProcHandle) -> Option<i64> {
            // The process's sequence number = its row in `spawn_rc`.
            let idx = (proc as usize).checked_sub(1)?;
            self.spawn_rc.get(idx).copied().flatten()
        }
        fn pid(&mut self, proc: ProcHandle) -> Option<i64> {
            Some(1000 + proc)
        }
        fn kill_process(&mut self, proc: ProcHandle) {
            self.killed.push(proc);
        }
        fn has_tool(&mut self, tool: &str) -> bool {
            self.tools.iter().any(|t| t == tool)
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, path: &str) -> bool {
            if path.contains("/rexon/video-stills/") {
                return self.stills_cached;
            }
            !self.missing_paths.iter().any(|p| p == path)
        }
        fn is_dir(&mut self, path: &str) -> bool {
            self.dirs.iter().any(|p| p == path)
        }
        fn listdir(&mut self, path: &str) -> Vec<String> {
            self.listing.get(path).cloned().unwrap_or_default()
        }
        fn read_text(&mut self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write_text(&mut self, path: &str, content: &str) {
            self.written.push((path.to_string(), content.to_string()));
            if self.write_through {
                self.files.insert(path.to_string(), content.to_string());
            }
        }
        fn remove_file(&mut self, path: &str) {
            self.removed.push(path.to_string());
        }
        fn env(&mut self, name: &str) -> String {
            self.envs.get(name).cloned().unwrap_or_default()
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 0.01;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, live: bool, name: &str, announce: bool) {
            self.bridge.push((live, name.to_string(), announce));
        }
        fn blur_image(&mut self, source: &str, output: &str, radius: i64) -> Result<(), String> {
            self.blurred.push((source.to_string(), output.to_string(), radius));
            if self.blur_fails {
                Err("картинка не разобрана".to_string())
            } else {
                Ok(())
            }
        }
        fn pause_process(&mut self, proc: ProcHandle, pause: bool) {
            self.signals.push((proc, pause));
        }
        fn wallpaper_host(&mut self) -> Option<String> {
            self.host.clone()
        }
        fn file_size(&mut self, _path: &str) -> Option<u64> {
            Some(4096)
        }
        fn extract_frame(&mut self, source: &str, output: &str, _timestamp: f64) -> bool {
            self.frames.push((source.to_string(), output.to_string()));
            self.frame_ok
        }
        fn touch_file(&mut self, path: &str) {
            self.touched.push(path.to_string());
        }
        fn prune_video_stills(&mut self, dir: &str, keep: &str) {
            self.pruned.push((dir.to_string(), keep.to_string()));
        }
        fn download_youtube(&mut self, _url: &str, _quality: &str) -> Option<String> {
            self.yt_download.clone()
        }
        fn wallpaper_shell_owner(&mut self, _de: &str, _session: &str) -> Option<DesktopShell> {
            self.shell_owner
        }
        fn stop_we_wallpapers(&mut self, _timeout_ms: u64) -> Option<(String, i64)> {
            self.we_stops.push((self.ran.len(), self.spawned.len()));
            self.we_stop.clone()
        }
        fn release_waywallen_display(&mut self) {
            self.display_releases.push((self.ran.len(), self.spawned.len()));
        }
        fn own_layer_surface_up(&mut self) {
            self.layer_ups += 1;
        }
        fn cinnamon_layer_shell(&self) -> bool {
            self.cinnamon_layer
        }
        fn remove_orphan_shell_ext_copy(&mut self, de: &str) -> Option<String> {
            self.ran.push(vec!["<remove-orphan-copy>".to_string(), de.to_string()]);
            self.orphan_copy.clone()
        }
        fn note_apply_started(&mut self) {
            self.apply_marks += 1;
        }
    }

    fn exec(de: &str, session: &str, sys: FakeSys) -> WallpaperExec<FakeSys> {
        WallpaperExec::with_env(sys, de, session)
    }

    /// A live WE wallpaper and a static picture: the executor FIRST stops the WE
    /// wallpaper (before the plan's first command), then sets the picture —
    /// otherwise it would land under the WE surface and the person would see the
    /// previous wallpaper.
    #[test]
    fn static_wallpaper_stops_live_we_wallpapers_first() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        sys.we_stop = Some((crate::we_client::STOP_STOPPED.to_string(), 2));
        let mut e = exec("kde", "wayland", sys);
        let res = e.apply("/home/u/лес.png", "static", "");
        assert!(res.ok, "{}", res.message);
        let sys = e.sys_mut();
        assert_eq!(sys.we_stops, vec![(0, 0)], "WE-обои сняты не первыми: {:?}", sys.we_stops);
        assert!(!sys.ran.is_empty(), "картинка так и не поставлена");
        assert!(!res.message.contains("Wallpaper Engine"), "лишняя приписка: {}", res.message);
    }

    /// The waywallen output (GNOME: the extension with a black placeholder) is
    /// removed together with stopping WE wallpapers — before our own wallpaper
    /// and even without the stop hook; not for the sake of a refusal.
    #[test]
    fn own_wallpaper_releases_waywallen_display() {
        for fmt in ["static", "video"] {
            let mut sys = FakeSys::default();
            sys.host = Some("/usr/bin/rexon-wallpaper-host".to_string());
            sys.we_stop = None;
            let mut e = exec("gnome", "wayland", sys);
            e.apply("/home/u/лес.png", fmt, "");
            let releases = e.sys_mut().display_releases.clone();
            assert_eq!(releases, vec![(0, 0)], "{fmt}: {releases:?}");
        }
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/home/u/нет.png".to_string());
        let mut e = exec("gnome", "wayland", sys);
        assert!(!e.apply("/home/u/нет.png", "static", "").ok);
        assert!(e.sys_mut().display_releases.is_empty(), "убрали ради отказа");
    }

    /// Stopping the WE wallpaper failed: our wallpaper is set anyway, and the
    /// person learns why it is not visible. No daemon / no hook — silently.
    #[test]
    fn we_stop_failure_is_told_and_absence_is_silent() {
        for (answer, told) in [
            (Some(crate::we_client::STOP_ERROR), true),
            // The daemon answered that there is nothing to stop (neither renderers nor
            // monitors) — silently: nothing to warn about.
            (Some(crate::we_client::STOP_STOPPED), false),
            (Some(crate::we_client::STOP_NOT_RUNNING), false),
            (Some(crate::we_client::STOP_NO_QT), false),
            (None, false),
        ] {
            let mut sys = FakeSys::default();
            sys.tools.push("plasma-apply-wallpaperimage".to_string());
            sys.we_stop = answer.map(|a| (a.to_string(), 0));
            let mut e = exec("kde", "wayland", sys);
            let res = e.apply("/home/u/лес.png", "static", "");
            assert!(res.ok, "{answer:?}: {}", res.message);
            let note = we_yield_note(crate::we_client::STOP_ERROR).unwrap();
            assert_eq!(res.warning.as_deref() == Some(note.as_str()), told, "{answer:?}: {res:?}");
            // The warning is a separate field, it is not glued into the success message
            // (otherwise the result would go under a green check mark).
            assert!(!res.message.contains(&note), "{answer:?}: {}", res.message);
        }
    }

    /// The other wallpaper is not stopped for the sake of a refusal: the file is
    /// missing, the format is withdrawn from the product, a foreign shell holds the layer.
    #[test]
    fn refusals_do_not_stop_we_wallpapers() {
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/home/u/нет.png".to_string());
        sys.we_stop = Some((crate::we_client::STOP_STOPPED.to_string(), 1));
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.apply("/home/u/нет.png", "static", "").ok);
        assert!(!e.apply("/home/u/сцена", "scene", "").ok);
        assert!(e.sys_mut().we_stops.is_empty(), "сняли ради отказа: {:?}", e.sys_mut().we_stops);

        let mut sys = FakeSys::default();
        sys.shell_owner = Some(DesktopShell::Noctalia);
        sys.we_stop = Some((crate::we_client::STOP_STOPPED.to_string(), 1));
        let mut e = exec("hyprland", "wayland", sys);
        assert!(!e.apply("/home/u/лес.png", "static", "").ok);
        assert!(e.sys_mut().we_stops.is_empty(), "сняли при чужом шелле");
    }

    /// All of our own formats stop WE wallpapers; withdrawn ones do not.
    #[test]
    fn every_own_format_yields_we_wallpapers() {
        for fmt in ["static", "animated", "video", "youtube", "web", "swf"] {
            assert!(yields_we_wallpapers(fmt), "{fmt}");
        }
        for fmt in ["scene", "application", "", "что-то"] {
            assert!(!yields_we_wallpapers(fmt), "{fmt}");
        }
    }

    /// The first successful step ends the plan: the second one never gets its turn.
    #[test]
    fn the_first_step_that_works_ends_the_plan() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        let plan = wpe::static_plan(
            "kde",
            "wayland",
            "/tmp/нет.png",
            &["plasma-apply-wallpaperimage".to_string()],
            "",
        );
        let mut e = exec("kde", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "первый шаг отдал успех: {}", res.message);
        assert_eq!(e.sys_mut().ran.len(), 1, "запущено больше одного шага");
    }

    /// A failed NON-terminal step leads to the next one, not to a refusal:
    /// exactly this makes the plan's fallback paths reachable.
    #[test]
    fn a_failed_step_hands_over_to_the_next_one() {
        let mut sys = FakeSys::default();
        // swww answers non-zero (the daemon answers query but does not take the
        // picture), and the queue must get to swaybg.
        sys.answer("swww", 1, "");
        sys.tools.push("swww".to_string());
        sys.tools.push("swaybg".to_string());
        sys.spawn_rc = vec![None];
        let tools = vec!["swww".to_string(), "swaybg".to_string()];
        let plan = wpe::static_plan("sway", "wayland", "/tmp/нет.png", &tools, "");
        assert!(plan.len() >= 2, "у плана wlroots нет запасного шага");
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "запасной шаг не отработал: {}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned.iter().any(|(argv, _)| argv[0] == "swaybg"),
            "до запасного шага очередь не дошла: {spawned:?}"
        );
    }

    /// `{stderr}` of the terminal step is filled with the command's real output —
    /// otherwise the customer would see a refusal without a reason.
    #[test]
    fn the_terminal_failure_carries_the_real_stderr() {
        let mut sys = FakeSys::default();
        sys.stderr_answer("gsettings", 1, "схема не найдена");
        let plan = wpe::static_plan("gnome", "wayland", "/tmp/нет.png", &[], "");
        let mut e = exec("gnome", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(!res.ok);
        assert!(!res.message.is_empty(), "отказ без слов");
    }

    /// COSMIC: the picture goes into cosmic-bg's native key (nothing is launched),
    /// our former swaybg is stopped; the key is in `$XDG_CONFIG_HOME`.
    #[test]
    fn cosmic_static_wallpaper_is_written_to_cosmic_bg() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["cosmic-bg".to_string(), "swaybg".to_string()];
        sys.write_through = true;
        sys.envs.insert("HOME".to_string(), "/h".to_string());
        sys.envs.insert("XDG_CONFIG_HOME".to_string(), "/x".to_string());
        let mut e = exec("cosmic", "wayland", sys);
        e.static_holder = Some(77);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("cosmic", "wayland", "/w.png", &tools, "");
        let res = e.run_plan(&plan, "/w.png");
        assert!(res.ok, "{}", res.message);
        let all = "/x/cosmic/com.system76.CosmicBackground/v1/all";
        let written = e.sys_mut().files.get(all).cloned().unwrap_or_default();
        assert_eq!(wpe::cosmic_bg_source_path(&written).as_deref(), Some("/w.png"));
        assert!(e.sys_mut().spawned.is_empty(), "поднят свой демон фона");
        assert_eq!(e.sys_mut().killed, vec![77], "прежний swaybg не снят");
        // Exactly this key is remembered before a live wallpaper.
        e.saved_wallpaper = None;
        e.save_current_wallpaper();
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/w.png"));
    }

    /// sway: the background is set by sway itself (`swaymsg output * bg`), our
    /// own swaybg is not started, and our former one is stopped — otherwise it
    /// would hang around as an extra process. No sway session (swaymsg did not
    /// answer) — fallback swaybg.
    #[test]
    fn sway_static_wallpaper_uses_sway_own_background() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["swaymsg".to_string(), "swaybg".to_string()];
        let mut e = exec("sway", "wayland", sys);
        e.static_holder = Some(77);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("sway", "wayland", "/w.png", &tools, "");
        let res = e.run_plan(&plan, "/w.png");
        assert!(res.ok, "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty(), "поднят свой swaybg");
        assert_eq!(e.sys_mut().killed, vec![77], "прежний swaybg не снят");
        assert!(e.sys_mut().ran.iter().any(|a| a[0] == "swaymsg"), "{:?}", e.sys_mut().ran);

        let mut sys = FakeSys::default();
        sys.tools = vec!["swaymsg".to_string(), "swaybg".to_string()];
        sys.answer("swaymsg", 1, "");
        sys.spawn_rc = vec![None];
        let mut e = exec("sway", "wayland", sys);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("sway", "wayland", "/w.png", &tools, "");
        assert!(e.run_plan(&plan, "/w.png").ok);
        assert_eq!(e.sys_mut().spawned.len(), 1, "запасной swaybg не поднят");
    }

    /// Writing the COSMIC key failed (did not read back) — the plan moves on to
    /// the external daemon rather than reporting success.
    #[test]
    fn cosmic_bg_failure_falls_back_to_a_daemon() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["cosmic-bg".to_string(), "swaybg".to_string()];
        sys.spawn_rc = vec![None];
        let mut e = exec("cosmic", "wayland", sys);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("cosmic", "wayland", "/w.png", &tools, "");
        let res = e.run_plan(&plan, "/w.png");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1, "запасной swaybg не поднят");
    }

    /// A `fail` step answers with a refusal without launching anything.
    #[test]
    fn a_fail_step_runs_nothing() {
        let sys = FakeSys::default();
        // GNOME-Wayland: nothing to show an animation with — a plan of a single refusal.
        let plan = wpe::animated_plan(
            "gnome",
            "wayland",
            "/tmp/нет.gif",
            &[],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("gnome", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.gif");
        assert!(!res.ok);
        assert!(e.sys_mut().ran.is_empty(), "отказ что-то запустил");
        assert!(e.sys_mut().spawned.is_empty(), "отказ что-то поднял");
    }

    /// A dead launch does NOT count as success: a process that died within the
    /// grace period sends the plan to the fallback step. This is exactly the fix
    /// for the green „wallpaper started“ on an empty desktop.
    #[test]
    fn a_process_that_dies_at_once_is_not_a_success() {
        let mut sys = FakeSys::default();
        sys.tools.push("swaybg".to_string());
        sys.spawn_rc = vec![Some(1)]; // the very first started one is already dead
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/нет.png",
            &["swaybg".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(!res.ok, "мёртвый запуск прошёл как успех: {}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1);
        assert_eq!(e.sys_mut().killed.len(), 1, "труп не подобран");
    }

    /// A live launch is a success, the process is remembered and the PID is put into the message.
    #[test]
    fn a_living_process_is_remembered() {
        let mut sys = FakeSys::default();
        sys.tools.push("swaybg".to_string());
        sys.spawn_rc = vec![None]; // alive
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/нет.png",
            &["swaybg".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "живой запуск не прошёл: {}", res.message);
        assert!(e.static_holder.is_some(), "держатель фона не запомнен");
        assert!(!res.message.contains("{pid}"), "метка PID осталась сырой");
    }

    /// System observations are requested LAZILY: a plan without placeholders must
    /// not poke `wlr-randr`/`xrandr` at all.
    #[test]
    fn observations_are_asked_only_when_a_step_needs_them() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        let plan = wpe::static_plan(
            "kde",
            "wayland",
            "/tmp/нет.png",
            &["plasma-apply-wallpaperimage".to_string()],
            "",
        );
        let mut e = exec("kde", "wayland", sys);
        e.run_plan(&plan, "/tmp/нет.png");
        let asked: Vec<String> = e
            .sys_mut()
            .ran
            .iter()
            .filter_map(|c| c.first().cloned())
            .collect();
        assert!(!asked.iter().any(|b| b == "wlr-randr" || b == "xrandr"));
    }

    /// And when there is a placeholder, the observation is requested ONCE per run.
    #[test]
    fn an_observation_is_asked_once_per_plan() {
        let mut sys = FakeSys::default();
        sys.answer("xrandr", 0, "   1280x720     60.00*+\n");
        sys.tools.push("xwinwrap".to_string());
        sys.tools.push("mpv".to_string());
        sys.spawn_rc = vec![None];
        let tools = vec!["xwinwrap".to_string(), "mpv".to_string()];
        let plan = wpe::animated_plan(
            "generic",
            "x11",
            "/tmp/нет.gif",
            &tools,
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("generic", "x11", sys);
        e.run_plan(&plan, "/tmp/нет.gif");
        let xrandr = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| c.first().map(|b| b == "xrandr").unwrap_or(false))
            .count();
        assert_eq!(xrandr, 1, "разрешение спрошено {xrandr} раз вместо одного");
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, _)| argv.iter().any(|a| a.contains("1280x720"))),
            "метка разрешения не подставлена: {spawned:?}"
        );
    }

    /// Xfce: the path is written into EVERY `/last-image` property, not just the first.
    #[test]
    fn xfce_writes_every_monitor_property() {
        let mut sys = FakeSys::default();
        sys.answer(
            "xfconf-query",
            0,
            "/backdrop/screen0/monitorHDMI-1/workspace0/last-image\n\
             /backdrop/screen0/monitorDP-2/workspace0/last-image\n\
             /backdrop/screen0/monitorDP-2/workspace0/image-style\n",
        );
        let plan = wpe::static_plan("xfce", "x11", "/tmp/нет.png", &[], "");
        let mut e = exec("xfce", "x11", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "{}", res.message);
        let writes = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| c.iter().any(|a| a.contains("last-image")) && c.contains(&"-s".to_string()))
            .count();
        assert_eq!(writes, 2, "записано {writes} свойств вместо двух");
    }

    /// Xfce without a single background property — a refusal with the plan's
    /// SECOND message, not the first: "no monitors" and "xfconf-query did not
    /// answer" are different troubles.
    #[test]
    fn xfce_without_properties_says_the_other_reason() {
        let mut sys = FakeSys::default();
        sys.answer("xfconf-query", 0, "/backdrop/screen0/monitorDP-2/workspace0/image-style\n");
        let plan = wpe::static_plan("xfce", "x11", "/tmp/нет.png", &[], "");
        let alt = plan[0].alt_message.clone();
        let err = plan[0].err_message.clone();
        let mut e = exec("xfce", "x11", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(!res.ok);
        assert_eq!(res.message, alt);
        assert_ne!(res.message, err, "две разные беды объяснены одной строкой");
    }

    /// A plan that produced no outcome from any step still ANSWERS.
    #[test]
    fn an_exhausted_plan_still_answers() {
        let sys = FakeSys::default();
        let mut e = exec("generic", "x11", sys);
        let res = e.run_plan(&[], "/tmp/нет.png");
        assert!(!res.ok);
        assert!(!res.message.is_empty(), "пустой план ответил молчанием");
    }

    /// A static wallpaper success does not lie to the bridge: static is not a
    /// live wallpaper, and the `video_wallpaper_active` flag must be false.
    #[test]
    fn static_wallpaper_is_not_announced_as_live() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        let plan = wpe::static_plan(
            "kde",
            "wayland",
            "/tmp/нет.png",
            &["plasma-apply-wallpaperimage".to_string()],
            "",
        );
        let mut e = exec("kde", "wayland", sys);
        e.run_plan(&plan, "/tmp/нет.png");
        let said = e.sys_mut().bridge.clone();
        assert_eq!(said.len(), 1, "мосту сказано {} раз", said.len());
        assert!(!said[0].0, "статика объявлена живыми обоями");
    }

    /// The swww daemon is started only when a step asks for it, and it is not
    /// started again while the daemon is alive.
    #[test]
    fn the_swww_daemon_is_not_raised_twice() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, ""); // both query and img answer zero
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/нет.gif",
            &["swww".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.gif");
        assert!(res.ok, "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty(), "демон поднят при живом демоне");
        let queries = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| c.get(1).map(|a| a == "query").unwrap_or(false))
            .count();
        assert_eq!(queries, 1);
    }

    // ── stop and desktop restore ──────────────────────────────────────────

    /// The run's flat commands as one string — easier to search for the order.
    fn lines(e: &mut WallpaperExec<FakeSys>) -> Vec<String> {
        e.sys_mut().ran.iter().map(|c| c.join(" ")).collect()
    }

    /// A stop with nothing running is not silent: the customer pressed the button
    /// and must get an answer, even when there was nothing to stop.
    #[test]
    fn stopping_nothing_still_answers() {
        let sys = FakeSys::default();
        let mut e = exec("generic", "x11", sys);
        let res = e.stop(true);
        assert!(res.ok);
        assert_eq!(res.message, wpe::stopped_message(None));
        let said = e.sys_mut().bridge.clone();
        assert_eq!(said.len(), 1, "мосту не сказано об остановке");
        assert!(!said[0].0, "после остановки мосту обещаны живые обои");
    }

    /// Cinnamon with layer-shell: the extension is taken off the wallpaper — first
    /// it is disabled (foreign uuids stay in place), then its keys are cleared,
    /// and all of this once; with a screen effect the extension stays enabled.
    #[test]
    fn cinnamon_layer_shell_retires_extension_wallpaper() {
        let mut sys = FakeSys::default();
        sys.answer_tail("rexon-wallpaper source", "'/home/u/.cache/rexon/faststart/bb.mp4'\n");
        sys.answer_tail("rexon-wallpaper effect-shader", "''\n");
        sys.answer_tail("get org.cinnamon enabled-extensions", &format!("['a@x', '{}']\n", wpe::SHELL_EXT_ID));
        let mut e = exec("cinnamon-mint", "wayland", sys);
        e.retire_shell_extension_wallpaper();
        let ran = lines(&mut e);
        let disable = ran.iter().position(|l| l.contains("set org.cinnamon enabled-extensions")).expect("выключили");
        assert!(ran[disable].ends_with("[\"a@x\"]"), "{}", ran[disable]);
        let clear = ran.iter().position(|l| l.ends_with("set org.cinnamon.extensions.rexon-wallpaper source ")).expect("источник очищен");
        assert!(ran.iter().any(|l| l.ends_with("set org.cinnamon.extensions.rexon-wallpaper daemon-argv []")));
        assert!(disable < clear, "выключить ДО правки ключей — иначе правка будит расширение");
        let before = ran.len();
        e.retire_shell_extension_wallpaper();
        assert_eq!(lines(&mut e).len(), before, "второй раз — ничего");

        let mut sys = FakeSys::default();
        sys.answer_tail("rexon-wallpaper source", "''\n");
        sys.answer_tail("rexon-wallpaper effect-shader", "'/home/u/.config/rexon/screen-effects/shell-a.glsl'\n");
        sys.answer_tail("get org.cinnamon enabled-extensions", &format!("['{}']\n", wpe::SHELL_EXT_ID));
        let mut e = exec("cinnamon", "wayland", sys);
        e.retire_shell_extension_wallpaper();
        assert!(!lines(&mut e).iter().any(|l| l.contains(" set ")), "эффект идёт, ключи пусты — ничего не пишем");

        let mut e = exec("gnome", "wayland", FakeSys::default());
        e.retire_shell_extension_wallpaper();
        assert!(lines(&mut e).is_empty(), "GNOME — не наш случай");
    }

    /// The abandoned copy of the extension is removed with the same function as
    /// for screen effects, and BEFORE reading the keys (the schema directory is
    /// then the system one); if the extension stays for the effect, the shell
    /// re-reads it (`ReloadXlet`), if it is disabled there is nothing to re-read.
    #[test]
    fn cinnamon_layer_shell_removes_orphan_copy_like_screen_effects() {
        let copy = "/home/u/.local/share/cinnamon/extensions/rexon-wallpaper@rexon.app".to_string();
        let mut sys = FakeSys { orphan_copy: Some(copy.clone()), ..FakeSys::default() };
        sys.answer_tail("rexon-wallpaper source", "''\n");
        sys.answer_tail("rexon-wallpaper effect-shader", "'/home/u/.config/rexon/screen-effects/shell-a.glsl'\n");
        sys.answer_tail("get org.cinnamon enabled-extensions", &format!("['{}']\n", wpe::SHELL_EXT_ID));
        let mut e = exec("cinnamon", "wayland", sys);
        e.retire_shell_extension_wallpaper();
        let ran = lines(&mut e);
        let removed = ran.iter().position(|l| l == "<remove-orphan-copy> cinnamon").expect("копию снимали");
        let read = ran.iter().position(|l| l.ends_with("rexon-wallpaper source")).expect("ключи читали");
        assert!(removed < read, "{ran:?}");
        let reload = crate::screen_effects::cinnamon_reload_argv().join(" ");
        assert!(ran.iter().any(|l| *l == reload), "эффект идёт — перечитать расширение: {ran:?}");

        let mut sys = FakeSys { orphan_copy: Some(copy), ..FakeSys::default() };
        sys.answer_tail("rexon-wallpaper source", "''\n");
        sys.answer_tail("rexon-wallpaper effect-shader", "''\n");
        sys.answer_tail("get org.cinnamon enabled-extensions", &format!("['{}']\n", wpe::SHELL_EXT_ID));
        let mut e = exec("cinnamon", "wayland", sys);
        e.retire_shell_extension_wallpaper();
        let ran = lines(&mut e);
        assert!(ran.iter().any(|l| l.contains("set org.cinnamon enabled-extensions")), "{ran:?}");
        assert!(!ran.iter().any(|l| l.contains("ReloadXlet")), "выключено — перечитывать нечего: {ran:?}");

        // No copy — nothing to re-read either.
        let mut sys = FakeSys::default();
        sys.answer_tail("rexon-wallpaper effect-shader", "'/x.glsl'\n");
        sys.answer_tail("get org.cinnamon enabled-extensions", &format!("['{}']\n", wpe::SHELL_EXT_ID));
        let mut e = exec("cinnamon", "wayland", sys);
        e.retire_shell_extension_wallpaper();
        assert!(!lines(&mut e).iter().any(|l| l.contains("ReloadXlet")));
    }

    /// Cinnamon without layer-shell: Rexon enables its disabled extension itself
    /// (foreign uuids stay in the list), and if it did not get enabled — an
    /// honest refusal, not «✅ applied» on an empty desktop.
    #[test]
    fn disabled_cinnamon_extension_is_turned_on_or_refused() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "['a@b']\n");
        let mut e = exec("cinnamon", "wayland", sys);
        let plan = wpe::shell_ext_plan("cinnamon", "/x/a.mp4", HOST, false);
        let res = e.run_shell_extension(plan, "/x/a.mp4", "video");
        let enable = lines(&mut e)
            .into_iter()
            .find(|l| l.contains("set org.cinnamon enabled-extensions"))
            .expect("Rexon включает расширение сам");
        assert!(enable.contains("a@b") && enable.contains(wpe::SHELL_EXT_ID), "{enable}");
        assert!(!res.ok, "оболочка так и не включила — не удача: {}", res.message);

        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, &format!("['a@b', '{}']\n", wpe::SHELL_EXT_ID));
        let mut e = exec("cinnamon", "wayland", sys);
        let plan = wpe::shell_ext_plan("cinnamon", "/x/a.mp4", HOST, false);
        assert!(e.run_shell_extension(plan, "/x/a.mp4", "video").ok);
        assert!(!lines(&mut e).iter().any(|l| l.contains("set org.cinnamon enabled-extensions")), "включённое не трогаем");
    }

    /// Cinnamon 6.8+: the previous picture list is remembered in full (with
    /// fitting, colors, per-monitor pictures), and the "what was set" picture is
    /// taken from it; a stop restores the list and the mode as they were, not a
    /// single picture via `picture-uri`.
    #[test]
    fn cinnamon_picture_list_is_saved_and_restored_whole() {
        let list = "[{'picture-uri': <'file:///usr/share/backgrounds/a.jpg'>, 'picture-options': <'scaled'>}]";
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, &format!("{list}\n"));
        let mut e = exec("cinnamon", "wayland", sys);
        e.save_cinnamon_bg_list();
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/usr/share/backgrounds/a.jpg"));
        assert_eq!(e.saved_cinnamon_bg.as_ref().map(|(l, _)| l.as_str()), Some(list));
        e.sys_mut().ran.clear();
        e.saved_de = Some("cinnamon".to_string());
        e.stop(true);
        let out = lines(&mut e);
        assert!(
            out.iter().any(|l| l == &format!("gsettings set org.cinnamon.desktop.background picture-uri-list {list}")),
            "{out:?}"
        );
        assert!(out.iter().any(|l| l.starts_with("gsettings set org.cinnamon.desktop.background background-mode")));
        assert!(!out.iter().any(|l| l.contains("picture-uri file://")), "одной картинкой не возвращаем: {out:?}");
        assert!(e.saved_cinnamon_bg.is_none() && e.saved_wallpaper.is_none());
    }

    /// Old Cinnamon (no list key): the former single-picture path.
    #[test]
    fn old_cinnamon_without_list_keeps_single_picture() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 1, "");
        let mut e = exec("cinnamon", "x11", sys);
        e.save_cinnamon_bg_list();
        assert!(e.saved_cinnamon_bg.is_none());
    }

    /// A stop names the format that WAS PLAYING, not the one the background
    /// restore wrote. This is exactly the fix for „static“ being said after a video.
    #[test]
    fn stopping_names_the_format_that_played() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "");
        let mut e = exec("gnome", "wayland", sys);
        e.current_format = Some("video".to_string());
        e.saved_wallpaper = Some("/tmp/старые.png".to_string());
        e.saved_de = Some("gnome".to_string());
        let res = e.stop(true);
        assert!(res.message.contains(&wpe::format_label("video")), "{}", res.message);
        assert!(
            !res.message.contains(&wpe::format_label("static")),
            "возврат фона переписал формат: {}",
            res.message
        );
    }

    /// The restore sets exactly the saved wallpaper — with the same plan as a
    /// normal apply.
    #[test]
    fn stopping_puts_the_saved_wallpaper_back() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "");
        let mut e = exec("gnome", "wayland", sys);
        e.saved_wallpaper = Some("/tmp/старые.png".to_string());
        e.saved_de = Some("gnome".to_string());
        e.stop(true);
        assert!(
            lines(&mut e)
                .iter()
                .any(|l| l.contains("picture-uri file:///tmp/старые.png")),
            "сохранённые обои не вернулись: {:?}",
            lines(&mut e)
        );
    }

    /// The restore does not set a wallpaper that is not on disk: the command
    /// would pass, but the desktop would stay empty — and we would not notice.
    #[test]
    fn a_saved_wallpaper_that_vanished_is_not_restored() {
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/tmp/удалённые.png".to_string());
        let mut e = exec("gnome", "wayland", sys);
        e.saved_wallpaper = Some("/tmp/удалённые.png".to_string());
        e.saved_de = Some("gnome".to_string());
        e.stop(true);
        assert!(
            !lines(&mut e).iter().any(|l| l.contains("удалённые.png")),
            "поставлены обои, которых нет"
        );
    }

    /// The wallpaper that was there before us is read ONCE: a second read would
    /// record our own substitute as "what was there before us".
    #[test]
    fn the_previous_wallpaper_is_read_only_once() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///tmp/старые.png'\n");
        let mut e = exec("gnome", "wayland", sys);
        e.save_current_wallpaper();
        e.save_current_wallpaper();
        let gets = lines(&mut e).iter().filter(|l| l.contains("gsettings get")).count();
        assert_eq!(gets, 1, "прежние обои прочитаны {gets} раз");
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/tmp/старые.png"));
    }

    /// KDE: the foreign desktop plugin is restored AFTER the picture. The reverse
    /// order wiped out the restore — the picture plan on KDE unconditionally sets org.kde.image.
    #[test]
    fn the_foreign_kde_plugin_comes_back_after_the_image() {
        let sys = FakeSys::default();
        let mut e = exec("kde", "wayland", sys);
        e.saved_wallpaper = Some("/tmp/старые.png".to_string());
        e.saved_de = Some("kde".to_string());
        e.saved_kde_plugin = Some("org.waywallen.kde".to_string());
        e.stop(true);
        let out = lines(&mut e);
        let reset = out.iter().position(|l| l.contains("wallpaperPlugin = 'org.kde.image'"));
        let back = out.iter().position(|l| l.contains("org.waywallen.kde"));
        assert!(reset.is_some(), "штатный плагин не возвращался: {out:?}");
        assert!(back.is_some(), "чужой плагин не возвращён: {out:?}");
        assert!(reset < back, "чужой плагин возвращён ДО картинки — затрётся");
    }

    /// A stop kills ONLY its own process — by handle, never by name.
    #[test]
    fn stopping_kills_only_our_own_process() {
        let mut sys = FakeSys::default();
        sys.spawn_rc = vec![None];
        let mut e = exec("generic", "x11", sys);
        let proc = e.sys_mut().spawn(&["mpv".to_string()], true).unwrap();
        e.process = Some(proc);
        e.stop(true);
        assert_eq!(e.sys_mut().killed, vec![proc]);
        assert!(e.process.is_none());
    }

    /// MATE: icons are not touched if the desktop is not owned by Caja. Only
    /// hiding has the check — icons must be brought back in any case.
    #[test]
    fn mate_icons_are_left_alone_when_caja_does_not_own_the_desktop() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "false\n");
        let mut e = exec("mate", "x11", sys);
        e.set_desktop_icons(false);
        let hidden = lines(&mut e).iter().filter(|l| l.contains("set org.mate")).count();
        assert_eq!(hidden, 0, "иконки погашены под чужим столом");
        e.set_desktop_icons(true);
        let shown = lines(&mut e)
            .iter()
            .filter(|l| l.contains("set org.mate.background show-desktop-icons true"))
            .count();
        assert_eq!(shown, 1, "иконки не вернулись");
    }

    /// The PID file is written by a live process and removed by a stop: orphans
    /// of the previous run are picked up by it.
    #[test]
    fn the_pid_file_follows_the_process() {
        let mut sys = FakeSys::default();
        sys.envs.insert("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string());
        sys.spawn_rc = vec![None];
        let mut e = exec("generic", "x11", sys);
        let proc = e.sys_mut().spawn(&["mpv".to_string()], true).unwrap();
        e.process = Some(proc);
        e.write_pidfile();
        let written = e.sys_mut().written.clone();
        assert_eq!(written.len(), 1);
        assert!(written[0].0.ends_with(wpe::PIDFILE_NAME), "{}", written[0].0);
        assert!(!written[0].1.is_empty(), "PID-файл пуст");
        e.stop(true);
        assert_eq!(e.sys_mut().removed.len(), 1, "PID-файл остался после остановки");
    }

    /// A missing file — a refusal with the file's NAME, not a generic "did not work".
    #[test]
    fn applying_a_missing_file_names_it() {
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/tmp/нет-такого.png".to_string());
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/tmp/нет-такого.png", "static", "");
        assert!(!res.ok);
        assert!(res.message.contains("нет-такого.png"), "{}", res.message);
        assert!(e.sys_mut().ran.is_empty(), "отказ что-то запустил");
    }

    /// The executor carries all live formats — none answers with the "for now the
    /// classic window shows it" boundary. Flash was the last to move over.
    #[test]
    fn every_live_format_is_carried() {
        for fmt in wpe::LIVE_FORMATS {
            assert!(carries_format(fmt), "«{fmt}» отослан бы в классическое окно");
            let mut e = exec("kde", "wayland", FakeSys::default());
            let res = e.apply("/tmp/что-то", fmt, "");
            assert!(!res.message.contains(&live_formats_not_carried()), "{fmt}: {}", res.message);
        }
    }

    /// The "what the executor carries" rule and the `apply` boundary itself are
    /// one and the same: the window asks `carries_format`, and the answer must
    /// match what the executor will do, otherwise the button would promise one
    /// thing and do another.
    #[test]
    fn the_carried_formats_rule_matches_apply() {
        assert!(carries_format("static"));
        assert!(carries_format("web"));
        assert!(carries_format("video"));
        assert!(carries_format("youtube"));
        assert!(carries_format("swf"), "Flash снова граница");
        // Flash without a host — an honest "nothing to show with" refusal, not the boundary.
        let mut e = exec("gnome", "wayland", FakeSys::default());
        let res = e.apply("/tmp/ролик.swf", "swf", "");
        assert_eq!(res.message, wpe::video_host_missing_message());
        // Everything else is carried: either applied or definitively refused —
        // and no answer says "the previous engine shows it for now".
        for fmt in ["static", "animated", "scene", "application", "", "что-то-новое"] {
            assert!(carries_format(fmt), "формат «{fmt}» отослан бы в классическое окно");
            let mut e = exec("gnome", "wayland", FakeSys::default());
            let res = e.apply("/tmp/что-то", fmt, "");
            assert!(!res.message.contains(&live_formats_not_carried()), "{fmt}: {}", res.message);
        }
    }

    /// Scenes, application wallpapers and an unknown format — the same words as in
    /// Python, and ⭐ a fix: the refusal does NOT stop a running live wallpaper
    /// (Python turned it off first and refused afterwards).
    #[test]
    fn retired_formats_refuse_without_touching_live_wallpaper() {
        for (fmt, want) in [
            ("scene", wpe::scene_unsupported()),
            ("application", wpe::application_unsupported()),
            ("parallax", wpe::unknown_format_message("parallax")),
        ] {
            let (mut e, handle) = exec_with_live_process();
            let res = e.apply("/tmp/пакет/data/обои", fmt, "");
            assert!(!res.ok, "{fmt}");
            assert_eq!(res.message, want, "{fmt}");
            assert!(!e.sys_mut().killed.contains(&handle), "{fmt}: отказ снял живые обои");
            assert_eq!(e.current_format(), Some("web"), "{fmt}: отказ переписал формат");
        }
    }

    /// GIF on sway with swww: the engine plan is executed, the format is
    /// remembered as "animation" — as in Python's `_apply_animated`. `swww img`
    /// is a one-off command to the swww daemon, there is no process of its own
    /// behind it, so there is no PID file either (it is written only for a live process).
    #[test]
    fn animated_gif_runs_the_engine_plan() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/tmp/котик.gif", "animated", "");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.current_format(), Some("animated"));
        let ran = e.sys_mut().ran.clone();
        assert!(
            ran.iter().any(|a| a.len() > 2 && a[0] == "swww" && a[1] == "img" && a[2] == "/tmp/котик.gif"),
            "swww img не запущен: {ran:?}"
        );
        assert!(
            !e.sys_mut().written.iter().any(|(p, _)| p.ends_with(wpe::PIDFILE_NAME)),
            "PID-файл записан без процесса"
        );
        assert!(!format_wants_load_monitor(e.current_format()), "GIF под надзором — у Python нет");
    }

    /// ⭐ A fix: on GNOME-Wayland the GIF plan is a single refusal, and it is
    /// given BEFORE doing anything: the live wallpaper is not stopped, the
    /// backdrop was not set, nothing was launched.
    /// The executor's English answers contain no Russian letters (the journal
    /// does not count: we read it, it is not translated).
    #[test]
    fn english_exec_answers_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let mut e = exec("kde", "wayland", FakeSys::default());
        let no_ext = e.run_shell_extension(None, "/x/a.mp4", "video");
        assert!(!no_ext.ok);
        assert_eq!(no_ext.message, "This desktop environment does not need the shell extension");
        let mut sys = LiveSys::new();
        let empty = sys.spawn(&[], false).err().expect("пустая команда — отказ");
        for text in [live_formats_not_carried(), no_ext.message, empty] {
            assert!(!cyrillic(&text), "русское в английском интерфейсе: {text}");
        }
    }

    #[test]
    fn animated_refusal_on_gnome_touches_nothing() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let (mut e, handle) = exec_with_live_process();
        e.de = "gnome".to_string();
        let ran_before = e.sys_mut().ran.len();
        let spawned_before = e.sys_mut().spawned.len();
        let res = e.apply("/tmp/котик.gif", "animated", "");
        assert!(!res.ok);
        assert!(res.message.contains("there's nothing to show animated wallpapers with"), "{}", res.message);
        assert!(!e.sys_mut().killed.contains(&handle), "отказ снял живые обои");
        assert_eq!(e.sys_mut().ran.len(), ran_before, "отказ что-то запустил");
        assert_eq!(e.sys_mut().spawned.len(), spawned_before, "отказ что-то поднял");
    }

    /// Applying a static wallpaper remembers what was set before it — otherwise
    /// there would be nothing to restore after a stop.
    #[test]
    fn applying_remembers_what_stood_before() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///tmp/старые.png'\n");
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/tmp/новые.png", "static", "Закат");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/tmp/старые.png"));
        assert_eq!(e.current_path.as_deref(), Some("/tmp/новые.png"));
    }

    /// A foreign shell holds the wallpaper layer: no apply entry puts the
    /// wallpaper under its surface or reports success. The refusal comes before
    /// doing anything: the previous live wallpaper is not stopped, the desktop is
    /// not remembered, nothing is launched, and the text names the shell and the button.
    #[test]
    fn a_shell_owning_the_layer_refuses_every_apply_before_any_work() {
        for fmt in ["static", "video", "animated", "web", "swf"] {
            let mut sys = FakeSys::default();
            sys.shell_owner = Some(DesktopShell::Noctalia);
            sys.host = Some("/usr/bin/rexon-wallpaper-host".to_string());
            sys.tools.push("swww".to_string());
            let mut e = exec("niri", "wayland", sys);
            e.process = Some(77);
            let res = e.apply("/tmp/обои", fmt, "");
            assert!(!res.ok, "{fmt}: обои легли под чужой шелл с зелёным рапортом");
            assert!(res.message.contains("Noctalia"), "{fmt}: {}", res.message);
            assert!(res.message.contains("Give Wallpaper to Rexon"), "{fmt}: {}", res.message);
            assert!(e.sys_mut().spawned.is_empty(), "{fmt}: отказ что-то поднял");
            assert!(e.sys_mut().ran.is_empty(), "{fmt}: отказ что-то запустил");
            assert!(e.sys_mut().killed.is_empty(), "{fmt}: отказ снял прежние обои");
            assert!(e.saved_wallpaper.is_none(), "{fmt}: отказ запомнил стол");
        }
        let mut sys = FakeSys::default();
        sys.shell_owner = Some(DesktopShell::DankMaterialShell);
        sys.tools.push("yt-dlp".to_string());
        let mut e = exec("hyprland", "wayland", sys);
        let res = e.apply_youtube("https://www.youtube.com/watch?v=abc", "1080p", "stream", true);
        assert!(!res.ok);
        assert!(res.message.contains("DankMaterialShell"), "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty() && e.sys_mut().ran.is_empty());
    }

    /// Without a foreign shell the apply entry works as before (the bite of the
    /// check above: the same desktop, the same file — and success).
    #[test]
    fn without_a_shell_the_same_apply_goes_through() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///tmp/старые.png'\n");
        let mut e = exec("gnome", "wayland", sys);
        assert!(e.apply("/tmp/новые.png", "static", "").ok);
    }

    /// D12: LXQt. `pcmanfm-qt --set-wallpaper` always answers zero; «✅» only if
    /// the wallpaper path really landed in its settings.
    #[test]
    fn lxqt_reports_success_only_when_pcmanfm_saved_the_wallpaper() {
        let conf = "/home/u/.config/pcmanfm-qt/lxqt/settings.conf";
        let run = |saved: Option<&str>| {
            let mut sys = FakeSys::default();
            sys.tools.push("pcmanfm-qt".to_string());
            // This desktop's fallback paths "do not work": otherwise the fake seam would
            // report success for them, and it would not be pcmanfm-qt being checked.
            for other in ["gsettings", "feh", "xwallpaper", "nitrogen", "hsetroot", "xsetroot", "swaybg", "swww"] {
                sys.missing.push(other.to_string());
            }
            sys.envs.insert("HOME".to_string(), "/home/u".to_string());
            if let Some(text) = saved {
                sys.files.insert(conf.to_string(), text.to_string());
            }
            let mut e = exec("lxqt", "x11", sys);
            e.apply("/tmp/новые.png", "static", "")
        };
        let ok = run(Some("[Desktop]\nWallpaperMode=fit\nWallpaper=/tmp/новые.png\n"));
        assert!(ok.ok, "{}", ok.message);
        // A foreign path written, no key, no file — no success.
        for saved in [
            Some("[Desktop]\nWallpaper=/usr/share/lxqt/wallpapers/старые.png\n"),
            Some("[Desktop]\nWallpaperMode=fit\n"),
            Some("[Other]\nWallpaper=/tmp/новые.png\n"),
            None,
        ] {
            let res = run(saved);
            assert!(!res.ok, "ложный успех при {saved:?}: {}", res.message);
        }
    }

    /// The state snapshot does not pass off a dead process as a live wallpaper.
    #[test]
    fn the_status_does_not_call_a_dead_process_alive() {
        let mut sys = FakeSys::default();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("generic", "x11", sys);
        let proc = e.sys_mut().spawn(&["mpv".to_string()], true).unwrap();
        e.process = Some(proc);
        e.current_format = Some("video".to_string());
        let st = e.status();
        assert!(!st.running, "мёртвый процесс объявлен живыми обоями");
        assert!(st.pid.is_none(), "у мёртвого процесса назван PID");
        assert_eq!(st.de, "generic");
    }

    /// Restoring the desktop after a stop takes a clean picture, not our own
    /// blurred frame from the cache: otherwise «Stop» would leave the person with
    /// a blur instead of a wallpaper.
    #[test]
    fn the_clean_image_is_never_our_blur_frame() {
        let mut sys = FakeSys::default();
        sys.dirs.push("/usr/share/wallpapers/Next".to_string());
        let mut e = exec("kde", "wayland", sys);
        e.saved_wallpaper = Some("/home/w/.cache/rexon/blur/кадр.jpg".to_string());
        let clean = e.kde_clean_static_image();
        assert!(!clean.contains(".cache/rexon"), "возвращён наш же кадр: {clean}");
        assert_eq!(clean, "/usr/share/wallpapers/Next/");
    }

    /// Neither a saved wallpaper nor distribution directories — any installed
    /// Plasma wallpaper package is taken, but only a REAL one (with pictures).
    #[test]
    fn the_last_resort_package_must_actually_have_images() {
        let mut sys = FakeSys::default();
        sys.listing.insert(
            wpe::PLASMA_WALLPAPER_ROOT.to_string(),
            vec!["Пустой".to_string(), "Сгорание".to_string()],
        );
        sys.dirs
            .push("/usr/share/wallpapers/Сгорание/contents/images".to_string());
        let mut e = exec("kde", "wayland", sys);
        assert_eq!(e.kde_clean_static_image(), "/usr/share/wallpapers/Сгорание/");
    }


    // ── background sounds ─────────────────────────────────────────────────

    fn with_mpv() -> FakeSys {
        let mut sys = FakeSys::default();
        sys.tools.push("mpv".to_string());
        sys
    }

    /// A looped sound that died within the grace period is a failure: it does not exit by itself.
    #[test]
    fn a_looped_sound_that_dies_is_a_failure() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.active_ambient().is_empty());
        assert_eq!(e.sys_mut().killed.len(), 1, "труп mpv не подобран");
    }

    /// ⭐ But a ONE-SHOT sound that honestly finished within the grace period is a
    /// success. Measuring it by the wallpaper rule ("alive after half a second")
    /// would mean refusing a working short sound.
    #[test]
    fn a_one_shot_sound_that_finished_is_a_success() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("щелчок", "/tmp/щелчок.wav", "Щелчок", 50, false));
    }

    /// But a one-shot sound that failed with a non-zero code (a broken file) does
    /// not count as success.
    #[test]
    fn a_one_shot_sound_that_broke_is_still_a_failure() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![Some(2)];
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.play_ambient("битый", "/tmp/битый.wav", "", 50, false));
    }

    /// No mpv — a refusal, not a quiet pretense that the sound started.
    #[test]
    fn without_mpv_the_sound_is_refused() {
        let sys = FakeSys::default(); // mpv is not in the list
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.sys_mut().spawned.is_empty(), "mpv запускался без mpv");
    }

    /// Starting the same sound again stops the previous instance: otherwise two
    /// mpv processes would play the same thing on top of each other.
    #[test]
    fn replaying_a_sound_stops_the_previous_copy() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None, None];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert_eq!(e.sys_mut().killed.len(), 1, "прежний экземпляр остался играть");
        assert_eq!(e.active_ambient().len(), 1);
    }

    /// ⭐ A volume change restarts mpv in THE SAME mode: previously a one-shot
    /// sound looped forever after a volume tweak.
    #[test]
    fn changing_the_volume_keeps_the_playback_mode() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None, None];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("щелчок", "/tmp/щелчок.wav", "Щелчок", 30, false));
        assert!(e.set_ambient_volume("щелчок", 80));
        let spawned = e.sys_mut().spawned.clone();
        assert_eq!(spawned.len(), 2);
        assert!(
            !spawned[1].0.iter().any(|a| a == "--loop=inf"),
            "разовый звук зациклился при смене громкости: {:?}",
            spawned[1].0
        );
        assert!(
            spawned[1].0.iter().any(|a| a == "--volume=80"),
            "громкость не сменилась: {:?}",
            spawned[1].0
        );
    }

    /// Stopping the wallpaper also stops background sounds: otherwise they would
    /// play on a bare desktop.
    #[test]
    fn stopping_the_wallpaper_also_stops_the_sounds() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None, None];
        let mut e = exec("generic", "x11", sys);
        assert!(e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.play_ambient("ветер", "/tmp/ветер.ogg", "Ветер", 40, true));
        e.stop(true);
        assert!(e.active_ambient().is_empty(), "звуки пережили остановку обоев");
        assert_eq!(e.sys_mut().killed.len(), 2);
    }

    /// A finished sound leaves the list by itself, and a name without a label is
    /// taken from the file name — nothing to make up on the extension's behalf.
    #[test]
    fn a_finished_sound_leaves_the_list_by_itself() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("дождь", "/tmp/звуки/дождь.ogg", "", 50, true));
        let active = e.active_ambient();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "дождь.ogg");
        assert_eq!(active[0].volume, 50);
        // …the process has ended.
        e.sys_mut().spawn_rc = vec![Some(0)];
        assert!(e.active_ambient().is_empty(), "доигравший звук остался в списке");
        assert!(!e.ambient_playing("дождь"));
    }


    // ── video from a file ────────────────────────────────────────────────

    const HOST: &str = "/opt/rexon/rexon-wallpaper-host";

    /// `gdbus` answer to `GetExtensionInfo` for a running extension — a
    /// dictionary like gnome-shell's (`serializeExtension`: numbers packed as `d`).
    const GDBUS_EXT_ACTIVE: &str = "({'uuid': <'rexon-wallpaper@rexon.app'>, \
        'name': <'Rexon Wallpaper Engine'>, 'version': <1.0>, 'type': <2.0>, \
        'state': <1.0>, 'error': <''>, 'hasPrefs': <false>},)\n";

    /// Video in our own window: the host is started in its own session, the
    /// wallpaper is live, it needs load supervision.
    #[test]
    fn video_file_raises_our_host() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, own)| argv[..4] == [HOST, "--mode", "video", "/home/u/волны.mp4"] && *own),
            "хост видео не поднят своим сеансом: {spawned:?}"
        );
        let st = e.status();
        assert!(st.running);
        assert_eq!(st.format.as_deref(), Some("video"));
        assert!(format_wants_load_monitor(e.current_format()));
        assert!(e.monitor_target().is_some(), "надзору не за кем следить");
    }

    /// MATE on X11, Caja without the transparent background key: the video got
    /// applied, and the person is told the icons are under the wallpaper; the
    /// icons themselves are not turned off. «Stop» without a recorded desktop
    /// state rolls nothing back.
    #[test]
    fn video_on_mate_x11_says_icons_are_hidden_and_keeps_them_on() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        // Any gsettings answers «true»: icons are on, and Caja's key list has no
        // `transparent-background`.
        sys.answer("gsettings", 0, "true\n");
        let mut e = exec("mate", "x11", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        let hidden = crate::wallpaper_backend::desktop_icons_hidden_text(crate::wallpaper_backend::DesktopIconsOwner::Caja);
        assert!(res.message.ends_with(&hidden), "{}", res.message);
        let ran = lines(&mut e);
        assert!(!ran.iter().any(|l| l.contains("show-desktop-icons false")), "значки выключены: {ran:?}");

        // A picture creates no window — not a word about icons.
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "true\n");
        let mut e = exec("mate", "x11", sys);
        let res = e.apply("/home/u/лес.png", "static", "");
        assert!(!res.message.contains(&hidden), "{}", res.message);

        // «Stop» with a recorded previous state — the Caja key goes back.
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gsettings", 0, "true\n");
        sys.envs.insert("HOME".into(), "/home/u".into());
        let state = crate::wallpaper_backend::desktop_icons_state_path("", "/home/u");
        let saved = crate::wallpaper_backend::DesktopIconsSaved {
            caja_recorded: true,
            caja_transparent_was_off: true,
            ..Default::default()
        };
        sys.files.insert(state.clone(), saved.to_text());
        let mut e = exec("mate", "x11", sys);
        e.stop(true);
        let ran = lines(&mut e);
        assert!(
            ran.iter().any(|l| l.contains("org.mate.caja.desktop transparent-background false")),
            "{ran:?}"
        );
        assert!(e.sys_mut().removed.contains(&state));
    }

    /// No host — an honest refusal before launching; not a video file — a refusal with a reason.
    #[test]
    fn video_file_refuses_honestly() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(!res.ok);
        assert_eq!(res.message, wpe::video_host_missing_message());
        assert!(e.sys_mut().spawned.is_empty());

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/home/u/заметки.txt", "video", "");
        assert!(!res.ok);
        assert!(res.message.contains(".txt"), "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty());
    }

    /// GNOME Wayland: the video goes to the shell extension — no process of our
    /// own, the host command line is written; not installed — a refusal.
    #[test]
    fn video_on_gnome_goes_to_the_shell_extension() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        let ran = lines(&mut e);
        assert!(ran.iter().any(|l| l.contains("source /home/u/волны.mp4")), "{ran:?}");
        assert!(ran.iter().any(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY) && l.contains(HOST)));
        assert!(e.sys_mut().spawned.is_empty(), "на GNOME поднят свой процесс");
        assert_eq!(e.current_format(), Some("video"));

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gsettings", 1, "");
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(!res.ok);
        assert!(res.message.contains("shell extension"), "{}", res.message);
    }

    /// The extension's schema lies only in its own directory (it is not in the
    /// shared schema directory) — keys are written with `--schemadir` of that
    /// directory, a personal copy in the home directory overrides the packaged
    /// one; no directory — bare `gsettings`.
    #[test]
    fn shell_extension_keys_are_written_through_its_own_schema_dir() {
        let user = format!("/home/u/.local/share/gnome-shell/extensions/{}/schemas", wpe::SHELL_EXT_ID);
        let pkg = format!("/usr/share/gnome-shell/extensions/{}/schemas", wpe::SHELL_EXT_ID);
        let local = format!("/usr/local/share/gnome-shell/extensions/{}/schemas", wpe::SHELL_EXT_ID);
        let written = |missing: &[&String]| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
            sys.envs.insert("HOME".to_string(), "/home/u".to_string());
            sys.missing_paths = missing.iter().map(|d| format!("{d}/gschemas.compiled")).collect();
            let mut e = exec("gnome", "wayland", sys);
            let res = e.apply("/home/u/волны.mp4", "video", "");
            assert!(res.ok, "{}", res.message);
            e.stop(false);
            e.sys_mut()
                .ran
                .iter()
                .filter(|a| a.first().map(String::as_str) == Some("gsettings"))
                .filter(|a| a.iter().any(|w| w.contains("rexon-wallpaper")))
                .cloned()
                .collect::<Vec<_>>()
        };
        let all = written(&[]);
        assert_eq!(all.len(), 4, "{all:?}");
        assert!(all.iter().all(|a| a[1] == "--schemadir" && a[2] == user), "{all:?}");
        let pkg_only = written(&[&user, &local]);
        assert!(pkg_only.iter().all(|a| a[1] == "--schemadir" && a[2] == pkg), "{pkg_only:?}");
        let none = written(&[&user, &local, &pkg]);
        assert!(none.iter().all(|a| a[1] == "set"), "{none:?}");
    }

    /// «Done» on GNOME is said only when the shell itself calls the extension
    /// running (`GetExtensionInfo`, state 1). Previously we looked at
    /// `gnome-extensions list --enabled`, and with no answer assumed "enabled" —
    /// and reported success on an empty desktop.
    #[test]
    fn gnome_reports_ready_only_when_the_shell_runs_the_extension() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let ok_message = wpe::shell_ext_plan("gnome", "/home/u/волны.mp4", HOST, true)
            .expect("план GNOME")
            .ok_message;
        let apply = |answer: Option<(i64, &str)>| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            match answer {
                Some((rc, out)) => sys.answer("gdbus", rc, out),
                None => sys.missing.push("gdbus".to_string()),
            }
            let mut e = exec("gnome", "wayland", sys);
            let res = e.apply("/home/u/волны.mp4", "video", "");
            let asked = lines(&mut e)
                .iter()
                .any(|l| l.contains("org.gnome.Shell.Extensions.GetExtensionInfo"));
            assert!(asked, "оболочку о расширении не спросили");
            res
        };
        let res = apply(Some((0, GDBUS_EXT_ACTIVE)));
        assert!(res.ok && res.message == ok_message, "{}", res.message);

        // An error in the extension and "wrong version" — there will be no wallpaper: a refusal.
        for state in ["3.0", "4.0"] {
            let out = GDBUS_EXT_ACTIVE.replace("'state': <1.0>", &format!("'state': <{state}>"));
            let res = apply(Some((0, &out)));
            assert!(!res.ok, "state {state}: {}", res.message);
            assert_ne!(res.message, ok_message);
        }
        // Disabled: Rexon enables the extension itself; the shell still says
        // "disabled" — there is no wallpaper on the desktop, this is a refusal, not a success.
        let disabled = GDBUS_EXT_ACTIVE.replace("'state': <1.0>", "'state': <2.0>");
        let res = apply(Some((0, disabled.as_str())));
        assert!(!res.ok, "выключено: {}", res.message);
        // Not loaded (an empty dictionary), the shell did not answer or there is no
        // gdbus — the wallpaper is handed over, but «done» is not said.
        for answer in [
            Some((0, "(@a{sv} {},)\n")),
            Some((1, "")),
            None,
        ] {
            let res = apply(answer);
            assert!(res.ok, "{answer:?}: {}", res.message);
            assert_ne!(res.message, ok_message, "{answer:?}");
        }
    }

    /// Flash in our own window: the `--mode swf` host in its own session, no
    /// external `ruffle`, the wallpaper under load supervision — the same way as video.
    #[test]
    fn swf_file_raises_our_host_under_the_load_monitor() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("kde", "wayland", sys);
        let res = e.apply("/home/u/ролик.swf", "swf", "");
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, own)| argv[..] == [HOST, "--mode", "swf", "/home/u/ролик.swf"] && *own),
            "хост Flash не поднят своим сеансом: {spawned:?}"
        );
        assert!(
            !spawned.iter().any(|(argv, _)| argv.iter().any(|a| a.contains("ruffle"))),
            "запущен внешний ruffle: {spawned:?}"
        );
        assert_eq!(e.current_format(), Some("swf"));
        assert!(format_wants_load_monitor(e.current_format()));
        assert!(e.monitor_target().is_some(), "надзору не за кем следить");
    }

    /// Flash: no host or no file — a refusal BEFORE stopping the previous wallpaper.
    #[test]
    fn swf_refuses_before_touching_live_wallpaper() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply("/home/u/ролик.swf", "swf", "");
        assert_eq!(res.message, wpe::video_host_missing_message());
        assert!(e.sys_mut().spawned.is_empty());
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.missing_paths.push("/home/u/нет.swf".to_string());
        sys.dirs.push("/home/u/каталог.swf".to_string());
        let mut e = exec("sway", "wayland", sys);
        for p in ["/home/u/нет.swf", "/home/u/каталог.swf"] {
            let res = e.apply(p, "swf", "");
            assert!(!res.ok);
            assert_eq!(res.message, wpe::swf_missing_message(p));
        }
        assert!(e.sys_mut().spawned.is_empty());
        assert!(e.sys_mut().ran.is_empty(), "отказ что-то запустил");
    }

    /// Flash on GNOME — the shell extension gets the `--mode swf` host command line.
    #[test]
    fn swf_on_gnome_goes_to_the_shell_extension() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/home/u/ролик.swf", "swf", "");
        assert!(res.ok, "{}", res.message);
        let ran = lines(&mut e);
        assert!(ran.iter().any(|l| l.contains("engine swf")), "{ran:?}");
        assert!(
            ran.iter().any(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY) && l.contains("\"swf\"")),
            "{ran:?}"
        );
        assert!(e.sys_mut().spawned.is_empty(), "на GNOME поднят свой процесс");
        assert_eq!(e.current_format(), Some("swf"));
    }

    /// Backdrop on X11: FIRST the video (host started, result reported), no movie
    /// frame extracted yet; the backdrop comes next, as a sharp frame from the
    /// frame cache, without blur, and the shown format stays "video".
    #[test]
    fn video_starts_before_its_backdrop() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.frame_ok = true;
        sys.envs.insert("XDG_CACHE_HOME".to_string(), "/home/u/.cache".to_string());
        let mut e = exec("generic", "x11", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1, "хост не поднят");
        assert!(e.sys_mut().frames.is_empty(), "кадр доставали до показа");
        assert!(!e.status().blur_active);
        let shown_at = e.sys_mut().spawn_marks[0];

        assert!(e.run_pending_backdrop(), "подложка не встала");
        let frames = e.sys_mut().frames.clone();
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert_eq!(frames[0].0, "/home/u/волны.mp4");
        assert!(frames[0].1.starts_with("/home/u/.cache/rexon/video-stills/") && frames[0].1.ends_with(".jpg"), "{frames:?}");
        assert!(e.sys_mut().blurred.is_empty(), "кадр размыт");
        assert_eq!(
            e.sys_mut().pruned,
            vec![("/home/u/.cache/rexon/video-stills".to_string(), frames[0].1.clone())],
            "кеш кадров не ужат после нового кадра"
        );
        let ran = lines(&mut e);
        let set = ran.iter().position(|l| l.contains("gsettings set") && l.contains(&frames[0].1)).expect("подложка не поставлена");
        assert!(set >= shown_at, "подложка раньше видео: {ran:?}");
        assert!(e.status().blur_active);
        assert_eq!(e.current_format(), Some("video"), "подложка перебила формат видео");
        let bridge = e.sys_mut().bridge.clone();
        assert!(bridge.last().is_some_and(|(live, _, _)| *live), "мосту сказано «не живые»: {bridge:?}");
        assert!(!e.run_pending_backdrop(), "подложка поставлена дважды");
    }

    /// GNOME (shell extension): the host command line is written BEFORE the
    /// backdrop; the desktop picture comes after, quietly.
    #[test]
    fn gnome_extension_backdrop_goes_after_the_video() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.frame_ok = true;
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        let before = lines(&mut e);
        assert!(!before.iter().any(|l| l.contains("/rexon/video-stills/")), "подложка до показа: {before:?}");
        assert!(e.run_pending_backdrop());
        let ran = lines(&mut e);
        let daemon = ran.iter().position(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY)).expect("хост не передан");
        let backdrop = ran
            .iter()
            .position(|l| l.contains("org.gnome.desktop.background") && l.contains("/rexon/video-stills/"))
            .expect("подложки нет");
        assert!(daemon < backdrop, "{ran:?}");
        assert_eq!(e.current_format(), Some("video"));
    }

    /// wlroots (sway): the desktop picture is held by our own background-layer
    /// surface — the backdrop is set BEFORE showing, otherwise it would land over the video.
    #[test]
    fn wlroots_backdrop_stays_before_the_video() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.frame_ok = true;
        sys.tools.push("swaymsg".to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        let ran = lines(&mut e);
        let bg = ran.iter().position(|l| l.starts_with("swaymsg output") && l.contains("/rexon/video-stills/")).expect("подложки нет");
        let shown_at = e.sys_mut().spawn_marks[0];
        assert!(bg < shown_at, "подложка после видео: {ran:?}");
        assert!(!e.run_pending_backdrop(), "на wlroots отложенной подложки быть не должно");
        assert_eq!(e.current_format(), Some("video"));
    }

    /// The frame is already in the cache — ffmpeg is not called; the wallpaper
    /// changed or was stopped before the backdrop was ready — it is not set at
    /// all; no frame — the video runs without a backdrop, the apply result is unchanged.
    #[test]
    fn backdrop_uses_cache_and_drops_when_stale() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.stills_cached = true;
        let mut e = exec("generic", "x11", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        assert!(e.run_pending_backdrop());
        assert!(e.sys_mut().frames.is_empty(), "кадр из кеша доставали заново");
        let touched = e.sys_mut().touched.clone();
        assert!(touched.len() == 1 && touched[0].contains("/rexon/video-stills/"), "кадр из кеша не освежён: {touched:?}");
        assert!(e.sys_mut().pruned.is_empty(), "кеш чистили без нового кадра");

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.frame_ok = true;
        let mut e = exec("generic", "x11", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        let job = e.take_pending_backdrop().expect("подложка не отложена");
        e.stop(true);
        let ran_before = e.sys_mut().ran.len();
        assert!(!e.finish_backdrop(&job, Some("/home/u/.cache/rexon/video-stills/x.jpg".to_string())));
        assert_eq!(e.sys_mut().ran.len(), ran_before, "подложка после остановки");

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("generic", "x11", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        let job = e.take_pending_backdrop().expect("подложка не отложена");
        assert!(e.apply("/home/u/лес.mp4", "video", "").ok);
        assert!(!e.finish_backdrop(&job, Some("/home/u/.cache/rexon/video-stills/x.jpg".to_string())), "подложка прежних обоев");
        assert!(!e.run_pending_backdrop(), "кадра нет, а подложка «встала»");
        assert_eq!(e.current_format(), Some("video"));
    }

    /// Cinnamon 6.8 with layer-shell: the backdrop is a SHARP movie frame, AFTER
    /// showing (before it the background service's surface lies over ours). The
    /// picture goes into the Cinnamon background schema: the lock screen takes it
    /// from there too, and without a backdrop it showed a stale picture (16.11.2026).
    #[test]
    fn cinnamon_layer_shell_video_sets_a_sharp_backdrop_after_show() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.frame_ok = true;
        sys.cinnamon_layer = true;
        let mut e = exec("cinnamon-mint", "wayland", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        let before = lines(&mut e);
        assert!(!before.iter().any(|l| l.contains("/rexon/video-stills/")), "подложка до показа: {before:?}");
        assert!(e.run_pending_backdrop(), "подложка не отложена на после показа");
        let ran = lines(&mut e);
        assert!(
            ran.iter().any(|l| l.contains(wpe::CINNAMON_BG_SCHEMA) && l.contains("/rexon/video-stills/")),
            "кадр не ушёл в схему фона Cinnamon: {ran:?}"
        );
        assert!(e.sys_mut().blurred.is_empty(), "подложка размыта — нужен чёткий кадр");
        assert_eq!(e.current_format(), Some("video"));
    }

    /// Video sound is chosen by the caller: `false` — the host is started muted,
    /// `true` and the `apply` default — with sound. One path for all calls.
    #[test]
    fn video_audio_choice_reaches_our_host() {
        let host_argv = |audio: Option<bool>| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            let mut e = exec("sway", "wayland", sys);
            let res = match audio {
                Some(a) => e.apply_with_audio("/home/u/волны.mp4", "video", "Волны", a),
                None => e.apply("/home/u/волны.mp4", "video", "Волны"),
            };
            assert!(res.ok, "{}", res.message);
            let spawned = e.sys_mut().spawned.clone();
            spawned
                .into_iter()
                .map(|(argv, _)| argv)
                .find(|argv| argv[0] == HOST)
                .expect("хост видео не поднят")
        };
        assert!(host_argv(Some(false)).iter().any(|a| a == "--no-audio"));
        assert!(!host_argv(Some(true)).iter().any(|a| a == "--no-audio"));
        assert_eq!(VIDEO_AUDIO_DEFAULT, true);
        assert!(!host_argv(None).iter().any(|a| a == "--no-audio"), "умолчание — со звуком");
    }

    /// GNOME Wayland: a muted video — the host command line at the shell
    /// extension carries `--no-audio`, with sound it does not.
    #[test]
    fn video_audio_choice_reaches_the_shell_extension() {
        let daemon_line = |audio: bool| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
            let mut e = exec("gnome", "wayland", sys);
            let res = e.apply_with_audio("/home/u/волны.mp4", "video", "", audio);
            assert!(res.ok, "{}", res.message);
            lines(&mut e)
                .into_iter()
                .find(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY))
                .expect("строка запуска хоста не записана")
        };
        assert!(daemon_line(false).contains("--no-audio"));
        assert!(!daemon_line(true).contains("--no-audio"));
    }

    /// The YouTube format through the shared entry: sound reaches the stream player.
    #[test]
    fn youtube_through_apply_keeps_the_audio_choice() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string(), "mpvpaper".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_with_audio("https://youtube.com/watch?v=abc", "youtube", "", false);
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        let argv = &spawned.last().expect("ничего не поднято").0;
        assert!(argv.iter().any(|a| a.contains("no-audio")), "{argv:?}");
    }

    // ── process executor slot ────────────────────────────────────────────

    /// The process slot is one: every call returns the same one, and it does not
    /// create an executor itself (the test does not touch the live machine).
    #[test]
    fn live_slot_is_one_per_process() {
        let a = live_slot();
        assert!(std::sync::Arc::ptr_eq(&a, &live_slot()));
    }

    /// Two slot holders see one executor: what was set through the first is
    /// visible and can be stopped through the second; a busy lock gives `None`,
    /// not waiting.
    #[test]
    fn slot_holders_share_one_executor() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let slot: ExecSlot<FakeSys> =
            std::sync::Arc::new(Mutex::new(Some(exec("sway", "wayland", sys))));
        let other = slot.clone();

        let res = with_slot(&slot, |e| e.apply_with_audio("/home/u/волны.mp4", "video", "", false));
        assert!(res.ok, "{}", res.message);
        assert_eq!(try_with_slot(&other, |e| e.status().running), Some(true));

        {
            let _held = slot.lock().unwrap();
            assert!(try_with_slot(&other, |e| e.status().running).is_none(), "занятый замок ждали");
        }

        let stopped = try_with_slot(&other, |e| e.stop(true)).expect("замок свободен");
        assert!(stopped.ok, "{}", stopped.message);
        assert_eq!(with_slot(&slot, |e| e.status().running), false);
        assert!(!with_slot(&slot, |e| e.sys_mut().killed.is_empty()), "хост не снят");
    }

    /// The apply door: the work result is returned, no supervision is started for
    /// a static wallpaper (the core refuses by itself), the slot is free after the door.
    #[test]
    fn run_in_slot_returns_the_work_and_frees_the_slot() {
        let slot: ExecSlot<FakeSys> =
            std::sync::Arc::new(Mutex::new(Some(exec("gnome", "wayland", FakeSys::default()))));
        let res = run_in_slot(&slot, |e| e.apply("/home/u/картинка.png", "static", ""));
        assert!(res.ok, "{}", res.message);
        assert!(!format_wants_load_monitor(Some("static")));
        assert!(slot.try_lock().is_ok(), "дверь не отпустила замок");
    }

    // ── YouTube ──────────────────────────────────────────────────────────

    /// A stream under Wayland: mpvpaper with a format by quality and the link,
    /// sound and volume as switches; the wallpaper is live, it needs supervision.
    #[test]
    fn youtube_stream_raises_mpvpaper() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string(), "mpvpaper".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "720p", "stream", false);
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        let argv = &spawned.last().expect("ничего не поднято").0;
        assert_eq!(argv[0], "mpvpaper");
        assert!(argv.iter().any(|a| a.contains("ytdl-format=") && a.contains("no-audio")), "{argv:?}");
        assert_eq!(argv.last().unwrap(), "https://youtube.com/watch?v=abc");
        assert_eq!(e.current_format(), Some("youtube"));
        assert!(format_wants_load_monitor(e.current_format()));
    }

    /// GNOME Wayland: a stream cannot be carried out — the video is downloaded and
    /// set as a video, and the person is told why it was downloaded.
    #[test]
    fn youtube_on_gnome_downloads_and_says_why() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string()];
        sys.host = Some(HOST.to_string());
        sys.yt_download = Some("/home/u/.cache/rexon/youtube/abc.mp4".to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "1080p", "stream", true);
        assert!(res.ok, "{}", res.message);
        let note = wpe::youtube_mode_downgraded_note("gnome", "wayland", "stream");
        assert!(!note.is_empty() && res.message.contains(&note), "{}", res.message);
        assert_eq!(e.current_format(), Some("video"));
    }

    /// Refusals: no yt-dlp, a foreign link, the download failed.
    #[test]
    fn youtube_refuses_honestly() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "1080p", "stream", true);
        assert!(!res.ok);
        assert!(res.message.contains("yt-dlp"), "{}", res.message);

        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_youtube("ftp://example.com/x", "1080p", "stream", true);
        assert!(!res.ok);
        assert_eq!(res.message, wpe::youtube_unsupported_message("ftp://example.com/x"));

        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "1080p", "download", true);
        assert!(!res.ok);
        assert_eq!(res.message, wpe::youtube_download_failed_message("sway", "wayland"));
        assert!(e.sys_mut().spawned.is_empty());
    }

    // ── load supervision ─────────────────────────────────────────────────

    /// An executor with a live wallpaper process (the web host on sway) —
    /// supervision has someone to watch.
    /// Web wallpaper as a layer on Wayland — the executor calls the background
    /// restack (the `own_layer_surface_up` seam); a picture and X11 do not: the
    /// shell draws the picture itself, X11 has no layer.
    #[test]
    fn own_layer_surface_reported_only_for_live_layer_formats() {
        let (e, _) = exec_with_live_process();
        let mut e = e;
        // Web, video and Flash are shown by our host: it reports showing its surface
        // itself, on the first frame — the executor does not duplicate that.
        assert_eq!(e.sys_mut().layer_ups, 0, "веб на sway — перестановку заказывает сам хост");
        assert_eq!(e.sys_mut().apply_marks, 1, "начало применения отмечено для замера первого кадра");
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        let mut gif = exec("sway", "wayland", sys);
        assert!(gif.apply("/home/u/a.gif", "animated", "").ok);
        assert_eq!(gif.sys_mut().layer_ups, 1, "GIF через swww — чужая поверхность, заказ от исполнителя");
        assert_eq!(gif.sys_mut().apply_marks, 0, "GIF показывает не наш хост — мерить некому");
        assert!(own_layer_surface_format("video") && own_layer_surface_format("swf"));
        assert!(!own_layer_surface_format("static") && !own_layer_surface_format("scene"));
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut x11 = exec("xfce", "x11", sys);
        x11.apply("https://example.com", "web", "");
        assert_eq!(x11.sys_mut().layer_ups, 0, "X11 — окно, не слой");
    }

    fn exec_with_live_process() -> (WallpaperExec<FakeSys>, ProcHandle) {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply("https://example.com", "web", "").ok);
        let (handle, pid) = e.monitor_target().expect("есть за кем следить");
        assert!(pid > 0);
        (e, handle)
    }

    const CALM: LoadObservation =
        LoadObservation { battery_low: false, fullscreen: false, cpu_high: false };

    /// A low battery freezes the wallpaper, charging unfreezes it; a repeated
    /// sample with the same reason does not signal a second time.
    #[test]
    fn monitor_pauses_on_low_battery_and_resumes() {
        let (mut e, h) = exec_with_live_process();
        let mut strikes = 0;
        let low = LoadObservation { battery_low: true, ..CALM };
        assert_eq!(e.monitor_apply(h, &mut strikes, &low), MonitorVerdict::Continue);
        assert_eq!(e.monitor_apply(h, &mut strikes, &low), MonitorVerdict::Continue);
        assert_eq!(e.sys_mut().signals, vec![(h, true)]);
        assert_eq!(e.monitor_apply(h, &mut strikes, &CALM), MonitorVerdict::Continue);
        assert_eq!(e.sys_mut().signals, vec![(h, true), (h, false)]);
    }

    /// A stop first unfreezes a frozen process — otherwise SIGTERM would not
    /// reach it.
    #[test]
    fn stopping_a_paused_wallpaper_resumes_it_first() {
        let (mut e, h) = exec_with_live_process();
        let mut strikes = 0;
        let full = LoadObservation { fullscreen: true, ..CALM };
        e.monitor_apply(h, &mut strikes, &full);
        e.stop(true);
        assert_eq!(e.sys_mut().signals, vec![(h, true), (h, false)]);
        assert_eq!(e.sys_mut().killed, vec![h]);
    }

    /// CPU above the threshold — a stop on the N-th sample in a row, not
    /// earlier; a calm sample resets the count.
    #[test]
    fn monitor_stops_a_cpu_leak_after_the_strikes() {
        let (mut e, h) = exec_with_live_process();
        let mut strikes = 0;
        let hot = LoadObservation { cpu_high: true, ..CALM };
        for _ in 0..wpe::CPU_HIGH_STRIKES - 1 {
            assert_eq!(e.monitor_apply(h, &mut strikes, &hot), MonitorVerdict::Continue);
        }
        e.monitor_apply(h, &mut strikes, &CALM);
        assert_eq!(strikes, 0, "спокойный замер не обнулил счёт");
        for _ in 0..wpe::CPU_HIGH_STRIKES - 1 {
            e.monitor_apply(h, &mut strikes, &hot);
        }
        assert_eq!(e.monitor_apply(h, &mut strikes, &hot), MonitorVerdict::Done);
        assert!(!e.status().running, "протёкший процесс не остановлен");
        assert_eq!(e.sys_mut().killed, vec![h]);
    }

    /// A crashed process: supervision ends. Supervision leaves our own host to the
    /// crash watcher (the desktop is not restored — the watcher brings the same
    /// wallpaper back); a foreign process (a YouTube stream via mpv) — the desktop
    /// is restored, the bridge learns about the stop.
    #[test]
    fn monitor_restores_the_desktop_when_the_host_crashes() {
        let (mut e, h) = exec_with_live_process();
        e.sys_mut().spawn_rc = vec![Some(1)];
        let mut strikes = 0;
        assert_eq!(e.monitor_apply(h, &mut strikes, &CALM), MonitorVerdict::Done);
        assert_eq!(e.status().format.as_deref(), Some("web"), "надзор вернул стол вместо сторожа");
        assert!(e.host_snapshot().exited);

        // Not our own host — the former path: desktop back.
        e.host_request = None;
        assert_eq!(e.monitor_apply(h, &mut strikes, &CALM), MonitorVerdict::Done);
        let st = e.status();
        assert!(!st.running);
        assert_eq!(st.format, None);
        assert!(
            matches!(e.sys_mut().bridge.last(), Some((false, _, _))),
            "мост не узнал о падении: {:?}",
            e.sys_mut().bridge
        );
    }

    /// The wallpaper changed — the previous supervision ends by itself.
    #[test]
    fn monitor_of_replaced_wallpaper_ends() {
        let (mut e, old) = exec_with_live_process();
        assert!(e.apply("https://example.org", "web", "").ok);
        let mut strikes = 0;
        assert_eq!(e.monitor_apply(old, &mut strikes, &CALM), MonitorVerdict::Done);
    }

    /// Battery: the first `BAT*`, charge below the threshold and not charging.
    #[test]
    fn battery_observation_follows_the_power_supply() {
        let cfg = MonitorConfig { pause_on_fullscreen: false, battery_pause_pct: 20 };
        let setup = |status: &str| {
            let mut sys = FakeSys::default();
            sys.dirs.push(wpe::POWER_SUPPLY_DIR.to_string());
            sys.listing.insert(
                wpe::POWER_SUPPLY_DIR.to_string(),
                vec!["AC".to_string(), "BAT0".to_string()],
            );
            sys.files.insert(format!("{}/BAT0/capacity", wpe::POWER_SUPPLY_DIR), "15\n".into());
            sys.files.insert(format!("{}/BAT0/status", wpe::POWER_SUPPLY_DIR), status.into());
            sys
        };
        let mut sys = setup("Discharging\n");
        assert!(observe_load(&mut sys, "sway", "wayland", 42, &cfg).battery_low);
        let mut sys = setup("Charging\n");
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).battery_low);
        let mut sys = FakeSys::default();
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).battery_low, "нет батареи");
    }

    /// A full-screen window on X11: a window covering the whole screen — yes; the
    /// person's setting is off — do not ask at all; no xdotool — no.
    #[test]
    fn fullscreen_observation_obeys_the_setting() {
        let on = MonitorConfig { pause_on_fullscreen: true, battery_pause_pct: 20 };
        let off = MonitorConfig { pause_on_fullscreen: false, battery_pause_pct: 20 };
        let setup = || {
            let mut sys = FakeSys::default();
            sys.tools.push("xdotool".to_string());
            sys.answer("xdotool", 0, "WINDOW=1\nWIDTH=1920\nHEIGHT=1080\n");
            sys
        };
        let mut sys = setup();
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen);
        let mut sys = setup();
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &off).fullscreen);
        assert!(!sys.ran.iter().any(|a| a[0] == "xdotool"), "выключенная пауза спросила окно");
        let mut sys = FakeSys::default();
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "без xdotool");
    }

    /// A window covering the whole SMALLER of two monitors is full-screen too.
    ///
    /// The "first resolution" yardstick saw 1920x1080 and did not notice a window
    /// on the neighboring 1600x900: the wallpaper burned the CPU the whole game.
    #[test]
    fn fullscreen_is_measured_by_the_smaller_monitor() {
        let on = MonitorConfig { pause_on_fullscreen: true, battery_pause_pct: 20 };
        let mut sys = FakeSys::default();
        sys.tools.push("xdotool".to_string());
        sys.answer("xdotool", 0, "WINDOW=1\nWIDTH=1600\nHEIGHT=900\n");
        sys.answer("xrandr", 0, XRANDR_TWO);
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen);
    }

    /// Load supervision does not take the desktop window (and our own wallpaper)
    /// for a full-screen window over the wallpaper.
    ///
    /// ⭐ Measurement of 16.09.2026 on an X11 test bench with a window manager:
    /// `xdotool getactivewindow` named THE WALLPAPER ITSELF as the active window —
    /// a full-screen window with `_NET_WM_WINDOW_TYPE_DESKTOP`, and the wallpaper
    /// paused because of itself.
    #[test]
    fn the_desktop_window_is_not_a_fullscreen_app() {
        let on = MonitorConfig { pause_on_fullscreen: true, battery_pause_pct: 20 };
        let setup = |props: &str, tools: &[&str]| {
            let mut sys = FakeSys::default();
            for t in tools {
                sys.tools.push((*t).to_string());
            }
            sys.answer("xdotool", 0, "WINDOW=99\nWIDTH=1920\nHEIGHT=1080\n");
            sys.answer("xprop", 0, props);
            sys
        };
        let both = ["xdotool", "xprop"];

        let mut sys = setup("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DESKTOP\n", &both);
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "рабочий стол");

        let mut sys = setup("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DOCK\n", &both);
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "панель");

        // The type was not set, but the window belongs to the wallpaper process (PID=42).
        let mut sys = setup("_NET_WM_PID(CARDINAL) = 42\n", &both);
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "свои обои");

        // A foreign full-screen game still pauses the wallpaper.
        let mut sys = setup(
            "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_NORMAL\n\
             _NET_WM_PID(CARDINAL) = 7\n",
            &both,
        );
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "игра");

        // Foolproofing: there is no xprop on the system — the behavior is UNCHANGED,
        // and nobody tries to run it.
        let mut sys = setup("", &["xdotool"]);
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "без xprop");
        assert!(!sys.ran.iter().any(|a| a[0] == "xprop"), "xprop звали, хотя его нет");

        // xprop exists but stayed silent — also the unchanged behavior.
        let mut sys = setup("", &both);
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "xprop молчит");
    }

    /// CPU: above the threshold — yes, below or no answer — no.
    #[test]
    fn cpu_observation_reads_ps() {
        let cfg = MonitorConfig { pause_on_fullscreen: false, battery_pause_pct: 20 };
        let mut sys = FakeSys::default();
        sys.answer("ps", 0, " 95.5\n");
        assert!(observe_load(&mut sys, "sway", "wayland", 42, &cfg).cpu_high);
        let mut sys = FakeSys::default();
        sys.answer("ps", 0, " 3.0\n");
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).cpu_high);
        let mut sys = FakeSys::default();
        sys.missing.push("ps".to_string());
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).cpu_high);
    }

    /// ⭐ Pausing on a full-screen window listens to the same setting as the host.
    #[test]
    fn monitor_config_follows_the_hide_when_covered_setting() {
        let default = monitor_config_from(&serde_json::Map::new());
        assert!(default.pause_on_fullscreen, "умолчание продукта — пауза");
        assert_eq!(default.battery_pause_pct, wpe::BATTERY_PAUSE_PCT);
        let off = crate::wallpaper_audio::AudioSettings {
            actions: crate::wallpaper_audio::actions_for_hide_when_covered(false),
            ..Default::default()
        }
        .to_map();
        assert!(!monitor_config_from(&off).pause_on_fullscreen, "галочка снята, а пауза осталась");
    }

    /// Web wallpaper: the host is started in ITS OWN session with the engine's
    /// command line and counts as a live wallpaper — «Stop» will stop it.
    #[test]
    fn web_wallpaper_raises_our_host() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("https://example.com/обои", "web", "");
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert_eq!(
            spawned,
            vec![(
                vec![
                    HOST.to_string(),
                    "--mode".to_string(),
                    "web".to_string(),
                    "https://example.com/обои".to_string()
                ],
                true
            )]
        );
        let st = e.status();
        assert!(st.running, "веб-обои не считаются живыми");
        assert_eq!(st.format.as_deref(), Some("web"));
        e.stop(true);
        assert!(!e.sys_mut().killed.is_empty(), "остановка не сняла веб-хост");
    }

    /// No host — a refusal before launching; no HTML file — a refusal with its name.
    #[test]
    fn web_wallpaper_refuses_honestly() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply("https://example.com", "web", "");
        assert!(!res.ok);
        assert_eq!(res.message, wpe::web_host_missing());
        assert!(e.sys_mut().spawned.is_empty(), "без хоста что-то поднялось");

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.missing_paths.push("/home/u/нет.html".to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/home/u/нет.html", "web", "");
        assert!(!res.ok);
        assert!(res.message.contains("нет.html"), "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty());
        // The same missing file as a `file://` address — also a refusal.
        let res = e.apply("file:///home/u/нет.html", "web", "");
        assert!(!res.ok, "адрес отсутствующего файла прошёл");
    }

    /// ⭐ A local HTML file as a `file://` address (the way the window passes it)
    /// gets applied rather than answering «not found».
    #[test]
    fn local_web_page_as_file_url_is_applied() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("file:///home/u/страница.html", "web", "");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1);
    }

    /// Blur on GNOME: the copy is taken from the real wallpaper, written to the
    /// cache and set on the desktop; «Remove blur» restores the real wallpaper.
    #[test]
    fn blurring_uses_the_real_wallpaper_and_comes_back() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///usr/share/backgrounds/закат.png'\n");
        sys.envs.insert("XDG_CACHE_HOME".to_string(), "/home/u/.cache".to_string());
        let mut e = exec("gnome", "wayland", sys);
        assert!(e.blur_current_wallpaper(20));
        let blurred = e.sys_mut().blurred.clone();
        assert_eq!(
            blurred,
            vec![(
                "/usr/share/backgrounds/закат.png".to_string(),
                "/home/u/.cache/rexon/blur/закат_blur.jpg".to_string(),
                20
            )]
        );
        assert!(
            lines(&mut e).iter().any(|l| l.contains("gsettings set")
                && l.contains("/home/u/.cache/rexon/blur/закат_blur.jpg")),
            "размытая копия не поставлена: {:?}",
            lines(&mut e)
        );
        assert!(e.status().blur_active);
        assert!(e.unblur_wallpaper());
        assert!(
            lines(&mut e).iter().any(|l| l.contains("gsettings set")
                && l.contains("/usr/share/backgrounds/закат.png")),
            "настоящие обои не вернулись"
        );
        assert!(!e.status().blur_active);
        assert!(!e.unblur_wallpaper(), "второй раз возвращать уже нечего");
    }

    /// Blur does not report success when there is nothing to blur or it failed.
    #[test]
    fn blurring_reports_failures() {
        // There is no picture wallpaper (an environment without gsettings and not KDE).
        let mut e = exec("sway", "wayland", FakeSys::default());
        assert!(!e.blur_current_wallpaper(20));
        assert!(e.sys_mut().blurred.is_empty());
        // There is a picture, but it did not parse.
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'/tmp/обои.png'\n");
        sys.blur_fails = true;
        let mut e = exec("gnome", "wayland", sys);
        assert!(!e.blur_current_wallpaper(20));
        assert!(!e.status().blur_active);
    }


    // ── live run: real processes, someone else's desktop is left alone ────

    /// A fake plan of harmless commands: none has anything to do with wallpapers,
    /// but they go through THE SAME executor branches as a real one.
    fn harmless_step(kind: &str, backend: &str, argv: &[&str]) -> wpe::Step {
        wpe::Step {
            kind: kind.to_string(),
            backend: backend.to_string(),
            ensure_swww: false,
            pre: Vec::new(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            post: Vec::new(),
            ok_message: format!("{backend} отработал (PID {{pid}})"),
            err_message: format!("{backend} не смог: {{stderr}}"),
            alt_message: String::new(),
            format: "static".to_string(),
            terminal: false,
            ok_on_none: false,
            timeout: None,
            verify_ini: None,
            drops_holder: false,
        }
    }

    /// A live seam WITHOUT the only side effect that reaches outside the check —
    /// notices to the bridge.
    ///
    /// The bridge lives in `~/.config/rexon/bridge.json`, that is, in the real
    /// profile of whoever ran `cargo test`; a third-party application listens to
    /// it. Writing there from a check is not allowed, not even the truth — that
    /// is already the "check touches the live profile" divergence we got burned
    /// on with the bridge in September. Everything else — processes, files, PATH,
    /// clock — is real here, otherwise the run would not be live.
    struct LiveWithoutBridge {
        inner: LiveSys,
        said: Vec<(bool, String, bool)>,
        /// Fake `HOME` and `XDG_RUNTIME_DIR`: the PID file and configs must land in
        /// their own directory, not in the machine owner's profile.
        sandbox: String,
    }

    impl WpSys for LiveWithoutBridge {
        fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut> {
            self.inner.run(argv, timeout)
        }
        fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String> {
            self.inner.spawn(argv, new_session)
        }
        fn poll(&mut self, proc: ProcHandle) -> Option<i64> {
            self.inner.poll(proc)
        }
        fn pid(&mut self, proc: ProcHandle) -> Option<i64> {
            self.inner.pid(proc)
        }
        fn kill_process(&mut self, proc: ProcHandle) {
            self.inner.kill_process(proc)
        }
        fn has_tool(&mut self, tool: &str) -> bool {
            self.inner.has_tool(tool)
        }
        fn package_hint(&mut self, tool: &str) -> String {
            self.inner.package_hint(tool)
        }
        fn exists(&mut self, path: &str) -> bool {
            self.inner.exists(path)
        }
        fn is_dir(&mut self, path: &str) -> bool {
            self.inner.is_dir(path)
        }
        fn listdir(&mut self, path: &str) -> Vec<String> {
            self.inner.listdir(path)
        }
        fn read_text(&mut self, path: &str) -> Option<String> {
            self.inner.read_text(path)
        }
        fn write_text(&mut self, path: &str, content: &str) {
            self.inner.write_text(path, content)
        }
        fn remove_file(&mut self, path: &str) {
            self.inner.remove_file(path)
        }
        fn env(&mut self, name: &str) -> String {
            match name {
                "HOME" | "XDG_RUNTIME_DIR" => self.sandbox.clone(),
                _ => self.inner.env(name),
            }
        }
        fn monotonic(&mut self) -> f64 {
            self.inner.monotonic()
        }
        fn sleep(&mut self, seconds: f64) {
            self.inner.sleep(seconds)
        }
        fn bridge_notify(&mut self, live: bool, name: &str, announce: bool) {
            self.said.push((live, name.to_string(), announce));
        }
    }

    /// ⭐ A LIVE RUN of the executor with real processes.
    ///
    /// Everything the fake seam imitates is done for real here: the command is
    /// run, its exit code is read, the process is started and stopped with a
    /// signal. Three things are checked, the very ones the executor was written
    /// for — and each would be green while broken if measured only on the fake seam:
    ///
    ///   1. a failure of a NON-terminal step leads to the next one;
    ///   2. a process that died right after launch does NOT count as success;
    ///   3. a live process survives the grace period, gets into the state and is
    ///      stopped by its handle — not by name.
    ///
    /// ⛔ Not a single command here has anything to do with wallpapers: the
    /// desktop of whoever ran the check is not touched at all.
    #[test]
    #[cfg_attr(
        feature = "isolated-checks",
        ignore = "живой прогон: настоящие подпроцессы машины"
    )]
    fn the_live_executor_runs_real_processes() {
        let sandbox = std::env::temp_dir()
            .join(format!("rexon-wp-exec-{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let sys = LiveWithoutBridge {
            inner: LiveSys::new(),
            said: Vec::new(),
            sandbox,
        };
        let mut e = WallpaperExec::with_env(sys, "generic", "x11");

        // 1. A non-zero code leads to the next step.
        let plan = vec![
            harmless_step(wpe::step_kind::RUN, "false", &["false"]),
            harmless_step(wpe::step_kind::RUN, "true", &["true"]),
        ];
        let res = e.run_plan(&plan, "/tmp/подставной-путь");
        assert!(res.ok, "живой прогон не дошёл до запасного шага: {}", res.message);
        assert!(res.message.starts_with("true"), "исход отдал не тот шаг: {}", res.message);

        // 2. A process that dies instantly is not a success. `false` exits right
        //    away, that is, within the "is the launched process alive" grace period.
        let plan = vec![harmless_step(wpe::step_kind::SPAWN_SESSION, "false", &["false"])];
        let res = e.run_plan(&plan, "/tmp/подставной-путь");
        assert!(!res.ok, "мгновенно умерший процесс прошёл как успех: {}", res.message);

        // 3. A live process survives the grace period and gets into the state.
        let plan = vec![harmless_step(
            wpe::step_kind::SPAWN_SESSION,
            "sleep",
            &["sleep", "30"],
        )];
        let res = e.run_plan(&plan, "/tmp/подставной-путь");
        assert!(res.ok, "живой процесс не прошёл: {}", res.message);
        let pid = res.pid.expect("живой запуск не назвал PID");
        assert!(pid > 0);
        assert!(e.status().running, "живой процесс не считается работающим");

        // …and is stopped by ITS OWN handle. A stop on the `generic` environment
        // touches neither icons, nor plugins, nor wallpapers: it has nothing to
        // restore, because nothing was saved.
        let stopped = e.stop(true);
        assert!(stopped.ok, "{}", stopped.message);
        assert!(!e.status().running, "процесс пережил остановку");
        // The stopped process's PID no longer responds (signal 0 is only a probe).
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        assert!(!alive, "процесс PID={pid} пережил остановку");
    }


    // ── per-monitor wallpaper layout ──────────────────────────────────────

    /// The check's call journal — printed with `cargo test -- --nocapture`.
    /// This is exactly "which commands went out": the checks below compare the
    /// same journal line by line.
    fn journal(title: &str, e: &mut WallpaperExec<FakeSys>) {
        println!("── {title} ──");
        for cmd in e.sys_mut().ran.clone() {
            println!("  run   {}", cmd.join(" "));
        }
        for (cmd, _) in e.sys_mut().spawned.clone() {
            println!("  spawn {}", cmd.join(" "));
        }
    }

    /// Two outputs in a `wlr-randr --json` answer.
    const WLR_TWO: &str = r#"[
        {"name":"DP-1","enabled":true,"position":{"x":0,"y":0},
         "modes":[{"width":1280,"height":720,"current":false},
                  {"width":2560,"height":1440,"current":true}]},
        {"name":"HDMI-A-1","enabled":true,"position":{"x":2560,"y":0},
         "modes":[{"width":1920,"height":1080,"current":true}]},
        {"name":"DP-9","enabled":false,"modes":[]}
    ]"#;

    /// The same two outputs, but the query did not say where they stand.
    const WLR_TWO_NOWHERE: &str = r#"[
        {"name":"DP-1","enabled":true,
         "modes":[{"width":2560,"height":1440,"current":true}]},
        {"name":"HDMI-A-1","enabled":true,
         "modes":[{"width":1920,"height":1080,"current":true}]}
    ]"#;

    /// One output in a `wlr-randr --json` answer.
    const WLR_ONE: &str = r#"[{"name":"DP-1","enabled":true,
        "position":{"x":0,"y":0},
        "modes":[{"width":2560,"height":1440,"current":true}]}]"#;

    /// One output in an `xrandr --current` answer.
    const XRANDR_ONE: &str = "Screen 0: minimum 320 x 200, current 1920 x 1080\n\
        eDP-1 connected primary 1920x1080+0+0 (normal left inverted) 340mm x 190mm\n\
        \x20  1920x1080     60.00*+\n";

    /// Two outputs in an `xrandr --current` answer.
    const XRANDR_TWO: &str = "Screen 0: minimum 320 x 200, current 3840 x 1080\n\
        DP-1 connected primary 1920x1080+0+0 (normal left inverted) 530mm x 300mm\n\
        \x20  1920x1080     60.00*+  59.94\n\
        HDMI-1 connected 1600x900+1920+0 (normal left inverted) 480mm x 270mm\n\
        \x20  1600x900      60.00*+\n\
        DP-2 disconnected (normal left inverted right x axis y axis)\n";

    /// Parsing `wlr-randr --json`: name and size of the CURRENT mode, switched-off
    /// outputs are skipped.
    #[test]
    fn outputs_are_read_out_of_wlr_randr() {
        let got = parse_outputs_json(WLR_TWO);
        assert_eq!(
            got,
            vec![
                ScreenOutput {
                    name: "DP-1".into(),
                    resolution: "2560x1440".into(),
                    position: Some((0, 0)),
                },
                ScreenOutput {
                    name: "HDMI-A-1".into(),
                    resolution: "1920x1080".into(),
                    position: Some((2560, 0)),
                },
            ]
        );
    }

    /// Parsing `hyprctl monitors -j`: the size lies directly in the monitor, and a
    /// disabled one is marked `disabled`.
    #[test]
    fn outputs_are_read_out_of_hyprctl() {
        let json = r#"[{"name":"eDP-1","width":1920,"height":1200,"x":1280,"y":0,
                        "disabled":false},
                       {"name":"DP-3","width":3840,"height":2160,"disabled":true}]"#;
        assert_eq!(
            parse_outputs_json(json),
            vec![ScreenOutput {
                name: "eDP-1".into(),
                resolution: "1920x1200".into(),
                position: Some((1280, 0)),
            }]
        );
    }

    /// Parsing `xrandr --current`: connected outputs with their size,
    /// disconnected ones are skipped.
    #[test]
    fn outputs_are_read_out_of_xrandr() {
        assert_eq!(
            parse_xrandr_outputs(XRANDR_TWO),
            vec![
                ScreenOutput {
                    name: "DP-1".into(),
                    resolution: "1920x1080".into(),
                    position: Some((0, 0)),
                },
                ScreenOutput {
                    name: "HDMI-1".into(),
                    resolution: "1600x900".into(),
                    position: Some((1920, 0)),
                },
            ]
        );
    }

    /// Truncated xrandr output (mode lines only) is not emptiness but one
    /// nameless output with the active resolution: that is how the old rule
    /// behaved, and the new layout must not make the wallpaper set any worse.
    #[test]
    fn a_trimmed_xrandr_still_gives_a_resolution() {
        assert_eq!(
            parse_xrandr_outputs("   1280x720     60.00*+\n"),
            vec![ScreenOutput {
                name: "*".into(),
                resolution: "1280x720".into(),
                position: None,
            }]
        );
        // Such an output has no position — the geometry comes out with the former
        // zero offset.
        assert_eq!(
            parse_xrandr_outputs("   1280x720     60.00*+\n")[0].geometry(),
            "1280x720+0+0"
        );
    }

    /// swww names ALL outputs in one command — this is the `List` table row.
    #[test]
    fn swww_names_every_output_in_one_command() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, "");
        sys.answer("wlr-randr", 0, WLR_TWO);
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/котик.png",
            &["swww".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/котик.png");
        assert!(res.ok, "{}", res.message);
        journal("swww, два выхода", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert!(
            ran.iter()
                .any(|l| l == "swww img /tmp/котик.png --outputs DP-1,HDMI-A-1"),
            "{ran:?}"
        );
    }

    /// One output — the same single command, but the output is still named in it.
    #[test]
    fn swww_names_the_only_output_too() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, "");
        sys.answer("wlr-randr", 0, WLR_ONE);
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/котик.png",
            &["swww".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.png").ok);
        journal("swww, один выход", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert!(
            ran.iter().any(|l| l == "swww img /tmp/котик.png --outputs DP-1"),
            "{ran:?}"
        );
    }

    /// ⚠️ Foolproofing: there is no output query tool on the system — the
    /// wallpaper is set anyway, with exactly the command that was in the plan.
    #[test]
    fn without_a_probe_tool_the_command_stays_as_it_was() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, "");
        for missing in ["wlr-randr", "hyprctl", "xrandr"] {
            sys.missing.push(missing.to_string());
        }
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/котик.png",
            &["swww".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/котик.png");
        assert!(res.ok, "без опроса выходов обои не встали: {}", res.message);
        journal("swww, инструмента опроса нет", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert!(
            ran.iter().any(|l| l == "swww img /tmp/котик.png"),
            "{ran:?}"
        );
        assert!(
            !ran.iter().any(|l| l.contains("--outputs")),
            "выходы названы, хотя их никто не перечислил: {ran:?}"
        );
    }

    /// hyprpaper covers all monitors with ITS OWN form (an empty name before the
    /// comma): one command, and outputs are not queried for it at all.
    #[test]
    fn hyprpaper_covers_every_output_by_its_own_form() {
        let mut sys = FakeSys::default();
        sys.tools.push("hyprpaper".to_string());
        sys.answer("wlr-randr", 0, WLR_TWO);
        let plan = wpe::static_plan(
            "hyprland",
            "wayland",
            "/tmp/котик.png",
            &["hyprpaper".to_string()],
            "",
        );
        let mut e = exec("hyprland", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.png").ok);
        journal("hyprpaper, два выхода", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert_eq!(
            ran,
            vec![
                "hyprctl hyprpaper preload /tmp/котик.png".to_string(),
                "hyprctl hyprpaper wallpaper ,/tmp/котик.png".to_string(),
            ],
            "hyprpaper сходил куда-то ещё"
        );
    }

    /// mpvpaper accepts one output — so on two monitors it is started twice,
    /// and each launch carries ITS OWN output.
    #[test]
    fn mpvpaper_is_raised_once_per_output() {
        let mut sys = FakeSys::default();
        sys.tools.push("mpvpaper".to_string());
        sys.answer("wlr-randr", 0, WLR_TWO);
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/котик.gif",
            &["mpvpaper".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/котик.gif");
        assert!(res.ok, "{}", res.message);
        journal("mpvpaper, два выхода", &mut e);
        let spawned: Vec<String> = e
            .sys_mut()
            .spawned
            .iter()
            .map(|(c, _)| c.join(" "))
            .collect();
        assert_eq!(spawned.len(), 2, "{spawned:?}");
        assert!(spawned[0].ends_with("DP-1 /tmp/котик.gif"), "{spawned:?}");
        assert!(spawned[1].ends_with("HDMI-A-1 /tmp/котик.gif"), "{spawned:?}");
        // The wallpaper on the second monitor is part of the same wallpaper: a stop turns off both.
        assert_eq!(e.companions.len(), 1, "спутник не запомнен");
        e.stop(true);
        assert_eq!(e.sys_mut().killed.len(), 2, "второй монитор остался играть");
        assert!(e.companions.is_empty(), "спутник пережил остановку");
    }

    /// One monitor — one launch: the layout does not spawn an extra process.
    #[test]
    fn mpvpaper_with_one_output_is_raised_once() {
        let mut sys = FakeSys::default();
        sys.tools.push("mpvpaper".to_string());
        sys.answer("wlr-randr", 0, WLR_ONE);
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/котик.gif",
            &["mpvpaper".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("mpvpaper, один выход", &mut e);
        let spawned: Vec<String> = e
            .sys_mut()
            .spawned
            .iter()
            .map(|(c, _)| c.join(" "))
            .collect();
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].ends_with("DP-1 /tmp/котик.gif"), "{spawned:?}");
        assert!(e.companions.is_empty(), "лишний процесс на одном мониторе");
    }

    /// ⚠️ Foolproofing: an engine from the `Each` row, but the outputs were not
    /// enumerated — start ONE process with the former placeholder value, not
    /// zero and not two identical ones.
    #[test]
    fn each_engine_without_outputs_is_raised_once() {
        let mut sys = FakeSys::default();
        sys.tools.push("mpvpaper".to_string());
        for missing in ["wlr-randr", "hyprctl", "xrandr"] {
            sys.missing.push(missing.to_string());
        }
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/котик.gif",
            &["mpvpaper".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("mpvpaper, выходы не перечислились", &mut e);
        let spawned: Vec<String> = e
            .sys_mut()
            .spawned
            .iter()
            .map(|(c, _)| c.join(" "))
            .collect();
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].ends_with("* /tmp/котик.gif"), "{spawned:?}");
    }

    /// The GIF wallpaper plan under X11 — the same one the executor builds: it
    /// asks for the geometry with the `{geometry}` placeholder.
    fn x11_gif_plan() -> Vec<wpe::Step> {
        let tools = vec!["xwinwrap".to_string(), "mpv".to_string()];
        wpe::animated_plan(
            "generic",
            "x11",
            "/tmp/котик.gif",
            &tools,
            "{monitor}",
            wpe::GEOMETRY_MARK,
            &wpe::Hints::default(),
        )
    }

    /// An executor with xwinwrap and mpv "on the system".
    fn x11_exec(xrandr: Option<&str>) -> WallpaperExec<FakeSys> {
        let mut sys = FakeSys::default();
        sys.tools.push("xwinwrap".to_string());
        sys.tools.push("mpv".to_string());
        match xrandr {
            Some(out) => sys.answer("xrandr", 0, out),
            None => sys.missing.push("xrandr".to_string()),
        }
        exec("generic", "x11", sys)
    }

    /// What went out for launching.
    fn spawned_lines(e: &mut WallpaperExec<FakeSys>) -> Vec<String> {
        e.sys_mut().spawned.iter().map(|(c, _)| c.join(" ")).collect()
    }

    /// The `{monitor}` and `{geometry}` placeholders come from ONE query, and it is
    /// asked exactly once for the whole plan — however many commands the step
    /// produces.
    #[test]
    fn marks_come_from_one_and_the_same_probe() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(Some(XRANDR_TWO));
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap (X11), два выхода", &mut e);
        let probes = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| {
                matches!(c.first().map(|b| b.as_str()), Some("xrandr" | "wlr-randr" | "hyprctl"))
            })
            .count();
        assert_eq!(probes, 1, "выходы спрошены {probes} раз вместо одного");
    }

    /// xwinwrap covers the root window by geometry — so on two monitors it is
    /// started twice, and each launch has ITS OWN size with ITS OWN offset.
    #[test]
    fn xwinwrap_is_raised_once_per_output() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(Some(XRANDR_TWO));
        let res = e.run_plan(&plan, "/tmp/котик.gif");
        assert!(res.ok, "{}", res.message);
        journal("xwinwrap, два выхода", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 2, "{spawned:?}");
        assert!(spawned[0].contains("-g 1920x1080+0+0"), "{spawned:?}");
        assert!(spawned[1].contains("-g 1600x900+1920+0"), "{spawned:?}");
        // The second monitor's wallpaper is part of the same wallpaper: they go out together.
        assert_eq!(e.companions.len(), 1, "спутник не запомнен");
        e.stop(true);
        assert_eq!(e.sys_mut().killed.len(), 2, "второй монитор остался играть");
        assert!(e.companions.is_empty(), "спутник пережил остановку");
    }

    /// One monitor — one command, and the geometry in it is unchanged.
    #[test]
    fn xwinwrap_with_one_output_is_raised_once() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(Some(XRANDR_ONE));
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap, один выход", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].contains("-g 1920x1080+0+0"), "{spawned:?}");
        assert!(e.companions.is_empty(), "лишний процесс на одном мониторе");
    }

    /// ⚠️ Foolproofing: there are two outputs, but the query did not say where
    /// they stand. A second command would land on top of the first, so one goes
    /// out, with the former zero offset.
    #[test]
    fn xwinwrap_without_output_positions_is_raised_once() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(None);
        e.sys_mut().answer("wlr-randr", 0, WLR_TWO_NOWHERE);
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap, положение выходов неизвестно", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].contains("-g 2560x1440+0+0"), "{spawned:?}");
    }

    /// ⚠️ Foolproofing: nothing to enumerate outputs with — exactly the command
    /// from the plan goes out, with the engine default in the geometry.
    #[test]
    fn xwinwrap_without_a_probe_tool_is_raised_once() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(None);
        for missing in ["wlr-randr", "hyprctl"] {
            e.sys_mut().missing.push(missing.to_string());
        }
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap, инструмента опроса нет", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].contains("-g 1920x1080+0+0"), "{spawned:?}");
    }

    /// The old `{resolution}` placeholder is intact and still means ONLY the size:
    /// it is used by those who do not need the offset.
    #[test]
    fn the_resolution_mark_still_means_just_the_size() {
        let mut sys = FakeSys::default();
        sys.answer("xrandr", 0, XRANDR_TWO);
        let mut e = exec("generic", "x11", sys);
        let mut seen = Seen::new("x11");
        assert_eq!(
            fill(e.sys_mut(), "{resolution}", &mut seen),
            "1920x1080"
        );
        assert_eq!(
            fill(e.sys_mut(), wpe::GEOMETRY_MARK, &mut seen),
            "1920x1080+0+0"
        );
    }

    /// The engine table is responsible for the choice, not a guess from the look
    /// of the command: an unfamiliar engine behaves as before.
    #[test]
    fn an_unknown_engine_keeps_the_old_behaviour() {
        assert_eq!(engine_spread("нет-такого"), Spread::Whole);
        assert_eq!(engine_spread("feh"), Spread::Whole);
        assert_eq!(engine_spread("mpvpaper"), Spread::Each);
        assert!(matches!(engine_spread("swww"), Spread::List { .. }));
    }

    /// Extreme outputs: one picture for the whole desktop is measured by the
    /// LARGEST monitor, "a window covering the whole screen" by the smallest.
    #[test]
    fn the_extreme_outputs_are_picked_by_area() {
        let two = parse_xrandr_outputs(XRANDR_TWO);
        assert_eq!(largest_output(&two).map(|o| o.name.as_str()), Some("DP-1"));
        assert_eq!(largest_output(&two).and_then(|o| o.size()), Some((1920, 1080)));
        assert_eq!(smallest_output(&two).map(|o| o.name.as_str()), Some("HDMI-1"));
        assert_eq!(smallest_output(&two).and_then(|o| o.size()), Some((1600, 900)));
    }

    /// One output is both the largest and the smallest: the same answer the old
    /// "first resolution" parsing gave.
    #[test]
    fn a_single_output_answers_as_before() {
        let one = vec![ScreenOutput {
            name: "eDP-1".into(),
            resolution: "2560x1600".into(),
            position: Some((0, 0)),
        }];
        assert_eq!(largest_output(&one).and_then(|o| o.size()), Some((2560, 1600)));
        assert_eq!(smallest_output(&one).and_then(|o| o.size()), Some((2560, 1600)));
    }

    /// ⚠️ Foolproofing: no outputs at all, the resolution did not parse, a zero
    /// size — no extreme output is found, and the caller falls back to its
    /// former behavior.
    #[test]
    fn extremes_survive_a_failed_probe() {
        assert!(largest_output(&[]).is_none());
        assert!(smallest_output(&[]).is_none());
        let junk = vec![
            ScreenOutput { name: "A".into(), resolution: "нет".into(), position: None },
            ScreenOutput { name: "B".into(), resolution: "0x0".into(), position: None },
        ];
        assert!(largest_output(&junk).is_none());
        assert!(smallest_output(&junk).is_none());
        // If at least one parsed, it is taken rather than "nothing".
        let mixed = vec![
            ScreenOutput { name: "A".into(), resolution: "нет".into(), position: None },
            ScreenOutput { name: "B".into(), resolution: "1280x720".into(), position: None },
        ];
        assert_eq!(largest_output(&mixed).map(|o| o.name.as_str()), Some("B"));
    }

    /// Equal area — the name decides: the same machine must give the same answer.
    #[test]
    fn an_equal_area_is_settled_by_the_name() {
        let same = vec![
            ScreenOutput { name: "HDMI-1".into(), resolution: "1920x1080".into(), position: None },
            ScreenOutput { name: "DP-1".into(), resolution: "1920x1080".into(), position: None },
        ];
        assert_eq!(largest_output(&same).map(|o| o.name.as_str()), Some("DP-1"));
        assert_eq!(smallest_output(&same).map(|o| o.name.as_str()), Some("DP-1"));
    }

    // ── host crash watcher ───────────────────────────────────────────────

    /// Our own host went down by itself: the executor hands this to the watcher
    /// (does not restore the desktop, remembers the process), the restart is the
    /// same apply as by the button.
    #[test]
    fn a_crashed_host_is_left_to_the_watch_and_restarted_by_apply() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply_with_audio("/home/u/волны.mp4", "video", "Волны", false).ok);
        let snap = e.host_snapshot();
        assert_eq!(snap.subject.as_deref(), Some("video:/home/u/волны.mp4"));
        assert!(!snap.exited && !snap.via_extension);
        assert_eq!(snap.pids.len(), 1);

        // The process went down (SIGSEGV).
        e.sys_mut().spawn_rc = vec![Some(-1)];
        let watched = e.process.unwrap();
        let mut strikes = 0;
        assert_eq!(e.monitor_apply(watched, &mut strikes, &LoadObservation::default()), MonitorVerdict::Done);
        assert_eq!(e.current_format(), Some("video"), "надзор вернул стол сам — сторожу нечего поднимать");
        assert!(e.host_snapshot().exited);

        let before = e.sys_mut().spawned.len();
        let res = e.restart_host();
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert_eq!(spawned.len(), before + 1, "{spawned:?}");
        let last = &spawned.last().unwrap().0;
        assert_eq!(last[..4], [HOST, "--mode", "video", "/home/u/волны.mp4"]);
        assert!(last.iter().any(|a| a == "--no-audio"), "подъём потерял выбор звука: {last:?}");
        assert!(!e.host_snapshot().exited, "после подъёма хост всё ещё «мёртв»");
    }

    /// The watcher does not watch what the person stopped, nor foreign formats.
    #[test]
    fn stopped_and_foreign_wallpapers_are_not_watched() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        e.stop(true);
        assert_eq!(e.host_snapshot(), HostSnapshot::default());

        let mut sys = FakeSys::default();
        sys.tools.push("swaybg".to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply("/home/u/лес.png", "static", "").ok);
        assert_eq!(e.host_snapshot().subject, None);
        assert!(!host_watched_format("youtube") && !host_watched_format("static"));
    }

    /// GNOME: the host is a child of the extension. The restart clears the source
    /// and writes it again: the extension does not count the same value as a change.
    #[test]
    fn extension_host_is_restarted_through_its_settings() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        let snap = e.host_snapshot();
        assert!(snap.via_extension);
        assert_eq!(snap.host_binary.as_deref(), Some(HOST));
        assert!(snap.pids.is_empty());

        e.sys_mut().ran.clear();
        assert!(e.restart_host().ok);
        let ran = lines(&mut e);
        let clear = ran.iter().position(|l| l.ends_with("source ")).expect("источник не погашен");
        let again = ran.iter().position(|l| l.contains("source /home/u/волны.mp4")).expect("источник не записан");
        assert!(clear < again, "{ran:?}");
    }

    /// Crashes in a row: for video a frame from the movie becomes the desktop
    /// picture, and the previous picture stays remembered for «Stop».
    #[test]
    fn still_frame_of_a_video_replaces_the_live_wallpaper() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply("/home/u/волны.mp4", "video", "").ok);
        e.saved_wallpaper = Some("/home/u/прежняя.png".to_string());
        e.sys_mut().frame_ok = true;
        e.sys_mut().tools.push("swaymsg".to_string());
        e.sys_mut().spawn_rc = vec![Some(-1)];
        assert!(e.show_still_frame());
        assert_eq!(e.current_format(), Some("static"));
        let still = e.current_path().unwrap().to_string();
        assert!(still.contains("/rexon/video-stills/") && still.ends_with(".jpg"), "{still}");
        let frames = e.sys_mut().frames.clone();
        assert!(frames.iter().any(|(src, out)| src == "/home/u/волны.mp4" && *out == still), "{frames:?}");
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/home/u/прежняя.png"));
        assert_eq!(e.host_snapshot().subject, None, "после кадра сторож всё ещё поднимает обои");
        assert!(e.process.is_none());
    }

    /// No frame (a website's web address) — the previous desktop picture, as on
    /// «Stop»; for a local web page — the preview next to it.
    #[test]
    fn still_frame_falls_back_to_the_previous_picture() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        e.current_format = Some("web".to_string());
        e.host_request = Some(HostRequest {
            path: "https://example.com".into(),
            fmt: "web".into(),
            name: String::new(),
            audio: true,
        });
        assert!(!e.show_still_frame());
        assert_eq!(e.current_format(), None);

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.tools.push("swaymsg".to_string());
        let mut e = exec("sway", "wayland", sys);
        e.current_format = Some("web".to_string());
        e.host_request = Some(HostRequest {
            path: "file:///home/u/стр/index.html".into(),
            fmt: "web".into(),
            name: String::new(),
            audio: true,
        });
        assert!(e.show_still_frame());
        assert_eq!(e.current_path(), Some("/home/u/стр/preview.jpg"));
    }

}
