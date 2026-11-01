//! Golden-сверка порта `remote_l10n.rs` с оригиналом
//! `rexon/qt_wallpaper_engine/qml_ui/remote_l10n.py`.
//!
//! Эталон снят одноразовым генератором с ЖИВОГО Python read-only: `QM_PATH`
//! подменялся на временный каталог, вместо `lrelease` запускался `/bin/true` —
//! ни `.ts`, ни `.qm` в дереве проекта не трогались, Qt не поднимался.
//! Записаны все таблицы словарей, значения функций на живых и вырожденных
//! входах, полный вывод `translate_source` (сериализованный JSON, порядок
//! ключей как у Python-словаря), карты обратного перевода по источникам и
//! содержимое `.ts`, собранного по НАСТОЯЩЕМУ вендорному дереву QML.
//!
//! `golden/remote_l10n_cases.json` — снятые значения. Расхождения с Python —
//! ТОЛЬКО осознанные починки (правило #16, перечислены в шапке модуля), каждая
//! закреплена здесь отдельным тестом с пометкой «правило #16».
//!
//! # Подписи вендорного QML — своего словаря здесь нет
//!
//! Подписи страниц waywallen переводит ВЕНДОРНЫЙ каталог
//! `waywallen/ui/i18n/waywallen_ru.ts`, вендоренный тем же `git diff`-ом, что
//! и `ui/qml`. Своего словаря (`QML_TEXTS`) больше нет: он держал 135 строк из
//! вендорных 486. Поэтому проверки тут три:
//!
//! 1. **Полнота** — каждая строка `qsTr` живого `ui/qml` переведена вендорным
//!    каталогом (`every_vendor_string_has_a_vendor_translation`), и наоборот в
//!    каталоге нет записей, которых `ui/qml` уже не отдаёт.
//! 2. **Свежесть** — закоммиченный `.qm` собран из ЭТОГО `.ts`
//!    (`committed_qm_is_compiled_from_the_vendored_catalog`).
//! 3. **Ничего не потеряно** — каждая строка, которую переводил старый
//!    Python-словарь и которая дожила до сегодняшнего вендора, переведена и
//!    сейчас (`nothing_the_python_dictionary_translated_was_lost`).

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

/// Сверка таблицы-среза со словарём Python: те же пары в том же порядке.
///
/// Значения таблиц — английские msgid; словарь Python — русский. Сверяется
/// ПЕРЕВОД значения из `po/ru.po` (вызывающий держит `i18n_pin::ru()`).
fn assert_table(table: &[(&str, &str)], golden: &Value, name: &str) {
    let obj: Vec<(&String, &Value)> = golden.as_object().unwrap().iter().collect();
    assert_eq!(table.len(), obj.len(), "{name}: разное число записей");
    for ((k, v), (gk, gv)) in table.iter().zip(obj.iter()) {
        assert_eq!(k, gk, "{name}: ключ");
        assert_eq!(i18n::tr(v), s(gv), "{name}: значение для {k}");
    }
}

// ============================================================================
// Таблицы словарей
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
    // `QML_TEXTS` здесь БОЛЬШЕ НЕТ: подписи самого вендорного QML переводит
    // вендорный каталог, а не наш словарь. Что при этом ничего не потерялось,
    // стережёт `nothing_the_python_dictionary_translated_was_lost`.
}

#[test]
fn tags_back_matches_python_comprehension() {
    let _lang = i18n_pin::ru();
    // `{ru: en for en, ru in TAGS.items()}` — при повторе подписи побеждает
    // ПОСЛЕДНЯЯ пара: «Комп. графика» → CGI, а не CG.
    let g = fx();
    let back = g["TAGS_BACK"].as_object().unwrap();
    for (label, en) in back {
        assert_eq!(l10n::tag_back(label), Some(s(en)), "обратный тег {label}");
    }
    // столько же различных подписей, сколько у Python
    let uniq: std::collections::BTreeSet<String> = l10n::TAGS.iter().map(|(_, m)| i18n::tr(m)).collect();
    assert_eq!(uniq.len(), back.len());
    assert_eq!(l10n::tag_back("Комп. графика"), Some("CGI"));
    assert_eq!(l10n::tag_back("Такого нет"), None);
}

