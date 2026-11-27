//! Golden check of the `user_props.rs` port against the original
//! `rexon/wallpapers/user_props.py`. ⚠️ The module itself was removed from the
//! tree on 2026-08-23 together with the Python wallpaper host: the
//! observations recorded below are the only remaining record of its behaviour.
//!
//! The reference was captured by running LIVE Python: each `project.json` from
//! the set was written to a temporary file, fed to `parse_project_json`, and
//! `golden/user_props_cases.json` holds the file text itself, the result
//! (`dataclasses.asdict` in full, including every property) and
//! `defaults_for_js` — or the exception TYPE AND TEXT if the original crashed.
//! ⛔ No wallpapers were launched, only temporary files were read.
//!
//! The set hits real corners of the WE Workshop: a capitalised `type`, a
//! numeric `workshopid`, a property without `type`, `color.value` of three
//! floats, CJK in the title, `options` mixed with garbage, and also all places
//! where the «tolerant» parser of the original actually crashed or produced
//! garbage.
//!
//! Scenarios fall into two groups:
//!   * [`MATCHING`] — the port must match Python down to the last field;
//!   * the rest — divergences BY RULE #16: the test checks both that the
//!     original really broke in the reference and that the port gives a
//!     meaningful result. Such a reference cannot be passed by «any»
//!     implementation: it simultaneously pins the old behaviour and demands
//!     the new one.

use rexon_core::user_props as up;
use serde_json::{json, Map, Value};

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/user_props_cases.json")).unwrap()
}

/// Scenarios where the port matches the original byte for byte.
const MATCHING: &[&str] = &[
    "full_web",
    "missing_type",
    "empty_object",
    "order_variants",
    "general_null",
    "audio_string_empty",
    "audio_zero",
    "prop_not_dict",
    "workshopid_float",
    "workshopid_string",
    "numeric_strings",
    "defaults_edge",
    "upper_types",
];

/// A property in the same form as `dataclasses.asdict` in the reference generator.
fn prop_json(p: &up::UserProperty) -> Value {
    json!({
        "name": p.name,
        "type": p.r#type,
        "value": p.value.clone().unwrap_or(Value::Null),
        "text": p.text,
        "order": p.order,
        "index": p.index,
        "condition": p.condition,
        "min": p.min.clone().unwrap_or(Value::Null),
        "max": p.max.clone().unwrap_or(Value::Null),
        "step": p.step.clone().unwrap_or(Value::Null),
        "precision": p.precision.clone().unwrap_or(Value::Null),
        "fraction": p.fraction.clone().unwrap_or(Value::Null),
        "options": p.options.iter().map(|o| json!({
            "label": o.label,
            "value": o.value,
        })).collect::<Vec<_>>(),
        "mode": p.mode,
        "file_type": p.file_type,
        "extra": Value::Object(p.extra.clone()),
    })
}

/// A project in `dataclasses.asdict` form.
fn project_json(p: &up::Project) -> Value {
    let mut props = Map::new();
    for (name, prop) in &p.properties {
        props.insert(name.clone(), prop_json(prop));
    }
    let mut loc = Map::new();
    for (lang, rows) in &p.localization {
        let mut table = Map::new();
        for (k, v) in rows {
            table.insert(k.clone(), Value::from(v.clone()));
        }
        loc.insert(lang.clone(), Value::Object(table));
    }
    json!({
        "type": p.r#type,
        "file": p.file,
        "title": p.title,
        "description": p.description,
        "preview": p.preview,
        "tags": p.tags,
        "content_rating": p.content_rating,
        "visibility": p.visibility,
        "workshop_id": p.workshop_id,
        "workshop_url": p.workshop_url,
        "version": p.version,
        "properties": Value::Object(props),
        "supports_audio": p.supports_audio,
        "localization": Value::Object(loc),
        "extra": Value::Object(p.extra.clone()),
    })
}

fn parse(case: &Value) -> Result<up::Project, up::ParseError> {
    up::parse_project_text(case["json"].as_str().unwrap()).map(|(p, _)| p)
}

#[test]
fn matching_cases_are_identical_to_python() {
    let g = fx();
    for name in MATCHING {
        let case = &g["cases"][*name];
        assert!(
            case["exception"].is_null(),
            "сценарий «{name}» помечен как совпадающий, но Python на нём падал"
        );
        let project = parse(case)
            .unwrap_or_else(|e| panic!("сценарий «{name}» не разобрался: {e}"));
        assert_eq!(
            project_json(&project),
            case["project"],
            "разбор разошёлся с Python на сценарии «{name}»"
        );
        assert_eq!(
            Value::Object(project.defaults_for_js()),
            case["defaults_for_js"],
            "defaults_for_js разошёлся с Python на сценарии «{name}»"
        );
    }
}

