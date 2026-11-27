//! Live wallpaper watchdog — ONE for both kinds of processes that draw them:
//! waywallen renderers (Wallpaper Engine scenes and web) and Rexon's own host
//! `rexon-wallpaper-host` (video, web, Flash). If one crashes — bring the
//! wallpaper up again; if it crashes repeatedly — stop and tell the person.
//!
//! What the two kinds share is the decision "whether to bring it up once more"
//! ([`CrashBudget`]: how many restarts of the same wallpaper within a time
//! window) and the watchdog thread ([`run_forever`]). What differs is how to
//! learn that the process died and what to restart it with: a waywallen
//! renderer's process is foreign (a child of the daemon), its crash is visible
//! from a `systemd-coredump` record, and the daemon restarts it; the host is our
//! child (the `wallpaper_exec` executor) or a child of the GNOME/Cinnamon shell
//! extension, and it is restarted by the same apply as on the button. The
//! second half of the module is devoted to this ([`HostWatch`]).
//!
//! WAYWALLEN RENDERERS. WHY. Scenes and web wallpapers are drawn by a separate
//! waywallen process (`waywallen-<kind>-renderer`). If it terminates abnormally
//! — a graphics driver failure, an error in the scene itself — the daemon only
//! removes it from its lists: the chosen wallpaper stays assigned
//! (`CurrentWallpaperId`), but the desktop is empty and nobody tells the person
//! anything (a live case on 10.10.2026: GPU reset → renderer crashed → desktop
//! without a wallpaper).
//!
//! WHAT IT DOES. While Rexon is running (the tray icon), the watchdog looks at
//! the renderer processes once every [`POLL_SECS`]. If a process has vanished,
//! it asks the system journal whether that was an abnormal exit (a
//! `systemd-coredump` record with the process number). If it was, it asks the
//! daemon to apply the same wallpaper again and says so in a notification.
//!
//! WHAT IT DOES NOT DO.
//! * It does not touch wallpapers stopped on purpose: the daemon's auto-pause
//!   rules ("session locked — stop") and the person changing the wallpaper also
//!   remove the process, but there is no crash record then — and the watchdog
//!   stays silent.
//! * It does not restart in a loop: after [`MAX_RECOVERIES`] restarts of the
//!   same wallpaper within [`RECOVERY_WINDOW_SECS`] it stops and says that the
//!   wallpaper crashes by itself. Each such crash may cost a GPU reset.
//! * It does not see crashes without a crash record (the process was killed for
//!   lack of memory, exited with an error code) and does not work where there is
//!   no `journalctl`/`busctl`.
//!
//! The decision is the pure function [`Watch::tick`]; everything that goes
//! outside is in the "live" part below.
//!
//! OUR OWN WALLPAPER HOST. The host is started once on "Apply"; when it died
//! (GPU reset, sleep, a driver error) the desktop went empty, and there was
//! nobody to bring it back. Now [`HostWatch::tick`] decides by three signs:
//! * the process exited by itself (not on "Stop" and not for a replacement);
//! * the process is alive but does not draw: once every
//!   [`HEARTBEAT_INTERVAL_MS`] the host writes a heartbeat mark
//!   ([`heartbeat_path`]) from the interface thread and at the same time
//!   requests a frame from every window. If rendering hangs, the interface
//!   thread stops too, and the mark stops being updated. The check runs after
//!   resuming from sleep (the logind signal `PrepareForSleep(false)`) and, more
//!   leniently, all the time;
//! * a GPU reset (a new `devcoredump` record of a DRM device or the kernel event
//!   `WEDGED=`): the display is recreated even if the process is alive — after a
//!   reset the picture is sometimes black with a live process.
//! Restarts are the same as for the renderer, but more lenient in number: after
//! [`HOST_MAX_RESTARTS`] restarts within the window, the next crash is no longer
//! restarted — a still frame of the same wallpaper is put on the desktop (for
//! video — a frame from the clip, for web and Flash — the preview next to it if
//! there is one; otherwise the previous desktop picture) and ONE notification is
//! shown. The restarts themselves are silent — only the log: the person has
//! nothing to do with them.
//!
//! WHY NOT A systemd SERVICE with `Restart=on-failure` (like the waywallen
//! engine). The host is fully owned by the wallpaper executor: freezing under
//! load (SIGSTOP to its child), companions on other monitors, fallback steps of
//! the plan, restoring the desktop on "Stop". Under a service all of this would
//! have to be done a second way through `systemctl`, and a service cannot do the
//! frame after crashes, the single notification and the check after sleep — the
//! watchdog would be needed anyway, and there would be two mechanisms. On GNOME
//! and Cinnamon-Wayland the host is a child of the shell altogether, it cannot
//! be handed to a service. And without systemd (Void, Alpine, Gentoo with
//! OpenRC) the watchdog works the same way.

use std::collections::HashMap;

use crate::i18n::tr;

/// How often to look at the processes, seconds.
pub const POLL_SECS: u64 = 10;
/// How many passes to wait for a crash record after a process vanished:
/// `systemd-coredump` does not write it instantly (a large memory image takes
/// seconds).
pub const CRASH_WAIT_TICKS: u32 = 3;
/// The window in which restarts of the same wallpaper are counted, seconds.
pub const RECOVERY_WINDOW_SECS: u64 = 600;
/// How many times per window to restart the same wallpaper. Once is enough for
/// a random failure; a second time in a row means the wallpaper crashes by
/// itself.
pub const MAX_RECOVERIES: usize = 1;

/// How many times in a row to restart the same wallpaper: restarts within a
/// `window` of seconds, per wallpaper. Shared by waywallen renderers and the
/// host — they differ only in the limit.
#[derive(Debug, Default)]
pub struct CrashBudget {
    /// Restarts done: (time, wallpaper).
    recoveries: Vec<(u64, String)>,
    /// Wallpapers it was decided not to restart any more (until the person changes them).
    given_up: Option<String>,
}

impl CrashBudget {
    /// The person chose another wallpaper — the previous refusal does not apply to it.
    pub fn forget_if_changed(&mut self, current: &str) {
        if self.given_up.as_deref().is_some_and(|id| id != current) {
            self.given_up = None;
        }
    }

