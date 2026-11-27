//! Golden comparison of the `remote_l10n.rs` port against the original
//! `rexon/qt_wallpaper_engine/qml_ui/remote_l10n.py`.
//!
//! The golden data was captured by a one-off generator from the LIVE Python,
//! read-only: `QM_PATH` was redirected to a temporary directory and `/bin/true`
//! ran instead of `lrelease` — neither `.ts` nor `.qm` in the project tree was
//! touched, Qt was never started.
//! Recorded: all dictionary tables, function values on live and degenerate
//! inputs, the full output of `translate_source` (serialized JSON, key order
//! as in the Python dict), reverse-translation maps per source, and the
//! contents of the `.ts` built over the REAL vendored QML tree.
//!
//! `golden/remote_l10n_cases.json` holds the captured values. Differences from
//! Python are ONLY deliberate fixes (rule #16, listed in the module header),
//! each pinned here by a separate test marked "rule #16".
//!
//! # Vendored QML labels — there is no dictionary of our own here
//!
//! waywallen page labels are translated by the VENDORED catalog
//! `waywallen/ui/i18n/waywallen_ru.ts`, vendored by the same `git diff` as
//! `ui/qml`. Our own dictionary (`QML_TEXTS`) is gone: it held 135 strings out
//! of the vendor's 486. So there are three checks here:
//!
//! 1. **Completeness** — every `qsTr` string of the live `ui/qml` is translated
//!    by the vendored catalog (`every_vendor_string_has_a_vendor_translation`),
//!    and conversely the catalog has no entries `ui/qml` no longer emits.
//! 2. **Freshness** — the committed `.qm` is built from THIS `.ts`
//!    (`committed_qm_is_compiled_from_the_vendored_catalog`).
//! 3. **Nothing lost** — every string the old Python dictionary translated
//!    that still exists in today's vendor tree is still translated
//!    (`nothing_the_python_dictionary_translated_was_lost`).

use rexon_core::i18n;
use rexon_core::remote_l10n as l10n;
use serde_json::{Map, Value};

mod i18n_pin;
use std::path::{Path, PathBuf};

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/remote_l10n_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

/// Compares a table slice with the Python dictionary: same pairs, same order.
///
/// Table values are English msgids; the Python dictionary is Russian. What is
/// compared is the TRANSLATION of the value from `po/ru.po` (the caller holds
/// `i18n_pin::ru()`).
fn assert_table(table: &[(&str, &str)], golden: &Value, name: &str) {
    let obj: Vec<(&String, &Value)> = golden.as_object().unwrap().iter().collect();
    assert_eq!(table.len(), obj.len(), "{name}: разное число записей");
    for ((k, v), (gk, gv)) in table.iter().zip(obj.iter()) {
        assert_eq!(k, gk, "{name}: ключ");
        assert_eq!(i18n::tr(v), s(gv), "{name}: значение для {k}");
    }
}

// ============================================================================
// Dictionary tables
// ============================================================================

#[test]
fn tables_match_python_dicts() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_table(l10n::SOURCE_NAMES, &g["SOURCE_NAMES"], "SOURCE_NAMES");
    assert_table(l10n::WP_TYPES, &g["WP_TYPES"], "WP_TYPES");
    assert_table(l10n::SORTS, &g["SORTS"], "SORTS");
    assert_table(l10n::FILTER_TITLES, &g["FILTER_TITLES"], "FILTER_TITLES");
    assert_table(
        l10n::FILTER_DESCRIPTIONS,
        &g["FILTER_DESCRIPTIONS"],
        "FILTER_DESCRIPTIONS",
    );
    assert_table(
        l10n::FILTER_CONFIRMATIONS,
        &g["FILTER_CONFIRMATIONS"],
        "FILTER_CONFIRMATIONS",
    );
    assert_table(l10n::TAGS, &g["TAGS"], "TAGS");
    assert_table(l10n::ACTION_LABELS, &g["ACTION_LABELS"], "ACTION_LABELS");
    assert_table(l10n::TEXTS, &g["TEXTS"], "TEXTS");
    // `QML_TEXTS` is GONE from here: the vendored QML's own labels are translated
    // by the vendored catalog, not by our dictionary. That nothing got lost is
    // guarded by `nothing_the_python_dictionary_translated_was_lost`.
}

#[test]
fn tags_back_matches_python_comprehension() {
    let _lang = i18n_pin::ru();
    // `{ru: en for en, ru in TAGS.items()}` — on a repeated label the LAST pair
    // wins: «Комп. графика» → CGI, not CG.
    let g = fx();
    let back = g["TAGS_BACK"].as_object().unwrap();
    for (label, en) in back {
        assert_eq!(l10n::tag_back(label), Some(s(en)), "обратный тег {label}");
    }
    // as many distinct labels as in Python
    let uniq: std::collections::BTreeSet<String> = l10n::TAGS.iter().map(|(_, m)| i18n::tr(m)).collect();
    assert_eq!(uniq.len(), back.len());
    assert_eq!(l10n::tag_back("Комп. графика"), Some("CGI"));
    assert_eq!(l10n::tag_back("Такого нет"), None);
}

// ============================================================================
// Single labels
// ============================================================================

#[test]
fn wp_type_label_matches() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for pair in g["wp_type_label"].as_array().unwrap() {
        let p = pair.as_array().unwrap();
        assert_eq!(l10n::wp_type_label(s(&p[0])), s(&p[1]), "вход {}", s(&p[0]));
    }
    // `str(value or "")`: an empty string stays empty
    assert_eq!(l10n::wp_type_label(""), s(&g["wp_type_label_none"]));
}

#[test]
fn tag_label_matches() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for pair in g["tag_label"].as_array().unwrap() {
        let p = pair.as_array().unwrap();
        assert_eq!(l10n::tag_label(s(&p[0])), s(&p[1]), "вход {}", s(&p[0]));
    }
    for pair in g["tag_labels"].as_array().unwrap() {
        let p = pair.as_array().unwrap();
        let input: Vec<String> = p[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        let want: Vec<String> = p[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        assert_eq!(l10n::tag_labels(&input), want);
    }
    let empty: [String; 0] = [];
    assert!(l10n::tag_labels(&empty).is_empty());
}

#[test]
fn text_matches() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for pair in g["_text"].as_array().unwrap() {
        let p = pair.as_array().unwrap();
        assert_eq!(l10n::text(s(&p[0])), s(&p[1]), "вход {}", s(&p[0]));
    }
    // TEXTS takes precedence over ACTION_LABELS: «Signed in» exists only in TEXTS
    assert_eq!(l10n::text("Signed in"), "Вход выполнен");
    // and «API key» only in ACTION_LABELS
    assert_eq!(l10n::text("API key"), "Ключ доступа");
    assert_eq!(l10n::text("Whatever else"), "Whatever else");
}

