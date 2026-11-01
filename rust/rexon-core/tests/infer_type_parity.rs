//! Golden-сверка порта определения типа обои по содержимому
//! (`user_props::infer_type`) с оригиналом
//! `rexon/wallpapers/factory.py::_infer_type`.
//!
//! Эталон снят прогоном ЖИВОГО `_infer_type` на настоящих временных
//! каталогах: под каждый случай раскладывались файлы, собирался `Project` с
//! нужным входным файлом, и записывался возвращённый тип.
//!
//! `golden/infer_type_cases.json` — снятые значения: для каждого случая
//! состав каталога (имя файла → содержимое, `null` — файл не читается в
//! UTF-8), входной файл и ожидаемый тип.

use rexon_core::user_props as up;
use serde_json::Value;
use std::collections::BTreeMap;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/infer_type_cases.json")).unwrap()
}

/// Каталог обои из голдена. Содержимое `None` означает файл, который есть,
/// но не читается как UTF-8.
struct DirIo {
    files: BTreeMap<String, Option<String>>,
}

impl DirIo {
    fn from(case: &Value) -> Self {
        let files = case["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().map(str::to_string)))
            .collect();
        Self { files }
    }
}

impl up::ProjectDirIo for DirIo {
    fn is_file(&mut self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.files.contains_key(name)
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.files.get(name).cloned().flatten()
    }
}

fn project_with_entry(entry: &str) -> up::Project {
    let raw = serde_json::json!({ "file": entry });
    up::parse_project(&raw).unwrap().0
}

/// Прогнать один случай голдена и вернуть, что получилось.
fn infer(case: &Value) -> String {
    let project = project_with_entry(case["entry"].as_str().unwrap());
    let mut io = DirIo::from(case);
    up::infer_type("/обои", &project, &mut io).to_string()
}

/// Все случаи, кроме тех, где порт намеренно расходится с оригиналом.
#[test]
fn all_cases_match_original() {
    let g = fx();
    // Расхождения по правилу #16 — разбираются отдельными проверками ниже.
    let fixed = ["web_htm_entry", "web_htm_only"];

    let mut checked = 0;
    for (name, case) in g.as_object().unwrap() {
        if name.starts_with('_') || fixed.contains(&name.as_str()) {
            continue;
        }
        assert_eq!(
            infer(case),
            case["want"].as_str().unwrap(),
            "случай {name}: состав {:?}, вход {:?}",
            case["files"],
            case["entry"]
        );
        checked += 1;
    }
    assert!(checked >= 25, "случаев в голдене стало меньше: {checked}");
}

/// Набор видео-расширений — тот же, что в оригинале, ПЛЮС `.ogv`.
///
/// ⚠️ ПОЧИНЕНО (правило #16). Списков было два: этот и свой у видео-хоста на
/// Python, и они разъехались — `.ogv` знал только хост. Из-за этого обои с
/// входным `.ogv` и без поля `type` в `project.json` определялись как «не
/// подошло ничего» и молча не применялись, хотя сам хост их играет.
/// Голден хранит исходный набор оригинала, порт добавляет к нему `.ogv`.
#[test]
fn video_extensions_match_original() {
    let g = fx();
    let mut want: Vec<&str> = g["_video_exts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(!want.contains(&".ogv"), "голден оригинала уже знает .ogv");
    // Порядок значим только для чтения; `.ogv` встал перед `.gif`, как он
    // стоял в списке видео-хоста.
    want.insert(want.len() - 1, ".ogv");
    // Ролики Windows DreamScene хост играет сам — встали там же, перед `.gif`.
    for ext in [".wmv", ".mpg", ".mpeg"] {
        assert!(!want.contains(&ext), "голден оригинала уже знает {ext}");
        want.insert(want.len() - 1, ext);
    }
    assert_eq!(up::VIDEO_ENTRY_EXTS.to_vec(), want);
}

/// Вход `.ogv` теперь опознаётся как видео (см. починку выше).
#[test]
fn ogv_entry_is_now_recognised() {
    let project = up::Project {
        file: "clip.OGV".to_string(),
        ..Default::default()
    };
    struct NoFiles;
    impl up::ProjectDirIo for NoFiles {
        fn is_file(&mut self, _path: &str) -> bool {
            false
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
    }
    assert_eq!(up::infer_type("/w", &project, &mut NoFiles), "video");
}

/// ⚠️ ПОЧИНЕНО (правило #16). Вход `.htm` оригинал не опознавал никак —
/// возвращалась пустая строка, и такие обои молча не применялись. Голден
/// хранит именно это поведение, а порт отвечает `web`.
#[test]
fn htm_entry_is_now_recognised() {
    let g = fx();
    assert_eq!(
        g["web_htm_entry"]["want"].as_str(),
        Some(""),
        "оригинал не опознавал .htm"
    );
    assert_eq!(infer(&g["web_htm_entry"]), "web", "порт опознаёт");

    // Без входного файла `.htm` рядом по-прежнему ничего не значит: ищется
    // только index.html, и это поведение НЕ менялось.
    assert_eq!(g["web_htm_only"]["want"].as_str(), Some(""));
    assert_eq!(infer(&g["web_htm_only"]), "");
}

/// Порядок проверок значим: видео раньше веба, веб раньше приложения,
/// упакованная сцена раньше разложенной.
#[test]
fn probe_order_is_preserved() {
    let g = fx();
    assert_eq!(infer(&g["video_beats_index_html"]), "video");
    // Вход .exe рядом с index.html даёт web — проверка веба стоит раньше.
    assert_eq!(infer(&g["index_html_beats_exe"]), "web");
    assert_eq!(infer(&g["web_index_beats_scene_pkg"]), "web");
    assert_eq!(infer(&g["pkg_beats_json"]), "scene");
    // Входной .json проверяется раньше scene.json.
    assert_eq!(infer(&g["scene_json_entry_bad_default_good"]), "scene");
}

/// Сценой считается только объект с массивом `objects`; пустой массив
/// сценой считается, словарь вместо массива — нет.
#[test]
fn scene_json_shape_rules() {
    let g = fx();
    assert_eq!(infer(&g["scene_json_empty_objects"]), "scene");
    assert_eq!(infer(&g["scene_json_objects_not_list"]), "");
    assert_eq!(infer(&g["scene_json_no_objects"]), "");
    assert_eq!(infer(&g["scene_json_not_dict"]), "");
    // Битый JSON и нечитаемый в UTF-8 файл оригинал глушил — не падаем тоже.
    assert_eq!(infer(&g["scene_json_broken"]), "");
    assert_eq!(infer(&g["scene_json_bad_utf8"]), "");
}

/// Регистр расширения входного файла значения не имеет.
#[test]
fn entry_extension_case_insensitive() {
    let g = fx();
    assert_eq!(infer(&g["video_upper"]), "video");
}

/// `infer_type_fs` на НАСТОЯЩЕМ каталоге даёт то же, что общая реализация.
///
/// Продукт ходит именно этим путём, а весь голден выше снят на подставном
/// вводе-выводе — без этой проверки файловая обёртка осталась бы
/// непокрытой, и опечатка в склейке путей прошла бы незамеченной.
#[test]
fn fs_wrapper_matches_generic() {
    use std::fs;
    use std::path::PathBuf;

    let base: PathBuf = std::env::temp_dir().join(format!(
        "rexon-infer-type-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&base);

    // (имя случая, файлы каталога, входной файл, ожидаемый тип)
    let cases: &[(&str, &[(&str, &str)], &str, &str)] = &[
        ("видео", &[("clip.mp4", "")], "clip.mp4", "video"),
        ("веб по расширению", &[("page.html", "")], "page.html", "web"),
        // index.html рядом делает обои веб-обоями, даже если вход назван иначе
        ("веб по index.html", &[("index.html", "")], "нечто.bin", "web"),
        ("приложение", &[("run.exe", "")], "run.exe", "application"),
        ("упакованная сцена", &[("scene.pkg", "")], "нечто.bin", "scene"),
        (
            "разложенная сцена по входу",
            &[("assets.json", r#"{"objects": []}"#)],
            "assets.json",
            "scene",
        ),
        (
            "разложенная сцена запасным путём",
            &[("scene.json", r#"{"objects": [1]}"#)],
            "нечто.bin",
            "scene",
        ),
        ("не подошло ничего", &[("нечто.bin", "")], "нечто.bin", ""),
        // Каталога вовсе нет — не паникуем, отвечаем «не определилось».
        ("каталога нет", &[], "нечто.bin", ""),
    ];

    for (i, (name, files, entry, expected)) in cases.iter().enumerate() {
        let dir = base.join(format!("case{i}"));
        if !files.is_empty() {
            fs::create_dir_all(&dir).unwrap();
            for (fname, body) in *files {
                fs::write(dir.join(fname), body).unwrap();
            }
        }
        let project = project_with_entry(entry);
        let got = up::infer_type_fs(&dir, &project);
        assert_eq!(got, *expected, "случай «{name}»");
    }

    let _ = fs::remove_dir_all(&base);
}
