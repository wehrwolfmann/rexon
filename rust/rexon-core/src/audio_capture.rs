//! Pure audio capture logic for audio-reactive wallpapers — port of
//! `rexon/audio_capture.py`.
//!
//! The original `AudioCapture` captures system audio (PipeWire →
//! PulseAudio → ALSA), reads PCM s16le stereo @ 44100 from a subprocess pipe and
//! runs two spectral analysis pipelines in a background thread:
//!   * **v9** (production, calibrated on 2026-05-21 against a WE-Windows dump, r>0.95):
//!     Hann 4096 → rfft → quartic-root edges (64 bins/channel) → MAX reduction →
//!     concat L+R (128) → pow(v, 0.02) × GAIN 1.9 → clamp → noise gate →
//!     motion blur + convolution [1,2,1]/4;
//!   * **legacy**: Hamming 4096 → rfft → linear bands 50–22050 Hz (average)
//!     → pink correction √(f/f_min) → whitening by a long-term EMA → gamma 0.5 →
//!     clamp [0.25, 1] → blur + convolution + post-clamp.
//!
//! The deterministic part: the argv of capture commands (pw-record / parec /
//! arecord) and availability probes, choosing the monitor/microphone device from
//! the observed output of pactl / `wpctl` / `arecord -l`, mixing frames in the
//! 'both' mode, s16le → f64 conversion, FFT (Cooley-Tukey radix-2), the
//! Hamming/Hann windows, distributing amplitudes into bands, both pipelines as
//! stateful analyzers ([`LegacyAnalyzer`], [`V9Analyzer`]), assembling a frame from
//! streams ([`FrameAssembler`]), the capture loop body ([`Engine`]), the subprocess
//! launch plan ([`capture_plan`]), the bass/mid/high slices, VU meter texts and
//! the JSON-stream IPC line. Observations of the environment (pactl output, presence of commands,
//! the PCM frames themselves) are passed as PARAMETERS or come through the traits
//! [`CaptureIo`] / [`PcmStreams`] — modeled on `system_plan.rs`, `importers.rs`.
//!
//! Further down the file, in a separate block (like the production transport in `net.rs`), lives
//! the EXECUTOR [`CaptureRuntime`]: starting and stopping subprocesses, reading their
//! pipes and the background thread. Since 2026-08-23 this is Rust too — all that is left in Python
//! is the facade `rexon/audio_capture.py` (the class wrapper, constant names and
//! the console test).
//!
//! NOT ported: the numpy fast path (Rust has one path — the native one),
//! `_benchmark`, CLI argument parsing and printing to the terminal (the core builds the texts,
//! the facade prints them), logging (the core accumulates lines in `notes`).
//!
//! Parity with live Python — `tests/audio_capture_parity.rs` (math:
//! the reference was taken read-only from `rexon.audio_capture`, subprocess mocked,
//! `HAS_NUMPY=False` — we pin the pure-Python path with the same
//! sequence of fp operations) and `tests/audio_capture_runtime.rs`
//! (the executor part: frame assembly, both capture loops and the launch plan
//! are checked against a recorded reference of the Python path on synthetic PCM —
//! sine / silence / clipping / two streams with underrun).
//!
//! Fixes of the original's bugs (the rule "bugs are fixed during the port"):
//!   * `_v9_bin_edges` computed `hz_per_bin` from the global `FFT_SIZE`,
//!     ignoring its own parameter `n_fft_bins` — with a non-standard FFT
//!     the edges would be wrong. Here `hz_per_bin` is derived from
//!     `n_fft_bins * 2` (for all real calls the value is the same).
//!   * The dead state `_peak_history` / `PEAK_HISTORY_LEN` / `PEAK_TRIM`
//!     (a vestige of peak-hold normalization, displaced by whitening; set up but
//!     never read) — not ported.
//!   * The confusing alias `_hann_window = _hamming_window` (a leftover from a
//!     rename) — not ported; Hann and Hamming are separate functions.
//!   * The original's out-of-sync comments (GAMMA "0.6" with code 0.5,
//!     CLAMP_MIN "0.15" with code 0.25, DIVISOR "2x avg" with code 1.0) — in
//!     the port the documentation matches the actual values.
//!
//! A preserved quirk (parity, behavior unchanged): the 'both' mix averages
//! samples with floor division (`//` in Python) — on odd negative sums
//! it gives a −1 LSB offset; it does not affect the sound/spectrum.

use std::f64::consts::PI;

// ============================================================================
// Capture constants (port of the audio_capture.py module constants)
// ============================================================================

/// Sample rate — port of `SAMPLE_RATE`.
pub const SAMPLE_RATE: u32 = 44100;
/// Channels: stereo (the WE format requires an L+R split) — port of `CHANNELS`.
pub const CHANNELS: usize = 2;
/// Sample format (16-bit signed little-endian) — port of `SAMPLE_FORMAT`.
pub const SAMPLE_FORMAT: &str = "s16le";
/// Bytes per sample — port of `BYTES_PER_SAMPLE`.
pub const BYTES_PER_SAMPLE: usize = 2;
/// FFT size (WE-compatible, ~5.4 Hz/bin) — port of `FFT_SIZE`.
pub const FFT_SIZE: usize = 4096;
/// Bands in the legacy `get_bands` API — port of `NUM_BANDS` (WE mode — 128).
pub const NUM_BANDS: usize = 16;
/// Bands in WE mode: 64 L + 64 R.
pub const WE_NUM_BANDS: usize = 128;
/// Motion blur 0.34 = WE spec (prev*0.34 + cur*0.66) — port of `SMOOTHING`.
pub const SMOOTHING: f64 = 0.34;
/// "Silence" threshold by raw amplitude (~−60 dB) — port of `NOISE_FLOOR`.
pub const NOISE_FLOOR: f64 = 0.001;
/// Bytes per analysis frame (stereo interleaved s16) — port of `READ_CHUNK`.
pub const READ_CHUNK: usize = FFT_SIZE * CHANNELS * BYTES_PER_SAMPLE;

/// Lower bound of the pink correction, Hz — port of `PINK_CORRECTION_FMIN`.
pub const PINK_CORRECTION_FMIN: f64 = 50.0;

// ── The v9 pipeline (calibrated on 2026-05-21, r>0.95 against a WE-Windows dump) ──

/// Exponent of the quartic-root edge distribution — port of `V9_SPREAD_POWER`.
pub const V9_SPREAD_POWER: f64 = 0.197;
/// Exponent of the dynamic compression pow(v, 0.02) — port of `V9_EXPONENT`.
pub const V9_EXPONENT: f64 = 0.02;
/// Lower frequency, Hz — port of `V9_FREQ_MIN`.
pub const V9_FREQ_MIN: f64 = 77.0;
/// Upper frequency, Hz (WE rounds sr/2 up) — port of `V9_FREQ_MAX`.
pub const V9_FREQ_MAX: f64 = 22062.0;
/// Output multiplier — port of `V9_GAIN`.
pub const V9_GAIN: f64 = 1.9;
/// Bands per channel in the WE format — port of `V9_BANDS_PER_CHANNEL`.
pub const V9_BANDS_PER_CHANNEL: usize = 64;

// ── Constants of the legacy pipeline's whitening stage (from `_capture_loop`) ──

/// Minimum of the long-term EMA (division guard) — port of `MIN_PEAK_FLOOR`.
pub const MIN_PEAK_FLOOR: f64 = 0.001;
/// EMA attack (~0.7 s to the new norm @30fps) — port of `EMA_ATTACK`.
pub const EMA_ATTACK: f64 = 0.05;
/// EMA release (~7 s to 1/e) — port of `EMA_RELEASE`.
pub const EMA_RELEASE: f64 = 0.005;
/// Gamma compression of the output: v^0.5 (sqrt) — port of `GAMMA`.
pub const GAMMA: f64 = 0.5;
/// Minimum "beam" length when there is sound — port of `CLAMP_MIN`.
pub const CLAMP_MIN: f64 = 0.25;
/// Whitening divisor: raw / (DIVISOR·avg) — port of `DIVISOR`.
pub const DIVISOR: f64 = 1.0;

// ============================================================================
// Capture source — port of SOURCE_* / VALID_SOURCES
// ============================================================================

/// Monitor of the default sink (the system audio output) — port of `SOURCE_MONITOR`.
pub const SOURCE_MONITOR: &str = "monitor";
/// Default source (microphone) — port of `SOURCE_MICROPHONE`.
pub const SOURCE_MICROPHONE: &str = "microphone";
/// A mono mix of monitor + microphone — port of `SOURCE_BOTH`.
pub const SOURCE_BOTH: &str = "both";
/// Valid source values — port of `VALID_SOURCES`.
pub const VALID_SOURCES: &[&str] = &[SOURCE_MONITOR, SOURCE_MICROPHONE, SOURCE_BOTH];

/// Source normalization: an unknown value → `monitor` — port of the fallback in
/// `_AudioBackend.__init__` / `AudioCapture.__init__` (there also with a warning in the log).
pub fn normalize_source(source: &str) -> &'static str {
    match source {
        SOURCE_MICROPHONE => SOURCE_MICROPHONE,
        SOURCE_BOTH => SOURCE_BOTH,
        _ => SOURCE_MONITOR,
    }
}

/// Resolving the source in the CLI: the CLI argument > env `REXON_WP_AUDIO_SOURCE` >
/// default `monitor`; an unknown value → `monitor` — port of `main()`.
pub fn resolve_source(cli: Option<&str>, env: Option<&str>) -> &'static str {
    let raw = cli.or(env).unwrap_or(SOURCE_MONITOR);
    if VALID_SOURCES.contains(&raw) {
        normalize_source(raw)
    } else {
        SOURCE_MONITOR
    }
}

