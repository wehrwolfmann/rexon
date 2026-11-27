//! Golden check of the `we_parity.rs` port against the original
//! `rexon/qt_wallpaper_engine/parity.py`.
//!
//! The reference was captured by running LIVE Python: for each scenario a real
//! `RendererPluginListResponse` (protobuf from `proto/control_pb2.py`) was
//! built, run through `parity.compare_schema` and `parity.render_report_text`,
//! and `golden/we_parity_cases.json` records the input, the whole report
//! (kind/renderer/key/text/severity of each divergence) and the text dump byte
//! for byte. ⛔ The daemon was not started — the check is pure, on data.
//!
//! The registry, the type table and the set of special keys were also captured
//! from the live module: adding a setting to the UI must break this test until
//! the reference is recaptured.
//!
//! Two records pin an intentional divergence (⚠ rule #16): reading
//! `UPSTREAM_TAG` no longer fails on a broken byte, and the `v` prefix is
//! stripped exactly once (the original's `lstrip("v")` ate them all).
//!
//! ⚠️ On 10.10.2026 the reference was corrected BY HAND, without Python (it is
//! in the archive): settings of scene and web wallpaper renderers that the
//! engine does not have were moved from `stale_in_ui` divergences to
//! `unchecked` («not checked»). The reason and live measurements are in the
//! `absent_plugin_*` and `live_daemon_*` checks below.

use rexon_core::proto::control as pb;
use rexon_core::we_parity as p;
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/we_parity_cases.json")).unwrap()
}

/// Build a protobuf response from the input description in the reference —
/// the same way the Python generator built it.
fn build(input: &Value) -> pb::RendererPluginListResponse {
    let mut resp = pb::RendererPluginListResponse::default();
    for t in input["supported_types"].as_array().unwrap() {
        resp.supported_types.push(t.as_str().unwrap().to_string());
    }
    for r in input["renderers"].as_array().unwrap() {
        let mut ri = pb::RendererPluginInfo {
            name: r["name"].as_str().unwrap().to_string(),
            ..Default::default()
        };
        for s in r["settings"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let mut sc = pb::SettingSchema {
                key: s["key"].as_str().unwrap().to_string(),
                r#type: s["type"].as_i64().unwrap_or(0) as i32,
                ..Default::default()
            };
            if let Some(g) = s["group"].as_str() {
                sc.group = g.to_string();
            }
            if let Some(ch) = s["choices"].as_array() {
                for c in ch {
                    sc.choices.push(c.as_str().unwrap().to_string());
                }
            }
            ri.settings.push(sc);
        }
        resp.renderers.push(ri);
    }
    resp
}

/// The report in the same form in which the Python generator serialised it.
fn report_json(r: &p::ParityReport) -> Value {
    let mut engine_keys = serde_json::Map::new();
    for (name, keys) in &r.engine_keys {
        engine_keys.insert(name.clone(), Value::from(keys.clone()));
    }
    serde_json::json!({
        "supported_types": r.supported_types,
        "renderers": r.renderers,
        "engine_keys": Value::Object(engine_keys),
        "issues": r.issues.iter().map(|i| serde_json::json!({
            "kind": i.kind.as_str(),
            "renderer": i.renderer,
            "key": i.key,
            "detail": i.detail,
            "severity": i.severity(),
        })).collect::<Vec<_>>(),
        "unchecked": r.unchecked.iter().map(|(types, keys)| serde_json::json!({
            "types": types,
            "keys": keys,
        })).collect::<Vec<_>>(),
        "has_drift": r.has_drift(),
        "warning_count": r.warning_count(),
    })
}

#[test]
fn registry_matches_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let want = g["registry"].as_array().unwrap();
    assert_eq!(
        p::REXON_SETTING_REGISTRY.len(),
        want.len(),
        "в реестре UI изменилось число настроек — переснять эталон"
    );
    // Order matters: it defines the order of stale_in_ui divergences.
    for (got, exp) in p::REXON_SETTING_REGISTRY.iter().zip(want) {
        assert_eq!(got.key, exp["key"].as_str().unwrap());
        assert_eq!(
            got.value_type.map(i64::from),
            exp["value_type"].as_i64(),
            "тип для «{}»",
            got.key
        );
        let want_choices: Option<Vec<&str>> = exp["choices"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_str().unwrap()).collect());
        assert_eq!(
            got.choices.map(<[&str]>::to_vec),
            want_choices,
            "choices для «{}»",
            got.key
        );
        assert_eq!(got.note, exp["note"].as_str().unwrap(), "примечание «{}»", got.key);
        let want_types: Vec<&str> = exp["only_types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(got.only_types.to_vec(), want_types, "типы обоев «{}»", got.key);
    }
}

