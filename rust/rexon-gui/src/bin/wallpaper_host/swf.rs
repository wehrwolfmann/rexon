// "Flash" mode of the wallpaper host: the SWF clip is drawn by Ruffle
// through wgpu (the `rexon-swf` crate), not QML — so the mode does not
// enter `run_qml` and returns the code by itself, from the same core table
// (`wallpaper_engine::host_exit`).
//
// Whose decisions are where:
//   * how to place the window (layer / X11 / frame stream) — the shared
//     `HostEnv::read`, the same decision as for video and web; the Qt
//     platform for it is set by the shared `main.rs` between [`plan`] and
//     [`run`] — as for the QML modes;
//   * the frame file contract (size, rate, the generation counter path,
//     permissions) — the core functions `rexon-core::wallpaper_mmap` and
//     `wallpaper::mmap_size`;
//   * the layer — `layer_shell::apply_spec` with `LayerSpec::WALLPAPER`, X11
//     — `wallpaper_host::apply_x11_desktop_layer`;
//   * rendering, pace, pause, sound — `rexon-swf`; here only the Qt event
//     loop.
//
// The load supervisor's pause is SIGSTOP/SIGCONT of the process, as for
// video: after continuing, the clip does not fast-forward (the time step
// is limited in `rexon-swf`).
//
// Warning: sound (16.09.2026). Before this Flash went silent: the volume
// came as a start option, the mode did not watch the settings, and the
// Ruffle scale is non-linear — half of the slider sounded ten times quieter
// than the same half for video. Now everything is as for video and FROM THE
// SAME places:
//   * WHAT the settings mean and what to do in a given situation — the core
//     `rexon-core::wallpaper_audio` (the `wallpaper_audio_*` keys in
//     `settings.json`, one slider for both kinds of wallpaper);
//   * WHO watches the settings file and polls the windows and the sound
//     system — the shared loop `rexon-gui::wallpaper_audio::watch`, the same
//     one that feeds the video wallpaper QML;
//   * converting the volume into the Ruffle scale — `rexon_swf::ruffle_volume`,
//     and nowhere else.
// Here there is only delivering the decision to the display and honest
// complaints when there will be no sound.

use std::path::PathBuf;

use rexon_core::wallpaper_engine::{host_exit, swf_missing_message};
use rexon_gui::wallpaper_host::{parse_args, Cli, USAGE};

use crate::HostEnv;

/// Log line tag of the mode.
const TAG: &str = "REXON-SWF";

/// Everything the Flash display needs, computed BEFORE the first Qt call.
pub struct SwfPlan {
    pub cli: Cli,
    pub path: PathBuf,
    pub env: HostEnv,
    /// The display method from the shared decision — by it `main.rs` sets the
    /// Qt platform.
    pub placement: &'static str,
}

/// Option parsing and honest refusals before any Qt; `Err` is a ready exit
/// code.
pub fn plan(argv: &[String]) -> Result<SwfPlan, i32> {
    // A Vulkan driver failure or the library itself must not leave the
    // customer a "the program crashed" window: the kernel does not call the
    // dump handler for a non-dumpable process.
    // SAFETY: prctl with PR_SET_DUMPABLE does not touch the process memory.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };

    let cli = match parse_args(argv) {
        Ok(cli) => cli,
        Err(message) if message == USAGE => {
            eprintln!("{message}");
            return Err(host_exit::OK);
        }
        Err(message) => {
            // The parsing is shared with video, and its "no video file given" is
            // untrue here.
            let message = if message.starts_with("не указан файл") {
                "не указан файл Flash (.swf)".to_string()
            } else {
                message
            };
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {message}");
            return Err(host_exit::NOTHING_TO_SHOW);
        }
    };
    let path = PathBuf::from(&cli.path);
    if !path.is_file() {
        rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {}", swf_missing_message(&cli.path));
        return Err(host_exit::NOTHING_TO_SHOW);
    }
    // Warning: BEFORE the first audio operation: the sound client reads the
    // stream properties when it creates its context, that is, when the output
    // is opened in `rexon-swf`. The same list as for video (`video.rs`) —
    // otherwise the wallpaper gets into the sound system as a nameless client,
    // and the audio supervisor takes our own stream for "another program" and
    // mutes us.
    for (name, value) in rexon_core::wallpaper_audio::stream_env() {
        if std::env::var_os(name).is_none() {
            // SAFETY: called from main before the Qt application is created and before
            // any threads — there are no environment readers yet.
            unsafe { std::env::set_var(name, &value) };
        }
    }
    let env = HostEnv::read(cli.windowed);
    let placement = env.placement;
    Ok(SwfPlan { cli, path, env, placement })
}