/// Backend auto-detection order — port of `AudioCapture._detect_backend`:
/// PipeWire → PulseAudio → ALSA. The availability of each is the executor's observation.
pub fn detect_backend(pipewire_ok: bool, pulse_ok: bool, alsa_ok: bool) -> Option<&'static str> {
    if pipewire_ok {
        Some("pipewire")
    } else if pulse_ok {
        Some("pulseaudio")
    } else if alsa_ok {
        Some("alsa")
    } else {
        None
    }
}

// ============================================================================
// argv of capture commands and probes — port of _spawn / available()
// ============================================================================

/// `pw-record` argv for a given target — port of `PipeWireBackend._spawn`
/// (stdout — into the pipe, `-`).
pub fn pw_record_argv(target: &str) -> Vec<String> {
    vec![
        "pw-record".into(),
        "--format".into(),
        "s16".into(),
        "--rate".into(),
        SAMPLE_RATE.to_string(),
        "--channels".into(),
        CHANNELS.to_string(),
        "--target".into(),
        target.into(),
        "-".into(),
    ]
}

/// `parec` argv for a given device — port of `PulseAudioBackend._spawn`.
pub fn parec_argv(device: &str) -> Vec<String> {
    vec![
        "parec".into(),
        format!("--format={SAMPLE_FORMAT}"),
        format!("--rate={SAMPLE_RATE}"),
        format!("--channels={CHANNELS}"),
        format!("--device={device}"),
        "--raw".into(),
    ]
}

/// `arecord` argv for a given device — port of `ALSABackend._spawn`.
pub fn arecord_argv(device: &str) -> Vec<String> {
    vec![
        "arecord".into(),
        "-D".into(),
        device.into(),
        "-f".into(),
        "S16_LE".into(),
        "-r".into(),
        SAMPLE_RATE.to_string(),
        "-c".into(),
        CHANNELS.to_string(),
        "-t".into(),
        "raw".into(),
        "-q".into(),
        "-".into(),
    ]
}

/// The "PipeWire daemon is running" probe — port of `pw-cli info 0` from `available()`.
pub fn pipewire_probe_argv() -> Vec<String> {
    vec!["pw-cli".into(), "info".into(), "0".into()]
}

/// The "PulseAudio responds" probe — port of `pactl info` from `available()`.
pub fn pulseaudio_probe_argv() -> Vec<String> {
    vec!["pactl".into(), "info".into()]
}

/// `pactl get-default-sink` — query the default sink.
pub fn pactl_default_sink_argv() -> Vec<String> {
    vec!["pactl".into(), "get-default-sink".into()]
}

/// `pactl get-default-source` — query the default source.
pub fn pactl_default_source_argv() -> Vec<String> {
    vec!["pactl".into(), "get-default-source".into()]
}

/// `pactl list short sources` — the list of sources.
pub fn pactl_short_sources_argv() -> Vec<String> {
    vec!["pactl".into(), "list".into(), "short".into(), "sources".into()]
}

/// `arecord -l` — the list of ALSA capture devices (looking for loopback).
pub fn arecord_list_argv() -> Vec<String> {
    vec!["arecord".into(), "-l".into()]
}

/// `wpctl status -n` — a snapshot of the WirePlumber graph by node NAMES.
///
/// ⚠️ Why a separate branch rather than substituting `pactl` → `wpctl`. Ubuntu 24.04,
/// Kubuntu 24.04 and Debian 13 have pure PipeWire, while `pactl` lives in
/// `pulseaudio-utils`, which is not installed there (on Arch/openSUSE it is
/// pulled in transitively via `pipewire-pulse`, so the defect is invisible
/// on a dev machine). `wpctl`, though, is everywhere PipeWire is — it comes from
/// `wireplumber`, a mandatory part of the stack. But it is NOT a synonym of `pactl`:
/// a different set of commands and quite different output, hence a separate parser.
/// The `-n` flag is mandatory: without it `wpctl` prints DESCRIPTIONS ("Ryzen HD Audio
/// Controller Speaker"), while `pw-record --target` accepts node names.
pub fn wpctl_status_argv() -> Vec<String> {
    vec!["wpctl".into(), "status".into(), "-n".into()]
}

// ============================================================================
// Parsing `wpctl status -n` (wireplumber) — a replacement for pactl where it is missing
// ============================================================================

/// The audio section of the `wpctl status -n` output.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WpctlAudio {
    /// Node name of the default sink (the line is marked with `*`).
    pub default_sink: Option<String>,
    /// All sinks in output order.
    pub sinks: Vec<String>,
    /// Node name of the default source (the line is marked with `*`).
    pub default_source: Option<String>,
    /// All sources in output order.
    pub sources: Vec<String>,
}

/// Strip from the left the tree frame `wpctl` draws its sections with.
fn strip_tree(line: &str) -> &str {
    line.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '│' | '├' | '└' | '┌' | '─' | '|')
    })
}

/// Parse an entry line like `*   67. alsa_output.…  [vol: 0.78]`.
/// Returns (is_default, node_name). A node name contains no spaces,
/// so we take the first token — this also cuts off the `[vol: …]` tail.
fn parse_wpctl_entry(body: &str) -> Option<(bool, String)> {
    let (is_default, rest) = match body.strip_prefix('*') {
        Some(r) => (true, r.trim_start()),
        None => (false, body),
    };
    // `<id>. <name>`
    let (id, tail) = rest.split_once('.')?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let name = tail.split_whitespace().next()?;
    if name.is_empty() {
        return None;
    }
    Some((is_default, name.to_string()))
}

/// Parse `wpctl status -n`: the sinks and sources of the `Audio` SECTION.
///
/// The `Video` section is skipped entirely — it has its own `Sources` (cameras), and
/// the very first of them is marked `*`; without accounting for the section, a webcam would end up
/// in audio capture. Inside `Audio` only the `Sinks:` and `Sources:` subsections
/// are taken (`Streams:`/`Filters:` have lines of the same form).
pub fn parse_wpctl_status(stdout: &str) -> WpctlAudio {
    let mut out = WpctlAudio::default();
    let mut in_audio = false;
    let mut subsection = String::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // A top-level header is a line without indentation and without a frame.
        let first = line.chars().next().unwrap_or(' ');
        if !first.is_whitespace() && !matches!(first, '│' | '├' | '└' | '┌' | '─' | '|') {
            in_audio = line.trim() == "Audio";
            subsection.clear();
            continue;
        }
        if !in_audio {
            continue;
        }
        let body = strip_tree(line).trim_end();
        if let Some(name) = body.strip_suffix(':') {
            subsection = name.trim().to_string();
            continue;
        }
        let Some((is_default, name)) = parse_wpctl_entry(body) else {
            continue;
        };
        match subsection.as_str() {
            "Sinks" => {
                if is_default {
                    out.default_sink = Some(name.clone());
                }
                out.sinks.push(name);
            }
            "Sources" => {
                if is_default {
                    out.default_source = Some(name.clone());
                }
                out.sources.push(name);
            }
            _ => {}
        }
    }
    out
}

/// The `pw-record` target for the system output from the `wpctl status -n` output.
///
/// `None` — nothing was extracted from the output (no `wpctl`, an empty graph): then
/// the caller tries `pactl`, and after it the alias `@DEFAULT_MONITOR@`.
///
/// `wpctl` does not show monitor sources at all (in PipeWire they are ports
/// of the sink, not a separate node), so there is no way to check `<sink>.monitor` against a list
/// as in the `pactl` branch — the name is built from the default sink, and in its
/// absence from the first sink. The result has the same form as in the
/// `pactl` branch, so further down the code there is no difference.
pub fn wpctl_monitor_target(status_stdout: Option<&str>) -> Option<String> {
    let st = parse_wpctl_status(status_stdout?);
    let sink = st
        .default_sink
        .or_else(|| st.sinks.into_iter().next())?;
    Some(format!("{sink}.monitor"))
}

/// The `pw-record` target for the microphone from the `wpctl status -n` output:
/// the default source (if it is not a monitor) → the first non-monitor from the list.
pub fn wpctl_microphone_target(status_stdout: Option<&str>) -> Option<String> {
    let st = parse_wpctl_status(status_stdout?);
    if let Some(src) = st.default_source {
        if !src.contains(".monitor") {
            return Some(src);
        }
    }
    st.sources.into_iter().find(|n| !n.contains(".monitor"))
}

// ============================================================================
// Choosing a device from the observed pactl / arecord output
// ============================================================================
//
// Command output is passed as parameters: `Some(stdout)` — the command succeeded
// (rc == 0), `None` — it failed/is unavailable. The original treats a stdout that is empty after trim
// as no value — the same here.

/// Names of all sources from `pactl list short sources` — column 2
/// (line format: `<id>\t<name>\t<driver>\t<spec>\t<state>`).
pub fn parse_short_sources(stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            out.push(parts[1].to_string());
        }
    }
    out
}

/// Only monitor sources (the name contains `.monitor`) — port of building
/// `monitor_sources` in `PipeWireBackend._find_monitor`.
pub fn monitor_sources(stdout: &str) -> Vec<String> {
    parse_short_sources(stdout)
        .into_iter()
        .filter(|name| name.contains(".monitor"))
        .collect()
}

