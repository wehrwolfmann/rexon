//! Golden-сверка порта `youtube_wallpaper.rs` с оригиналом
//! `rexon/youtube_wallpaper.py`.
//!
//! Эталон снят read-only с ЖИВОГО `rexon.youtube_wallpaper` одноразовым
//! генератором. Реальный yt-dlp / скачивание / subprocess НЕ исполнялись:
//! stdout `--get-url`/`--dump-json`/`--flat-playlist` синтетический, HOME и
//! кеш нормализованы в `/HOME` (`XDG_CACHE_HOME=/HOME/.cache`), `time.time()`
//! → фикс. Покрыто: выбор формата по качеству (stream/download), пути кеша
//! (cache_dir/meta/output-template/meta_path) и хеш URL, валидация/
//! классификация URL (supported/youtube/playlist), argv всех вызовов yt-dlp
//! (+ install/update/version), разбор `--get-url`, прогресс скачивания,
//! список форматов, карточка видео, разбор плейлиста, format_info_text,
//! вытеснение из кеша, предикат кеш-файла, precheck длительности/live.
//!
//! `golden/youtube_wallpaper_cases.json` — снятые значения.

use rexon_core::youtube_wallpaper as yt;
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/youtube_wallpaper_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn opt_s(v: &Value) -> Option<String> {
    v.as_str().map(String::from)
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

const HOME: &str = "/HOME";
const URL: &str = "https://youtu.be/dQw4w9WgXcQ";

#[test]
fn constants() {
    let g = fx();
    assert_eq!(yt::MAX_CACHE_MB, g["max_cache_mb"].as_u64().unwrap());
    assert_eq!(yt::MAX_DURATION_SEC, g["max_duration_sec"].as_i64().unwrap());
    assert_eq!(
        yt::MAX_DOWNLOAD_TIMEOUT,
        g["max_download_timeout"].as_u64().unwrap()
    );
    assert_eq!(yt::STREAM_FORMAT, s(&g["stream_format"]));
}

#[test]
fn format_selection() {
    let g = fx();
    for (q, fmt) in g["quality_map"].as_object().unwrap() {
        assert_eq!(yt::stream_format(q), s(fmt), "stream_format {q}");
    }
    for (q, fmt) in g["download_format"].as_object().unwrap() {
        assert_eq!(yt::download_format(q), s(fmt), "download_format {q}");
    }
}

#[test]
fn cache_paths_and_hash() {
    let g = fx();
    // cache_dir: XDG задан ИЛИ выведен из HOME — оба дают тот же путь
    let cd_xdg = yt::cache_dir(Some("/HOME/.cache"), HOME);
    let cd_home = yt::cache_dir(None, HOME);
    assert_eq!(cd_xdg, s(&g["cache_dir"]));
    assert_eq!(cd_home, s(&g["cache_dir"]));

    let md = yt::meta_dir(&cd_xdg);
    assert_eq!(md, s(&g["meta_dir"]));

    let vh = yt::video_id_hash(URL);
    assert_eq!(vh, s(&g["vid_hash"]));
    assert_eq!(yt::meta_path(&md, &vh), s(&g["meta_path"]));
    assert_eq!(yt::output_template(&cd_xdg, &vh), s(&g["output_template"]));

    for (url, h) in g["video_id_hash"].as_object().unwrap() {
        assert_eq!(yt::video_id_hash(url), s(h), "hash {url}");
    }
}

#[test]
fn meta_cache_freshness() {
    let g = fx();
    for case in g["meta_fresh"].as_array().unwrap() {
        let age = case["age"].as_f64().unwrap();
        assert_eq!(
            yt::meta_cache_fresh(age),
            case["fresh"].as_bool().unwrap(),
            "meta_fresh {age}"
        );
    }
}

#[test]
fn url_supported() {
    let g = fx();
    for (url, expected) in g["is_supported"].as_object().unwrap() {
        assert_eq!(
            yt::is_supported_url(url),
            expected.as_bool().unwrap(),
            "supported {url:?}"
        );
    }
}

#[test]
fn url_youtube() {
    let g = fx();
    for (url, expected) in g["is_youtube"].as_object().unwrap() {
        assert_eq!(
            yt::is_youtube_url(url),
            expected.as_bool().unwrap(),
            "youtube {url:?}"
        );
    }
}

#[test]
fn url_playlist() {
    let g = fx();
    for (url, expected) in g["is_playlist"].as_object().unwrap() {
        assert_eq!(
            yt::is_playlist_url(url),
            expected.as_bool().unwrap(),
            "playlist {url:?}"
        );
    }
}

#[test]
fn argv_info() {
    let g = fx();
    assert_eq!(yt::info_argv(URL), str_vec(&g["info_cmd"]));
}

#[test]
fn argv_stream() {
    let g = fx();
    assert_eq!(
        yt::stream_argv(URL, yt::stream_format("1080p")),
        str_vec(&g["stream_cmd_1080p"])
    );
    assert_eq!(
        yt::stream_argv(URL, yt::stream_format("best")),
        str_vec(&g["stream_cmd_best"])
    );
    // неизвестное качество → STREAM_FORMAT
    assert_eq!(
        yt::stream_argv(URL, yt::stream_format("unknown")),
        str_vec(&g["stream_cmd_unknown"])
    );
}

#[test]
fn argv_download() {
    let g = fx();
    let cd = yt::cache_dir(None, HOME);
    let vh = yt::video_id_hash(URL);
    let tmpl = yt::output_template(&cd, &vh);
    assert_eq!(
        yt::download_argv(URL, yt::download_format("1080p"), &tmpl),
        str_vec(&g["download_cmd_1080p"])
    );
    assert_eq!(
        yt::download_argv(URL, yt::download_format("4k"), &tmpl),
        str_vec(&g["download_cmd_4k"])
    );
    // неизвестное качество → 1080p
    assert_eq!(
        yt::download_argv(URL, yt::download_format("unknown"), &tmpl),
        str_vec(&g["download_cmd_unknown"])
    );
}

#[test]
fn argv_playlist() {
    let g = fx();
    assert_eq!(yt::playlist_argv(URL, 50), str_vec(&g["playlist_cmd_50"]));
    assert_eq!(yt::playlist_argv(URL, 10), str_vec(&g["playlist_cmd_10"]));
}

#[test]
fn argv_install_update_version() {
    let g = fx();
    assert_eq!(yt::install_pacman_argv(), str_vec(&g["install_pacman"]));
    assert_eq!(yt::install_apt_argv(), str_vec(&g["install_apt"]));
    assert_eq!(yt::install_dnf_argv(), str_vec(&g["install_dnf"]));
    assert_eq!(yt::install_pip_argv(), str_vec(&g["install_pip"]));
    assert_eq!(yt::update_self_argv(), str_vec(&g["update_self"]));
    assert_eq!(yt::update_pip_argv(), str_vec(&g["update_pip"]));
    assert_eq!(yt::version_argv(), str_vec(&g["version_cmd"]));
}

/// pip-ветка установки: имя бинаря и `--break-system-packages` по факту
/// отказа PEP 668, а не по семейству дистрибутива.
#[test]
fn pip_fallback() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let c = &g["pip_argv"];
    assert_eq!(yt::PIP_BINARIES.to_vec(), str_vec(&c["binaries"]));
    assert_eq!(yt::pip_argv("pip3", false, false), str_vec(&c["plain"]));
    assert_eq!(yt::pip_argv("pip3", false, true), str_vec(&c["break"]));
    assert_eq!(yt::pip_argv("pip3", true, false), str_vec(&c["upgrade"]));
    assert_eq!(
        yt::pip_argv("pip", true, true),
        str_vec(&c["upgrade_break"])
    );

    let p = &g["pip_pep668"];
    for case in p["yes"].as_array().unwrap() {
        assert!(yt::pip_break_system_needed(s(case)), "{case}");
    }
    for case in p["no"].as_array().unwrap() {
        assert!(!yt::pip_break_system_needed(s(case)), "{case}");
    }

    let m = &g["install_messages"];
    assert_eq!(yt::msg_installed_via("pacman"), s(&m["via_pacman"]));
    assert_eq!(yt::msg_installed_via("zypper"), s(&m["via_zypper"]));
    assert_eq!(yt::msg_installed_via_pip("pip3"), s(&m["via_pip"]));
    assert_eq!(yt::msg_updated_via_pip("pip3"), s(&m["updated_pip"]));
    assert_eq!(yt::msg_install_failed(), s(&m["failed"]));
}

