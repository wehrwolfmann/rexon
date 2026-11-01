//! Golden-сверка порта `we_parity.rs` с оригиналом
//! `rexon/qt_wallpaper_engine/parity.py`.
//!
//! Эталон снят прогоном ЖИВОГО Python: для каждого сценария собирался
//! настоящий `RendererPluginListResponse` (protobuf из `proto/control_pb2.py`),
//! прогонялся через `parity.compare_schema` и `parity.render_report_text`, а в
//! `golden/we_parity_cases.json` записаны вход, весь отчёт (вид/renderer/ключ/
//! текст/важность каждого расхождения) и текстовый дамп байт-в-байт. ⛔ Демон
//! не запускался — сверка чистая, по данным.
//!
//! Реестр, таблица типов и набор спец-ключей тоже сняты с живого модуля:
//! добавление настройки в UI обязано ронять этот тест, пока эталон не
//! переснят.
//!
//! Две записи фиксируют осознанное расхождение (⚠ правило #16): чтение
//! `UPSTREAM_TAG` больше не падает на битом байте, а префикс `v` снимается
//! ровно один раз (`lstrip("v")` оригинала съедал их все).
//!
//! ⚠️ 10.10.2026 эталон поправлен РУКОЙ, без Python (он в архиве): настройки
//! рисовальщиков сцен и веб-обоев, которых у движка нет, из расхождений
//! `stale_in_ui` перенесены в `unchecked` («не проверено»). Причина и живые
//! замеры — в сверках `absent_plugin_*` и `live_daemon_*` ниже.

use rexon_core::proto::control as pb;
use rexon_core::we_parity as p;
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/we_parity_cases.json")).unwrap()
}

/// Собрать protobuf-ответ из описания входа в эталоне — тем же способом,
/// каким его собирал генератор на Python.
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

/// Отчёт в той же форме, в какой его сериализовал генератор на Python.
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
    // Порядок значим: он задаёт порядок расхождений stale_in_ui.
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

/// Отдельно — самые «хрупкие» места, чтобы падение было адресным, а не
/// «где-то в сценарии».
#[test]
fn quirks_preserved() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let cases = &g["cases"];

    // Неизвестный тип на зарегистрированном ключе НЕ даёт type_mismatch:
    // сторож `vt in KNOWN_VALUE_TYPES` в оригинале глушит его.
    let r = p::compare_schema(&build(&cases["unknown_type_regular"]["input"]));
    assert_eq!(r.issues.len(), 1);
    assert_eq!(r.issues[0].kind, p::IssueKind::UnknownType);
    assert_eq!(r.issues[0].severity(), "warning");

    // Спец-ключи не ругаются даже на заведомо чужой тип.
    let r = p::compare_schema(&build(&cases["unknown_type_special"]["input"]));
    assert!(r.issues.is_empty(), "спец-ключи должны молчать: {:?}", r.issues);

    // Пустой набор choices у движка сверку не запускает…
    let r = p::compare_schema(&build(&cases["choices_empty_silent"]["input"]));
    assert!(!r.has_drift());
    // …а порядок и дубликаты не важны (сравниваются множества).
    let r = p::compare_schema(&build(&cases["choices_reordered_silent"]["input"]));
    assert!(!r.has_drift());

    // Одинаковые имена renderer'ов: в списке оба, в карте ключей — последний.
    let r = p::compare_schema(&build(&cases["duplicate_renderer_name"]["input"]));
    assert_eq!(r.renderers, vec!["mpv", "mpv"]);
    assert_eq!(r.engine_keys.len(), 1);
    assert_eq!(r.engine_keys_of("mpv"), Some(["volume".to_string()].as_slice()));
    assert_eq!(r.engine_keys_of("weweb"), None);

    // Полная схема без расхождений и пустой ответ — крайние точки.
    assert!(!p::compare_schema(&build(&cases["clean_full"]["input"])).has_drift());
    let empty = p::compare_schema(&build(&cases["empty_response"]["input"]));
    // Без единого рисовальщика «пропавшими» считаются только настройки
    // встроенных; настройки плагинных (сцены, веб) — «не проверено».
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
    // Живой файл rexon/qt_wallpaper_engine/proto/UPSTREAM_TAG, байт в байт.
    assert_eq!(
        p::proto_upstream_tag(Some(&bytes)),
        g["upstream_tag_file"]["value"].as_str().unwrap()
    );
    // Файла нет / файл пуст — тег неизвестен.
    assert_eq!(p::proto_upstream_tag(None), "");
    assert_eq!(p::proto_upstream_tag(Some(b"")), "");
    assert_eq!(p::proto_upstream_tag(Some(b"  \n")), "");

    // ⚠ правило #16: оригинал открывал файл строгим UTF-8 и ловил только
    // OSError, поэтому битый байт ронял всю сверку паритета
    // UnicodeDecodeError'ом (эталон это зафиксировал).
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
    // Входы, где порт СОЗНАТЕЛЬНО расходится: `lstrip("v")` оригинала снимал
    // все ведущие «v», а не префикс, поэтому «vv0.2.6» считался равным
    // «0.2.6». Порт снимает ровно один «v» → эти пары больше не совпадают.
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

    // Живой путь: тег из файла уже без пробелов, версия демона — с «v» и без.
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", "0.2.6"), Some(true));
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", "v0.2.6"), Some(true));
    assert_eq!(p::daemon_matches_snapshot("v0.2.6", "0.2.7"), Some(false));
    // ⚠ правило #16: в живом файле тега рядом с версией стоит хеш коммита
    // апстрима. Оригинал сравнивал строку целиком и всегда докладывал
    // расхождение — бейдж «✓ схема» не мог показаться ни при каких версиях.
    assert_eq!(p::daemon_matches_snapshot("v0.3.8 (8550258)", "0.3.8"), Some(true));
    assert_eq!(p::daemon_matches_snapshot("v0.3.8 (8550258)", "0.3.7"), Some(false));
    // Скобок нет — это часть версии, отрезать её нельзя.
    assert_eq!(p::daemon_matches_snapshot("v0.3.8", "0.3.8 dirty"), Some(false));
    // Сравнить нечем.
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
    // Каталога нет — пустой тег, без паники (как `except OSError` оригинала).
    assert_eq!(p::read_proto_upstream_tag(dir), "");
}

