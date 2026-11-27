// Wallpaper audio supervisor: live application of the settings and muting
// depending on the situation.
//
// What appeared here and why. Before this, wallpaper sound was set ONCE,
// with command line options when the host started, and changed only
// together with restarting the whole process: the user moved a setting —
// nothing happened on screen until the wallpaper was set again. There was
// no muting depending on the situation at all, so wallpapers played over
// games and over films.
//
// The division of duties is the same as in the whole host: WHAT the
// settings mean and WHAT to do in a given situation is decided by the core
// (`rexon-core::wallpaper_audio`); here there is only I/O: reading the
// settings file, polling the window manager and the sound system,
// delivering the decision to QML. There are deliberately no rules or
// numbers of our own in this file — otherwise "wallpaper volume" would
// again start meaning different things in two places.
//
// Live application is done WITHOUT a separate channel: the host watches the
// same `~/.config/rexon/settings.json` that the settings window writes.
// No socket, no state file, no second format — "saved" and "heard" cannot
// diverge, because there is one source (rule #12).
//
// Warning: watching is started by TWO host modes, and they share ONE loop
// (16.09.2026). Video lives in QML and gets the decision through the
// properties of the cxx-qt object below; Flash (`--mode swf`) is drawn by
// Ruffle through wgpu, has no QML at all, and takes the decision with an
// ordinary closure — [`watch`]. It is the loop that is shared
// ([`watch_loop`]): a copy of "re-read the settings, poll the windows, ask
// the core" of its own for the second mode would mean that "mute when a
// full-screen window plays on top" would one day diverge between video and
// Flash.

use cxx_qt::{CxxQtType, Threading};

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rexon_core::wallpaper_audio::{
    gamemode_client_count_argv, is_gamescope_comm, logind_session_argv, other_audio_playing,
    parse_dbus_bool, parse_dbus_int, parse_logind_session, parse_screensaver_active,
    sysfs_on_battery, upower_on_battery_argv, Action, AudioSettings, Conditions, Decision,
    // Signs by which OUR stream is seen in the sound system: it must not pass
    // for "foreign audio" and mute itself. There is deliberately no list of
    // our own here — the table is one for the product and lives in the core
    // (rule #12), otherwise Flash and video would recognize themselves by
    // different signs.
    OWN_STREAM_MARKERS as OWN_STREAM_NAMES,
};

/// cxx-qt bridge: wallpaper audio state for QML.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// Live state of the wallpaper audio.
        ///
        /// QML holds bindings on these properties, so a change of a setting or of
        /// the situation reaches the player by itself, without a restart.
        #[qobject]
        #[qml_element]
        // Whether to keep the audio stream open. Warning: exactly "there is a
        // stream / there is no stream", not "volume 0": a silent stream hangs in
        // the mixer as a separate slider for the whole session and does not let
        // the sound card sleep, and above all — with it "plays quietly" cannot be
        // told from "does not play at all".
        #[qproperty(bool, audio_on, cxx_name = "audioOn")]
        #[qproperty(bool, paused)]
        #[qproperty(f64, volume)]
        // Why it is so right now, in words. Not for looks: in the self-test this
        // is the only way to tell "silence because it was asked for" from
        // "silence because it did not start".
        #[qproperty(QString, reason)]
        type WallpaperAudio = super::WallpaperAudioRust;
    }

    extern "RustQt" {
        /// Start watching. The screen size is needed to tell a full-screen window
        /// from an ordinary one — the wallpaper window is the whole screen.
        #[qinvokable]
        fn start(self: Pin<&mut WallpaperAudio>, screen_w: i32, screen_h: i32);

        /// The script that delivers the current decision (pause, volume) to the
        /// wallpaper's web page. The script text is the core's
        /// (`rexon-core::wallpaper::web_state_js`); QML has no JS of its own.
        #[qinvokable]
        #[cxx_name = "webStateScript"]
        fn web_state_script(self: &WallpaperAudio) -> QString;
    }

    impl cxx_qt::Threading for WallpaperAudio {}
    impl cxx_qt::Constructor<()> for WallpaperAudio {}
}