#[test]
fn parse_get_url() {
    let g = fx();
    for case in g["stream_out"].as_array().unwrap() {
        let stdout = s(&case["stdout"]);
        assert_eq!(
            yt::parse_stream_url(stdout),
            opt_s(&case["result"]),
            "stream_out {stdout:?}"
        );
    }
}

#[test]
fn parse_download_progress() {
    let g = fx();
    for case in g["progress"].as_array().unwrap() {
        let line = s(&case["line"]);
        let expected = case["pct"].as_f64();
        assert_eq!(yt::parse_progress(line), expected, "progress {line:?}");
    }
}

#[test]
fn extract_formats() {
    let g = fx();
    // Вход синтетический — та же yt-dlp-структура, что в генераторе; сверяем
    // прямой выход _extract_format_list (в golden — "extract_formats").
    let input = serde_json::json!({
        "formats": [
            {"height": 1080, "fps": 30, "vcodec": "avc1", "filesize": 50000000},
            {"height": 1080, "fps": 60, "vcodec": "vp9", "filesize": 40000000},
            {"height": 720, "fps": 30, "vcodec": "avc1", "filesize_approx": 20000000},
            {"height": 480, "vcodec": "avc1"},
            {"height": 144, "fps": 30},
            {"height": null},
            {"fps": 30},
            {"height": 2160, "fps": 30, "vcodec": "vp9", "filesize": 0, "filesize_approx": 99}
        ]
    });
    let out = Value::from(yt::extract_format_list(&input));
    assert_eq!(out, g["extract_formats"]);
}

