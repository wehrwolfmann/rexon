//! Golden check of the EXECUTING part of audio capture against the recorded golden file
//! of the live Python path.
//!
//! The neighbour [`audio_capture_parity`](../audio_capture_parity.rs) checks the pure
//! math; here is what remained in Python until this round: assembling
//! a frame from one/two capture streams ([`FrameAssembler`], port of
//! `_read_frame_mixed`) and the whole capture loop ([`Engine`], port of
//! `_rs_capture_loop` together with both pipelines), as well as the subprocess
//! launch plan ([`capture_plan`], port of `available()` / `_find_*` /
//! `start_multi`).
//!
//! The golden file is the recorded file of the former Python check (since 02.10.2026 it lives
//! here, `tests/golden/audio_capture_reference.json`). It was captured from the
//! LIVE `rexon.audio_capture` before the move, so a match here
//! means exactly what is needed: the Rust path repeats the behaviour the
//! Python path had, quirks included (floor division of the mix, a silence frame on
//! underrun, death of one stream out of two).
//!
//! Real sound is NOT launched: the streams are prepared synthetic PCM,
//! environment observations are a fixture table instead of `pactl`/`wpctl`/`arecord -l`.

use rexon_core::audio_capture as ac;
use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::PI;

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "golden/audio_capture_reference.json"
    ))
    .unwrap()
}

