//! Чистая логика аудиозахвата для audio-reactive обоев — порт
//! `rexon/audio_capture.py`.
//!
//! Оригинальный `AudioCapture` захватывает системное аудио (PipeWire →
//! PulseAudio → ALSA), читает PCM s16le stereo @ 44100 из subprocess-пайпа и
//! в фоновом потоке гонит два конвейера спектрального анализа:
//!   * **v9** (production, калиброван 2026-05-21 к WE-Windows dump, r>0.95):
//!     Hann 4096 → rfft → quartic-root границы (64 bin/канал) → MAX-редукция →
//!     concat L+R (128) → pow(v, 0.02) × GAIN 1.9 → clamp → noise gate →
//!     motion blur + свёртка [1,2,1]/4;
//!   * **legacy**: Hamming 4096 → rfft → линейные полосы 50–22050 Гц (среднее)
//!     → pink-коррекция √(f/f_min) → whitening по long-term EMA → gamma 0.5 →
//!     clamp [0.25, 1] → blur + свёртка + post-clamp.
//!
//! Детерминированная часть: argv команд захвата (pw-record / parec /
//! arecord) и проб доступности, выбор monitor/microphone-устройства по
//! наблюдённому выводу pactl / `wpctl` / `arecord -l`, микс кадров режима
//! 'both', конвертация s16le → f64, FFT (Cooley-Tukey radix-2), окна
//! Hamming/Hann, раскладка амплитуд по полосам, оба конвейера как
//! stateful-анализаторы ([`LegacyAnalyzer`], [`V9Analyzer`]), сборка кадра из
//! потоков ([`FrameAssembler`]), тело цикла захвата ([`Engine`]), план запуска
//! подпроцессов ([`capture_plan`]), срезы bass/mid/high, тексты VU-метра и
//! строка JSON-stream IPC. Наблюдения за средой (вывод pactl, наличие команд,
//! сами PCM-кадры) подаются ПАРАМЕТРАМИ либо приходят через трейты
//! [`CaptureIo`] / [`PcmStreams`] — образец `system_plan.rs`, `importers.rs`.
//!
//! Ниже по файлу, отдельным блоком (как боевой транспорт в `net.rs`), живёт
//! ИСПОЛНИТЕЛЬ [`CaptureRuntime`]: запуск и остановка подпроцессов, чтение их
//! пайпов и фоновый поток. С 2026-08-23 это тоже Rust — на Python остался
//! только фасад `rexon/audio_capture.py` (обёртка класса, имена констант и
//! консольный тест).
//!
//! НЕ портировано: numpy fast-path (в Rust путь один — нативный),
//! `_benchmark`, разбор аргументов CLI и печать в терминал (тексты собирает
//! ядро, печатает фасад), logging (ядро копит строки в `notes`).
//!
//! Паритет с живым Python — `tests/audio_capture_parity.rs` (математика:
//! эталон снят read-only с `rexon.audio_capture`, subprocess замокан,
//! `HAS_NUMPY=False` — фиксируем pure-Python путь с той же
//! последовательностью fp-операций) и `tests/audio_capture_runtime.rs`
//! (исполнительная часть: сборка кадра, оба цикла захвата и план запуска
//! сверяются с записанным эталоном Python-пути на синтетическом PCM —
//! синус / тишина / клиппинг / два потока с underrun).
//!
//! Починки багов оригинала (правило «баги при порте чиним»):
//!   * `_v9_bin_edges` считал `hz_per_bin` от глобального `FFT_SIZE`,
//!     игнорируя собственный параметр `n_fft_bins` — при нестандартном FFT
//!     границы были бы неверны. Здесь `hz_per_bin` выводится из
//!     `n_fft_bins * 2` (для всех реальных вызовов значение то же).
//!   * Мёртвое состояние `_peak_history` / `PEAK_HISTORY_LEN` / `PEAK_TRIM`
//!     (рудимент peak-hold-нормализации, вытесненной whitening; заводится, но
//!     нигде не читается) — не портировано.
//!   * Сбивающий с толку алиас `_hann_window = _hamming_window` (наследие
//!     переименования) — не портирован; Hann и Hamming — отдельные функции.
//!   * Комментарии-рассинхроны оригинала (GAMMA «0.6» при коде 0.5,
//!     CLAMP_MIN «0.15» при коде 0.25, DIVISOR «2x avg» при коде 1.0) — в
//!     порте документация соответствует фактическим значениям.
//!
//! Сохранённый квирк (паритет, поведение не меняем): микс 'both' усредняет
//! сэмплы floor-делением (`//` в Python) — на нечётных отрицательных суммах
//! даёт смещение в −1 LSB; на слух/спектр не влияет.

use std::f64::consts::PI;

// ============================================================================
// Константы захвата (порт модульных констант audio_capture.py)
// ============================================================================

/// Частота дискретизации — порт `SAMPLE_RATE`.
pub const SAMPLE_RATE: u32 = 44100;
/// Каналы: стерео (WE-формат требует L+R split) — порт `CHANNELS`.
pub const CHANNELS: usize = 2;
/// Формат сэмпла (16-bit signed little-endian) — порт `SAMPLE_FORMAT`.
pub const SAMPLE_FORMAT: &str = "s16le";
/// Байт на сэмпл — порт `BYTES_PER_SAMPLE`.
pub const BYTES_PER_SAMPLE: usize = 2;
/// Размер FFT (WE-совместимый, ~5.4 Гц/bin) — порт `FFT_SIZE`.
pub const FFT_SIZE: usize = 4096;
/// Полос в legacy-API `get_bands` — порт `NUM_BANDS` (WE-режим — 128).
pub const NUM_BANDS: usize = 16;
/// Полос в WE-режиме: 64 L + 64 R.
pub const WE_NUM_BANDS: usize = 128;
/// Motion blur 0.34 = WE-spec (prev*0.34 + cur*0.66) — порт `SMOOTHING`.
pub const SMOOTHING: f64 = 0.34;
/// Порог «тишины» по raw-амплитуде (~−60 дБ) — порт `NOISE_FLOOR`.
pub const NOISE_FLOOR: f64 = 0.001;
/// Байт на один кадр анализа (stereo interleaved s16) — порт `READ_CHUNK`.
pub const READ_CHUNK: usize = FFT_SIZE * CHANNELS * BYTES_PER_SAMPLE;

