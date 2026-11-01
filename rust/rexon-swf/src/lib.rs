//! rexon-swf — SWF-режим хоста обоев Rexon: Ruffle + wgpu (Vulkan), без Qt.
//!
//! Хост сам размещает окна (LayerShellQt, X11 `_NET_WM_WINDOW_TYPE_DESKTOP`) и отдаёт сюда только
//! raw-handle и размер ([`OutputTarget::Window`]); на GNOME/Cinnamon окна нет — [`OutputTarget::Mmap`].
//!
//! ```ignore
//! let gpu = rexon_swf::GpuContext::new();            // ДО окон: VkInstance нужен QVulkanInstance
//! let vk = gpu.vk_instance_handle();                 // Option<u64> для QWindow VulkanSurface
//! /* хост создаёт и размещает окна */
//! let mut wp = unsafe { rexon_swf::Wallpaper::new(gpu, &settings, targets)? };
//! wp.run(None, |wp| { host_pump_events(); /* wp.resize(i, w, h)?; wp.set_paused(p);
//!                                            wp.set_audio(on, volume01); */ true })?;
//! ```
//! Всё, что может отказать из-за окружения, возвращает [`Error`] (с [`Error::exit_code`]), а не паникует.
//! Пауза надзора Rexon (SIGSTOP/SIGCONT) безопасна: шаг ролика ограничен [`Settings::dt_cap`].

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

/// Наибольшая сторона вывода.
pub const MAX_SIDE: u32 = 16384;

/// Масштаб ролика во весь вывод.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scale {
    /// Заполнить вывод с обрезкой краёв (как `cover` у видео-обоев).
    #[default]
    NoBorder,
    /// Вписать целиком с полями.
    ShowAll,
    /// Растянуть без сохранения пропорций.
    ExactFit,
}

/// Настройки показа.
#[derive(Debug, Clone)]
pub struct Settings {
    pub swf: PathBuf,
    pub scale: Scale,
    /// Потолок частоты рендера, к/с; 0 — рисовать по готовности кадров ролика.
    pub max_fps: f64,
    /// Наибольший шаг времени ролика за один tick (разрыв после SIGSTOP/SIGCONT не проматывает ролик).
    pub dt_cap: Duration,
    /// `None` — звукового выхода нет вовсе (по умолчанию). `Some(громкость)` — звук у первого
    /// вывода (фича `sound`).
    ///
    /// ⚠️ Громкость здесь ЛИНЕЙНАЯ, 0..1 — ровно та же величина, что у видео-обоев
    /// (`AudioOutput.volume` в QML). Нелинейность Ruffle библиотека прячет в [`ruffle_volume`],
    /// наружу она не выходит: одно и то же положение ползунка обязано звучать одинаково громко у
    /// обоих видов обоев.
    pub audio_volume: Option<f32>,
}

/// Перевод ЛИНЕЙНОЙ громкости 0..1 в ту, которую ждёт `Player::set_volume` Ruffle.
///
/// Ruffle считает, что ему дают не усиление, а положение ползунка «для логарифмического слуха», и
/// сам переводит его в усиление (`core/src/backend/audio/mixer.rs`):
///
/// ```text
/// усиление = (10^(log10(81) · v) − 1) / 80 = (81^v − 1) / 80
/// ```
///
/// То есть `v = 0,5` даёт усиление 0,1 — в десять раз тише единицы (−20 дБ), тогда как у видео-обоев
/// та же половина ползунка даёт ровно 0,5 (−6 дБ). Разница слышна как «у Flash звука почти нет».
/// Здесь считается точное обращение этой формулы:
///
/// ```text
/// v = ln(80·a + 1) / ln(81)
/// ```
///
/// Обращение именно точное, а не подогнанное на слух: на выходе микшера получается ровно то
/// усиление `a`, которое просили, — значит, одно и то же положение ползунка даёт у Flash и у видео
/// один и тот же уровень сигнала. Края сходятся сами: 0 → 0, 1 → 1.
/// ⚠️ `NaN` здесь не «не бывает»: громкость приходит из файла настроек человека, а `clamp` NaN не
/// зажимает — он его пропускает насквозь, и дальше ролик замолкает целиком без единой жалобы.
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