fn f_vec(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

fn assert_vec_close(got: &[f64], want: &[f64], ctx: &str) {
    assert_eq!(got.len(), want.len(), "{ctx}: длина");
    for (i, (a, b)) in got.iter().zip(want).enumerate() {
        assert!(
            (a - b).abs() <= 1e-12 * b.abs().max(1.0),
            "{ctx}[{i}]: {a} против эталона {b}"
        );
    }
}

// ── Synthetic PCM: byte-exact port of `synth_pcm` from the Python check ──

fn synth_pcm(kind: &str, n_samples: usize, t0: i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(n_samples * 4);
    for i in 0..n_samples {
        let t = t0 + i as i64;
        let tf = t as f64;
        let (l, r): (i16, i16) = match kind {
            "sine" => (
                (20000.0 * (2.0 * PI * 440.0 * tf / 44100.0).sin()) as i16,
                (15000.0 * (2.0 * PI * 880.0 * tf / 44100.0 + 0.5).sin()) as i16,
            ),
            "sine2" => (
                (9000.0 * (2.0 * PI * 120.0 * tf / 44100.0).sin()) as i16,
                (4000.0 * (2.0 * PI * 9000.0 * tf / 44100.0).sin()) as i16,
            ),
            "clip" => (
                if (t / 16) % 2 == 0 { 32767 } else { -32768 },
                if (t / 24) % 2 == 0 { -32768 } else { 32767 },
            ),
            _ => (0, 0),
        };
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    out
}

/// Port of `FakePipe`: `chunk` limits a single read (a slow source
/// → underrun silence frames).
struct FakePipes {
    data: Vec<Vec<u8>>,
    pos: Vec<usize>,
    chunk: Vec<Option<usize>>,
}

impl FakePipes {
    fn one(data: Vec<u8>) -> Self {
        FakePipes {
            data: vec![data],
            pos: vec![0],
            chunk: vec![None],
        }
    }

    fn many(streams: Vec<(Vec<u8>, Option<usize>)>) -> Self {
        let mut data = Vec::new();
        let mut chunk = Vec::new();
        for (d, c) in streams {
            data.push(d);
            chunk.push(c);
        }
        let pos = vec![0; data.len()];
        FakePipes { data, pos, chunk }
    }
}

impl ac::PcmStreams for FakePipes {
    fn count(&self) -> usize {
        self.data.len()
    }

    fn read(&mut self, i: usize, max: usize) -> Option<Vec<u8>> {
        let take = match self.chunk[i] {
            Some(c) => max.min(c),
            None => max,
        };
        let start = self.pos[i];
        let end = (start + take).min(self.data[i].len());
        self.pos[i] = end;
        Some(self.data[i][start..end].to_vec())
    }
}

/// Run the loop to the end of the stream and take the same quantities that
/// `_stats` of the Python check took.
fn run_loop(
    pipeline: &str,
    num_bands: usize,
    streams: &mut FakePipes,
) -> (ac::Snapshot, f64, f64, f64) {
    let count = ac::PcmStreams::count(streams);
    let mut engine = ac::Engine::new(pipeline, num_bands, ac::SMOOTHING, true, count);
    while engine.step(streams) {}
    let snap = engine.snapshot();
    let (bass, mid, high) = (
        ac::bass_level(&snap.bands),
        ac::mid_level(&snap.bands),
        ac::high_level(&snap.bands),
    );
    (snap, bass, mid, high)
}

fn check_loop(key: &str, pipeline: &str, num_bands: usize, streams: &mut FakePipes) {
    let want = &reference()[key];
    let (snap, bass, mid, high) = run_loop(pipeline, num_bands, streams);
    assert_vec_close(&snap.bands, &f_vec(&want["bands"]), &format!("{key}.bands"));
    assert_vec_close(
        &snap.raw_bands,
        &f_vec(&want["raw"]),
        &format!("{key}.raw"),
    );
    for (got, w, name) in [
        (snap.peak, want["peak"].as_f64().unwrap(), "peak"),
        (bass, want["bass"].as_f64().unwrap(), "bass"),
        (mid, want["mid"].as_f64().unwrap(), "mid"),
        (high, want["high"].as_f64().unwrap(), "high"),
    ] {
        assert!(
            (got - w).abs() <= 1e-12 * w.abs().max(1.0),
            "{key}.{name}: {got} против эталона {w}"
        );
    }
}

/// Check frames: sine → silence → clipping → a second sine.
fn scenario_frames() -> Vec<u8> {
    let n = ac::FFT_SIZE;
    let mut f = synth_pcm("sine", n, 0);
    f.extend(synth_pcm("silence", n, 0));
    f.extend(synth_pcm("clip", n, 0));
    f.extend(synth_pcm("sine2", n, n as i64));
    f
}

#[test]
fn loop_v9_we_and_mono() {
    check_loop(
        "loop_v9_128",
        "v9",
        128,
        &mut FakePipes::one(scenario_frames()),
    );
    check_loop("loop_v9_16", "v9", 16, &mut FakePipes::one(scenario_frames()));
}

#[test]
fn loop_legacy_we_and_mono() {
    check_loop(
        "loop_legacy_128",
        "legacy",
        128,
        &mut FakePipes::one(scenario_frames()),
    );
    check_loop(
        "loop_legacy_16",
        "legacy",
        16,
        &mut FakePipes::one(scenario_frames()),
    );
}

#[test]
fn loop_both_streams_with_underrun() {
    // The first stream is slow (chunk=10000 → underrun silence frames), the second
    // is shorter and dies after the first frame.
    let n = ac::FFT_SIZE;
    let mut s1 = synth_pcm("sine", n, 0);
    s1.extend(synth_pcm("sine2", n, n as i64));
    s1.extend(synth_pcm("sine", n, 2 * n as i64));
    let s2 = synth_pcm("sine2", n, 0);
    let mut pipes = FakePipes::many(vec![(s1, Some(10000)), (s2, None)]);
    check_loop("loop_both", "v9", 128, &mut pipes);
}

#[test]
fn frame_assembler_matches_recorded_quirks() {
    let r = reference();

    // Floor division quirk: (-3 + 0) // 2 = -2 (floor, not truncation toward zero).
    let a: Vec<u8> = [-3i16, 5, -32768, 32767]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let b: Vec<u8> = [0i16, 2, -32768, -32768]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();

    let mut asm = ac::FrameAssembler::new(2, 8);
    let mut pipes = FakePipes::many(vec![(a.clone(), None), (b.clone(), None)]);
    let mut got: Vec<String> = Vec::new();
    while let Some(frame) = asm.next_frame(&mut pipes) {
        got.push(frame.iter().map(|x| format!("{x:02x}")).collect());
    }
    let want: Vec<String> = r["mix_floor"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want, "mix_floor");

    // A single stream — the frame as is.
    let mut asm = ac::FrameAssembler::new(1, 8);
    let mut pipes = FakePipes::one(a);
    let frame = asm.next_frame(&mut pipes).expect("кадр одиночного потока");
    let hex: String = frame.iter().map(|x| format!("{x:02x}")).collect();
    assert_eq!(hex, r["mix_single"].as_str().unwrap(), "mix_single");

    // A dead stream → end of the loop.
    let mut asm = ac::FrameAssembler::new(1, 8);
    let mut pipes = FakePipes::one(Vec::new());
    assert!(asm.next_frame(&mut pipes).is_none(), "mix_dead");
    assert!(r["mix_dead"].is_null(), "эталон mix_dead");
}

// ── Launch plan: environment observations are fake, no live commands ────

/// Fake executor: a table "space-joined argv → (rc, stdout)",
/// port of `with_fixture` from the Python check.
struct FixtureIo {
    present: Vec<&'static str>,
    table: HashMap<String, (i32, String)>,
}

impl FixtureIo {
    fn new(present: Vec<&'static str>, pairs: &[(&str, &str)]) -> Self {
        FixtureIo {
            present,
            table: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (0, (*v).to_string())))
                .collect(),
        }
    }
}

