//! Golden-сверка порта `audio_capture.rs` с оригиналом
//! `rexon/audio_capture.py`.
//!
//! Эталон снят read-only с ЖИВОГО `rexon.audio_capture` одноразовым
//! генератором: subprocess.run/Popen замоканы (реальные pactl/pw-record/
//! arecord НЕ запускались), `HAS_NUMPY=False` — снят pure-Python путь (та же
//! последовательность fp-операций, что в порте). Capture-циклы прогнаны на
//! синтетическом PCM (синус 440/880 Гц, тишина, клиппинг-меандр) через
//! BytesIO-стримы — сами PCM-сэмплы лежат в фикстуре. Покрыто: константы,
//! окна Hamming/Hann, FFT/амплитуды, линейные полосы, v9-границы и
//! MAX-редукция, pink-факторы, срезы bass/mid/high, микс 'both', оба полных
//! конвейера (по 2 кадра — сглаживание с ненулевым prev), argv команд,
//! выбор monitor/microphone/loopback-устройств, JSON-stream строка.
//!
//! Числовая сверка — с относительной точностью 1e-9 (одна libm, одна
//! последовательность операций; допуск страхует последний ulp).
//!
//! `golden/audio_capture_cases.json` — снятые значения.

use rexon_core::audio_capture as ac;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/audio_capture_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn f_vec(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(f).collect()
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| s(x).to_string())
        .collect()
}

fn i16_vec(v: &Value) -> Vec<i16> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_i64().unwrap() as i16)
        .collect()
}

fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

fn assert_close(a: f64, b: f64, ctx: &str) {
    let tol = 1e-9 * a.abs().max(b.abs()).max(1.0);
    assert!((a - b).abs() <= tol, "{ctx}: {a} != {b}");
}

fn assert_vec_close(a: &[f64], b: &[f64], ctx: &str) {
    assert_eq!(a.len(), b.len(), "{ctx}: len {} != {}", a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_close(*x, *y, &format!("{ctx}[{i}]"));
    }
}

#[test]
fn constants() {
    let g = fx();
    let c = &g["constants"];
    assert_eq!(ac::SAMPLE_RATE as u64, c["sample_rate"].as_u64().unwrap());
    assert_eq!(ac::CHANNELS as u64, c["channels"].as_u64().unwrap());
    assert_eq!(ac::SAMPLE_FORMAT, s(&c["sample_format"]));
    assert_eq!(
        ac::BYTES_PER_SAMPLE as u64,
        c["bytes_per_sample"].as_u64().unwrap()
    );
    assert_eq!(ac::FFT_SIZE as u64, c["fft_size"].as_u64().unwrap());
    assert_eq!(ac::NUM_BANDS as u64, c["num_bands"].as_u64().unwrap());
    assert_eq!(ac::SMOOTHING, f(&c["smoothing"]));
    assert_eq!(ac::NOISE_FLOOR, f(&c["noise_floor"]));
    assert_eq!(ac::READ_CHUNK as u64, c["read_chunk"].as_u64().unwrap());
    assert_eq!(ac::PINK_CORRECTION_FMIN, f(&c["pink_fmin"]));
    assert_eq!(ac::V9_SPREAD_POWER, f(&c["v9_spread_power"]));
    assert_eq!(ac::V9_EXPONENT, f(&c["v9_exponent"]));
    assert_eq!(ac::V9_FREQ_MIN, f(&c["v9_freq_min"]));
    assert_eq!(ac::V9_FREQ_MAX, f(&c["v9_freq_max"]));
    assert_eq!(ac::V9_GAIN, f(&c["v9_gain"]));
    assert_eq!(
        ac::V9_BANDS_PER_CHANNEL as u64,
        c["v9_bands_per_channel"].as_u64().unwrap()
    );
}

#[test]
fn windows() {
    let g = fx();
    assert_vec_close(&ac::hamming_window(16), &f_vec(&g["hamming_16"]), "hamming16");
    assert_vec_close(&ac::hann_window(16), &f_vec(&g["hann_16"]), "hann16");

    let hamming = ac::hamming_window(ac::FFT_SIZE);
    for (idx, val) in g["hamming_4096_samples"].as_object().unwrap() {
        let i: usize = idx.parse().unwrap();
        assert_close(hamming[i], f(val), &format!("hamming4096[{i}]"));
    }
    let hann = ac::hann_window(ac::FFT_SIZE);
    for (idx, val) in g["hann_4096_samples"].as_object().unwrap() {
        let i: usize = idx.parse().unwrap();
        assert_close(hann[i], f(val), &format!("hann4096[{i}]"));
    }
}

