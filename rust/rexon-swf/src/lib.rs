//! rexon-swf — the SWF mode of the Rexon wallpaper host: Ruffle + wgpu (Vulkan), without Qt.
//!
//! The host places the windows itself (LayerShellQt, X11 `_NET_WM_WINDOW_TYPE_DESKTOP`) and passes here only
//! the raw handle and size ([`OutputTarget::Window`]); on GNOME/Cinnamon there is no window — [`OutputTarget::Mmap`].
//!
//! ```ignore
//! let gpu = rexon_swf::GpuContext::new();            // BEFORE the windows: QVulkanInstance needs the VkInstance
//! let vk = gpu.vk_instance_handle();                 // Option<u64> for the QWindow VulkanSurface
//! /* the host creates and places the windows */
//! let mut wp = unsafe { rexon_swf::Wallpaper::new(gpu, &settings, targets)? };
//! wp.run(None, |wp| { host_pump_events(); /* wp.resize(i, w, h)?; wp.set_paused(p);
//!                                            wp.set_audio(on, volume01); */ true })?;
//! ```
//! Everything that can fail because of the environment returns an [`Error`] (with [`Error::exit_code`]) instead of panicking.
//! A Rexon supervision pause (SIGSTOP/SIGCONT) is safe: the movie step is capped by [`Settings::dt_cap`].

mod error;
mod mmap;
mod pacer;
#[cfg(feature = "sound")]
mod sound;

use std::any::Any;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ash::vk::Handle;
use ruffle_core::backend::locale::DeterministicLocaleBackend;
use ruffle_core::tag_utils::movie_from_path;
use ruffle_core::{FloatDuration, Player, PlayerBuilder, StageScaleMode, ViewportDimensions};
use ruffle_render_wgpu::backend::{WgpuRenderBackend, create_wgpu_instance, request_adapter_and_device};
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::{SwapChainTarget, TextureTarget};
use ruffle_render_wgpu::wgpu;

pub use error::Error;
pub use mmap::{MmapSpec, MmapWriter};
pub use pacer::{Pacer, Step};
pub use raw_window_handle;

/// The largest output side.
pub const MAX_SIDE: u32 = 16384;

/// Scaling of the movie to the whole output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scale {
    /// Fill the output, cropping the edges (like `cover` for video wallpapers).
    #[default]
    NoBorder,
    /// Fit entirely with margins.
    ShowAll,
    /// Stretch without keeping the aspect ratio.
    ExactFit,
}

/// Display settings.
#[derive(Debug, Clone)]
pub struct Settings {
    pub swf: PathBuf,
    pub scale: Scale,
    /// Render rate cap, fps; 0 — draw whenever the movie frames are ready.
    pub max_fps: f64,
    /// The largest movie time step per tick (a gap after SIGSTOP/SIGCONT does not fast-forward the movie).
    pub dt_cap: Duration,
    /// `None` — no audio output at all (the default). `Some(volume)` — sound on the first
    /// output (feature `sound`).
    ///
    /// ⚠️ The volume here is LINEAR, 0..1 — exactly the same quantity as for video wallpapers
    /// (`AudioOutput.volume` in QML). The library hides Ruffle's nonlinearity in [`ruffle_volume`],
    /// it does not leak outside: the same slider position must sound equally loud for
    /// both kinds of wallpaper.
    pub audio_volume: Option<f32>,
}

/// Conversion of a LINEAR volume 0..1 into the one Ruffle's `Player::set_volume` expects.
///
/// Ruffle assumes it is given not a gain but a slider position "for logarithmic hearing", and
/// converts it into a gain itself (`core/src/backend/audio/mixer.rs`):
///
/// ```text
/// gain = (10^(log10(81) · v) − 1) / 80 = (81^v − 1) / 80
/// ```
///
/// So `v = 0.5` gives a gain of 0.1 — ten times quieter than one (−20 dB), whereas for video wallpapers
/// the same half of the slider gives exactly 0.5 (−6 dB). The difference is heard as "Flash has almost no sound".
/// Here the exact inverse of that formula is computed:
///
/// ```text
/// v = ln(80·a + 1) / ln(81)
/// ```
///
/// The inverse is exact, not tuned by ear: the mixer output gets exactly the
/// gain `a` that was asked for — so the same slider position gives Flash and video
/// the same signal level. The ends match by themselves: 0 → 0, 1 → 1.
/// ⚠️ `NaN` here is not "impossible": the volume comes from the person's settings file, and `clamp` does not
/// clamp NaN — it passes it straight through, and then the movie goes silent entirely without a single complaint.
pub fn ruffle_volume(linear01: f64) -> f32 {
    if !linear01.is_finite() {
        return 0.0;
    }
    let a = linear01.clamp(0.0, 1.0);
    ((80.0 * a + 1.0).ln() / 81.0_f64.ln()) as f32
}