/// The pw-record target for the system output — port of
/// `PipeWireBackend._find_monitor`. Strategy: default sink + `.monitor`
/// (verified against the list) → the first monitor from the list → the alias
/// `@DEFAULT_MONITOR@` (not "0" — a numeric id may turn out to be a microphone).
pub fn pipewire_monitor_target(
    default_sink_stdout: Option<&str>,
    sources_stdout: Option<&str>,
) -> String {
    let monitors = sources_stdout.map(monitor_sources).unwrap_or_default();

    // (1) The exact path: default sink → its monitor. If the sources list is empty
    // (pactl failed) — we trust the candidate; if it is not empty but has no candidate, the
    // sink may be suspended → fall back to the first available monitor.
    if let Some(sink) = nonempty_trimmed(default_sink_stdout) {
        let candidate = format!("{sink}.monitor");
        if monitors.is_empty() || monitors.contains(&candidate) {
            return candidate;
        }
    }

    // (2) The first monitor from the sources list
    if let Some(first) = monitors.into_iter().next() {
        return first;
    }

    // (3) Last resort: alias PipeWire (pw-record >= 0.3.50)
    "@DEFAULT_MONITOR@".to_string()
}

/// The pw-record target for the microphone — port of `PipeWireBackend._find_microphone`.
/// Strategy: the default source if it is not a `.monitor` → the first non-monitor from
/// the list → the alias `@DEFAULT_SOURCE@`.
pub fn pipewire_microphone_target(
    default_source_stdout: Option<&str>,
    sources_stdout: Option<&str>,
) -> String {
    if let Some(src) = nonempty_trimmed(default_source_stdout) {
        if !src.ends_with(".monitor") {
            return src.to_string();
        }
    }
    if let Some(stdout) = sources_stdout {
        for name in parse_short_sources(stdout) {
            if !name.contains(".monitor") {
                return name;
            }
        }
    }
    "@DEFAULT_SOURCE@".to_string()
}

/// The parec device for the system output — port of `PulseAudioBackend._find_monitor`:
/// default sink + `.monitor` (without verification, unlike the PipeWire branch) →
/// the first source from a LINE containing `.monitor` → None.
pub fn pulse_monitor_device(
    default_sink_stdout: Option<&str>,
    sources_stdout: Option<&str>,
) -> Option<String> {
    if let Some(sink) = nonempty_trimmed(default_sink_stdout) {
        return Some(format!("{sink}.monitor"));
    }
    if let Some(stdout) = sources_stdout {
        for line in stdout.lines() {
            // The original checks `.monitor` against the WHOLE line, not the name.
            if line.contains(".monitor") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    return Some(parts[1].to_string());
                }
            }
        }
    }
    None
}

/// The parec device for the microphone — port of `PulseAudioBackend._find_microphone`.
pub fn pulse_microphone_device(
    default_source_stdout: Option<&str>,
    sources_stdout: Option<&str>,
) -> Option<String> {
    if let Some(src) = nonempty_trimmed(default_source_stdout) {
        if !src.ends_with(".monitor") {
            return Some(src.to_string());
        }
    }
    if let Some(stdout) = sources_stdout {
        for name in parse_short_sources(stdout) {
            if !name.contains(".monitor") {
                return Some(name);
            }
        }
    }
    None
}

/// The ALSA loopback device from `arecord -l` output — port of
/// `ALSABackend._find_loopback`: a line with "loopback" (case-insensitive) →
/// `card N: …` → `hw:N,1`. A non-numeric card number — the line is skipped; a line
/// without tokens before `:` raised IndexError in the original, which was swallowed by an outer
/// `except` → None (the same observable behavior here).
pub fn alsa_loopback_device(arecord_l_stdout: Option<&str>) -> Option<String> {
    let lower = arecord_l_stdout?.to_lowercase();
    for line in lower.lines() {
        if !line.contains("loopback") {
            continue;
        }
        let card_part = line.split(':').next().unwrap_or("").trim();
        // No tokens → IndexError in the original → the general except → None
        let card_num = card_part.split_whitespace().last()?;
        if card_num.parse::<i64>().is_ok() {
            return Some(format!("hw:{card_num},1"));
        }
        // ValueError → pass (next line)
    }
    None
}

/// The arecord device for the chosen source — port of `ALSABackend.start`:
/// microphone → `default`; monitor/both → loopback or `default`
/// (in the original with a warning that this is a microphone without snd-aloop).
pub fn alsa_capture_device(source: &str, loopback: Option<&str>) -> String {
    if normalize_source(source) == SOURCE_MICROPHONE {
        return "default".to_string();
    }
    loopback.unwrap_or("default").to_string()
}

fn nonempty_trimmed(stdout: Option<&str>) -> Option<&str> {
    let s = stdout?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ============================================================================
// PCM: mixing 'both' frames and stereo parsing of s16le
// ============================================================================

/// A silence frame (underrun) — port of `b"\x00" * frame_bytes`.
pub fn silence_frame(frame_bytes: usize) -> Vec<u8> {
    vec![0u8; frame_bytes]
}

/// Mixing N s16le frames sample by sample — port of the tail of `_read_frame_mixed`:
/// an average by floor division (Python `//`, a preserved quirk) clamped to i16.
/// A single frame is returned as is; an empty list → an empty vector.
/// Frames must be of the same (even) length — as in the original.
pub fn mix_frames_s16le(frames: &[&[u8]]) -> Vec<u8> {
    match frames {
        [] => Vec::new(),
        [one] => one.to_vec(),
        _ => {
            let frame_bytes = frames[0].len();
            let n_samples = frame_bytes / BYTES_PER_SAMPLE;
            let n_streams = frames.len() as i64;
            let mut mixed = Vec::with_capacity(frame_bytes);
            for i in 0..n_samples {
                let off = i * BYTES_PER_SAMPLE;
                let mut acc: i64 = 0;
                for f in frames {
                    acc += i16::from_le_bytes([f[off], f[off + 1]]) as i64;
                }
                // floor division like Python `//` (div_euclid for n > 0)
                let avg = acc.div_euclid(n_streams).clamp(-32768, 32767) as i16;
                mixed.extend_from_slice(&avg.to_le_bytes());
            }
            mixed
        }
    }
}

/// Split an interleaved-stereo s16le frame into two channels [-1.0 .. 1.0) —
/// port of the split loop in the capture loops (`l / 32768.0`).
pub fn split_stereo_s16le(frame: &[u8]) -> (Vec<f64>, Vec<f64>) {
    let stride = CHANNELS * BYTES_PER_SAMPLE; // 4 bytes per L+R pair
    let n = frame.len() / stride;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for pair in frame.chunks_exact(stride) {
        let l = i16::from_le_bytes([pair[0], pair[1]]);
        let r = i16::from_le_bytes([pair[2], pair[3]]);
        left.push(l as f64 / 32768.0);
        right.push(r as f64 / 32768.0);
    }
    (left, right)
}

// ============================================================================
// FFT (Cooley-Tukey radix-2) and windows — port of _fft / _rfft_magnitudes / the windows
// ============================================================================

/// FFT (Cooley-Tukey radix-2, in place via bit reversal) — port of `_fft`.
/// The input length is a power of two (an invariant of the callers, as in the original).
/// Complex values are (re, im) pairs.
fn fft(samples: &[f64]) -> Vec<(f64, f64)> {
    let n = samples.len();
    if n <= 1 {
        return vec![(samples.first().copied().unwrap_or(0.0), 0.0)];
    }

    // Bit-reversal permutation
    let bits = n.trailing_zeros();
    let mut result = vec![(0.0f64, 0.0f64); n];
    for (i, &s) in samples.iter().enumerate() {
        let mut rev = 0usize;
        let mut val = i;
        for _ in 0..bits {
            rev = (rev << 1) | (val & 1);
            val >>= 1;
        }
        result[rev] = (s, 0.0);
    }

    // Butterfly
    let mut size = 2;
    while size <= n {
        let half = size / 2;
        let w_base = -2.0 * PI / size as f64;
        let mut start = 0;
        while start < n {
            for k in 0..half {
                let angle = w_base * k as f64;
                let (wc, ws) = (angle.cos(), angle.sin());
                let even = result[start + k];
                let o = result[start + k + half];
                // (a+bi)(c+di) = (ac−bd) + (ad+bc)i — like CPython's complex multiplication
                let odd = (o.0 * wc - o.1 * ws, o.0 * ws + o.1 * wc);
                result[start + k] = (even.0 + odd.0, even.1 + odd.1);
                result[start + k + half] = (even.0 - odd.0, even.1 - odd.1);
            }
            start += size;
        }
        size *= 2;
    }
    result
}

/// FFT → amplitudes of positive frequencies (n/2 bins, without Nyquist),
/// normalized by dividing by n/2 — port of `_rfft_magnitudes`.
pub fn rfft_magnitudes(samples: &[f64]) -> Vec<f64> {
    let spectrum = fft(samples);
    let n = spectrum.len() / 2;
    spectrum[..n]
        .iter()
        .map(|c| (c.0 * c.0 + c.1 * c.1).sqrt() / n as f64)
        .collect()
}

/// Hamming window: 0.54 − 0.46·cos(2πi/(n−1)) — port of `_hamming_window`
/// (used by the legacy pipeline; the WE parameter `hammingparam`).
pub fn hamming_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.54 - 0.46 * (2.0 * PI * i as f64 / (n - 1) as f64).cos())
        .collect()
}

/// Hann window: 0.5 − 0.5·cos(2πi/(n−1)) — port of `_hann_window_v9`
/// (the v9 pipeline; NOT Hamming — calibration).
pub fn hann_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / (n - 1) as f64).cos())
        .collect()
}

// ============================================================================
// Bands: linear (legacy) and quartic-root + MAX (v9)
// ============================================================================

