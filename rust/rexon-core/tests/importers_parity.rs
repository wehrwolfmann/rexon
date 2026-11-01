//! Golden-сверка порта `importers.rs` с оригиналом
//! `rexon/extensions/importers.py`.
//!
//! Эталоны сняты с ЖИВОГО Python по схеме «виртуальная ФС + журнал»:
//! одноразовый harness исполнял настоящие импортёры, перехватывая все
//! обращения к os/shutil/subprocess/tempfile/zipfile/open/_write_manifest/
//! _pack_rxn/_safe_zip_extract/ET.parse прокси над виртуальной ФС и
//! записывая КАЖДЫЙ вызов с аргументами и результатом в журнал
//! (`golden/importers_cases.json`). subprocess моделировался правилами
//! кейса (rc/stdout + эффект на ФС), tempfile.mkdtemp — детерминированным
//! счётчиком.
//!
//! Здесь журнал реплеится: [`MockIo`] на каждый вызов Rust-порта сверяет
//! op и аргументы с очередной записью эталона (порядок обязан совпасть
//! 1-в-1, тексты write_file — байт-в-байт) и возвращает записанный
//! результат. В конце — журнал обязан быть исчерпан, а результат/ошибка
//! (тип Python-исключения + текст) совпасть.
//!
//! Кейсы, задетые починкой шести граблей оригинала (санкционированные
//! расхождения — см. заголовок importers.rs), переписаны вручную под
//! ПОЧИНЕННОЕ поведение по тексту Python-хвостов; незадетые кейсы —
//! байт-в-байт с живым Python.

mod i18n_pin;

use rexon_core::importers::{
    cursor_can_import, dreamscene_import_to_rxn,
    gadget_import_to_rxn, icon_can_import, icon_import_to_rxn, ImpError,
    ImpIo, msstyles_can_import, msstyles_import_to_rxn, RunResult,
    theme_file_import_to_rxn, themepack_can_import, themepack_import_to_rxn,
    WalkEntry, windows_theme_can_import, windows_theme_import_to_rxn,
};
use serde_json::{json, Value};

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/importers_cases.json")).unwrap()
}