/// Нижняя граница pink-коррекции, Гц — порт `PINK_CORRECTION_FMIN`.
pub const PINK_CORRECTION_FMIN: f64 = 50.0;

// ── Конвейер v9 (калибровка 2026-05-21, r>0.95 к WE-Windows dump) ──────────

/// Показатель quartic-root-распределения границ — порт `V9_SPREAD_POWER`.
pub const V9_SPREAD_POWER: f64 = 0.197;
/// Экспонента динамической компрессии pow(v, 0.02) — порт `V9_EXPONENT`.
pub const V9_EXPONENT: f64 = 0.02;
/// Нижняя частота, Гц — порт `V9_FREQ_MIN`.
pub const V9_FREQ_MIN: f64 = 77.0;
/// Верхняя частота, Гц (WE округляет sr/2 вверх) — порт `V9_FREQ_MAX`.
pub const V9_FREQ_MAX: f64 = 22062.0;
/// Выходной множитель — порт `V9_GAIN`.
pub const V9_GAIN: f64 = 1.9;
/// Полос на канал в WE-формате — порт `V9_BANDS_PER_CHANNEL`.
pub const V9_BANDS_PER_CHANNEL: usize = 64;

// ── Константы whitening-этапа legacy-конвейера (из `_capture_loop`) ────────

/// Минимум long-term EMA (защита деления) — порт `MIN_PEAK_FLOOR`.
pub const MIN_PEAK_FLOOR: f64 = 0.001;
/// EMA attack (~0.7 с до новой нормы @30fps) — порт `EMA_ATTACK`.
pub const EMA_ATTACK: f64 = 0.05;
/// EMA release (~7 с до 1/e) — порт `EMA_RELEASE`.
pub const EMA_RELEASE: f64 = 0.005;
/// Gamma-компрессия выхода: v^0.5 (sqrt) — порт `GAMMA`.
pub const GAMMA: f64 = 0.5;
/// Минимальная длина «луча» при звуке — порт `CLAMP_MIN`.
pub const CLAMP_MIN: f64 = 0.25;
/// Делитель whitening: raw / (DIVISOR·avg) — порт `DIVISOR`.
pub const DIVISOR: f64 = 1.0;

// ============================================================================
// Источник захвата — порт SOURCE_* / VALID_SOURCES
// ============================================================================

/// Monitor default sink (системный аудиовыход) — порт `SOURCE_MONITOR`.
pub const SOURCE_MONITOR: &str = "monitor";
/// Default source (микрофон) — порт `SOURCE_MICROPHONE`.
pub const SOURCE_MICROPHONE: &str = "microphone";
/// Mono-микс monitor + microphone — порт `SOURCE_BOTH`.
pub const SOURCE_BOTH: &str = "both";
/// Допустимые значения source — порт `VALID_SOURCES`.
pub const VALID_SOURCES: &[&str] = &[SOURCE_MONITOR, SOURCE_MICROPHONE, SOURCE_BOTH];

/// Нормализация source: неизвестное значение → `monitor` — порт fallback в
/// `_AudioBackend.__init__` / `AudioCapture.__init__` (там же warning в лог).
pub fn normalize_source(source: &str) -> &'static str {
    match source {
        SOURCE_MICROPHONE => SOURCE_MICROPHONE,
        SOURCE_BOTH => SOURCE_BOTH,
        _ => SOURCE_MONITOR,
    }
}

/// Резолв source в CLI: CLI-аргумент > env `REXON_WP_AUDIO_SOURCE` >
/// default `monitor`; неизвестное значение → `monitor` — порт `main()`.
pub fn resolve_source(cli: Option<&str>, env: Option<&str>) -> &'static str {
    let raw = cli.or(env).unwrap_or(SOURCE_MONITOR);
    if VALID_SOURCES.contains(&raw) {
        normalize_source(raw)
    } else {
        SOURCE_MONITOR
    }
}

/// Порядок авто-детекта бэкендов — порт `AudioCapture._detect_backend`:
/// PipeWire → PulseAudio → ALSA. Доступность каждого — наблюдение исполнителя.
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
// argv команд захвата и проб — порт _spawn / available()
// ============================================================================

/// argv `pw-record` на заданный target — порт `PipeWireBackend._spawn`
/// (stdout — в пайп, `-`).
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

/// argv `parec` на заданный device — порт `PulseAudioBackend._spawn`.
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

/// argv `arecord` на заданный device — порт `ALSABackend._spawn`.
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

/// Проба «PipeWire daemon запущен» — порт `pw-cli info 0` из `available()`.
pub fn pipewire_probe_argv() -> Vec<String> {
    vec!["pw-cli".into(), "info".into(), "0".into()]
}

/// Проба «PulseAudio отвечает» — порт `pactl info` из `available()`.
pub fn pulseaudio_probe_argv() -> Vec<String> {
    vec!["pactl".into(), "info".into()]
}

/// `pactl get-default-sink` — запрос default sink.
pub fn pactl_default_sink_argv() -> Vec<String> {
    vec!["pactl".into(), "get-default-sink".into()]
}

/// `pactl get-default-source` — запрос default source.
pub fn pactl_default_source_argv() -> Vec<String> {
    vec!["pactl".into(), "get-default-source".into()]
}

/// `pactl list short sources` — список источников.
pub fn pactl_short_sources_argv() -> Vec<String> {
    vec!["pactl".into(), "list".into(), "short".into(), "sources".into()]
}

/// `arecord -l` — список ALSA capture-устройств (поиск loopback).
pub fn arecord_list_argv() -> Vec<String> {
    vec!["arecord".into(), "-l".into()]
}

/// `wpctl status -n` — снимок графа WirePlumber ИМЕНАМИ узлов.
///
/// ⚠️ Зачем отдельная ветка, а не подмена `pactl` → `wpctl`. На Ubuntu 24.04,
/// Kubuntu 24.04 и Debian 13 стоит чистый PipeWire, а `pactl` лежит в
/// `pulseaudio-utils`, который туда не ставится (на Arch/openSUSE он
/// подтягивается транзитивно через `pipewire-pulse`, поэтому дефект невидим
/// на dev-машине). `wpctl` же есть везде, где есть PipeWire, — он из
/// `wireplumber`, обязательной части стека. Но это НЕ синоним `pactl`:
/// другой набор команд и совсем другой вывод, поэтому и разбор отдельный.
/// Флаг `-n` обязателен: без него `wpctl` печатает ОПИСАНИЯ («Ryzen HD Audio
/// Controller Speaker»), а `pw-record --target` принимает имена узлов.
pub fn wpctl_status_argv() -> Vec<String> {
    vec!["wpctl".into(), "status".into(), "-n".into()]
}