/// Куда выводить.
#[derive(Debug, Clone)]
pub enum OutputTarget {
    /// Окно хоста: native-хэндлы (Wayland wl_display+wl_surface или X11 xcb) и размер в физических пикселях.
    Window { display: raw_window_handle::RawDisplayHandle, window: raw_window_handle::RawWindowHandle, width: u32, height: u32 },
    /// Файл кадра для расширения оболочки.
    Mmap(MmapSpec),
}

/// wgpu-инстанс. Создаётся до окон, чтобы хост мог отдать тот же VkInstance в QVulkanInstance.
pub struct GpuContext {
    instance: wgpu::Instance,
}

impl GpuContext {
    pub fn new() -> Self {
        Self { instance: create_wgpu_instance(wgpu::Backends::VULKAN, wgpu::BackendOptions::default(), None) }
    }

    /// Сырой `VkInstance` (для `QVulkanInstance::setVkInstance`), если бэкенд — Vulkan.
    pub fn vk_instance_handle(&self) -> Option<u64> {
        unsafe { self.instance.as_hal::<wgpu::hal::api::Vulkan>().map(|i| i.shared_instance().raw_instance().handle().as_raw()) }
    }
}

impl Default for GpuContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Итог одного `tick`.
#[derive(Debug, Clone, Copy)]
pub struct TickReport {
    pub rendered: bool,
    /// Разрыв реального времени больше `dt_cap` (например, после SIGCONT).
    pub gap: Option<Duration>,
    /// Когда звать `tick` снова.
    pub next_wake: Instant,
}

/// Счётчики и времена (миллисекунды, по кадрам).
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub ticks: u64,
    pub frames: u64,
    pub gaps: u64,
    pub movie_time: Duration,
    /// Номер кадра главной временной шкалы первого вывода.
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

/// Что сейчас со звуком обоев — то же разделение, что у видео: «поток есть / потока нет» отдельно
/// от громкости.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioState {
    /// Звуковой выход у обоев вообще предусмотрен (сборка со звуком, ключ не запретил, устройство
    /// нашлось). `false` — звука не будет никогда, и спрашивать про уровень бессмысленно.
    pub available: bool,
    /// Поток открыт прямо сейчас.
    pub on: bool,
    /// Линейная громкость 0..1, как её задал ползунок.
    pub volume01: f64,
    /// Наибольший уровень, который устройство забрало с начала показа. Ровно 0 при `on` и
    /// непустом `callbacks` — ролик молчит сам, а не приглушён.
    pub peak: f64,
    /// Сколько раз устройство приходило за сэмплами.
    pub callbacks: u64,
}

pub struct Wallpaper {
    descriptors: Arc<Descriptors>,
    outputs: Vec<Output>,
    /// Номера выводов из исходного `targets`, в которые видеокарта рисовать
    /// отказалась. Хост убирает по ним свои окна, иначе на этих экранах
    /// осталась бы чёрная заслонка вместо обоев.
    refused_targets: Vec<usize>,
    pacer: Pacer,
    paused: bool,
    stats: Stats,
    warnings: Vec<String>,
    /// Желаемое состояние звука. Держится отдельно от игрока намеренно: Ruffle сам зовёт
    /// `play`/`pause` звукового бэкенда при каждой смене состояния показа
    /// (`Player::set_is_playing`), и без своей памяти снятие паузы возвращало бы звук, который
    /// человек выключил.
    audio_on: bool,
    audio_volume01: f64,
    /// Уровень сигнала у устройства — `None`, когда звукового выхода нет.
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
    /// Загрузить ролик и поднять вывод(ы). Звук — только у первого вывода.
    ///
    /// # Safety
    /// Хэндлы в [`OutputTarget::Window`] должны быть валидны до уничтожения `Wallpaper`.
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
        // Ролик проверяется до GPU: битый файл не должен стоить инициализации Vulkan.
        let load = || movie_from_path(&settings.swf, None).map_err(|e| Error::MovieLoad(format!("{}: {e}", settings.swf.display())));
        let first_movie = load()?;