// ============================================================================
// Подписи-одиночки
// ============================================================================

#[test]
fn wp_type_label_matches() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for pair in g["wp_type_label"].as_array().unwrap() {
        let p = pair.as_array().unwrap();
        assert_eq!(l10n::wp_type_label(s(&p[0])), s(&p[1]), "вход {}", s(&p[0]));
    }
    // `str(value or "")`: пустая строка остаётся пустой
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
    // TEXTS главнее ACTION_LABELS: «Signed in» есть только в TEXTS
    assert_eq!(l10n::text("Signed in"), "Вход выполнен");
    // а «API key» — только в ACTION_LABELS
    assert_eq!(l10n::text("API key"), "Ключ доступа");
    assert_eq!(l10n::text("Whatever else"), "Whatever else");
}

#[test]
fn sort_and_source_lookups() {
    let _lang = i18n_pin::ru();
    assert_eq!(l10n::sort_label("top_rated", "Top rated"), "Лидеры рейтинга");
    // незнакомая сортировка нового плагина остаётся вендорной подписью
    assert_eq!(l10n::sort_label("brand_new", "Brand new"), "Brand new");
    assert_eq!(l10n::source_name("wallpaper_engine").as_deref(), Some("Мастерская Steam"));
    assert_eq!(l10n::source_name("нет такого"), None);
}

// ============================================================================
// translate_source — полный вывод байт-в-байт
// ============================================================================

/// Прогон всех источников эталона одним переводчиком — как в мосте, где
/// `translate_source` зовётся по очереди для каждого источника выдачи демона.
///
/// ⚠ Эталон снят с ЖИВОГО Python-моста, а тот знал ещё плоское поле
/// `RemoteFilterDef.values` (waywallen ≤0.3.7). С 0.3.8 вариант фильтра —
/// тройка `{value, label, labelText}`, поэтому вход раскладывается
/// [`to_options`], а выдача складывается обратно [`fold_options`]. Иначе
/// пришлось бы переписать эталон руками, и он перестал бы быть снимком.
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

/// Описание источника Python-эпохи → форма waywallen 0.3.8.
///
/// В каждом фильтре `values: ["Anime"]` становится
/// `options: [{"value": "Anime"}]` НА ТОМ ЖЕ месте — порядок ключей в
/// сверке значим. Подписи (`label`) плагин здесь не даёт: в эталоне её и
/// не было.
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

/// Обратное к [`to_options`]: `options` снова становится `values` из подписей.
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

/// Пройти по `filters[]` описания источника и заменить один ключ на другой,
/// сохранив его МЕСТО в карте (сверка идёт строкой JSON).
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
        // «extra_in_sort» — осознанная починка, проверяется отдельным тестом
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
    // незнакомый источник → общая карта (у неё «Комп. графика» = CGI)
    assert_eq!(
        loc.untranslate_tags(&["Комп. графика".to_string()], "нет_такого"),
        vec![s(&g["untranslate_unknown_source"][0])]
    );
}

#[test]
fn cg_cgi_collision_is_resolved_per_source() {
    let _lang = i18n_pin::ru();
    // Ради чего вообще заведены карты по источникам: одна подпись —
    // РАЗНЫЕ исходные теги у Мастерской (CGI) и Wallhaven (CG).
    let g = fx();
    let (loc, _) = run_all_cases(&g);
    let label = ["Комп. графика".to_string()];
    assert_eq!(loc.untranslate_tags(&label, "wallpaper_engine"), vec!["CGI"]);
    assert_eq!(loc.untranslate_tags(&label, "wallhaven"), vec!["CG"]);
}