#[test]
fn sort_and_source_lookups() {
    let _lang = i18n_pin::ru();
    assert_eq!(l10n::sort_label("top_rated", "Top rated"), "Лидеры рейтинга");
    // an unknown sort of a new plugin keeps the vendor's label
    assert_eq!(l10n::sort_label("brand_new", "Brand new"), "Brand new");
    assert_eq!(l10n::source_name("wallpaper_engine").as_deref(), Some("Мастерская Steam"));
    assert_eq!(l10n::source_name("нет такого"), None);
}

// ============================================================================
// translate_source — full output, byte for byte
// ============================================================================

/// Runs all golden sources through one translator — as in the bridge, where
/// `translate_source` is called in turn for each source the daemon returns.
///
/// ⚠ The golden data was captured from the LIVE Python bridge, which still knew
/// the flat `RemoteFilterDef.values` field (waywallen ≤0.3.7). Since 0.3.8 a
/// filter option is a triple `{value, label, labelText}`, so the input is
/// unfolded by [`to_options`] and the output folded back by [`fold_options`].
/// Otherwise the golden data would have to be rewritten by hand and would stop
/// being a snapshot.
fn run_all_cases(g: &Value) -> (l10n::Localizer, Vec<(String, Value, Value)>) {
    let mut loc = l10n::Localizer::new();
    let mut out = Vec::new();
    for case in g["translate_source"].as_array().unwrap() {
        let name = s(&case["name"]).to_string();
        let got = loc.translate_source(&to_options(&case["input"])).unwrap();
        out.push((name, case["output"].clone(), fold_options(&got)));
    }
    (loc, out)
}

/// Python-era source description → waywallen 0.3.8 shape.
///
/// In each filter `values: ["Anime"]` becomes `options: [{"value": "Anime"}]`
/// IN THE SAME place — key order matters for the comparison. The plugin gives
/// no labels (`label`) here: the golden data had none either.
fn to_options(info: &Value) -> Value {
    map_filters(info, |key, value| {
        if key != "values" {
            return None;
        }
        let options: Vec<Value> = value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|v| serde_json::json!({ "value": v }))
                    .collect()
            })
            .unwrap_or_default();
        Some(("options".to_string(), Value::Array(options)))
    })
}

/// Inverse of [`to_options`]: `options` becomes `values` of labels again.
fn fold_options(info: &Value) -> Value {
    map_filters(info, |key, value| {
        if key != "options" {
            return None;
        }
        let values: Vec<Value> = value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|o| o.get("label").cloned().unwrap_or(Value::Null))
                    .collect()
            })
            .unwrap_or_default();
        Some(("values".to_string(), Value::Array(values)))
    })
}

/// Walks the `filters[]` of a source description and replaces one key with
/// another, keeping its PLACE in the map (the comparison is on the JSON string).
fn map_filters<F>(info: &Value, rename: F) -> Value
where
    F: Fn(&str, &Value) -> Option<(String, Value)>,
{
    let Some(src) = info.as_object() else {
        return info.clone();
    };
    let mut out = Map::new();
    for (key, value) in src {
        if key != "filters" {
            out.insert(key.clone(), value.clone());
            continue;
        }
        let filters: Vec<Value> = value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|f| match f.as_object() {
                        Some(fm) => {
                            let mut item = Map::new();
                            for (k, v) in fm {
                                match rename(k, v) {
                                    Some((nk, nv)) => item.insert(nk, nv),
                                    None => item.insert(k.clone(), v.clone()),
                                };
                            }
                            Value::Object(item)
                        }
                        None => f.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.insert(key.clone(), Value::Array(filters));
    }
    Value::Object(out)
}

#[test]
fn translate_source_matches_python_byte_for_byte() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let (_, cases) = run_all_cases(&g);
    for (name, want, got) in cases {
        // "extra_in_sort" is a deliberate fix, checked by a separate test
        if name == "extra_in_sort" {
            continue;
        }
        assert_eq!(
            serde_json::to_string(&got).unwrap(),
            serde_json::to_string(&want).unwrap(),
            "источник {name}: вывод (в т.ч. порядок ключей) разошёлся"
        );
    }
}

#[test]
fn translate_source_keeps_input_untouched() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let case = &g["translate_source"].as_array().unwrap()[0];
    let before = serde_json::to_string(&case["input"]).unwrap();
    l10n::Localizer::new().translate_source(&case["input"]).unwrap();
    assert_eq!(serde_json::to_string(&case["input"]).unwrap(), before);
}

#[test]
fn source_back_maps_match_python() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let (loc, _) = run_all_cases(&g);
    let want = g["TAGS_BACK_BY_SOURCE"].as_object().unwrap();
    for (source_id, map) in want {
        let got = loc
            .source_back_map(source_id)
            .unwrap_or_else(|| panic!("нет карты источника {source_id}"));
        let map = map.as_object().unwrap();
        assert_eq!(got.len(), map.len(), "размер карты {source_id}");
        for (label, en) in map {
            assert_eq!(got.get(label).map(String::as_str), Some(s(en)), "{source_id}/{label}");
        }
    }
}

#[test]
fn untranslate_tags_matches_python() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let (loc, _) = run_all_cases(&g);
    for row in g["untranslate_by_source"].as_array().unwrap() {
        let r = row.as_array().unwrap();
        let source_id = s(&r[0]);
        let input: Vec<String> = r[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        let want: Vec<String> = r[2]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        assert_eq!(loc.untranslate_tags(&input, source_id), want, "{source_id}");
    }
}