impl Settings {
    pub fn new(swf: impl Into<PathBuf>) -> Self {
        Self { swf: swf.into(), scale: Scale::NoBorder, max_fps: 60.0, dt_cap: Duration::from_millis(100), audio_volume: None }
    }
}

/// Where to output.
#[derive(Debug, Clone)]
pub enum OutputTarget {
    /// Host window: native handles (Wayland wl_display+wl_surface or X11 xcb) and size in physical pixels.
    Window { display: raw_window_handle::RawDisplayHandle, window: raw_window_handle::RawWindowHandle, width: u32, height: u32 },
    /// Frame file for the shell extension.
    Mmap(MmapSpec),
}

/// wgpu instance. Created before the windows so that the host can hand the same VkInstance to QVulkanInstance.
pub struct GpuContext {
    instance: wgpu::Instance,
}

impl GpuContext {
    pub fn new() -> Self {
        Self { instance: create_wgpu_instance(wgpu::Backends::VULKAN, wgpu::BackendOptions::default(), None) }
    }

    /// The raw `VkInstance` (for `QVulkanInstance::setVkInstance`), if the backend is Vulkan.
    pub fn vk_instance_handle(&self) -> Option<u64> {
        unsafe { self.instance.as_hal::<wgpu::hal::api::Vulkan>().map(|i| i.shared_instance().raw_instance().handle().as_raw()) }
    }
}

impl Default for GpuContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of one `tick`.
#[derive(Debug, Clone, Copy)]
pub struct TickReport {
    pub rendered: bool,
    /// The real-time gap is larger than `dt_cap` (for example, after SIGCONT).
    pub gap: Option<Duration>,
    /// When to call `tick` again.
    pub next_wake: Instant,
}

/// Counters and timings (milliseconds, per frame).
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub ticks: u64,
    pub frames: u64,
    pub gaps: u64,
    pub movie_time: Duration,
    /// Frame number of the main timeline of the first output.
    pub current_frame: Option<u16>,
    pub render_ms: Vec<f32>,
    pub mmap_ms: Vec<f32>,
}

struct MmapOut {
    writer: MmapWriter,
    spec: MmapSpec,
    readback: wgpu::Buffer,
    padded: u32,
    scratch: Vec<u8>,
}

struct Output {
    player: Arc<Mutex<Player>>,
    mmap: Option<MmapOut>,
}

/// What is happening with the wallpaper sound right now — the same split as for video: "there is a stream / there is no stream" separately
/// from the volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioState {
    /// An audio output is provided for the wallpaper at all (a build with sound, the key did not forbid it, a device
    /// was found). `false` — there will never be sound, and asking about the level is pointless.
    pub available: bool,
    /// The stream is open right now.
    pub on: bool,
    /// Linear volume 0..1, as the slider set it.
    pub volume01: f64,
    /// The highest level the device has taken since the start of display. Exactly 0 with `on` and
    /// non-empty `callbacks` — the movie itself is silent, not muted.
    pub peak: f64,
    /// How many times the device came for samples.
    pub callbacks: u64,
}

pub struct Wallpaper {
    descriptors: Arc<Descriptors>,
    outputs: Vec<Output>,
    /// Indices of outputs from the original `targets` that the GPU refused
    /// to draw into. The host removes its windows by them, otherwise a black
    /// shutter would remain on those screens instead of the wallpaper.
    refused_targets: Vec<usize>,
    pacer: Pacer,
    paused: bool,
    stats: Stats,
    warnings: Vec<String>,
    /// Desired sound state. Kept separately from the player on purpose: Ruffle itself calls
    /// `play`/`pause` of the audio backend on every display state change
    /// (`Player::set_is_playing`), and without our own memory unpausing would bring back sound the
    /// person turned off.
    audio_on: bool,
    audio_volume01: f64,
    /// Signal level at the device — `None` when there is no audio output.
    #[cfg(feature = "sound")]
    level: Option<Arc<sound::Level>>,
}

