//! Golden check of the `wallpaper.rs` port against the original `rexon/wallpaper.py`.
//!
//! The reference was captured by running the same property-override
//! normalisation formula and the same frame-size parsing as in the original on
//! a set of edge values (empty, `{}`, a bare value, an already wrapped one,
//! Cyrillic keys, a non-object, broken JSON, garbage in sizes). ⛔ No
//! wallpapers were launched.
//!
//! `golden/wallpaper_cases.json` — the captured values.

use rexon_core::wallpaper as wp;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/wallpaper_cases.json")).unwrap()
}

#[test]
fn legacy_suffixes_match_original() {
    let g = fx();
    let want: Vec<&str> = g["legacy_suffixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(wp::LEGACY_FILE_SUFFIXES.to_vec(), want);
}

#[test]
fn parent_dir_only_for_legacy_files() {
    // A file with the «old» extension — go up to the directory.
    assert!(wp::use_parent_dir("/обои/index.html", true));
    assert!(wp::use_parent_dir("/обои/scene.json", true));
    assert!(wp::use_parent_dir("/обои/видео.mp4", true));
    assert!(wp::use_parent_dir("/обои/app.EXE", true), "регистр не важен");
    // A file with another extension — no.
    assert!(!wp::use_parent_dir("/обои/preview.png", true));
    // A directory — never, even if the name ends similarly.
    assert!(!wp::use_parent_dir("/обои/scene.json", false));
    assert!(!wp::use_parent_dir("/обои", false));
}

#[test]
fn user_props_override_matches_original() {
    let g = fx();
    for (raw, want) in g["user_props"].as_object().unwrap() {
        let got = wp::parse_user_props_override(raw);
        match want {
            // The original returned None both on an empty value and on valid JSON
            // that is not an object.
            Value::Null => assert_eq!(
                got,
                wp::UserPropsOverride::None,
                "вход {raw:?} должен давать отсутствие переопределения"
            ),
            // The generator's marker for an unparsed value.
            Value::String(s) if s == "<битый>" => assert_eq!(
                got,
                wp::UserPropsOverride::Broken,
                "вход {raw:?} должен читаться как битый"
            ),
            Value::Object(expected) => match got {
                wp::UserPropsOverride::Props(props) => {
                    assert_eq!(&props, expected, "вход {raw:?}")
                }
                other => panic!("вход {raw:?}: ожидались свойства, получено {other:?}"),
            },
            other => panic!("неожиданный эталон {other} для {raw:?}"),
        }
    }
}

/// A bare value is wrapped, an already wrapped one is not touched — otherwise
/// the configurator's saved settings would not reach the renderer.
#[test]
fn user_props_wrapping_is_idempotent() {
    let once = wp::parse_user_props_override(r#"{"скорость": 0.5}"#);
    let wp::UserPropsOverride::Props(props) = once else {
        panic!("ожидались свойства");
    };
    let text = serde_json::to_string(&Value::Object(props.clone())).unwrap();
    let twice = wp::parse_user_props_override(&text);
    assert_eq!(twice, wp::UserPropsOverride::Props(props), "повторный проход");
}

/// A fix in the port: a non-numeric environment variable value crashed the
/// wallpaper launch with `ValueError`; here it falls back to the default.
#[test]
fn mmap_size_survives_garbage() {
    let g = fx();
    let want = g["mmap_size"].as_object().unwrap();

    // Cases where the original worked normally — they match.
    assert_eq!(wp::mmap_size(Some(""), Some("")), (1920, 1080));
    assert_eq!(want["''/''"], serde_json::json!([1920, 1080]));
    assert_eq!(wp::mmap_size(Some("2560"), Some("1440")), (2560, 1440));
    assert_eq!(want["'2560'/'1440'"], serde_json::json!([2560, 1440]));
    assert_eq!(wp::mmap_size(Some("1920"), Some("")), (1920, 1080));
    assert_eq!(wp::mmap_size(Some(" 800 "), Some("600")), (800, 600));
    assert_eq!(wp::mmap_size(None, None), (1920, 1080));

    // But here the original crashed — the reference recorded exactly that.
    assert_eq!(
        want["'abc'/'1080'"].as_str(),
        Some("<ValueError>"),
        "эталон обязан фиксировать падение оригинала"
    );
    assert_eq!(
        wp::mmap_size(Some("abc"), Some("1080")),
        (1920, 1080),
        "мусор должен откатываться к умолчанию, а не ронять запуск"
    );
    assert_eq!(wp::mmap_size(Some("-5"), Some("1e3")), (-5, 1080));
}

// ── video host: decisions taken from `rexon/video_wallpaper.py` ───────────────
//
// The reference is the original's code (`get_qml_file`, `setup_renderer`,
// `_as_bool`, `_as_float01`, the type refusal in `_build_ctx_from_dir`). ⛔ No
// wallpapers were launched, Qt was not started: everything here is pure input → output.

use serde_json::{json, Map};

#[test]
fn video_qml_file_prefers_offscreen() {
    // Offscreen overrides everything: with mmap output there is no window on screen at all.
    assert_eq!(wp::video_qml_file(true, true), wp::QML_OFFSCREEN);
    assert_eq!(wp::video_qml_file(true, false), wp::QML_OFFSCREEN);
    assert_eq!(wp::video_qml_file(false, true), wp::QML_WAYLAND);
    assert_eq!(wp::video_qml_file(false, false), wp::QML_X11);
    // ⚠️ Files with these names no longer exist in the tree: the Python
    // wallpaper host was removed, and the Rust host draws with a single
    // `WallpaperVideo.qml` with three modes. The constants remained as decision
    // TOKENS — `rexon-gui::wallpaper_host::placement` turns them into a display
    // mode. The values are pinned here so that a quiet edit of one side does
    // not drift away from the other.
    assert_eq!(wp::QML_OFFSCREEN, "VideoWallpaperOffscreen.qml");
    assert_eq!(wp::QML_WAYLAND, "VideoWallpaper.qml");
    assert_eq!(wp::QML_X11, "VideoWallpaperX11.qml");
}

#[test]
fn user_prop_bool_matches_original() {
    // Port of `_as_bool`: bool as is, a number by zero, a string by the list.
    assert!(wp::as_bool(Some(&json!(true)), false));
    assert!(!wp::as_bool(Some(&json!(false)), true));
    assert!(wp::as_bool(Some(&json!(1)), false));
    assert!(!wp::as_bool(Some(&json!(0)), true));
    assert!(wp::as_bool(Some(&json!(0.5)), false));
    for yes in ["1", "true", "TRUE", " yes ", "on"] {
        assert!(wp::as_bool(Some(&json!(yes)), false), "{yes:?}");
    }
    // A string outside the list gives False, NOT the default — as in the original.
    for no in ["false", "0", "off", "нет", ""] {
        assert!(!wp::as_bool(Some(&json!(no)), true), "{no:?}");
    }
    // No property / null → default.
    assert!(wp::as_bool(None, true));
    assert!(!wp::as_bool(None, false));
    assert!(wp::as_bool(Some(&Value::Null), true));
}

#[test]
fn user_prop_volume_matches_original() {
    // WE stores the volume in 0..100, QML expects 0..1.
    assert_eq!(wp::as_volume01(Some(&json!(50)), 0.5), 0.5);
    assert_eq!(wp::as_volume01(Some(&json!(100)), 0.5), 1.0);
    assert_eq!(wp::as_volume01(Some(&json!(0.25)), 0.5), 0.25);
    assert_eq!(wp::as_volume01(Some(&json!("75")), 0.5), 0.75);
    // Above a hundred is clamped, a negative one too.
    assert_eq!(wp::as_volume01(Some(&json!(500)), 0.5), 1.0);
    assert_eq!(wp::as_volume01(Some(&json!(-3)), 0.5), 0.0);
    // Unparsed → default.
    assert_eq!(wp::as_volume01(None, 0.42), 0.42);
    assert_eq!(wp::as_volume01(Some(&Value::Null), 0.42), 0.42);
    assert_eq!(wp::as_volume01(Some(&json!("громко")), 0.42), 0.42);
    assert_eq!(wp::as_volume01(Some(&json!([1, 2])), 0.42), 0.42);
}

#[test]
fn video_context_properties_match_setup_renderer() {
    // Empty properties → all of the original's defaults.
    let empty = Map::new();
    let props = wp::video_context_properties("/видео/клип.mp4", false, &empty, 1920, 1080);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "videoUrl",
            "isWindowed",
            "audioEnabled",
            "audioVolume",
            "loopVideo",
            "renderWidth",
            "renderHeight",
        ],
        "имена и порядок свойств контекста"
    );
    assert_eq!(
        props[0].value,
        wp::QmlValue::LocalFileUrl("/видео/клип.mp4".into())
    );
    assert_eq!(props[1].value, wp::QmlValue::Bool(false));
    assert_eq!(props[2].value, wp::QmlValue::Bool(wp::DEFAULT_AUDIO_ENABLED));
    assert_eq!(props[3].value, wp::QmlValue::Real(wp::DEFAULT_VOLUME));
    assert_eq!(props[4].value, wp::QmlValue::Bool(wp::DEFAULT_LOOP));
    assert_eq!(props[5].value, wp::QmlValue::Int(1920));
    assert_eq!(props[6].value, wp::QmlValue::Int(1080));

    // The `defaults_for_js` format is `{key: {"value": …}}`.
    let wrapped: Map<String, Value> = serde_json::from_value(json!({
        "audioenabled": {"value": "false"},
        "volume": {"value": 20},
        "loopvideo": {"value": 0},
    }))
    .unwrap();
    let props = wp::video_context_properties("/x.mp4", true, &wrapped, 800, 600);
    assert_eq!(props[1].value, wp::QmlValue::Bool(true), "isWindowed");
    assert_eq!(props[2].value, wp::QmlValue::Bool(false), "audioEnabled");
    assert_eq!(props[3].value, wp::QmlValue::Real(0.2), "audioVolume");
    assert_eq!(props[4].value, wp::QmlValue::Bool(false), "loopVideo");

    // Flat values (from the configurator) are read the same way.
    let flat: Map<String, Value> = serde_json::from_value(json!({
        "audioenabled": false,
        "volume": 100,
    }))
    .unwrap();
    let props = wp::video_context_properties("/x.mp4", false, &flat, 1, 1);
    assert_eq!(props[2].value, wp::QmlValue::Bool(false));
    assert_eq!(props[3].value, wp::QmlValue::Real(1.0));
}