#[test]
fn untranslate_tags_without_source_uses_common_map() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let loc = l10n::Localizer::new();
    for row in g["untranslate_global"].as_array().unwrap() {
        let r = row.as_array().unwrap();
        let input: Vec<String> = r[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        let want: Vec<String> = r[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        assert_eq!(loc.untranslate_tags(&input, ""), want);
    }
    // unknown source → the common map (there «Комп. графика» = CGI)
    assert_eq!(
        loc.untranslate_tags(&["Комп. графика".to_string()], "нет_такого"),
        vec![s(&g["untranslate_unknown_source"][0])]
    );
}

#[test]
fn cg_cgi_collision_is_resolved_per_source() {
    let _lang = i18n_pin::ru();
    // The whole reason for per-source maps: one label maps to DIFFERENT
    // source tags for the Workshop (CGI) and Wallhaven (CG).
    let g = fx();
    let (loc, _) = run_all_cases(&g);
    let label = ["Комп. графика".to_string()];
    assert_eq!(loc.untranslate_tags(&label, "wallpaper_engine"), vec!["CGI"]);
    assert_eq!(loc.untranslate_tags(&label, "wallhaven"), vec!["CG"]);
}

#[test]
fn numeric_resolution_passes_through_untouched() {
    let _lang = i18n_pin::ru();
    // Numeric resolutions need no dictionary: they go back as they are.
    let g = fx();
    let (loc, _) = run_all_cases(&g);
    assert_eq!(
        loc.untranslate_tags(&["1920 x 1080".to_string()], "wallpaper_engine"),
        vec!["1920 x 1080"]
    );
}

#[test]
fn translate_source_rejects_non_object() {
    let _lang = i18n_pin::ru();
    // Python crashed here with TypeError ("'NoneType' object is not iterable" /
    // "object is not iterable"); here the same case is expressed by the error type.
    let g = fx();
    assert_eq!(s(&g["errors"]["translate_source_none"][0]), "TypeError");
    assert_eq!(s(&g["errors"]["translate_source_list"][0]), "TypeError");
    let mut loc = l10n::Localizer::new();
    assert_eq!(
        loc.translate_source(&Value::Null),
        Err(l10n::L10nError::NotAnObject)
    );
    assert_eq!(
        loc.translate_source(&serde_json::json!([1, 2])),
        Err(l10n::L10nError::NotAnObject)
    );
}

#[test]
fn field_and_status_keys_are_always_filled() {
    let _lang = i18n_pin::ru();
    // ORIGINAL'S QUIRK KEPT ON PURPOSE: fields and status rows get
    // label/description/placeholder/group/value even when the input had none.
    // The vendored QML tells undefined from "" — a "fix" here would break the
    // page.
    let mut loc = l10n::Localizer::new();
    let out = loc
        .translate_source(&serde_json::json!({
            "id": "x",
            "actions": [{"id": "a", "fields": [{"key": "k"}]}],
            "status": [{"id": "s"}],
        }))
        .unwrap();
    let field = &out["actions"][0]["fields"][0];
    assert_eq!(field["label"], Value::String(String::new()));
    assert_eq!(field["description"], Value::String(String::new()));
    assert_eq!(field["placeholder"], Value::String(String::new()));
    let row = &out["status"][0];
    assert_eq!(row["label"], Value::String(String::new()));
    assert_eq!(row["group"], Value::String(String::new()));
    assert_eq!(row["value"], Value::String(String::new()));
}

#[test]
fn identifiers_are_never_translated() {
    let _lang = i18n_pin::ru();
    // Both the vendored QML and the daemon go by source, filter and action ids
    // and by sort keys — they must not be touched.
    let g = fx();
    let (_, cases) = run_all_cases(&g);
    for (name, _, got) in cases {
        let input = &g["translate_source"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| s(&c["name"]) == name)
            .unwrap()["input"];
        assert_eq!(got["id"], input["id"], "{name}: id источника");
        for (kind, key) in [("filters", "id"), ("actions", "id"), ("sorts", "key")] {
            let want = input.get(kind).and_then(Value::as_array);
            let Some(want) = want else { continue };
            for (i, item) in want.iter().enumerate() {
                // Python fills a missing sort key with an empty value —
                // compare only what the source actually sent
                let Some(id) = item.get(key) else { continue };
                assert_eq!(got[kind][i].get(key), Some(id), "{name}: {kind}[{i}].{key}");
            }
        }
    }
}

// ── rule #16 fixes ────────────────────────────────────────────────────

#[test]
fn rule16_sort_entry_keeps_extra_keys() {
    let _lang = i18n_pin::ru();
    // ⚠ Rule #16: Python rebuilt a sorts element from two fields and lost
    // everything else (`icon`, `order`), although filters/actions are copied
    // whole. Here extra keys are kept, key/label stay first.
    let g = fx();
    let case = g["translate_source"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| s(&c["name"]) == "extra_in_sort")
        .unwrap();
    let python = &case["output"]["sorts"][0];
    assert!(python.get("icon").is_none(), "Python терял лишние ключи");

    let got = l10n::Localizer::new().translate_source(&case["input"]).unwrap();
    assert_eq!(
        serde_json::to_string(&got["sorts"][0]).unwrap(),
        r#"{"key":"recent","label":"Недавно добавленные","icon":"clock","order":7}"#
    );
}

#[test]
fn empty_source_mapping_stays_empty() {
    let _lang = i18n_pin::ru();
    // Pins behaviour: the original's `or` is replaced here by a key check
    // (rule #16, defensive). There is NO observable difference today — an empty
    // value gets into the map only under an empty label, and the common map has
    // no answer for it either; the test pins the result of both versions.
    let mut loc = l10n::Localizer::new();
    loc.translate_source(&serde_json::json!({
        "id": "странный",
        "filters": [{"id": "type", "values": [""]}],
    }))
    .unwrap();
    // the source named the empty tag with the label «» — it goes back empty
    assert_eq!(loc.untranslate_tags(&["".to_string()], "странный"), vec![""]);
}

#[test]
fn rule16_source_map_is_rebuilt_not_accumulated() {
    let _lang = i18n_pin::ru();
    // ⚠ Rule #16: in Python the source map grew forever (`setdefault`), and
    // after a plugin update it kept labels of tags that had disappeared.
    let mut loc = l10n::Localizer::new();
    let old = serde_json::json!({
        "id": "src",
        "filters": [{"id": "genre",
                     "options": [{"value": "CGI"}, {"value": "Anime"}]}],
    });
    let new = serde_json::json!({
        "id": "src",
        "filters": [{"id": "genre", "options": [{"value": "CG"}]}],
    });
    loc.translate_source(&old).unwrap();
    loc.translate_source(&new).unwrap();
    let map = loc.source_back_map("src").unwrap();
    assert_eq!(map.len(), 1, "остались подписи прошлой выдачи: {map:?}");
    assert_eq!(
        loc.untranslate_tags(&["Комп. графика".to_string()], "src"),
        vec!["CG"]
    );
}

// ============================================================================
// Vendored QML translation catalog
// ============================================================================

#[test]
fn paths_match_python() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(l10n::qm_path("/mod"), format!("/mod/{}", s(&g["qm_path_rel"])));
    assert_eq!(
        l10n::default_qml_dir("/mod"),
        format!("/mod/{}", s(&g["default_qml_dir_rel"]))
    );
    // ⚠ The catalog source is no longer "a .ts next to the .qm": it is vendored
    // and lives inside the vendored tree, next to the very `ui/qml` it translates.
    assert_eq!(
        l10n::vendor_ts_path("/mod"),
        "/mod/waywallen/ui/i18n/waywallen_ru.ts"
    );
    assert_eq!(l10n::DEFAULT_LRELEASE, s(&g["default_lrelease"]));
    // ⚠ Rule #16: the hard-coded Arch path is kept as the last fallback
    assert_eq!(l10n::LRELEASE_CANDIDATES.last(), Some(&l10n::DEFAULT_LRELEASE));
}

/// Root of the vendored tree in the working copy.
fn vendor_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/qt_wallpaper_engine/qml_ui")
}