    /// This wallpaper has already been given up on.
    pub fn is_given_up(&self, id: &str) -> bool {
        self.given_up.as_deref() == Some(id)
    }

    /// One more crash of wallpaper `id`: true — restart (the restart is recorded),
    /// false — the limit for the window is exhausted, the wallpaper is marked
    /// "do not restart any more".
    pub fn spend(&mut self, now: u64, id: &str, max: usize, window: u64) -> bool {
        self.recoveries.retain(|(at, _)| now.saturating_sub(*at) <= window);
        let recent = self.recoveries.iter().filter(|(_, was)| was == id).count();
        if recent >= max {
            self.given_up = Some(id.to_string());
            return false;
        }
        self.recoveries.push((now, id.to_string()));
        true
    }
}

/// What to do after the next pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing: everything works or there is no need to intervene.
    Nothing,
    /// A renderer crashed — apply wallpaper `id` again.
    Recover { id: String },
    /// Wallpaper `id` crashes repeatedly — do not restart any more, tell the person.
    GiveUp { id: String },
}

/// The watchdog's memory between passes.
#[derive(Debug, Default)]
pub struct Watch {
    /// Live renderers: process number → the wallpaper chosen while it ran.
    seen: HashMap<u32, String>,
    /// Vanished processes for which we are still waiting for a crash record:
    /// (process number, wallpaper, how many passes are left to wait).
    pending: Vec<(u32, String, u32)>,
    /// How much restarting is still allowed.
    budget: CrashBudget,
}

impl Watch {
    /// One pass.
    ///
    /// `now` — time, seconds; `alive` — numbers of the live renderer processes;
    /// `current_id` — the wallpaper chosen in the daemon (empty — the daemon is
    /// silent or there is no wallpaper); `crashed(pid)` — whether the system journal
    /// has a record of an abnormal exit of this process.
    pub fn tick(
        &mut self,
        now: u64,
        alive: &[u32],
        current_id: &str,
        crashed: &dyn Fn(u32) -> bool,
    ) -> Step {
        self.budget.forget_if_changed(current_id);

        // Vanished processes go into waiting for a crash record.
        let gone: Vec<u32> = self.seen.keys().copied().filter(|pid| !alive.contains(pid)).collect();
        for pid in gone {
            if let Some(id) = self.seen.remove(&pid) {
                self.pending.push((pid, id, CRASH_WAIT_TICKS));
            }
        }
        // New processes are remembered with the wallpaper chosen now.
        for pid in alive {
            self.seen.entry(*pid).or_insert_with(|| current_id.to_string());
        }

        // Which of the vanished ones actually crashed.
        let mut crashed_id: Option<String> = None;
        let mut still_waiting = Vec::new();
        for (pid, id, left) in std::mem::take(&mut self.pending) {
            if crashed(pid) {
                crashed_id = Some(id);
            } else if left > 1 {
                still_waiting.push((pid, id, left - 1));
            }
        }
        self.pending = still_waiting;

        let Some(id) = crashed_id else { return Step::Nothing };
        // We restart only what the person still wants to see: the same
        // wallpaper, no renderer (the daemon did not restart it by itself, the
        // person did not choose another one), and we have not given up on this
        // wallpaper yet.
        if id.is_empty() || id != current_id || !alive.is_empty() {
            return Step::Nothing;
        }
        if self.budget.is_given_up(&id) {
            return Step::Nothing;
        }
        if !self.budget.spend(now, &id, MAX_RECOVERIES, RECOVERY_WINDOW_SECS) {
            return Step::GiveUp { id };
        }
        Step::Recover { id }
    }
}

// ── Our own wallpaper host: the decision ───────────────────────────────

/// How many times in a row to restart the host with the same wallpaper within
/// [`RECOVERY_WINDOW_SECS`]. The next crash (the third in a row) gets a frame
/// instead of the wallpaper: the host can fail through no fault of its own
/// (sleep, GPU reset), so the limit is more lenient than for a waywallen
/// renderer, but there is still no crash loop.
pub const HOST_MAX_RESTARTS: usize = 2;
/// How often the host writes the heartbeat mark, milliseconds.
pub const HEARTBEAT_INTERVAL_MS: u64 = 2000;
/// A mark older than this with a live process — the host has hung, seconds. The
/// period is deliberately long: a false restart of a working wallpaper is worse
/// than a minute of a frozen frame.
pub const HEARTBEAT_STALE_SECS: u64 = 60;
/// How long to wait after resuming from sleep before demanding a fresh mark,
/// seconds: the compositor and the driver need time to bring the screens back.
pub const RESUME_GRACE_SECS: u64 = 15;
/// Beginning of the heartbeat mark file name in `$XDG_RUNTIME_DIR`.
pub const HEARTBEAT_PREFIX: &str = "rexon-wp-heartbeat-";

/// What happened to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTrouble {
    /// The process exited by itself.
    Crashed,
    /// The process is alive but does not draw (the heartbeat mark is not updated).
    Hung,
    /// The GPU was reset — the display has to be recreated.
    GpuReset,
}

/// What to do with the host after a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStep {
    /// Nothing.
    Nothing,
    /// Bring the same wallpaper up again.
    Restart { subject: String, why: HostTrouble },
    /// The wallpaper crashes repeatedly — put its still frame and tell the person.
    StillFrame { subject: String },
}

/// Observation of the host for one pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostTick {
    /// What our host should be showing now (format and path); `None` — we have no
    /// live wallpaper, there is nothing to watch.
    pub subject: Option<String>,
    /// Our host process exited by itself (the executor has its status).
    pub exited: bool,
    /// Marks of live host processes: the time of the last one (seconds of
    /// [`boottime_secs`]) or `None` — no mark (a host of an older build).
    pub beats: Vec<Option<u64>>,
    /// The host is frozen by the load supervisor — the mark's silence is legitimate.
    pub paused: bool,
    /// The host is a child of the shell extension: we do not have its status, and
    /// only the disappearance of a process with a mark tells of its death.
    pub by_heartbeat_only: bool,
}