/// The reference must pin the original behaviour — otherwise the divergence
/// check below proves nothing.
fn assert_python_raised(g: &Value, name: &str, exc: &str) {
    assert_eq!(
        g["cases"][name]["exception"]["type"].as_str(),
        Some(exc),
        "эталон обязан фиксировать падение оригинала на «{name}»"
    );
}

#[test]
fn null_no_longer_becomes_the_word_none() {
    // ⚠ rule #16 (1): str(None) gave the string «None» in the preview file name,
    // the wallpaper type and the Workshop id.
    let g = fx();
    let py = &g["cases"]["nulls_everywhere"]["project"];
    assert_eq!(py["preview"].as_str(), Some("None"));
    assert_eq!(py["type"].as_str(), Some("none"));
    assert_eq!(py["workshop_id"].as_str(), Some("None"));
    assert_eq!(py["properties"]["p"]["mode"].as_str(), Some("none"));

    let p = parse(&g["cases"]["nulls_everywhere"]).unwrap();
    assert_eq!(p.r#type, "");
    assert_eq!(p.file, "");
    assert_eq!(p.title, "");
    assert_eq!(p.description, "");
    assert_eq!(p.preview, "preview.jpg", "должно вернуться умолчание");
    assert_eq!(p.content_rating, "");
    assert_eq!(p.visibility, "public", "должно вернуться умолчание");
    assert_eq!(p.workshop_id, None);
    assert_eq!(p.workshop_url, None);
    assert_eq!(p.version, None);
    let prop = p.property("p").unwrap();
    assert_eq!(prop.r#type, "label");
    assert_eq!(prop.text, "");
    assert_eq!(prop.condition, "");
    assert_eq!(prop.mode, None);
    assert_eq!(prop.file_type, None);
}

#[test]
fn broken_order_no_longer_kills_the_whole_file() {
    // ⚠ rule #16 (2): because of int() on one malformed property the wallpaper
    // was left without any user settings at all.
    let g = fx();
    assert_python_raised(&g, "order_garbage", "ValueError");
    assert_python_raised(&g, "index_garbage", "ValueError");

    let p = parse(&g["cases"]["order_garbage"]).unwrap();
    assert_eq!(p.property("p").unwrap().order, 0);
    assert_eq!(p.r#type, "scene", "остальные поля обязаны уцелеть");
    let p = parse(&g["cases"]["index_garbage"]).unwrap();
    assert_eq!(p.property("p").unwrap().index, 3, "«3.5» усекается, как int()");
}

#[test]
fn non_dict_containers_no_longer_crash() {
    // ⚠ rule #16 (3): AttributeError on a non-dict.
    let g = fx();
    for (name, exc) in [
        ("general_not_dict", "AttributeError"),
        ("properties_not_dict", "AttributeError"),
        ("top_level_list", "AttributeError"),
        ("top_level_string", "AttributeError"),
    ] {
        assert_python_raised(&g, name, exc);
    }

    // general / properties not an object — just empty, the rest is read.
    let p = parse(&g["cases"]["general_not_dict"]).unwrap();
    assert_eq!(p.r#type, "web");
    assert!(p.properties.is_empty());
    assert!(!p.supports_audio);
    let p = parse(&g["cases"]["properties_not_dict"]).unwrap();
    assert!(p.properties.is_empty());

    // Top level not an object — an honest typed error, not a panic.
    assert_eq!(
        parse(&g["cases"]["top_level_list"]),
        Err(up::ParseError::NotObject("массив"))
    );
    assert_eq!(
        parse(&g["cases"]["top_level_string"]),
        Err(up::ParseError::NotObject("строка"))
    );
}

#[test]
fn tags_are_taken_only_from_arrays() {
    // ⚠ rule #16 (4): a string was iterated by characters, a dict by keys.
    let g = fx();
    assert_eq!(
        g["cases"]["tags_string"]["project"]["tags"],
        json!(["a", "b", "c"])
    );
    assert_eq!(g["cases"]["tags_dict"]["project"]["tags"], json!(["a", "b"]));

    assert!(parse(&g["cases"]["tags_string"]).unwrap().tags.is_empty());
    assert!(parse(&g["cases"]["tags_dict"]).unwrap().tags.is_empty());
    // An array is still parsed, numbers are turned into strings.
    let p = parse(&g["cases"]["full_web"]).unwrap();
    assert_eq!(p.tags, vec!["Nature".to_string(), "42".to_string()]);
}

#[test]
fn string_false_no_longer_enables_audio() {
    // ⚠ rule #16 (5): bool("false") in Python is true.
    let g = fx();
    assert_eq!(
        g["cases"]["audio_string_false"]["project"]["supports_audio"],
        json!(true),
        "эталон обязан фиксировать ошибку оригинала"
    );
    assert!(!parse(&g["cases"]["audio_string_false"]).unwrap().supports_audio);
    // Values where the original was right anyway did not break.
    assert!(!parse(&g["cases"]["audio_string_empty"]).unwrap().supports_audio);
    assert!(!parse(&g["cases"]["audio_zero"]).unwrap().supports_audio);
    assert!(parse(&g["cases"]["full_web"]).unwrap().supports_audio);
}

#[test]
fn version_and_localization_match_their_declared_types() {
    // ⚠ rule #16 (6) and (7): fields went to the calling code «as is».
    let g = fx();
    assert_eq!(g["cases"]["version_string"]["project"]["version"], json!("3"));
    assert_eq!(g["cases"]["version_float"]["project"]["version"], json!(3.9));
    assert_eq!(parse(&g["cases"]["version_string"]).unwrap().version, Some(3));
    assert_eq!(parse(&g["cases"]["version_float"]).unwrap().version, Some(3));

    assert_eq!(
        g["cases"]["localization_not_dict"]["project"]["localization"],
        json!([1, 2])
    );
    assert!(parse(&g["cases"]["localization_not_dict"])
        .unwrap()
        .localization
        .is_empty());
    // A normal localisation is read.
    let p = parse(&g["cases"]["full_web"]).unwrap();
    assert_eq!(p.localization.len(), 1);
    assert_eq!(p.localization[0].0, "ru-ru");
    assert_eq!(
        p.localization[0].1,
        vec![("ui_props_speed".to_string(), "Скорость".to_string())]
    );
}

#[test]
fn combo_option_label_is_empty_not_none() {
    // ⚠ rule #16 (1) in the option list: the label «None» reached the UI.
    let g = fx();
    assert_eq!(
        g["cases"]["options_shapes"]["project"]["properties"]["r"]["options"][0]["label"],
        json!("None")
    );
    let p = parse(&g["cases"]["options_shapes"]).unwrap();
    let r = p.property("r").unwrap();
    assert_eq!(r.options.len(), 1);
    assert_eq!(r.options[0].label, "");
    assert_eq!(r.options[0].value, Value::Null);
    // options not an array — no options (as in the original).
    assert!(p.property("p").unwrap().options.is_empty());
    assert!(p.property("q").unwrap().options.is_empty());
}

#[test]
fn errors_are_typed_not_thrown() {
    let g = fx();
    // Broken JSON: both error out, but the port's error is typed.
    assert_python_raised(&g, "broken_json", "JSONDecodeError");
    assert!(matches!(
        parse(&g["cases"]["broken_json"]),
        Err(up::ParseError::NotJson(_))
    ));
    // No file.
    assert_python_raised(&g, "missing_file", "FileNotFoundError");
    let missing = std::path::Path::new("/такого/каталога/нет/project.json");
    assert!(matches!(
        up::parse_project_json(missing),
        Err(up::ParseError::Io(_))
    ));

    // ⚠ rule #16 (8): a non-UTF-8 file is no longer lost entirely.
    assert_python_raised(&g, "invalid_utf8", "UnicodeDecodeError");
    let (p, _) = up::parse_project_bytes(b"{\"type\": \"w\xffeb\", \"file\": \"i.html\"}")
        .expect("битый байт не должен ронять разбор");
    assert_eq!(p.r#type, "w\u{fffd}eb");
    assert_eq!(p.file, "i.html", "остальные поля обязаны уцелеть");
}

#[test]
fn warning_on_missing_type_like_original() {
    let g = fx();
    // The original wrote log.warning and went on; the port returns the same
    // warning to the caller.
    let (_, warns) = up::parse_project_text(g["cases"]["missing_type"]["json"].as_str().unwrap())
        .unwrap();
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("type"), "предупреждение: {}", warns[0]);
    // Type present — no warning.
    let (_, warns) =
        up::parse_project_text(g["cases"]["full_web"]["json"].as_str().unwrap()).unwrap();
    assert!(warns.is_empty());
}

#[test]
fn typed_accessors_read_raw_schema_values() {
    // Raw WE values can be both a number and a string — the original stored them
    // as is despite the signature; the port stores them the same way but gives
    // typed access.
    let (p, _) = up::parse_project_text(
        r#"{"type":"scene","file":"s.json","general":{"properties":{
             "a":{"type":"slider","min":0,"max":"200","step":"0.5",
                  "precision":"2","fraction":"false"},
             "b":{"type":"slider","min":"мусор"}}}}"#,
    )
    .unwrap();
    let a = p.property("a").unwrap();
    assert_eq!(a.min_num(), Some(0.0));
    assert_eq!(a.max_num(), Some(200.0));
    assert_eq!(a.step_num(), Some(0.5));
    assert_eq!(a.precision_int(), Some(2));
    assert_eq!(a.fraction_bool(), Some(false), "строка «false» — это ложь");
    let b = p.property("b").unwrap();
    assert_eq!(b.min_num(), None);
    assert_eq!(b.max_num(), None);
}