/// Display until a signal (or until the end of the self-test). The Qt
/// platform is already set.
pub fn run(plan: SwfPlan) -> i32 {
    #[cfg(feature = "swf")]
    {
        imp::run(plan)
    }
    #[cfg(not(feature = "swf"))]
    {
        let _ = plan;
        rexon_core::log_error!(
            "wallpaper-host",
            "{TAG}-ERROR этот хост обоев собран без Flash (фича swf крейта rexon-gui) — \
             пересоберите Rexon со стандартным набором возможностей"
        );
        host_exit::NOTHING_TO_SHOW
    }
}

#[cfg(feature = "swf")]
mod imp {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use cxx_qt_lib::{QGuiApplication, QString};
    use raw_window_handle as rwh;
    use rexon_core::wallpaper_audio::Decision;
    use rexon_core::wallpaper_engine::host_exit;
    use rexon_core::wallpaper_mmap;
    use rexon_gui::layer_shell::{apply_spec, pin_to_window_screen, LayerSpec};
    use rexon_gui::wallpaper_host::{
        apply_x11_desktop_layer, keep_alive_without_windows, prepare_x11_desktop_layer, swf_window,
        verdict, PLACEMENT_LAYER, PLACEMENT_OFFSCREEN, PLACEMENT_X11,
    };
    use rexon_swf::{Error, GpuContext, MmapSpec, OutputTarget, Settings, Wallpaper};

    use super::{SwfPlan, TAG};

    /// Name of the wallpaper layer for ordering within the background at the
    /// compositor.
    const LAYER_SCOPE: &str = "rexon-swf-wallpaper";

    /// How long to wait for the compositor to show the window.
    const EXPOSE_TIMEOUT: Duration = Duration::from_secs(10);

    /// How long to play before saying "the clip has no sound".
    ///
    /// Sound in an SWF does not necessarily start from the first frame, and a
    /// hasty complaint would be a lie the other way round. Five seconds is
    /// noticeably longer than the intro of an ordinary Flash splash and still
    /// "at once" by human standards.
    const SILENT_VERDICT_AFTER: Duration = Duration::from_secs(5);

    /// Live state of the wallpaper audio in Flash mode.
    ///
    /// The decision is entirely computed by the core and delivered by the
    /// shared watching loop; here there is only moving it into the display and
    /// honest complaints.
    struct AudioLive {
        /// The last decision that arrived. The display loop takes it and resets it:
        /// delivery comes from another thread, while the display may be touched
        /// only from its own.
        inbox: Arc<Mutex<Option<(Decision, String)>>>,
        /// While it lives, watching works.
        _watch: rexon_gui::wallpaper_audio::AudioWatch,
        /// The volume is pinned with the `--volume` option and does not follow the
        /// setting (the same agreement as for video: a manual run sounds as
        /// requested).
        fixed_volume: Option<f64>,
        verbose: bool,
        /// The clip — for the complaint "sound was requested, but there is none".
        movie: String,
        started: Instant,
        /// We say about a silent clip once per run.
        warned_silent: bool,
    }