/// The host watchdog's memory between passes.
#[derive(Debug, Default)]
pub struct HostWatch {
    budget: CrashBudget,
    /// The wallpaper we are watching now.
    subject: Option<String>,
    /// This wallpaper already had a live mark (before that, death cannot be judged
    /// by the process vanishing: the extension may not have started yet).
    seen_beat: bool,
    /// When we resumed from sleep — the "does it draw" check is scheduled for later.
    woke_at: Option<u64>,
    /// There was a GPU reset.
    gpu_reset: bool,
}

impl HostWatch {
    /// The system resumed from sleep at moment `at` (seconds of [`boottime_secs`]).
    pub fn woke(&mut self, at: u64) {
        self.woke_at = Some(at);
    }

    /// The kernel reported a GPU reset.
    pub fn gpu_reset(&mut self) {
        self.gpu_reset = true;
    }

    /// One pass. `now` — seconds of [`boottime_secs`].
    pub fn tick(&mut self, now: u64, t: &HostTick) -> HostStep {
        let Some(subject) = t.subject.clone() else {
            // We have no wallpaper of our own: events that arrived without it do
            // not apply to a future wallpaper.
            self.subject = None;
            self.seen_beat = false;
            self.woke_at = None;
            self.gpu_reset = false;
            return HostStep::Nothing;
        };
        self.budget.forget_if_changed(&subject);
        if self.subject.as_deref() != Some(subject.as_str()) {
            self.subject = Some(subject.clone());
            self.seen_beat = false;
        }
        if self.budget.is_given_up(&subject) {
            self.woke_at = None;
            self.gpu_reset = false;
            return HostStep::Nothing;
        }
        if t.beats.iter().any(Option::is_some) {
            self.seen_beat = true;
        }
        let gone = t.exited || (t.by_heartbeat_only && self.seen_beat && t.beats.is_empty());
        let trouble = if gone {
            Some(HostTrouble::Crashed)
        } else if std::mem::take(&mut self.gpu_reset) {
            Some(HostTrouble::GpuReset)
        } else if t.paused {
            // Frozen — silent legitimately; we do not accumulate the post-sleep check.
            self.woke_at = None;
            None
        } else {
            self.hung(now, &t.beats).then_some(HostTrouble::Hung)
        };
        let Some(why) = trouble else { return HostStep::Nothing };
        self.woke_at = None;
        self.gpu_reset = false;
        if !self.budget.spend(now, &subject, HOST_MAX_RESTARTS, RECOVERY_WINDOW_SECS) {
            return HostStep::StillFrame { subject };
        }
        // A new process must first check in by itself.
        self.seen_beat = false;
        HostStep::Restart { subject, why }
    }

    /// Whether the host has hung according to the marks. Processes without a mark
    /// (an older build) are not judged at all.
    fn hung(&mut self, now: u64, beats: &[Option<u64>]) -> bool {
        if let Some(woke) = self.woke_at {
            if now < woke + RESUME_GRACE_SECS {
                return false; // too early to judge after sleep
            }
            self.woke_at = None;
            if beats.iter().flatten().any(|beat| *beat < woke) {
                return true; // never checked in after sleep
            }
        }
        beats.iter().flatten().any(|beat| now.saturating_sub(*beat) > HEARTBEAT_STALE_SECS)
    }
}

/// Heartbeat mark file of host process `pid` in the session directory.
pub fn heartbeat_path(runtime_dir: &str, pid: i64) -> String {
    format!("{}/{HEARTBEAT_PREFIX}{pid}", runtime_dir.trim_end_matches('/'))
}

/// Process number from a mark file name; not a mark — `None`.
pub fn heartbeat_pid(file_name: &str) -> Option<i64> {
    file_name.strip_prefix(HEARTBEAT_PREFIX)?.parse().ok().filter(|pid| *pid > 0)
}

/// Time from a mark file; half-written or empty — `None`.
pub fn parse_heartbeat(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// Seconds of the `CLOCK_BOOTTIME` clock: it runs during sleep too and does not
/// jump when the system time changes (the clock of a machine with Windows next
/// to it runs in local time and gets adjusted). One clock for the host and the
/// watchdog — the marks are comparable.
pub fn boottime_secs() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } != 0 {
        return 0;
    }
    ts.tv_sec.max(0) as u64
}

/// Write this process's heartbeat mark. If there is no session directory —
/// nothing: without it the watchdog could not find the mark anyway. The host
/// calls this from the interface thread.
pub fn write_heartbeat_live() {
    let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else { return };
    if dir.is_empty() {
        return;
    }
    let path = heartbeat_path(&dir, std::process::id() as i64);
    let _ = std::fs::write(path, boottime_secs().to_string());
}

/// A `gdbus monitor` line about sleep: `Some(true)` — going to sleep,
/// `Some(false)` — resumed from it; anything else — `None`. The line looks like:
/// `/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)`.
pub fn parse_prepare_for_sleep(line: &str) -> Option<bool> {
    let (_, args) = line.split_once("org.freedesktop.login1.Manager.PrepareForSleep")?;
    let args = args.trim_start();
    if args.starts_with("(true") {
        Some(true)
    } else if args.starts_with("(false") {
        Some(false)
    } else {
        None
    }
}

/// A `udevadm monitor --kernel --property` line saying that a DRM device
/// "wedged" and was reset (the kernel event `WEDGED=<method>`, kernels 6.14+).
pub fn parse_drm_wedged(line: &str) -> bool {
    line.trim_start().starts_with("WEDGED=")
}

/// Whether a new `devcoredump` record from the GPU has appeared. `known` — the
/// names seen by the previous pass (`None` — the first pass: what was there
/// before the watchdog started does not count as an event); `now` — (name, is it
/// a GPU).
pub fn new_gpu_dump(known: &mut Option<Vec<String>>, now: &[(String, bool)]) -> bool {
    let fresh = match known.as_ref() {
        None => false,
        Some(seen) => now.iter().any(|(name, gpu)| *gpu && !seen.contains(name)),
    };
    *known = Some(now.iter().map(|(name, _)| name.clone()).collect());
    fresh
}