#[test]
fn known_types_and_special_keys_match_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let want = g["known_value_types"].as_object().unwrap();
    assert_eq!(p::KNOWN_VALUE_TYPES.len(), want.len());
    for (num, name) in p::KNOWN_VALUE_TYPES {
        assert_eq!(
            want[&num.to_string()].as_str().unwrap(),
            *name,
            "имя типа {num}"
        );
        assert_eq!(p::value_type_name(*num), Some(*name));
    }
    assert_eq!(p::value_type_name(0), None);
    assert_eq!(p::value_type_name(6), None);

    let mut special: Vec<&str> = p::SPECIAL_KEYS.to_vec();
    special.sort_unstable();
    let want_special: Vec<&str> = g["special_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(special, want_special);
}

#[test]
fn compare_schema_matches_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let cases = g["cases"].as_object().unwrap();
    assert!(cases.len() >= 12, "эталон подозрительно короткий");
    for (name, case) in cases {
        let resp = build(&case["input"]);
        let report = p::compare_schema(&resp);
        assert_eq!(
            report_json(&report),
            case["report"],
            "отчёт разошёлся с Python на сценарии «{name}»"
        );
        assert_eq!(
            p::render_report_text(&report),
            case["text"].as_str().unwrap(),
            "текст отчёта разошёлся с Python на сценарии «{name}»"
        );
    }
}

/// Separately — the most «fragile» spots, so a failure is targeted rather
/// than «somewhere in the scenario».
#[test]
fn quirks_preserved() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let cases = &g["cases"];

    // An unknown type on a registered key does NOT give type_mismatch: the
    // `vt in KNOWN_VALUE_TYPES` guard in the original suppresses it.
    let r = p::compare_schema(&build(&cases["unknown_type_regular"]["input"]));
    assert_eq!(r.issues.len(), 1);
    assert_eq!(r.issues[0].kind, p::IssueKind::UnknownType);
    assert_eq!(r.issues[0].severity(), "warning");

    // Special keys do not complain even about a clearly foreign type.
    let r = p::compare_schema(&build(&cases["unknown_type_special"]["input"]));
    assert!(r.issues.is_empty(), "спец-ключи должны молчать: {:?}", r.issues);

    // An empty choices set on the engine does not trigger the check…
    let r = p::compare_schema(&build(&cases["choices_empty_silent"]["input"]));
    assert!(!r.has_drift());
    // …and order and duplicates do not matter (sets are compared).
    let r = p::compare_schema(&build(&cases["choices_reordered_silent"]["input"]));
    assert!(!r.has_drift());

    // Identical renderer names: both are in the list, the last one in the key map.
    let r = p::compare_schema(&build(&cases["duplicate_renderer_name"]["input"]));
    assert_eq!(r.renderers, vec!["mpv", "mpv"]);
    assert_eq!(r.engine_keys.len(), 1);
    assert_eq!(r.engine_keys_of("mpv"), Some(["volume".to_string()].as_slice()));
    assert_eq!(r.engine_keys_of("weweb"), None);

    // A full schema without divergences and an empty response — the extreme points.
    assert!(!p::compare_schema(&build(&cases["clean_full"]["input"])).has_drift());
    let empty = p::compare_schema(&build(&cases["empty_response"]["input"]));
    // Without a single renderer only the settings of built-in ones count as
    // «missing»; settings of plugin ones (scenes, web) — «not checked».
    let builtin = p::REXON_SETTING_REGISTRY.iter().filter(|e| e.only_types.is_empty()).count();
    assert_eq!(empty.issues.len(), builtin);
    assert_eq!(builtin, 7);
    let unchecked: usize = empty.unchecked.iter().map(|(_, keys)| keys.len()).sum();
    assert_eq!(builtin + unchecked, p::REXON_SETTING_REGISTRY.len());
    assert!(empty.issues.iter().all(|i| i.kind == p::IssueKind::StaleInUi));
    assert_eq!(empty.warning_count(), 0);
}