// ============================================================================
// Разбор `wpctl status -n` (wireplumber) — замена pactl там, где его нет
// ============================================================================

/// Аудио-раздел вывода `wpctl status -n`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WpctlAudio {
    /// Имя узла sink по умолчанию (строка помечена `*`).
    pub default_sink: Option<String>,
    /// Все sink'и в порядке вывода.
    pub sinks: Vec<String>,
    /// Имя узла source по умолчанию (строка помечена `*`).
    pub default_source: Option<String>,
    /// Все source'ы в порядке вывода.
    pub sources: Vec<String>,
}

/// Убрать слева рамку дерева, которой `wpctl` рисует разделы.
fn strip_tree(line: &str) -> &str {
    line.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '│' | '├' | '└' | '┌' | '─' | '|')
    })
}

/// Разобрать строку записи вида `*   67. alsa_output.…  [vol: 0.78]`.
/// Возвращает (это_умолчание, имя_узла). Имя узла пробелов не содержит,
/// поэтому берём первый токен — так же отсекается и хвост `[vol: …]`.
fn parse_wpctl_entry(body: &str) -> Option<(bool, String)> {
    let (is_default, rest) = match body.strip_prefix('*') {
        Some(r) => (true, r.trim_start()),
        None => (false, body),
    };
    // `<id>. <имя>`
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

/// Разобрать `wpctl status -n`: sink'и и source'ы РАЗДЕЛА `Audio`.
///
/// Раздел `Video` пропускается целиком — там свои `Sources` (камеры), и
/// первая же из них помечена `*`; без учёта раздела веб-камера уехала бы
/// в аудио-захват. Внутри `Audio` берутся только подразделы `Sinks:` и
/// `Sources:` (в `Streams:`/`Filters:` строки той же формы).
pub fn parse_wpctl_status(stdout: &str) -> WpctlAudio {
    let mut out = WpctlAudio::default();
    let mut in_audio = false;
    let mut subsection = String::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // Заголовок верхнего уровня — строка без отступа и без рамки.
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

/// Target `pw-record` для системного выхода по выводу `wpctl status -n`.
///
/// `None` — из вывода ничего не извлеклось (нет `wpctl`, пустой граф): тогда
/// вызывающий пробует `pactl`, а за ним алиас `@DEFAULT_MONITOR@`.
///
/// Монитор-источников `wpctl` не показывает вовсе (в PipeWire это порты
/// sink'а, а не отдельный узел), поэтому проверить `<sink>.monitor` по списку,
/// как в `pactl`-ветке, нечем — имя строится от sink'а по умолчанию, а при его
/// отсутствии от первого sink'а. Результат по форме тот же, что у
/// `pactl`-ветки, так что дальше по коду разницы нет.
pub fn wpctl_monitor_target(status_stdout: Option<&str>) -> Option<String> {
    let st = parse_wpctl_status(status_stdout?);
    let sink = st
        .default_sink
        .or_else(|| st.sinks.into_iter().next())?;
    Some(format!("{sink}.monitor"))
}

/// Target `pw-record` для микрофона по выводу `wpctl status -n`:
/// source по умолчанию (если это не монитор) → первый не-монитор из списка.
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
// Выбор устройства по наблюдённому выводу pactl / arecord
// ============================================================================
//
// Вывод команд подаётся параметрами: `Some(stdout)` — команда отработала
// (rc == 0), `None` — упала/недоступна. Пустой после trim stdout оригинал
// трактует как отсутствие значения — здесь так же.

/// Имена всех source из `pactl list short sources` — колонка 2
/// (формат строки: `<id>\t<name>\t<driver>\t<spec>\t<state>`).
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

/// Только monitor-источники (имя содержит `.monitor`) — порт сборки
/// `monitor_sources` в `PipeWireBackend._find_monitor`.
pub fn monitor_sources(stdout: &str) -> Vec<String> {
    parse_short_sources(stdout)
        .into_iter()
        .filter(|name| name.contains(".monitor"))
        .collect()
}

/// Target pw-record для системного выхода — порт
/// `PipeWireBackend._find_monitor`. Стратегия: default sink + `.monitor`
/// (с верификацией по списку) → первый monitor из списка → alias
/// `@DEFAULT_MONITOR@` (не «0» — numeric id может оказаться микрофоном).
pub fn pipewire_monitor_target(
    default_sink_stdout: Option<&str>,
    sources_stdout: Option<&str>,
) -> String {
    let monitors = sources_stdout.map(monitor_sources).unwrap_or_default();

    // (1) Точный путь: default sink → его monitor. Если список sources пуст
    // (pactl упал) — доверяем кандидату; если непуст, но кандидата нет —
    // sink, возможно, suspended → fallback на первый доступный monitor.
    if let Some(sink) = nonempty_trimmed(default_sink_stdout) {
        let candidate = format!("{sink}.monitor");
        if monitors.is_empty() || monitors.contains(&candidate) {
            return candidate;
        }
    }

    // (2) Первый monitor из списка sources
    if let Some(first) = monitors.into_iter().next() {
        return first;
    }

    // (3) Last resort: alias PipeWire (pw-record >= 0.3.50)
    "@DEFAULT_MONITOR@".to_string()
}

/// Target pw-record для микрофона — порт `PipeWireBackend._find_microphone`.
/// Стратегия: default source, если это не `.monitor` → первый не-monitor из
/// списка → alias `@DEFAULT_SOURCE@`.
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

/// Device parec для системного выхода — порт `PulseAudioBackend._find_monitor`:
/// default sink + `.monitor` (без верификации, в отличие от PipeWire-ветки) →
/// первый source из СТРОКИ с `.monitor` → None.
pub fn pulse_monitor_device(
    default_sink_stdout: Option<&str>,
    sources_stdout: Option<&str>,
) -> Option<String> {
    if let Some(sink) = nonempty_trimmed(default_sink_stdout) {
        return Some(format!("{sink}.monitor"));
    }
    if let Some(stdout) = sources_stdout {
        for line in stdout.lines() {
            // Оригинал проверяет `.monitor` по ВСЕЙ строке, не по имени.
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

/// Device parec для микрофона — порт `PulseAudioBackend._find_microphone`.
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

/// ALSA loopback device из вывода `arecord -l` — порт
/// `ALSABackend._find_loopback`: строка с «loopback» (без регистра) →
/// `card N: …` → `hw:N,1`. Нечисловой номер карты — пропуск строки; строка
/// без токенов до `:` в оригинале роняла IndexError, гасившийся внешним
/// `except` → None (здесь то же наблюдаемое поведение).
pub fn alsa_loopback_device(arecord_l_stdout: Option<&str>) -> Option<String> {
    let lower = arecord_l_stdout?.to_lowercase();
    for line in lower.lines() {
        if !line.contains("loopback") {
            continue;
        }
        let card_part = line.split(':').next().unwrap_or("").trim();
        // Нет токенов → IndexError в оригинале → общий except → None
        let card_num = card_part.split_whitespace().last()?;
        if card_num.parse::<i64>().is_ok() {
            return Some(format!("hw:{card_num},1"));
        }
        // ValueError → pass (следующая строка)
    }
    None
}

/// Device arecord для выбранного source — порт `ALSABackend.start`:
/// microphone → `default`; monitor/both → loopback либо `default`
/// (в оригинале с предупреждением, что это микрофон без snd-aloop).
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
// PCM: микс кадров 'both' и стерео-разбор s16le
// ============================================================================

/// Кадр тишины (underrun) — порт `b"\x00" * frame_bytes`.
pub fn silence_frame(frame_bytes: usize) -> Vec<u8> {
    vec![0u8; frame_bytes]
}

/// Микс N кадров s16le сэмпл-в-сэмпл — порт хвоста `_read_frame_mixed`:
/// среднее floor-делением (Python `//`, сохранённый квирк) с клампом в i16.
/// Один кадр возвращается как есть; пустой список → пустой вектор.
/// Кадры должны быть одинаковой длины (чётной) — как в оригинале.
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
                // floor-деление как Python `//` (div_euclid при n > 0)
                let avg = acc.div_euclid(n_streams).clamp(-32768, 32767) as i16;
                mixed.extend_from_slice(&avg.to_le_bytes());
            }
            mixed
        }
    }
}

/// Разобрать interleaved-stereo s16le кадр на два канала [-1.0 .. 1.0) —
/// порт split-цикла в capture-циклах (`l / 32768.0`).
pub fn split_stereo_s16le(frame: &[u8]) -> (Vec<f64>, Vec<f64>) {
    let stride = CHANNELS * BYTES_PER_SAMPLE; // 4 байта на пару L+R
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
// FFT (Cooley-Tukey radix-2) и окна — порт _fft / _rfft_magnitudes / окон
// ============================================================================

/// БПФ (Cooley-Tukey radix-2, in-place по bit-reversal) — порт `_fft`.
/// Длина входа — степень двойки (инвариант вызывающих, как в оригинале).
/// Комплексные значения — пары (re, im).
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
                // (a+bi)(c+di) = (ac−bd) + (ad+bc)i — как complex-умножение CPython
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

/// FFT → амплитуды положительных частот (n/2 bins, без Nyquist),
/// нормировка делением на n/2 — порт `_rfft_magnitudes`.
pub fn rfft_magnitudes(samples: &[f64]) -> Vec<f64> {
    let spectrum = fft(samples);
    let n = spectrum.len() / 2;
    spectrum[..n]
        .iter()
        .map(|c| (c.0 * c.0 + c.1 * c.1).sqrt() / n as f64)
        .collect()
}

/// Окно Hamming: 0.54 − 0.46·cos(2πi/(n−1)) — порт `_hamming_window`
/// (используется legacy-конвейером; WE-параметр `hammingparam`).
pub fn hamming_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.54 - 0.46 * (2.0 * PI * i as f64 / (n - 1) as f64).cos())
        .collect()
}