impl ac::CaptureIo for FixtureIo {
    fn has_cmd(&mut self, cmd: &str) -> bool {
        self.present.contains(&cmd)
    }

    fn run(&mut self, argv: &[String], _timeout: f64) -> Option<ac::ObsOut> {
        let key = argv.join(" ");
        match self.table.get(&key) {
            Some((rc, out)) => Some(ac::ObsOut {
                ok: *rc == 0,
                stdout: out.clone(),
            }),
            None => Some(ac::ObsOut::default()),
        }
    }
}

const SOURCES_FULL: &str = "55\talsa_output.pci.analog-stereo.monitor\tPipeWire\ts16le 2ch 44100Hz\tIDLE\n\
     56\talsa_input.pci.analog-stereo\tPipeWire\ts16le 2ch 44100Hz\tRUNNING\n\
     57\tother_sink.monitor\tPipeWire\ts16le\tIDLE\n";

/// The `pw-record` target from the built argv (the value after `--target`).
fn pw_target(argv: &[String]) -> &str {
    let i = argv.iter().position(|a| a == "--target").expect("--target");
    &argv[i + 1]
}

#[test]
fn plan_pipewire_targets_match_reference() {
    let r = reference();
    let pactl = [
        ("pactl get-default-sink", "alsa_output.pci.analog-stereo\n"),
        ("pactl list short sources", SOURCES_FULL),
    ];
    // No wpctl on the system — the path goes through pactl, exactly as in the golden cases.
    let mut io = FixtureIo::new(vec!["pw-record", "pactl"], &pactl);
    let plan = ac::capture_plan(&mut io, "pipewire", "monitor");
    assert_eq!(plan.argvs.len(), 1);
    assert_eq!(
        pw_target(&plan.argvs[0]),
        r["pw_mon_ok"].as_str().unwrap(),
        "pw_mon_ok"
    );

    // The default sink is not found among the sources — the first monitor is taken.
    let mut io = FixtureIo::new(
        vec!["pw-record", "pactl"],
        &[
            ("pactl get-default-sink", "ghost_sink\n"),
            ("pactl list short sources", SOURCES_FULL),
        ],
    );
    let plan = ac::capture_plan(&mut io, "pipewire", "monitor");
    assert_eq!(
        pw_target(&plan.argvs[0]),
        r["pw_mon_susp"].as_str().unwrap(),
        "pw_mon_susp"
    );

    // Neither a sink nor a list — the universal PipeWire target.
    let mut io = FixtureIo::new(vec!["pw-record"], &[]);
    let plan = ac::capture_plan(&mut io, "pipewire", "monitor");
    assert_eq!(
        pw_target(&plan.argvs[0]),
        r["pw_mon_empty"].as_str().unwrap(),
        "pw_mon_empty"
    );

    // Microphone.
    let mut io = FixtureIo::new(
        vec!["pw-record", "pactl"],
        &[(
            "pactl get-default-source",
            "alsa_input.pci.analog-stereo\n",
        )],
    );
    let plan = ac::capture_plan(&mut io, "pipewire", "microphone");
    assert_eq!(
        pw_target(&plan.argvs[0]),
        r["pw_mic_ok"].as_str().unwrap(),
        "pw_mic_ok"
    );

    // 'both' — two subprocesses: monitor and microphone.
    let mut io = FixtureIo::new(vec!["pw-record", "pactl"], &pactl);
    let plan = ac::capture_plan(&mut io, "pipewire", "both");
    assert_eq!(plan.argvs.len(), 2, "'both' — два подпроцесса");
    assert_eq!(pw_target(&plan.argvs[0]), r["pw_mon_ok"].as_str().unwrap());
    // The fixture has no default source — the microphone is taken from the list
    // of sources (the first non-monitor), as in the golden case `pw_mic_mon`.
    assert_eq!(
        pw_target(&plan.argvs[1]),
        r["pw_mic_mon"].as_str().unwrap(),
        "микрофон из списка источников"
    );
}