#[test]
fn numeric_resolution_passes_through_untouched() {
    let _lang = i18n_pin::ru();
    // Числовые разрешения в словаре не нужны: уходят обратно как есть.
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
    // Python на этом падал TypeError («'NoneType' object is not iterable» /
    // «object is not iterable»); здесь тот же случай выражен типом ошибки.
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
    // КВИРК ОРИГИНАЛА СОХРАНЁН НАМЕРЕННО: у полей и строк состояния
    // label/description/placeholder/group/value проставляются даже когда их не
    // было во входе. Вендорный QML отличает undefined от "" — «починка» тут
    // сломала бы страницу.
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
    // По id источника, фильтров и действий и по ключам сортировок ходят и
    // вендорный QML, и демон — их трогать нельзя.
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
                // отсутствующий ключ сортировки Python подставляет пустым —
                // сверяем только то, что источник реально прислал
                let Some(id) = item.get(key) else { continue };
                assert_eq!(got[kind][i].get(key), Some(id), "{name}: {kind}[{i}].{key}");
            }
        }
    }
}

// ── починки правила #16 ────────────────────────────────────────────────

#[test]
fn rule16_sort_entry_keeps_extra_keys() {
    let _lang = i18n_pin::ru();
    // ⚠ Правило #16: Python пересобирал элемент sorts из двух полей и терял
    // всё остальное (`icon`, `order`), хотя filters/actions копируются
    // целиком. Здесь лишние ключи сохраняются, key/label остаются первыми.
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
    // Закрепление поведения: `or` оригинала здесь заменён на проверку ключа
    // (правило #16, защитно). Наблюдаемого расхождения сегодня НЕТ — пустое
    // значение попадает в карту только под пустой подписью, и общая карта на
    // неё тоже ничего не отвечает; тест фиксирует результат обеих версий.
    let mut loc = l10n::Localizer::new();
    loc.translate_source(&serde_json::json!({
        "id": "странный",
        "filters": [{"id": "type", "values": [""]}],
    }))
    .unwrap();
    // источник назвал пустой тег подписью «» — она и уходит обратно пустой
    assert_eq!(loc.untranslate_tags(&["".to_string()], "странный"), vec![""]);
}

#[test]
fn rule16_source_map_is_rebuilt_not_accumulated() {
    let _lang = i18n_pin::ru();
    // ⚠ Правило #16: в Python карта источника копилась вечно (`setdefault`),
    // и после обновления плагина в ней оставались подписи исчезнувших тегов.
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
// Каталог переводов вендорного QML
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
    // ⚠ Исходник каталога больше не «.ts рядом с .qm»: он вендорный и лежит
    // внутри вендорного дерева, рядом с тем самым `ui/qml`, который переводит.
    assert_eq!(
        l10n::vendor_ts_path("/mod"),
        "/mod/waywallen/ui/i18n/waywallen_ru.ts"
    );
    assert_eq!(l10n::DEFAULT_LRELEASE, s(&g["default_lrelease"]));
    // ⚠ Правило #16: жёсткий арчевый путь остался последним запасным
    assert_eq!(l10n::LRELEASE_CANDIDATES.last(), Some(&l10n::DEFAULT_LRELEASE));
}

/// Корень вендорного дерева в рабочей копии.
fn vendor_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/qt_wallpaper_engine/qml_ui")
}

/// Все `.qml` вендорного дерева: (путь, содержимое) — то, что в Python даёт
/// `glob("**/*.qml", recursive=True)` + чтение файла.
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

/// Вендорный каталог переводов: путь и содержимое.
fn vendor_ts() -> (String, String) {
    let path = l10n::vendor_ts_path(&vendor_root().to_string_lossy());
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("вендорный каталог переводов не найден ({path}): {e}"));
    (path, body)
}