/// Окно Hann: 0.5 − 0.5·cos(2πi/(n−1)) — порт `_hann_window_v9`
/// (v9-конвейер; НЕ Hamming — калибровка).
pub fn hann_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / (n - 1) as f64).cos())
        .collect()
}

// ============================================================================
// Полосы: линейные (legacy) и quartic-root + MAX (v9)
// ============================================================================

/// Разложить амплитуды в ЛИНЕЙНЫЕ полосы 50–22050 Гц (среднее по bins,
/// WE использует linear-распределение) — порт `_magnitudes_to_bands`.
/// Возвращает сырые средние амплитуды (нормализация — на вызывающем).
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

        let bin_lo = (f_lo / hz_per_bin) as usize; // int() положительного = trunc
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

/// Пары (bin_lo, bin_hi) включительно для quartic-root-распределения v9 —
/// порт `_v9_bin_edges`. Верхняя граница полосы i:
/// `f_upper(i) = ((i+1)/bands)^SPREAD_POWER · FREQ_MAX + FREQ_MIN`
/// (плотно на низах, редко на верхах — WE-spec, r>0.95 к дампу).
///
/// FIX: оригинал считал `hz_per_bin = SAMPLE_RATE / FFT_SIZE` от глобальной
/// константы, игнорируя параметр `n_fft_bins`; здесь — от `n_fft_bins * 2`
/// (для штатного `n_fft_bins = FFT_SIZE/2` значения совпадают).
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
        // Clamp ОБА конца: FREQ_MAX=22062 > Nyquist 22050 → bin вне диапазона
        let bin_lo = ((edges_hz[i] / hz_per_bin) as usize).min(n_fft_bins - 1);
        let mut bin_hi = ((edges_hz[i + 1] / hz_per_bin) as usize).min(n_fft_bins - 1);
        if bin_hi < bin_lo {
            bin_hi = bin_lo;
        }
        pairs.push((bin_lo, bin_hi));
    }
    pairs
}

/// MAX-редукция: пик амплитуды в каждом диапазоне (не среднее!) —
/// порт `_v9_reduce_max`.
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
        // hi < bin_lo → пустой диапазон (Python range) → пик 0.0
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

/// Таблица pink-коррекции √(f_center/f_min) на полосу (выравнивание баса и
/// treble под 1/f-спектр музыки) — порт `AudioCapture._build_pink_factors`.
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
// Срезы bass / mid / high — порт _band_slice / get_bass / get_mid / get_high
// ============================================================================