#[test]
fn plan_pipewire_prefers_wireplumber() {
    // The same graph snapshot as in the golden wireplumber branch.
    let wp = "Audio\n \u{251c}\u{2500} Sinks:\n \u{2502}  *   55. \u{412}\u{441}\u{442}\u{440}\u{43e}\u{435}\u{43d}\u{43d}\u{43e}\u{435} \u{430}\u{443}\u{434}\u{438}\u{43e}                [vol: 0.65]\n \u{2502}      56. HDMI                            [vol: 1.00]\n \u{251c}\u{2500} Sources:\n \u{2502}      60. alsa_output.pci.analog.monitor  [vol: 1.00]\n \u{2502}  *   61. \u{41c}\u{438}\u{43a}\u{440}\u{43e}\u{444}\u{43e}\u{43d}                        [vol: 0.80]\n";
    let r = reference();
    let want = &r["wpctl_targets"][0];
    let mut io = FixtureIo::new(vec!["pw-record", "wpctl"], &[("wpctl status -n", wp)]);
    let plan = ac::capture_plan(&mut io, "pipewire", "monitor");
    assert_eq!(pw_target(&plan.argvs[0]), want[0].as_str().unwrap());
    let mut io = FixtureIo::new(vec!["pw-record", "wpctl"], &[("wpctl status -n", wp)]);
    let plan = ac::capture_plan(&mut io, "pipewire", "microphone");
    assert_eq!(pw_target(&plan.argvs[0]), want[1].as_str().unwrap());
}