/// Декодер base64 (стандартный алфавит) для полей `*_b64` фикстуры.
fn b64d(s: &str) -> Vec<u8> {
    const AL: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let idx = |c: u8| -> u32 {
        AL.iter().position(|&a| a == c).expect("символ base64") as u32
    };
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

/// Реплей журнала эталона: каждый вызов трейта сверяется с очередной
/// записью; несовпадение порядка/аргументов — провал паритета.
struct MockIo {
    case: String,
    journal: Vec<Value>,
    pos: usize,
}

impl MockIo {
    fn new(case: &str, journal: &Value) -> Self {
        MockIo {
            case: case.to_string(),
            journal: journal.as_array().expect("journal — массив").clone(),
            pos: 0,
        }
    }

    fn next(&mut self, op: &str) -> Value {
        assert!(
            self.pos < self.journal.len(),
            "{}: журнал исчерпан на шаге {}, Rust просит op={op}",
            self.case,
            self.pos
        );
        let e = self.journal[self.pos].clone();
        assert_eq!(
            e["op"].as_str().unwrap_or(""),
            op,
            "{}: шаг {}: Rust просит {op}, эталон {}",
            self.case,
            self.pos,
            e["op"]
        );
        self.pos += 1;
        e
    }

    fn check(&self, e: &Value, field: &str, want: &Value) {
        assert_eq!(
            &e[field], want,
            "{}: шаг {}: поле {field} не совпало",
            self.case,
            self.pos - 1
        );
    }

    /// Чтение файла: следующая запись — либо сам op, либо `open_fail`
    /// (OSError на open → у порта None).
    fn next_read(&mut self, op: &str, path: &str, n: Option<usize>) -> Option<Value> {
        assert!(
            self.pos < self.journal.len(),
            "{}: журнал исчерпан на шаге {}, Rust просит op={op}",
            self.case,
            self.pos
        );
        if self.journal[self.pos]["op"] == "open_fail" {
            let e = self.next("open_fail");
            self.check(&e, "path", &json!(path));
            return None;
        }
        let e = self.next(op);
        self.check(&e, "path", &json!(path));
        if let Some(n) = n {
            self.check(&e, "n", &json!(n));
        }
        Some(e)
    }
}

impl ImpIo for MockIo {
    fn mkdtemp(&mut self, prefix: &str) -> String {
        let e = self.next("mkdtemp");
        self.check(&e, "prefix", &json!(prefix));
        e["result"].as_str().unwrap().to_string()
    }

    fn makedirs(&mut self, path: &str) {
        let e = self.next("makedirs");
        self.check(&e, "path", &json!(path));
    }

    fn run(&mut self, argv: Vec<String>, timeout: u64) -> RunResult {
        let e = self.next("run");
        self.check(&e, "argv", &json!(argv));
        self.check(&e, "timeout", &json!(timeout));
        RunResult {
            ok: e["rc"].as_i64() == Some(0),
            stdout: b64d(e["stdout_b64"].as_str().unwrap_or("")),
        }
    }

    fn walk(&mut self, root: &str) -> Vec<WalkEntry> {
        let e = self.next("walk");
        self.check(&e, "root", &json!(root));
        e["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let strs = |v: &Value| -> Vec<String> {
                    v.as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s.as_str().unwrap().to_string())
                        .collect()
                };
                (
                    t[0].as_str().unwrap().to_string(),
                    strs(&t[1]),
                    strs(&t[2]),
                )
            })
            .collect()
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let e = self.next("listdir");
        self.check(&e, "path", &json!(path));
        e["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    }

    fn is_file(&mut self, path: &str) -> bool {
        let e = self.next("isfile");
        self.check(&e, "path", &json!(path));
        e["result"].as_bool().unwrap()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        let e = self.next("isdir");
        self.check(&e, "path", &json!(path));
        e["result"].as_bool().unwrap()
    }

    fn exists(&mut self, path: &str) -> bool {
        let e = self.next("exists");
        self.check(&e, "path", &json!(path));
        e["result"].as_bool().unwrap()
    }

    fn getsize(&mut self, path: &str) -> u64 {
        let e = self.next("getsize");
        self.check(&e, "path", &json!(path));
        e["result"].as_u64().unwrap()
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        self.next_read("read_text", path, None)
            .map(|e| e["result"].as_str().unwrap().to_string())
    }

    fn read_head_text(&mut self, path: &str, n: usize) -> Option<String> {
        self.next_read("read_head_text", path, Some(n))
            .map(|e| e["result"].as_str().unwrap().to_string())
    }

    fn read_head_bytes(&mut self, path: &str, n: usize) -> Option<Vec<u8>> {
        self.next_read("read_head_bytes", path, Some(n))
            .map(|e| b64d(e["result_b64"].as_str().unwrap()))
    }

    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        self.next_read("read_bytes", path, None)
            .map(|e| b64d(e["result_b64"].as_str().unwrap()))
    }

    fn is_zipfile(&mut self, path: &str) -> Option<bool> {
        let e = self.next("is_zipfile");
        self.check(&e, "path", &json!(path));
        e["result"].as_bool()
    }

    fn zip_open(&mut self, path: &str) -> bool {
        let e = self.next("zip_open");
        self.check(&e, "path", &json!(path));
        e["ok"].as_bool().unwrap()
    }

    fn which(&mut self, name: &str) -> bool {
        let e = self.next("which");
        self.check(&e, "name", &json!(name));
        e["result"].as_bool().unwrap()
    }

    fn copy_file(&mut self, src: &str, dest: &str) {
        let e = self.next("copy2");
        self.check(&e, "src", &json!(src));
        self.check(&e, "dest", &json!(dest));
    }

    fn copy_tree(&mut self, src: &str, dest: &str) {
        let e = self.next("copytree");
        self.check(&e, "src", &json!(src));
        self.check(&e, "dest", &json!(dest));
    }

    fn symlink(&mut self, target: &str, link: &str) -> bool {
        let e = self.next("symlink");
        self.check(&e, "target", &json!(target));
        self.check(&e, "link", &json!(link));
        e["ok"].as_bool().unwrap_or(true)
    }

    fn remove_file(&mut self, path: &str) {
        let e = self.next("remove");
        self.check(&e, "path", &json!(path));
    }

    fn remove_tree(&mut self, path: &str) {
        let e = self.next("rmtree");
        self.check(&e, "path", &json!(path));
    }

    fn write_file(&mut self, path: &str, content: &str) {
        let e = self.next("write_file");
        self.check(&e, "path", &json!(path));
        self.check(&e, "content", &json!(content));
    }

    fn extract_zip(&mut self, archive: &str, dest: &str) {
        let e = self.next("extract_zip");
        self.check(&e, "archive", &json!(archive));
        self.check(&e, "dest", &json!(dest));
    }

    fn pack_rxn(&mut self, src: &str, dest: &str) {
        let e = self.next("pack_rxn");
        self.check(&e, "src", &json!(src));
        self.check(&e, "dest", &json!(dest));
    }

    // Перевод курсоров с 03.10.2026 — свой код на Rust, у эталона Python
    // таких шагов нет: в реплее они не встречаются (импорт курсоров сверяется
    // в `importers::cursor_tests` на настоящей ФС).
    fn write_bytes(&mut self, path: &str, _data: &[u8]) {
        panic!("{}: write_bytes({path}) — в эталоне Python такого шага нет", self.case);
    }

    fn zip_names(&mut self, path: &str) -> Option<Vec<String>> {
        panic!("{}: zip_names({path}) — в эталоне Python такого шага нет", self.case);
    }

    fn is_symlink(&mut self, path: &str) -> bool {
        panic!("{}: is_symlink({path}) — в эталоне Python такого шага нет", self.case);
    }

    fn upscale_wallpapers(&mut self, data_dir: &str) -> u64 {
        let e = self.next("upscale_wallpapers");
        self.check(&e, "path", &json!(data_dir));
        e["result"].as_u64().unwrap_or(0)
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn importers_match_python() {
    // Эталон Python — русские тексты манифестов и ошибок.
    let _lang = i18n_pin::ru();
    let fx = fx();
    for (name, case) in fx.as_object().unwrap() {
        let mut io = MockIo::new(name, &case["journal"]);
        let args = &case["args"];
        let path = args["path"].as_str().unwrap();
        let out_dir = args.get("output_dir").and_then(Value::as_str);
        let theme_name = args
            .get("theme_name")
            .and_then(Value::as_str)
            .unwrap_or("");

        let got: Result<Value, ImpError> = match case["fn"].as_str().unwrap() {
            "themepack_can_import" => Ok(json!(themepack_can_import(&mut io, path))),
            "wt_can_import" => Ok(json!(windows_theme_can_import(&mut io, path))),
            "cursor_can_import" => Ok(json!(cursor_can_import(&mut io, path))),
            "msstyles_can_import" => Ok(json!(msstyles_can_import(&mut io, path))),
            "icon_can_import" => Ok(json!(icon_can_import(&mut io, path))),
            "themepack_import" => {
                themepack_import_to_rxn(&mut io, path, out_dir).map(|p| json!(p))
            }
            "wt_import" => {
                windows_theme_import_to_rxn(&mut io, path, out_dir).map(|p| json!(p))
            }
            "theme_file_import" => {
                theme_file_import_to_rxn(&mut io, path, out_dir).map(|p| json!(p))
            }
            "dreamscene_import" => {
                dreamscene_import_to_rxn(&mut io, path, out_dir).map(|p| json!(p))
            }
            "gadget_import" => {
                gadget_import_to_rxn(&mut io, path, out_dir).map(|p| json!(p))
            }
            "icon_import" => icon_import_to_rxn(&mut io, path, theme_name, out_dir)
                .map(|p| json!(p)),
            "msstyles_import" => {
                msstyles_import_to_rxn(&mut io, path, out_dir).map(|p| json!(p))
            }
            other => panic!("{name}: неизвестная точка входа {other}"),
        };

        let got_json = match got {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "error": { "kind": e.kind, "message": e.message } }),
        };
        assert_eq!(got_json, case["result"], "{name}: результат/ошибка");
        assert_eq!(
            io.pos,
            io.journal.len(),
            "{name}: журнал не исчерпан ({} из {})",
            io.pos,
            io.journal.len()
        );
    }
}

