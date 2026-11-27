//! Golden check of the port of detecting a wallpaper type by content
//! (`user_props::infer_type`) against the original
//! `rexon/wallpapers/factory.py::_infer_type`.
//!
//! The golden was captured by running the LIVE `_infer_type` on real temporary
//! directories: for each case files were laid out, a `Project` was built with
//! the needed entry file, and the returned type was recorded.
//!
//! `golden/infer_type_cases.json` holds the captured values: for each case the
//! directory contents (file name → content, `null` — the file is not readable as
//! UTF-8), the entry file and the expected type.

use rexon_core::user_props as up;
use serde_json::Value;
use std::collections::BTreeMap;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/infer_type_cases.json")).unwrap()
}

/// A wallpaper directory from the golden. Content `None` means a file that exists
/// but is not readable as UTF-8.
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

/// Run one golden case and return what came out.
fn infer(case: &Value) -> String {
    let project = project_with_entry(case["entry"].as_str().unwrap());
    let mut io = DirIo::from(case);
    up::infer_type("/обои", &project, &mut io).to_string()
}

/// All cases except those where the port deliberately differs from the original.
#[test]
fn all_cases_match_original() {
    let g = fx();
    // Deviations under rule #16 are handled by separate checks below.
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

/// The set of video extensions is the same as in the original, PLUS `.ogv`.
///
/// ⚠️ FIXED (rule #16). There were two lists: this one and the Python video host's
/// own, and they diverged — only the host knew `.ogv`. Because of this wallpapers with
/// an `.ogv` entry and no `type` field in `project.json` were detected as "nothing
/// matched" and silently not applied, although the host itself plays them.
/// The golden keeps the original's set, the port adds `.ogv` to it.
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
    // Order matters only for reading; `.ogv` went before `.gif`, as it
    // stood in the video host's list.
    want.insert(want.len() - 1, ".ogv");
    // Windows DreamScene clips are played by the host itself — they went in the same place, before `.gif`.
    for ext in [".wmv", ".mpg", ".mpeg"] {
        assert!(!want.contains(&ext), "голден оригинала уже знает {ext}");
        want.insert(want.len() - 1, ext);
    }
    assert_eq!(up::VIDEO_ENTRY_EXTS.to_vec(), want);
}

/// An `.ogv` entry is now recognized as video (see the fix above).
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

/// ⚠️ FIXED (rule #16). The original did not recognize an `.htm` entry at all —
/// an empty string was returned, and such wallpapers were silently not applied. The golden
/// keeps exactly this behaviour, and the port answers `web`.
#[test]
fn htm_entry_is_now_recognised() {
    let g = fx();
    assert_eq!(
        g["web_htm_entry"]["want"].as_str(),
        Some(""),
        "оригинал не опознавал .htm"
    );
    assert_eq!(infer(&g["web_htm_entry"]), "web", "порт опознаёт");

    // Without an entry file, an `.htm` nearby still means nothing: only
    // index.html is looked for, and this behaviour did NOT change.
    assert_eq!(g["web_htm_only"]["want"].as_str(), Some(""));
    assert_eq!(infer(&g["web_htm_only"]), "");
}

/// The order of checks matters: video before web, web before application,
/// packed scene before an unpacked one.
#[test]
fn probe_order_is_preserved() {
    let g = fx();
    assert_eq!(infer(&g["video_beats_index_html"]), "video");
    // An .exe entry next to index.html gives web — the web check comes first.
    assert_eq!(infer(&g["index_html_beats_exe"]), "web");
    assert_eq!(infer(&g["web_index_beats_scene_pkg"]), "web");
    assert_eq!(infer(&g["pkg_beats_json"]), "scene");
    // An entry .json is checked before scene.json.
    assert_eq!(infer(&g["scene_json_entry_bad_default_good"]), "scene");
}

/// Only an object with an `objects` array counts as a scene; an empty array
/// counts as a scene, a dictionary instead of an array does not.
#[test]
fn scene_json_shape_rules() {
    let g = fx();
    assert_eq!(infer(&g["scene_json_empty_objects"]), "scene");
    assert_eq!(infer(&g["scene_json_objects_not_list"]), "");
    assert_eq!(infer(&g["scene_json_no_objects"]), "");
    assert_eq!(infer(&g["scene_json_not_dict"]), "");
    // The original swallowed broken JSON and a file unreadable as UTF-8 — we do not crash either.
    assert_eq!(infer(&g["scene_json_broken"]), "");
    assert_eq!(infer(&g["scene_json_bad_utf8"]), "");
}

/// The case of the entry file's extension does not matter.
#[test]
fn entry_extension_case_insensitive() {
    let g = fx();
    assert_eq!(infer(&g["video_upper"]), "video");
}

/// `infer_type_fs` on a REAL directory gives the same as the shared implementation.
///
/// The product goes exactly this way, and the whole golden above was captured on fake
/// I/O — without this check the file wrapper would stay
/// uncovered, and a typo in path joining would go unnoticed.
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

    // (case name, directory files, entry file, expected type)
    let cases: &[(&str, &[(&str, &str)], &str, &str)] = &[
        ("видео", &[("clip.mp4", "")], "clip.mp4", "video"),
        ("веб по расширению", &[("page.html", "")], "page.html", "web"),
        // index.html nearby makes the wallpaper a web wallpaper, even if the entry is named differently
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
        // No directory at all — we do not panic, we answer "not determined".
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