#[test]
fn plan_pulse_and_alsa() {
    let r = reference();
    // PulseAudio: the device exists.
    let mut io = FixtureIo::new(
        vec!["parec", "pactl"],
        &[
            ("pactl get-default-sink", "alsa_output.pci.analog-stereo\n"),
            ("pactl list short sources", SOURCES_FULL),
        ],
    );
    let plan = ac::capture_plan(&mut io, "pulseaudio", "monitor");
    assert_eq!(
        plan.argvs[0]
            .iter()
            .find(|a| a.starts_with("--device="))
            .unwrap(),
        &format!("--device={}", r["pu_mon_ok"].as_str().unwrap())
    );

    // PulseAudio: no device — no subprocess at all (golden: None).
    assert!(r["pu_mon_empty"].is_null());
    let mut io = FixtureIo::new(vec!["parec"], &[]);
    let plan = ac::capture_plan(&mut io, "pulseaudio", "monitor");
    assert!(plan.argvs.is_empty(), "нет устройства — нет запуска");
    assert!(plan.notes.iter().any(|n| n.contains("source не найден")));

    // ALSA: loopback found.
    let mut io = FixtureIo::new(
        vec!["arecord"],
        &[(
            "arecord -l",
            "**** List of CAPTURE Hardware Devices ****\n\
             card 1: Loopback [Loopback], device 0: Loopback PCM\n\
             card 2: PCH [HDA Intel PCH], device 0: ALC287\n",
        )],
    );
    let plan = ac::capture_plan(&mut io, "alsa", "monitor");
    assert!(
        plan.argvs[0].contains(&r["alsa_loop"].as_str().unwrap().to_string()),
        "loopback в argv"
    );

    // ALSA: loopback not found — default plus a warning about snd-aloop.
    assert!(r["alsa_noloop"].is_null());
    let mut io = FixtureIo::new(vec!["arecord"], &[("arecord -l", "card 0: PCH\n")]);
    let plan = ac::capture_plan(&mut io, "alsa", "monitor");
    assert!(plan.argvs[0].contains(&"default".to_string()));
    assert!(plan.notes.iter().any(|n| n.contains("snd-aloop")));

    // ALSA 'both' is not supported — one subprocess and a warning.
    let mut io = FixtureIo::new(vec!["arecord"], &[]);
    let plan = ac::capture_plan(&mut io, "alsa", "both");
    assert_eq!(plan.argvs.len(), 1);
    assert!(plan
        .notes
        .iter()
        .any(|n| n.contains("'both' не поддерживается")));
}

#[test]
fn spawn_argv_matches_reference() {
    let r = reference();
    let want = r["spawn_argv"].as_array().unwrap();
    let as_vec = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ac::pw_record_argv("mon.target"), as_vec(&want[0]));
    assert_eq!(ac::parec_argv("dev.monitor"), as_vec(&want[1]));
    assert_eq!(ac::arecord_argv("hw:1,1"), as_vec(&want[2]));
}

#[test]
fn backend_detection_matches_reference() {
    let r = reference();
    let want = r["detect_backend"].as_array().unwrap();
    // The same five availability combinations as in the golden file.
    let combos = [
        (true, true, true),
        (false, true, true),
        (false, false, true),
        (false, false, false),
        (true, false, false),
    ];
    for (i, (pw, pu, al)) in combos.iter().enumerate() {
        let mut present: Vec<&'static str> = Vec::new();
        let mut probes: Vec<(&str, &str)> = Vec::new();
        if *pw {
            present.push("pw-record");
            probes.push(("pw-cli info 0", ""));
        }
        if *pu {
            present.push("parec");
            probes.push(("pactl info", ""));
        }
        if *al {
            present.push("arecord");
        }
        let mut io = FixtureIo::new(present, &probes);
        let got = ac::detect_backend_live(&mut io);
        match want[i].as_str() {
            Some(name) => assert_eq!(got, Some(name), "detect_backend[{i}]"),
            None => assert!(got.is_none(), "detect_backend[{i}]"),
        }
    }
}

#[test]
fn pipeline_normalization() {
    assert_eq!(ac::normalize_pipeline("v9"), "v9");
    assert_eq!(ac::normalize_pipeline("legacy"), "legacy");
    // A historical CLI bug: the default was 'v23' — a silent fallback to
    // legacy. Now any unknown name gives the calibrated v9.
    assert_eq!(ac::normalize_pipeline("v23"), "v9");
    assert_eq!(ac::normalize_pipeline(""), "v9");
}

#[test]
fn vu_meter_texts() {
    assert_eq!(
        ac::vu_levels_line(0.5, 0.25, 0.125, 1.0),
        "  Bass: 0.50  Mid: 0.25  High: 0.12  Peak: 1.00"
    );
    let lines = ac::vu_bar_lines(&[0.0, 1.0], 4);
    assert_eq!(lines[0], "   0 [░░░░] 0.00");
    assert_eq!(lines[1], "   1 [████] 1.00");
    assert_eq!(
        ac::vu_compact_line(0.1, 0.2, 0.3, &[0.05, 0.95]),
        "  B=0.10 M=0.20 H=0.30 | 0.1 0.9    "
    );
}