/// All `.qml` of the vendored tree: (path, contents) — what Python gets from
/// `glob("**/*.qml", recursive=True)` + reading the file.
fn vendor_qml_files() -> Vec<(String, String)> {
    let root = vendor_root().join("waywallen/ui/qml");
    let mut out = Vec::new();
    walk(&root, &mut out);
    assert!(!out.is_empty(), "вендорное дерево QML не найдено: {root:?}");
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().map(|e| e == "qml").unwrap_or(false) {
            out.push((
                path.to_string_lossy().into_owned(),
                std::fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

/// Vendored translation catalog: path and contents.
fn vendor_ts() -> (String, String) {
    let path = l10n::vendor_ts_path(&vendor_root().to_string_lossy());
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("вендорный каталог переводов не найден ({path}): {e}"));
    (path, body)
}

/// Build plan over the live vendored tree.
fn live_plan() -> (Vec<(String, String)>, l10n::QmBuildPlan) {
    let files = vendor_qml_files();
    let (ts_path, ts) = vendor_ts();
    let plan = l10n::plan_build_qm(&files, &ts, &ts_path, &qm_path_in_tree(), &lrelease());
    (files, plan)
}

/// Where the built catalog goes (next to the UI module).
fn qm_path_in_tree() -> String {
    l10n::qm_path(&vendor_root().to_string_lossy())
}

#[test]
fn every_vendor_string_has_a_vendor_translation() {
    let _lang = i18n_pin::ru();
    // The main check: EVERYTHING the vendored QML sends for translation is
    // translated in its own catalog. Non-zero means `ui/qml` and `ui/i18n` were
    // vendored from different commits, and the buyer will see these strings in English.
    let (files, plan) = live_plan();
    assert!(
        awaiting_upstream_unexpected(&plan, &RU_AWAITING_UPSTREAM).is_empty(),
        "{}",
        vendor_drift_report(&plan)
    );
    // and this is not "almost everything": our own dictionary had 135 vendor
    // labels, the vendored catalog covers the whole tree — an order of magnitude more.
    let all = l10n::vendor_sources(&files).len();
    assert!(all > 350, "вендорных строк всего {all} — дерево не то?");
}

/// Catalog strings that are not and will not be in `ui/qml`: the vendor
/// declares them in its C++ (`ui/src`), and we vendor only `ui/qml` — all of
/// the C++ is rewritten in Rust on our side.
///
/// The list is CLOSED and itemized: it does not "allow divergence in general",
/// it names each string and who shows it on our side. A string may be added
/// here only together with the name of our consuming module.
///
/// * «Scheme color», «Playback speed» — labels of predefined wallpaper
///   properties. The vendor emits them from `ui/src/model/user_property_model.cpp`
///   (via `tr()` since 0.3.8), we from `rexon-gui/src/user_property_model.rs`.
/// * «Mouse parallax» — the third such label (mouse parallax, 0.4.2); the
///   entry reached the vendor's Russian catalog in v0.4.4. Same consumer —
///   `rexon-gui/src/user_property_model.rs`.
///
const CATALOG_ONLY_IN_VENDOR_CPP: [&str; 3] =
    ["Scheme color", "Playback speed", "Mouse parallax"];

/// The daemon's tray icon menu (context `TrayMenu`, vendor's
/// `ui/src/tray_i18n.cpp`, since 0.5.0). Rexon has NO daemon tray: Rexon
/// starts it with `--no-tray` (`rexon_core::tray`), the tray menu language is
/// not passed (`DaemonDBusClient.setTrayLanguage` is not needed), nobody can
/// show these strings. A closed itemized list, as above. «Mute», «Pause» and
/// «Shuffle» from the same menu are not here: the markup uses those words too.
const CATALOG_TRAY_NOT_SHOWN: [&str; 15] = [
    "1 hour",
    "1 minute",
    "15 minutes",
    "30 seconds",
    "5 minutes",
    "Linux wallpaper daemon",
    "Next",
    "Off",
    "Open UI",
    "Previous",
    "Quit",
    "Rescan wallpapers",
    "Resume",
    "Rotate",
    "Unmute",
];

/// Vendor strings missing from upstream's RUSSIAN catalog. The 0.5.0 strings
/// (smart playlists, the «Library» filter) were translated upstream by 0.6.0 —
/// the list no longer holds them. In 0.6.0 the markup called seven new strings,
/// and the tag's catalogs (all four: ru, de, fr, zh_CN) were built without them —
/// `lupdate` was not rerun before the release, there are no entries at all,
/// not even `unfinished`:
///
/// * «B», «KiB», «MiB», «GiB», «TiB» — size units of `I18n.formatBytes`
///   (wallpaper size, download progress);
/// * «h», «m» — hours and minutes of the wallpaper change interval on
///   `PlaylistEditPage` (with the translator note "hours"/"minutes").
///
/// Another program's translation is fixed in that program — these strings go
/// as a SUBMISSION to waywallen (from the Russian original: «Б», «КиБ», «МиБ»,
/// «ГиБ», «ТиБ», «ч», «мин»), not as our patch. The list is closed and removes
/// itself: as soon as upstream translates a string, the check demands it be
/// removed from here.
///
/// waywallen 0.6.1 repeated it: the markup asks for sixteen more strings
/// (card badges and their rules page, the artist page, "Back"/"Menu" of the
/// new navigation, "Change library folder", the unavailable-source notice,
/// "Wallpaper type") and only zh_CN got them; ru, de, uk, es and fr have no
/// entries at all, upstream main neither (checked 2026-11-24). Same answer:
/// a translation submitted to waywallen, not a local dictionary.
const RU_AWAITING_UPSTREAM: [&str; 23] = [
    "%1: %2 → %3",
    "Artist",
    "B",
    "Back",
    "Badge text",
    "Card badge rules",
    "Card badges",
    "Change library folder",
    "Edit",
    "Edit rules",
    "GiB",
    "KiB",
    "Match field",
    "Matching value",
    "Menu",
    "MiB",
    "Rules ignore case and match the whole value. Use * for any number of characters and ? for one character. A tag rule matches any tag on the wallpaper.",
    "Show card badges",
    "This source is no longer available.",
    "TiB",
    "Wallpaper type",
    "h",
    "m",
];

/// The same for upstream's GERMAN catalog: the same strings (seven from 0.6.0,
/// sixteen from 0.6.1). Rexon's German translation is made FROM the Russian
/// original — and so is the submission to waywallen.
const DE_AWAITING_UPSTREAM: [&str; 23] = RU_AWAITING_UPSTREAM;

/// 0.6.0 catalog entries whose strings no longer exist anywhere upstream —
/// neither in `ui/qml` nor in `ui/src` (on tag v0.6.0 they remain only in `.ts`):
/// «256 MiB» and «512 MiB» — former «Image GPU cache» options in settings,
/// «Reverse order» — a former sort item. The same missed `lupdate` run as
/// above, only the other way round; harmless — nobody will ask for the translation.
/// A closed itemized list, removes itself when upstream cleans the catalog.
const CATALOG_STALE_IN_UPSTREAM: [&str; 3] = ["256 MiB", "512 MiB", "Reverse order"];

#[test]
fn every_vendor_string_has_a_german_translation() {
    // Upstream's German catalog is built into its own `.qm` and shown to German
    // Rexon — its gaps are visible to the buyer just as in the Russian one.
    let files = vendor_qml_files();
    let root = vendor_root().to_string_lossy().into_owned();
    let ts_path = l10n::vendor_ts_path_for(&root, "de");
    let ts = std::fs::read_to_string(&ts_path)
        .unwrap_or_else(|e| panic!("немецкий каталог вендора не найден ({ts_path}): {e}"));
    let plan = l10n::plan_build_qm(&files, &ts, &ts_path, &l10n::qm_path_for(&root, "de"), &lrelease());
    assert!(
        awaiting_upstream_unexpected(&plan, &DE_AWAITING_UPSTREAM).is_empty(),
        "{}",
        vendor_drift_report(&plan)
    );
}

/// Untranslated plan strings that are NOT in the awaiting-upstream list.
/// The reverse is checked right here too: a listed string that upstream has
/// already translated is a reason to strike it out (otherwise the list goes stale).
fn awaiting_upstream_unexpected<'a>(
    plan: &'a l10n::QmBuildPlan,
    awaiting: &[&str],
) -> Vec<&'a String> {
    for wait in awaiting {
        assert!(
            plan.untranslated.iter().any(|s| s == wait),
            "«{wait}» апстрим уже перевёл — вычеркнуть из списка ждущих"
        );
    }
    plan.untranslated
        .iter()
        .filter(|s| !awaiting.contains(&s.as_str()))
        .collect()
}

#[test]
fn catalog_has_nothing_the_vendor_qml_lost() {
    let _lang = i18n_pin::ru();
    // Reverse skew: a catalog entry that `ui/qml` no longer has. Harmless by
    // itself, but it is exactly the same symptom — `qml` and `i18n` vendored
    // from different commits, and in the other direction the skew already hurts.
    let (_, plan) = live_plan();
    let stale: Vec<&String> = plan
        .stale
        .iter()
        .filter(|s| !CATALOG_ONLY_IN_VENDOR_CPP.contains(&s.as_str()))
        .filter(|s| !CATALOG_TRAY_NOT_SHOWN.contains(&s.as_str()))
        .filter(|s| !CATALOG_STALE_IN_UPSTREAM.contains(&s.as_str()))
        .collect();
    assert!(stale.is_empty(), "{}", vendor_drift_report(&plan));
    // The exception list must not go stale: every string must BE in the catalog,
    // otherwise it has long been unneeded and should be thrown out of here.
    for extra in CATALOG_ONLY_IN_VENDOR_CPP
        .iter()
        .chain(CATALOG_TRAY_NOT_SHOWN.iter())
        .chain(CATALOG_STALE_IN_UPSTREAM.iter())
    {
        assert!(
            plan.stale.iter().any(|s| s == *extra),
            "исключение «{extra}» каталогу больше не нужно — снять его"
        );
    }
}

/// What to tell a person when the catalog has diverged from the vendored QML.
///
/// A bare "mismatch" is useless: it is unclear WHAT diverged and what to do.
fn vendor_drift_report(plan: &l10n::QmBuildPlan) -> String {
    let mut out = String::new();
    if plan.untranslated.is_empty() && plan.stale.is_empty() {
        return "каталог и вендорный QML сошлись полностью\n".into();
    }
    if !plan.untranslated.is_empty() {
        out.push_str(&format!(
            "вендорных строк без перевода: {} (покупатель видит их по-английски)\n",
            plan.untranslated.len()
        ));
        // The whole list: it is what goes into the awaiting-upstream list.
        for lost in plan.untranslated.iter() {
            out.push_str(&format!("  ❌ «{lost}»\n"));
        }
    }
    if !plan.stale.is_empty() {
        out.push_str(&format!(
            "записей каталога, которых в `ui/qml` больше нет: {}\n",
            plan.stale.len()
        ));
        for extra in plan.stale.iter().take(20) {
            out.push_str(&format!("  ? «{extra}»\n"));
        }
    }
    out.push_str(
        "что делать: перевод подписей чужой программы правится У НЕЁ, а не у нас. \
         Довендорить `waywallen/ui/i18n/waywallen_ru.ts` с того же коммита, что и \
         `waywallen/ui/qml` (`ui/UPSTREAM_TAG`); если строка у вендора новая и \
         непереведённая — подать перевод в waywallen, чтобы он достался всем.\n",
    );
    out
}

/// The first available `lrelease` — as in `remote_l10n._find_lrelease()`.
fn lrelease() -> String {
    for cand in l10n::LRELEASE_CANDIDATES {
        if cand.contains('/') {
            if Path::new(cand).exists() {
                return (*cand).to_string();
            }
        } else if let Ok(paths) = std::env::var("PATH") {
            for dir in paths.split(':') {
                if Path::new(dir).join(cand).exists() {
                    return Path::new(dir).join(cand).to_string_lossy().into_owned();
                }
            }
        }
    }
    l10n::DEFAULT_LRELEASE.to_string()
}

/// ⚠️ A live machine is needed ESSENTIALLY: the check runs the real `lrelease`
/// and compares its output with the committed `.qm` byte for byte. There is
/// nothing to fake the catalog builder with — that is the whole question.
/// Skipped with a reason under `cargo test-core-isolated`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: runs the real lrelease")]
fn committed_qm_is_compiled_from_the_vendored_catalog() {
    // The catalog shipped in the build must be compiled from THE SAME vendored
    // `.ts` that lies in the tree: otherwise a vendor update silently leaves the
    // buyer with old labels. Build into a temporary file and compare the bytes.
    // There are as many catalogs as vendored `.ts` (since 0.5.0 — ru and de), and
    // each must lie built next to the module: without a `.qm` the language does not
    // get into the tab's language list, and the pages stay in English.
    let lrelease = lrelease();
    if !Path::new(&lrelease).exists() {
        eprintln!("lrelease не найден ({lrelease}) — сверка собранного каталога пропущена");
        return;
    }
    let root = vendor_root().to_string_lossy().into_owned();
    let codes = vendor_catalogs();
    assert!(
        ["ru", "de", "zh_CN", "fr"].iter().all(|w| codes.iter().any(|c| c == w)),
        "вендорных каталогов: {codes:?} — ждали ru, de, zh_CN и fr (fr — с waywallen 0.6.0)"
    );
    for code in &codes {
        let ts_path = l10n::vendor_ts_path_for(&root, code);
        let qm_path = l10n::qm_path_for(&root, code);
        let tmp = std::env::temp_dir().join(format!("rexon-waywallen-{code}-check.qm"));
        let out = std::process::Command::new(&lrelease)
            .args([&ts_path, &"-qm".to_string(), &tmp.to_string_lossy().into_owned()])
            .output()
            .unwrap_or_else(|e| panic!("не запустился {lrelease}: {e}"));
        assert!(
            out.status.success(),
            "lrelease не собрал вендорный каталог {code}:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let fresh = std::fs::read(&tmp).expect("свежий .qm");
        let _ = std::fs::remove_file(&tmp);
        let on_disk = std::fs::read(&qm_path)
            .unwrap_or_else(|e| panic!("нет собранного каталога {qm_path}: {e} — scripts/rexon-golden-update waywallen_qm"));
        assert_eq!(
            on_disk,
            fresh,
            "собранный каталог {code} отстал от вендорного .ts — пересобрать: \
             scripts/rexon-golden-update waywallen_qm"
        );
    }
}

/// Languages of the vendored `.ts` catalogs in the tree.
fn vendor_catalogs() -> Vec<String> {
    let dir = vendor_root().join(l10n::VENDOR_I18N_REL);
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("нет каталога переводов вендора {dir:?}: {e}"))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    l10n::vendor_catalog_languages(&names)
}

/// Every upstream translation catalog (`ui/i18n/waywallen_*.ts` of the tag from
/// `ui/UPSTREAM_TAG`) lies in the vendor tree, word for word. On 08.11.2026 only
/// ru and de were vendored, and Chinese, translated upstream since 0.5.0, got
/// lost — Rexon in Chinese showed the wallpaper tab in English.
///
/// The upstream clone is `$REXON_WAYWALLEN_SOURCE`, otherwise the same as for
/// `scripts/rexon-vendor-update` (`~/Загрузки/owe-ref/waywallen`); no clone or
/// no tag in it — «НЕ ПРОВЕРЕНО», not a silent skip. Without a clone it at
/// least guards that the sync takes `ui/i18n` whole.
#[test]
fn vendor_has_every_upstream_catalog() {
    let script = include_str!("../../../scripts/sync-waywallen-qml.py");
    assert!(
        script.contains("\"ui/i18n\",") && !script.contains("\"ui/i18n/waywallen_"),
        "sync-waywallen-qml.py обязан брать каталог ui/i18n целиком, а не список языков"
    );
    let source = std::env::var("REXON_WAYWALLEN_SOURCE").unwrap_or_else(|_| {
        format!("{}/Загрузки/owe-ref/waywallen", std::env::var("HOME").unwrap_or_default())
    });
    let tag_file = vendor_root().join("waywallen/ui/UPSTREAM_TAG");
    let tag_text = std::fs::read_to_string(&tag_file).expect("ui/UPSTREAM_TAG");
    let tag = tag_text.split_whitespace().next().expect("тег в ui/UPSTREAM_TAG").to_string();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| o.stdout)
    };
    let Some(listing) = git(&["ls-tree", "--name-only", &tag, "ui/i18n/"]) else {
        eprintln!("НЕ ПРОВЕРЕНО: нет клона waywallen с тегом {tag} в {source} (REXON_WAYWALLEN_SOURCE)");
        return;
    };
    let upstream: Vec<String> = String::from_utf8_lossy(&listing)
        .lines()
        .filter_map(|l| l.strip_prefix("ui/i18n/"))
        .filter(|n| n.starts_with("waywallen_") && n.ends_with(".ts"))
        .map(str::to_string)
        .collect();
    assert!(!upstream.is_empty(), "у тега {tag} нет каталогов ui/i18n/waywallen_*.ts");
    let dir = vendor_root().join(l10n::VENDOR_I18N_REL);
    for name in &upstream {
        let theirs = git(&["show", &format!("{tag}:ui/i18n/{name}")]).expect("файл тега");
        let ours = std::fs::read(dir.join(name))
            .unwrap_or_else(|_| panic!("каталог апстрима {name} ({tag}) не вендорен — scripts/sync-waywallen-qml.py --ref {tag}"));
        assert!(ours == theirs, "{name} разошёлся с апстримом {tag}");
    }
}