#[test]
fn fft_and_magnitudes() {
    let g = fx();
    // rfft_magnitudes покрывает и внутренний fft (порядок бабочек/битреверса)
    let rin = f_vec(&g["rfft16"]["input"]);
    assert_vec_close(
        &ac::rfft_magnitudes(&rin),
        &f_vec(&g["rfft16"]["magnitudes"]),
        "rfft16",
    );
    // fft8: сверяем амплитуды по снятому спектру (re, im)
    let fin = f_vec(&g["fft8"]["input"]);
    let mags = ac::rfft_magnitudes(&fin);
    let expected: Vec<f64> = g["fft8"]["output"].as_array().unwrap()[..4]
        .iter()
        .map(|c| {
            let re = f(&c[0]);
            let im = f(&c[1]);
            (re * re + im * im).sqrt() / 4.0
        })
        .collect();
    assert_vec_close(&mags, &expected, "fft8-магнитуды");
}

#[test]
fn linear_bands() {
    let g = fx();
    let mags = f_vec(&g["m2b"]["magnitudes"]);
    assert_vec_close(
        &ac::magnitudes_to_bands(&mags, 16, ac::SAMPLE_RATE),
        &f_vec(&g["m2b"]["bands16"]),
        "bands16",
    );
    assert_vec_close(
        &ac::magnitudes_to_bands(&mags, 8, ac::SAMPLE_RATE),
        &f_vec(&g["m2b"]["bands8"]),
        "bands8",
    );
    assert_vec_close(
        &ac::magnitudes_to_bands(&[], 16, ac::SAMPLE_RATE),
        &f_vec(&g["m2b"]["empty"]),
        "bands-empty",
    );
}

fn edge_pairs(v: &Value) -> Vec<(usize, usize)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_u64().unwrap() as usize,
                p[1].as_u64().unwrap() as usize,
            )
        })
        .collect()
}

#[test]
fn v9_edges() {
    let g = fx();
    assert_eq!(
        ac::v9_bin_edges(64, ac::SAMPLE_RATE, ac::FFT_SIZE / 2),
        edge_pairs(&g["v9_edges_64"]),
        "edges64"
    );
    assert_eq!(
        ac::v9_bin_edges(16, ac::SAMPLE_RATE, ac::FFT_SIZE / 2),
        edge_pairs(&g["v9_edges_16"]),
        "edges16"
    );
}

#[test]
fn v9_max_reduction() {
    let g = fx();
    let mags = f_vec(&g["v9_reduce"]["magnitudes"]);
    let edges = edge_pairs(&g["v9_reduce"]["edges"]);
    assert_vec_close(
        &ac::v9_reduce_max(&mags, &edges),
        &f_vec(&g["v9_reduce"]["out"]),
        "v9_reduce",
    );
}

#[test]
fn pink_factors() {
    let g = fx();
    assert_vec_close(&ac::build_pink_factors(64), &f_vec(&g["pink_64"]), "pink64");
    assert_vec_close(&ac::build_pink_factors(16), &f_vec(&g["pink_16"]), "pink16");
}

#[test]
fn band_slices() {
    let g = fx();
    for key in ["band_slice_128", "band_slice_16"] {
        let case = &g[key];
        let bands = f_vec(&case["bands"]);
        assert_close(ac::bass_level(&bands), f(&case["bass"]), &format!("{key} bass"));
        assert_close(ac::mid_level(&bands), f(&case["mid"]), &format!("{key} mid"));
        assert_close(ac::high_level(&bands), f(&case["high"]), &format!("{key} high"));
    }
}

#[test]
fn both_mode_mix() {
    let g = fx();
    let a = pcm_bytes(&i16_vec(&g["mix"]["a"]));
    let b = pcm_bytes(&i16_vec(&g["mix"]["b"]));
    let mixed = ac::mix_frames_s16le(&[&a, &b]);
    assert_eq!(mixed, pcm_bytes(&i16_vec(&g["mix"]["mixed"])), "mix2");
    // один стрим — passthrough
    let single = ac::mix_frames_s16le(&[&a]);
    assert_eq!(single, pcm_bytes(&i16_vec(&g["mix_single"])), "mix1");
    // кадр тишины (underrun)
    assert_eq!(ac::silence_frame(8), vec![0u8; 8], "silence");
}