/// How often to re-read the situation.
///
/// Half a second is a compromise that is visible to the eye: an edit of the
/// volume slider responds "at once", while polling the window manager and
/// the sound system costs two short processes per second. There is no
/// reason to go lower, and higher makes muting on game start come late.
const TICK: Duration = Duration::from_millis(500);

/// Every how many ticks to ask about the battery, games and the session.
///
/// These circumstances do not change within half a second: people unplug,
/// start a game and switch sessions — a difference of a second and a half
/// is invisible to the eye, while three extra processes on every tick are
/// visible in the load.
const SLOW_EVERY: u32 = 4;

/// How long to wait for an external polling command.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct WallpaperAudioRust {
    audio_on: bool,
    paused: bool,
    volume: f64,
    reason: cxx_qt_lib::QString,
    started: bool,
    stop: Option<Arc<AtomicBool>>,
}

impl Drop for WallpaperAudioRust {
    fn drop(&mut self) {
        // The watching thread must not outlive the window: it holds a handle to
        // the object and `queue` no longer goes through on a dead object, but it
        // would keep spawning polling processes.
        if let Some(flag) = &self.stop {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

impl cxx_qt::Initialize for qobject::WallpaperAudio {
    fn initialize(self: Pin<&mut Self>) {
        // The initial state is "sound as written in the settings, not muted by
        // anything". Otherwise the wallpaper would be silent for the first half
        // second, and on a short self-test run — for the whole run.
        let settings = load_settings();
        let decision = settings.decide(&Conditions::default());
        let mut this = self;
        this.as_mut().apply(decision, "initial settings");
    }
}

impl qobject::WallpaperAudio {
    /// Spread the decision over the properties. The setters are generated by
    /// cxx-qt and stay silent by themselves when the value did not change.
    fn apply(mut self: Pin<&mut Self>, d: Decision, why: &str) {
        self.as_mut().set_audio_on(d.audio_on);
        self.as_mut().set_paused(d.paused);
        self.as_mut().set_volume(d.volume01);
        self.as_mut().set_reason(cxx_qt_lib::QString::from(why));
    }

    /// The decision for the web page — see the bridge.
    pub fn web_state_script(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&rexon_core::wallpaper::web_state_js(
            self.rust().paused,
            self.rust().volume,
        ))
    }

    /// Start watching. A repeated call does nothing.
    pub fn start(mut self: Pin<&mut Self>, screen_w: i32, screen_h: i32) {
        if self.started {
            return;
        }
        self.as_mut().rust_mut().started = true;
        let stop = Arc::new(AtomicBool::new(false));
        self.as_mut().rust_mut().stop = Some(stop.clone());
        let handle = self.as_ref().get_ref().qt_thread();
        let (w, h) = (i64::from(screen_w), i64::from(screen_h));
        // An ordinary thread, not a Qt timer: polling goes to external programs,
        // and doing that in the render thread means jerking the video on every
        // tick.
        let spawned = std::thread::Builder::new()
            .name("rexon-wp-audio".into())
            .spawn(move || {
                watch_loop(stop, w, h, |decision, why| {
                    // `Err` means the object died (the window is being closed): there is
                    // nothing to watch any more, the loop exits by itself.
                    handle
                        .queue(move |object| object.apply(decision, &why))
                        .is_ok()
                })
            });
        if let Err(e) = spawned {
            rexon_core::log_warn!(
                "wallpaper-host",
                "REXON-WPH-AUDIO watching did not start ({e}) — the wallpaper sound \
                 stays as the settings had it at start"
            );
        }
    }
}

/// The watching loop. It lives in its own thread; only a ready decision
/// goes out, and only when it changed.
///
/// `deliver` returns `false` when there is nobody left to deliver to — then
/// the loop exits. Neither Qt nor QML is mentioned here on purpose: the
/// Flash mode, which has no QML, lives on the same loop (see [`watch`]).
fn watch_loop(
    stop: Arc<AtomicBool>,
    screen_w: i64,
    screen_h: i64,
    mut deliver: impl FnMut(Decision, String) -> bool,
) {
    let path = rexon_core::settings::settings_file();
    let backend = window_backend();
    let mut settings = load_settings();
    let mut stamp = mtime(&path);
    let mut last: Option<Decision> = None;
    let mut slow = SlowFacts::default();
    let mut tick: u32 = 0;
    while !stop.load(Ordering::SeqCst) {
        // The settings are re-read only when the file really changed: parsing JSON
        // every half second for nothing is of no use to anyone.
        let now = mtime(&path);
        if now != stamp {
            stamp = now;
            settings = load_settings();
        }
        // Slow circumstances — on the first tick (the decision must be right at
        // once, not two seconds later) and then once every SLOW_EVERY.
        if tick % SLOW_EVERY == 0 {
            slow = probe_slow(&settings);
        }
        tick = tick.wrapping_add(1);
        let conditions = probe(&settings, backend, screen_w, screen_h, &slow);
        let decision = settings.decide(&conditions);
        if last != Some(decision) {
            last = Some(decision);
            let why = explain(&settings, &conditions, &decision);
            if !deliver(decision, why) {
                return;
            }
        }
        std::thread::sleep(TICK);
    }
}

/// A live subscription to wallpaper audio decisions for the mode WITHOUT
/// QML.
///
/// The same loop, the same settings and the same core decision as for
/// video — otherwise "wallpaper volume" would again start meaning different
/// things for the two modes (rule #12). `on_change` is called from a
/// separate thread and only on a CHANGE of the decision; the first decision
/// arrives at once, there is no need to wait half a second.
///
/// The screen size is the same as for video: the SMALLEST of the connected
/// ones. Why exactly the smallest is analysed in `WallpaperVideo.qml`
/// (`audioProbeSize`): muting needlessly is a trifle, playing over a film
/// is a disaster.
///
/// Watching lives as long as the returned [`AudioWatch`] does.
pub fn watch(
    screen_w: i64,
    screen_h: i64,
    mut on_change: impl FnMut(Decision, String) + Send + 'static,
) -> Result<AudioWatch, std::io::Error> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("rexon-wp-audio".into())
        .spawn(move || {
            watch_loop(flag, screen_w, screen_h, |decision, why| {
                on_change(decision, why);
                true
            })
        })?;
    Ok(AudioWatch { stop })
}

/// Live watching of the wallpaper audio. Dropped — watching stopped.
pub struct AudioWatch {
    stop: Arc<AtomicBool>,
}

impl Drop for AudioWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Wallpaper audio settings right now — so that the mode without QML starts
/// from what the user has written down, not from the defaults.
pub fn settings_now() -> AudioSettings {
    load_settings()
}

/// Settings from the shared file. No file — the core's defaults, not
/// silence.
fn load_settings() -> AudioSettings {
    AudioSettings::from_map(&rexon_core::settings::load_settings(
        &rexon_core::settings::settings_file(),
    ))
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// What to ask about windows with in this session (the core decides — the
/// table is one for the product).
fn window_backend() -> &'static str {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    // The environment — with the same answer as for the wallpaper engine (rule
    // #12): there used to be a chain of substrings of our own here, and its
    // order gave `Budgie:GNOME` as gnome.
    let de = rexon_core::wallpaper_engine::detect_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
    );
    let session = env("XDG_SESSION_TYPE");
    rexon_core::wallpaper_engine::fullscreen_backend(&de, &session)
}