// ============================================================================
// Язык интерфейса
// ============================================================================

use rexon_core::importers::{
    friendly_error_text, import_dialog_filter, packages_phrase, require_tools,
};
use rexon_core::utils::tool_package_name;

fn pkg(bin: &str) -> String {
    let p = tool_package_name(bin, "arch");
    if p.is_empty() { bin.to_string() } else { p }
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Отказы, предупреждения и фильтр окна выбора — по-русски дословно как до
/// перевода на каталог (сборка фраз `ca42e74^`).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_texts_as_before() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        require_tools("themepack", "arch", &|_| false).unwrap_err(),
        format!(
            "для распаковки темы нет нужной программы. Установите пакет «{}» либо пакет «{}».",
            pkg("cabextract"),
            pkg("7z")
        )
    );
    assert_eq!(
        packages_phrase(&[&["win2xcur"], &["convert", "xcursorgen"]], "arch"),
        format!(
            "пакет «{}» либо пакеты «{}» и «{}»",
            pkg("win2xcur"),
            pkg("convert"),
            pkg("xcursorgen")
        )
    );
    assert_eq!(
        require_tools("themepack", "arch", &|t| t == "cabextract").unwrap(),
        vec![format!(
            "Нет программ для подготовки картинок и обложки — импорт пойдёт упрощённо (пакет «{}»)",
            pkg("convert")
        )]
    );
    let filter = import_dialog_filter();
    assert!(filter.starts_with("Все темы Windows (*.themepack "), "{filter}");
    for group in [
        "Темы и стили (",
        "Иконки (",
        "Курсоры (",
        "Видеообои DreamScene (",
        "Гаджеты рабочего стола (*.gadget)",
    ] {
        assert!(filter.contains(group), "{group}: {filter}");
    }
    assert!(filter.ends_with(";;Все файлы (*)"), "{filter}");
    // Арчевая подсказка в тексте ошибки заменяется пакетом этой системы.
    let fixed = friendly_error_text(
        "Не удалось распаковать .themepack. Установите: sudo pacman -S cabextract",
        "debian",
    )
    .unwrap();
    assert_eq!(
        fixed,
        format!(
            "Не удалось распаковать .themepack. Установите пакет «{}»",
            tool_package_name("cabextract", "debian")
        )
    );
    let mut io = MockIo::new("ru_unsupported", &json!([]));
    let e = windows_theme_import_to_rxn(&mut io, "/x/file.xyz", None).unwrap_err();
    assert_eq!(e.message, "Неподдерживаемый формат: .xyz");
}