/// Every Rexon language for which the vendor ships a page catalog gets exactly
/// its own `.qm` (Rexon language code ↔ file name): `zh_CN` →
/// `waywallen_zh_CN.qm`, not the English sources.
#[test]
fn every_rexon_language_with_vendor_catalog_picks_its_qm() {
    let codes = vendor_catalogs();
    let linguas = include_str!("../../../po/LINGUAS");
    for lang in linguas.split_whitespace().filter(|l| !l.starts_with('#')) {
        if !codes.iter().any(|c| c == lang) {
            continue;
        }
        let plan = l10n::plan_ui_language(lang, "", &codes).expect("план языка");
        assert_eq!(plan.qm_file, l10n::qm_file_name(lang), "язык Rexon {lang}");
        assert!(vendor_root().join(&plan.qm_file).is_file(), "нет собранного {}", plan.qm_file);
    }
    assert!(codes.iter().any(|c| c == "zh_CN"), "китайского каталога в вендоре нет: {codes:?}");
}

#[test]
fn vendor_catalog_languages_from_file_names() {
    let names = ["waywallen_ru.ts", "waywallen_de.ts", "waywallen_ru.qm", "waywallen_.ts", "other_de.ts"];
    assert_eq!(l10n::vendor_catalog_languages(&names), vec!["de", "ru"]);
    assert_eq!(
        l10n::vendor_ts_path_for("/mod", "de"),
        "/mod/waywallen/ui/i18n/waywallen_de.ts"
    );
    assert_eq!(l10n::qm_path_for("/mod", "de"), "/mod/waywallen_de.qm");
    // the former Russian catalog path is a special case of the general one
    assert_eq!(l10n::vendor_ts_path_for("/mod", "ru"), l10n::vendor_ts_path("/mod"));
    assert_eq!(l10n::qm_path_for("/mod", "ru"), l10n::qm_path("/mod"));
}