    impl AudioLive {
        /// Start watching. `screen` is the smallest screen of the session: the core
        /// compares foreign windows with it (analysis in `wallpaper_audio::watch`).
        fn start(
            screen: (i64, i64),
            fixed_volume: Option<f64>,
            verbose: bool,
            movie: String,
        ) -> Option<Self> {
            let inbox: Arc<Mutex<Option<(Decision, String)>>> = Arc::new(Mutex::new(None));
            let sink = inbox.clone();
            match rexon_gui::wallpaper_audio::watch(screen.0, screen.1, move |d, why| {
                *sink.lock().unwrap() = Some((d, why));
            }) {
                Ok(watch) => Some(Self {
                    inbox,
                    _watch: watch,
                    fixed_volume,
                    verbose,
                    movie,
                    started: Instant::now(),
                    warned_silent: false,
                }),
                Err(e) => {
                    // The wallpaper must play, not stay silent: without watching, the sound
                    // simply stays as the settings were when it was caught.
                    rexon_core::log_warn!(
                        "wallpaper-host",
                        "{TAG}-WARN наблюдение за звуком не поднялось ({e}) — громкость \
                         останется той, что записана в настройках"
                    );
                    None
                }
            }
        }

        /// Deliver the decision to the display. Called from the display loop on
        /// every iteration.
        fn apply(&mut self, wp: &mut Wallpaper) {
            let taken = self.inbox.lock().unwrap().take();
            if let Some((decision, why)) = taken {
                if self.verbose {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-AUDIO {why}");
                }
                // Pause before sound: `set_paused` touches the audio output by itself, and
                // the order "pause first, then sound" leaves the last word to the core's
                // decision.
                wp.set_paused(decision.paused);
                wp.set_audio(
                    decision.audio_on,
                    self.fixed_volume.unwrap_or(decision.volume01),
                );
            }
            self.check_silent(wp);
        }

        /// "The clip is silent" is a separate complaint, and it is not nitpicking.
        ///
        /// A silent SWF by itself looks no different from a sounding one: the clip
        /// runs, the device is open, the volume is as requested. Without this line
        /// someone who set a Flash wallpaper with sound would see "the volume
        /// works" and would not understand why it is quiet.
        fn check_silent(&mut self, wp: &Wallpaper) {
            if self.warned_silent || self.started.elapsed() < SILENT_VERDICT_AFTER {
                return;
            }
            let state = wp.audio_state();
            // Asking makes sense only when the sound SHOULD have been playing: the
            // output is open, the volume is not zero, the device came for samples.
            // Otherwise the silence is explained by the request, not by the clip.
            if !state.available || !state.on || state.volume01 <= 0.0 || state.callbacks == 0 {
                return;
            }
            if state.peak > 0.0 {
                self.warned_silent = true;
                return;
            }
            self.warned_silent = true;
            rexon_core::log_warn!(
                "wallpaper-host",
                "{TAG}-NOAUDIO звук включён, но за {} с ролик не издал ни звука — \
                 похоже, звуковой дорожки в нём нет: {}",
                SILENT_VERDICT_AFTER.as_secs(),
                self.movie
            );
        }
    }

    /// Exit code for a library error — from the core table.
    fn exit_code(e: &Error) -> i32 {
        match e {
            Error::MovieLoad(_) => host_exit::SWF_MOVIE_FAILED,
            Error::NoAdapter(_) | Error::Surface(_) | Error::Render(_) | Error::GpuLost(_) => {
                host_exit::SWF_NO_GPU
            }
            Error::Output(_) => host_exit::FRAMES_FAILED,
            Error::NoOutputs | Error::InvalidSize { .. } | Error::OutputIndex(_) => {
                host_exit::NOTHING_TO_SHOW
            }
        }
    }