/// Среднее полос в диапазоне [start_ratio .. end_ratio) каждого канала —
/// порт `_band_slice`: при 128 полосах усредняются участки L (0..63) и
/// R (64..127); иначе — линейный срез по всем полосам.
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

/// Точная семантика builtin `sum()` CPython ≥ 3.12 для float —
/// компенсированное суммирование Ноймайера (float fast-path в
/// bltinmodule.c). Наивный fold давал расхождение с живым Python в
/// последних ULP на длинных срезах `_band_slice` — починка паритета при
/// подключении моста (в `magnitudes_to_bands` НЕ применяется: там эталон
/// суммирует вручную `total += …`, наивный fold и есть паритет).
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

/// Уровень баса (нижняя четверть) — порт `get_bass`.
pub fn bass_level(bands: &[f64]) -> f64 {
    band_slice(bands, 0.0, 0.25)
}

/// Уровень средних (вторая+третья четверти) — порт `get_mid`.
pub fn mid_level(bands: &[f64]) -> f64 {
    band_slice(bands, 0.25, 0.75)
}

/// Уровень высоких (верхняя четверть) — порт `get_high`.
pub fn high_level(bands: &[f64]) -> f64 {
    band_slice(bands, 0.75, 1.0)
}

// ============================================================================
// V9Analyzer — порт `_capture_loop_v9` (без чтения пайпа)
// ============================================================================

/// Stateful-конвейер v9 (production, калиброван к WE-Windows dump):
/// один вызов [`V9Analyzer::process_frame`] = одна итерация `_capture_loop_v9`
/// над готовым PCM-кадром длиной [`READ_CHUNK`].
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
    /// Аналайзер на `num_bands` полос (128 = WE-режим L+R, иначе mono-fallback
    /// из левого канала) со сглаживанием `smoothing` — порт полей
    /// `AudioCapture.__init__` + преамбулы `_capture_loop_v9`.
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

    /// Обработать один PCM-кадр (interleaved s16le stereo, [`READ_CHUNK`]
    /// байт): Hann → rfft → MAX-редукция → pow(0.02)·GAIN → clamp →
    /// noise gate → motion blur → свёртка [1,2,1]/4.
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

        // Динамическая компрессия pow(v, 0.02)·1.9 + clamp [0, 1]
        // (даже тихие bins прижимаются к 1.0 — спецификация WE)
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

        // Noise gate по RAW-пику до компрессии (pow(0.02) поднял бы шум к 0.87)
        let raw_peak = all_bands.iter().copied().fold(0.0f64, f64::max);
        if raw_peak < NOISE_FLOOR {
            processed = vec![0.0; n];
        }

        // Motion blur + свёртка [1,2,1]/4 (WE-spec)
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

    /// Сглаженные полосы [0.0 .. 1.0] — порт `get_bands`.
    pub fn bands(&self) -> &[f64] {
        &self.bands
    }

    /// Несглаженные полосы — порт `get_raw_bands`.
    pub fn raw_bands(&self) -> &[f64] {
        &self.raw_bands
    }

    /// Пиковый уровень — порт `get_peak`.
    pub fn peak(&self) -> f64 {
        self.peak
    }
}

// ============================================================================
// LegacyAnalyzer — порт `_capture_loop` (без чтения пайпа)
// ============================================================================

/// Stateful legacy-конвейер (pink → whitening EMA → gamma → clamp):
/// один вызов [`LegacyAnalyzer::process_frame`] = одна итерация
/// `_capture_loop` над готовым PCM-кадром.
pub struct LegacyAnalyzer {
    smoothing: f64,
    we_mode: bool,
    bands_per_channel: usize,
    pink_correction: bool,
    pink_factors: Vec<f64>,
    hamming: Vec<f64>,
    avg_band: Vec<f64>, // long-term EMA; пуст до первого кадра
    bands: Vec<f64>,
    raw_bands: Vec<f64>,
    peak: f64,
}

impl LegacyAnalyzer {
    /// Аналайзер на `num_bands` полос (128 = WE-режим) — порт
    /// `AudioCapture.__init__` (pink-факторы: 64/канал в WE-режиме).
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

    /// Обработать один PCM-кадр: Hamming → rfft → линейные полосы → pink →
    /// whitening (EMA attack/release) → gamma 0.5 → clamp [0.25, 1] →
    /// noise floor → motion blur → свёртка [1,2,1]/4 → post-clamp.
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

        // Pink-коррекция: статический pre-boost ВЧ ∝ √f до whitening
        // (в WE-режиме факторы длиной 64 применяются к L и R по модулю)
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

        // Long-term EMA каждого bin (инициализация на первом кадре)
        if self.avg_band.len() != n {
            self.avg_band = all_bands.iter().map(|&v| v.max(MIN_PEAK_FLOOR)).collect();
        }
        for (avg, &cur) in self.avg_band.iter_mut().zip(&all_bands) {
            let prev = *avg;
            // Выше avg → быстрый rise (attack), ниже → медленный fall (release)
            let new_avg = if cur > prev {
                prev * (1.0 - EMA_ATTACK) + cur * EMA_ATTACK
            } else {
                prev * (1.0 - EMA_RELEASE) + cur * EMA_RELEASE
            };
            *avg = new_avg.max(MIN_PEAK_FLOOR);
        }

        // Whitening raw/avg + gamma + clamp; весь спектр тихий → нули
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

        // Motion blur → свёртка [1,2,1]/4 → post-clamp (иначе blur затирает
        // минимум первые кадры)
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

    /// Сглаженные полосы [0.0 .. 1.0] — порт `get_bands`.
    pub fn bands(&self) -> &[f64] {
        &self.bands
    }

    /// Несглаженные полосы (после pink-коррекции) — порт `get_raw_bands`.
    pub fn raw_bands(&self) -> &[f64] {
        &self.raw_bands
    }

    /// Пиковый уровень — порт `get_peak`.
    pub fn peak(&self) -> f64 {
        self.peak
    }
}

/// Свёртка [1, 2, 1]/4 с репликацией краёв — общий шаг обоих конвейеров.
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
// JSON-stream IPC — порт stream-режима main()
// ============================================================================

/// Строка JSON-stream (`{"b":[...]}` компактно, как
/// `json.dumps(..., separators=(",", ":"))`) — порт stream-цикла `main()`.
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