#[test]
fn upstream_tag_reads_like_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let bytes: Vec<u8> = g["upstream_tag_file"]["bytes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    // The live file rexon/qt_wallpaper_engine/proto/UPSTREAM_TAG, byte for byte.
    assert_eq!(
        p::proto_upstream_tag(Some(&bytes)),
        g["upstream_tag_file"]["value"].as_str().unwrap()
    );
    // No file / empty file — tag unknown.
    assert_eq!(p::proto_upstream_tag(None), "");
    assert_eq!(p::proto_upstream_tag(Some(b"")), "");
    assert_eq!(p::proto_upstream_tag(Some(b"  \n")), "");

    // ⚠ rule #16: the original opened the file as strict UTF-8 and caught only
    // OSError, so a broken byte crashed the whole parity check with a
    // UnicodeDecodeError (the reference recorded this).
    assert_eq!(
        g["read_probe"]["invalid_utf8"].as_str().unwrap(),
        "<UnicodeDecodeError>",
        "эталон обязан фиксировать падение оригинала"
    );
    assert_eq!(p::proto_upstream_tag(Some(b"v0.2.\xff6\n")), "v0.2.\u{fffd}6");
}

#[test]
fn daemon_matches_snapshot_matches_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    // Inputs where the port DELIBERATELY diverges: the original's `lstrip("v")`
    // stripped all leading «v», not the prefix, so «vv0.2.6» was considered equal
    // to «0.2.6». The port strips exactly one «v» → these pairs no longer match.
    let fixed: [(&str, &str); 5] = [
        ("vv0.2.6", "0.2.6"),
        ("vv0.2.6", "v0.2.6"),
        ("vv0.2.6", "  0.2.6 "),
        ("v0.2.6", "vv0.2.6"),
        ("0.2.6", "vv0.2.6"),
    ];
    let mut diverged = 0;
    for case in g["daemon_matches_snapshot"].as_array().unwrap() {
        let tag = case["tag_file"].as_str().unwrap();
        let ver = case["daemon_version"].as_str().unwrap_or("");
        let got = p::daemon_matches_snapshot(tag, ver);
        let want = case["result"].as_bool();
        if fixed.contains(&(tag, ver)) {
            assert_eq!(want, Some(true), "эталон обязан фиксировать квирк оригинала");
            assert_eq!(got, Some(false), "«{tag}» ≠ «{ver}» после починки lstrip");
            diverged += 1;
            continue;
        }
        assert_eq!(got, want, "tag «{tag}» ↔ версия «{ver}»");
    }
    assert_eq!(diverged, fixed.len(), "набор расхождений изменился — пересмотреть эталон");

    // Live path: the tag from the file is already trimmed, the daemon version — with and without «v».
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", "0.2.6"), Some(true));
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", "v0.2.6"), Some(true));
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", "0.2.7"), Some(false));
    // ⚠ rule #16: in the live tag file the upstream commit hash stands next to
    // the version. The original compared the whole string and always reported a
    // divergence — the «✓ schema» badge could never show for any version.
    assert_eq!(p::daemon_matches_snapshot("v0.3.8 (8550258)", "0.3.8"), Some(true));
    assert_eq!(p::daemon_matches_snapshot("v0.3.8 (8550258)", "0.3.7"), Some(false));
    // No brackets — this is part of the version, it must not be cut.
    assert_eq!(p::daemon_matches_snapshot("v0.3.8", "0.3.8 dirty"), Some(false));
    // Nothing to compare with.
    assert_eq!(p::daemon_matches_snapshot("", "0.2.6"), None);
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", ""), None);
    assert_eq!(p::daemon_matches_snapshot("v", "0.2.6"), None);
}

#[test]
fn tag_path_and_read_wrapper() {
    let _lang = i18n_pin::ru();
    let dir = std::path::Path::new("/что-то/qt_wallpaper_engine");
    assert_eq!(
        p::upstream_tag_path(dir),
        std::path::Path::new("/что-то/qt_wallpaper_engine/proto/UPSTREAM_TAG")
    );
    // No directory — empty tag, no panic (like the original's `except OSError`).
    assert_eq!(p::read_proto_upstream_tag(dir), "");
}

