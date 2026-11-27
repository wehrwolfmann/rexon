//! Golden check of the `rxn_build.rs` port against the original
//! `rexon/scripts/build_rxn_v4.py`.
//!
//! The golden was captured by a REAL build: the live `build()` was run on temporary
//! files, and `manifest.json` and the list of names were read from the resulting archives.
//! Covered: the full set (three resolutions, two tracks, preview, description),
//! the minimal one (one video), and all four error codes — no video, video not
//! found, a malformed track specification, track not found.
//!
//! `golden/rxn_build_cases.json` — the captured values.

use rexon_core::rxn_build as rxn;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/rxn_build_cases.json")).unwrap()
}

/// The `plan` branch of the same golden was captured by a REPEATED run of the live `build()`
/// now over the whole build process: rc, stdout, stderr, the list of names in the archive and the manifest
/// text for each case. The bridge was off during capture
/// (the Python path before its removal), the golden is the pure Python 3.14 path.
fn plan_fx() -> Value {
    fx()["plan"].clone()
}

/// The golden's file system: exactly the files that the golden generator created
/// exist.
struct GoldenIo {
    existing: Vec<String>,
}

impl GoldenIo {
    fn new() -> Self {
        let g = plan_fx();
        let existing = g["_sources"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        Self { existing }
    }
}

impl rxn::RxnBuildIo for GoldenIo {
    fn is_file(&mut self, path: &str) -> bool {
        self.existing.iter().any(|p| p == path)
    }
}

fn src(key: &str) -> String {
    plan_fx()["_sources"][key].as_str().unwrap().to_string()
}

fn missing(name: &str) -> String {
    format!("{}/{name}", plan_fx()["_tmp"].as_str().unwrap())
}

fn args_of(videos: &[(&str, String)], audio: &[String]) -> rxn::BuildArgs {
    rxn::BuildArgs {
        title: "Ведьмак 3".into(),
        default_locale: "en".into(),
        volume: 50.0,
        videos: videos
            .iter()
            .map(|(r, s)| ((*r).to_string(), s.clone()))
            .collect(),
        audio: audio.to_vec(),
        ..Default::default()
    }
}

/// Run the plan and return it if the build took place.
fn plan_ok(args: &rxn::BuildArgs) -> rxn::BuildPlan {
    match rxn::plan_build(args, &mut GoldenIo::new()) {
        rxn::BuildOutcome::Ready(plan) => *plan,
        other => panic!("ожидался план, получено {other:?}"),
    }
}

/// The stdout the executor would print for this plan. It is assembled from exactly
/// the same pieces the original printed: the header, a line for each file,
/// the result with the size.
fn stdout_of(plan: &rxn::BuildPlan, out_name: &str, out_path: &str, size: u64) -> String {
    let mut s = format!("{}\n", rxn::msg_start(out_name));
    for e in &plan.entries {
        if !e.line.is_empty() {
            s.push_str(&e.line);
            s.push('\n');
        }
    }
    s.push_str(&rxn::msg_done(out_path, size));
    s.push('\n');
    s
}

/// The plan's stderr: warnings, each on its own line.
fn stderr_of(plan: &rxn::BuildPlan) -> String {
    plan.warnings
        .iter()
        .map(|w| format!("{w}\n"))
        .collect::<String>()
}

/// Names in the archive: the manifest first, then the plan's entries.
fn names_of(plan: &rxn::BuildPlan) -> Vec<String> {
    let mut names = vec!["manifest.json".to_string()];
    names.extend(plan.entries.iter().map(|e| e.archive_path.clone()));
    names
}

fn want_names(case: &Value) -> Vec<String> {
    case["names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

#[test]
fn audio_ext_table_matches_original() {
    let g = fx();
    let want: Vec<&str> = g["audio_exts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(rxn::AUDIO_EXTS.to_vec(), want, "набор расширений звука");
    assert!(rxn::audio_ext_known(".mp3"));
    assert!(rxn::audio_ext_known(".MP3"), "регистр не важен");
    assert!(!rxn::audio_ext_known(".exotic"));
}

#[test]
fn archive_paths() {
    assert_eq!(rxn::video_archive_path("2160p"), "data/video/2160p.mp4");
    assert_eq!(rxn::audio_archive_path("ru", ".ogg"), "data/audio/ru.ogg");
    // The extension is kept as is, even an unknown one.
    assert_eq!(
        rxn::audio_archive_path("de", ".exotic"),
        "data/audio/de.exotic"
    );
}

/// The FIRST equals sign counts as the separator: it occurs legitimately in a path,
/// and splitting on the last one would break such paths.
#[test]
fn audio_spec_parsing() {
    assert_eq!(
        rxn::parse_audio_spec("ru=/путь/к/ru.mp3"),
        Some(rxn::AudioSpec {
            lang: "ru".into(),
            path: "/путь/к/ru.mp3".into()
        })
    );
    assert_eq!(
        rxn::parse_audio_spec("en=/tmp/a=b/en.mp3").map(|s| s.path),
        Some("/tmp/a=b/en.mp3".to_string())
    );
    assert_eq!(rxn::parse_audio_spec("простослово"), None);
    assert_eq!(
        rxn::parse_audio_spec("=/только/путь").map(|s| s.lang),
        Some(String::new())
    );
}

#[test]
fn exit_codes_and_messages_match_original() {
    let g = fx();
    assert_eq!(g["no_video"]["rc"].as_i64(), Some(rxn::EXIT_NO_VIDEO as i64));
    assert_eq!(
        g["missing_video"]["rc"].as_i64(),
        Some(rxn::EXIT_VIDEO_NOT_FOUND as i64)
    );
    assert_eq!(
        g["bad_audio_spec"]["rc"].as_i64(),
        Some(rxn::EXIT_BAD_AUDIO_SPEC as i64)
    );
    assert_eq!(
        g["missing_audio"]["rc"].as_i64(),
        Some(rxn::EXIT_AUDIO_NOT_FOUND as i64)
    );

    // The message texts are verbatim the same as the original printed.
    assert_eq!(g["no_video"]["stderr"].as_str(), Some(rxn::MSG_NO_VIDEO));
    assert_eq!(
        g["bad_audio_spec"]["stderr"].as_str(),
        Some(rxn::msg_bad_audio_spec("простослово").as_str())
    );
    let missing_video = g["missing_video"]["stderr"].as_str().unwrap();
    let path = missing_video
        .strip_prefix("❌ video file not found: ")
        .expect("формат сообщения изменился");
    assert_eq!(rxn::msg_video_not_found(path), missing_video);
    let missing_audio = g["missing_audio"]["stderr"].as_str().unwrap();
    let path = missing_audio
        .strip_prefix("❌ audio file not found: ")
        .expect("формат сообщения изменился");
    assert_eq!(rxn::msg_audio_not_found(path), missing_audio);
}

#[test]
fn manifest_minimal_byte_for_byte() {
    let g = fx();
    let input = rxn::ManifestInput {
        title: "Ведьмак 3".into(),
        description: String::new(),
        author: String::new(),
        has_preview: false,
        videos: pairs(&[("2160p", "data/video/2160p.mp4")]),
        audio_tracks: Vec::new(),
        default_locale: "en".into(),
        audio_volume_default: 0.5,
    };
    assert_eq!(
        rxn::manifest_json(&input),
        g["minimal"]["manifest"].as_str().unwrap()
    );
}

#[test]
fn manifest_full_byte_for_byte() {
    let g = fx();
    let input = rxn::ManifestInput {
        title: "Ведьмак 3".into(),
        description: "Описание".into(),
        author: "Автор".into(),
        has_preview: true,
        // The order of resolutions follows the table, not the command line.
        videos: pairs(&[
            ("720p", "data/video/720p.mp4"),
            ("1080p", "data/video/1080p.mp4"),
            ("2160p", "data/video/2160p.mp4"),
        ]),
        audio_tracks: pairs(&[("en", "data/audio/en.mp3"), ("ru", "data/audio/ru.ogg")]),
        default_locale: "en".into(),
        audio_volume_default: 0.5,
    };
    assert_eq!(
        rxn::manifest_json(&input),
        g["full"]["manifest"].as_str().unwrap()
    );
}

/// The fallback video is `1080p` if it exists; otherwise the first by the order of
/// resolutions, not by the order of arguments.
#[test]
fn fallbacks() {
    let with_1080 = pairs(&[
        ("720p", "data/video/720p.mp4"),
        ("1080p", "data/video/1080p.mp4"),
    ]);
    assert_eq!(
        rxn::fallback_video(&with_1080),
        Some("data/video/1080p.mp4")
    );
    let without = pairs(&[
        ("720p", "data/video/720p.mp4"),
        ("2160p", "data/video/2160p.mp4"),
    ]);
    assert_eq!(rxn::fallback_video(&without), Some("data/video/720p.mp4"));
    assert_eq!(rxn::fallback_video(&[]), None);

    let tracks = pairs(&[("ru", "data/audio/ru.ogg"), ("en", "data/audio/en.mp3")]);
    assert_eq!(rxn::fallback_audio(&tracks, "en"), Some("data/audio/en.mp3"));
    // There is no default language — the first given track is taken.
    assert_eq!(rxn::fallback_audio(&tracks, "de"), Some("data/audio/ru.ogg"));
    assert_eq!(rxn::fallback_audio(&[], "en"), None);
}

/// The absence of a preview and tracks means the ABSENCE of the keys, not `null`:
/// the original dropped keys with a None value.
#[test]
fn absent_keys_are_dropped() {
    let g = fx();
    let minimal = g["minimal"]["manifest"].as_str().unwrap();
    assert!(!minimal.contains("preview"), "ключ превью не должен появляться");
    assert!(
        !minimal.contains("fallback_audio"),
        "ключ запасной дорожки не должен появляться"
    );
    assert!(!minimal.contains("null"), "null-значений быть не должно");

    // The default language is absent among the tracks — there is still a fallback.
    let locale_miss = g["locale_miss"]["manifest"].as_str().unwrap();
    assert!(locale_miss.contains("\"fallback_audio\": \"data/audio/ru.ogg\""));
}

// ── The whole build process ─────────────────────────────────────────────

/// ⚠️ FIXED (rule #16). An ordinary call from the command line: `--volume`
/// is declared an integer, and the golden writes `"audio_volume_default": 50`. Through the bridge
/// the same gave `50.0` — the manifest differed from the golden byte-wise on
/// every build. This case catches the fix.
#[test]
fn cli_default_volume_stays_integer() {
    let g = plan_fx();
    let args = args_of(&[("1080p", src("v1080"))], &[]);
    let plan = plan_ok(&args);
    assert_eq!(
        plan.manifest_json,
        g["cli_default_volume"]["manifest"].as_str().unwrap(),
        "манифест при громкости по умолчанию"
    );
    assert!(plan.manifest_json.contains("\"audio_volume_default\": 50,"));
    assert!(
        !plan.manifest_json.contains("50.0"),
        "целая громкость не должна становиться дробной"
    );
}

/// A fractional volume we leave fractional — it is reachable too.
#[test]
fn float_volume_stays_float() {
    let g = plan_fx();
    let mut args = args_of(&[("1080p", src("v1080"))], &[]);
    args.volume = 0.5;
    assert_eq!(
        plan_ok(&args).manifest_json,
        g["float_volume"]["manifest"].as_str().unwrap()
    );
}

/// The full set: preview, three resolutions, two tracks. The resolutions are passed
/// NOT in the table's order — in the archive and the manifest they must line up by the table.
#[test]
fn full_build_matches_original() {
    let g = plan_fx();
    let case = &g["full"];
    let mut args = args_of(
        &[
            ("2160p", src("v2160")),
            ("720p", src("v720")),
            ("1080p", src("v1080")),
        ],
        &[
            format!("en={}", src("a_en")),
            format!("ru={}", src("a_ru")),
        ],
    );
    args.description = "Описание".into();
    args.author = "Автор".into();
    args.preview = Some(src("prev"));
    args.volume = 70.0;

    let plan = plan_ok(&args);
    assert_eq!(plan.manifest_json, case["manifest"].as_str().unwrap());
    assert_eq!(names_of(&plan), want_names(case), "порядок имён в архиве");
    assert_eq!(stderr_of(&plan), case["stderr"].as_str().unwrap());
    assert_eq!(
        stdout_of(&plan, "b.rxn", &format!("{}/b.rxn", g["_tmp"].as_str().unwrap()), 3 * 1024 * 1024),
        case["stdout"].as_str().unwrap(),
        "печать: заголовок, строки файлов, итог с размером"
    );
}

/// An unknown sound extension — a warning verbatim, but the archive is built.
#[test]
fn unknown_audio_ext_warns_but_builds() {
    let g = plan_fx();
    let case = &g["unknown_ext"];
    let args = args_of(
        &[("1080p", src("v1080"))],
        &[format!("de={}", src("a_de"))],
    );
    let plan = plan_ok(&args);
    assert_eq!(stderr_of(&plan), case["stderr"].as_str().unwrap());
    assert_eq!(names_of(&plan), want_names(case));
    assert_eq!(plan.manifest_json, case["manifest"].as_str().unwrap());
}

/// A name with a trailing dot: the extension is the dot itself (Python 3.14).
#[test]
fn trailing_dot_suffix_matches_original() {
    let g = plan_fx();
    let case = &g["trailing_dot"];
    let args = args_of(
        &[("1080p", src("v1080"))],
        &[format!("xx={}", src("a_dot"))],
    );
    let plan = plan_ok(&args);
    assert_eq!(names_of(&plan), want_names(case), "путь дорожки в архиве");
    assert_eq!(plan.manifest_json, case["manifest"].as_str().unwrap());
    assert_eq!(stderr_of(&plan), case["stderr"].as_str().unwrap());
}

/// One language twice with DIFFERENT extensions: the names in the archive do not collide, both
/// files fit, the manifest refers to the last one. ⚠️ ADDED
/// (rule #16) — the original was silent, we warn, so the golden's stderr is
/// empty, while ours is not; everything else matches.
#[test]
fn duplicate_language_keeps_both_files() {
    let g = plan_fx();
    let case = &g["dup_lang"];
    let args = args_of(
        &[("1080p", src("v1080"))],
        &[
            format!("ru={}", src("a_ru")),
            format!("ru={}", src("a_en")),
        ],
    );
    let plan = plan_ok(&args);
    assert_eq!(names_of(&plan), want_names(case));
    assert_eq!(plan.manifest_json, case["manifest"].as_str().unwrap());
    assert_eq!(case["stderr"].as_str(), Some(""), "оригинал молчал");
    assert_eq!(
        plan.warnings,
        vec![rxn::msg_duplicate_lang(
            "ru",
            "data/audio/ru.ogg",
            "data/audio/ru.mp3"
        )]
    );
}

/// A repeated language does NOT move its place in the manifest: a Python dict updates
/// the value, leaving the key where it first appeared.
#[test]
fn duplicate_language_keeps_key_order() {
    let g = plan_fx();
    let case = &g["dup_lang_order"];
    let args = args_of(
        &[("1080p", src("v1080"))],
        &[
            format!("ru={}", src("a_ru")),
            format!("en={}", src("a_en")),
            format!("ru={}", src("a_en")),
        ],
    );
    let plan = plan_ok(&args);
    assert_eq!(plan.manifest_json, case["manifest"].as_str().unwrap());
    assert_eq!(names_of(&plan), want_names(case));
}

/// ⭐ One language twice with ONE extension: there is one place in the archive.
///
/// ⚠️ FIXED (rule #16). The original put TWO entries under this name and
/// kept silent: unpacking kept the last one, the first track was lost — the package
/// author learned about the loss from the buyer. Now there is one entry, the very one
/// the manifest refers to, and the dropped one is mentioned out loud. The case deliberately has
/// no golden: the former behaviour is not a golden here but a defect. Not
/// a single former case covered it — all language repeats in the set had
/// different extensions, where the names in the archive do not collide.
#[test]
fn duplicate_language_same_extension_keeps_one_entry() {
    let mut io = GoldenIo::new();
    let second = format!("{}/src/ru2.mp3", plan_fx()["_tmp"].as_str().unwrap());
    io.existing.push(second.clone());

    let args = args_of(
        &[("1080p", src("v1080"))],
        &[format!("ru={}", src("a_en")), format!("ru={second}")],
    );
    let plan = match rxn::plan_build(&args, &mut io) {
        rxn::BuildOutcome::Ready(plan) => *plan,
        other => panic!("ожидался план, получено {other:?}"),
    };

    let want: Vec<String> = ["manifest.json", "data/video/1080p.mp4", "data/audio/ru.mp3"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(
        names_of(&plan),
        want,
        "две записи одного имени — это потеря данных, а не «мёртвый груз»"
    );
    assert_eq!(
        plan.entries.last().unwrap().source,
        second,
        "в архив идёт та дорожка, на которую ссылается манифест"
    );
    assert_eq!(
        plan.warnings,
        vec![rxn::msg_overwritten_lang("ru", "data/audio/ru.mp3")]
    );
    assert!(plan
        .manifest_json
        .contains("\"ru\": \"data/audio/ru.mp3\""));
}

/// The default language does not occur among the tracks.
#[test]
fn locale_miss_plan() {
    let g = plan_fx();
    let case = &g["locale_miss"];
    let mut args = args_of(
        &[("1080p", src("v1080"))],
        &[format!("ru={}", src("a_ru"))],
    );
    args.default_locale = "de".into();
    let plan = plan_ok(&args);
    assert_eq!(plan.manifest_json, case["manifest"].as_str().unwrap());
}

/// All four refusals: the code and the text — verbatim as in the original.
#[test]
fn failures_match_original() {
    let g = plan_fx();
    let io = &mut GoldenIo::new();

    let cases: Vec<(&str, rxn::BuildArgs)> = vec![
        ("no_video", args_of(&[], &[])),
        (
            "missing_video",
            args_of(&[("1080p", missing("нет.mp4"))], &[]),
        ),
        (
            "bad_audio_spec",
            args_of(&[("1080p", src("v1080"))], &["простослово".to_string()]),
        ),
        (
            "missing_audio",
            args_of(
                &[("1080p", src("v1080"))],
                &[format!("ru={}", missing("нет.mp3"))],
            ),
        ),
        // The order: video is checked before the tracks.
        (
            "video_beats_audio",
            args_of(
                &[("1080p", missing("нет.mp4"))],
                &["простослово".to_string()],
            ),
        ),
        // And before that — "no video at all".
        (
            "no_video_beats_audio",
            args_of(&[], &["простослово".to_string()]),
        ),
    ];

    for (name, args) in cases {
        let case = &g[name];
        match rxn::plan_build(&args, io) {
            rxn::BuildOutcome::Failed { code, message } => {
                assert_eq!(
                    i64::from(code),
                    case["rc"].as_i64().unwrap(),
                    "код возврата случая {name}"
                );
                assert_eq!(
                    format!("{message}\n"),
                    case["stderr"].as_str().unwrap(),
                    "сообщение случая {name}"
                );
            }
            other => panic!("случай {name}: ожидался отказ, получено {other:?}"),
        }
    }
}

/// The preview is put into the archive, but NOT printed about — that is how the original did it.
#[test]
fn preview_is_silent() {
    let mut args = args_of(&[("1080p", src("v1080"))], &[]);
    args.preview = Some(src("prev"));
    let plan = plan_ok(&args);
    let preview = plan
        .entries
        .iter()
        .find(|e| e.archive_path == "preview.png")
        .expect("превью должно попасть в архив");
    assert_eq!(preview.line, "", "о превью оригинал не печатал");
    assert_eq!(plan.entries[0].archive_path, "preview.png", "превью первым");
}

/// The result line: the path and the size with one digit after the decimal point.
#[test]
fn done_line_format() {
    assert_eq!(
        rxn::msg_done("/tmp/a.rxn", 3 * 1024 * 1024),
        "✅ /tmp/a.rxn (3.0 MB)"
    );
    assert_eq!(rxn::msg_done("/tmp/a.rxn", 0), "✅ /tmp/a.rxn (0.0 MB)");
    assert_eq!(rxn::msg_start("a.rxn"), "📦 Сборка a.rxn…");
}