/// План сборки на живом вендорном дереве.
fn live_plan() -> (Vec<(String, String)>, l10n::QmBuildPlan) {
    let files = vendor_qml_files();
    let (ts_path, ts) = vendor_ts();
    let plan = l10n::plan_build_qm(&files, &ts, &ts_path, &qm_path_in_tree(), &lrelease());
    (files, plan)
}

/// Куда едет собранный каталог (рядом с модулем UI).
fn qm_path_in_tree() -> String {
    l10n::qm_path(&vendor_root().to_string_lossy())
}

#[test]
fn every_vendor_string_has_a_vendor_translation() {
    let _lang = i18n_pin::ru();
    // Главная проверка: ВСЁ, что вендорный QML отдаёт на перевод, переведено
    // в его же каталоге. Не ноль — значит `ui/qml` и `ui/i18n` вендорены с
    // разных коммитов, и покупатель увидит эти строки по-английски.
    let (files, plan) = live_plan();
    assert!(
        awaiting_upstream_unexpected(&plan, &RU_AWAITING_UPSTREAM).is_empty(),
        "{}",
        vendor_drift_report(&plan)
    );
    // и это не «почти всё»: своего словаря на вендорные подписи было 135
    // строк, вендорный каталог закрывает всё дерево — на порядок больше.
    let all = l10n::vendor_sources(&files).len();
    assert!(all > 350, "вендорных строк всего {all} — дерево не то?");
}

/// Строки каталога, которых в `ui/qml` нет и не будет: вендор объявляет их в
/// своём C++ (`ui/src`), а мы вендорим только `ui/qml` — весь C++ у нас
/// переписан на Rust.
///
/// Список ЗАКРЫТЫЙ и поимённый: он не «разрешает расхождение вообще», а
/// называет каждую строку и того, кто её показывает у нас. Строку сюда
/// можно добавлять, только назвав наш модуль-потребитель.
///
/// * «Scheme color», «Playback speed» — подписи предопределённых свойств
///   обои. У вендора их выдаёт `ui/src/model/user_property_model.cpp`
///   (с 0.3.8 через `tr()`), у нас — `rexon-gui/src/user_property_model.rs`.
/// * «Mouse parallax» — третья такая подпись (параллакс от мыши, 0.4.2);
///   в русский каталог вендора запись приехала с v0.4.4. Потребитель тот
///   же — `rexon-gui/src/user_property_model.rs`.
///
const CATALOG_ONLY_IN_VENDOR_CPP: [&str; 3] =
    ["Scheme color", "Playback speed", "Mouse parallax"];

/// Меню значка в лотке демона (контекст `TrayMenu`, у вендора
/// `ui/src/tray_i18n.cpp`, с 0.5.0). У Rexon лотка демона НЕТ: Rexon
/// запускает его с `--no-tray` (`rexon_core::tray`), язык меню лотка не
/// передаётся (`DaemonDBusClient.setTrayLanguage` не нужен), показать эти
/// строки некому. Закрытый поимённый список, как выше. «Mute», «Pause» и
/// «Shuffle» из того же меню сюда не входят: эти слова зовёт и разметка.
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

/// Вендорные строки 0.5.0, которых в РУССКОМ каталоге апстрима ещё нет
/// (`type="unfinished"`): умные плейлисты и фильтр «Библиотека». Перевод
/// чужой программы правится у неё — эти строки идут ПОДАЧЕЙ в waywallen, а
/// не нашим патчем. Список закрытый и сам себя снимает: как только апстрим
/// переведёт строку, сверка потребует убрать её отсюда.
const RU_AWAITING_UPSTREAM: [&str; 13] = [
    "Add group",
    "Clear rules",
    "Create smart playlist",
    "Default order",
    "Library",
    "No manual playlists found",
    "Select library",
    "Smart playlist",
    "Unavailable library #%1",
    "Waiting for matching wallpapers",
    "Wallpapers matching these filters are updated automatically.",
    "excludes",
    "includes",
];