    pub fn run(plan: SwfPlan) -> i32 {
        let SwfPlan { cli, path, env, .. } = plan;
        let cli = &cli;
        let path: &Path = &path;
        let mut settings = Settings::new(path);
        // Flash wallpapers have sound by default, like video. The `--no-audio`
        // option remains a veto: it comes from whoever started the wallpaper,
        // while the setting comes from the user, and neither silently cancels the
        // other. After that the sound lives by itself: the volume and "play/stay
        // silent" come from the settings through watching (`AudioLive`).
        let audio_allowed = cli.audio != Some(false);
        let audio_settings = rexon_gui::wallpaper_audio::settings_now();
        settings.audio_volume = audio_allowed
            .then(|| cli.volume.unwrap_or_else(|| audio_settings.volume01()) as f32);
        let selftest = (cli.selftest_ms > 0).then(|| Duration::from_millis(cli.selftest_ms as u64));
        if cli.verbose {
            rexon_core::log_info!(
                "wallpaper-host",
                "{TAG}-START файл={} способ={} окно={} звук={:?} закреплённая_громкость={:?}",
                path.display(),
                env.placement,
                cli.windowed,
                settings.audio_volume,
                cli.volume
            );
        }
        // Watching is started only where sound is possible at all: without it
        // there is no reason to poll the windows and the sound system every half
        // second.
        let live = |screen: (i64, i64)| {
            audio_allowed.then(|| {
                AudioLive::start(screen, cli.volume, cli.verbose, path.display().to_string())
            })
            .flatten()
        };

        if env.placement == PLACEMENT_OFFSCREEN {
            // GNOME/Cinnamon Wayland: there is no window at all, Qt is not needed.
            let Some(frame) = env.mmap_output.clone() else {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR офскрин-режим без REXON_WP_OUTPUT: кадры некуда писать");
                return verdict(host_exit::NOTHING_TO_SHOW);
            };
            settings.max_fps = 1000.0 / wallpaper_mmap::headless_tick_ms(env.fps) as f64;
            let spec = MmapSpec {
                generation_path: Some(wallpaper_mmap::frame_generation_path(&frame).into()),
                frame_path: frame.into(),
                width: env.mmap_w.clamp(1, i64::from(u32::MAX)) as u32,
                height: env.mmap_h.clamp(1, i64::from(u32::MAX)) as u32,
                file_mode: wallpaper_mmap::FRAME_FILE_MODE,
            };
            // SAFETY: the mmap output has no external handles.
            let wallpaper = unsafe { Wallpaper::new(GpuContext::new(), &settings, vec![OutputTarget::Mmap(spec)]) };
            // Offscreen mode knows no screens — the core will compare foreign windows
            // with the frame size, exactly as video does (`audioProbeSize`).
            let audio = live((env.mmap_w, env.mmap_h));
            return verdict(drive(wallpaper, selftest, audio, false, |_| true));
        }

        settings.max_fps = 60.0;
        // The VkInstance is created before the window: that way the surface and
        // the adapter come from one instance.
        let gpu = GpuContext::new();
        let mut app = QGuiApplication::new();
        if app.as_mut().is_none() {
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR приложение Qt не создалось");
            return verdict(host_exit::NOTHING_TO_SHOW);
        }
        // The departure of the last monitor must not kill the wallpaper process.
        keep_alive_without_windows();

        // There are as many windows as monitors: EVERY screen gets a wallpaper,
        // not only the primary one. The debug window is one and has no screen.
        // Sound is not doubled meanwhile: `rexon-swf` creates an audio output only
        // for the first output, the others are silent.
        let screens = swf_window::screen_count();
        let windows_wanted = if cli.windowed { 1 } else { screens.max(0) };
        if windows_wanted <= 0 {
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR мониторов в сеансе нет — обои класть некуда");
            return verdict(host_exit::NOTHING_TO_SHOW);
        }
        if cli.verbose {
            rexon_core::log_info!("wallpaper-host", "{TAG}-SCREENS экранов={screens} окон={windows_wanted}");
        }

        let (w, h) = if cli.windowed { (1280, 720) } else { (0, 0) };
        let mut windows = Vec::with_capacity(windows_wanted as usize);
        for i in 0..windows_wanted {
            // The debug window gets no screen: it is one and lives wherever the
            // compositor puts it.
            let screen_index = if cli.windowed { -1 } else { i };
            // SAFETY: the main thread, the Qt application is alive.
            let window = unsafe { swf_window::create(w, h, screen_index) };
            if window.is_null() {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR окно не создалось (экран {i})");
                return verdict(host_exit::NOTHING_TO_SHOW);
            }
            if env.placement == PLACEMENT_LAYER && !cli.windowed {
                // SAFETY: the window is alive and not yet shown — the layer is set before
                // showing.
                let ok = unsafe { apply_spec(window.cast(), &LayerSpec::WALLPAPER, &QString::from(LAYER_SCOPE)) };
                if ok {
                    rexon_gui::wallpaper_host::cinnamon_layer::surface_up("Flash-обои слоем стола");
                }
                if !ok {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN слой обоев не поставился: композитор без wlr-layer-shell?");
                }
                // Warning: and only now — the surface output. Without this all windows
                // would lie wherever the compositor decides, that is, most likely on the
                // same monitor.
                // SAFETY: the window is alive, the layer is already set, there has been no
                // show yet.
                else if !unsafe { pin_to_window_screen(window.cast()) } {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN поверхность не прицелилась на экран {i}");
                }
            }
            if env.placement == PLACEMENT_X11 && !cli.windowed {
                // The "desktop" type and state — BEFORE showing (EWMH): the window manager
                // reads the type when the window is shown.
                // SAFETY: the window comes from swf_window::create and is not yet shown.
                let id = unsafe { swf_window::x11_window_id(window) };
                if id != 0 && !prepare_x11_desktop_layer(id) {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN окно X11 не подготовлено к роли рабочего стола");
                }
            }
            // SAFETY: the window comes from swf_window::create.
            unsafe { swf_window::show(window) };
            windows.push(window);
        }

        let shown = Instant::now();
        // SAFETY: the windows come from swf_window::create.
        while windows.iter().any(|w| !unsafe { swf_window::exposed(*w) })
            && shown.elapsed() < EXPOSE_TIMEOUT
        {
            swf_window::process_events();
            std::thread::sleep(Duration::from_millis(5));
        }
        if env.placement == PLACEMENT_X11 && !cli.windowed {
            for window in &windows {
                // SAFETY: the window is shown.
                let id = unsafe { swf_window::x11_window_id(*window) };
                if id != 0 && !apply_x11_desktop_layer(id) {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN окно X11 не стало рабочим столом (EWMH)");
                }
            }
        }

        let mut targets = Vec::with_capacity(windows.len());
        let mut sizes = Vec::with_capacity(windows.len());
        for window in &windows {
            let Some((display, window_handle)) = native_handles(*window) else {
                return verdict(host_exit::SWF_NO_GPU);
            };
            // SAFETY: the window is shown.
            let (width, height) = unsafe { swf_window::size(*window) };
            let size = (width.max(1) as u32, height.max(1) as u32);
            sizes.push(size);
            targets.push(OutputTarget::Window {
                display,
                window: window_handle,
                width: size.0,
                height: size.1,
            });
        }

        // SAFETY: the windows and the connection live until the process exits.
        let wallpaper = unsafe { Wallpaper::new(gpu, &settings, targets) };
        // Warning: screens the GPU refused to draw into are taken away together
        // with their windows: an empty wallpaper window is a black rectangle over
        // the whole monitor, which is worse than honestly leaving the previous
        // background. The wallpaper stays on the monitors where drawing IS
        // possible: "all or nothing" for a person with two monitors is the same
        // empty desktop.
        if let Ok(wp) = &wallpaper {
            for index in wp.refused_targets().iter().rev() {
                rexon_core::log_warn!(
                    "wallpaper-host",
                    "{TAG}-WARN на экране {index} обои показать нечем: видеокарта не берётся \
                     рисовать в это окно. Обои остаются на остальных экранах."
                );
                // SAFETY: the window is ours, its surface has already been released by
                // the library.
                unsafe { swf_window::close(windows.remove(*index)) };
                sizes.remove(*index);
            }
        }
        // The smallest screen — the same choice as for video: by the large one, a
        // film maximized on a small monitor would not count as full screen, and
        // the wallpaper would play over it.
        let screen = sizes.iter().fold((i64::MAX, i64::MAX), |acc, (w, h)| {
            (acc.0.min(i64::from(*w)), acc.1.min(i64::from(*h)))
        });
        let audio = live(screen);
        // Whether to restart when the set of monitors changes. Not in a self-test
        // run: it measures what it found.
        let may_restart = selftest.is_none() && !cli.windowed;
        verdict(drive(wallpaper, selftest, audio, !cli.windowed, move |wp| {
            swf_window::process_events();
            // Warning: BEFORE rendering, right after processing events. When a monitor
            // is disconnected, the compositor destroys the layer surface, and the very
            // first frame into the destroyed surface is a protocol error: the client
            // is disconnected, and the wallpaper disappears from ALL monitors at once
            // (measurement 16.09: exit code 255, "error in client communication").
            // Here the set of windows is fixed when the outputs are created and does
            // not change on the fly (that is how the `rexon-swf` crate is built), so
            // the only honest answer to a configuration change is to start over.
            // Restarting in place, `execv`: the process number is preserved, and
            // nothing changes for the wallpaper executor.
            if may_restart && swf_window::screen_count() != windows_wanted {
                restart_in_place();
            }
            for (index, window) in windows.iter().enumerate() {
                // SAFETY: the window is alive.
                let (w, h) = unsafe { swf_window::size(*window) };
                let size = (w.max(1) as u32, h.max(1) as u32);
                if size != sizes[index] {
                    sizes[index] = size;
                    if let Err(e) = wp.resize(index, size.0, size.1) {
                        rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN новый размер окна не принят: {e}");
                    }
                }
            }
            true
        }))
    }