/// Circumstances polled less often than windows (see [`SLOW_EVERY`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SlowFacts {
    on_battery: bool,
    gamemode: bool,
    session_inactive: bool,
    /// logind's `LockedHint` — a lock that is visible even where there is no
    /// `org.freedesktop.ScreenSaver` screensaver (wlroots desktops).
    session_locked: bool,
}

/// Polling the battery, games and the session. Only what the user assigned
/// an action to is asked.
fn probe_slow(settings: &AudioSettings) -> SlowFacts {
    let a = settings.actions;
    let mut f = SlowFacts::default();
    if a.on_battery != Action::None {
        f.on_battery = on_battery();
    }
    if a.gamemode != Action::None {
        f.gamemode = game_running();
    }
    if a.session_matters() {
        let id = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        if let Some(out) = run(&logind_session_argv(&id)) {
            (f.session_inactive, f.session_locked) = parse_logind_session(&out);
        }
    }
    f
}

/// Whether we run on battery: UPower, and if it is missing or silent — the
/// battery state in `/sys` (the same core rule as in UPower).
fn on_battery() -> bool {
    if let Some(v) = run(&upower_on_battery_argv()).and_then(|o| parse_dbus_bool(&o)) {
        return v;
    }
    let dir = rexon_core::wallpaper_engine::POWER_SUPPLY_DIR;
    let statuses: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("BAT"))
        .filter_map(|e| std::fs::read_to_string(e.path().join("status")).ok())
        .collect();
    sysfs_on_battery(&statuses)
}