/// Distribute amplitudes into LINEAR bands 50–22050 Hz (an average over bins,
/// WE uses a linear distribution) — port of `_magnitudes_to_bands`.
/// Returns raw average amplitudes (normalization is up to the caller).
pub fn magnitudes_to_bands(magnitudes: &[f64], num_bands: usize, sample_rate: u32) -> Vec<f64> {
    let n = magnitudes.len();
    if n == 0 {
        return vec![0.0; num_bands];
    }

    let freq_min = 50.0f64;
    let freq_max = 22050.0f64.min(sample_rate as f64 / 2.0);
    let bandwidth = (freq_max - freq_min) / num_bands as f64;
    let hz_per_bin = sample_rate as f64 / (n * 2) as f64;

    let mut bands = Vec::with_capacity(num_bands);
    for i in 0..num_bands {
        let f_lo = freq_min + i as f64 * bandwidth;
        let f_hi = freq_min + (i + 1) as f64 * bandwidth;

        let bin_lo = (f_lo / hz_per_bin) as usize; // int() of a positive number = trunc
        let mut bin_hi = ((f_hi / hz_per_bin) as usize).min(n - 1);
        if bin_hi < bin_lo {
            bin_hi = bin_lo;
        }

        let avg = if bin_lo < n {
            let count = bin_hi - bin_lo + 1;
            let total: f64 = magnitudes[bin_lo..=bin_hi].iter().sum();
            total / count.max(1) as f64
        } else {
            0.0
        };
        bands.push(avg);
    }
    bands
}

/// Inclusive (bin_lo, bin_hi) pairs for the v9 quartic-root distribution —
/// port of `_v9_bin_edges`. The upper edge of band i:
/// `f_upper(i) = ((i+1)/bands)^SPREAD_POWER · FREQ_MAX + FREQ_MIN`
/// (dense at the lows, sparse at the highs — WE spec, r>0.95 against the dump).
///
/// FIX: the original computed `hz_per_bin = SAMPLE_RATE / FFT_SIZE` from the global
/// constant, ignoring the `n_fft_bins` parameter; here — from `n_fft_bins * 2`
/// (for the standard `n_fft_bins = FFT_SIZE/2` the values match).
pub fn v9_bin_edges(
    bands_per_channel: usize,
    sample_rate: u32,
    n_fft_bins: usize,
) -> Vec<(usize, usize)> {
    let hz_per_bin = sample_rate as f64 / (n_fft_bins * 2) as f64;

    let mut edges_hz: Vec<f64> = Vec::with_capacity(bands_per_channel + 1);
    edges_hz.push(V9_FREQ_MIN);
    for i in 0..bands_per_channel {
        let ratio = (i + 1) as f64 / bands_per_channel as f64;
        edges_hz.push(ratio.powf(V9_SPREAD_POWER) * V9_FREQ_MAX + V9_FREQ_MIN);
    }

    let mut pairs = Vec::with_capacity(bands_per_channel);
    for i in 0..bands_per_channel {
        // Clamp BOTH ends: FREQ_MAX=22062 > Nyquist 22050 → a bin out of range
        let bin_lo = ((edges_hz[i] / hz_per_bin) as usize).min(n_fft_bins - 1);
        let mut bin_hi = ((edges_hz[i + 1] / hz_per_bin) as usize).min(n_fft_bins - 1);
        if bin_hi < bin_lo {
            bin_hi = bin_lo;
        }
        pairs.push((bin_lo, bin_hi));
    }
    pairs
}

/// MAX reduction: the amplitude peak in each range (not the average!) —
/// port of `_v9_reduce_max`.
pub fn v9_reduce_max(magnitudes: &[f64], edges: &[(usize, usize)]) -> Vec<f64> {
    let n_mag = magnitudes.len();
    let mut out = Vec::with_capacity(edges.len());
    for &(bin_lo, bin_hi) in edges {
        if bin_lo >= n_mag {
            out.push(0.0);
            continue;
        }
        let hi = bin_hi.min(n_mag - 1);
        let mut peak = 0.0f64;
        // hi < bin_lo → an empty range (Python range) → peak 0.0
        if hi >= bin_lo {
            for &v in &magnitudes[bin_lo..=hi] {
                if v > peak {
                    peak = v;
                }
            }
        }
        out.push(peak);
    }
    out
}

/// Table of the pink correction √(f_center/f_min) per band (evening out bass and
/// treble for the 1/f spectrum of music) — port of `AudioCapture._build_pink_factors`.
pub fn build_pink_factors(bands_per_channel: usize) -> Vec<f64> {
    let freq_min = PINK_CORRECTION_FMIN;
    let freq_max = 22050.0f64.min(SAMPLE_RATE as f64 / 2.0);
    let bandwidth = (freq_max - freq_min) / bands_per_channel.max(1) as f64;
    (0..bands_per_channel)
        .map(|i| {
            let f_center = freq_min + (i as f64 + 0.5) * bandwidth;
            (f_center / freq_min).sqrt()
        })
        .collect()
}

// ============================================================================
// bass / mid / high slices — port of _band_slice / get_bass / get_mid / get_high
// ============================================================================

/// Average of the bands in the range [start_ratio .. end_ratio) of each channel —
/// port of `_band_slice`: with 128 bands the L (0..63) and
/// R (64..127) parts are averaged; otherwise — a linear slice over all bands.
pub fn band_slice(bands: &[f64], start_ratio: f64, end_ratio: f64) -> f64 {
    let n = bands.len();
    if n == 0 {
        return 0.0;
    }
    if n == WE_NUM_BANDS {
        let ch = V9_BANDS_PER_CHANNEL;
        let l_lo = (start_ratio * ch as f64) as usize;
        let l_hi = ((end_ratio * ch as f64) as usize).max(l_lo + 1).min(ch);
        let (r_lo, r_hi) = (ch + l_lo, ch + l_hi);
        let total: f64 = py_sum(&bands[l_lo..l_hi]) + py_sum(&bands[r_lo..r_hi]);
        let count = (l_hi - l_lo) + (r_hi - r_lo);
        if count == 0 {
            return 0.0;
        }
        return total / count as f64;
    }
    let lo = (start_ratio * n as f64) as usize;
    let hi = ((end_ratio * n as f64) as usize).max(lo + 1).min(n);
    let seg = &bands[lo..hi];
    if seg.is_empty() {
        0.0
    } else {
        py_sum(seg) / seg.len() as f64
    }
}

/// The exact semantics of CPython ≥ 3.12 builtin `sum()` for floats —
/// Neumaier compensated summation (the float fast path in
/// bltinmodule.c). A naive fold diverged from live Python in
/// the last ULPs on long `_band_slice` slices — a parity fix made when
/// hooking up the bridge (NOT applied in `magnitudes_to_bands`: there the reference
/// sums by hand with `total += …`, so the naive fold is the parity).
fn py_sum(values: &[f64]) -> f64 {
    let mut total = 0.0f64;
    let mut c = 0.0f64;
    for &x in values {
        let t = total + x;
        if total.abs() >= x.abs() {
            c += (total - t) + x;
        } else {
            c += (x - t) + total;
        }
        total = t;
    }
    total + c
}

/// Bass level (the lower quarter) — port of `get_bass`.
pub fn bass_level(bands: &[f64]) -> f64 {
    band_slice(bands, 0.0, 0.25)
}

/// Mid level (the second+third quarters) — port of `get_mid`.
pub fn mid_level(bands: &[f64]) -> f64 {
    band_slice(bands, 0.25, 0.75)
}

/// High level (the upper quarter) — port of `get_high`.
pub fn high_level(bands: &[f64]) -> f64 {
    band_slice(bands, 0.75, 1.0)
}

// ============================================================================
// V9Analyzer — port of `_capture_loop_v9` (without reading the pipe)
// ============================================================================

/// The stateful v9 pipeline (production, calibrated against a WE-Windows dump):
/// one call of [`V9Analyzer::process_frame`] = one iteration of `_capture_loop_v9`
/// over a ready PCM frame of length [`READ_CHUNK`].
pub struct V9Analyzer {
    smoothing: f64,
    we_mode: bool,
    hann: Vec<f64>,
    edges: Vec<(usize, usize)>,
    bands: Vec<f64>,
    raw_bands: Vec<f64>,
    peak: f64,
}

impl V9Analyzer {
    /// An analyzer for `num_bands` bands (128 = WE mode L+R, otherwise a mono fallback
    /// from the left channel) with smoothing `smoothing` — port of the fields of
    /// `AudioCapture.__init__` + the preamble of `_capture_loop_v9`.
    pub fn new(num_bands: usize, smoothing: f64) -> Self {
        let we_mode = num_bands == WE_NUM_BANDS;
        let bands_per_channel = if we_mode {
            V9_BANDS_PER_CHANNEL
        } else {
            num_bands
        };
        V9Analyzer {
            smoothing,
            we_mode,
            hann: hann_window(FFT_SIZE),
            edges: v9_bin_edges(bands_per_channel, SAMPLE_RATE, FFT_SIZE / 2),
            bands: vec![0.0; num_bands],
            raw_bands: vec![0.0; num_bands],
            peak: 0.0,
        }
    }