#[test]
fn english_importers_have_no_russian() {
    let _lang = i18n_pin::en();
    let refusal = require_tools("themepack", "arch", &|_| false).unwrap_err();
    assert_eq!(
        refusal,
        format!(
            "there's no program for unpacking the theme. Install the “{}” package or the “{}” package.",
            pkg("cabextract"),
            pkg("7z")
        )
    );
    assert_eq!(
        packages_phrase(&[&["win2xcur"], &["convert", "xcursorgen"]], "arch"),
        format!(
            "the “{}” package or the “{}” and “{}” packages",
            pkg("win2xcur"),
            pkg("convert"),
            pkg("xcursorgen")
        )
    );
    for kind in ["themepack", "theme", "dreamscene", "cursor", "icon", "icon_pe", "msstyles"] {
        match require_tools(kind, "arch", &|_| false) {
            Ok(w) => w.iter().for_each(|t| assert!(!has_cyrillic(t), "{kind}: {t}")),
            Err(t) => assert!(!has_cyrillic(&t), "{kind}: {t}"),
        }
    }
    let filter = import_dialog_filter();
    assert!(!has_cyrillic(&filter), "{filter}");
    assert!(filter.starts_with("All Windows Themes (*.themepack "), "{filter}");
    assert!(filter.contains(";;Desktop Gadgets (*.gadget);;"), "{filter}");
    assert!(filter.ends_with(";;All files (*)"), "{filter}");
    // Подсказка в английской ошибке узнаётся и заменяется так же.
    let fixed = friendly_error_text(
        "Couldn't unpack the .themepack. Install it: sudo pacman -S cabextract",
        "debian",
    )
    .unwrap();
    assert!(!has_cyrillic(&fixed), "{fixed}");
    assert!(!fixed.contains("pacman"), "{fixed}");
    let mut io = MockIo::new("en_unsupported", &json!([]));
    let e = windows_theme_import_to_rxn(&mut io, "/x/file.xyz", None).unwrap_err();
    assert_eq!(e.message, "Unsupported format: .xyz");
}