/// Whether a game is running: game mode was requested (`ClientCount` > 0)
/// or gamescope of the same user is running.
fn game_running() -> bool {
    let gamemode = run(&gamemode_client_count_argv())
        .and_then(|o| parse_dbus_int(&o))
        .is_some_and(|n| n > 0);
    gamemode || gamescope_running()
}

/// Whether gamescope is running as the same user. Only reading `/proc`: no
/// process is started.
fn gamescope_running() -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(me) = std::fs::metadata("/proc/self") else {
        return false;
    };
    let uid = me.uid();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let mine = e.metadata().map(|m| m.uid() == uid).unwrap_or(false);
        mine && std::fs::read_to_string(e.path().join("comm"))
            .map(|c| is_gamescope_comm(&c))
            .unwrap_or(false)
    })
}

/// Polling the situation. Only what really affects the decision is asked:
/// if the user did not ask to mute on foreign audio, nobody pokes the sound
/// system, and without "window" actions — the window manager.
fn probe(
    settings: &AudioSettings,
    backend: &str,
    screen_w: i64,
    screen_h: i64,
    slow: &SlowFacts,
) -> Conditions {
    let mut c = Conditions::default();
    if settings.actions.windows_matter() {
        c = probe_windows(backend, screen_w, screen_h);
    }
    if settings.actions.session_locked != Action::None {
        // The screensaver — as before (KDE and everyone who holds one), plus
        // logind's `LockedHint`: it is also set by the lockers of wlroots desktops,
        // which have no screensaver on the bus.
        c.session_locked = slow.session_locked
            || run(&rexon_core::wallpaper_audio::screensaver_argv())
                .map(|out| parse_screensaver_active(&out))
                .unwrap_or(false);
    }
    c.session_inactive = slow.session_inactive;
    c.gamemode = slow.gamemode;
    c.on_battery = slow.on_battery;
    if settings.mute_when_other_audio {
        c.other_audio = run(&rexon_core::wallpaper_audio::sink_inputs_argv())
            .map(|out| other_audio_playing(&out, &OWN_STREAM_NAMES))
            .unwrap_or(false);
    }
    c
}