    /// Start the display over with the same process: the number of monitors
    /// changed, and the set of Flash mode outputs is fixed at creation.
    ///
    /// Returns only if the start failed — then the display continues on the
    /// previous windows: a spoiled wallpaper is better than none.
    fn restart_in_place() {
        use std::os::unix::process::CommandExt;

        let argv: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        rexon_core::log_info!("wallpaper-host", "{TAG} набор мониторов сменился — начинаю показ заново");
        let error = std::process::Command::new("/proc/self/exe").args(&argv).exec();
        rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN перезапуск не удался ({error}) — обои остаются на прежних экранах");
    }

    /// Window handles for wgpu: a Wayland surface or an X11 window. `None`
    /// means the window has neither (there is nobody left to complain to).
    fn native_handles(
        window: *mut rexon_gui::wallpaper_host::qobject::QObject,
    ) -> Option<(rwh::RawDisplayHandle, rwh::RawWindowHandle)> {
        let display = swf_window::wayland_display();
        if display != 0 {
            // SAFETY: the window is shown — the platform has a surface.
            let surface = unsafe { swf_window::wayland_surface(window) };
            let (Some(d), Some(s)) = (
                std::ptr::NonNull::new(display as *mut std::ffi::c_void),
                std::ptr::NonNull::new(surface as *mut std::ffi::c_void),
            ) else {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR у окна нет поверхности Wayland");
                return None;
            };
            return Some((
                rwh::RawDisplayHandle::Wayland(rwh::WaylandDisplayHandle::new(d)),
                rwh::RawWindowHandle::Wayland(rwh::WaylandWindowHandle::new(s)),
            ));
        }
        // SAFETY: the window is shown.
        let id = unsafe { swf_window::x11_window_id(window) };
        let connection = std::ptr::NonNull::new(swf_window::xcb_connection() as *mut std::ffi::c_void);
        let Some(id) = std::num::NonZeroU32::new(id as u32).filter(|_| connection.is_some()) else {
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR у окна нет ни поверхности Wayland, ни окна X11");
            return None;
        };
        Some((
            rwh::RawDisplayHandle::Xcb(rwh::XcbDisplayHandle::new(connection, 0)),
            rwh::RawWindowHandle::Xcb(rwh::XcbWindowHandle::new(id)),
        ))
    }