fn check_size(w: u32, h: u32) -> Result<(), Error> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(Error::InvalidSize { width: w, height: h });
    }
    Ok(())
}

fn scale_mode(s: Scale) -> StageScaleMode {
    match s {
        Scale::NoBorder => StageScaleMode::NoBorder,
        Scale::ShowAll => StageScaleMode::ShowAll,
        Scale::ExactFit => StageScaleMode::ExactFit,
    }
}

fn readback_buffer(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Buffer, u32) {
    let padded = (w * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rexon-swf mmap readback"),
        size: padded as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    (buf, padded)
}

impl Wallpaper {
    /// Load the movie and bring up the output(s). Sound — only on the first output.
    ///
    /// # Safety
    /// The handles in [`OutputTarget::Window`] must stay valid until the `Wallpaper` is destroyed.
    pub unsafe fn new(gpu: GpuContext, settings: &Settings, targets: Vec<OutputTarget>) -> Result<Self, Error> {
        if targets.is_empty() {
            return Err(Error::NoOutputs);
        }
        for t in &targets {
            let (w, h) = match t {
                OutputTarget::Window { width, height, .. } => (*width, *height),
                OutputTarget::Mmap(s) => (s.width, s.height),
            };
            check_size(w, h)?;
        }
        // The movie is checked before the GPU: a broken file must not cost a Vulkan initialization.
        let load = || movie_from_path(&settings.swf, None).map_err(|e| Error::MovieLoad(format!("{}: {e}", settings.swf.display())));
        let first_movie = load()?;

        let instance = gpu.instance;
        let mut surfaces = Vec::new();
        // Which output the surface belongs to: `targets` may mix windows and
        // a frame file, and the failure of one window has no right to shift the numbering
        // of the others — the host removes its windows by these numbers.
        let surface_target = {
            let mut map = Vec::new();
            for (index, t) in targets.iter().enumerate() {
                if let OutputTarget::Window { display, window, .. } = t {
                    let s = unsafe {
                        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(*display), raw_window_handle: *window })
                    }
                    .map_err(|e| Error::Surface(e.to_string()))?;
                    surfaces.push(s);
                    map.push(index);
                }
            }
            map
        };
        let (adapter, device, queue) = futures::executor::block_on(request_adapter_and_device(
            wgpu::Backends::VULKAN,
            &instance,
            surfaces.first(),
            wgpu::PowerPreference::LowPower,
        ))
        .map_err(|e| Error::NoAdapter(e.to_string()))?;
        // ⚠️ ALL windows are checked, including the first, and the failure of one does not cancel
        // the others. Previously the first window was considered usable "by construction"
        // (it was passed to `request_adapter` as the sample), and any failure
        // of the second and further ones took down the whole wallpaper.
        //
        // Both assumptions turned out wrong. Measured on 16.09.2026, X11 with two
        // screens joined via `+xinerama`: `request_adapter` picks a
        // GPU, SAYING the surface suits it, and `Surface::configure`
        // a moment later answers "the surface is not suitable for this queue" —
        // and the wgpu validation error PANICS by default (analysis — in the host,
        // `wallpaper_host::install_panic_guard`). And "all or nothing" for
        // a person with two monitors is worse than a wallpaper on one: an empty
        // desktop instead of half of it.
        let adapter_name = adapter.get_info().name;
        let mut refused = Vec::new();
        let mut warnings = Vec::new();
        for (n, s) in surfaces.iter().enumerate() {
            if s.get_capabilities(&adapter).formats.is_empty() {
                warnings.push(format!(
                    "вывод #{}: видеокарта {adapter_name} не показывает в это окно ничего — \
                     этот экран остаётся без Flash-обоев",
                    surface_target[n]
                ));
                refused.push(surface_target[n]);
            }
        }
        // ⚠️ wgpu meets a validation error with a panic by default, and a panic in the
        // wallpaper host is not "crashed and came back": stack unwinding goes through Qt
        // teardown, on which the process once hung forever (analysis — at
        // `rexon_gui::wallpaper_host::verdict`). So errors are collected
        // here, and the verdict on the output is made from them: "the surface did not
        // configure" is a black monitor, and it must not be kept silent.
        let gpu_errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = gpu_errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            sink.lock().unwrap().push(e.to_string());
        }));
        let descriptors = Arc::new(Descriptors::new(instance, adapter, device, queue));

        #[cfg(feature = "sound")]
        let mut level = None;
        let mut outputs = Vec::new();
        let mut surfaces = surfaces.into_iter();
        let mut first_movie = Some(first_movie);
        for (i, t) in targets.into_iter().enumerate() {
            let is_window = matches!(t, OutputTarget::Window { .. });
            if refused.contains(&i) {
                // The failure is caught before display — release the surface too.
                if is_window {
                    drop(surfaces.next());
                }
                continue;
            }
            let movie = match first_movie.take() {
                Some(m) => m,
                None => load()?,
            };
            let builder = PlayerBuilder::new()
                .with_locale(DeterministicLocaleBackend::default())
                .with_movie(movie)
                .with_scale_mode(scale_mode(settings.scale), true)
                .with_autoplay(true);
            // GPU complaints are counted from this point: everything that comes
            // next is said about THIS output.
            let complaints_before = gpu_errors.lock().unwrap().len();
            let (builder, mmap, (w, h)) = match t {
                OutputTarget::Window { width, height, .. } => {
                    let surface = surfaces.next().expect("one surface per window target");
                    let target = SwapChainTarget::new(surface, &descriptors.adapter, (width, height), &descriptors.device);
                    let backend = WgpuRenderBackend::new(descriptors.clone(), target).map_err(|e| Error::Render(e.to_string()))?;
                    (builder.with_renderer(backend), None, (width, height))
                }
                OutputTarget::Mmap(spec) => {
                    let target = TextureTarget::new(&descriptors.device, (spec.width, spec.height)).map_err(|e| Error::Render(e.to_string()))?;
                    let backend = WgpuRenderBackend::new(descriptors.clone(), target).map_err(|e| Error::Render(e.to_string()))?;
                    let writer = MmapWriter::create(&spec)?;
                    let (readback, padded) = readback_buffer(&descriptors.device, spec.width, spec.height);
                    let size = (spec.width, spec.height);
                    (builder.with_renderer(backend), Some(MmapOut { writer, spec, readback, padded, scratch: Vec::new() }), size)
                }
            };
            #[allow(unused_mut)]
            let mut builder = builder.with_viewport_dimensions(w, h, 1.0);
            // Sound — only to the first LIVE output: however many monitors there
            // are, the wallpaper sounds once. Live specifically: an output the
            // GPU refused would take the sound with it into silence.
            let mut sound_note: Option<String> = None;
            #[cfg(feature = "sound")]
            let mut sound_level = None;
            if outputs.is_empty() && settings.audio_volume.is_some() {
                #[cfg(feature = "sound")]
                match sound::CpalBackend::new() {
                    Ok(be) => {
                        sound_note = Some(format!("audio: on, {}", be.desc));
                        sound_level = Some(be.level.clone());
                        builder = builder.with_audio(be);
                    }
                    Err(e) => sound_note = Some(format!("audio: device unavailable ({e}), silent")),
                }
                #[cfg(not(feature = "sound"))]
                {
                    sound_note =
                        Some("audio: requested, but built without feature `sound`; silent".into());
                }
            }
            let player = builder.build();
            // ⚠️ The verdict on the output is made HERE, and it is not decoration. Measured on
            // 16.09.2026 (X11, two screens via `+xinerama`): the surface
            // configures on the first try, and on the second — the same window with
            // the same numbers — the GPU answers "the surface is not suitable for
            // this queue". Frames after that are "drawn" fine, not a single one reaches
            // the screen, and the self-check reported 60 fps about a black
            // monitor. A silently green report on an empty screen is exactly the
            // class of breakage the project has already fixed.
            let complaint = gpu_errors.lock().unwrap().get(complaints_before).cloned();
            if let (true, Some(why)) = (is_window, complaint) {
                warnings.push(format!(
                    "вывод #{i}: видеокарта {adapter_name} отказалась показывать в это окно \
                     ({}) — этот экран остаётся без Flash-обоев",
                    why.lines().next().unwrap_or("без объяснения").trim()
                ));
                refused.push(i);
                drop(player);
                continue;
            }
            if let Some(note) = sound_note {
                warnings.push(note);
            }
            #[cfg(feature = "sound")]
            if sound_level.is_some() {
                level = sound_level;
            }
            if let Some(v) = settings.audio_volume {
                player.lock().unwrap().set_volume(ruffle_volume(f64::from(v)));
            }
            outputs.push(Output { player, mmap });
        }
        // Nothing to display ANYWHERE — this is a failure, not "the wallpaper runs on zero windows".
        if outputs.is_empty() {
            return Err(Error::Surface(format!(
                "видеокарта {adapter_name} не взялась показывать обои ни в одно окно ({} шт.)",
                refused.len()
            )));
        }
        refused.sort_unstable();
        Ok(Self {
            descriptors,
            outputs,
            refused_targets: refused,
            pacer: Pacer::new(settings.max_fps, settings.dt_cap),
            paused: false,
            stats: Stats::default(),
            warnings,
            audio_on: settings.audio_volume.is_some(),
            audio_volume01: f64::from(settings.audio_volume.unwrap_or(0.0)),
            #[cfg(feature = "sound")]
            level,
        })
    }

    /// Live sound state: whether to keep the stream open and at what LINEAR volume 0..1
    /// (the same quantity as for video wallpapers).
    ///
    /// Called on the go — from the `rexon-core::wallpaper_audio` decision the host follows. Silence
    /// closes the stream rather than turning the volume down to zero: see `sound.rs`.
    pub fn set_audio(&mut self, on: bool, volume01: f64) {
        // Garbage from the settings turns into 0, not NaN: otherwise the comparison below would never
        // match and the sound would be reopened on every frame.
        let volume01 = if volume01.is_finite() { volume01.clamp(0.0, 1.0) } else { 0.0 };
        if (on, volume01) == (self.audio_on, self.audio_volume01) {
            return;
        }
        self.audio_on = on;
        self.audio_volume01 = volume01;
        self.apply_audio();
    }

    /// Apply the desired sound state to the player of the first output.
    fn apply_audio(&mut self) {
        let Some(out) = self.outputs.first() else {
            return;
        };
        let want = self.audio_on && !self.paused;
        let mut player = out.player.lock().unwrap();
        // The volume is set even when silent: if the stream later opens for someone else's reason
        // (Ruffle calls `play` itself when display is unpaused), it must not start at the old level.
        player.set_volume(if want { ruffle_volume(self.audio_volume01) } else { 0.0 });
        if want {
            player.audio_mut().play();
        } else {
            player.audio_mut().pause();
        }
    }

    /// What is happening with the sound right now — for the host report and for an honest "the movie is silent".
    pub fn audio_state(&self) -> AudioState {
        #[cfg(feature = "sound")]
        let (available, peak, callbacks) = match &self.level {
            Some(level) => {
                let (_rms, peak, _samples) = level.stats();
                (true, peak, level.callbacks.load(std::sync::atomic::Ordering::Relaxed))
            }
            None => (false, 0.0, 0),
        };
        #[cfg(not(feature = "sound"))]
        let (available, peak, callbacks) = (false, 0.0, 0);
        AudioState {
            available,
            on: available && self.audio_on && !self.paused,
            volume01: self.audio_volume01,
            peak,
            callbacks,
        }
    }

    pub fn outputs(&self) -> usize {
        self.outputs.len()
    }

    /// Startup warnings (for example, sound is unavailable).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Outputs from the original `targets` that had to be dropped: the GPU
    /// refused to draw into them. An empty list — all were taken.
    ///
    /// The numbering is the original one, BEFORE filtering: the host finds its windows by it.
    /// The remaining outputs (`resize`, `run`) are numbered without the dropped ones.
    pub fn refused_targets(&self) -> &[usize] {
        &self.refused_targets
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pause inside the process (movie, sound and render stop; after unpausing the paused time does not go into the movie).
    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        for o in &self.outputs {
            o.player.lock().unwrap().set_is_playing(!paused);
        }
        // ⚠️ AFTER `set_is_playing`, and this is not overcaution: Ruffle itself calls `play()` of the audio
        // backend when unpausing (`Player::set_run_state`). Without this line a wallpaper muted
        // by the person or by someone else's sound would start playing on every return from pause.
        self.apply_audio();
        if !paused {
            self.pacer.resume(Instant::now());
        }
    }

    /// New output size in physical pixels. For an mmap output the frame and counter files are recreated.
    pub fn resize(&mut self, index: usize, width: u32, height: u32) -> Result<(), Error> {
        check_size(width, height)?;
        let device = self.descriptors.device.clone();
        let out = self.outputs.get_mut(index).ok_or(Error::OutputIndex(index))?;
        if let Some(m) = &mut out.mmap {
            if m.writer.size() == (width, height) {
                return Ok(());
            }
            m.spec.width = width;
            m.spec.height = height;
            m.writer = MmapWriter::create(&m.spec)?;
            (m.readback, m.padded) = readback_buffer(&device, width, height);
        }
        out.player.lock().unwrap().set_viewport_dimensions(ViewportDimensions { width, height, scale_factor: 1.0 });
        Ok(())
    }

    /// Clock step, render if needed, writing of mmap frames.
    pub fn tick(&mut self, now: Instant) -> Result<TickReport, Error> {
        if self.paused {
            return Ok(TickReport { rendered: false, gap: None, next_wake: now + Duration::from_millis(100) });
        }
        let step = self.pacer.advance(now);
        self.stats.ticks += 1;
        if step.gap.is_some() {
            self.stats.gaps += 1;
        }
        self.stats.movie_time += step.dt;
        let dt = FloatDuration::from_std(step.dt);
        let mut wants = false;
        for o in &self.outputs {
            let mut p = o.player.lock().unwrap();
            p.tick(dt);
            wants |= p.needs_render();
        }
        let rendered = self.pacer.due(now, wants);
        if rendered {
            let t0 = Instant::now();
            for i in 0..self.outputs.len() {
                self.outputs[i].player.lock().unwrap().render();
                if self.outputs[i].mmap.is_some() {
                    let t1 = Instant::now();
                    self.write_mmap(i)?;
                    self.stats.mmap_ms.push(t1.elapsed().as_secs_f32() * 1000.0);
                }
            }
            self.stats.render_ms.push(t0.elapsed().as_secs_f32() * 1000.0);
            self.stats.frames += 1;
            self.pacer.rendered(now);
        }
        let hint = self.outputs[0].player.lock().unwrap().time_til_next_frame();
        Ok(TickReport { rendered, gap: step.gap, next_wake: self.pacer.next_wake(Instant::now(), hint) })
    }

    fn write_mmap(&mut self, index: usize) -> Result<(), Error> {
        let d = self.descriptors.clone();
        let out = &mut self.outputs[index];
        let texture = {
            let mut p = out.player.lock().unwrap();
            let r = <dyn Any>::downcast_mut::<WgpuRenderBackend<TextureTarget>>(p.renderer_mut())
                .ok_or_else(|| Error::Render("mmap output without texture target".into()))?;
            r.target().texture.clone()
        };
        let m = out.mmap.as_mut().expect("mmap output");
        let (w, h) = m.writer.size();
        let mut enc = d.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &m.readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(m.padded), rows_per_image: None } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        d.queue.submit([enc.finish()]);
        let slice = m.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        d.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).map_err(|e| Error::GpuLost(e.to_string()))?;
        let result = {
            let mapped = slice.get_mapped_range().map_err(|e| Error::GpuLost(format!("{e:?}")))?;
            let row = w as usize * 4;
            if m.padded as usize == row {
                m.writer.write_frame(&mapped[..row * h as usize])
            } else {
                m.scratch.resize(row * h as usize, 0);
                for y in 0..h as usize {
                    let o = y * m.padded as usize;
                    m.scratch[y * row..(y + 1) * row].copy_from_slice(&mapped[o..o + row]);
                }
                m.writer.write_frame(&m.scratch)
            }
        };
        m.readback.unmap();
        result.map_err(Error::Output)
    }

    /// Counters (frame times accumulate since the start or since the last `take_stats`).
    pub fn stats(&self) -> Stats {
        let mut s = self.stats.clone();
        s.current_frame = self.outputs[0].player.lock().unwrap().current_frame();
        s
    }

    pub fn take_stats(&mut self) -> Stats {
        let s = self.stats();
        self.stats.render_ms.clear();
        self.stats.mmap_ms.clear();
        s
    }

    /// A simple loop: `between` is called on every iteration (the host pumps its events, may call
    /// `resize`/`set_paused`; `false` — exit), then `tick` and sleep until `next_wake` (no longer than 16 ms).
    pub fn run(&mut self, duration: Option<Duration>, mut between: impl FnMut(&mut Self) -> bool) -> Result<Stats, Error> {
        let start = Instant::now();
        while duration.is_none_or(|d| start.elapsed() < d) {
            if !between(self) {
                break;
            }
            let r = self.tick(Instant::now())?;
            let wait = r.next_wake.saturating_duration_since(Instant::now());
            std::thread::sleep(wait.clamp(Duration::from_micros(500), Duration::from_millis(16)));
        }
        Ok(self.stats())
    }
}