#[test]
fn video_host_takes_only_video() {
    assert_eq!(wp::video_host_reject_reason("video", "/w/project.json"), None);
    let scene = wp::video_host_reject_reason("scene", "/w/project.json").unwrap();
    assert_eq!(
        scene,
        "Обои типа 'scene' рисует движок waywallen, а не этот хост: /w/project.json"
    );
    // The type was not determined at all — a question mark, as the original printed.
    let unknown = wp::video_host_reject_reason("", "/w/project.json").unwrap();
    assert!(unknown.starts_with("Обои типа '?' рисует"), "{unknown}");
}

/// The list of video extensions is ONE for the whole product: the video host
/// must not have its own (that is exactly how only it knew `.ogv`).
#[test]
fn video_entry_exts_include_ogv() {
    let exts = rexon_core::user_props::VIDEO_ENTRY_EXTS;
    for want in [
        ".mp4", ".webm", ".mkv", ".avi", ".mov", ".m4v", ".ogv", ".wmv", ".mpg", ".mpeg", ".gif",
    ] {
        assert!(exts.contains(&want), "нет расширения {want}");
    }
    assert_eq!(exts.len(), 11);
}

// ── web wallpapers ───────────────────────────────────────────────────────────────
//
// The behaviour reference is the removed `rexon/web_wallpaper.py` (visible in
// git history, commit 2091eac^). Checked against it: the empty property value
// rule, the set of WE stubs, the applyUserProperties wrapper and the Chromium flag.