fn run_v9(pcm: &[i16], num_bands: usize, frames: usize) -> ac::V9Analyzer {
    let bytes = pcm_bytes(pcm);
    let mut an = ac::V9Analyzer::new(num_bands, ac::SMOOTHING);
    for _ in 0..frames {
        an.process_frame(&bytes);
    }
    an
}

fn run_legacy(pcm: &[i16], num_bands: usize, frames: usize, pink: bool) -> ac::LegacyAnalyzer {
    let bytes = pcm_bytes(pcm);
    let mut an = ac::LegacyAnalyzer::new(num_bands, ac::SMOOTHING, pink);
    for _ in 0..frames {
        an.process_frame(&bytes);
    }
    an
}

#[test]
fn v9_pipeline() {
    let g = fx();
    for (pcm_key, case_key, num_bands) in [
        ("sine", "v9_sine_128", 128),
        ("silence", "v9_silence_128", 128),
        ("clip", "v9_clip_128", 128),
        ("sine", "v9_sine_16", 16),
    ] {
        let pcm = i16_vec(&g["pcm"][pcm_key]);
        let an = run_v9(&pcm, num_bands, 2);
        let case = &g[case_key];
        assert_vec_close(an.bands(), &f_vec(&case["bands"]), &format!("{case_key} bands"));
        assert_vec_close(
            an.raw_bands(),
            &f_vec(&case["raw_bands"]),
            &format!("{case_key} raw"),
        );
        assert_close(an.peak(), f(&case["peak"]), &format!("{case_key} peak"));
    }
}

#[test]
fn legacy_pipeline() {
    let g = fx();
    for (pcm_key, case_key, num_bands, pink) in [
        ("sine", "legacy_sine_128", 128, true),
        ("silence", "legacy_silence_128", 128, true),
        ("clip", "legacy_clip_128", 128, true),
        ("sine", "legacy_sine_16", 16, true),
        ("sine", "legacy_sine_128_nopink", 128, false),
    ] {
        let pcm = i16_vec(&g["pcm"][pcm_key]);
        let an = run_legacy(&pcm, num_bands, 2, pink);
        let case = &g[case_key];
        assert_vec_close(an.bands(), &f_vec(&case["bands"]), &format!("{case_key} bands"));
        assert_vec_close(
            an.raw_bands(),
            &f_vec(&case["raw_bands"]),
            &format!("{case_key} raw"),
        );
        assert_close(an.peak(), f(&case["peak"]), &format!("{case_key} peak"));
    }
}

#[test]
fn capture_argv() {
    let g = fx();
    assert_eq!(ac::pw_record_argv("TARGET"), str_vec(&g["pw_record_cmd"]));
    assert_eq!(ac::parec_argv("DEVICE"), str_vec(&g["parec_cmd"]));
    assert_eq!(ac::arecord_argv("hw:1,1"), str_vec(&g["arecord_cmd"]));
    // пробы доступности — литералы из available()
    assert_eq!(ac::pipewire_probe_argv(), vec!["pw-cli", "info", "0"]);
    assert_eq!(ac::pulseaudio_probe_argv(), vec!["pactl", "info"]);
    assert_eq!(ac::pactl_default_sink_argv(), vec!["pactl", "get-default-sink"]);
    assert_eq!(
        ac::pactl_default_source_argv(),
        vec!["pactl", "get-default-source"]
    );
    assert_eq!(
        ac::pactl_short_sources_argv(),
        vec!["pactl", "list", "short", "sources"]
    );
    assert_eq!(ac::arecord_list_argv(), vec!["arecord", "-l"]);
    assert_eq!(ac::wpctl_status_argv(), vec!["wpctl", "status", "-n"]);
}

