//! Сверка порта `rexon/rxn_v4_installer.py` с живым Python.
//!
//! Эталон `golden/rxn_v4_installer_cases.json` снят read-only из чистой
//! Python-ветки (Python-путь до его удаления): slug/hash/сбор путей/проверка
//! наличия/валидация/удаление-slug/раскладка треков при листинге. Реальная
//! распаковка НЕ исполнялась — план и решения сверяются по наблюдениям.
//! `golden/sample.rxn` — реальный Python-архив (для `short_hash`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rexon_core::rxn_v4_installer::{
    build_listed_entry, collect_manifest_paths, plan_install_v4, remove_slug_error, short_hash,
    slugify, verify_paths_present, InstallOutcome, InstallStep,
};
use serde_json::{Map, Value};

mod i18n_pin;

fn golden() -> Value {
    let raw = include_str!("golden/rxn_v4_installer_cases.json");
    serde_json::from_str(raw).unwrap()
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn obj(v: &Value) -> &Map<String, Value> {
    v.as_object().unwrap()
}

/// `_slugify` байт-в-байт на разных title (латиница/кириллица/пунктуация).
#[test]
fn slugify_matches_python() {
    for c in golden()["slugify"].as_array().unwrap() {
        let title = c["title"].as_str().unwrap();
        let want = c["slug"].as_str().unwrap();
        assert_eq!(slugify(title), want, "slugify({title:?})");
    }
}

/// `short_hash` реального Python-архива совпадает с `_short_hash`.
#[test]
fn short_hash_matches_python_sample() {
    let g = golden();
    let file = g["short_hash"]["file"].as_str().unwrap();
    let want = g["short_hash"]["hash"].as_str().unwrap();
    let data = std::fs::read(golden_dir().join(file)).unwrap();
    assert_eq!(short_hash(&data), want);
}

/// Известные векторы SHA-256 (без Python) — проверяют hex + усечение до 8.
#[test]
fn short_hash_known_vectors() {
    // sha256("") = e3b0c442..., sha256("abc") = ba7816bf...
    assert_eq!(short_hash(b""), "e3b0c442");
    assert_eq!(short_hash(b"abc"), "ba7816bf");
}

/// `_collect_manifest_paths`: порядок и состав путей.
#[test]
fn collect_paths_matches_python() {
    for c in golden()["collect"].as_array().unwrap() {
        let meta = obj(&c["manifest"]);
        let want: Vec<String> = c["paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(collect_manifest_paths(meta), want);
    }
}

/// `_verify_paths_inside_zip` (чистая часть): ok/err-тексты байт-в-байт.
#[test]
fn verify_paths_matches_python() {
    for c in golden()["verify"].as_array().unwrap() {
        let names: HashSet<String> = c["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let paths: Vec<String> = c["paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let res = verify_paths_present(&names, &paths);
        if c["ok"].as_bool().unwrap() {
            assert!(res.is_ok(), "ожидался ok для {paths:?}");
        } else {
            assert_eq!(res.unwrap_err(), c["err"].as_str().unwrap());
        }
    }
}

/// `plan_install_v4`: текст «Неверный manifest: …» на невалидных манифестах.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plan_rejects_invalid_manifest() {
    // Хвост сообщения — текст валидатора manifest_v4 (каталог перевода),
    // эталон Python — русский: он в po/ru.po.
    let _lang = i18n_pin::ru();
    let names: HashSet<String> = HashSet::new();
    for c in golden()["validate_msg"].as_array().unwrap() {
        let manifest = &c["manifest"];
        let out = plan_install_v4(manifest, &names, "deadbeef", "/root", false, "/a.rxn");
        match out {
            InstallOutcome::Error { message } => {
                assert_eq!(message, c["message"].as_str().unwrap());
            }
            other => panic!("ожидалась Error, получено {other:?}"),
        }
    }
}

/// `plan_install_v4`: happy-path даёт план из 3 шагов и корректный каталог.
#[test]
fn plan_install_builds_steps() {
    // Валидный минимальный v4-манифест.
    let manifest = serde_json::json!({
        "format_version": 4,
        "type": "wallpaper-video-adaptive",
        "title": "Demo A",
        "videos": {"1080p": "data/video/1080p.mp4"},
        "fallback_video": "data/video/1080p.mp4",
        "default_locale": "ru"
    });
    let names: HashSet<String> = ["data/video/1080p.mp4".to_string()].into_iter().collect();
    let root = "/home/u/.local/share/rexon/wallpapers";
    let out = plan_install_v4(&manifest, &names, "5623f3d7", root, false, "/tmp/demo.rxn");
    let expected_dir = format!("{root}/demo-a-5623f3d7");
    match out {
        InstallOutcome::Plan { install_dir, steps } => {
            assert_eq!(install_dir, expected_dir);
            assert_eq!(
                steps,
                vec![
                    InstallStep::MakeDirs { path: root.to_string(), exist_ok: true },
                    InstallStep::MakeDirs { path: expected_dir.clone(), exist_ok: false },
                    InstallStep::ExtractZip {
                        archive: "/tmp/demo.rxn".to_string(),
                        dest: expected_dir,
                    },
                ]
            );
        }
        other => panic!("ожидался Plan, получено {other:?}"),
    }
}

/// `plan_install_v4`: существующий каталог → AlreadyInstalled (без шагов).
#[test]
fn plan_install_already_installed() {
    let manifest = serde_json::json!({
        "format_version": 4,
        "type": "wallpaper-video-adaptive",
        "title": "Demo A",
        "videos": {"1080p": "data/video/1080p.mp4"},
        "fallback_video": "data/video/1080p.mp4",
        "default_locale": "ru"
    });
    let names: HashSet<String> = ["data/video/1080p.mp4".to_string()].into_iter().collect();
    let root = "/root/wp";
    let out = plan_install_v4(&manifest, &names, "5623f3d7", root, true, "/a.rxn");
    assert_eq!(
        out,
        InstallOutcome::AlreadyInstalled { install_dir: format!("{root}/demo-a-5623f3d7") }
    );
}

/// `plan_install_v4`: недостающий трек → Error с текстом verify.
#[test]
fn plan_install_missing_track() {
    let manifest = serde_json::json!({
        "format_version": 4,
        "type": "wallpaper-video-adaptive",
        "title": "T",
        "videos": {"1080p": "data/video/1080p.mp4"},
        "fallback_video": "data/video/1080p.mp4",
        "default_locale": "ru"
    });
    let names: HashSet<String> = HashSet::new(); // архив пуст
    let out = plan_install_v4(&manifest, &names, "abc", "/r", false, "/a.rxn");
    assert_eq!(
        out,
        InstallOutcome::Error {
            message: "Объявленный в manifest файл отсутствует в архиве: 'data/video/1080p.mp4'"
                .to_string()
        }
    );
}

/// `remove_rxn_v4_wallpaper` (чистая часть): валидация slug байт-в-байт.
#[test]
fn remove_slug_matches_python() {
    for c in golden()["remove_slug"].as_array().unwrap() {
        let slug = c["slug"].as_str().unwrap();
        match c["error"].as_str() {
            Some(msg) => assert_eq!(remove_slug_error(slug), Some(msg.to_string()), "slug {slug:?}"),
            None => assert_eq!(remove_slug_error(slug), None, "slug {slug:?}"),
        }
    }
}

/// `list_rxn_v4_wallpapers` (обогащение записи): meta_out 1-в-1 с Python,
/// включая порядок ключей и раскладку video/audio-треков.
#[test]
fn list_entry_matches_python() {
    for c in golden()["list_entries"].as_array().unwrap() {
        let sub = c["sub"].as_str().unwrap();
        let manifest = obj(&c["manifest"]);
        let got = build_listed_entry(
            sub,
            manifest,
            c["preview_exists"].as_bool().unwrap(),
            c["fallback_video_exists"].as_bool().unwrap(),
            c["fallback_audio_exists"].as_bool().unwrap(),
        );
        assert_eq!(got, c["expected"], "entry {sub}");
        // Порядок ключей meta_out идентичен (preserve_order).
        let got_keys: Vec<&String> = got.as_object().unwrap().keys().collect();
        let want_keys: Vec<&String> = c["expected"].as_object().unwrap().keys().collect();
        assert_eq!(got_keys, want_keys, "порядок ключей entry {sub}");
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  Корень установки и тексты начала установки
// ══════════════════════════════════════════════════════════════════════════
//
// `install_root` — починка оригинала (он игнорировал `$XDG_DATA_HOME`):
// при стандартном окружении путь совпадает с прежним литералом, при
// нестандартном уходит в XDG-корень данных, как весь остальной Rexon.

#[test]
fn install_root_follows_xdg_data_home() {
    use rexon_core::rxn_v4_installer as R;
    // стандартное окружение — путь как у оригинала
    assert_eq!(
        R::install_root("/home/wehr", None),
        "/home/wehr/.local/share/rexon/wallpapers"
    );
    assert_eq!(
        R::install_root("/home/wehr", Some("")),
        "/home/wehr/.local/share/rexon/wallpapers"
    );
    // нестандартный XDG_DATA_HOME — путь уходит туда
    assert_eq!(
        R::install_root("/home/wehr", Some("/data/xdg")),
        "/data/xdg/rexon/wallpapers"
    );
    // тот же ответ, что у общего расширителя проекта
    assert_eq!(
        R::install_root("/home/wehr", Some("/data/xdg")),
        rexon_core::utils::expand_user_xdg(
            "~/.local/share/rexon/wallpapers",
            "/home/wehr",
            None,
            Some("/data/xdg")
        )
    );
}

#[test]
fn start_messages_match_python() {
    use rexon_core::rxn_v4_installer as R;
    assert_eq!(R::MSG_FILE_NOT_FOUND_PREFIX, "Файл не найден: ");
    assert_eq!(
        R::MSG_MANIFEST_READ_FAILED_PREFIX,
        "Не удалось прочитать manifest: "
    );
}