/// The snapshot tag is looked up in the TAB directory, not in the data root.
///
/// Guard on the live tree: the tabs crate is given the whole data directory
/// (in the developer tree — `data/`, in the shipped build — `rexon/`), and the
/// tag lies one level below. While the step between them was missing, the
/// report of the «Developer» QML page said «(tag not written)» with a live file
/// on disk (stand snapshot of 06.09.2026).
#[test]
fn tag_is_read_from_the_tab_package_inside_rexon() {
    let _lang = i18n_pin::ru();
    let pkg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rexon_core::installer::DEV_TREE_DATA_DIR)
        .canonicalize()
        .expect("каталог данных data/ есть в дереве");
    let pkg = pkg.to_str().expect("путь дерева в UTF-8");
    // Directly from the data directory there is no file — exactly what was hit.
    assert_eq!(p::read_proto_upstream_tag(std::path::Path::new(pkg)), "");
    // With the step into the tab directory — it is there, and non-empty.
    let tag = p::read_proto_upstream_tag_from_pkg(pkg);
    assert!(!tag.is_empty(), "UPSTREAM_TAG вкладки не прочитался");
    assert!(tag.starts_with('v'), "тег апстрима пишется как «v0.3.7»: {tag}");
}

/// The parity report on the «Developer» page is in English: no scenario of the
/// reference contains a single word of our Russian templates. The check is by
/// words, not letters: keys and groups in the reference are deliberately
/// Russian sometimes (the Russian words for «blur», «Effects») — that is engine
/// data, not our text.
#[test]
fn english_parity_report_has_no_russian() {
    let _lang = i18n_pin::en();
    let g = fx();
    let mut seen_issue = false;
    for (name, case) in g["cases"].as_object().expect("cases") {
        let report = p::compare_schema(&build(&case["input"]));
        seen_issue |= !report.issues.is_empty();
        let text = p::render_report_text(&report);
        for word in [
            "Типы обоев", "Renderer'ы", "Паритет", "Расхождений", "предупреждений",
            "движок умеет", "в реестре", "UI ждёт", "движок отдаёт", "неизвестен UI",
            "известные", "ожидает", "группа", "тип значения", "Не проверено", "сверить",
        ] {
            assert!(!text.contains(word), "{name}: «{word}» в английском отчёте:\n{text}");
        }
    }
    assert!(seen_issue, "в эталоне нет ни одного расхождения — английские детали не проверены");
}

// ─────────────────────────────────────────────────────────────────────────
//  A daemon without the scene and web wallpaper plugin: «not checked», not a divergence
// ─────────────────────────────────────────────────────────────────────────