/// Ветка wireplumber (`wpctl status -n`) — там, где нет `pactl`.
/// Эталон снят с живого `wpctl status -n` (CachyOS, PipeWire 1.6.8),
/// имена узлов обезличены; команда read-only, ничего не переключает.
#[test]
fn device_selection_wpctl() {
    let g = fx();
    let c = &g["wpctl"];
    let status = s(&c["status_stdout"]);

    let parsed = ac::parse_wpctl_status(status);
    assert_eq!(parsed.sinks, str_vec(&c["sinks"]));
    assert_eq!(parsed.default_sink.as_deref(), c["default_sink"].as_str());
    assert_eq!(parsed.sources, str_vec(&c["sources"]));
    assert_eq!(
        parsed.default_source.as_deref(),
        c["default_source"].as_str()
    );

    assert_eq!(
        ac::wpctl_monitor_target(Some(status)).as_deref(),
        c["monitor_target"].as_str()
    );
    assert_eq!(
        ac::wpctl_microphone_target(Some(status)).as_deref(),
        c["microphone_target"].as_str()
    );

    // Ни один узел не помечен `*` — берём первый по списку.
    let nodef = s(&c["status_no_default"]);
    assert_eq!(
        ac::wpctl_monitor_target(Some(nodef)).as_deref(),
        c["monitor_target_no_default"].as_str()
    );
    assert_eq!(
        ac::wpctl_microphone_target(Some(nodef)).as_deref(),
        c["microphone_target_no_default"].as_str()
    );

    // Раздел Video НЕ должен просачиваться в аудио (там своя `*`-камера).
    let video = s(&c["status_video_only"]);
    let only_video = ac::parse_wpctl_status(video);
    assert!(only_video.sinks.is_empty() && only_video.sources.is_empty());
    assert_eq!(ac::wpctl_monitor_target(Some(video)), None);
    assert_eq!(ac::wpctl_microphone_target(Some(video)), None);

    // Команды нет / упала → None, вызывающий уходит на pactl и алиасы.
    assert_eq!(ac::wpctl_monitor_target(None), None);
    assert_eq!(ac::wpctl_microphone_target(None), None);
    assert_eq!(ac::wpctl_monitor_target(Some("")), None);
}

#[test]
fn device_selection_pipewire() {
    let g = fx();
    let fixtures = &g["pactl_fixtures"];
    let list = s(&fixtures["sources_list"]);
    let list_alt = s(&fixtures["sources_list_alt"]);
    let sink = s(&fixtures["default_sink"]);
    let mic = s(&fixtures["default_source_mic"]);
    let mon = s(&fixtures["default_source_monitor"]);

    let cases = &g["pw_monitor"];
    assert_eq!(
        ac::pipewire_monitor_target(Some(sink), Some(list)),
        s(&cases["exact"])
    );
    assert_eq!(
        ac::pipewire_monitor_target(Some("ghost-sink"), Some(list)),
        s(&cases["stale_sink"])
    );
    assert_eq!(
        ac::pipewire_monitor_target(Some("ghost-sink"), Some(list_alt)),
        s(&cases["stale_sink_alt"])
    );
    assert_eq!(
        ac::pipewire_monitor_target(Some(sink), None),
        s(&cases["no_list"])
    );
    assert_eq!(ac::pipewire_monitor_target(None, None), s(&cases["nothing"]));

    let cases = &g["pw_microphone"];
    assert_eq!(
        ac::pipewire_microphone_target(Some(mic), None),
        s(&cases["default"])
    );
    assert_eq!(
        ac::pipewire_microphone_target(Some(mon), Some(list)),
        s(&cases["default_is_monitor"])
    );
    assert_eq!(
        ac::pipewire_microphone_target(None, None),
        s(&cases["nothing"])
    );
}

#[test]
fn device_selection_pulse() {
    let g = fx();
    let fixtures = &g["pactl_fixtures"];
    let list = s(&fixtures["sources_list"]);
    let sink = s(&fixtures["default_sink"]);
    let mic = s(&fixtures["default_source_mic"]);
    let mon = s(&fixtures["default_source_monitor"]);

    let cases = &g["pulse_monitor"];
    assert_eq!(
        ac::pulse_monitor_device(Some(sink), None).as_deref(),
        cases["default"].as_str()
    );
    assert_eq!(
        ac::pulse_monitor_device(None, Some(list)).as_deref(),
        cases["fallback_list"].as_str()
    );
    assert_eq!(
        ac::pulse_monitor_device(None, None).as_deref(),
        cases["nothing"].as_str()
    );

    let cases = &g["pulse_microphone"];
    assert_eq!(
        ac::pulse_microphone_device(Some(mic), None).as_deref(),
        cases["default"].as_str()
    );
    assert_eq!(
        ac::pulse_microphone_device(Some(mon), Some(list)).as_deref(),
        cases["default_is_monitor"].as_str()
    );
    assert_eq!(
        ac::pulse_microphone_device(None, None).as_deref(),
        cases["nothing"].as_str()
    );
}