/// Тег снапшота ищется в каталоге ВКЛАДКИ, а не в корне каталога данных.
///
/// Сторож на живом дереве: крейту вкладок сообщают каталог данных целиком
/// (в дереве разработчика — `data/`, в поставке — `rexon/`), а тег лежит на
/// уровень ниже. Пока шаг между ними отсутствовал, отчёт QML-страницы
/// «Разработчик» писал «(тег не записан)» при живом файле на диске
/// (снимок стенда 06.09.2026).
#[test]
fn tag_is_read_from_the_tab_package_inside_rexon() {
    let _lang = i18n_pin::ru();
    let pkg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rexon_core::installer::DEV_TREE_DATA_DIR)
        .canonicalize()
        .expect("каталог данных data/ есть в дереве");
    let pkg = pkg.to_str().expect("путь дерева в UTF-8");
    // Прямо от каталога данных файла нет — именно на это и наступили.
    assert_eq!(p::read_proto_upstream_tag(std::path::Path::new(pkg)), "");
    // С шагом в каталог вкладки — есть, и он непустой.
    let tag = p::read_proto_upstream_tag_from_pkg(pkg);
    assert!(!tag.is_empty(), "UPSTREAM_TAG вкладки не прочитался");
    assert!(tag.starts_with('v'), "тег апстрима пишется как «v0.3.7»: {tag}");
}

/// Отчёт паритета на странице «Разработчик» по-английски: во всех
/// сценариях эталона нет ни одного слова наших русских шаблонов. Проверка
/// по словам, а не по буквам: ключи и группы в эталоне нарочно бывают
/// русскими («мыло», «Эффекты») — это данные движка, а не наш текст.
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
//  Демон без плагина сцен и веб-обоев: «не проверено», а не расхождение
// ─────────────────────────────────────────────────────────────────────────

/// Рисовальщик так, как его отдаёт живой демон: имя, типы обоев, ключи.
fn renderer(name: &str, types: &[&str], keys: &[&str]) -> pb::RendererPluginInfo {
    pb::RendererPluginInfo {
        name: name.to_string(),
        types: types.iter().map(|t| t.to_string()).collect(),
        settings: keys
            .iter()
            .map(|key| pb::SettingSchema {
                key: key.to_string(),
                // Тип — тот, что ждёт реестр: здесь проверяется НАЛИЧИЕ.
                r#type: p::expected_for(key).and_then(|e| e.value_type).unwrap_or(5),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Встроенные рисовальщики демона waywallen 0.5.0 (`plugin.toml` выпуска:
/// с 0.5.0 у image и video есть `custom_extent`).
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

/// Рисовальщики плагина Open Wallpaper Engine 0.3.1 (`plugin.toml` выпуска:
/// с 0.3.1 `custom_extent` есть и у weweb).
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

/// Боевой демон с плагином: всё сошлось и проверено всё.
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

/// Демон БЕЗ плагина (только image/video): четыре настройки сцен и веб-обоев
/// раньше давали «Расхождений: 4». Отдавать их некому — это не расхождение,
/// и отчёт говорит словами, что именно не проверено и почему.
#[test]
fn absent_plugin_settings_are_unchecked_not_differences() {
    let _lang = i18n_pin::ru();
    let r = p::compare_schema(&response(&["image", "video"], builtin_renderers()));
    assert!(r.issues.is_empty(), "расхождения без плагина: {:?}", r.issues);
    assert!(!r.has_drift());
    // `custom_extent` с 0.5.0 отдают image/video — проверен, не в списке.
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

/// Проверка на укус: рисовальщик ЕСТЬ, а настройку не отдаёт — это
/// настоящее расхождение, и послабление для отсутствующих его не глушит.
#[test]
fn present_renderer_missing_its_setting_is_still_a_difference() {
    let _lang = i18n_pin::ru();
    // Тип «web» назван только у самого рисовальщика, не в общем списке.
    let mut all = builtin_renderers();
    all.push(renderer("weweb-renderer", &["web"], &["fps", "shared_texture_enabled"]));
    let r = p::compare_schema(&response(&["image", "video"], all));
    let stale: Vec<&str> = r.issues.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(stale, vec!["remote_debugging_port"]);
    assert_eq!(r.issues[0].kind, p::IssueKind::StaleInUi);
    // Сцен по-прежнему нет — их настройки не проверены.
    assert_eq!(r.unchecked.len(), 1);
    assert_eq!(r.unchecked[0].0, "scene");

    // Тип назван в общем списке, а рисовальщика с настройками нет — тоже
    // расхождение: движок заявил тип, спросить есть с кого.
    // С 0.5.0 `custom_extent` отдают image/video — расхождений три, не четыре.
    let r = p::compare_schema(&response(&["image", "video", "scene", "web"], builtin_renderers()));
    assert_eq!(r.issues.len(), 3);
    assert!(r.unchecked.is_empty());
}
