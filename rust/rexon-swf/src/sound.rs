//! Звук Ruffle (фича `sound`): вывод через cpal и проба без устройства (перенесено из host-proto).
//! Микшер — ruffle_core::backend::audio::AudioMixer, как у десктопного Ruffle, но без frontend-utils
//! (тот тянет reqwest/zip).
//!
//! ⚠️ «Тишина» здесь — это ОТСУТСТВИЕ потока, а не громкость 0. То же правило, что у видео-обоев
//! (там ради него `MediaPlayer` остаётся без `audioOutput`): открытый молчащий поток весь сеанс
//! висит в системном микшере отдельным ползунком, который ничего не играет, не даёт звуковой карте
//! уснуть, и по нему невозможно отличить «играет тихо» от «не играет вовсе». Поэтому `pause`
//! закрывает поток целиком, а `play` открывает заново — устройство и формат для этого запомнены.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ruffle_core::backend::audio::{
    AudioBackend, AudioMixer, AudioMixerProxy, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform, swf,
};
use ruffle_core::impl_audio_mixer_backend;

/// Сколько раз устройство забирало сэмплы и их уровень (RMS по всем каналам).
#[derive(Default)]
pub struct Level {
    pub callbacks: AtomicU64,
    acc: Mutex<(f64, u64, f64)>,
}

impl Level {
    fn add(&self, it: impl Iterator<Item = f64>) {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        let mut a = self.acc.lock().unwrap();
        for s in it {
            a.0 += s * s;
            a.1 += 1;
            a.2 = a.2.max(s.abs());
        }
    }
    /// (rms, peak, samples). Пик копится с начала работы и не сбрасывается: по нему хост отличает
    /// «ролик молчит» от «ролик звучит, но сейчас тихо».
    pub fn stats(&self) -> (f64, f64, u64) {
        let a = self.acc.lock().unwrap();
        (if a.1 > 0 { (a.0 / a.1 as f64).sqrt() } else { 0.0 }, a.2, a.1)
    }
}

fn on_stream_error(e: cpal::StreamError) {
    rexon_core::log_warn!("wallpaper-host", "REXON-SWF-AUDIO звуковой поток сообщил об ошибке: {e}");
}

/// Открыть поток на запомненном устройстве. Вынесено из `new`, потому что зовётся ещё и при
/// снятии тишины: поток закрывается насовсем, а не приглушается.
fn open_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    proxy: AudioMixerProxy,
    level: Arc<Level>,
) -> Result<cpal::Stream, String> {
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |b: &mut [f32], _| {
                proxy.mix::<f32>(b);
                level.add(b.iter().map(|s| *s as f64));
            },
            on_stream_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |b: &mut [i16], _| {
                proxy.mix::<i16>(b);
                level.add(b.iter().map(|s| *s as f64 / 32768.0));
            },
            on_stream_error,
            None,
        ),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

pub struct CpalBackend {
    mixer: AudioMixer,
    /// Устройство и формат запоминаются, чтобы поток можно было открыть заново.
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
    /// `None` — потока нет вовсе (тишина, пауза или неудачное переоткрытие).
    stream: Option<cpal::Stream>,
    pub level: Arc<Level>,
    pub desc: String,
    /// О неудаче переоткрытия рассказываем один раз: устройство могли выдернуть, и повторять
    /// жалобу каждые полсекунды незачем.
    reopen_failed: bool,
}

impl CpalBackend {
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no default output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let mixer = AudioMixer::new(config.channels as u8, config.sample_rate);
        let level = Arc::new(Level::default());
        let stream = open_stream(&device, &config, format, mixer.proxy(), level.clone())?;
        let desc = format!("host {:?}, {} ch, {} Hz, {:?}", host.id(), config.channels, config.sample_rate, format);
        Ok(Self { mixer, device, config, format, stream: Some(stream), level, desc, reopen_failed: false })
    }
}

impl AudioBackend for CpalBackend {
    impl_audio_mixer_backend!(mixer);

    fn play(&mut self) {
        if let Some(stream) = &self.stream {
            let _ = stream.play();
            return;
        }
        if self.reopen_failed {
            return;
        }
        match open_stream(&self.device, &self.config, self.format, self.mixer.proxy(), self.level.clone()) {
            Ok(stream) => self.stream = Some(stream),
            Err(e) => {
                self.reopen_failed = true;
                rexon_core::log_warn!("wallpaper-host", "REXON-SWF-AUDIO звуковой выход не открылся заново ({e}) — дальше без звука");
            }
        }
    }

    fn pause(&mut self) {
        // Именно закрыть, а не приостановить: молчащий поток в микшере — это ползунок, который
        // ничего не делает, и невозможность отличить «тихо» от «никак».
        self.stream = None;
    }
}

/// Бэкенд без устройства: сэмплы забираются вручную через `proxy()` (проверка декодирования и громкости).
pub(crate) struct ProbeBackend {
    mixer: AudioMixer,
}

impl ProbeBackend {
    pub(crate) fn new(channels: u8, rate: u32) -> Self {
        Self { mixer: AudioMixer::new(channels, rate) }
    }
    pub(crate) fn proxy(&self) -> AudioMixerProxy {
        self.mixer.proxy()
    }
}

impl AudioBackend for ProbeBackend {
    impl_audio_mixer_backend!(mixer);
    fn play(&mut self) {}
    fn pause(&mut self) {}
}