    /// Process one PCM frame (interleaved s16le stereo, [`READ_CHUNK`]
    /// bytes): Hann → rfft → MAX reduction → pow(0.02)·GAIN → clamp →
    /// noise gate → motion blur → convolution [1,2,1]/4.
    pub fn process_frame(&mut self, frame: &[u8]) {
        let (left, right) = split_stereo_s16le(frame);

        let left_windowed: Vec<f64> = left.iter().zip(&self.hann).map(|(s, h)| s * h).collect();
        let left_mag = rfft_magnitudes(&left_windowed);
        let left_bands = v9_reduce_max(&left_mag, &self.edges);

        let all_bands = if self.we_mode {
            let right_windowed: Vec<f64> =
                right.iter().zip(&self.hann).map(|(s, h)| s * h).collect();
            let right_mag = rfft_magnitudes(&right_windowed);
            let right_bands = v9_reduce_max(&right_mag, &self.edges);
            let mut all = left_bands;
            all.extend(right_bands);
            all
        } else {
            left_bands
        };

        // Dynamic compression pow(v, 0.02)·1.9 + clamp [0, 1]
        // (even quiet bins are pushed toward 1.0 — the WE specification)
        let n = all_bands.len();
        let mut processed: Vec<f64> = Vec::with_capacity(n);
        for &v in &all_bands {
            let out = if v > 0.0 {
                (v.powf(V9_EXPONENT) * V9_GAIN).clamp(0.0, 1.0)
            } else {
                0.0
            };
            processed.push(out);
        }

        // Noise gate by the RAW peak before compression (pow(0.02) would raise noise to 0.87)
        let raw_peak = all_bands.iter().copied().fold(0.0f64, f64::max);
        if raw_peak < NOISE_FLOOR {
            processed = vec![0.0; n];
        }

        // Motion blur + convolution [1,2,1]/4 (WE spec)
        let sm = self.smoothing;
        let prev: Vec<f64> = if self.bands.len() == n {
            self.bands.clone()
        } else {
            vec![0.0; n]
        };
        let blurred: Vec<f64> = (0..n)
            .map(|i| prev[i] * sm + processed[i] * (1.0 - sm))
            .collect();
        self.bands = convolve_121(&blurred);
        self.raw_bands = all_bands;
        self.peak = self.bands.iter().copied().fold(0.0f64, f64::max);
    }

    /// Smoothed bands [0.0 .. 1.0] — port of `get_bands`.
    pub fn bands(&self) -> &[f64] {
        &self.bands
    }

    /// Unsmoothed bands — port of `get_raw_bands`.
    pub fn raw_bands(&self) -> &[f64] {
        &self.raw_bands
    }

    /// Peak level — port of `get_peak`.
    pub fn peak(&self) -> f64 {
        self.peak
    }
}

// ============================================================================
// LegacyAnalyzer — port of `_capture_loop` (without reading the pipe)
// ============================================================================

/// The stateful legacy pipeline (pink → whitening EMA → gamma → clamp):
/// one call of [`LegacyAnalyzer::process_frame`] = one iteration of
/// `_capture_loop` over a ready PCM frame.
pub struct LegacyAnalyzer {
    smoothing: f64,
    we_mode: bool,
    bands_per_channel: usize,
    pink_correction: bool,
    pink_factors: Vec<f64>,
    hamming: Vec<f64>,
    avg_band: Vec<f64>, // long-term EMA; empty until the first frame
    bands: Vec<f64>,
    raw_bands: Vec<f64>,
    peak: f64,
}

impl LegacyAnalyzer {
    /// An analyzer for `num_bands` bands (128 = WE mode) — port of
    /// `AudioCapture.__init__` (pink factors: 64/channel in WE mode).
    pub fn new(num_bands: usize, smoothing: f64, pink_correction: bool) -> Self {
        let we_mode = num_bands == WE_NUM_BANDS;
        let bands_per_channel = if we_mode {
            V9_BANDS_PER_CHANNEL
        } else {
            num_bands
        };
        LegacyAnalyzer {
            smoothing,
            we_mode,
            bands_per_channel,
            pink_correction,
            pink_factors: build_pink_factors(bands_per_channel),
            hamming: hamming_window(FFT_SIZE),
            avg_band: Vec::new(),
            bands: vec![0.0; num_bands],
            raw_bands: vec![0.0; num_bands],
            peak: 0.0,
        }
    }

    /// Process one PCM frame: Hamming → rfft → linear bands → pink →
    /// whitening (EMA attack/release) → gamma 0.5 → clamp [0.25, 1] →
    /// noise floor → motion blur → convolution [1,2,1]/4 → post-clamp.
    pub fn process_frame(&mut self, frame: &[u8]) {
        let (left, right) = split_stereo_s16le(frame);

        let left_windowed: Vec<f64> = left.iter().zip(&self.hamming).map(|(s, h)| s * h).collect();
        let left_mag = rfft_magnitudes(&left_windowed);
        let left_bands = magnitudes_to_bands(&left_mag, self.bands_per_channel, SAMPLE_RATE);

        let mut all_bands = if self.we_mode {
            let right_windowed: Vec<f64> =
                right.iter().zip(&self.hamming).map(|(s, h)| s * h).collect();
            let right_mag = rfft_magnitudes(&right_windowed);
            let right_bands = magnitudes_to_bands(&right_mag, self.bands_per_channel, SAMPLE_RATE);
            let mut all = left_bands;
            all.extend(right_bands);
            all
        } else {
            left_bands
        };

        // Pink correction: a static HF pre-boost ∝ √f before whitening
        // (in WE mode factors of length 64 are applied to L and R modulo)
        if self.pink_correction && !self.pink_factors.is_empty() {
            let pf = &self.pink_factors;
            let n_pf = pf.len();
            if self.we_mode {
                for (i, v) in all_bands.iter_mut().enumerate() {
                    *v *= pf[i % n_pf];
                }
            } else {
                for (i, v) in all_bands.iter_mut().enumerate() {
                    if i < n_pf {
                        *v *= pf[i];
                    }
                }
            }
        }

        let n = all_bands.len();
        let current_peak = all_bands.iter().copied().fold(0.0f64, f64::max);

        // Long-term EMA of each bin (initialized on the first frame)
        if self.avg_band.len() != n {
            self.avg_band = all_bands.iter().map(|&v| v.max(MIN_PEAK_FLOOR)).collect();
        }
        for (avg, &cur) in self.avg_band.iter_mut().zip(&all_bands) {
            let prev = *avg;
            // Above avg → fast rise (attack), below → slow fall (release)
            let new_avg = if cur > prev {
                prev * (1.0 - EMA_ATTACK) + cur * EMA_ATTACK
            } else {
                prev * (1.0 - EMA_RELEASE) + cur * EMA_RELEASE
            };
            *avg = new_avg.max(MIN_PEAK_FLOOR);
        }

        // Whitening raw/avg + gamma + clamp; the whole spectrum quiet → zeros
        let silent = current_peak < NOISE_FLOOR;
        let clamped: Vec<f64> = if silent {
            vec![0.0; n]
        } else {
            (0..n)
                .map(|i| {
                    let w = all_bands[i] / (DIVISOR * self.avg_band[i]);
                    let w = if w > 0.0 { w.powf(GAMMA) } else { 0.0 };
                    w.clamp(CLAMP_MIN, 1.0)
                })
                .collect()
        };

        // Motion blur → convolution [1,2,1]/4 → post-clamp (otherwise blur wipes out
        // the minimum in the first frames)
        let sm = self.smoothing;
        let blurred: Vec<f64> = (0..n)
            .map(|i| self.bands[i] * sm + clamped[i] * (1.0 - sm))
            .collect();
        let mut smoothed = convolve_121(&blurred);
        if !silent {
            for v in &mut smoothed {
                if *v < CLAMP_MIN {
                    *v = CLAMP_MIN;
                }
            }
        }

        self.bands = smoothed;
        self.raw_bands = all_bands;
        self.peak = self.bands.iter().copied().fold(0.0f64, f64::max);
    }

    /// Smoothed bands [0.0 .. 1.0] — port of `get_bands`.
    pub fn bands(&self) -> &[f64] {
        &self.bands
    }

    /// Unsmoothed bands (after the pink correction) — port of `get_raw_bands`.
    pub fn raw_bands(&self) -> &[f64] {
        &self.raw_bands
    }

    /// Peak level — port of `get_peak`.
    pub fn peak(&self) -> f64 {
        self.peak
    }
}

/// Convolution [1, 2, 1]/4 with edge replication — a step shared by both pipelines.
fn convolve_121(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    (0..n)
        .map(|i| {
            let a = if i >= 1 { values[i - 1] } else { values[i] };
            let b = values[i];
            let c = if i + 1 < n { values[i + 1] } else { values[i] };
            (a + 2.0 * b + c) / 4.0
        })
        .collect()
}

// ============================================================================
// JSON-stream IPC — port of the stream mode of main()
// ============================================================================

/// A JSON-stream line (`{"b":[...]}` compact, like
/// `json.dumps(..., separators=(",", ":"))`) — port of the stream loop of `main()`.
pub fn stream_json_line(bands: &[f64]) -> String {
    let mut s = String::from("{\"b\":[");
    for (i, v) in bands.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&py_float_repr(*v));
    }
    s.push_str("]}");
    s
}

/// `repr()` of a floating-point number BY PYTHON'S RULES — the same way
/// `json.dumps` prints it (it uses `float.__repr__`).
///
/// Both sides give the shortest representation, but the thresholds and formatting
/// DIFFER, and in the `--stream` stream it shows: ryu gives `0.00001` and
/// `1.5e-7`, Python — `1e-05` and `1.5e-07`. Quiet bands give exactly such
/// values, so the divergence is reachable, not theoretical. Python's
/// rules: exponential notation when `exp < -4` or `exp >= 16`, the exponent sign
/// always, at least two digits; integer values — with `.0`.
pub fn py_float_repr(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }

    // `{:e}` in Rust is the shortest representation in scientific form: `d[.ddd]e<exp>`.
    let sci = format!("{:e}", v.abs());
    let (mant, exp) = match sci.split_once('e') {
        Some(pair) => pair,
        None => return sci, // unreachable for finite numbers
    };
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let nd = digits.len() as i32;

    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    if !(-4..16).contains(&exp) {
        out.push_str(&digits[..1]);
        if nd > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        let a = exp.unsigned_abs();
        if a < 10 {
            out.push('0');
        }
        out.push_str(&a.to_string());
    } else if exp >= 0 {
        let ip = (exp + 1) as usize; // digits in the integer part
        if nd as usize <= ip {
            out.push_str(&digits);
            for _ in 0..(ip - nd as usize) {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            out.push_str(&digits[..ip]);
            out.push('.');
            out.push_str(&digits[ip..]);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.push_str(&digits);
    }
    out
}

// ============================================================================
// The pipeline as a single entity — port of the `pipeline` selector of `AudioCapture`
// ============================================================================

/// The default pipeline (calibrated against a WE-Windows dump) — port of `"v9"`.
pub const PIPELINE_V9: &str = "v9";
/// The fallback pipeline (whitening + gamma + clamp + pink) — port of `"legacy"`.
pub const PIPELINE_LEGACY: &str = "legacy";

/// Pipeline name normalization: everything except `legacy` is `v9` (port of
/// the fallback in `AudioCapture.__init__`, where an unknown value became
/// `v9` with a warning).
pub fn normalize_pipeline(pipeline: &str) -> &'static str {
    if pipeline == PIPELINE_LEGACY {
        PIPELINE_LEGACY
    } else {
        PIPELINE_V9
    }
}