#[test]
fn nothing_the_python_dictionary_translated_was_lost() {
    let _lang = i18n_pin::ru();
    // Our own dictionary is gone — but no string it translated may be left
    // untranslated. The `build_qm_ts_real` golden data was captured from the live
    // Python on vendored tree v0.2.6+9: take all `<source>` from it and require
    // that each one still present in today's `ui/qml` is translated by the
    // vendored catalog.
    let g = fx();
    let (files, plan) = live_plan();
    let alive: std::collections::BTreeSet<String> =
        l10n::vendor_sources(&files).into_iter().collect();
    let (_, ts) = vendor_ts();
    let translated = l10n::catalog_translated(&ts);

    let mut checked = 0;
    for line in s(&g["build_qm_ts_real"]).lines() {
        let source = match (line.find("<source>"), line.find("</source>")) {
            (Some(a), Some(b)) => &line[a + "<source>".len()..b],
            _ => continue,
        };
        if !alive.contains(source) {
            continue; // the vendor dropped the string — nothing to translate
        }
        checked += 1;
        assert!(
            translated.contains(source),
            "строка «{source}» переводилась Python-словарём, а вендорный каталог \
             её не переводит — перевод потерян"
        );
    }
    assert!(checked > 100, "эталон Python дал всего {checked} живых строк");
    assert!(awaiting_upstream_unexpected(&plan, &RU_AWAITING_UPSTREAM).is_empty());
}