/// Candidates for the still frame of web and Flash wallpapers: the preview next
/// to the wallpaper. `path` — the wallpaper path (a directory or a file, a
/// `file://` address too); `project_json` — the text of the `project.json` in its
/// directory, if any. A site address — empty: we have no frame of a site.
pub fn still_frame_candidates(path: &str, project_json: Option<&str>) -> Vec<String> {
    let local = path.strip_prefix("file://").unwrap_or(path);
    if !local.starts_with('/') {
        return Vec::new();
    }
    let lower = local.to_lowercase();
    let dir = if [".html", ".htm", ".swf"].iter().any(|ext| lower.ends_with(ext)) {
        local.rsplit_once('/').map(|(d, _)| d).unwrap_or(local)
    } else {
        local
    };
    let dir = dir.trim_end_matches('/');
    let mut out = Vec::new();
    if let Some(name) = project_json
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v["preview"].as_str().map(str::to_string))
        .filter(|n| !n.is_empty() && !n.contains('/'))
    {
        out.push(format!("{dir}/{name}"));
    }
    for ext in ["jpg", "png", "gif", "webp"] {
        let cand = format!("{dir}/preview.{ext}");
        if !out.contains(&cand) {
            out.push(cand);
        }
    }
    out
}

/// The wallpaper directory in which to look for `project.json` for
/// [`still_frame_candidates`].
pub fn still_frame_dir(path: &str) -> Option<String> {
    still_frame_candidates(path, None)
        .first()
        .and_then(|c| c.rsplit_once('/').map(|(d, _)| d.to_string()))
}

// ── Labels ─────────────────────────────────────────────────────────────

/// Title of the watchdog's notifications.
pub fn notice_title() -> String {
    tr("Live wallpaper")
}

/// The wallpaper was brought back after a renderer crash.
pub fn recovered_text() -> String {
    tr("The program that draws your wallpaper stopped unexpectedly. Rexon has started the wallpaper again.")
}

/// The wallpaper crashed again — we no longer restart it.
pub fn gave_up_text() -> String {
    tr("Your wallpaper keeps stopping unexpectedly, so Rexon no longer restarts it. \
        Choose another wallpaper in the Wallpaper Engine tab.")
}

/// Bringing the wallpaper back failed (the daemon refused).
pub fn recover_failed_text() -> String {
    tr("The program that draws your wallpaper stopped unexpectedly, and Rexon couldn't start it again. \
        Apply the wallpaper again in the Wallpaper Engine tab.")
}

/// Our own live wallpaper crashed repeatedly — its still frame stands in its place.
pub fn host_still_frame_text() -> String {
    tr("Your live wallpaper stopped unexpectedly several times in a row, so Rexon has put a still picture of it on the desktop instead. \
        To try the live wallpaper again, apply it once more.")
}

/// Our own live wallpaper crashed repeatedly, there is no frame — the previous
/// picture came back.
pub fn host_previous_picture_text() -> String {
    tr("Your live wallpaper stopped unexpectedly several times in a row, so Rexon has put back your previous desktop picture. \
        To try the live wallpaper again, apply it once more.")
}

// ── Parsing system answers ─────────────────────────────────────────────

/// A waywallen renderer process by the first word of its command line: the file
/// `waywallen-<kind>-renderer`.
pub fn is_renderer_exe(argv0: &str) -> bool {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    name.starts_with("waywallen-") && name.ends_with("-renderer")
}

/// Value of a string property from a `busctl get-property` answer:
/// `s "16352"` → `16352`. Not a string or empty — an empty string.
pub fn busctl_string(output: &str) -> String {
    let line = output.trim();
    let Some(rest) = line.strip_prefix("s ") else { return String::new() };
    rest.trim().trim_matches('"').to_string()
}

/// Whether the journal answer (`journalctl -o json`, crash records by process
/// number) says that it was the renderer that crashed: the record has the
/// program path `COREDUMP_EXE`, and it is a renderer.
pub fn journal_names_renderer_crash(journal_json: &str) -> bool {
    journal_json.lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v["COREDUMP_EXE"].as_str().map(is_renderer_exe))
            .unwrap_or(false)
    })
}

// ── Live part ──────────────────────────────────────────────────────────

/// The `systemd-coredump` message id in the journal ("process dumped core") — a
/// permanent identifier from the systemd message catalog.
const COREDUMP_MESSAGE_ID: &str = "fc2e22bc6ee647b6b90729ab34a250b1";
const DAEMON_BUS: &str = "org.waywallen.waywallen.Daemon";
const DAEMON_PATH: &str = "/org/waywallen/waywallen/Daemon";
const DAEMON_IFACE: &str = "org.waywallen.waywallen.Daemon1";

fn run_capture(argv: &[&str]) -> Option<(i32, String)> {
    let spec = crate::utils::RunSpec {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        output: crate::utils::RunOutput::Capture,
        timeout_secs: Some(15),
        ..crate::utils::RunSpec::default()
    };
    let report = crate::utils::run(&spec, std::env::var("PATH").ok().as_deref());
    let code = report.returncode?;
    Some((code, String::from_utf8_lossy(&report.stdout).into_owned()))
}

/// Numbers of this user's live renderer processes.
fn live_renderer_pids() -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut pids = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or_default();
        if is_renderer_exe(&String::from_utf8_lossy(argv0)) {
            pids.push(pid);
        }
    }
    pids
}

/// Whether this user's WE wallpaper renderers are alive (Rexon's playlist learns
/// from this that the screen is taken by Wallpaper Engine wallpapers).
pub fn renderers_running() -> bool {
    !live_renderer_pids().is_empty()
}

/// The wallpaper chosen in the daemon; empty — the daemon does not answer or
/// there is no wallpaper.
fn live_current_id() -> String {
    run_capture(&["busctl", "--user", "get-property", DAEMON_BUS, DAEMON_PATH, DAEMON_IFACE, "CurrentWallpaperId"])
        .filter(|(code, _)| *code == 0)
        .map(|(_, out)| busctl_string(&out))
        .unwrap_or_default()
}

/// Whether the journal has a record of an abnormal exit of process `pid`.
fn live_crashed(pid: u32) -> bool {
    let by_id = format!("MESSAGE_ID={COREDUMP_MESSAGE_ID}");
    let by_pid = format!("COREDUMP_PID={pid}");
    run_capture(&["journalctl", "--user", "--no-pager", "-o", "json", "--since", "-15min", &by_id, &by_pid])
        .map(|(_, out)| journal_names_renderer_crash(&out))
        .unwrap_or(false)
}

