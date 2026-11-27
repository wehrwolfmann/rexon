//! Golden check of the `youtube_wallpaper.rs` port against the original
//! `rexon/youtube_wallpaper.py`.
//!
//! The reference was captured read-only from the LIVE `rexon.youtube_wallpaper`
//! by a one-off generator. Real yt-dlp / download / subprocess were NOT run:
//! the stdout of `--get-url`/`--dump-json`/`--flat-playlist` is synthetic,
//! HOME and the cache are normalised to `/HOME`
//! (`XDG_CACHE_HOME=/HOME/.cache`), `time.time()` → fixed. Covered: format
//! choice by quality (stream/download), cache paths
//! (cache_dir/meta/output-template/meta_path) and the URL hash, URL
//! validation/classification (supported/youtube/playlist), argv of all yt-dlp
//! calls (+ install/update/version), parsing `--get-url`, download progress,
//! format list, video card, playlist parsing, format_info_text, cache
//! eviction, cache-file predicate, duration/live precheck.
//!
//! `golden/youtube_wallpaper_cases.json` — the captured values.

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
    // cache_dir: XDG set OR derived from HOME — both give the same path
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
    // unknown quality → STREAM_FORMAT
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
    // unknown quality → 1080p
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

/// pip install branch: the binary name and `--break-system-packages` depend on
/// an actual PEP 668 refusal, not on the distro family.
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
    // Synthetic input — the same yt-dlp structure as in the generator; we compare
    // the direct output of _extract_format_list (in golden — "extract_formats").
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
    // within the limit → delete nothing
    let under = yt::select_evictions(1000.0, &files);
    assert_eq!(under, str_vec(&g["evict_under_limit"]));
    // dir_size_mb: sum of bytes / 1 MiB (2600 MB)
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

/// A downloaded clip in the cache: only our hash, without partial `.part` and
/// `.ytdl`, the first name in order.
#[test]
fn cached_file_is_found_by_hash() {
    let names: Vec<String> = ["zz.mp4", "abc.webm.part", "abc.mp4", "abc.ytdl", "abd.mp4"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(yt::cached_file_name(&names, "abc"), Some("abc.mp4".to_string()));
    assert_eq!(yt::cached_file_name(&names, "нет"), None);
}

/// pip: success on the first try — no flag; a PEP 668 refusal — exactly one
/// retry with `--break-system-packages`; any other refusal — no retry.
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

/// Results of installing and updating yt-dlp (shown on the «Settings» tab)
/// are in English — without a single Russian letter.
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

/// In Russian — word for word as before the translation.
#[test]
fn russian_ytdlp_messages_read_as_before() {
    let _lang = i18n_pin::ru();
    assert_eq!(yt::msg_not_installed(), "yt-dlp не установлен");
    assert_eq!(yt::msg_updated(), "yt-dlp обновлён");
    assert_eq!(yt::msg_update_failed(), "Не удалось обновить yt-dlp");
    assert_eq!(yt::msg_installed_via_pip("pip3"), "yt-dlp установлен через pip3 (--user)");
}
