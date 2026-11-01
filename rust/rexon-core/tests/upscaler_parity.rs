//! Golden-сверка порта `upscaler.rs` с оригиналом
//! `rexon/extensions/upscaler.py`.
//!
//! Эталон снят с ЖИВОГО Python read-only: одноразовый harness исполнял
//! настоящий `WallpaperUpscaler`, перехватывая subprocess.run/shutil.which/
//! os.path.*/os.remove/os.listdir/os.replace прокси над виртуальной ФС —
//! НИЧЕГО реально не выполнялось (апскейл/identify/скачивание не
//! запускались). Каждый вызов записан в журнал с аргументами и результатом
//! (`golden/upscaler_cases.json`).
//!
//! Здесь журнал реплеится: [`MockIo`] на каждый вызов Rust-порта сверяет op и
//! аргументы (argv/timeout байт-в-байт) с очередной записью эталона (порядок
//! обязан совпасть 1-в-1) и возвращает записанный результат. В конце — журнал
//! исчерпан, итог совпал. Плюс — чистые таблицы `suggest_scale`/`needs_upscale`.

use rexon_core::upscaler::{
    needs_upscale, process_wallpapers_in_dir, suggest_scale, upscale, RunResult,
    UpIo, MIN_DESKTOP_WIDTH,
};
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/upscaler_cases.json")).unwrap()
}