/// ⚠️ A rule that is easy to lose: a property WITHOUT a value goes into the
/// page as an empty object `{}`, it is not skipped. A whole code branch of the
/// wallpapers themselves depends on it — seeing `{}`, the wallpaper loads its
/// fallback file. Skip it — and wallpapers with an unselected picture stay empty.
#[test]
fn web_defaults_keep_empty_property_as_empty_object() {
    // r##…## delimiters: inside there is a colour with a hash, and a plain r#"
    // would close itself on it.
    let props: serde_json::Map<String, Value> = serde_json::from_str(
        r##"{
            "schemecolor": {"type": "color", "value": "#ff00ff"},
            "картинка":    {"type": "file",  "value": ""},
            "без_значения":{"type": "file"},
            "нуль":        {"type": "file",  "value": null},
            "голое":       7
        }"##,
    )
    .unwrap();
    let out = wp::web_defaults_for_js(&props);

    assert_eq!(out["schemecolor"]["value"], Value::from("#ff00ff"));
    // A bare value from the configurator is wrapped the same way as one from
    // project.json: after that they cannot be told apart.
    assert_eq!(out["голое"]["value"], Value::from(7));
    for empty in ["картинка", "без_значения", "нуль"] {
        assert!(out.contains_key(empty), "свойство {empty} пропало целиком");
        assert_eq!(
            out[empty],
            serde_json::json!({}),
            "{empty} обязано быть пустым объектом, а не отсутствовать"
        );
    }
}