#[test]
fn plural_forms_match_russian_rules() {
    // Qt counts plural forms by the catalog language: Russian has three
    // (1 / 2–4 / 5 and more). Fewer forms — lrelease complains and some numbers
    // stay untranslated.
    let (_, ts) = vendor_ts();
    assert!(
        ts.contains("language=\"ru_RU\"") || ts.contains("language=\"ru\""),
        "вендорный каталог не русский"
    );
    let plurals = ts.matches("<message numerus=\"yes\">").count();
    assert!(plurals >= 3, "строк со счётчиком в каталоге: {plurals}");
    for block in ts.split("<message numerus=\"yes\">").skip(1) {
        let block = &block[..block.find("</message>").expect("незакрытое сообщение")];
        let source = block
            [block.find("<source>").expect("нет source") + "<source>".len()..]
            .split("</source>")
            .next()
            .unwrap();
        assert!(source.contains("%n"), "строка со счётчиком без %n: {source}");
        assert_eq!(
            block.matches("<numerusform>").count(),
            3,
            "формы числа для «{source}»"
        );
    }
}

#[test]
fn playlist_counter_is_translated_on_both_pages() {
    // The wallpaper counter in a playlist is the only string whose translation
    // depends on a number. It appears on two vendor pages, and both must be in
    // the catalog, otherwise one of them shows the English «3 wallpapers».
    let (_, ts) = vendor_ts();
    assert_eq!(
        ts.matches("<source>%n wallpaper(s)</source>").count(),
        2,
        "счётчик обоев не на обеих страницах плейлистов"
    );
    for context in ["AddToPlaylistSheetContent", "PlaylistListSheet"] {
        assert!(ts.contains(&format!("<name>{context}</name>")), "нет контекста {context}");
    }
    assert!(l10n::catalog_translated(&ts).contains("%n wallpaper(s)"));
}

#[test]
fn vendor_sources_reads_both_quote_styles_and_plurals() {
    // ⚠ Rule #16: Python saw only `qsTr("…")` — neither single quotes
    // (the vendor writes `qsTr('Status')` in `Window.qml`) nor the counted call
    // `qsTr("…", "", n)`. Such a string silently stayed untranslated.
    let files = [(
        "/a/Mix.qml".to_string(),
        "qsTr(\"Search\") qsTr('Status') qsTr(\"%n wallpaper(s)\", \"\", n) \
         qsTr(\"Names\") text: \"Search\""
            .to_string(),
    )];
    assert_eq!(
        l10n::vendor_sources(&files),
        vec!["%n wallpaper(s)", "Names", "Search", "Status"]
    );
    // and the vendor really uses both forms
    let live = vendor_qml_files();
    assert!(live.iter().any(|(_, body)| body.contains("qsTr('")));
    assert!(live.iter().any(|(_, body)| body.contains(", \"\", ")));
}

#[test]
fn vendor_sources_reads_qs_translate() {
    // waywallen 0.5.0: `qsTranslate("Context", "…")` — the string is the SECOND
    // argument. An expression instead of a literal and foreign functions are skipped.
    let files = [(
        "/a/Rules.qml".to_string(),
        "qsTranslate(\"WallpaperFilterDialog\", \"Group %1\").arg(n) \
         qsTranslate('Window',  'OR') qsTranslate(\"Ctx\", name) \
         myqsTranslate(\"A\", \"B\") qsTranslate(\"Ctx\")"
            .to_string(),
    )];
    assert_eq!(l10n::vendor_sources(&files), vec!["Group %1", "OR"]);
    let live = vendor_qml_files();
    assert!(live.iter().any(|(_, body)| body.contains("qsTranslate(\"")));
}

#[test]
fn catalog_reader_skips_unfinished_and_unescapes_xml() {
    // An unfinished translation does NOT count as a translation: Qt returns the
    // source string and the buyer sees English. XML special characters are decoded
    // back — otherwise the string `A & B` from QML never matches the catalog entry.
    let ts = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE TS>
<TS version="2.1" language="ru_RU">
<context>
    <name>Page</name>
    <message>
        <source>Done</source>
        <translation>Готово</translation>
    </message>
    <message>
        <source>Raw</source>
        <translation type="unfinished"></translation>
    </message>
    <message>
        <source>A &amp; B &lt;C&gt;</source>
        <translation>А и Б</translation>
    </message>
    <message numerus="yes">
        <source>%n item(s)</source>
        <translation>
            <numerusform>%n штука</numerusform>
            <numerusform>%n штуки</numerusform>
            <numerusform>%n штук</numerusform>
        </translation>
    </message>
</context>
</TS>
"#;
    let done = l10n::catalog_translated(ts);
    assert!(done.contains("Done"));
    assert!(done.contains("A & B <C>"), "спецсимволы XML не разобраны");
    assert!(done.contains("%n item(s)"), "строка со счётчиком не засчитана");
    assert!(!done.contains("Raw"), "незавершённый перевод засчитан за перевод");
    assert_eq!(done.len(), 3);
}

#[test]
fn plan_names_the_compiler_command() {
    let _lang = i18n_pin::ru();
    let files = [("/a/P.qml".to_string(), "qsTr(\"Done\") qsTr(\"Gone\")".to_string())];
    let ts = "<TS><context><name>P</name><message><source>Done</source>\
              <translation>Готово</translation></message>\
              <message><source>Old</source><translation>Старое</translation>\
              </message></context></TS>";
    let plan = l10n::plan_build_qm(&files, ts, "/v/ru.ts", "/out/ru.qm", "/bin/lrelease");
    assert_eq!(plan.ts_path, "/v/ru.ts");
    assert_eq!(plan.qm_path, "/out/ru.qm");
    assert_eq!(
        plan.lrelease_argv,
        vec!["/bin/lrelease", "/v/ru.ts", "-qm", "/out/ru.qm"]
    );
    // the skew is visible in both directions
    assert_eq!(plan.untranslated, vec!["Gone"]);
    assert_eq!(plan.stale, vec!["Old"]);
}

// ============================================================================
// Degenerate inputs (in Python — "or" and str())
// ============================================================================

#[test]
fn degenerate_inputs() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let loc = l10n::Localizer::new();
    let empty: [String; 0] = [];
    assert!(loc.untranslate_tags(&empty, "").is_empty());
    assert!(g["untranslate_none"].as_array().unwrap().is_empty());
    assert!(g["tag_labels_none"].as_array().unwrap().is_empty());
    // empty source description: the dictionaries are filled in anyway
    let mut loc = l10n::Localizer::new();
    let out = loc.translate_source(&Value::Object(Map::new())).unwrap();
    assert_eq!(
        serde_json::to_string(&out).unwrap(),
        r#"{"sorts":[],"tags":[],"filters":[],"actions":[],"status":[]}"#
    );
}

// ============================================================================
// Interface language of the vendored pages (original's ui_language.cpp)
// ============================================================================