/// Декодер base64 (стандартный алфавит) для поля `stdout_b64`.
fn b64d(s: &str) -> Vec<u8> {
    const AL: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let idx = |c: u8| -> u32 { AL.iter().position(|&a| a == c).unwrap() as u32 };
    let raw: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(raw.len() * 3 / 4);
    for chunk in raw.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= idx(c) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// Реплей журнала: каждый вызов трейта проверяется против очередной записи.
struct MockIo {
    journal: Vec<Value>,
    pos: usize,
    scenario: String,
}

impl MockIo {
    fn new(journal: Vec<Value>, scenario: &str) -> Self {
        MockIo { journal, pos: 0, scenario: scenario.to_string() }
    }

    fn next(&mut self, op: &str) -> Value {
        assert!(
            self.pos < self.journal.len(),
            "[{}] журнал исчерпан, но вызван op={op}",
            self.scenario
        );
        let e = self.journal[self.pos].clone();
        assert_eq!(
            e["op"].as_str().unwrap(),
            op,
            "[{}] шаг {}: ожидался op={}, вызван {op}",
            self.scenario,
            self.pos,
            e["op"]
        );
        self.pos += 1;
        e
    }

    fn done(&self) {
        assert_eq!(
            self.pos,
            self.journal.len(),
            "[{}] журнал не исчерпан: {} из {} шагов",
            self.scenario,
            self.pos,
            self.journal.len()
        );
    }
}

impl UpIo for MockIo {
    fn run(&mut self, argv: Vec<String>, timeout: u64) -> RunResult {
        let e = self.next("run");
        let exp_argv: Vec<String> = e["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(argv, exp_argv, "[{}] argv на шаге {}", self.scenario, self.pos - 1);
        assert_eq!(
            timeout,
            e["timeout"].as_u64().unwrap(),
            "[{}] timeout на шаге {}",
            self.scenario,
            self.pos - 1
        );
        RunResult {
            ok: e["ok"].as_bool().unwrap(),
            stdout: b64d(e["stdout_b64"].as_str().unwrap()),
        }
    }

    fn which(&mut self, name: &str) -> Option<String> {
        let e = self.next("which");
        assert_eq!(name, e["name"].as_str().unwrap(), "[{}] which name", self.scenario);
        e["result"].as_str().map(str::to_string)
    }

    fn exists(&mut self, path: &str) -> bool {
        let e = self.next("exists");
        assert_eq!(path, e["path"].as_str().unwrap(), "[{}] exists path", self.scenario);
        e["result"].as_bool().unwrap()
    }

    fn is_file(&mut self, path: &str) -> bool {
        let e = self.next("is_file");
        assert_eq!(path, e["path"].as_str().unwrap(), "[{}] is_file path", self.scenario);
        e["result"].as_bool().unwrap()
    }

    fn access_x(&mut self, path: &str) -> bool {
        let e = self.next("access_x");
        assert_eq!(path, e["path"].as_str().unwrap(), "[{}] access_x path", self.scenario);
        e["result"].as_bool().unwrap()
    }

    fn remove_file(&mut self, path: &str) {
        let e = self.next("remove_file");
        assert_eq!(path, e["path"].as_str().unwrap(), "[{}] remove_file path", self.scenario);
    }

    fn download_realesrgan(&mut self) -> Option<String> {
        let e = self.next("download_realesrgan");
        e["result"].as_str().map(str::to_string)
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let e = self.next("listdir");
        assert_eq!(path, e["path"].as_str().unwrap(), "[{}] listdir path", self.scenario);
        e["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn replace(&mut self, src: &str, dst: &str) {
        let e = self.next("replace");
        assert_eq!(src, e["src"].as_str().unwrap(), "[{}] replace src", self.scenario);
        assert_eq!(dst, e["dst"].as_str().unwrap(), "[{}] replace dst", self.scenario);
    }
}

#[test]
fn suggest_scale_table() {
    let d = fx();
    for row in d["suggest_scale"].as_array().unwrap() {
        let r = row.as_array().unwrap();
        let (w, h, exp) = (r[0].as_i64().unwrap(), r[1].as_i64().unwrap(), r[2].as_i64().unwrap());
        assert_eq!(suggest_scale(w, h), exp, "suggest_scale({w},{h})");
    }
}

#[test]
fn min_desktop_width_matches() {
    let d = fx();
    assert_eq!(MIN_DESKTOP_WIDTH, d["min_desktop_width"].as_i64().unwrap());
}

#[test]
fn needs_upscale_size_table() {
    // needs_upscale через identify: для чистой таблицы моделируем identify,
    // возвращающий заданный размер, и сверяем булев результат.
    let d = fx();
    for row in d["needs_upscale_size"].as_array().unwrap() {
        let r = row.as_array().unwrap();
        let (w, h, exp) = (r[0].as_i64().unwrap(), r[1].as_i64().unwrap(), r[2].as_bool().unwrap());
        let stdout = format!("{w} {h}").into_bytes();
        let journal = vec![serde_json::json!({
            "op": "run",
            "argv": ["identify", "-format", "%w %h", "/img"],
            "timeout": 10,
            "ok": true,
            "stdout_b64": b64e(&stdout),
        })];
        let mut io = MockIo::new(journal, &format!("needs_upscale({w},{h})"));
        assert_eq!(needs_upscale(&mut io, "/img"), exp, "needs_upscale({w},{h})");
        io.done();
    }
}

/// base64-энкодер для локальной сборки stdout в таблице needs_upscale.
fn b64e(data: &[u8]) -> String {
    const AL: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(AL[(n >> 18) as usize & 63] as char);
        out.push(AL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { AL[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { AL[n as usize & 63] as char } else { '=' });
    }
    out
}

#[test]
fn scenarios_replay() {
    let d = fx();
    let local_exe = d["local_exe"].as_str().unwrap();
    let scenarios = d["scenarios"].as_array().unwrap();
    assert!(scenarios.len() >= 18, "ожидалось ≥18 сценариев");
    for s in scenarios {
        let name = s["name"].as_str().unwrap();
        let call = &s["call"];
        let journal: Vec<Value> = s["journal"].as_array().unwrap().clone();
        let mut io = MockIo::new(journal, name);
        let got: Value = match call["fn"].as_str().unwrap() {
            "upscale" => {
                let dest = call["dest"].as_str();
                let res = upscale(
                    &mut io,
                    call["src"].as_str().unwrap(),
                    dest,
                    call["scale"].as_i64().unwrap(),
                    call["method"].as_str().unwrap(),
                    local_exe,
                );
                Value::String(res)
            }
            "needs_upscale" => {
                Value::Bool(needs_upscale(&mut io, call["path"].as_str().unwrap()))
            }
            "process_dir" => Value::from(process_wallpapers_in_dir(
                &mut io,
                call["data_dir"].as_str().unwrap(),
                local_exe,
            )),
            other => panic!("неизвестная fn {other}"),
        };
        assert_eq!(got, s["result"], "[{name}] итоговый результат");
        io.done();
    }
}

/// Исполнитель, умеющий мерить размер сам (в живом мосте это Pillow из
/// `WallpaperUpscaler.get_image_size`): ядро обязано взять его ответ и НЕ
/// запускать `identify` вовсе.
///
/// Это и есть закрытая при аудите дыра: без хука `image_size` порт на машине
/// без ImageMagick (SteamOS) всегда получал (0, 0) и апскейл молча не делался,
/// тогда как Python-фолбэк работал через Pillow.
struct SizeIo {
    size: Option<(i64, i64)>,
    runs: usize,
}

impl UpIo for SizeIo {
    fn run(&mut self, _argv: Vec<String>, _timeout: u64) -> RunResult {
        self.runs += 1;
        RunResult { ok: false, stdout: Vec::new() }
    }
    fn which(&mut self, _name: &str) -> Option<String> {
        None
    }
    fn exists(&mut self, _path: &str) -> bool {
        false
    }
    fn is_file(&mut self, _path: &str) -> bool {
        false
    }
    fn access_x(&mut self, _path: &str) -> bool {
        false
    }
    fn remove_file(&mut self, _path: &str) {}
    fn download_realesrgan(&mut self) -> Option<String> {
        None
    }
    fn listdir(&mut self, _path: &str) -> Vec<String> {
        Vec::new()
    }
    fn replace(&mut self, _src: &str, _dst: &str) {}
    fn image_size(&mut self, _path: &str) -> Option<(i64, i64)> {
        self.size
    }
}

#[test]
fn image_size_hook_replaces_identify() {
    // 640×480 — меньше 1024, апскейл нужен; identify не звался ни разу.
    let mut io = SizeIo { size: Some((640, 480)), runs: 0 };
    assert!(needs_upscale(&mut io, "/img.png"));
    assert_eq!(io.runs, 0, "identify не должен запускаться при своём размере");

    // 1920×1080 — уже достаточно.
    let mut io = SizeIo { size: Some((1920, 1080)), runs: 0 };
    assert!(!needs_upscale(&mut io, "/img.png"));
    assert_eq!(io.runs, 0);

    // (0, 0) от исполнителя — ответ окончательный (Python уже попробовал и
    // Pillow, и identify), повторного запуска нет.
    let mut io = SizeIo { size: Some((0, 0)), runs: 0 };
    assert!(!needs_upscale(&mut io, "/img.png"));
    assert_eq!(io.runs, 0);

    // Исполнитель не умеет — запасной путь через identify (одна попытка).
    let mut io = SizeIo { size: None, runs: 0 };
    assert!(!needs_upscale(&mut io, "/img.png"));
    assert_eq!(io.runs, 1, "без хука ядро обязано звать identify");
}