/// Sound probe without a device (feature `sound`): movie ticks, samples pulled from the mixer by hand. Returns (RMS, peak).
/// No GPU needed (Ruffle's stub renderer).
///
/// `volume` — a LINEAR volume 0..1, as for the slider and video wallpapers: the probe measures exactly what
/// the person will hear, not what is stored inside Ruffle.
#[cfg(feature = "sound")]
pub fn probe_audio(swf: &std::path::Path, seconds: f64, volume: f32) -> Result<(f64, f32), Error> {
    const RATE: u32 = 44100;
    let movie = movie_from_path(swf, None).map_err(|e| Error::MovieLoad(format!("{}: {e}", swf.display())))?;
    let fps = movie.frame_rate().to_f64().max(1.0);
    let be = sound::ProbeBackend::new(2, RATE);
    let proxy = be.proxy();
    let player = PlayerBuilder::new().with_audio(be).with_movie(movie).with_autoplay(true).build();
    player.lock().unwrap().set_volume(ruffle_volume(f64::from(volume)));
    let mut buf = vec![0f32; ((RATE as f64 / fps).round() as usize).max(1) * 2];
    let (mut sum, mut n, mut peak) = (0f64, 0u64, 0f32);
    for _ in 0..(seconds * fps).ceil() as u32 {
        player.lock().unwrap().tick(FloatDuration::from_secs(1.0 / fps));
        buf.iter_mut().for_each(|s| *s = 0.0);
        proxy.mix::<f32>(&mut buf);
        for s in &buf {
            sum += (*s as f64) * (*s as f64);
            peak = peak.max(s.abs());
        }
        n += buf.len() as u64;
    }
    Ok(((sum / n.max(1) as f64).sqrt(), peak))
}