#[test]
fn device_selection_alsa() {
    let g = fx();
    let cases = &g["alsa_loopback"];
    let arecord_l = s(&cases["arecord_l_stdout"]);
    assert_eq!(
        ac::alsa_loopback_device(Some(arecord_l)).as_deref(),
        cases["found"].as_str()
    );
    assert_eq!(
        ac::alsa_loopback_device(Some("card 0: PCH [HDA Intel PCH]\n")).as_deref(),
        cases["absent"].as_str()
    );
    assert_eq!(ac::alsa_loopback_device(None).as_deref(), cases["fail"].as_str());

    // выбор устройства arecord по source
    assert_eq!(ac::alsa_capture_device("microphone", Some("hw:2,1")), "default");
    assert_eq!(ac::alsa_capture_device("monitor", Some("hw:2,1")), "hw:2,1");
    assert_eq!(ac::alsa_capture_device("monitor", None), "default");
    assert_eq!(ac::alsa_capture_device("both", None), "default");
}

#[test]
fn source_and_backend() {
    // нормализация source: неизвестное → monitor (порт fallback)
    assert_eq!(ac::normalize_source("monitor"), "monitor");
    assert_eq!(ac::normalize_source("microphone"), "microphone");
    assert_eq!(ac::normalize_source("both"), "both");
    assert_eq!(ac::normalize_source("bogus"), "monitor");
    // резолв CLI > env > default (порт main())
    assert_eq!(ac::resolve_source(Some("microphone"), Some("both")), "microphone");
    assert_eq!(ac::resolve_source(None, Some("both")), "both");
    assert_eq!(ac::resolve_source(None, None), "monitor");
    assert_eq!(ac::resolve_source(None, Some("bogus")), "monitor");
    // порядок детекта бэкендов
    assert_eq!(ac::detect_backend(true, true, true), Some("pipewire"));
    assert_eq!(ac::detect_backend(false, true, true), Some("pulseaudio"));
    assert_eq!(ac::detect_backend(false, false, true), Some("alsa"));
    assert_eq!(ac::detect_backend(false, false, false), None);
}

#[test]
fn stream_json() {
    let g = fx();
    let bands = f_vec(&g["stream_json"]["bands"]);
    assert_eq!(ac::stream_json_line(&bands), s(&g["stream_json"]["line"]));
}

/// `repr` float по правилам Python — эталон снят с самого CPython
/// (`json.dumps` берёт `float.__repr__`). Значения покрывают оба порога
/// (`exp < -4`, `exp >= 16`) и зону, где ryu расходился с Python.
#[test]
fn python_float_repr() {
    for (v, expect) in [
        (0.0_f64, "0.0"),
        (1.0, "1.0"),
        (0.5, "0.5"),
        (3.0, "3.0"),
        (12.345, "12.345"),
        (0.1, "0.1"),
        (0.001, "0.001"),
        (0.0001, "0.0001"),          // порог: ещё десятичная
        (1e-5, "1e-05"),             // порог: уже экспонента (ryu давал 0.00001)
        (9.9e-5, "9.9e-05"),         // ryu давал 0.000099
        (1.5e-7, "1.5e-07"),         // ryu давал 1.5e-7
        (1e-9, "1e-09"),             // ryu давал 1e-9
        (0.000123456, "0.000123456"),
        (0.1 + 0.2, "0.30000000000000004"),
        (2.0 / 3.0, "0.6666666666666666"),
        (1e15, "1000000000000000.0"),  // порог: ещё десятичная
        (1e16, "1e+16"),               // порог: уже экспонента
        (1e17, "1e+17"),
        (5e-324, "5e-324"),            // denormal: экспонента без добивки
        (-2.5, "-2.5"),
        (-1e-9, "-1e-09"),
    ] {
        assert_eq!(ac::py_float_repr(v), expect, "py_float_repr({v:?})");
    }
    assert_eq!(ac::py_float_repr(f64::NAN), "NaN");
    assert_eq!(ac::py_float_repr(f64::INFINITY), "Infinity");
    assert_eq!(ac::py_float_repr(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(ac::py_float_repr(-0.0), "-0.0");
}