#[test]
fn ui_language_codes_from_catalogs() {
    // Exactly what lies next to the module: one name is ours, the rest are skipped.
    let names = [
        "waywallen_ru.qm".to_string(),
        "waywallen_zh_CN.qm".to_string(),
        // the English catalog is dropped — the source strings already are English
        "waywallen_en.qm".to_string(),
        "waywallen_en_GB.qm".to_string(),
        // not our files
        "qtbase_ru.qm".to_string(),
        "waywallen_ru.ts".to_string(),
        "waywallen_.qm".to_string(),
    ];
    assert_eq!(
        l10n::ui_language_codes(&names),
        vec!["ru".to_string(), "zh_CN".to_string()]
    );
    let none: [String; 0] = [];
    assert!(l10n::ui_language_codes(&none).is_empty());
}

#[test]
fn ui_language_normalize() {
    let codes = vec!["ru".to_string(), "zh_CN".to_string()];
    // "as in the system" and English are always accepted
    assert_eq!(
        l10n::normalize_ui_language("system", &codes).as_deref(),
        Some("system")
    );
    assert_eq!(
        l10n::normalize_ui_language("en_US", &codes).as_deref(),
        Some("en")
    );
    // exact match
    assert_eq!(
        l10n::normalize_ui_language("zh_CN", &codes).as_deref(),
        Some("zh_CN")
    );
    // by base language in both directions
    assert_eq!(
        l10n::normalize_ui_language("ru_RU.UTF-8", &codes).as_deref(),
        Some("ru")
    );
    assert_eq!(
        l10n::normalize_ui_language("zh", &codes).as_deref(),
        Some("zh_CN")
    );
    // unsupported and empty — rejected (in the original `setUiLanguage` → false)
    assert!(l10n::normalize_ui_language("de", &codes).is_none());
    assert!(l10n::normalize_ui_language("", &codes).is_none());
}

#[test]
fn ui_language_plan() {
    let codes = vec!["ru".to_string()];

    // "as in the system": the system locale picks the catalog, but the catalog is optional
    let system = l10n::plan_ui_language("system", "ru_RU", &codes).unwrap();
    assert_eq!(system.preference, "system");
    assert_eq!(system.locale_name, "");
    assert_eq!(system.qm_file, "waywallen_ru.qm");
    assert!(!system.required);

    // the system is English — no catalog, and that is NOT an error
    let system_en = l10n::plan_ui_language("system", "en_US", &codes).unwrap();
    assert_eq!(system_en.qm_file, "");
    assert!(!system_en.required);

    // the system is German, no catalog — source strings as well
    let system_de = l10n::plan_ui_language("system", "de_DE", &codes).unwrap();
    assert_eq!(system_de.qm_file, "");

    // English: no catalog needed, the strings are the sources
    let english = l10n::plan_ui_language("en", "ru_RU", &codes).unwrap();
    assert_eq!(english.preference, "en");
    assert_eq!(english.locale_name, "en");
    assert_eq!(english.qm_file, "");
    assert!(!english.required);

    // Russian: the catalog is required
    let russian = l10n::plan_ui_language("ru_RU", "en_US", &codes).unwrap();
    assert_eq!(russian.preference, "ru");
    assert_eq!(russian.locale_name, "ru");
    assert_eq!(russian.qm_file, "waywallen_ru.qm");
    assert!(russian.required);

    // no language — no plan
    assert!(l10n::plan_ui_language("de", "ru_RU", &codes).is_none());
}

// ============================================================================
// Interface language: English source, Russian as before
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// With the English interface no catalog label comes out in Russian.
#[test]
fn english_remote_labels_have_no_russian() {
    let _lang = i18n_pin::en();
    for table in [
        l10n::SOURCE_NAMES,
        l10n::WP_TYPES,
        l10n::SORTS,
        l10n::FILTER_TITLES,
        l10n::FILTER_DESCRIPTIONS,
        l10n::FILTER_CONFIRMATIONS,
        l10n::TAGS,
        l10n::ACTION_LABELS,
        l10n::TEXTS,
    ] {
        for (key, msgid) in table {
            let shown = i18n::tr(msgid);
            assert!(!has_cyrillic(&shown), "{key}: {shown}");
        }
    }
    // And the full translation of the golden source description too.
    // Identifiers (`id`, `key`) are not translated by construction — the golden
    // data has a Cyrillic filter `id` as well, it must pass through as is.
    fn shown_strings(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| shown_strings(x, out)),
            Value::Object(m) => m
                .iter()
                .filter(|(k, _)| k.as_str() != "id" && k.as_str() != "key")
                .for_each(|(_, x)| shown_strings(x, out)),
            _ => {}
        }
    }
    // An unknown source string passes through as is (by design), so only what
    // the input did not have is compared — our label.
    let g = fx();
    let (_, cases) = run_all_cases(&g);
    for ((name, _, got), case) in cases.into_iter().zip(g["translate_source"].as_array().unwrap()) {
        let mut input = Vec::new();
        shown_strings(&case["input"], &mut input);
        let mut texts = Vec::new();
        shown_strings(&got, &mut texts);
        for text in texts.into_iter().filter(|t| !input.contains(t)) {
            assert!(!has_cyrillic(&text), "{name}: {text}");
        }
    }
}

/// English labels use the words of Steam and of the plugin itself, not a calque.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(l10n::source_name("wallpaper_engine").as_deref(), Some("Steam Workshop"));
    assert_eq!(l10n::wp_type_label("scene"), "Scene");
    assert_eq!(l10n::sort_label("top_rated", "Top rated"), "Top Rated");
    assert_eq!(l10n::sort_label("trend_week", "Trending this week"), "Trending (Week)");
    assert_eq!(l10n::tag_label("Mature"), "18+ (Mature)");
    assert_eq!(l10n::tag_label("Anime"), "Anime");
    assert_eq!(l10n::text("Sign out"), "Sign out");
    // Reverse translation works by the English label too: CG/CGI merge into
    // one, and the last pair wins — as in Russian.
    assert_eq!(l10n::tag_back("CGI"), Some("CGI"));
    assert_eq!(l10n::tag_back("18+ (Mature)"), Some("Mature"));
    let loc = l10n::Localizer::new();
    assert_eq!(loc.untranslate_tags(&["0+ (Everyone)"], "wallhaven"), vec!["Everyone"]);
}

/// Our layer in plugin label resolution (`plugin_l10n::resolve`) speaks
/// Russian word for word as before: «Вход не выполнен», «Выйти».
#[test]
fn plugin_resolve_falls_back_to_our_russian() {
    let _lang = i18n_pin::ru();
    let catalogs = rexon_core::plugin_l10n::Catalogs::new();
    assert_eq!(
        rexon_core::plugin_l10n::resolve(
            &catalogs,
            "org.waywallen.open-wallpaper-engine",
            "Not signed in",
            "",
            "ru_RU"
        ),
        "Вход не выполнен"
    );
    assert_eq!(l10n::text("Sign out"), "Выйти");
    assert_eq!(l10n::text("Status"), "Состояние");
}