/// `repr()` числа с плавающей точкой ПО ПРАВИЛАМ PYTHON — так же, как его
/// печатает `json.dumps` (он берёт `float.__repr__`).
///
/// Кратчайшее представление дают обе стороны, но пороги и оформление
/// РАЗНЫЕ, и в потоке `--stream` это видно: ryu отдаёт `0.00001` и
/// `1.5e-7`, Python — `1e-05` и `1.5e-07`. Тихие полосы дают ровно такие
/// значения, поэтому расхождение достижимое, а не теоретическое. Правила
/// Python: экспоненциальная запись при `exp < -4` либо `exp >= 16`, знак
/// экспоненты всегда, минимум две цифры; целые значения — с `.0`.
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

    // `{:e}` в Rust — кратчайшее представление в научной форме: `d[.ddd]e<exp>`.
    let sci = format!("{:e}", v.abs());
    let (mant, exp) = match sci.split_once('e') {
        Some(pair) => pair,
        None => return sci, // недостижимо для конечных чисел
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
        let ip = (exp + 1) as usize; // цифр в целой части
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
// Конвейер как одна сущность — порт селектора `pipeline` из `AudioCapture`
// ============================================================================

/// Конвейер по умолчанию (калиброван к WE-Windows dump) — порт `"v9"`.
pub const PIPELINE_V9: &str = "v9";
/// Запасной конвейер (whitening + gamma + clamp + pink) — порт `"legacy"`.
pub const PIPELINE_LEGACY: &str = "legacy";

/// Нормализация имени конвейера: всё, кроме `legacy`, — это `v9` (порт
/// fallback'а `AudioCapture.__init__`, где неизвестное значение с
/// предупреждением превращалось в `v9`).
pub fn normalize_pipeline(pipeline: &str) -> &'static str {
    if pipeline == PIPELINE_LEGACY {
        PIPELINE_LEGACY
    } else {
        PIPELINE_V9
    }
}

/// Оба конвейера за одним лицом — порт выбора `_capture_loop_v9` /
/// `_capture_loop` в `AudioCapture.start`.
pub enum Analyzer {
    /// Production-конвейер v9.
    V9(V9Analyzer),
    /// Запасной legacy-конвейер.
    Legacy(LegacyAnalyzer),
}

impl Analyzer {
    /// Анализатор по имени конвейера (неизвестное имя → `v9`).
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

    /// Обработать один PCM-кадр.
    pub fn process_frame(&mut self, frame: &[u8]) {
        match self {
            Analyzer::V9(a) => a.process_frame(frame),
            Analyzer::Legacy(a) => a.process_frame(frame),
        }
    }

    /// Сглаженные полосы.
    pub fn bands(&self) -> &[f64] {
        match self {
            Analyzer::V9(a) => a.bands(),
            Analyzer::Legacy(a) => a.bands(),
        }
    }

    /// Несглаженные полосы.
    pub fn raw_bands(&self) -> &[f64] {
        match self {
            Analyzer::V9(a) => a.raw_bands(),
            Analyzer::Legacy(a) => a.raw_bands(),
        }
    }

    /// Пиковый уровень.
    pub fn peak(&self) -> f64 {
        match self {
            Analyzer::V9(a) => a.peak(),
            Analyzer::Legacy(a) => a.peak(),
        }
    }
}

// ============================================================================
// Сборка кадра из потоков захвата — порт `_read_frame_mixed`
// ============================================================================

/// Потоки PCM, из которых собирается кадр. Само чтение — побочный эффект
/// исполнителя (пайп живого подпроцесса в бою, заготовленные байты в сверке),
/// поэтому оно за трейтом, а вся арифметика буферов — в [`FrameAssembler`].
pub trait PcmStreams {
    /// Сколько потоков.
    fn count(&self) -> usize;

    /// Прочитать до `max` байт из потока `i`.
    ///
    /// * `Some(данные)` — прочитано (пустой срез = поток кончился, EOF);
    /// * `None` — ошибка чтения (порт `except` в оригинале: поток мёртв).
    ///
    /// Читать НУЖНО до `max` байт либо до конца потока, как это делает
    /// `BufferedReader.read(n)` у Python: частичное чтение оригинал бы не
    /// вернул, и цикл получил бы лишние кадры тишины.
    fn read(&mut self, i: usize, max: usize) -> Option<Vec<u8>>;
}

/// Побайтовая сборка кадра анализа из одного или двух потоков захвата —
/// порт `AudioCapture._read_frame_mixed` без самого чтения.
///
/// Режим `both` даёт два потока: кадр собирается из тех, у кого уже накопился
/// полный кадр, и усредняется ([`mix_frames_s16le`]). Если полного кадра нет
/// ни у кого, но потоки живы, отдаётся кадр тишины — чтобы цикл не залипал на
/// медленном источнике (underrun).
pub struct FrameAssembler {
    frame_bytes: usize,
    buffers: Vec<Vec<u8>>,
}

impl FrameAssembler {
    /// Сборщик на `streams` потоков с кадром в `frame_bytes` байт.
    pub fn new(streams: usize, frame_bytes: usize) -> Self {
        FrameAssembler {
            frame_bytes,
            buffers: (0..streams).map(|_| Vec::new()).collect(),
        }
    }

    /// Размер кадра анализа в байтах.
    pub fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    /// Следующий кадр; `None` — все потоки кончились (порт `return None`).
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
                // Пустое чтение — поток кончился; живым он считается только
                // пока в буфере остаётся целый кадр (порт ветки `else`).
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
            // Underrun — кадр тишины, чтобы цикл не залип.
            return Some(silence_frame(self.frame_bytes));
        }

        let mixed = {
            let parts: Vec<&[u8]> = ready
                .iter()
                .map(|&i| &self.buffers[i][..self.frame_bytes])
                .collect();
            mix_frames_s16le(&parts)
        };
        // Буферы двигаются ТОЛЬКО после успешного микса (как в оригинале).
        for &i in &ready {
            self.buffers[i].drain(..self.frame_bytes);
        }
        Some(mixed)
    }
}

// ============================================================================
// Движок захвата — порт тела `_rs_capture_loop`
// ============================================================================

/// Снимок результатов кадра — порт полей `_bands` / `_raw_bands` / `_peak`,
/// которые оригинал копировал под `self._lock`.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Сглаженные полосы [0.0 .. 1.0].
    pub bands: Vec<f64>,
    /// Несглаженные полосы.
    pub raw_bands: Vec<f64>,
    /// Пиковый уровень.
    pub peak: f64,
}