/// Window state. The parsing is the core's in all cases; here there is only
/// starting the commands.
fn probe_windows(backend: &str, screen_w: i64, screen_h: i64) -> Conditions {
    use rexon_core::wallpaper_engine as we;
    match backend {
        "kdotool" => {
            let Some(id) = run(&we::kdotool_active_argv()) else {
                return Conditions::default();
            };
            let id = id.trim();
            if id.is_empty() {
                return Conditions::default();
            }
            let mut argv: Vec<String> =
                vec!["qdbus6".into(), we::KWIN_WINDOW_INFO[0].into(),
                     we::KWIN_WINDOW_INFO[1].into(), we::KWIN_WINDOW_INFO[2].into()];
            argv.push(id.to_string());
            let Some(info) = run(&argv) else {
                return Conditions::default();
            };
            rexon_core::wallpaper_audio::kwin_window_conditions(
                &info, screen_w, screen_h, &own_pids(),
            )
        }
        "hyprctl" => {
            let Some(out) = run(&we::hyprctl_active_argv()) else {
                return Conditions::default();
            };
            // Our own wallpaper window does not count as a "window on top of the
            // wallpaper" — the same filtering as on X11 and on KWin (rule #12).
            if we::hyprctl_window_is_wallpaper_layer(&out, &own_pids()) {
                return Conditions::default();
            }
            let full = we::parse_hyprctl_fullscreen(&out);
            Conditions {
                any_window: !out.trim().is_empty(),
                focused: !out.trim().is_empty(),
                fullscreen: full,
                maximized: full,
                ..Conditions::default()
            }
        }
        "gdbus" => {
            let full = run(&we::gdbus_fullscreen_argv())
                .map(|o| we::parse_gdbus_fullscreen(&o))
                .unwrap_or(false);
            Conditions {
                any_window: full,
                focused: full,
                fullscreen: full,
                maximized: full,
                ..Conditions::default()
            }
        }
        "xdotool" => {
            // Warning: geometry alone is not enough: the window manager also calls the
            // desktop ITSELF the active window, and our wallpaper is the desktop over
            // the whole screen — otherwise "a full-screen window plays on top" would
            // always be true. What this geometry means is decided by the core; here
            // there is only starting the polling commands.
            let Some(out) = run(&we::xdotool_geometry_argv()) else {
                return Conditions::default();
            };
            rexon_core::wallpaper_audio::x11_window_conditions(
                &out,
                &format!("{screen_w}x{screen_h}"),
                &own_pids(),
                |id| run(&we::xprop_window_info_argv(id)),
            )
        }
        // There is no way to find out — the wallpaper plays as asked. A user will
        // not go fixing a wallpaper muted "just in case", they will decide there
        // is no sound.
        _ => Conditions::default(),
    }
}

/// Processes whose windows are the wallpaper.
///
/// Watching lives INSIDE the wallpaper host, so our own process is exactly
/// that: a window with this `_NET_WM_PID` draws the wallpaper rather than
/// covering it. This is a safeguard for when there is no way to ask the
/// window type; the main filtering goes by `_NET_WM_WINDOW_TYPE_DESKTOP`.
fn own_pids() -> Vec<i64> {
    vec![i64::from(std::process::id())]
}