    /// The display loop until a signal (or until the end of the self-test) and
    /// the verdict.
    fn drive(
        wallpaper: Result<Wallpaper, Error>,
        selftest: Option<Duration>,
        mut audio: Option<AudioLive>,
        on_screen: bool,
        mut between: impl FnMut(&mut Wallpaper) -> bool,
    ) -> i32 {
        let mut wallpaper = match wallpaper {
            Ok(w) => w,
            Err(e) => {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {e}");
                return exit_code(&e);
            }
        };
        for w in wallpaper.warnings() {
            rexon_core::log_info!("wallpaper-host", "{TAG} {w}");
        }
        // Sound was requested, but the sound system cannot provide it. Exactly the
        // indistinguishability on which the wallpaper self-test once went green:
        // "the volume is set" and "the sound is playing" are different things.
        if audio.is_some() && !wallpaper.audio_state().available {
            rexon_core::log_warn!(
                "wallpaper-host",
                "{TAG}-NOAUDIO звук обоев включён, но звуковая система его не дала — \
                 Flash-обои идут немыми, картинка не пострадала"
            );
        }
        let started = Instant::now();
        // Heartbeat for the live wallpaper watchdog (`rexon-core::renderer_watch`):
        // we check in between frames, no more often than the core's period. If the
        // frame loop stops, the mark stops too.
        let beat_every = Duration::from_millis(rexon_core::renderer_watch::HEARTBEAT_INTERVAL_MS);
        let mut last_beat: Option<Instant> = None;
        // `between` is called before every tick: the second call means the clip's
        // first frame has already been drawn into the windows. Then — the wait
        // measurement and, on Cinnamon with layer-shell, moving the background
        // service under us (on the event, not after a delay). In frame stream mode
        // there is no window on screen — we do not measure.
        let mut calls: u32 = 0;
        let between = move |wp: &mut Wallpaper| {
            if on_screen && calls < 2 {
                calls += 1;
                if calls == 2 {
                    rexon_gui::wallpaper_host::first_frame_shown("Flash");
                }
            }
            if let Some(live) = audio.as_mut() {
                live.apply(wp);
            }
            if last_beat.is_none_or(|at| at.elapsed() >= beat_every) {
                rexon_core::renderer_watch::write_heartbeat_live();
                last_beat = Some(Instant::now());
            }
            between(wp)
        };
        match wallpaper.run(selftest, between) {
            Ok(stats) => {
                if selftest.is_none() {
                    return host_exit::OK;
                }
                let secs = started.elapsed().as_secs_f64().max(1e-3);
                let avg = |v: &[f32]| if v.is_empty() { 0.0 } else { v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64 };
                // Warning: sound in the report goes as separate values, not as a single
                // "sound=yes". "There is an output", "the stream is open", "the volume" and
                // "the level reached the device" are four different facts, and a green
                // report over silence came out exactly from mixing them up.
                let a = wallpaper.audio_state();
                eprintln!(
                    "{TAG}-SELFTEST кадров={} к/с={:.2} время_ролика={:.2}с разрывов={} рендер_мс={:.3} запись_кадра_мс={:.3} \
                     звук_выход={} звук_играет={} громкость={:.2} пик={:.4} обращений_устройства={}",
                    stats.frames,
                    stats.frames as f64 / secs,
                    stats.movie_time.as_secs_f64(),
                    stats.gaps,
                    avg(&stats.render_ms),
                    avg(&stats.mmap_ms),
                    a.available,
                    a.on,
                    a.volume01,
                    a.peak,
                    a.callbacks
                );
                if stats.frames == 0 {
                    host_exit::FRAMES_FAILED
                } else {
                    host_exit::OK
                }
            }
            Err(e) => {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {e}");
                exit_code(&e)
            }
        }
    }
}