/// То же для НЕМЕЦКОГО каталога апстрима (с 0.5.0): те же строки умных
/// плейлистов и фильтра «Библиотека» плюс вторая строка версии на «О
/// программе». Немецкий перевод Rexon делается С РУССКОГО оригинала — и
/// подача в waywallen тоже.
const DE_AWAITING_UPSTREAM: [&str; 14] = [
    "Add group",
    "Clear rules",
    "Create smart playlist",
    "Daemon version %1",
    "Default order",
    "Library",
    "No manual playlists found",
    "Select library",
    "Smart playlist",
    "Unavailable library #%1",
    "Waiting for matching wallpapers",
    "Wallpapers matching these filters are updated automatically.",
    "excludes",
    "includes",
];

#[test]
fn every_vendor_string_has_a_german_translation() {
    // Немецкий каталог апстрима собирается в свой `.qm` и показывается
    // немецкому Rexon — пробелы в нём видны покупателю так же, как в русском.
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

/// Непереведённые строки плана, которых НЕТ в списке ждущих апстрима.
/// Обратная сторона проверяется тут же: строка из списка, которую апстрим
/// уже перевёл, — повод вычеркнуть её (иначе список протухнет).
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
    // Обратный перекос: запись каталога, которой в `ui/qml` уже нет. Сам по
    // себе безвреден, но это ровно тот же симптом — `qml` и `i18n` вендорены
    // с разных коммитов, и в другую сторону перекос уже вредит.
    let (_, plan) = live_plan();
    let stale: Vec<&String> = plan
        .stale
        .iter()
        .filter(|s| !CATALOG_ONLY_IN_VENDOR_CPP.contains(&s.as_str()))
        .filter(|s| !CATALOG_TRAY_NOT_SHOWN.contains(&s.as_str()))
        .collect();
    assert!(stale.is_empty(), "{}", vendor_drift_report(&plan));
    // Список исключений не должен протухнуть: каждая строка обязана в
    // каталоге БЫТЬ, иначе она давно не нужна и её пора выкинуть отсюда.
    for extra in CATALOG_ONLY_IN_VENDOR_CPP
        .iter()
        .chain(CATALOG_TRAY_NOT_SHOWN.iter())
    {
        assert!(
            plan.stale.iter().any(|s| s == *extra),
            "исключение «{extra}» каталогу больше не нужно — снять его"
        );
    }
}

