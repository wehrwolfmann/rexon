//! Ruffle sound (feature `sound`): output via cpal and a probe without a device (moved from host-proto).
//! The mixer is ruffle_core::backend::audio::AudioMixer, as in desktop Ruffle, but without frontend-utils
//! (that one pulls in reqwest/zip).
//!
//! ⚠️ "Silence" here is the ABSENCE of a stream, not volume 0. The same rule as for video wallpapers
//! (there `MediaPlayer` stays without an `audioOutput` for its sake): an open silent stream hangs in the
//! system mixer all session as a separate slider that plays nothing, does not let the sound card
//! sleep, and makes it impossible to tell "playing quietly" from "not playing at all". So `pause`
//! closes the stream entirely, and `play` opens it again — the device and format are remembered for that.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ruffle_core::backend::audio::{
    AudioBackend, AudioMixer, AudioMixerProxy, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform, swf,
};
use ruffle_core::impl_audio_mixer_backend;

/// How many times the device took samples and their level (RMS over all channels).
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
    /// (rms, peak, samples). The peak accumulates from the start and is not reset: by it the host tells
    /// "the movie is silent" from "the movie plays, but is quiet right now".
    pub fn stats(&self) -> (f64, f64, u64) {
        let a = self.acc.lock().unwrap();
        (if a.1 > 0 { (a.0 / a.1 as f64).sqrt() } else { 0.0 }, a.2, a.1)
    }
}

fn on_stream_error(e: cpal::StreamError) {
    rexon_core::log_warn!("wallpaper-host", "REXON-SWF-AUDIO звуковой поток сообщил об ошибке: {e}");
}

/// Open a stream on the remembered device. Moved out of `new` because it is also called when
/// silence is lifted: the stream is closed for good, not muted.
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
    /// The device and format are remembered so that the stream can be opened again.
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
    /// `None` — there is no stream at all (silence, pause or a failed reopen).
    stream: Option<cpal::Stream>,
    pub level: Arc<Level>,
    pub desc: String,
    /// A failed reopen is reported once: the device may have been unplugged, and repeating
    /// the complaint every half second is pointless.
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
        // Close it, not pause it: a silent stream in the mixer is a slider that
        // does nothing, and makes it impossible to tell "quiet" from "nothing".
        self.stream = None;
    }
}

/// A backend without a device: samples are pulled by hand via `proxy()` (checking decoding and volume).
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