#[cfg(test)]
mod tests {
    use super::ruffle_volume;

    /// The gain the Ruffle mixer will apply to the samples — the formula from
    /// `core/src/backend/audio/mixer.rs`, repeated here for cross-checking.
    fn ruffle_gain(v: f64) -> f64 {
        (81.0_f64.powf(v) - 1.0) / 80.0
    }

    #[test]
    fn slider_position_gives_same_volume_as_video() {
        // For video `AudioOutput.volume` is a direct gain: half the slider is half the level.
        // After conversion Flash must give exactly the same, otherwise the same slider
        // sounds different for the two kinds of wallpaper.
        for pct in 0..=100 {
            let linear = f64::from(pct) / 100.0;
            let gain = ruffle_gain(f64::from(ruffle_volume(linear)));
            assert!(
                (gain - linear).abs() < 1e-4,
                "ползунок {pct}% дал усиление {gain:.5} вместо {linear:.5}"
            );
        }
    }

    #[test]
    fn without_mapping_half_slider_is_ten_times_quieter() {
        // What the conversion was written for: Ruffle's own scale gives −20 dB in the middle, not −6 dB.
        assert!((ruffle_gain(0.5) - 0.1).abs() < 1e-6);
        // And with the conversion the middle of the slider is the middle of the level.
        assert!((f64::from(ruffle_volume(0.5)) - 0.8451).abs() < 1e-3);
    }

    #[test]
    fn edges_and_garbage_do_not_break_mapping() {
        assert_eq!(ruffle_volume(0.0), 0.0);
        assert!((ruffle_volume(1.0) - 1.0).abs() < 1e-6);
        // Volume outside 0..1 and NaN should not arrive, but they can: from the person's settings.
        assert_eq!(ruffle_volume(-5.0), 0.0);
        assert!((ruffle_volume(17.0) - 1.0).abs() < 1e-6);
        assert_eq!(ruffle_volume(f64::NAN), 0.0);
    }
}