#[test]
fn web_defaults_json_is_valid_and_empty_stays_empty() {
    assert_eq!(wp::web_defaults_json(&serde_json::Map::new()), "{}");
    let props: serde_json::Map<String, Value> =
        serde_json::from_str(r#"{"a": {"value": "б"}}"#).unwrap();
    let json = wp::web_defaults_json(&props);
    // The string goes into the page as a JS literal — it must parse.
    let back: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(back["a"]["value"], Value::from("б"));
}

/// WE stubs are not decoration: a Workshop wallpaper checks for the function
/// SYNCHRONOUSLY while parsing HTML, and without it its script aborts entirely.
#[test]
fn we_polyfill_declares_every_global_the_wallpapers_probe() {
    for name in [
        "wallpaperRegisterAudioListener",
        "wallpaperRegisterMediaStatusListener",
        "wallpaperRegisterMediaPlaybackListener",
        "wallpaperRegisterMediaPropertiesListener",
        "wallpaperRegisterMediaThumbnailListener",
        "wallpaperRegisterMediaTimelineListener",
        "wallpaperRequestRandomFileForProperty",
        "wallpaperPropertyListener",
        "wallpaperPluginListener",
    ] {
        assert!(wp::WE_POLYFILL_JS.contains(name), "нет заглушки {name}");
    }
    // The stub must KEEP the audio listener, not swallow it: the wallpaper
    // redraws the canvas exactly from it.
    assert!(wp::WE_POLYFILL_JS.contains("_rexonAudioListeners.push"));
    // Repeated injection is harmless — the script is also installed into subframes.
    assert!(wp::WE_POLYFILL_JS.contains("__rexonWePolyfillInstalled"));
}

#[test]
fn we_ticker_fires_once_with_full_spectrum() {
    assert!(wp::WE_TICKER_JS.contains("__rexonInitialTickFired"));
    assert!(wp::WE_TICKER_JS.contains("Float32Array(128)"));
    // The tick is deferred: applyUserProperties must get applied first.
    assert!(wp::WE_TICKER_JS.contains("50"));
}

#[test]
fn web_pause_js_uses_page_visibility_and_keeps_the_frame() {
    let js = wp::WEB_PAUSE_JS;
    // Pause — the way the page itself understands it (Page Visibility), plus
    // the main engine of canvases and media.
    for needle in [
        "'hidden'",
        "visibilityState",
        "visibilitychange",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "animation-play-state:paused",
        "AudioContext",
        "suspend()",
        "video,audio",
        "__rexonWasPlaying",
    ] {
        assert!(js.contains(needle), "нет {needle}");
    }
    // Repeated injection is harmless.
    assert!(js.contains("if (window.__rexonWallpaperState) return;"));
    // The window is not hidden: otherwise a battery pause would show a black
    // screen instead of the wallpaper.
    assert!(!js.contains("display:none") && !js.contains("lifecycleState"));
}

#[test]
fn web_state_js_carries_the_decision() {
    assert_eq!(
        wp::web_state_js(true, 0.5),
        "window.__rexonWallpaperState&&window.__rexonWallpaperState(true,0.5);"
    );
    assert!(wp::web_state_js(false, 7.0).contains("(false,1)"), "громкость зажимается");
    assert!(wp::web_state_js(false, -1.0).contains("(false,0)"));
    assert!(wp::web_state_js(false, f64::NAN).contains("(false,1)"), "мусор — не тишина");
}

#[test]
fn web_apply_defaults_js_survives_wallpaper_without_listener() {
    let js = wp::web_apply_defaults_js("{\"a\":{\"value\":1}}");
    // Without try an exception in the injected script would also kill the audio tick.
    assert!(js.contains("try"));
    assert!(js.contains("applyUserProperties"));
    assert!(js.contains("{\"a\":{\"value\":1}}"));
}

/// ⛔ `--disable-gpu` is fatal here: with it Chromium does not composite the
/// canvas into the Qt scene graph, and the offscreen frame would contain only the DOM.
#[test]
fn web_offscreen_flags_keep_gpu() {
    assert_eq!(wp::WEB_OFFSCREEN_CHROMIUM_FLAGS, "--in-process-gpu");
    assert!(!wp::WEB_OFFSCREEN_CHROMIUM_FLAGS.contains("--disable-gpu"));
}

/// A link must not be wrapped in `QUrl::fromLocalFile` — that gives
/// «file:///https:/example.com», i.e. a silently empty page.
#[test]
fn web_target_url_distinguishes_link_from_path() {
    use rexon_core::wallpaper::QmlValue;
    let by_name = |props: Vec<wp::QmlContextProperty>, name: &str| {
        props.iter().find(|p| p.name == name).unwrap().value.clone()
    };

    let link = wp::web_context_properties("https://example.com", false, 1920, 1080);
    assert_eq!(
        by_name(link, "targetUrl"),
        QmlValue::Url("https://example.com".into())
    );
    let file_link = wp::web_context_properties("file:///w/index.html", false, 1, 1);
    assert_eq!(
        by_name(file_link, "targetUrl"),
        QmlValue::Url("file:///w/index.html".into())
    );
    let local = wp::web_context_properties("/w/index.html", true, 640, 480);
    assert_eq!(
        by_name(local.clone(), "targetUrl"),
        QmlValue::LocalFileUrl("/w/index.html".into())
    );
    assert_eq!(by_name(local.clone(), "isWindowed"), QmlValue::Bool(true));
    assert_eq!(by_name(local.clone(), "renderWidth"), QmlValue::Int(640));
    assert_eq!(by_name(local, "renderHeight"), QmlValue::Int(480));
}

#[test]
fn web_host_takes_only_web_type() {
    assert!(wp::web_host_reject_reason("web", "/w/project.json").is_none());
    let video = wp::web_host_reject_reason("video", "/w/project.json").unwrap();
    assert!(video.contains("'video'"), "{video}");
    assert!(video.contains("/w/project.json"), "{video}");
    let unknown = wp::web_host_reject_reason("", "/w/project.json").unwrap();
    assert!(unknown.contains("'?'"), "{unknown}");
}

/// The «this is a link» decision is one for the product: the web host must not
/// have its own copy. There is exactly one difference from
/// `wallpaper_engine::is_url`, and it is intentional — `file://` is a link
/// too, it must not be wrapped again.
#[test]
fn web_url_check_extends_engine_rule_by_file_scheme() {
    for link in ["http://a", "https://a", "file:///a"] {
        assert!(wp::is_web_url(link), "{link}");
    }
    for path in ["/a/index.html", "a.html", ""] {
        assert!(!wp::is_web_url(path), "{path}");
    }
    // Exactly what the engine decides, plus file://.
    assert_eq!(
        wp::is_web_url("http://a"),
        rexon_core::wallpaper_engine::is_url("http://a")
    );
}

// ── installing missing video dependencies ──────────────────────────────────────────
//
// ⛔ None of the commands below was executed: the check only BUILDS argv.

/// The wallpaper engine no longer has its own install table — the shared
/// package engine answers. The former copy knew three families out of eleven
/// and had already drifted from it, so the check goes over ALL values of the
/// enum, not a list of three.
#[test]
fn install_deps_argv_asks_the_package_engine() {
    use rexon_core::system::PkgFamily;
    use rexon_core::wallpaper_engine as we;

    for family in [
        "arch", "debian", "fedora", "suse", "gentoo", "alpine", "void", "nix",
        "solus", "clear", "alt", "unknown",
    ] {
        let fam = PkgFamily::from_value(family).unwrap();
        for session in ["wayland", "x11"] {
            let pkgs = we::install_deps_packages(family, session);
            for avail in [false, true] {
                assert_eq!(
                    we::install_deps_argv(family, &pkgs, avail),
                    rexon_core::system_plan::install_argv(fam, &pkgs, avail),
                    "{family}/{session}: движок обоев ответил своё (helper={avail})"
                );
            }
        }
    }
}

/// Exactly the two commands the former table built WRONG. Written as strings,
/// not derived from the engine: otherwise the reference would drift along with
/// an edit of the table.
///
/// * pacman without `--needed` reinstalled an already installed mpv — an extra
///   download, a repeated hook run and replacing a locally built package with
///   the repository one.
/// * `apt` itself declares its interface unstable for scripts, and without
///   `--no-install-recommends` mpv drags a tail of recommended packages.
#[test]
fn install_deps_argv_fixes_the_two_wrong_commands() {
    use rexon_core::wallpaper_engine as we;

    let arch = we::install_deps_packages("arch", "wayland");
    assert_eq!(arch, vec!["mpv".to_string(), "mpvpaper".to_string()]);
    assert_eq!(
        we::install_deps_argv("arch", &arch, false),
        Some(vec![
            "pkexec".into(), "pacman".into(), "-S".into(), "--needed".into(),
            "--noconfirm".into(), "mpv".into(), "mpvpaper".into(),
        ])
    );
    let debian = we::install_deps_packages("debian", "wayland");
    assert_eq!(
        we::install_deps_argv("debian", &debian, false),
        Some(vec![
            "pkexec".into(), "apt-get".into(), "install".into(), "-y".into(),
            "--no-install-recommends".into(), "mpv".into(),
        ])
    );
}

/// Eight families for which the former table answered «don't know» with a
/// fully known manager: the «Install dependencies» button simply did not work
/// on them. The same class as the openSUSE fix in `youtube_wallpaper` (rule #12).
#[test]
fn install_deps_argv_answers_the_eight_forgotten_families() {
    use rexon_core::wallpaper_engine as we;

    for family in ["suse", "gentoo", "alpine", "void", "nix", "solus", "clear", "alt"] {
        let pkgs = we::install_deps_packages(family, "wayland");
        let argv = we::install_deps_argv(family, &pkgs, false)
            .unwrap_or_else(|| panic!("{family}: движок промолчал"));
        assert!(argv.contains(&"mpv".to_string())
                || argv.contains(&"nixpkgs.mpv".to_string()),
                "{family}: {argv:?}");
    }
}

/// The honest «don't know» stayed in place: an unrecognised family and an
/// empty package list cannot be expressed as a command, the caller tells the
/// person to install by hand.
#[test]
fn install_deps_argv_stays_silent_when_there_is_nothing_to_say() {
    use rexon_core::wallpaper_engine as we;

    let pkgs = we::install_deps_packages("arch", "wayland");
    for family in ["unknown", "windows", "", "ARCH"] {
        assert_eq!(we::install_deps_argv(family, &pkgs, false), None, "{family}");
        assert_eq!(we::install_deps_argv(family, &pkgs, true), None, "{family}");
    }
    assert_eq!(we::install_deps_argv("arch", &[], false), None);
    assert_eq!(we::install_deps_argv("arch", &[], true), None);
}