        let instance = gpu.instance;
        let mut surfaces = Vec::new();
        // Какому выводу отвечает поверхность: `targets` могут мешать окна и
        // файл кадров, а отказ одного окна не имеет права сдвинуть нумерацию
        // остальных — по этим номерам хост убирает свои окна.
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
        // ⚠️ Проверяются ВСЕ окна, включая первое, и отказ одного не отменяет
        // остальные. Прежде первое окно считалось пригодным «по построению»
        // (его отдавали в `request_adapter` как образец), а любой отказ
        // второго и дальше уносил обои целиком.
        //
        // Оба допущения оказались неверны. Замер 16.09.2026, X11 с двумя
        // экранами, сведёнными через `+xinerama`: `request_adapter` берёт
        // видеокарту, ГОВОРЯ, что поверхность ей годится, а `Surface::configure`
        // мгновением позже отвечает «поверхность не годится этой очереди» —
        // и ошибка проверки wgpu по умолчанию ПАНИКУЕТ (разбор — в хосте,
        // `wallpaper_host::install_panic_guard`). А «всё или ничего» для
        // человека с двумя мониторами хуже, чем обои на одном: пустой
        // рабочий стол вместо половины.
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
        // ⚠️ Ошибку проверки wgpu по умолчанию встречает паникой, а паника у
        // хоста обоев — это не «упал и поднялся»: раскрутка стека идёт через разбор
        // Qt, на котором процесс однажды завис навсегда (разбор — у
        // `rexon_gui::wallpaper_host::verdict`). Поэтому ошибки собираются
        // сюда, и по ним же выносится приговор выводу: «поверхность не
        // настроилась» — это чёрный монитор, и молчать о нём нельзя.
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
                // Отказ пойман до показа — отпускаем и поверхность.
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
            // Жалобы видеокарты считаем от этого места: всё, что придёт
            // дальше, сказано про ЭТОТ вывод.
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
            // Звук — только первому ЖИВОМУ выводу: сколько бы мониторов ни
            // было, обои звучат один раз. Именно живому: вывод, от которого
            // видеокарта отказалась, забрал бы звук с собой в тишину.
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
            // ⚠️ Приговор выводу выносится ЗДЕСЬ, и он не украшение. Замер
            // 16.09.2026 (X11, два экрана через `+xinerama`): поверхность
            // настраивается с первого раза, а со второго — того же окна и с
            // теми же числами — видеокарта отвечает «поверхность не годится
            // этой очереди». Кадры после этого «рисуются» исправно, до экрана
            // не доходит ни один, и самопроверка рапортовала 60 к/с о чёрном
            // мониторе. Молчаливо зелёный отчёт при пустом экране — ровно тот
            // класс поломок, который в проекте уже чинили.
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
        // Показывать нечем НИГДЕ — это отказ, а не «обои идут на нуле окон».
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

    /// Живое состояние звука: держать ли поток открытым и с какой ЛИНЕЙНОЙ громкостью 0..1
    /// (та же величина, что у видео-обоев).
    ///
    /// Зовётся на ходу — из решения `rexon-core::wallpaper_audio`, за которым следит хост. Тишина
    /// закрывает поток, а не выкручивает громкость в ноль: см. `sound.rs`.
    pub fn set_audio(&mut self, on: bool, volume01: f64) {
        // Чепуха из настроек превращается в 0, а не в NaN: иначе сравнение ниже никогда не
        // совпадёт и звук переоткрывался бы на каждом кадре.
        let volume01 = if volume01.is_finite() { volume01.clamp(0.0, 1.0) } else { 0.0 };
        if (on, volume01) == (self.audio_on, self.audio_volume01) {
            return;
        }
        self.audio_on = on;
        self.audio_volume01 = volume01;
        self.apply_audio();
    }