impl Snapshot {
    /// Нулевой снимок на `num_bands` полос — порт сброса данных в `stop()`.
    pub fn zeroed(num_bands: usize) -> Self {
        Snapshot {
            bands: vec![0.0; num_bands],
            raw_bands: vec![0.0; num_bands],
            peak: 0.0,
        }
    }
}

/// Одна итерация цикла захвата целиком: собрать кадр из потоков и прогнать
/// его через конвейер — порт тела `while self._running` в `_rs_capture_loop`.
pub struct Engine {
    analyzer: Analyzer,
    assembler: FrameAssembler,
}

impl Engine {
    /// Движок на `streams` потоков с выбранным конвейером.
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

    /// Шаг цикла: `false` — поток захвата кончился, цикл пора завершать.
    pub fn step(&mut self, streams: &mut dyn PcmStreams) -> bool {
        match self.assembler.next_frame(streams) {
            Some(frame) => {
                self.analyzer.process_frame(&frame);
                true
            }
            None => false,
        }
    }

    /// Текущие данные кадра.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            bands: self.analyzer.bands().to_vec(),
            raw_bands: self.analyzer.raw_bands().to_vec(),
            peak: self.analyzer.peak(),
        }
    }
}

// ============================================================================
// План захвата: наблюдения среды за трейтом — порт `available()` /
// `_find_*` / `start_multi` (образец `system_plan.rs`, `importers.rs`)
// ============================================================================

/// Результат разовой команды-наблюдения — порт `subprocess.run(...,
/// capture_output=True, text=True)`.
#[derive(Clone, Debug, Default)]
pub struct ObsOut {
    /// `returncode == 0`.
    pub ok: bool,
    /// Текст stdout (`errors="replace"`).
    pub stdout: String,
}

/// Наблюдения за средой, нужные плану захвата. Реальные вызовы — у
/// исполнителя ([`LiveCaptureIo`] в бою, подставная таблица в сверке).
pub trait CaptureIo {
    /// `shutil.which(cmd) is not None`.
    fn has_cmd(&mut self, cmd: &str) -> bool;

    /// `subprocess.run(argv, capture_output=True, text=True, timeout=…)`;
    /// `None` — команда не запустилась либо не уложилась в таймаут (порт
    /// общего `except` оригинала).
    fn run(&mut self, argv: &[String], timeout: f64) -> Option<ObsOut>;
}

/// stdout команды при `returncode == 0`, иначе `None` — порт `_rs_run_stdout`.
fn run_stdout(io: &mut dyn CaptureIo, argv: &[String]) -> Option<String> {
    let r = io.run(argv, 3.0)?;
    if r.ok {
        Some(r.stdout)
    } else {
        None
    }
}

/// `returncode == 0` у пробы доступности бэкенда (таймаут 2 с, как в
/// `available()`).
fn probe_ok(io: &mut dyn CaptureIo, argv: &[String]) -> bool {
    io.run(argv, 2.0).map(|r| r.ok).unwrap_or(false)
}

/// Доступность PipeWire — порт `PipeWireBackend.available`.
pub fn pipewire_available(io: &mut dyn CaptureIo) -> bool {
    io.has_cmd("pw-record") && probe_ok(io, &pipewire_probe_argv())
}

/// Доступность PulseAudio — порт `PulseAudioBackend.available`.
pub fn pulseaudio_available(io: &mut dyn CaptureIo) -> bool {
    io.has_cmd("parec") && probe_ok(io, &pulseaudio_probe_argv())
}

/// Доступность ALSA — порт `ALSABackend.available` (`arecord` есть почти
/// всегда, отдельной пробы у оригинала нет).
pub fn alsa_available(io: &mut dyn CaptureIo) -> bool {
    io.has_cmd("arecord")
}

/// Живой авто-детект бэкенда — порт `AudioCapture._detect_backend` вместе с
/// пробами: PipeWire → PulseAudio → ALSA, `None` — звук захватить нечем.
pub fn detect_backend_live(io: &mut dyn CaptureIo) -> Option<&'static str> {
    detect_backend(
        pipewire_available(io),
        pulseaudio_available(io),
        alsa_available(io),
    )
}

/// Устройство захвата по снимку WirePlumber — порт `_wpctl_target`
/// (включая проверку наличия самого `wpctl`).
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

/// План запуска захвата: чем и с какими аргументами поднимать подпроцессы.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapturePlan {
    /// Имя бэкенда (`pipewire` / `pulseaudio` / `alsa`).
    pub backend: String,
    /// Нормализованный источник.
    pub source: String,
    /// argv подпроцессов: один для `monitor`/`microphone`, два для `both`.
    pub argvs: Vec<Vec<String>>,
    /// Строки для журнала (порт `log.info`/`log.warning` оригинала) —
    /// печатает их исполнитель, ядро только формулирует.
    pub notes: Vec<String>,
}

/// PipeWire: цель захвата для типа источника — порт
/// `PipeWireBackend._find_monitor` / `_find_microphone` (сначала снимок
/// WirePlumber, затем `pactl`; решение по наблюдениям — в ядре).
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

/// PulseAudio: устройство для типа источника — порт
/// `PulseAudioBackend._find_monitor` / `_find_microphone`. Ветки на `wpctl`
/// здесь намеренно нет: бэкенд включается только когда найден `parec`, а
/// `parec` и `pactl` приходят одним пакетом `pulseaudio-utils`.
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

/// План захвата для выбранного бэкенда и источника — порт `start_multi()`
/// всех трёх бэкендов (без самого `Popen`).
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
// Тексты консольного VU-метра — порт печати `main()`
// ============================================================================

/// Шапка VU-метра с уровнями диапазонов — порт строки `Bass/Mid/High/Peak`.
pub fn vu_levels_line(bass: f64, mid: f64, high: f64, peak: f64) -> String {
    format!("  Bass: {bass:.2}  Mid: {mid:.2}  High: {high:.2}  Peak: {peak:.2}")
}

/// Строки ASCII-баров по полосам — порт ветки `--bars`.
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

/// Компактная строка VU-метра — порт ветки без `--bars`.
pub fn vu_compact_line(bass: f64, mid: f64, high: f64, bands: &[f64]) -> String {
    let bar: Vec<String> = bands.iter().map(|b| format!("{b:.1}")).collect();
    format!(
        "  B={bass:.2} M={mid:.2} H={high:.2} | {}    ",
        bar.join(" ")
    )
}