/// Both pipelines behind one face — port of the choice between `_capture_loop_v9` /
/// `_capture_loop` in `AudioCapture.start`.
pub enum Analyzer {
    /// The production v9 pipeline.
    V9(V9Analyzer),
    /// The fallback legacy pipeline.
    Legacy(LegacyAnalyzer),
}

impl Analyzer {
    /// An analyzer by pipeline name (an unknown name → `v9`).
    pub fn new(pipeline: &str, num_bands: usize, smoothing: f64, pink_correction: bool) -> Self {
        match normalize_pipeline(pipeline) {
            PIPELINE_LEGACY => Analyzer::Legacy(LegacyAnalyzer::new(
                num_bands,
                smoothing,
                pink_correction,
            )),
            _ => Analyzer::V9(V9Analyzer::new(num_bands, smoothing)),
        }
    }

    /// Process one PCM frame.
    pub fn process_frame(&mut self, frame: &[u8]) {
        match self {
            Analyzer::V9(a) => a.process_frame(frame),
            Analyzer::Legacy(a) => a.process_frame(frame),
        }
    }

    /// Smoothed bands.
    pub fn bands(&self) -> &[f64] {
        match self {
            Analyzer::V9(a) => a.bands(),
            Analyzer::Legacy(a) => a.bands(),
        }
    }

    /// Unsmoothed bands.
    pub fn raw_bands(&self) -> &[f64] {
        match self {
            Analyzer::V9(a) => a.raw_bands(),
            Analyzer::Legacy(a) => a.raw_bands(),
        }
    }

    /// Peak level.
    pub fn peak(&self) -> f64 {
        match self {
            Analyzer::V9(a) => a.peak(),
            Analyzer::Legacy(a) => a.peak(),
        }
    }
}

// ============================================================================
// Assembling a frame from capture streams — port of `_read_frame_mixed`
// ============================================================================

/// PCM streams a frame is assembled from. The reading itself is a side effect of
/// the executor (a live subprocess pipe in production, prepared bytes in a check),
/// so it is behind a trait, while all the buffer arithmetic is in [`FrameAssembler`].
pub trait PcmStreams {
    /// How many streams.
    fn count(&self) -> usize;

    /// Read up to `max` bytes from stream `i`.
    ///
    /// * `Some(data)` — read (an empty slice = the stream ended, EOF);
    /// * `None` — a read error (port of `except` in the original: the stream is dead).
    ///
    /// Reading MUST go up to `max` bytes or to the end of the stream, as
    /// Python's `BufferedReader.read(n)` does: the original would not have returned a partial read,
    /// and the loop would get extra silence frames.
    fn read(&mut self, i: usize, max: usize) -> Option<Vec<u8>>;
}

/// Byte-wise assembly of an analysis frame from one or two capture streams —
/// port of `AudioCapture._read_frame_mixed` without the reading itself.
///
/// The `both` mode gives two streams: the frame is assembled from those that have already accumulated
/// a full frame, and averaged ([`mix_frames_s16le`]). If nobody has a full frame
/// but the streams are alive, a silence frame is returned — so the loop does not get stuck on
/// a slow source (underrun).
pub struct FrameAssembler {
    frame_bytes: usize,
    buffers: Vec<Vec<u8>>,
}

impl FrameAssembler {
    /// An assembler for `streams` streams with a frame of `frame_bytes` bytes.
    pub fn new(streams: usize, frame_bytes: usize) -> Self {
        FrameAssembler {
            frame_bytes,
            buffers: (0..streams).map(|_| Vec::new()).collect(),
        }
    }

    /// Analysis frame size in bytes.
    pub fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    /// The next frame; `None` — all streams ended (port of `return None`).
    pub fn next_frame(&mut self, streams: &mut dyn PcmStreams) -> Option<Vec<u8>> {
        let n = self.buffers.len();
        let mut alive: Vec<bool> = Vec::with_capacity(n);
        for i in 0..n {
            let need = self.frame_bytes.saturating_sub(self.buffers[i].len());
            if need == 0 {
                alive.push(true);
                continue;
            }
            match streams.read(i, need) {
                Some(data) if !data.is_empty() => {
                    self.buffers[i].extend_from_slice(&data);
                    alive.push(true);
                }
                // An empty read — the stream ended; it counts as alive only
                // while the buffer still holds a whole frame (port of the `else` branch).
                Some(_) => alive.push(self.buffers[i].len() >= self.frame_bytes),
                None => alive.push(false),
            }
        }

        let ready: Vec<usize> = (0..n)
            .filter(|&i| self.buffers[i].len() >= self.frame_bytes)
            .collect();
        if ready.is_empty() {
            if !alive.iter().any(|&a| a) {
                return None;
            }
            // Underrun — a silence frame so the loop does not get stuck.
            return Some(silence_frame(self.frame_bytes));
        }

        let mixed = {
            let parts: Vec<&[u8]> = ready
                .iter()
                .map(|&i| &self.buffers[i][..self.frame_bytes])
                .collect();
            mix_frames_s16le(&parts)
        };
        // Buffers move ONLY after a successful mix (as in the original).
        for &i in &ready {
            self.buffers[i].drain(..self.frame_bytes);
        }
        Some(mixed)
    }
}

// ============================================================================
// The capture engine — port of the body of `_rs_capture_loop`
// ============================================================================

/// A snapshot of a frame's results — port of the fields `_bands` / `_raw_bands` / `_peak`
/// that the original copied under `self._lock`.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Smoothed bands [0.0 .. 1.0].
    pub bands: Vec<f64>,
    /// Unsmoothed bands.
    pub raw_bands: Vec<f64>,
    /// Peak level.
    pub peak: f64,
}

impl Snapshot {
    /// A zero snapshot for `num_bands` bands — port of the data reset in `stop()`.
    pub fn zeroed(num_bands: usize) -> Self {
        Snapshot {
            bands: vec![0.0; num_bands],
            raw_bands: vec![0.0; num_bands],
            peak: 0.0,
        }
    }
}

/// One whole iteration of the capture loop: assemble a frame from the streams and run
/// it through the pipeline — port of the `while self._running` body in `_rs_capture_loop`.
pub struct Engine {
    analyzer: Analyzer,
    assembler: FrameAssembler,
}

impl Engine {
    /// An engine for `streams` streams with the chosen pipeline.
    pub fn new(
        pipeline: &str,
        num_bands: usize,
        smoothing: f64,
        pink_correction: bool,
        streams: usize,
    ) -> Self {
        Engine {
            analyzer: Analyzer::new(pipeline, num_bands, smoothing, pink_correction),
            assembler: FrameAssembler::new(streams, READ_CHUNK),
        }
    }

    /// A loop step: `false` — the capture stream ended, it is time to finish the loop.
    pub fn step(&mut self, streams: &mut dyn PcmStreams) -> bool {
        match self.assembler.next_frame(streams) {
            Some(frame) => {
                self.analyzer.process_frame(&frame);
                true
            }
            None => false,
        }
    }

    /// The current frame data.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            bands: self.analyzer.bands().to_vec(),
            raw_bands: self.analyzer.raw_bands().to_vec(),
            peak: self.analyzer.peak(),
        }
    }
}

// ============================================================================
// The capture plan: environment observations behind a trait — port of `available()` /
// `_find_*` / `start_multi` (modeled on `system_plan.rs`, `importers.rs`)
// ============================================================================

/// The result of a one-off observation command — port of `subprocess.run(...,
/// capture_output=True, text=True)`.
#[derive(Clone, Debug, Default)]
pub struct ObsOut {
    /// `returncode == 0`.
    pub ok: bool,
    /// stdout text (`errors="replace"`).
    pub stdout: String,
}

/// Environment observations the capture plan needs. The real calls are with
/// the executor ([`LiveCaptureIo`] in production, a fake table in a check).
pub trait CaptureIo {
    /// `shutil.which(cmd) is not None`.
    fn has_cmd(&mut self, cmd: &str) -> bool;

    /// `subprocess.run(argv, capture_output=True, text=True, timeout=…)`;
    /// `None` — the command did not start or did not fit into the timeout (port of
    /// the original's general `except`).
    fn run(&mut self, argv: &[String], timeout: f64) -> Option<ObsOut>;
}

/// A command's stdout when `returncode == 0`, otherwise `None` — port of `_rs_run_stdout`.
fn run_stdout(io: &mut dyn CaptureIo, argv: &[String]) -> Option<String> {
    let r = io.run(argv, 3.0)?;
    if r.ok {
        Some(r.stdout)
    } else {
        None
    }
}