#[test]
fn build_info_card() {
    let g = fx();
    let input = serde_json::json!({
        "id": "dQw4w9WgXcQ",
        "title": "Rick Astley - Never Gonna Give You Up",
        "duration": 213,
        "thumbnail": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg",
        "uploader": "Rick Astley",
        "view_count": 1600000000i64,
        "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "is_live": false,
        "description": "The official video. ".repeat(40),
        "formats": [
            {"height": 1080, "fps": 30, "vcodec": "avc1", "filesize": 50000000},
            {"height": 1080, "fps": 60, "vcodec": "vp9", "filesize": 40000000},
            {"height": 720, "fps": 30, "vcodec": "avc1", "filesize_approx": 20000000},
            {"height": 480, "vcodec": "avc1"},
            {"height": 144, "fps": 30},
            {"height": null},
            {"fps": 30},
            {"height": 2160, "fps": 30, "vcodec": "vp9", "filesize": 0, "filesize_approx": 99}
        ]
    });
    let vh = yt::video_id_hash(URL);
    let out = yt::build_video_info(&input, URL, &vh, 1234567890.5);
    assert_eq!(out, g["build_info"]);
}

#[test]
fn info_text_variants() {
    let g = fx();
    for case in g["info_text"].as_array().unwrap() {
        let info = &case["info"];
        assert_eq!(
            yt::format_info_text(info),
            s(&case["text"]),
            "info_text {}",
            s(&case["label"])
        );
    }
}

#[test]
fn playlist_parse() {
    let g = fx();
    let stdout = s(&g["playlist_stdout"]);
    let out = Value::from(yt::parse_playlist(stdout));
    assert_eq!(out, g["playlist_entries"]);
}

#[test]
fn cache_file_predicate() {
    let g = fx();
    let vh = s(&g["vid_hash"]);
    for case in g["cached_name"].as_array().unwrap() {
        assert_eq!(
            yt::is_cached_file(s(&case["name"]), vh),
            case["cached"].as_bool().unwrap(),
            "cached_name {}",
            s(&case["name"])
        );
    }
}

#[test]
fn cache_eviction() {
    let g = fx();
    let files: Vec<(String, f64, u64)> = g["evict_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                s(&f["path"]).to_string(),
                f["mtime"].as_f64().unwrap(),
                f["size"].as_u64().unwrap(),
            )
        })
        .collect();
    let size = g["evict_size_mb"].as_f64().unwrap();
    let deleted = yt::select_evictions(size, &files);
    assert_eq!(deleted, str_vec(&g["evictions"]));
    // в пределах лимита → ничего не удаляем
    let under = yt::select_evictions(1000.0, &files);
    assert_eq!(under, str_vec(&g["evict_under_limit"]));
    // dir_size_mb: сумма байт / 1 МиБ (2600 МБ)
    let sizes: Vec<u64> = files.iter().map(|f| f.2).collect();
    assert_eq!(yt::dir_size_mb(&sizes), size);
}

