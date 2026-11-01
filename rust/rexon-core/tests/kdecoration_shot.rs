//! Сверка снимка рамки KDE для плитки «Оформления окон»
//! (`kdecoration::shot_*`, `local_preview::shot_picture`). Ожидания —
//! `tests/golden/kdecoration_shot_cases.json`.
//!
//! Последний тест снимает настоящую рамку Breeze, если на машине есть
//! запускатель `qml` Qt6 и модуль превью KWin; иначе печатает «НЕ ПРОВЕРЕНО».

use rexon_core::kdecoration as kd;
use rexon_core::local_preview as lp;
use serde_json::Value;

fn cases() -> Value {
    serde_json::from_str(include_str!("golden/kdecoration_shot_cases.json")).expect("эталон читается")
}

fn s(v: &Value) -> Option<&str> {
    v.as_str()
}

fn request_of(v: &Value) -> kd::ShotRequest {
    kd::ShotRequest {
        library: v["library"].as_str().unwrap().into(),
        theme: v["theme"].as_str().unwrap().into(),
        caption: v["caption"].as_str().unwrap().into(),
        border_index: v["border_index"].as_u64().unwrap() as usize,
    }
}

#[test]
fn runner_needs_qt6_qml_and_kwin_preview_module() {
    for c in cases()["runner"].as_array().unwrap() {
        let files: Vec<&str> = c["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        let got = kd::shot_runner(&|p| files.contains(&p));
        assert_eq!(got.as_deref(), s(&c["want"]), "{}", c["name"]);
    }
}

#[test]
fn border_size_follows_kwinrc() {
    for c in cases()["border"].as_array().unwrap() {
        let got = kd::border_index(s(&c["size"]), s(&c["auto"]));
        assert_eq!(got as u64, c["want"].as_u64().unwrap(), "{}", c["name"]);
    }
}

#[test]
fn request_is_built_from_tile_id() {
    for c in cases()["request"].as_array().unwrap() {
        let got = kd::shot_request(
            c["id"].as_str().unwrap(),
            c["caption"].as_str().unwrap(),
            c["svg_library"].as_str().unwrap(),
            c["border"].as_u64().unwrap() as usize,
        );
        let want = (!c["want"].is_null()).then(|| request_of(&c["want"]));
        assert_eq!(got, want, "{}", c["name"]);
    }
}

#[test]
fn argv_and_cache_tag() {
    let all = cases();
    for c in all["argv"].as_array().unwrap() {
        let got = kd::shot_argv(
            c["runner"].as_str().unwrap(),
            c["script"].as_str().unwrap(),
            &request_of(&c["request"]),
            c["out"].as_str().unwrap(),
        );
        let want: Vec<String> = c["want"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect();
        assert_eq!(got, want, "{}", c["name"]);
    }
    for c in all["tag"].as_array().unwrap() {
        let got = kd::shot_cache_tag(s(&c["runner"]), c["border"].as_u64().unwrap() as usize, c["stamp"].as_i64().unwrap());
        assert_eq!(got, c["want"].as_str().unwrap(), "{}", c["name"]);
    }
}

#[test]
fn script_is_the_data_file_and_uses_kwin_preview_types() {
    let on_disk = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kdecoration-preview/preview.qml"))
        .expect("сценарий лежит в data/");
    assert_eq!(kd::SHOT_SCRIPT, on_disk);
    for needle in ["org.kde.kwin.private.kdecoration", "KDecoration.Bridge", "KDecoration.Settings", "KDecoration.Decoration", "grabToImage"] {
        assert!(kd::SHOT_SCRIPT.contains(needle), "в сценарии нет {needle}");
    }
}

#[test]
fn shot_is_laid_over_the_common_backdrop() {
    let p = lp::shot_picture("/tmp/x.png", "рамка окна, нарисованная её модулем");
    assert_eq!(p.source, lp::PreviewSource::Sample);
    assert_eq!((p.width, p.height), (lp::PREVIEW_WIDTH, lp::PREVIEW_HEIGHT));
    assert_eq!(p.ops.len(), 2);
    assert!(matches!(p.ops[0], lp::DrawOp::Gradient { .. }));
    assert_eq!(
        p.ops[1],
        lp::DrawOp::Image {
            rect: (0.0, 0.0, lp::PREVIEW_WIDTH as f64, lp::PREVIEW_HEIGHT as f64),
            path: "/tmp/x.png".into(),
            cover: false
        }
    );
}

#[test]
fn live_breeze_shot_or_not_checked() {
    let Some(runner) = kd::shot_runner(&|p| std::path::Path::new(p).is_file()) else {
        eprintln!("НЕ ПРОВЕРЕНО: нет запускателя qml Qt6 или модуля превью KWin");
        return;
    };
    let breeze = kd::PLUGIN_NAMESPACES.iter().any(|ns| std::path::Path::new(&format!("/usr/lib/qt6/plugins/{ns}/org.kde.breeze.so")).is_file());
    if !breeze {
        eprintln!("НЕ ПРОВЕРЕНО: модуля Breeze нет");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rexon-kdeco-shot-{}", std::process::id()));
    let dir_s = dir.to_string_lossy().into_owned();
    let out = format!("{dir_s}/breeze.png");
    let req = kd::shot_request("local:aurorae:org.kde.breeze/Breeze", "Breeze", kd::AURORAE_V2, kd::BORDER_NORMAL).unwrap();
    assert!(kd::take_shot(&runner, &dir_s, &req, &out), "Breeze не снялся");
    assert_eq!(lp::image_size(std::path::Path::new(&out)), Some((lp::PREVIEW_WIDTH as f64, lp::PREVIEW_HEIGHT as f64)));
    // Несуществующий модуль: снимка нет, файла тоже.
    let bad = kd::ShotRequest { library: "org.example.none".into(), ..req };
    let out_bad = format!("{dir_s}/none.png");
    assert!(!kd::take_shot(&runner, &dir_s, &bad, &out_bad));
    assert!(!std::path::Path::new(&out_bad).exists());
    let _ = std::fs::remove_dir_all(&dir);
}