/// Ask the daemon to apply wallpaper `id` again.
fn live_apply(id: &str) -> bool {
    run_capture(&["busctl", "--user", "call", DAEMON_BUS, DAEMON_PATH, DAEMON_IFACE, "ApplyById", "s", id])
        .is_some_and(|(code, _)| code == 0)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The notification button "Open Wallpaper Engine".
pub const ACTION_OPEN: &str = "open";

/// Buttons of the notification that asks to choose or apply the wallpaper again.
pub fn open_actions() -> Vec<(String, String)> {
    vec![(ACTION_OPEN.to_string(), tr("Open Wallpaper Engine"))]
}

/// Notification `text` with the "Open Wallpaper Engine" button: the text asks
/// the person to choose or apply the wallpaper in the tab — the button leads
/// right there. The answer is awaited in a separate thread: meanwhile the
/// watchdog keeps watching.
fn ask_open_wallpaper_engine(text: String) {
    std::thread::spawn(move || {
        let answer = crate::utils::notify_ask_live(&notice_title(), &text, &open_actions(), "", 24 * 3600);
        if answer.as_deref() == Some(ACTION_OPEN) {
            let section = format!("{}wallpaper_engine", crate::cli::SECTION_FLAG);
            if let Err(e) = crate::installer::open_rexon_live(&[&section]) {
                crate::log_warn!("wallpaper", "окно Wallpaper Engine не открылось: {e}");
            }
        }
    });
}

/// Start the watchdog in this process if it is not started yet. It is called by
/// the tray icon and by every live wallpaper apply (`wallpaper_exec::run_in_slot`):
/// without the icon (no tray in the session) our own host must also be watched.
pub fn ensure_running() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("renderer-watch".into())
            .spawn(run_forever);
        if let Err(e) = spawned {
            crate::log_warn!("wallpaper", "сторож живых обоев не запущен: {e}");
        }
    });
}

/// System events the watchdog responds to immediately, without waiting for a pass.
enum SystemEvent {
    /// Resumed from sleep.
    Woke,
    /// The kernel reported a GPU reset.
    GpuReset,
}