    /// Разложить желаемое состояние звука по игроку первого вывода.
    fn apply_audio(&mut self) {
        let Some(out) = self.outputs.first() else {
            return;
        };
        let want = self.audio_on && !self.paused;
        let mut player = out.player.lock().unwrap();
        // Громкость выставляется и при тишине: если поток потом откроется по чужой причине
        // (Ruffle зовёт `play` сам при снятии паузы показа), он не обязан начать с прежнего уровня.
        player.set_volume(if want { ruffle_volume(self.audio_volume01) } else { 0.0 });
        if want {
            player.audio_mut().play();
        } else {
            player.audio_mut().pause();
        }
    }

    /// Что сейчас со звуком — для отчёта хоста и для честного «ролик молчит».
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

    /// Предупреждения запуска (например, звук недоступен).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Выводы из исходного `targets`, которые пришлось бросить: видеокарта
    /// отказалась в них рисовать. Пустой список — взяты все.
    ///
    /// Нумерация — исходная, ДО отсева: по ней хост находит свои окна.
    /// Остальные выводы (`resize`, `run`) нумеруются уже без брошенных.
    pub fn refused_targets(&self) -> &[usize] {
        &self.refused_targets
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Пауза внутри процесса (ролик, звук и рендер стоят; после снятия время паузы в ролик не идёт).
    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        for o in &self.outputs {
            o.player.lock().unwrap().set_is_playing(!paused);
        }
        // ⚠️ ПОСЛЕ `set_is_playing`, и это не перестраховка: Ruffle сам зовёт `play()` звукового
        // бэкенда при снятии паузы (`Player::set_run_state`). Без этой строки обои, заглушённые
        // человеком или чужим звуком, начинали бы звучать при каждом возврате из паузы.
        self.apply_audio();
        if !paused {
            self.pacer.resume(Instant::now());
        }
    }

    /// Новый размер вывода в физических пикселях. Для mmap-вывода файлы кадра и счётчика пересоздаются.
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

    /// Шаг часов, рендер при необходимости, запись mmap-кадров.
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

    /// Счётчики (времена кадров копятся с начала или с прошлого `take_stats`).
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

    /// Простой цикл: `between` зовётся на каждой итерации (хост крутит свои события, может звать
    /// `resize`/`set_paused`; `false` — выйти), затем `tick` и сон до `next_wake` (не дольше 16 мс).
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

/// Проба звука без устройства (фича `sound`): тики ролика, сэмплы из микшера вручную. Возвращает (RMS, пик).
/// GPU не нужен (рендер-заглушка Ruffle).
///
/// `volume` — ЛИНЕЙНАЯ громкость 0..1, как у ползунка и у видео-обоев: проба меряет ровно то, что
/// услышит человек, а не то, что записано у Ruffle внутри.
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

    /// Усиление, которое микшер Ruffle применит к сэмплам, — формула из
    /// `core/src/backend/audio/mixer.rs`, повторённая здесь ради сверки.
    fn ruffle_gain(v: f64) -> f64 {
        (81.0_f64.powf(v) - 1.0) / 80.0
    }

    #[test]
    fn положение_ползунка_даёт_ту_же_громкость_что_у_видео() {
        // У видео `AudioOutput.volume` — прямое усиление: половина ползунка это половина уровня.
        // После перевода Flash обязан давать ровно столько же, иначе один и тот же ползунок
        // звучит у двух видов обоев по-разному.
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
    fn без_перевода_половина_ползунка_тише_в_десять_раз() {
        // Ради чего перевод и написан: своя шкала Ruffle на середине даёт −20 дБ, а не −6 дБ.
        assert!((ruffle_gain(0.5) - 0.1).abs() < 1e-6);
        // А с переводом середина ползунка — это середина уровня.
        assert!((f64::from(ruffle_volume(0.5)) - 0.8451).abs() < 1e-3);
    }

    #[test]
    fn края_и_чепуха_не_роняют_перевод() {
        assert_eq!(ruffle_volume(0.0), 0.0);
        assert!((ruffle_volume(1.0) - 1.0).abs() < 1e-6);
        // Громкость вне 0..1 и NaN приходить не должны, но приходить могут: из настроек человека.
        assert_eq!(ruffle_volume(-5.0), 0.0);
        assert!((ruffle_volume(17.0) - 1.0).abs() < 1e-6);
        assert_eq!(ruffle_volume(f64::NAN), 0.0);
    }
}