/// Run a polling command and take its output.
///
/// Warning: the C locale (`utils::parsed_output_locale`): under a Russian
/// locale `pactl` translates field names, and parsing "is anyone else
/// playing" stops finding them.
fn run(argv: &[String]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let mut child = rexon_core::utils::parsed_output_locale(&mut std::process::Command::new(program))
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Why it is so right now — in one line, for the host log (`audioReason=`),
/// so in English and not translated.
fn explain(settings: &AudioSettings, c: &Conditions, d: &Decision) -> String {
    if !settings.enabled {
        return "wallpaper sound is off in the settings".into();
    }
    if d.paused {
        let a = settings.actions;
        return if c.session_inactive && a.session_inactive.pauses() {
            "session inactive — wallpaper paused".into()
        } else if c.session_locked && a.session_locked.pauses() {
            "session locked — wallpaper paused".into()
        } else if c.gamemode && a.gamemode.pauses() {
            "a game is running — wallpaper paused".into()
        } else if c.on_battery && a.on_battery.pauses() {
            "running on battery — wallpaper paused".into()
        } else if c.fullscreen {
            "fullscreen window — wallpaper paused".into()
        } else {
            "wallpaper paused by a setting".into()
        };
    }
    if !d.audio_on {
        if settings.mute_when_other_audio && c.other_audio {
            return "another program is playing sound".into();
        }
        if c.maximized {
            return "a window covers the wallpaper".into();
        }
        return "muted by a setting".into();
    }
    format!("playing, volume {}%", (d.volume01 * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn свой_поток_не_считается_чужим_звуком() {
        // The signs must cover what our stream is labelled with in the sound
        // system: otherwise the wallpaper mutes itself, and it looks like "the
        // sound just does not work".
        let ours = "Sink Input #1\n\tCorked: no\n\t\tnode.name = \"RexonVideoWallpaper\"\n";
        assert!(!other_audio_playing(ours, &OWN_STREAM_NAMES));
    }

    #[test]
    fn поток_flash_обоев_тоже_свой() {
        // ⭐ A live measurement on 16.09.2026: Flash goes to audio as an ALSA
        // client, and PipeWire's ALSA plugin labels the stream in its own words.
        // None of the previous names gave an exact match — Flash wallpapers muted
        // themselves, while the log said "another program is playing sound" on an
        // empty desktop. The strings below are taken from `pactl list sink-inputs`
        // verbatim.
        let flash = "Sink Input #1\n\tCorked: no\n\t\t                     application.name = \"PipeWire ALSA [rexon-wallpaper-host]\"\n\t\t                     node.name = \"alsa_playback.rexon-wallpaper-host\"\n\t\t                     media.name = \"ALSA Playback\"\n\t\t                     application.id = \"com.rexon.wallpaper\"\n";
        assert!(!other_audio_playing(flash, &OWN_STREAM_NAMES));

        // And the other way round: a foreign player must stay foreign.
        let alien = "Sink Input #1\n\tCorked: no\n\t\t                     application.name = \"PipeWire ALSA [mpv]\"\n\t\t                     node.name = \"alsa_playback.mpv\"\n";
        assert!(other_audio_playing(alien, &OWN_STREAM_NAMES));
    }

    #[test]
    fn бэкенд_опроса_берётся_из_ядра() {
        // There is no "what to ask with" table of our own here — only converting
        // the environment name into what the core already knows.
        assert_eq!(
            rexon_core::wallpaper_engine::fullscreen_backend("kde", "wayland"),
            "kdotool"
        );
        assert_eq!(
            rexon_core::wallpaper_engine::fullscreen_backend("kde", "x11"),
            "xdotool"
        );
    }

    #[test]
    fn неизвестное_окружение_не_оставляет_без_звука() {
        // There is no way to learn about windows — the wallpaper must play, not
        // stay silent.
        assert_eq!(probe_windows("", 1920, 1080), Conditions::default());
    }

    #[test]
    fn объяснение_различает_тишину_по_просьбе_и_по_обстановке() {
        let off = AudioSettings {
            enabled: false,
            ..AudioSettings::default()
        };
        let c = Conditions::default();
        assert!(explain(&off, &c, &off.decide(&c)).contains("is off"));

        let on = AudioSettings::default();
        let loud = Conditions {
            other_audio: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &loud, &on.decide(&loud)).contains("another program"));

        let full = Conditions {
            fullscreen: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &full, &on.decide(&full)).contains("fullscreen"));

        let battery = Conditions {
            on_battery: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &battery, &on.decide(&battery)).contains("battery"));
        let game = Conditions {
            gamemode: true,
            on_battery: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &game, &on.decide(&game)).contains("game"));
        let away = Conditions {
            session_inactive: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &away, &on.decide(&away)).contains("inactive"));
        assert!(explain(&on, &c, &on.decide(&c)).contains("50%"));
    }
}