/// Read the lines of a long-lived command and hand the events to the watchdog.
/// The command dies together with the Rexon process (`PR_SET_PDEATHSIG`). If
/// there is no command or it exited — one line in the log; the watchdog lives on
/// without it.
fn spawn_line_monitor(
    argv: &'static [&'static str],
    parse: fn(&str) -> Option<SystemEvent>,
    tx: std::sync::mpsc::Sender<SystemEvent>,
) {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    let Some(bin) = crate::utils::which(argv[0], std::env::var("PATH").ok().as_deref()) else {
        crate::log_info!("wallpaper", "сторож живых обоев: нет {}, это событие не ловится", argv[0]);
        return;
    };
    let _ = std::thread::Builder::new()
        .name(format!("watch-{}", argv[0]))
        .spawn(move || {
            let mut cmd = std::process::Command::new(bin);
            cmd.args(&argv[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
            let Ok(mut child) = cmd.spawn() else {
                crate::log_warn!("wallpaper", "сторож живых обоев: {} не запустился", argv[0]);
                return;
            };
            if let Some(out) = child.stdout.take() {
                for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Some(event) = parse(&line) {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            crate::log_info!("wallpaper", "сторож живых обоев: {} завершился, событие больше не ловится", argv[0]);
        });
}

/// logind's signal about resuming from sleep — the system bus, without root.
const SLEEP_MONITOR: &[&str] = &[
    "gdbus", "monitor", "--system", "--dest", "org.freedesktop.login1", "--object-path", "/org/freedesktop/login1",
];
/// Kernel events from DRM devices (`WEDGED=` after a reset) — the netlink socket
/// is read without root.
const DRM_MONITOR: &[&str] = &["udevadm", "monitor", "--kernel", "--property", "--subsystem-match=drm"];
/// Memory image records of devices: after a reset a GPU gets `devcdN` here (it
/// lives for ~5 minutes, readable without root).
const DEVCOREDUMP_DIR: &str = "/sys/class/devcoredump";

/// `devcoredump` records now: (name, is it a GPU — the device has a `drm`
/// directory).
fn live_devcoredumps() -> Vec<(String, bool)> {
    let Ok(dir) = std::fs::read_dir(DEVCOREDUMP_DIR) else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            name.starts_with("devcd").then(|| {
                let gpu = e.path().join("failing_device/drm").is_dir();
                (name, gpu)
            })
        })
        .collect()
}

/// Whether the process's executable matches our host (checked by path, never by
/// name).
fn pid_runs(pid: i64, binary: &str) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else { return false };
    let want = std::fs::canonicalize(binary).unwrap_or_else(|_| binary.into());
    exe == want
}

/// A process mark from the session directory.
fn live_beat(runtime: &str, pid: i64) -> Option<u64> {
    std::fs::read_to_string(heartbeat_path(runtime, pid)).ok().and_then(|t| parse_heartbeat(&t))
}

/// Host processes started by the shell extension: a mark in the session
/// directory, the process is alive and it is our binary. Marks of dead processes
/// are removed.
fn live_extension_hosts(runtime: &str, binary: &str) -> Vec<i64> {
    let Ok(dir) = std::fs::read_dir(runtime) else { return Vec::new() };
    let mut pids = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(heartbeat_pid) else { continue };
        if pid_runs(pid, binary) {
            pids.push(pid);
        } else if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    pids
}

/// A pass over our own host: the observation is with the executor, the decision
/// with [`HostWatch`], the execution again with the executor.
fn live_host_pass(watch: &mut HostWatch) {
    use crate::wallpaper_exec as exec;
    let slot = exec::live_slot();
    // The executor is busy (an apply or a supervisor measurement is in progress) —
    // next time.
    let Some(snap) = exec::try_with_slot(&slot, |e| e.host_snapshot()) else { return };
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    // The marks are walked on every pass: along the way marks of dead
    // processes are removed (our children's too — they do not clean up after
    // themselves).
    let marked = if runtime.is_empty() {
        Vec::new()
    } else {
        live_extension_hosts(&runtime, snap.host_binary.as_deref().unwrap_or(""))
    };
    let pids = if snap.via_extension { marked } else { snap.pids.clone() };
    let tick = HostTick {
        subject: snap.subject.clone(),
        exited: snap.exited,
        beats: pids.iter().map(|pid| if runtime.is_empty() { None } else { live_beat(&runtime, *pid) }).collect(),
        paused: snap.paused,
        by_heartbeat_only: snap.via_extension,
    };
    match watch.tick(boottime_secs(), &tick) {
        HostStep::Nothing => {}
        HostStep::Restart { subject, why } => {
            crate::log_warn!("wallpaper", "хост обоев {subject}: {why:?} — поднимаю обои заново");
            if snap.via_extension {
                // A hung child of the shell: we take it down ourselves — verified by
                // path that it is our binary. Our own children are taken down by the
                // apply.
                if let Some(bin) = &snap.host_binary {
                    for pid in pids.iter().filter(|pid| pid_runs(**pid, bin)) {
                        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
                    }
                }
            }
            let res = exec::run_in_slot(&slot, |e| e.restart_host());
            crate::log_info!("wallpaper", "хост обоев {subject}: подъём {}", if res.ok { "удался" } else { "не удался" });
            if !res.ok {
                // Bringing it back failed (no file, the host refused) — from here on
                // the same as after repeated crashes.
                give_up_host(&slot, &subject);
            }
        }
        HostStep::StillFrame { subject } => give_up_host(&slot, &subject),
    }
}

/// Stop restarting our own wallpaper: a frame in its place and ONE notification.
fn give_up_host(slot: &crate::wallpaper_exec::LiveSlot, subject: &str) {
    let still = crate::wallpaper_exec::with_slot(slot, |e| e.show_still_frame());
    crate::log_warn!(
        "wallpaper",
        "хост обоев {subject} падает подряд — больше не поднимаю; {}",
        if still { "поставлен неподвижный кадр" } else { "возвращена прежняя картинка" }
    );
    let text = if still { host_still_frame_text() } else { host_previous_picture_text() };
    crate::utils::notify_live(&notice_title(), &text);
}

/// Watch while the process is alive. Call it in a separate thread (via
/// [`ensure_running`]).
///
/// The waywallen renderers need `journalctl` and `busctl`: without them this
/// half is silent (one line in the log), while our own host is watched anyway.
pub fn run_forever() {
    let path = std::env::var("PATH").ok();
    let renderers = ["journalctl", "busctl"].iter().all(|tool| {
        let found = crate::utils::which(tool, path.as_deref()).is_some();
        if !found {
            crate::log_info!("wallpaper", "сторож рисовальщика waywallen не работает: нет {tool}");
        }
        found
    });
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_line_monitor(SLEEP_MONITOR, |l| (parse_prepare_for_sleep(l) == Some(false)).then_some(SystemEvent::Woke), tx.clone());
    spawn_line_monitor(DRM_MONITOR, |l| parse_drm_wedged(l).then_some(SystemEvent::GpuReset), tx);
    crate::log_info!("wallpaper", "сторож живых обоев запущен: проход раз в {POLL_SECS} с");
    let mut watch = Watch::default();
    let mut host = HostWatch::default();
    let mut dumps: Option<Vec<String>> = None;
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(POLL_SECS)) {
            Ok(SystemEvent::Woke) => {
                crate::log_info!("wallpaper", "система вышла из сна — проверю, рисуют ли обои");
                host.woke(boottime_secs());
                continue; // the check — after RESUME_GRACE_SECS, in an ordinary pass
            }
            Ok(SystemEvent::GpuReset) => {
                crate::log_warn!("wallpaper", "ядро сообщило о сбросе видеокарты");
                host.gpu_reset();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // Both observers are gone — from now on only ordinary passes.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(std::time::Duration::from_secs(POLL_SECS));
            }
        }
        if new_gpu_dump(&mut dumps, &live_devcoredumps()) {
            crate::log_warn!("wallpaper", "видеокарта сброшена (новая запись devcoredump)");
            host.gpu_reset();
        }
        live_host_pass(&mut host);
        if !renderers {
            continue;
        }
        let alive = live_renderer_pids();
        // We ask the daemon only when there is something to ask about: renderers
        // are alive or recently vanished. When idle, the watchdog wakes nobody.
        if alive.is_empty() && watch.seen.is_empty() && watch.pending.is_empty() {
            continue;
        }
        let current = live_current_id();
        match watch.tick(now_secs(), &alive, &current, &live_crashed) {
            Step::Nothing => {}
            Step::Recover { id } => {
                crate::log_warn!("wallpaper", "рисовальщик обоев {id} аварийно завершился — применяю обои заново");
                let ok = live_apply(&id);
                crate::log_info!("wallpaper", "повторное применение обоев {id}: {}", if ok { "принято демоном" } else { "демон отказал" });
                if ok {
                    crate::utils::notify_live(&notice_title(), &recovered_text());
                } else {
                    ask_open_wallpaper_engine(recover_failed_text());
                }
            }
            Step::GiveUp { id } => {
                crate::log_warn!("wallpaper", "рисовальщик обоев {id} упал повторно — больше не перезапускаю");
                ask_open_wallpaper_engine(gave_up_text());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crashed_set(pids: &'static [u32]) -> impl Fn(u32) -> bool {
        move |pid| pids.contains(&pid)
    }

    #[test]
    fn crash_of_the_chosen_wallpaper_is_recovered_once() {
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        assert_eq!(w.tick(0, &[100], "16352", &none), Step::Nothing);
        // The process vanished, there is no crash record yet — we wait.
        assert_eq!(w.tick(10, &[], "16352", &none), Step::Nothing);
        // The record appeared on the next pass — we restart.
        assert_eq!(w.tick(20, &[], "16352", &crashed_set(&[100])), Step::Recover { id: "16352".into() });
        // The daemon started a new process; it crashed again in the same window — refusal.
        assert_eq!(w.tick(30, &[200], "16352", &none), Step::Nothing);
        assert_eq!(w.tick(40, &[], "16352", &crashed_set(&[200])), Step::GiveUp { id: "16352".into() });
        // After a refusal we no longer touch the same wallpaper, even if it was applied
        // by hand and crashed once more.
        assert_eq!(w.tick(50, &[300], "16352", &none), Step::Nothing);
        assert_eq!(w.tick(60, &[], "16352", &crashed_set(&[300])), Step::Nothing);
        // Another wallpaper — the count starts over.
        assert_eq!(w.tick(70, &[400], "777", &none), Step::Nothing);
        assert_eq!(w.tick(80, &[], "777", &crashed_set(&[400])), Step::Recover { id: "777".into() });
    }

    #[test]
    fn deliberate_stop_and_wallpaper_change_are_left_alone() {
        // The process vanished without a crash record (auto-pause, the daemon exiting).
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        w.tick(0, &[100], "1", &none);
        for at in 1..=5 {
            assert_eq!(w.tick(at * 10, &[], "1", &none), Step::Nothing);
        }
        assert!(w.pending.is_empty(), "ожидание записи об аварии не кончилось");

        // Crashed, but the person has already chosen another wallpaper — we do not
        // bring the old one back.
        let mut w = Watch::default();
        w.tick(0, &[100], "1", &none);
        assert_eq!(w.tick(10, &[], "2", &crashed_set(&[100])), Step::Nothing);

        // Crashed, but another renderer is already working — the daemon managed by itself.
        let mut w = Watch::default();
        w.tick(0, &[100], "1", &none);
        assert_eq!(w.tick(10, &[101], "1", &crashed_set(&[100])), Step::Nothing);

        // The daemon is silent (the wallpaper is unknown) — there is nothing to apply.
        let mut w = Watch::default();
        w.tick(0, &[100], "", &none);
        assert_eq!(w.tick(10, &[], "", &crashed_set(&[100])), Step::Nothing);
    }

    #[test]
    fn old_recoveries_fall_out_of_the_window() {
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        w.tick(0, &[1], "9", &none);
        assert_eq!(w.tick(10, &[], "9", &crashed_set(&[1])), Step::Recover { id: "9".into() });
        // The next crash — an hour later: this is no longer "in a row".
        let later = 10 + RECOVERY_WINDOW_SECS + 3000;
        w.tick(later, &[2], "9", &none);
        assert_eq!(w.tick(later + 10, &[], "9", &crashed_set(&[2])), Step::Recover { id: "9".into() });
    }

    fn alive(subject: &str, beats: &[Option<u64>]) -> HostTick {
        HostTick { subject: Some(subject.into()), beats: beats.to_vec(), ..HostTick::default() }
    }

    fn exited(subject: &str) -> HostTick {
        HostTick { subject: Some(subject.into()), exited: true, ..HostTick::default() }
    }

    #[test]
    fn host_crashes_are_restarted_twice_then_a_still_frame() {
        let mut w = HostWatch::default();
        let s = "video:/v.mp4";
        assert_eq!(w.tick(100, &alive(s, &[Some(99)])), HostStep::Nothing);
        assert_eq!(w.tick(110, &exited(s)), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed });
        assert_eq!(w.tick(120, &alive(s, &[Some(119)])), HostStep::Nothing);
        assert_eq!(w.tick(130, &exited(s)), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed });
        // The third crash in a row — a frame, not a crash loop.
        assert_eq!(w.tick(140, &exited(s)), HostStep::StillFrame { subject: s.into() });
        // This wallpaper was given up on: from now on we stay silent until the person
        // changes it.
        assert_eq!(w.tick(150, &exited(s)), HostStep::Nothing);
        // Another wallpaper — the count starts over.
        let other = "web:/page/index.html";
        assert_eq!(w.tick(160, &exited(other)), HostStep::Restart { subject: other.into(), why: HostTrouble::Crashed });
    }

    #[test]
    fn host_crashes_far_apart_are_not_in_a_row() {
        let mut w = HostWatch::default();
        let s = "swf:/a.swf";
        for k in 0..6u64 {
            let at = 100 + k * (RECOVERY_WINDOW_SECS + 1);
            assert_eq!(w.tick(at, &exited(s)), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed }, "падение {k}");
        }
    }

    #[test]
    fn nothing_of_ours_means_nothing_to_do() {
        let mut w = HostWatch::default();
        w.gpu_reset();
        w.woke(10);
        assert_eq!(w.tick(100, &HostTick::default()), HostStep::Nothing);
        // Events without our own wallpaper do not apply to the next wallpaper.
        assert_eq!(w.tick(110, &alive("video:/v.mp4", &[Some(109)])), HostStep::Nothing);
    }

    #[test]
    fn host_that_does_not_beat_after_sleep_is_hung() {
        let mut w = HostWatch::default();
        let s = "video:/v.mp4";
        assert_eq!(w.tick(100, &alive(s, &[Some(100)])), HostStep::Nothing);
        w.woke(1000);
        // Too early to judge: the screens are still coming back.
        assert_eq!(w.tick(1005, &alive(s, &[Some(100)])), HostStep::Nothing);
        // The period is over, the mark is still from before sleep — hung.
        assert_eq!(
            w.tick(1000 + RESUME_GRACE_SECS, &alive(s, &[Some(100)])),
            HostStep::Restart { subject: s.into(), why: HostTrouble::Hung }
        );

        // Checked in after sleep — alive, we do nothing.
        let mut w = HostWatch::default();
        w.tick(100, &alive(s, &[Some(100)]));
        w.woke(1000);
        assert_eq!(w.tick(1000 + RESUME_GRACE_SECS, &alive(s, &[Some(1003)])), HostStep::Nothing);
    }

    #[test]
    fn hang_is_not_judged_without_a_beat_or_while_paused() {
        let s = "video:/v.mp4";
        // A host of an older build (no mark) — not judged at all.
        let mut w = HostWatch::default();
        w.woke(10);
        assert_eq!(w.tick(10_000, &alive(s, &[None])), HostStep::Nothing);
        // Frozen by the load supervisor — the silence is legitimate.
        let mut w = HostWatch::default();
        let mut t = alive(s, &[Some(1)]);
        t.paused = true;
        assert_eq!(w.tick(10_000, &t), HostStep::Nothing);
        // Without sleep the mark becomes stale only after a long period.
        let mut w = HostWatch::default();
        assert_eq!(w.tick(100 + HEARTBEAT_STALE_SECS, &alive(s, &[Some(100)])), HostStep::Nothing);
        assert_eq!(
            w.tick(101 + HEARTBEAT_STALE_SECS, &alive(s, &[Some(100)])),
            HostStep::Restart { subject: s.into(), why: HostTrouble::Hung }
        );
    }

    #[test]
    fn gpu_reset_recreates_a_living_host() {
        let mut w = HostWatch::default();
        let s = "web:https://example.com";
        w.tick(100, &alive(s, &[Some(100)]));
        w.gpu_reset();
        assert_eq!(w.tick(101, &alive(s, &[Some(101)])), HostStep::Restart { subject: s.into(), why: HostTrouble::GpuReset });
        // The reset is handled once.
        assert_eq!(w.tick(102, &alive(s, &[Some(102)])), HostStep::Nothing);
    }

    #[test]
    fn extension_host_is_dead_only_after_it_was_seen_alive() {
        let s = "video:/v.mp4";
        let ext = |beats: &[Option<u64>]| HostTick {
            subject: Some(s.into()),
            beats: beats.to_vec(),
            by_heartbeat_only: true,
            ..HostTick::default()
        };
        let mut w = HostWatch::default();
        // The extension has not started the host yet (or a re-login is needed) — this
        // is not a crash.
        assert_eq!(w.tick(100, &ext(&[])), HostStep::Nothing);
        assert_eq!(w.tick(110, &ext(&[])), HostStep::Nothing);
        assert_eq!(w.tick(120, &ext(&[Some(120)])), HostStep::Nothing);
        // It was alive — it vanished.
        assert_eq!(w.tick(130, &ext(&[])), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed });
        // The new host has not checked in yet — we wait again rather than restart in
        // a loop.
        assert_eq!(w.tick(140, &ext(&[])), HostStep::Nothing);
    }

    #[test]
    fn crash_budget_is_shared_by_both_watchers() {
        let mut b = CrashBudget::default();
        assert!(b.spend(0, "a", 1, 600));
        assert!(!b.spend(10, "a", 1, 600));
        assert!(b.is_given_up("a"));
        b.forget_if_changed("a");
        assert!(b.is_given_up("a"));
        b.forget_if_changed("b");
        assert!(!b.is_given_up("a"));
    }

    #[test]
    fn host_signals_are_parsed() {
        let woke = "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)";
        let sleeping = "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)";
        assert_eq!(parse_prepare_for_sleep(woke), Some(false));
        assert_eq!(parse_prepare_for_sleep(sleeping), Some(true));
        assert_eq!(parse_prepare_for_sleep("/org/freedesktop/login1: org.freedesktop.login1.Manager.SessionNew ('3',)"), None);
        assert!(parse_drm_wedged("WEDGED=none"));
        assert!(parse_drm_wedged("  WEDGED=rebind,bus-reset"));
        assert!(!parse_drm_wedged("ACTION=change"));
        assert!(!parse_drm_wedged("KERNEL[12.3] change /devices/x/drm/card1 (drm)"));

        assert_eq!(heartbeat_path("/run/user/1000/", 42), "/run/user/1000/rexon-wp-heartbeat-42");
        assert_eq!(heartbeat_pid("rexon-wp-heartbeat-42"), Some(42));
        assert_eq!(heartbeat_pid("rexon-wp-heartbeat-"), None);
        assert_eq!(heartbeat_pid("rexon-wp-frame.rgba"), None);
        assert_eq!(parse_heartbeat("123\n"), Some(123));
        assert_eq!(parse_heartbeat(""), None);

        let mut known = None;
        // What was there before the watchdog started is not an event.
        assert!(!new_gpu_dump(&mut known, &[("devcd1".into(), true)]));
        assert!(!new_gpu_dump(&mut known, &[("devcd1".into(), true)]));
        // A new record not from the GPU — not our case.
        assert!(!new_gpu_dump(&mut known, &[("devcd1".into(), true), ("devcd2".into(), false)]));
        assert!(new_gpu_dump(&mut known, &[("devcd3".into(), true)]));
    }

    #[test]
    fn still_frame_is_looked_for_next_to_the_wallpaper() {
        let project = r#"{"title":"x","preview":"preview.gif","type":"web"}"#;
        let c = still_frame_candidates("/lib/123/index.html", Some(project));
        assert_eq!(c[0], "/lib/123/preview.gif");
        assert!(c.contains(&"/lib/123/preview.jpg".to_string()));
        assert_eq!(still_frame_candidates("file:///lib/7/Page.HTML", None)[0], "/lib/7/preview.jpg");
        assert_eq!(still_frame_candidates("/flash/a.swf", None)[0], "/flash/preview.jpg");
        assert_eq!(still_frame_candidates("/lib/123/", None)[0], "/lib/123/preview.jpg");
        assert!(still_frame_candidates("https://example.com", None).is_empty());
        // We do not take a foreign path into the preview (escaping the directory).
        let evil = r#"{"preview":"../../etc/x.png"}"#;
        assert_eq!(still_frame_candidates("/lib/1/index.html", Some(evil))[0], "/lib/1/preview.jpg");
        assert_eq!(still_frame_dir("/lib/1/index.html").as_deref(), Some("/lib/1"));
    }

    #[test]
    fn system_answers_are_parsed() {
        assert!(is_renderer_exe("/home/u/.local/share/waywallen/plugins/x/bin/waywallen-wescene-renderer"));
        assert!(is_renderer_exe("waywallen-weweb-renderer"));
        assert!(!is_renderer_exe("/tmp/.mount_x/usr/bin/waywallen"));
        assert!(!is_renderer_exe("rexon-wallpaper-host"));
        assert_eq!(busctl_string("s \"16352\"\n"), "16352");
        assert_eq!(busctl_string("s \"\"\n"), "");
        assert_eq!(busctl_string("u 5\n"), "");
        let entry = r#"{"COREDUMP_PID":"729862","COREDUMP_EXE":"/home/u/p/bin/waywallen-wescene-renderer","COREDUMP_SIGNAL_NAME":"SIGABRT"}"#;
        assert!(journal_names_renderer_crash(entry));
        assert!(!journal_names_renderer_crash(r#"{"COREDUMP_PID":"5","COREDUMP_EXE":"/usr/bin/firefox"}"#));
        assert!(!journal_names_renderer_crash(""));
    }
}