// ============================================================================
// Боевой исполнитель: подпроцессы захвата и фоновый поток
// ============================================================================
//
// Здесь заканчивается детерминированная часть и начинается работа с системой.
// Отдельным блоком — ровно как боевой транспорт в `net.rs`: выше по файлу всё
// сверяется голденами без единого системного вызова, а ниже живут запуск
// `pw-record`/`parec`/`arecord`, чтение их пайпов и фоновый поток. Наружу
// торчит один тип [`CaptureRuntime`] с тем же поведением, что было у
// `AudioCapture` на Python.
//
// Почему по-прежнему внешняя команда, а не библиотека звукового сервера.
// Разобраны три пути: `cpal` 0.18 (Apache-2.0; хост PipeWire появился только
// в июне 2026 и до сих пор чинится, на Linux безусловно тянет `alsa-sys`,
// то есть заголовки ALSA на сборке и `libasound.so.2` в поставке), сырой
// `pipewire` 0.10 (MIT, но `links = pipewire-0.3` + bindgen: заголовки
// PipeWire и libclang на сборке, ручная сборка SPA-POD в коде) и внешний
// бинарь через пайп. Для коробки в Steam выбран третий: ноль зависимостей
// сборки, ноль новой ABI-поверхности в поставке, отдельный процесс (зависший
// захват не роняет приложение) и — главное — уже готовая цепочка отступления
// pw-record → parec → arecord, которая спасает на машинах без PipeWire.
// Стоимость — лишняя копия 176 КБ/с через пайп, что на фоне FFT незаметно.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Наблюдения за живой системой — боевая реализация [`CaptureIo`].
pub struct LiveCaptureIo;

/// `shutil.which`: исполняемый файл с таким именем в `PATH`.
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
        // Всё, что идёт здесь, — опросы, чей вывод разбирается (`wpctl
        // status`, `pactl`, `arecord -l`: alsa-utils переводит «card N: …»).
        // Программ для человека этот путь не запускает.
        let mut child = crate::utils::parsed_output_locale(&mut Command::new(bin))
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        // Читатели в своих потоках: иначе полный пайп подвесит наблюдение.
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
        // Таймаут — как `subprocess.TimeoutExpired` у оригинала: неуспех.
        let code = status?;
        Some(ObsOut {
            ok: code == 0,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
        })
    }
}

/// Дождаться завершения процесса не дольше `timeout` секунд; `None` —
/// не дождались (порт `subprocess.run(timeout=…)` / `proc.wait(timeout=…)`).
/// По таймауту процесс добивается, чтобы не оставить сироту.
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

/// `proc.terminate()` — мягкая остановка (SIGTERM), чтобы `pw-record` успел
/// отцепиться от графа PipeWire. `Child::kill()` из std шлёт SIGKILL и такой
/// возможности не даёт.
fn send_sigterm(child: &std::process::Child) {
    // SAFETY: kill(2) на собственного ребёнка, которого мы ещё не дождались,
    // — pid принадлежит нам и не переиспользован.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
}

/// Пайпы живых подпроцессов захвата как источник кадров.
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
        // Дочитывать до `max` либо до конца — так же, как `BufferedReader
        // .read(n)` у Python. Иначе частичное чтение выдавалось бы циклу за
        // underrun и подмешивало кадры тишины в живой звук.
        let mut buf = vec![0u8; max];
        let mut got = 0usize;
        while got < max {
            match pipe.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    // Ошибка чтения — поток мёртв (порт `except` оригинала),
                    // но уже прочитанное не теряем.
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

/// Общее состояние между фоновым потоком и вызывающим — порт `self._lock`
/// вокруг `_bands` / `_raw_bands` / `_peak` и флага `self._running`.
struct Shared {
    running: AtomicBool,
    data: Mutex<Snapshot>,
}

/// Захват системного аудио целиком: выбор бэкенда, подпроцессы, фоновый
/// поток и потокобезопасные данные — порт класса `AudioCapture`.
///
/// Поведение внешнего API повторяет оригинал: бэкенд определяется при
/// создании, [`start`](Self::start) поднимает один или два подпроцесса и
/// фоновый поток, [`stop`](Self::stop) гасит их мягко (SIGTERM, ожидание 2 с,
/// затем SIGKILL) и дожидается потока, обнуляя данные.
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
    /// Создать захват и сразу выбрать бэкенд — порт `AudioCapture.__init__`
    /// (авто-детект PipeWire → PulseAudio → ALSA).
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

    /// Имя активного бэкенда — порт свойства `backend_name`.
    pub fn backend_name(&self) -> &'static str {
        self.backend.unwrap_or("none")
    }

    /// Есть ли бэкенд — порт свойства `available`.
    pub fn available(&self) -> bool {
        self.backend.is_some()
    }

    /// Нормализованный источник.
    pub fn source(&self) -> &'static str {
        self.source
    }

    /// Выбранный конвейер.
    pub fn pipeline(&self) -> &'static str {
        self.pipeline
    }

    /// Забрать накопленные строки журнала (исполнитель их печатает).
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Идёт ли захват — порт `is_active()`.
    pub fn is_active(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst) && !self.children.is_empty()
    }

    /// Начать захват — порт `start()`: план, подпроцессы, фоновый поток.
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

    /// Остановить захват — порт `stop()`.
    ///
    /// Порядок важен: сначала снимается флаг, затем гасятся подпроцессы, и
    /// только потом ждётся поток. Смерть подпроцесса закрывает пайп, чтение
    /// возвращает EOF, и цикл выходит сам — поэтому ожидание потока не может
    /// зависнуть на блокирующем чтении.
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

    /// Текущий снимок данных — порт `get_bands`/`get_raw_bands`/`get_peak`
    /// под общим `self._lock`.
    pub fn snapshot(&self) -> Snapshot {
        self.shared
            .data
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
}

impl Drop for CaptureRuntime {
    /// Осиротевших `pw-record` после нас остаться не должно даже тогда, когда
    /// `stop()` забыли позвать.
    fn drop(&mut self) {
        if !self.children.is_empty() || self.thread.is_some() {
            self.stop();
        }
    }
}

/// Поднять один подпроцесс захвата: stdout — в пайп, stderr — в никуда
/// (порт `subprocess.Popen(cmd, stdout=PIPE, stderr=DEVNULL)`).
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