/// A renderer as the live daemon returns it: name, wallpaper types, keys.
fn renderer(name: &str, types: &[&str], keys: &[&str]) -> pb::RendererPluginInfo {
    pb::RendererPluginInfo {
        name: name.to_string(),
        types: types.iter().map(|t| t.to_string()).collect(),
        settings: keys
            .iter()
            .map(|key| pb::SettingSchema {
                key: key.to_string(),
                // The type is the one the registry expects: PRESENCE is checked here.
                r#type: p::expected_for(key).and_then(|e| e.value_type).unwrap_or(5),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Built-in renderers of the waywallen 0.5.0 daemon (the release `plugin.toml`:
/// since 0.5.0 image and video have `custom_extent`).
fn builtin_renderers() -> Vec<pb::RendererPluginInfo> {
    vec![
        renderer(
            "waywallen-image",
            &["image"],
            &["fps", "resolution", "custom_extent", "render_node"],
        ),
        renderer(
            "waywallen-video",
            &["video"],
            &[
                "loop_file", "hwdec", "render_node", "enable_audio", "volume", "fps", "resolution",
                "custom_extent",
            ],
        ),
    ]
}

/// Renderers of the Open Wallpaper Engine 0.3.1 plugin (the release
/// `plugin.toml`: since 0.3.1 weweb also has `custom_extent`).
fn plugin_renderers() -> Vec<pb::RendererPluginInfo> {
    vec![
        renderer(
            "weweb-renderer",
            &["web"],
            &[
                "enable_audio", "volume", "fps", "resolution", "custom_extent",
                "shared_texture_enabled", "render_node", "remote_debugging_port",
            ],
        ),
        renderer(
            "wescene-renderer",
            &["scene"],
            &[
                "enable_audio", "volume", "fps", "hwdec", "resolution", "custom_extent",
                "msaa", "render_node",
            ],
        ),
    ]
}

fn response(types: &[&str], renderers: Vec<pb::RendererPluginInfo>) -> pb::RendererPluginListResponse {
    pb::RendererPluginListResponse {
        supported_types: types.iter().map(|t| t.to_string()).collect(),
        renderers,
        ..Default::default()
    }
}

/// A production daemon with the plugin: everything matched and everything was checked.
#[test]
fn live_daemon_with_plugin_has_no_differences() {
    let _lang = i18n_pin::ru();
    let mut all = builtin_renderers();
    all.extend(plugin_renderers());
    let r = p::compare_schema(&response(&["image", "scene", "video", "web"], all));
    assert!(r.issues.is_empty(), "расхождения на полном демоне: {:?}", r.issues);
    assert!(r.unchecked.is_empty(), "непроверенное на полном демоне: {:?}", r.unchecked);
    let text = p::render_report_text(&r);
    assert!(text.ends_with("Паритет UI: расхождений не найдено ✅"), "{text}");
}

/// A daemon WITHOUT the plugin (only image/video): four scene and web
/// wallpaper settings used to give «Divergences: 4». There is no one to serve
/// them — this is not a divergence, and the report says in words what exactly
/// was not checked and why.
#[test]
fn absent_plugin_settings_are_unchecked_not_differences() {
    let _lang = i18n_pin::ru();
    let r = p::compare_schema(&response(&["image", "video"], builtin_renderers()));
    assert!(r.issues.is_empty(), "расхождения без плагина: {:?}", r.issues);
    assert!(!r.has_drift());
    // `custom_extent` is served by image/video since 0.5.0 — checked, not in the list.
    let scene = ("scene".to_string(), vec!["msaa".to_string()]);
    let web = (
        "web".to_string(),
        vec!["shared_texture_enabled".to_string(), "remote_debugging_port".to_string()],
    );
    assert_eq!(r.unchecked, vec![scene, web]);
    assert_eq!(
        p::render_report_text(&r),
        "Типы обоев движка: image, video\n\
Renderer'ы: waywallen-image, waywallen-video\n\
Паритет UI: расхождений не найдено ✅\n\
Не проверено: у движка нет renderer'а для обоев «scene», поэтому сверить эти настройки не с чем: msaa\n\
Не проверено: у движка нет renderer'а для обоев «web», поэтому сверить эти настройки не с чем: shared_texture_enabled, remote_debugging_port"
    );
}

/// Bite test: the renderer IS there but does not serve the setting — this is a
/// real divergence, and the leniency for absent ones does not suppress it.
#[test]
fn present_renderer_missing_its_setting_is_still_a_difference() {
    let _lang = i18n_pin::ru();
    // The «web» type is named only by the renderer itself, not in the common list.
    let mut all = builtin_renderers();
    all.push(renderer("weweb-renderer", &["web"], &["fps", "shared_texture_enabled"]));
    let r = p::compare_schema(&response(&["image", "video"], all));
    let stale: Vec<&str> = r.issues.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(stale, vec!["remote_debugging_port"]);
    assert_eq!(r.issues[0].kind, p::IssueKind::StaleInUi);
    // There are still no scenes — their settings are not checked.
    assert_eq!(r.unchecked.len(), 1);
    assert_eq!(r.unchecked[0].0, "scene");

    // The type is named in the common list, but there is no renderer with
    // settings — also a divergence: the engine declared the type, someone can be asked.
    // Since 0.5.0 `custom_extent` is served by image/video — three divergences, not four.
    let r = p::compare_schema(&response(&["image", "video", "scene", "web"], builtin_renderers()));
    assert_eq!(r.issues.len(), 3);
    assert!(r.unchecked.is_empty());
}