/// `returncode == 0` of a backend availability probe (timeout 2 s, as in
/// `available()`).
fn probe_ok(io: &mut dyn CaptureIo, argv: &[String]) -> bool {
    io.run(argv, 2.0).map(|r| r.ok).unwrap_or(false)
}

/// PipeWire availability — port of `PipeWireBackend.available`.
pub fn pipewire_available(io: &mut dyn CaptureIo) -> bool {
    io.has_cmd("pw-record") && probe_ok(io, &pipewire_probe_argv())
}

/// PulseAudio availability — port of `PulseAudioBackend.available`.
pub fn pulseaudio_available(io: &mut dyn CaptureIo) -> bool {
    io.has_cmd("parec") && probe_ok(io, &pulseaudio_probe_argv())
}

/// ALSA availability — port of `ALSABackend.available` (`arecord` is almost
/// always there, the original has no separate probe).
pub fn alsa_available(io: &mut dyn CaptureIo) -> bool {
    io.has_cmd("arecord")
}

/// Live backend auto-detection — port of `AudioCapture._detect_backend` together with
/// the probes: PipeWire → PulseAudio → ALSA, `None` — there is nothing to capture sound with.
pub fn detect_backend_live(io: &mut dyn CaptureIo) -> Option<&'static str> {
    detect_backend(
        pipewire_available(io),
        pulseaudio_available(io),
        alsa_available(io),
    )
}

/// The capture device from a WirePlumber snapshot — port of `_wpctl_target`
/// (including checking that `wpctl` itself exists).
fn wpctl_target(io: &mut dyn CaptureIo, source_type: &str) -> Option<String> {
    if !io.has_cmd("wpctl") {
        return None;
    }
    let stdout = run_stdout(io, &wpctl_status_argv())?;
    if stdout.is_empty() {
        return None;
    }
    if source_type == SOURCE_MICROPHONE {
        wpctl_microphone_target(Some(&stdout))
    } else {
        wpctl_monitor_target(Some(&stdout))
    }
}

/// The capture launch plan: what to start subprocesses with and with which arguments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapturePlan {
    /// Backend name (`pipewire` / `pulseaudio` / `alsa`).
    pub backend: String,
    /// The normalized source.
    pub source: String,
    /// Subprocess argv: one for `monitor`/`microphone`, two for `both`.
    pub argvs: Vec<Vec<String>>,
    /// Lines for the log (port of the original's `log.info`/`log.warning`) —
    /// the executor prints them, the core only words them.
    pub notes: Vec<String>,
}

/// PipeWire: the capture target for the source type — port of
/// `PipeWireBackend._find_monitor` / `_find_microphone` (first the WirePlumber
/// snapshot, then `pactl`; the decision from observations is in the core).
fn pipewire_target(io: &mut dyn CaptureIo, source_type: &str, notes: &mut Vec<String>) -> String {
    let mic = source_type == SOURCE_MICROPHONE;
    let kind = if mic { "microphone" } else { "monitor" };
    if let Some(t) = wpctl_target(io, source_type) {
        notes.push(format!("Audio target: {kind} (wireplumber) = {t}"));
        return t;
    }
    let head = if mic {
        run_stdout(io, &pactl_default_source_argv())
    } else {
        run_stdout(io, &pactl_default_sink_argv())
    };
    let list = run_stdout(io, &pactl_short_sources_argv());
    let target = if mic {
        pipewire_microphone_target(head.as_deref(), list.as_deref())
    } else {
        pipewire_monitor_target(head.as_deref(), list.as_deref())
    };
    notes.push(format!("Audio target: {kind} = {target}"));
    target
}

/// PulseAudio: the device for the source type — port of
/// `PulseAudioBackend._find_monitor` / `_find_microphone`. There is deliberately no `wpctl`
/// branch here: the backend is enabled only when `parec` is found, and
/// `parec` and `pactl` come in one package, `pulseaudio-utils`.
fn pulse_device(io: &mut dyn CaptureIo, source_type: &str) -> Option<String> {
    let list = run_stdout(io, &pactl_short_sources_argv());
    if source_type == SOURCE_MICROPHONE {
        let src = run_stdout(io, &pactl_default_source_argv());
        pulse_microphone_device(src.as_deref(), list.as_deref())
    } else {
        let sink = run_stdout(io, &pactl_default_sink_argv());
        pulse_monitor_device(sink.as_deref(), list.as_deref())
    }
}

/// The capture plan for the chosen backend and source — port of `start_multi()`
/// of all three backends (without the `Popen` itself).
pub fn capture_plan(io: &mut dyn CaptureIo, backend: &str, source: &str) -> CapturePlan {
    let source = normalize_source(source);
    let mut notes: Vec<String> = Vec::new();
    let mut argvs: Vec<Vec<String>> = Vec::new();

    match backend {
        "pipewire" => {
            if source == SOURCE_BOTH {
                let mon = pipewire_target(io, SOURCE_MONITOR, &mut notes);
                let mic = pipewire_target(io, SOURCE_MICROPHONE, &mut notes);
                argvs.push(pw_record_argv(&mon));
                argvs.push(pw_record_argv(&mic));
            } else {
                let t = pipewire_target(io, source, &mut notes);
                argvs.push(pw_record_argv(&t));
            }
        }
        "pulseaudio" => {
            if source == SOURCE_BOTH {
                if let Some(mon) = pulse_device(io, SOURCE_MONITOR) {
                    argvs.push(parec_argv(&mon));
                }
                if let Some(mic) = pulse_device(io, SOURCE_MICROPHONE) {
                    argvs.push(parec_argv(&mic));
                }
                if argvs.is_empty() {
                    notes.push("PulseAudio 'both': ни один subprocess не запущен".into());
                }
            } else {
                match pulse_device(io, source) {
                    Some(dev) => argvs.push(parec_argv(&dev)),
                    None => notes.push(format!(
                        "PulseAudio: source не найден для type={source}"
                    )),
                }
            }
        }
        "alsa" => {
            if source == SOURCE_BOTH {
                notes.push(
                    "ALSA backend: режим 'both' не поддерживается, fallback на monitor".into(),
                );
            }
            let device = if source == SOURCE_MICROPHONE {
                alsa_capture_device(SOURCE_MICROPHONE, None)
            } else {
                let loopback = alsa_loopback_device(run_stdout(io, &arecord_list_argv()).as_deref());
                let dev = alsa_capture_device(source, loopback.as_deref());
                if dev == "default" {
                    notes.push(
                        "ALSA: используется default (микрофон). Для захвата системного \
                         аудио нужен snd-aloop: sudo modprobe snd-aloop"
                            .into(),
                    );
                }
                dev
            };
            argvs.push(arecord_argv(&device));
        }
        _ => {}
    }

    CapturePlan {
        backend: backend.to_string(),
        source: source.to_string(),
        argvs,
        notes,
    }
}

// ============================================================================
// Texts of the console VU meter — port of the printing in `main()`
// ============================================================================

/// The VU meter header with range levels — port of the `Bass/Mid/High/Peak` line.
pub fn vu_levels_line(bass: f64, mid: f64, high: f64, peak: f64) -> String {
    format!("  Bass: {bass:.2}  Mid: {mid:.2}  High: {high:.2}  Peak: {peak:.2}")
}

/// ASCII bar lines per band — port of the `--bars` branch.
pub fn vu_bar_lines(bands: &[f64], bar_width: usize) -> Vec<String> {
    bands
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let filled = ((b * bar_width as f64) as isize).clamp(0, bar_width as isize) as usize;
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);
            format!("  {i:2} [{bar}] {b:.2}")
        })
        .collect()
}

/// The compact VU meter line — port of the branch without `--bars`.
pub fn vu_compact_line(bass: f64, mid: f64, high: f64, bands: &[f64]) -> String {
    let bar: Vec<String> = bands.iter().map(|b| format!("{b:.1}")).collect();
    format!(
        "  B={bass:.2} M={mid:.2} H={high:.2} | {}    ",
        bar.join(" ")
    )
}

// ============================================================================
// The production executor: capture subprocesses and the background thread
// ============================================================================
//
// Here the deterministic part ends and work with the system begins.
// A separate block — exactly like the production transport in `net.rs`: everything above in the file
// is checked by goldens without a single system call, while below live the launching of
// `pw-record`/`parec`/`arecord`, reading their pipes and the background thread. A single
// type sticks out, [`CaptureRuntime`], with the same behavior that
// `AudioCapture` had in Python.
//
// Why still an external command and not a sound server library.
// Three paths were examined: `cpal` 0.18 (Apache-2.0; the PipeWire host appeared only
// in June 2026 and is still being fixed, on Linux it unconditionally pulls in `alsa-sys`,
// i.e. ALSA headers at build time and `libasound.so.2` in the shipment), raw
// `pipewire` 0.10 (MIT, but `links = pipewire-0.3` + bindgen: PipeWire headers
// and libclang at build time, manual SPA-POD assembly in code) and an external
// binary through a pipe. For the Steam box the third was chosen: zero build
// dependencies, zero new ABI surface in the shipment, a separate process (a hung
// capture does not bring down the application) and — most importantly — the ready fallback chain
// pw-record → parec → arecord, which saves the day on machines without PipeWire.
// The cost is an extra copy of 176 KB/s through a pipe, unnoticeable next to the FFT.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Observations of the live system — the production implementation of [`CaptureIo`].
pub struct LiveCaptureIo;

/// `shutil.which`: an executable file with this name in `PATH`.
fn which_in_path(cmd: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if cmd.contains('/') {
        return std::path::Path::new(cmd).is_file();
    }
    let path = std::env::var("PATH").unwrap_or_else(|_| crate::utils::CS_PATH_FALLBACK.to_string());
    for dir in path.split(':') {
        let dir = if dir.is_empty() { "." } else { dir };
        let candidate = std::path::Path::new(dir).join(cmd);
        if let Ok(md) = std::fs::metadata(&candidate) {
            if md.is_file() && md.permissions().mode() & 0o111 != 0 {
                return true;
            }
        }
    }
    false
}