/// Что сказать человеку, когда каталог разошёлся с вендорным QML.
///
/// Просто «не совпало» бесполезно: непонятно, ЧТО разошлось и что делать.
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
        for lost in plan.untranslated.iter().take(20) {
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

/// Первый доступный `lrelease` — как в `remote_l10n._find_lrelease()`.
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

/// ⚠️ Живая машина нужна ПО СУЩЕСТВУ: сверка запускает настоящий
/// `lrelease` и сравнивает его вывод с закоммиченным `.qm` байт в байт.
/// Подделать сборщик каталога нечем — в этом и вопрос. Под
/// `cargo test-core-isolated` пропускается с причиной.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: запускает настоящий lrelease")]
fn committed_qm_is_compiled_from_the_vendored_catalog() {
    // Каталог, который едет в сборку, обязан быть собран из ТОГО ЖЕ вендорного
    // `.ts`, что лежит в дереве: иначе обновление вендора тихо оставляет
    // покупателю старые подписи. Собираем во временный файл и сверяем байты.
    // Каталогов столько, сколько вендорено `.ts` (с 0.5.0 — ru и de), и
    // каждый обязан лежать собранным рядом с модулем: без `.qm` язык не
    // попадёт в список языков вкладки, и страницы останутся английскими.
    let lrelease = lrelease();
    if !Path::new(&lrelease).exists() {
        eprintln!("lrelease не найден ({lrelease}) — сверка собранного каталога пропущена");
        return;
    }
    let root = vendor_root().to_string_lossy().into_owned();
    let codes = vendor_catalogs();
    assert!(
        codes.iter().any(|c| c == "ru") && codes.iter().any(|c| c == "de"),
        "вендорных каталогов: {codes:?} — ждали ru и de"
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

/// Языки вендорных каталогов `.ts` в дереве.
fn vendor_catalogs() -> Vec<String> {
    let dir = vendor_root().join(l10n::VENDOR_I18N_REL);
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("нет каталога переводов вендора {dir:?}: {e}"))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    l10n::vendor_catalog_languages(&names)
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
    // прежний путь русского каталога — частный случай общего
    assert_eq!(l10n::vendor_ts_path_for("/mod", "ru"), l10n::vendor_ts_path("/mod"));
    assert_eq!(l10n::qm_path_for("/mod", "ru"), l10n::qm_path("/mod"));
}

#[test]
fn nothing_the_python_dictionary_translated_was_lost() {
    let _lang = i18n_pin::ru();
    // Своего словаря больше нет — но ни одна строка, которую он переводил, не
    // должна остаться без перевода. Эталон `build_qm_ts_real` снят с живого
    // Python на вендорном дереве v0.2.6+9: берём из него все `<source>` и
    // требуем, чтобы каждая, что дожила до сегодняшнего `ui/qml`, была
    // переведена вендорным каталогом.
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
            continue; // вендор убрал строку — переводить нечего
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
    // Формы множественного числа считает Qt по языку каталога: для русского их
    // три (1 / 2–4 / 5 и больше). Меньше форм — lrelease ругается, часть чисел
    // останется без перевода.
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
    // Счётчик обоев в плейлисте — единственная строка, где перевод зависит от
    // числа. Она стоит на двух вендорных страницах, и обе обязаны быть в
    // каталоге, иначе одна из них покажет английское «3 wallpapers».
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
    // ⚠ Правило #16: Python видел только `qsTr("…")` — ни одинарных кавычек
    // (вендор пишет `qsTr('Status')` в `Window.qml`), ни вызова со счётчиком
    // `qsTr("…", "", n)`. Такая строка молча оставалась без перевода.
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
    // и обе формы вендор действительно использует
    let live = vendor_qml_files();
    assert!(live.iter().any(|(_, body)| body.contains("qsTr('")));
    assert!(live.iter().any(|(_, body)| body.contains(", \"\", ")));
}

#[test]
fn vendor_sources_reads_qs_translate() {
    // waywallen 0.5.0: `qsTranslate("Контекст", "…")` — строка ВТОРЫМ
    // аргументом. Выражение вместо литерала и чужие функции — мимо.
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
    // Незавершённый перевод переводом НЕ считается: Qt отдаст исходную строку,
    // и покупатель увидит английский. Спецсимволы XML разбираются обратно —
    // иначе строка `A & B` из QML никогда не совпадёт с записью каталога.
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
    // перекос виден в обе стороны
    assert_eq!(plan.untranslated, vec!["Gone"]);
    assert_eq!(plan.stale, vec!["Old"]);
}

// ============================================================================
// Вырожденные входы (в Python — «or» и str())
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
    // пустое описание источника: словари всё равно проставляются
    let mut loc = l10n::Localizer::new();
    let out = loc.translate_source(&Value::Object(Map::new())).unwrap();
    assert_eq!(
        serde_json::to_string(&out).unwrap(),
        r#"{"sorts":[],"tags":[],"filters":[],"actions":[],"status":[]}"#
    );
}

// ============================================================================
// Язык интерфейса вендорных страниц (ui_language.cpp оригинала)
// ============================================================================

#[test]
fn ui_language_codes_from_catalogs() {
    // Ровно то, что лежит рядом с модулем: одно имя наше, остальные мимо.
    let names = [
        "waywallen_ru.qm".to_string(),
        "waywallen_zh_CN.qm".to_string(),
        // английский каталог отбрасывается — исходные строки и так его
        "waywallen_en.qm".to_string(),
        "waywallen_en_GB.qm".to_string(),
        // не наши файлы
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
    // «как в системе» и английский принимаются всегда
    assert_eq!(
        l10n::normalize_ui_language("system", &codes).as_deref(),
        Some("system")
    );
    assert_eq!(
        l10n::normalize_ui_language("en_US", &codes).as_deref(),
        Some("en")
    );
    // точное совпадение
    assert_eq!(
        l10n::normalize_ui_language("zh_CN", &codes).as_deref(),
        Some("zh_CN")
    );
    // по базовому языку в обе стороны
    assert_eq!(
        l10n::normalize_ui_language("ru_RU.UTF-8", &codes).as_deref(),
        Some("ru")
    );
    assert_eq!(
        l10n::normalize_ui_language("zh", &codes).as_deref(),
        Some("zh_CN")
    );
    // неподдерживаемый и пустой — отказ (у оригинала `setUiLanguage` → false)
    assert!(l10n::normalize_ui_language("de", &codes).is_none());
    assert!(l10n::normalize_ui_language("", &codes).is_none());
}

#[test]
fn ui_language_plan() {
    let codes = vec!["ru".to_string()];

    // «как в системе»: локаль системы решает каталог, но каталог не обязателен
    let system = l10n::plan_ui_language("system", "ru_RU", &codes).unwrap();
    assert_eq!(system.preference, "system");
    assert_eq!(system.locale_name, "");
    assert_eq!(system.qm_file, "waywallen_ru.qm");
    assert!(!system.required);

    // система английская — каталога нет, и это НЕ ошибка
    let system_en = l10n::plan_ui_language("system", "en_US", &codes).unwrap();
    assert_eq!(system_en.qm_file, "");
    assert!(!system_en.required);

    // система немецкая, каталога нет — тоже исходные строки
    let system_de = l10n::plan_ui_language("system", "de_DE", &codes).unwrap();
    assert_eq!(system_de.qm_file, "");

    // английский: каталог не нужен, строки исходные
    let english = l10n::plan_ui_language("en", "ru_RU", &codes).unwrap();
    assert_eq!(english.preference, "en");
    assert_eq!(english.locale_name, "en");
    assert_eq!(english.qm_file, "");
    assert!(!english.required);

    // русский: каталог обязателен
    let russian = l10n::plan_ui_language("ru_RU", "en_US", &codes).unwrap();
    assert_eq!(russian.preference, "ru");
    assert_eq!(russian.locale_name, "ru");
    assert_eq!(russian.qm_file, "waywallen_ru.qm");
    assert!(russian.required);

    // языка нет — плана нет
    assert!(l10n::plan_ui_language("de", "ru_RU", &codes).is_none());
}

// ============================================================================
// Язык интерфейса: английский исходник, русский — как было
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// На английском интерфейсе ни одна подпись каталогов не выходит по-русски.
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
    // И полный перевод описания источника из эталона — тоже.
    // Идентификаторы (`id`, `key`) не переводятся по построению — в эталоне
    // есть и кириллический `id` фильтра, он обязан пройти как был.
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
    // Незнакомая строка источника проходит как была (так задумано), поэтому
    // сверяется только то, чего во входе не было, — наша подпись.
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

/// Английские подписи — словами Steam и самого плагина, а не калькой.
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
    // Обратный перевод работает и по английской подписи: CG/CGI сходятся
    // в одну, и побеждает последняя пара — как по-русски.
    assert_eq!(l10n::tag_back("CGI"), Some("CGI"));
    assert_eq!(l10n::tag_back("18+ (Mature)"), Some("Mature"));
    let loc = l10n::Localizer::new();
    assert_eq!(loc.untranslate_tags(&["0+ (Everyone)"], "wallhaven"), vec!["Everyone"]);
}

/// Наш слой в разрешении подписи плагина (`plugin_l10n::resolve`) говорит
/// по-русски дословно как раньше: «Вход не выполнен», «Выйти».
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