#[test]
fn download_precheck() {
    let g = fx();
    for case in g["precheck"].as_array().unwrap() {
        let dur = case["duration"].as_i64().unwrap();
        let live = case["is_live"].as_bool().unwrap();
        assert_eq!(
            yt::download_block_reason(dur, live),
            opt_s(&case["reason"]),
            "precheck dur={dur} live={live}"
        );
    }
}

/// Скачанный ролик в кеше: только наш хеш, без недокачанных `.part` и
/// `.ytdl`, первое имя по порядку.
#[test]
fn cached_file_is_found_by_hash() {
    let names: Vec<String> = ["zz.mp4", "abc.webm.part", "abc.mp4", "abc.ytdl", "abd.mp4"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(yt::cached_file_name(&names, "abc"), Some("abc.mp4".to_string()));
    assert_eq!(yt::cached_file_name(&names, "нет"), None);
}

/// pip: удача с первой попытки — без флага; отказ PEP 668 — ровно один
/// повтор с `--break-system-packages`; любой другой отказ — без повтора.
#[test]
fn pip_retries_only_on_pep668() {
    let pep668 = "error: externally-managed-environment".to_string();

    let mut calls: Vec<Vec<String>> = Vec::new();
    let ok = yt::pip_run_with(
        &mut |argv| {
            calls.push(argv.to_vec());
            Some((0, String::new(), String::new()))
        },
        "pip3",
        false,
    );
    assert!(ok);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].iter().any(|a| a == "--break-system-packages"));

    let mut calls: Vec<Vec<String>> = Vec::new();
    let ok = yt::pip_run_with(
        &mut |argv| {
            calls.push(argv.to_vec());
            if argv.iter().any(|a| a == "--break-system-packages") {
                Some((0, String::new(), String::new()))
            } else {
                Some((1, String::new(), pep668.clone()))
            }
        },
        "pip3",
        true,
    );
    assert!(ok, "повтор после PEP 668 не случился");
    assert_eq!(calls.len(), 2);

    let mut calls = 0;
    let ok = yt::pip_run_with(
        &mut |_| {
            calls += 1;
            Some((1, String::new(), "нет сети".to_string()))
        },
        "pip",
        false,
    );
    assert!(!ok);
    assert_eq!(calls, 1, "обычный отказ не должен повторяться");

    assert!(!yt::pip_run_with(&mut |_| None, "pip", false), "нет pip — не успех");
}

/// Итоги установки и обновления yt-dlp (их показывает вкладка «Настройки»)
/// по-английски — без единой русской буквы.
#[test]
fn english_ytdlp_messages_have_no_russian() {
    let _lang = i18n_pin::en();
    let all = [
        yt::msg_installed_via("pacman"),
        yt::msg_installed_via_pip("pip3"),
        yt::msg_updated_via_pip("pip3"),
        yt::msg_install_failed(),
        yt::msg_not_installed(),
        yt::msg_updated(),
        yt::msg_update_failed(),
    ];
    for m in &all {
        assert!(
            !m.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
            "русский в английском: {m}"
        );
    }
    assert_eq!(yt::msg_installed_via("pacman"), "yt-dlp installed with pacman");
    assert_eq!(
        yt::msg_install_failed(),
        "Could not install yt-dlp automatically. Install it manually: pip install yt-dlp"
    );
}

/// По-русски — дословно как было до перевода.
#[test]
fn russian_ytdlp_messages_read_as_before() {
    let _lang = i18n_pin::ru();
    assert_eq!(yt::msg_not_installed(), "yt-dlp не установлен");
    assert_eq!(yt::msg_updated(), "yt-dlp обновлён");
    assert_eq!(yt::msg_update_failed(), "Не удалось обновить yt-dlp");
    assert_eq!(yt::msg_installed_via_pip("pip3"), "yt-dlp установлен через pip3 (--user)");
}