impl CaptureIo for LiveCaptureIo {
    fn has_cmd(&mut self, cmd: &str) -> bool {
        which_in_path(cmd)
    }

    fn run(&mut self, argv: &[String], timeout: f64) -> Option<ObsOut> {
        use std::process::{Command, Stdio};
        let (bin, rest) = argv.split_first()?;
        // Everything that goes here is queries whose output is parsed (`wpctl
        // status`, `pactl`, `arecord -l`: alsa-utils translates "card N: …").
        // This path runs no programs meant for the person.
        let mut child = crate::utils::parsed_output_locale(&mut Command::new(bin))
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        // Readers in their own threads: otherwise a full pipe would hang the observation.
        let t_out = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = out.as_mut() {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        });
        let t_err = std::thread::spawn(move || {
            let mut sink = Vec::new();
            if let Some(p) = err.as_mut() {
                let _ = p.read_to_end(&mut sink);
            }
        });
        let status = wait_with_deadline(&mut child, timeout);
        let stdout = t_out.join().unwrap_or_default();
        let _ = t_err.join();
        // A timeout — like the original's `subprocess.TimeoutExpired`: a failure.
        let code = status?;
        Some(ObsOut {
            ok: code == 0,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
        })
    }
}

/// Wait for the process to finish for no longer than `timeout` seconds; `None` —
/// did not finish in time (port of `subprocess.run(timeout=…)` / `proc.wait(timeout=…)`).
/// On timeout the process is killed so as not to leave an orphan.
fn wait_with_deadline(child: &mut std::process::Child, timeout: f64) -> Option<i32> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Some(st.code().unwrap_or(-1)),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

/// `proc.terminate()` — a gentle stop (SIGTERM), so that `pw-record` has time to
/// detach from the PipeWire graph. std's `Child::kill()` sends SIGKILL and gives no
/// such chance.
fn send_sigterm(child: &std::process::Child) {
    // SAFETY: kill(2) on our own child that we have not reaped yet
    // — the pid belongs to us and has not been reused.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
}

/// The pipes of live capture subprocesses as a frame source.
struct PipeStreams {
    pipes: Vec<Option<std::process::ChildStdout>>,
}

impl PcmStreams for PipeStreams {
    fn count(&self) -> usize {
        self.pipes.len()
    }

    fn read(&mut self, i: usize, max: usize) -> Option<Vec<u8>> {
        let slot = self.pipes.get_mut(i)?;
        let pipe = slot.as_mut()?;
        // Read up to `max` or to the end — just like Python's `BufferedReader
        // .read(n)`. Otherwise a partial read would be presented to the loop as an
        // underrun and would mix silence frames into live sound.
        let mut buf = vec![0u8; max];
        let mut got = 0usize;
        while got < max {
            match pipe.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    // A read error — the stream is dead (port of the original's `except`),
                    // but what was already read is not lost.
                    if got == 0 {
                        return None;
                    }
                    break;
                }
            }
        }
        buf.truncate(got);
        Some(buf)
    }
}

/// Shared state between the background thread and the caller — port of `self._lock`
/// around `_bands` / `_raw_bands` / `_peak` and the `self._running` flag.
struct Shared {
    running: AtomicBool,
    data: Mutex<Snapshot>,
}

/// Whole system audio capture: backend choice, subprocesses, the background
/// thread and thread-safe data — port of the `AudioCapture` class.
///
/// The external API behavior repeats the original: the backend is chosen at
/// creation, [`start`](Self::start) brings up one or two subprocesses and
/// the background thread, [`stop`](Self::stop) stops them gently (SIGTERM, a 2 s wait,
/// then SIGKILL) and waits for the thread, zeroing the data.
pub struct CaptureRuntime {
    backend: Option<&'static str>,
    source: &'static str,
    pipeline: &'static str,
    num_bands: usize,
    smoothing: f64,
    pink_correction: bool,
    shared: Arc<Shared>,
    children: Vec<std::process::Child>,
    thread: Option<std::thread::JoinHandle<()>>,
    notes: Vec<String>,
}

impl CaptureRuntime {
    /// Create a capture and choose the backend right away — port of `AudioCapture.__init__`
    /// (auto-detection PipeWire → PulseAudio → ALSA).
    pub fn new(
        num_bands: usize,
        smoothing: f64,
        source: &str,
        pink_correction: bool,
        pipeline: &str,
    ) -> Self {
        let source = normalize_source(source);
        let pipeline = normalize_pipeline(pipeline);
        let mut io = LiveCaptureIo;
        let backend = detect_backend_live(&mut io);
        let mut notes = Vec::new();
        match backend {
            Some(name) => notes.push(format!("Audio backend: {name} (source={source})")),
            None => notes.push(
                "Audio capture недоступен: не найден ни PipeWire, ни PulseAudio, \
                 ни ALSA. Audio-reactive отключён."
                    .into(),
            ),
        }
        CaptureRuntime {
            backend,
            source,
            pipeline,
            num_bands,
            smoothing,
            pink_correction,
            shared: Arc::new(Shared {
                running: AtomicBool::new(false),
                data: Mutex::new(Snapshot::zeroed(num_bands)),
            }),
            children: Vec::new(),
            thread: None,
            notes,
        }
    }

    /// Name of the active backend — port of the `backend_name` property.
    pub fn backend_name(&self) -> &'static str {
        self.backend.unwrap_or("none")
    }

    /// Whether there is a backend — port of the `available` property.
    pub fn available(&self) -> bool {
        self.backend.is_some()
    }

    /// The normalized source.
    pub fn source(&self) -> &'static str {
        self.source
    }

    /// The chosen pipeline.
    pub fn pipeline(&self) -> &'static str {
        self.pipeline
    }

    /// Take the accumulated log lines (the executor prints them).
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Whether capture is running — port of `is_active()`.
    pub fn is_active(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst) && !self.children.is_empty()
    }

    /// Start capturing — port of `start()`: the plan, subprocesses, the background thread.
    pub fn start(&mut self) -> bool {
        if self.shared.running.load(Ordering::SeqCst) {
            return true;
        }
        let Some(backend) = self.backend else {
            self.notes.push("Audio capture: нет доступного бэкенда".into());
            return false;
        };

        let mut io = LiveCaptureIo;
        let plan = capture_plan(&mut io, backend, self.source);
        self.notes.extend(plan.notes);

        let mut pipes: Vec<Option<std::process::ChildStdout>> = Vec::new();
        for argv in &plan.argvs {
            match spawn_capture(argv) {
                Some(mut child) => {
                    self.notes.push(format!(
                        "{backend} захват запущен ({}, PID={})",
                        argv.join(" "),
                        child.id()
                    ));
                    pipes.push(child.stdout.take());
                    self.children.push(child);
                }
                None => self
                    .notes
                    .push(format!("{backend} start error: {}", argv.join(" "))),
            }
        }
        if self.children.is_empty() {
            return false;
        }
        if self.source == SOURCE_BOTH && self.children.len() == 1 {
            self.notes.push(format!(
                "{backend} 'both': запущен только один subprocess (второй упал)"
            ));
        }

        self.shared.running.store(true, Ordering::SeqCst);
        let shared = Arc::clone(&self.shared);
        let (pipeline, num_bands, smoothing, pink) = (
            self.pipeline,
            self.num_bands,
            self.smoothing,
            self.pink_correction,
        );
        let streams_count = pipes.len();
        self.thread = std::thread::Builder::new()
            .name("rexon-audio-capture".into())
            .spawn(move || {
                let mut engine =
                    Engine::new(pipeline, num_bands, smoothing, pink, streams_count);
                let mut streams = PipeStreams { pipes };
                while shared.running.load(Ordering::SeqCst) {
                    if !engine.step(&mut streams) {
                        break;
                    }
                    if let Ok(mut slot) = shared.data.lock() {
                        *slot = engine.snapshot();
                    }
                }
            })
            .ok();
        self.thread.is_some()
    }

    /// Stop capturing — port of `stop()`.
    ///
    /// The order matters: first the flag is cleared, then the subprocesses are stopped, and
    /// only then the thread is awaited. A subprocess's death closes the pipe, the read
    /// returns EOF, and the loop exits by itself — so waiting for the thread cannot
    /// hang on a blocking read.
    pub fn stop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
        for child in self.children.iter_mut() {
            send_sigterm(child);
            if wait_with_deadline(child, 2.0).is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        self.children.clear();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Ok(mut slot) = self.shared.data.lock() {
            *slot = Snapshot::zeroed(self.num_bands);
        }
        self.notes.push("Audio capture остановлен".into());
    }

    /// The current data snapshot — port of `get_bands`/`get_raw_bands`/`get_peak`
    /// under the shared `self._lock`.
    pub fn snapshot(&self) -> Snapshot {
        self.shared
            .data
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
}

impl Drop for CaptureRuntime {
    /// No orphaned `pw-record` must be left after us even when
    /// `stop()` was forgotten.
    fn drop(&mut self) {
        if !self.children.is_empty() || self.thread.is_some() {
            self.stop();
        }
    }
}

/// Start one capture subprocess: stdout — into the pipe, stderr — nowhere
/// (port of `subprocess.Popen(cmd, stdout=PIPE, stderr=DEVNULL)`).
fn spawn_capture(argv: &[String]) -> Option<std::process::Child> {
    use std::process::{Command, Stdio};
    let (bin, rest) = argv.split_first()?;
    Command::new(bin)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}
