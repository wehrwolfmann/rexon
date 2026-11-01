//! Сверка порта `we_wallpaper.rs` с оригиналом waywallen —
//! `ui/src/query/wallpaper_query.cpp` (`WallpaperListQuery`,
//! `WallpaperGetQuery`) и вендоренным QML страницы «Обои».
//!
//! Проверяется: набор ключей карты записи (QML читает именно их — список
//! сверен грепом по `ui/qml`), порядок сигналов сеттеров, молчание при
//! повторе того же значения, ветка ошибки демона, битый кадр и КРУГ —
//! правила фильтра/сортировки уезжают в запрос теми же, какими пришли из
//! настроек демона. Кадры собираются prost'ом, живой демон не дёргается.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_query as q;
use rexon_core::we_settings as s;
use rexon_core::we_wallpaper as w;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Кадр ответа: статус OK и полезная нагрузка.
fn ok_frame(payload: pb::response::Payload) -> Vec<u8> {
    frame(pb::Status::Ok as i32, String::new(), Some(payload))
}

fn frame(status: i32, message: String, payload: Option<pb::response::Payload>) -> Vec<u8> {
    pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 11,
            status,
            message,
            error_code: 0,
            payload,
        })),
    }
    .encode_to_vec()
}

fn list_frame(wallpapers: Vec<pb::WallpaperEntry>, count: u32) -> Vec<u8> {
    ok_frame(pb::response::Payload::WallpaperList(
        pb::WallpaperListResponse { wallpapers, count },
    ))
}

/// Полностью заполненная запись: на ней проверяется, что ни одно поле
/// `WallpaperEntry` не потерялось по дороге в QML.
fn full_entry() -> pb::WallpaperEntry {
    pb::WallpaperEntry {
        id: "wp-42".into(),
        name: "Закат над морем".into(),
        wp_type: "scene".into(),
        resource: "/home/wehrwolfmann/обои/закат/project.json".into(),
        preview: "/home/wehrwolfmann/обои/закат/preview.gif".into(),
        metadata: HashMap::from([
            ("источник".to_string(), "workshop".to_string()),
            ("автор".to_string(), "kto-to".to_string()),
        ]),
        size: 123_456_789,
        width: 1920,
        height: 1080,
        content_rating: "everyone".into(),
        tags: vec!["природа".into(), "море".into()],
        user_properties_schema: "{\"u_speed\":{}}".into(),
        user_property_overrides: "{\"u_speed\":\"2\"}".into(),
        description: "Волны и чайки".into(),
        external_id: "1234567890".into(),
        web_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=1234567890"
            .into(),
        wallpaper_layout_override: Some(pb::LayoutPrefs {
            fillmode: 2,
            align: 3,
            rotation: 4,
            location_x: 30,
            location_y: 70,
            location_set: true,
            flip: 0,
        }),
        wallpaper_layout_override_set: true,
        supports_item_remove: true,
        supports_item_unsubscribe: true,
        // waywallen 0.4.4: флаг «скрыта». В карту QML не идёт — разметка
        // v0.4.3 его не читает; приедет с приёмом разметки v0.4.4.
        hidden: false,
    }
}

// ── карта записи библиотеки ─────────────────────────────────────────────

#[test]
fn entry_map_keys_match_qt_protobuf_gadget_properties() {
    let m = w::entry_to_map(&full_entry());
    let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
    keys.sort_unstable();
    // Все свойства гаджета `control::v1::WallpaperEntry`: json_name полей
    // proto, кроме `id` — его генератор Qt Protobuf зовёт `id_proto`.
    let mut expected = vec![
        "id_proto",
        "name",
        "wpType",
        "resource",
        "preview",
        "metadata",
        "size",
        "width",
        "height",
        "contentRating",
        "tags",
        "userPropertiesSchema",
        "userPropertyOverrides",
        "description",
        "externalId",
        "wallpaperLayoutOverride",
        "wallpaperLayoutOverrideSet",
        "supportsItemRemove",
        "supportsItemUnsubscribe",
        "webUrl",
        // waywallen 0.4.4: признак «скрыта».
        "hidden",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected);
}

#[test]
fn entry_map_carries_every_field_value() {
    let m = w::entry_to_map(&full_entry());
    assert_eq!(m["id_proto"], json!("wp-42"));
    assert_eq!(m["name"], json!("Закат над морем"));
    assert_eq!(m["wpType"], json!("scene"));
    assert_eq!(
        m["resource"],
        json!("/home/wehrwolfmann/обои/закат/project.json")
    );
    assert_eq!(
        m["preview"],
        json!("/home/wehrwolfmann/обои/закат/preview.gif")
    );
    assert_eq!(
        m["metadata"],
        json!({"автор": "kto-to", "источник": "workshop"})
    );
    assert_eq!(m["size"], json!(123_456_789_i64));
    assert_eq!(m["width"], json!(1920));
    assert_eq!(m["height"], json!(1080));
    assert_eq!(m["contentRating"], json!("everyone"));
    assert_eq!(m["tags"], json!(["природа", "море"]));
    assert_eq!(m["userPropertiesSchema"], json!("{\"u_speed\":{}}"));
    assert_eq!(m["userPropertyOverrides"], json!("{\"u_speed\":\"2\"}"));
    assert_eq!(m["description"], json!("Волны и чайки"));
    assert_eq!(m["externalId"], json!("1234567890"));
}

/// Дефект прежнего Python-моста: панель деталей читает эти три ключа, а
/// карта их не несла — блок «Layout override» показывал раскладку по
/// умолчанию, кнопка сброса и действие «Отписаться» были вечно серыми.
#[test]
fn entry_map_has_layout_override_and_unsubscribe_flags() {
    let m = w::entry_to_map(&full_entry());
    assert_eq!(m["wallpaperLayoutOverrideSet"], json!(true));
    assert_eq!(m["supportsItemUnsubscribe"], json!(true));
    assert_eq!(m["supportsItemRemove"], json!(true));
    // Ключи вложенной раскладки — те же, что у layout_to_map настроек,
    // плюс `flip` (v0.4.4): WallpaperDetailPanel читает
    // fillmode/locationX/locationY/rotation/flip.
    let mut want = s::layout_to_map(&pb::LayoutPrefs {
        fillmode: 2,
        align: 3,
        rotation: 4,
        location_x: 30,
        location_y: 70,
        location_set: true,
        flip: 0,
    });
    want.insert("flip".into(), json!(0));
    assert_eq!(m["wallpaperLayoutOverride"], Value::Object(want));
}

#[test]
fn entry_map_layout_override_present_even_when_unset() {
    // Гаджет Qt-Protobuf отдаёт неустановленное сообщение с полями по
    // умолчанию, и QML берёт `wallpaperLayoutOverride` безусловно —
    // форма карты не должна прыгать.
    let m = w::entry_to_map(&pb::WallpaperEntry::default());
    assert_eq!(m["wallpaperLayoutOverrideSet"], json!(false));
    let mut want = s::layout_to_map(&pb::LayoutPrefs::default());
    want.insert("flip".into(), json!(0));
    assert_eq!(m["wallpaperLayoutOverride"], Value::Object(want));
}

#[test]
fn entry_map_metadata_order_is_stable() {
    // Порядок HashMap проto недетерминирован: без сортировки одна и та же
    // запись выглядела бы «изменившейся» и дёргала сброс модели.
    let mut e = pb::WallpaperEntry::default();
    for i in 0..32 {
        e.metadata.insert(format!("k{i}"), format!("v{i}"));
    }
    let first = w::entry_to_map(&e);
    for _ in 0..8 {
        assert_eq!(w::entry_to_map(&e), first);
    }
}

// ── сеттеры свойств ─────────────────────────────────────────────────────

fn prop(name: &'static str) -> q::QuerySignal {
    q::QuerySignal::Property(name)
}

#[test]
fn scalar_setters_emit_own_signal_once() {
    let mut d = w::WallpaperListData::default();
    assert_eq!(
        w::set_property(&mut d, "wpType", &json!("scene")).unwrap(),
        vec![prop("wpType")]
    );
    assert_eq!(d.wp_type, "scene");
    // Повтор того же значения молчит (сеттеры оригинала сравнивают).
    assert!(w::set_property(&mut d, "wpType", &json!("scene"))
        .unwrap()
        .is_empty());

    assert_eq!(
        w::set_property(&mut d, "searchText", &json!("закат")).unwrap(),
        vec![prop("searchText")]
    );
    assert!(w::set_property(&mut d, "searchText", &json!("закат"))
        .unwrap()
        .is_empty());
}

#[test]
fn unknown_property_is_rejected() {
    let mut d = w::WallpaperListData::default();
    assert!(w::set_property(&mut d, "offset", &json!(1)).is_none());
    // Список имён, который читает шим, — тот же самый.
    for name in w::LIST_PROPERTIES {
        assert!(w::set_property(&mut d, name, &Value::Null).is_some());
    }
}

#[test]
fn sorts_setter_converts_rules_and_is_silent_on_repeat() {
    let mut d = w::WallpaperListData::default();
    let rules = json!([{"key": 3, "direction": 2}]);
    assert_eq!(
        w::set_property(&mut d, "sorts", &rules).unwrap(),
        vec![prop("sorts")]
    );
    assert_eq!(
        d.sorts,
        vec![pb::WallpaperSortRule {
            key: 3,
            direction: 2
        }]
    );
    assert!(w::set_property(&mut d, "sorts", &rules).unwrap().is_empty());
}

#[test]
fn filters_setter_order_is_filters_then_state_then_has_active() {
    let mut d = w::WallpaperListData::default();
    let rules = json!([{
        "type": 1, "group": 0,
        "stringFilter": {"value": "закат", "condition": 2}
    }]);
    // setFilters оригинала: filtersChanged → filterStateChanged →
    // hasOtherFiltersChanged (0.5.0, подписан на filterStateChanged) →
    // hasActiveFiltersChanged (оба — только когда признак перевернулся).
    assert_eq!(
        w::set_property(&mut d, "filters", &rules).unwrap(),
        vec![
            prop("filters"),
            prop("filterState"),
            prop("hasOtherFilters"),
            prop("hasActiveFilters")
        ]
    );
    assert!(d.has_active_filters());
    // Повтор — молчание целиком, включая filterStateChanged (иначе
    // страница перезагружала бы список впустую).
    assert!(w::set_property(&mut d, "filters", &rules)
        .unwrap()
        .is_empty());
    // Снятие последнего правила снова переворачивает признак.
    assert_eq!(
        w::set_property(&mut d, "filters", &json!([])).unwrap(),
        vec![
            prop("filters"),
            prop("filterState"),
            prop("hasOtherFilters"),
            prop("hasActiveFilters")
        ]
    );
    assert!(!d.has_active_filters());
}

#[test]
fn filter_logics_setter_does_not_touch_has_active_filters() {
    let mut d = w::WallpaperListData::default();
    // hasActiveFilters оригинала логику групп не учитывает.
    assert_eq!(
        w::set_property(&mut d, "filterLogics", &json!([{"op": 1, "groupA": 0, "groupB": 1}]))
            .unwrap(),
        vec![prop("filterLogics"), prop("filterState")]
    );
    assert!(!d.has_active_filters());
    assert_eq!(
        d.filter_logics,
        vec![pb::FilterLogic {
            op: 1,
            group_a: 0,
            group_b: 1
        }]
    );
}

/// Дефект прежнего Python-моста: быстрые списки меняли `hasActiveFilters`,
/// но сигнал не слали — подсветка кнопки «Фильтры» обновлялась лишь как
/// побочный эффект успешного ответа демона, а при ошибке не обновлялась.
#[test]
fn quick_filter_setters_emit_has_active_filters_on_flip() {
    for (name, value) in [
        ("skipTypes", json!(["video"])),
        ("filterTags", json!(["природа"])),
        ("skipContentRatings", json!(["mature"])),
    ] {
        let mut d = w::WallpaperListData::default();
        assert_eq!(
            w::set_property(&mut d, name, &value).unwrap(),
            vec![prop(name), prop("hasOtherFilters"), prop("hasActiveFilters")],
            "свойство {name}"
        );
        assert!(d.has_active_filters());
        // Второй такой же список — молчание.
        assert!(w::set_property(&mut d, name, &value).unwrap().is_empty());
        // Возврат к пустому — признак перевернулся обратно.
        assert_eq!(
            w::set_property(&mut d, name, &json!([])).unwrap(),
            vec![prop(name), prop("hasOtherFilters"), prop("hasActiveFilters")]
        );
        assert!(!d.has_active_filters());
    }
}

#[test]
fn quick_filter_setter_is_quiet_when_flag_does_not_flip() {
    let mut d = w::WallpaperListData::default();
    w::set_property(&mut d, "skipTypes", &json!(["video"])).unwrap();
    // Признак уже поднят пропуском типов — второй сигнал не нужен ни при
    // добавлении другого быстрого фильтра, ни при правке того же списка.
    assert_eq!(
        w::set_property(&mut d, "filterTags", &json!(["море"])).unwrap(),
        vec![prop("filterTags")]
    );
    assert_eq!(
        w::set_property(&mut d, "skipTypes", &json!(["video", "web"])).unwrap(),
        vec![prop("skipTypes")]
    );
    // И снятие одного из двух признак тоже не переворачивает.
    assert_eq!(
        w::set_property(&mut d, "skipTypes", &json!([])).unwrap(),
        vec![prop("skipTypes")]
    );
    assert!(d.has_active_filters());
}

// ── replaceFilterState ──────────────────────────────────────────────────

#[test]
fn replace_filter_state_emits_single_state_signal() {
    let mut d = w::WallpaperListData::default();
    let filters = json!([{"type": 1, "group": 0, "stringFilter": {"value": "a", "condition": 1}}]);
    let logics = json!([{"op": 1, "groupA": 0, "groupB": 1}]);
    let (changed, sig) = w::replace_filter_state(&mut d, &filters, &logics);
    assert!(changed);
    assert_eq!(
        sig,
        vec![
            prop("filters"),
            prop("filterLogics"),
            prop("filterState"),
            prop("hasOtherFilters"),
            prop("hasActiveFilters"),
        ]
    );
    // Повтор — «ничего не изменилось», страница по false не перезагружает.
    let (changed, sig) = w::replace_filter_state(&mut d, &filters, &logics);
    assert!(!changed);
    assert!(sig.is_empty());
}

#[test]
fn replace_filter_state_reports_only_changed_half() {
    let mut d = w::WallpaperListData::default();
    let filters = json!([{"type": 1, "group": 0, "stringFilter": {"value": "a", "condition": 1}}]);
    w::replace_filter_state(&mut d, &filters, &json!([]));
    // Меняется только логика групп: сигнал правил не летит, признак
    // активных фильтров не переворачивался.
    let (changed, sig) = w::replace_filter_state(
        &mut d,
        &filters,
        &json!([{"op": 2, "groupA": 0, "groupB": 1}]),
    );
    assert!(changed);
    assert_eq!(sig, vec![prop("filterLogics"), prop("filterState")]);
}

// ── запрос ──────────────────────────────────────────────────────────────

#[test]
fn request_carries_every_property_and_no_pagination() {
    let mut d = w::WallpaperListData::default();
    w::set_property(&mut d, "wpType", &json!("scene")).unwrap();
    w::set_property(&mut d, "searchText", &json!("закат")).unwrap();
    w::set_property(&mut d, "skipTypes", &json!(["video"])).unwrap();
    w::set_property(&mut d, "filterTags", &json!(["природа"])).unwrap();
    w::set_property(&mut d, "skipContentRatings", &json!(["mature"])).unwrap();
    w::set_property(&mut d, "sorts", &json!([{"key": 3, "direction": 2}])).unwrap();
    w::set_property(&mut d, "filterLogics", &json!([{"op": 1, "groupA": 0, "groupB": 1}])).unwrap();
    w::set_property(
        &mut d,
        "filters",
        &json!([
            {"type": 1, "group": 0, "stringFilter": {"value": "закат", "condition": 2}},
            {"type": 6, "group": 1, "intFilter": {"value": 1080, "condition": 5}},
        ]),
    )
    .unwrap();

    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    assert_eq!(req.wp_type, "scene");
    assert_eq!(req.search_text, "закат");
    assert_eq!(req.skip_types, vec!["video".to_string()]);
    assert_eq!(req.filter_tags, vec!["природа".to_string()]);
    assert_eq!(req.skip_content_ratings, vec!["mature".to_string()]);
    assert_eq!(req.sorts.len(), 1);
    assert_eq!(req.filters.len(), 2);
    assert_eq!(req.filter_logics.len(), 1);
    // initReqForReload при limit = 0: страница нулевая, размер страницы 0 —
    // по протоколу «без постраничности, отдать всё».
    assert_eq!(req.page, 0);
    assert_eq!(req.page_size, 0);
}

/// КРУГ: правила приходят на страницу из настроек демона
/// (`settings_get` → `wallpaperFilters`/`wallpaperSorts`) и уезжают в
/// запрос списка. Карта — единственное представление между ними, и она
/// обязана донести правила без потерь.
#[test]
fn rules_survive_round_trip_from_settings_to_request() {
    let sorts = vec![pb::WallpaperSortRule {
        key: pb::WallpaperSortKey::Size as i32,
        direction: pb::SortDirection::Desc as i32,
    }];
    let filters = vec![
        pb::WallpaperFilterRule {
            r#type: pb::WallpaperFilterType::Name as i32,
            group: 0,
            payload: Some(pb::wallpaper_filter_rule::Payload::StringFilter(
                pb::WallpaperStringFilter {
                    value: "закат".into(),
                    condition: pb::StringCondition::Contains as i32,
                },
            )),
        },
        pb::WallpaperFilterRule {
            r#type: pb::WallpaperFilterType::Width as i32,
            group: 1,
            payload: Some(pb::wallpaper_filter_rule::Payload::IntFilter(
                pb::WallpaperIntFilter {
                    value: 1920,
                    condition: pb::IntCondition::GreaterEqual as i32,
                },
            )),
        },
        pb::WallpaperFilterRule {
            r#type: pb::WallpaperFilterType::Tag as i32,
            group: 2,
            payload: Some(pb::wallpaper_filter_rule::Payload::TagFilter(
                pb::WallpaperTagFilter {
                    values: vec!["аниме".into(), "море".into()],
                    condition: pb::StringCondition::IsNot as i32,
                },
            )),
        },
        // Свежая строка редактора фильтров — правило без payload.
        pb::WallpaperFilterRule {
            r#type: pb::WallpaperFilterType::Unspecified as i32,
            group: 3,
            payload: None,
        },
    ];
    let logics = vec![pb::FilterLogic {
        op: pb::LogicOp::Or as i32,
        group_a: 0,
        group_b: 1,
    }];

    // Настройки → карты (ровно то, что QML отдаёт нашим сеттерам).
    let global = s::global_to_map(&pb::GlobalSettings {
        wallpaper_sorts: sorts.clone(),
        wallpaper_filters: filters.clone(),
        wallpaper_filter_logics: logics.clone(),
        ..Default::default()
    });

    let mut d = w::WallpaperListData::default();
    w::set_property(&mut d, "sorts", &global["wallpaperSorts"]).unwrap();
    let (changed, _) = w::replace_filter_state(
        &mut d,
        &global["wallpaperFilters"],
        &global["wallpaperFilterLogics"],
    );
    assert!(changed);

    // Карта донесла правила без потерь — вместе со свежей строкой…
    assert_eq!(d.filters, filters);
    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    assert_eq!(req.sorts, sorts);
    // …и на провод они уходят ВСЕ, как у оригинала: незаполненное правило
    // демон 0.5.0 отвергает сам, и страница показывает его ошибку.
    assert_eq!(req.filters, filters);
    assert_eq!(req.filter_logics, logics);
}

/// Запрос несёт правила, логики и сортировки КАК ЕСТЬ — паритет с
/// `listRequest` оригинала. Негодное (свежая строка редактора, условие
/// «любое», библиотека с «больше», логика-сирота) демон 0.5.0 отвергает
/// сам (`InvalidArgument` «invalid wallpaper selection»), и его ошибку
/// показывает страница. 24.10.2026 Rexon такие правила молча выкидывал —
/// и вместе с порчей гаджетов в шиме это стирало ВСЕ фильтры умного
/// плейлиста без единого слова.
#[test]
fn request_carries_rules_as_is_like_upstream() {
    let mut d = w::WallpaperListData::default();
    let filters = json!([
        {"type": 1, "group": 0, "stringFilter": {"value": "лес", "condition": 3}},
        // свежая строка редактора: тип 0, без нагрузки
        {"type": 0, "group": 1},
        // условие «любое» (STRING_CONDITION_UNSPECIFIED)
        {"type": 1, "group": 2, "stringFilter": {"value": "x", "condition": 0}},
        {"type": 3, "group": 3, "intFilter": {"value": 4, "condition": 5}},
        {"type": 3, "group": 4, "intFilter": {"value": 0, "condition": 1}},
        {"type": 3, "group": 5, "intFilter": {"value": 7, "condition": 2}},
        {"type": 5, "group": 6, "stringFilter": {"value": "1", "condition": 1}},
    ]);
    w::set_property(&mut d, "filters", &filters).unwrap();
    w::set_property(
        &mut d,
        "filterLogics",
        &json!([
            {"op": 2, "groupA": 0, "groupB": 5},
            {"op": 1, "groupA": 8, "groupB": 9},
            {"op": 0, "groupA": 0, "groupB": 5},
        ]),
    )
    .unwrap();
    w::set_property(
        &mut d,
        "sorts",
        &json!([{"key": 0, "direction": 1}, {"key": 1, "direction": 1}]),
    )
    .unwrap();

    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    let want: Vec<pb::WallpaperFilterRule> = filters
        .as_array()
        .unwrap()
        .iter()
        .map(|v| s::map_to_filter_rule(v.as_object().unwrap()))
        .collect();
    assert_eq!(req.filters, want);
    assert_eq!(req.filters.len(), 7, "ни одно правило не пропало");
    assert_eq!(
        req.filter_logics.iter().map(|l| (l.op, l.group_a, l.group_b)).collect::<Vec<_>>(),
        vec![(2, 0, 5), (1, 8, 9), (0, 0, 5)]
    );
    assert_eq!(
        req.sorts.iter().map(|r| r.key).collect::<Vec<_>>(),
        vec![0, pb::WallpaperSortKey::Name as i32]
    );
}

// ── разбор ответа ───────────────────────────────────────────────────────

#[test]
fn list_response_sets_items_then_total_then_finishes() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    q::begin_call(&mut st, true);

    let sig = w::apply_wallpaper_list(&mut st, &mut d, &list_frame(vec![full_entry()], 1)).unwrap();
    // Оригинал: сначала синхронизация модели, потом totalChanged, за ним
    // `setNoMore` (waywallen 0.5.0, база QueryList: весь список пришёл —
    // страниц больше нет), и лишь затем статус Finished.
    assert_eq!(
        sig,
        vec![
            prop("items"),
            prop("total"),
            prop("noMore"),
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
    assert_eq!(d.total, 1);
    assert_eq!(d.items.len(), 1);
    assert_eq!(d.items[0]["id_proto"], json!("wp-42"));
}

#[test]
fn same_list_repeats_silently() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    let f = list_frame(vec![full_entry()], 1);
    q::begin_call(&mut st, true);
    w::apply_wallpaper_list(&mut st, &mut d, &f).unwrap();

    // Пересканирование библиотеки без изменений: список тот же — сброса
    // модели быть не должно (иначе теряются прокрутка и выделение).
    q::begin_call(&mut st, true);
    let sig = w::apply_wallpaper_list(&mut st, &mut d, &f).unwrap();
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
}

#[test]
fn total_comes_from_count_field_not_page_length() {
    // Контракт демона: count — сколько записей подходит под фильтр,
    // независимо от размера отданной страницы.
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    q::begin_call(&mut st, true);
    w::apply_wallpaper_list(&mut st, &mut d, &list_frame(vec![full_entry()], 17)).unwrap();
    assert_eq!(d.total, 17);
    assert_eq!(d.items.len(), 1);
}

#[test]
fn daemon_error_keeps_previous_items() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    q::begin_call(&mut st, true);
    w::apply_wallpaper_list(&mut st, &mut d, &list_frame(vec![full_entry()], 1)).unwrap();

    q::begin_call(&mut st, true);
    let sig = w::apply_wallpaper_list(
        &mut st,
        &mut d,
        &frame(pb::Status::Internal as i32, "база занята".into(), None),
    )
    .unwrap();
    // inspect_set оригинала на ошибке не зовёт разбор — свойства целы.
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::ErrorChanged,
            q::QuerySignal::StatusChanged(q::STATUS_ERROR),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::ErrorOccurred("база занята".into()),
            q::QuerySignal::AppErrorOccurred("база занята".into()),
        ]
    );
    assert_eq!(st.error, "база занята");
    assert_eq!(d.items.len(), 1);
    assert_eq!(d.total, 1);
}

#[test]
fn broken_frame_is_an_error_not_a_panic() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    assert!(w::apply_wallpaper_list(&mut st, &mut d, b"\xff\xff\xff\xff").is_err());
    let mut g = w::WallpaperGetData::default();
    assert!(w::apply_wallpaper_get(&mut st, &mut g, b"\xff\xff\xff\xff").is_err());
}

// ── WallpaperGetQuery ───────────────────────────────────────────────────

#[test]
fn get_response_sets_wallpaper_then_finishes() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperGetData::default();
    q::begin_call(&mut st, true);
    let sig = w::apply_wallpaper_get(
        &mut st,
        &mut d,
        &ok_frame(pb::response::Payload::WallpaperGet(
            pb::WallpaperGetResponse {
                entry: Some(full_entry()),
            },
        )),
    )
    .unwrap();
    assert_eq!(
        sig,
        vec![
            prop("wallpaper"),
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
    assert_eq!(d.wallpaper, w::entry_to_map(&full_entry()));

    // Тот же ответ второй раз — молчание.
    q::begin_call(&mut st, true);
    let sig = w::apply_wallpaper_get(
        &mut st,
        &mut d,
        &ok_frame(pb::response::Payload::WallpaperGet(
            pb::WallpaperGetResponse {
                entry: Some(full_entry()),
            },
        )),
    )
    .unwrap();
    assert!(!sig.contains(&prop("wallpaper")));
}

#[test]
fn get_response_without_entry_clears_wallpaper() {
    // Оригинал присваивает `rsp.wallpaperGet().entry()` без проверки
    // присутствия: у Qt-Protobuf это запись с полями по умолчанию.
    // Панель по пустому `id_proto` сама падает обратно на строку списка.
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperGetData::default();
    q::begin_call(&mut st, true);
    w::apply_wallpaper_get(
        &mut st,
        &mut d,
        &ok_frame(pb::response::Payload::WallpaperGet(
            pb::WallpaperGetResponse {
                entry: Some(full_entry()),
            },
        )),
    )
    .unwrap();

    q::begin_call(&mut st, true);
    w::apply_wallpaper_get(
        &mut st,
        &mut d,
        &ok_frame(pb::response::Payload::WallpaperGet(
            pb::WallpaperGetResponse { entry: None },
        )),
    )
    .unwrap();
    assert_eq!(d.wallpaper["id_proto"], json!(""));
}

// ── WallpaperLayoutSetQuery ─────────────────────────────────────────────
//
// Сверка с `WallpaperLayoutSetQuery::reload()` и макросом `WW_SET`
// оригинала (`ui/src/query/wallpaper_query.cpp`).

fn layout_of(bytes: &[u8]) -> pb::WallpaperLayoutSetRequest {
    use prost::Message;
    pb::WallpaperLayoutSetRequest::decode(bytes).expect("запрос разобран")
}

#[test]
fn layout_set_without_wallpaper_id_sends_nothing() {
    // Оригинал: `if (m_wallpaper_id.isEmpty()) return;` — ДО setStatus,
    // то есть запрос не начинается вовсе.
    let d = w::LayoutSetData::default();
    assert!(w::encode_layout_set(&d).is_none());
}

#[test]
fn layout_set_sends_prefs_and_marks_location() {
    let d = w::LayoutSetData {
        wallpaper_id: "wp-1".into(),
        clear: false,
        fillmode: 3,
        location_x: 40,
        location_y: 60,
        rotation: 2,
        flip: 1,
    };
    let req = layout_of(&w::encode_layout_set(&d).expect("запрос собран"));
    assert_eq!(req.wallpaper_id, "wp-1");
    assert!(!req.clear);
    let l = req.layout.expect("тело раскладки уходит при clear=false");
    assert_eq!(l.fillmode, 3);
    assert_eq!(l.rotation, 2);
    assert_eq!((l.location_x, l.location_y), (40, 60));
    assert!(l.location_set, "оригинал всегда ставит location_set");
    assert_eq!(l.align, 0, "выравнивание оригинал здесь не трогает");
    assert_eq!(l.flip, 1, "v0.4.4: отражение уходит вместе с раскладкой");
}

#[test]
fn layout_set_clamps_percentages() {
    // std::clamp(v, 0, 100) оригинала: страница уже режет, но запрос
    // обязан быть корректен и без её помощи.
    for (x, y, want) in [(-5, 250, (0u32, 100u32)), (0, 100, (0, 100))] {
        let d = w::LayoutSetData {
            wallpaper_id: "wp".into(),
            location_x: x,
            location_y: y,
            ..Default::default()
        };
        let l = layout_of(&w::encode_layout_set(&d).unwrap()).layout.unwrap();
        assert_eq!((l.location_x, l.location_y), want);
    }
}

#[test]
fn layout_set_clear_drops_the_prefs_body() {
    // Сброс: демон снимает персональную раскладку, тела нет вовсе —
    // иначе он записал бы «пустую» раскладку вместо удаления оверрайда.
    let d = w::LayoutSetData {
        wallpaper_id: "wp".into(),
        clear: true,
        fillmode: 3,
        location_x: 10,
        rotation: 4,
        ..Default::default()
    };
    let req = layout_of(&w::encode_layout_set(&d).unwrap());
    assert!(req.clear);
    assert!(req.layout.is_none());
}

#[test]
fn layout_properties_signal_params_only_on_change() {
    let mut d = w::LayoutSetData::default();
    // WW_SET: сигнал только при реальном изменении, и он один на все
    // свойства — paramsChanged.
    assert_eq!(
        w::set_layout_property(&mut d, "fillmode", &json!(3)),
        Some(vec![q::QuerySignal::Property("params")])
    );
    assert_eq!(
        w::set_layout_property(&mut d, "fillmode", &json!(3)),
        Some(vec![])
    );
    assert_eq!(d.fillmode, 3);
    // Числа из QML приходят double'ами.
    assert_eq!(
        w::set_layout_property(&mut d, "locationX", &json!(49.6)),
        Some(vec![q::QuerySignal::Property("params")])
    );
    assert_eq!(d.location_x, 50);
    // Неизвестное имя — None (шим падает на Python-путь).
    assert!(w::set_layout_property(&mut d, "align", &json!(1)).is_none());
}

#[test]
fn layout_set_covers_every_property_of_the_original() {
    let mut d = w::LayoutSetData::default();
    for name in w::LAYOUT_SET_PROPERTIES {
        assert!(
            w::set_layout_property(&mut d, name, &json!(1)).is_some(),
            "свойство {name} должно приниматься"
        );
    }
    // v0.4.4: седьмое — `flip` (отражение).
    assert_eq!(w::LAYOUT_SET_PROPERTIES.len(), 7);
}

#[test]
fn ack_response_finishes_or_reports_daemon_error() {
    // Запрос-действие: полезной нагрузки нет, важен исход.
    let mut st = q::QueryState::default();
    q::begin_call(&mut st, true);
    let sig = q::apply_ack(
        &mut st,
        &ok_frame(pb::response::Payload::WallpaperLayoutSet(
            pb::WallpaperLayoutSetResponse { entry: None },
        )),
    )
    .unwrap();
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );

    let mut st = q::QueryState::default();
    q::set_forward_error(&mut st, false); // панель показывает ошибку сама
    q::begin_call(&mut st, true);
    let bad = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 1,
            status: pb::Status::Internal as i32,
            message: "нет такой обои".into(),
            error_code: 0,
            payload: None,
        })),
    };
    use prost::Message;
    let sig = q::apply_ack(&mut st, &bad.encode_to_vec()).unwrap();
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::ErrorChanged,
            q::QuerySignal::StatusChanged(q::STATUS_ERROR),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::ErrorOccurred("нет такой обои".into()),
        ],
        "forwardError=false — в глобальный обработчик не уходит"
    );
}

// ── waywallen 0.4.4: скрытые обои и отражение ───────────────────────────

#[test]
fn hidden_filter_counts_as_active_filter_and_goes_into_request() {
    let mut d = w::WallpaperListData::default();
    // 1 = WALLPAPER_HIDDEN_FILTER_ONLY: сигнал свойства, затем перелом
    // `hasActiveFilters` — порядок сеттера `setHiddenFilter` оригинала.
    assert_eq!(
        w::set_property(&mut d, "hiddenFilter", &json!(1)).unwrap(),
        vec![
            q::QuerySignal::Property("hiddenFilter"),
            q::QuerySignal::Property("hasOtherFilters"),
            q::QuerySignal::Property("hasActiveFilters"),
        ]
    );
    assert!(d.has_active_filters());
    assert!(w::set_property(&mut d, "hiddenFilter", &json!(1)).unwrap().is_empty());
    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    assert_eq!(req.hidden_filter, pb::WallpaperHiddenFilter::Only as i32);
    // 2 = INCLUDE: признак фильтра не переворачивается — только свойство.
    assert_eq!(
        w::set_property(&mut d, "hiddenFilter", &json!(2)).unwrap(),
        vec![q::QuerySignal::Property("hiddenFilter")]
    );
}

#[test]
fn entry_map_carries_hidden_flag_and_layout_flip() {
    let mut e = full_entry();
    e.hidden = true;
    e.wallpaper_layout_override = Some(pb::LayoutPrefs {
        flip: pb::Flip::Horizontal as i32,
        ..Default::default()
    });
    let m = w::entry_to_map(&e);
    assert_eq!(m.get("hidden"), Some(&Value::Bool(true)));
    assert_eq!(
        m["wallpaperLayoutOverride"].get("flip"),
        Some(&json!(pb::Flip::Horizontal as i32))
    );
    // Без оверрайда ключ всё равно есть — форма карты не прыгает.
    e.wallpaper_layout_override = None;
    let m = w::entry_to_map(&e);
    assert_eq!(m["wallpaperLayoutOverride"].get("flip"), Some(&json!(0)));
}

#[test]
fn layout_set_sends_flip() {
    let mut d = w::LayoutSetData {
        wallpaper_id: "a".into(),
        ..Default::default()
    };
    w::set_layout_property(&mut d, "flip", &json!(pb::Flip::Both as i32)).unwrap();
    let req = pb::WallpaperLayoutSetRequest::decode(&w::encode_layout_set(&d).unwrap()[..])
        .unwrap();
    assert_eq!(req.layout.unwrap().flip, pb::Flip::Both as i32);
}

#[test]
fn hide_request_like_the_original() {
    // Пустые отсеиваются; ничего не осталось — запроса нет вовсе.
    assert!(w::encode_hide_request(&["".into()], true).is_none());
    let one = pb::WallpaperHideRequest::decode(
        &w::encode_hide_request(&["".into(), "a".into()], true).unwrap()[..],
    )
    .unwrap();
    assert_eq!(one.wallpaper_id, "a");
    assert!(one.wallpaper_ids.is_empty());
    assert!(one.hidden);
    let many = pb::WallpaperHideRequest::decode(
        &w::encode_hide_request(&["a".into(), "b".into()], false).unwrap()[..],
    )
    .unwrap();
    assert_eq!(many.wallpaper_id, "");
    assert_eq!(many.wallpaper_ids, vec!["a".to_string(), "b".to_string()]);
    assert!(!many.hidden);
}

// ── WallpaperApplyQuery: остановленные плейлисты ────────────────────────

/// Ключи карты — те, что читает `toastStoppedPlaylists` панели деталей:
/// разойдись они с разметкой, тост молча показал бы «Плейлист #undefined».
#[test]
fn stopped_playlists_use_keys_the_detail_panel_reads() {
    const PANEL: &str = include_str!(
        "../../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/qml/page/WallpaperDetailPanel.qml"
    );
    let rsp = pb::WallpaperApplyResponse {
        renderer_id: "r1".into(),
        stopped_playlists: vec![
            pb::WallpaperApplyStoppedPlaylist {
                playlist_id: 7,
                playlist_name: "Вечер".into(),
                display_ids: vec![5, 9],
                all_displays: false,
            },
            pb::WallpaperApplyStoppedPlaylist {
                playlist_id: 8,
                playlist_name: String::new(),
                display_ids: Vec::new(),
                all_displays: true,
            },
        ],
        ..Default::default()
    };
    let list = w::stopped_playlists(&rsp);
    assert_eq!(
        list,
        json!([
            {"playlistId": 7, "playlistName": "Вечер", "allDisplays": false, "displayIds": [5, 9]},
            {"playlistId": 8, "playlistName": "", "allDisplays": true, "displayIds": []},
        ])
    );
    assert!(PANEL.contains("applyQuery.stoppedPlaylists"), "панель больше не читает список");
    for key in list[0].as_object().expect("карта").keys() {
        assert!(
            PANEL.contains(&format!("playlist.{key}")),
            "панель деталей не читает ключ {key}"
        );
    }
}

/// Ответ без остановленных плейлистов — пустой список, а не null: разметка
/// берёт у него `length`.
#[test]
fn stopped_playlists_empty_response_is_empty_list() {
    assert_eq!(w::stopped_playlists(&pb::WallpaperApplyResponse::default()), json!([]));
}

// ── waywallen 0.5.0: скрытые библиотеки ─────────────────────────────────

/// `setHiddenLibraryIds` оригинала: список нормализуется (по возрастанию,
/// без повторов) ДО сравнения; признак `hasActiveFilters` поднимают и
/// скрытые библиотеки, а `hasOtherFilters` — нет.
#[test]
fn hidden_library_ids_are_normalized_and_raise_only_active_filters() {
    let mut d = w::WallpaperListData::default();
    assert_eq!(
        w::set_property(&mut d, "hiddenLibraryIds", &json!([3, 1, 3])).unwrap(),
        vec![prop("hiddenLibraryIds"), prop("hasActiveFilters")]
    );
    assert_eq!(d.hidden_library_ids, vec![1, 3]);
    assert!(d.has_active_filters());
    assert!(!d.has_other_filters());
    // Тот же набор в другом порядке и с повтором — молчание.
    assert!(w::set_property(&mut d, "hiddenLibraryIds", &json!([3.0, 1.0]))
        .unwrap()
        .is_empty());
    // Строка с числом (так номер приходит из карты) — тот же номер, мусор
    // отбрасывается.
    assert_eq!(
        w::set_property(&mut d, "hiddenLibraryIds", &json!(["1", 3, "x", null])).unwrap(),
        Vec::<q::QuerySignal>::new()
    );
    // Не список — «скрытых нет».
    assert_eq!(
        w::set_property(&mut d, "hiddenLibraryIds", &Value::Null).unwrap(),
        vec![prop("hiddenLibraryIds"), prop("hasActiveFilters")]
    );
    assert!(d.hidden_library_ids.is_empty());
    assert!(!d.has_active_filters());
}

/// `toggleLibraryHidden` оригинала: был — снять, не было — добавить.
#[test]
fn toggle_library_hidden_flips_one_id() {
    let mut d = w::WallpaperListData::default();
    w::set_property(&mut d, "hiddenLibraryIds", &json!([3, 1, 3])).unwrap();
    assert_eq!(
        w::toggle_library_hidden(&mut d, 1),
        vec![prop("hiddenLibraryIds")]
    );
    assert_eq!(d.hidden_library_ids, vec![3]);
    assert_eq!(
        w::toggle_library_hidden(&mut d, 3),
        vec![prop("hiddenLibraryIds"), prop("hasActiveFilters")]
    );
    assert!(d.hidden_library_ids.is_empty());
    assert_eq!(
        w::toggle_library_hidden(&mut d, 7),
        vec![prop("hiddenLibraryIds"), prop("hasActiveFilters")]
    );
    assert_eq!(d.hidden_library_ids, vec![7]);
}

/// Таблица признаков: `hasOtherFilters` — всё, кроме скрытых библиотек,
/// `hasActiveFilters` — скрытые библиотеки ИЛИ прочие. При уже скрытой
/// библиотеке быстрый фильтр переворачивает только `hasOtherFilters`.
#[test]
fn other_and_active_filter_flags_follow_upstream_table() {
    let mut d = w::WallpaperListData::default();
    assert!(!d.has_other_filters() && !d.has_active_filters());
    w::set_property(&mut d, "hiddenLibraryIds", &json!([2])).unwrap();
    assert!(!d.has_other_filters() && d.has_active_filters());
    assert_eq!(
        w::set_property(&mut d, "skipTypes", &json!(["video"])).unwrap(),
        vec![prop("skipTypes"), prop("hasOtherFilters")]
    );
    assert!(d.has_other_filters() && d.has_active_filters());
    w::set_property(&mut d, "hiddenLibraryIds", &json!([])).unwrap();
    assert!(d.has_other_filters() && d.has_active_filters());
    // Логика групп не фильтр — признаков не трогает.
    assert_eq!(
        w::set_property(&mut d, "filterLogics", &json!([{"op": 1, "groupA": 0, "groupB": 1}]))
            .unwrap(),
        vec![prop("filterLogics"), prop("filterState")]
    );
}

/// Кадр несёт `library_visibility` ВСЕГДА, как `listRequest` оригинала:
/// пустой список — «показать все».
#[test]
fn list_request_carries_library_visibility() {
    let mut d = w::WallpaperListData::default();
    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    assert_eq!(
        req.library_visibility,
        Some(pb::WallpaperLibraryVisibility { hidden_library_ids: vec![] })
    );
    w::set_property(&mut d, "hiddenLibraryIds", &json!([5, 2])).unwrap();
    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    assert_eq!(
        req.library_visibility,
        Some(pb::WallpaperLibraryVisibility { hidden_library_ids: vec![2, 5] })
    );
}

// ── waywallen 0.5.0: выборка ────────────────────────────────────────────

/// Выборка, в которой заполнены все 10 полей (номера библиотек — уже по
/// возрастанию: сеттер их нормализует). Одно правило — незаполненная
/// строка редактора: в выборке она обязана сохраниться.
fn full_selection() -> pb::WallpaperSelection {
    use pb::wallpaper_filter_rule::Payload;
    pb::WallpaperSelection {
        filters: vec![
            pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Name as i32,
                group: 0,
                payload: Some(Payload::StringFilter(pb::WallpaperStringFilter {
                    value: "закат".into(),
                    condition: 2,
                })),
            },
            pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Library as i32,
                group: 1,
                payload: Some(Payload::IntFilter(pb::WallpaperIntFilter {
                    value: 4,
                    condition: pb::IntCondition::Equal as i32,
                })),
            },
            pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Tag as i32,
                group: 1,
                payload: Some(Payload::TagFilter(pb::WallpaperTagFilter {
                    values: vec!["море".into(), "ночь".into()],
                    condition: 1,
                })),
            },
            pb::WallpaperFilterRule::default(),
        ],
        filter_logics: vec![pb::FilterLogic {
            op: pb::LogicOp::Or as i32,
            group_a: 0,
            group_b: 1,
        }],
        sorts: vec![pb::WallpaperSortRule { key: 3, direction: 2 }],
        search_text: "волны".into(),
        wp_type: "scene".into(),
        skip_types: vec!["video".into()],
        filter_tags: vec!["природа".into()],
        skip_content_ratings: vec!["mature".into()],
        hidden_filter: 2,
        hidden_library_ids: vec![2, 7],
    }
}

/// selection(restore(x)) == x: все 10 полей проходят круг без потерь.
#[test]
fn selection_survives_restore_round_trip() {
    let x = full_selection();
    let mut d = w::WallpaperListData::default();
    let signals = w::restore_selection(&mut d, &w::encode_selection(&x));
    assert!(!signals.is_empty());
    assert_eq!(w::selection_of(&d), x);
    assert_eq!(w::decode_selection(&w::encode_selection(&x)), Some(x));
}

/// `matchesSelection` истинно сразу после `restoreSelection` и ложно после
/// смены ЛЮБОГО из 10 полей.
#[test]
fn matches_selection_breaks_on_each_of_ten_fields() {
    let x = w::encode_selection(&full_selection());
    let changes: [(&str, Value); 10] = [
        ("filters", json!([])),
        ("filterLogics", json!([])),
        ("sorts", json!([])),
        ("searchText", json!("другое")),
        ("wpType", json!("video")),
        ("skipTypes", json!([])),
        ("filterTags", json!([])),
        ("skipContentRatings", json!([])),
        ("hiddenFilter", json!(0)),
        ("hiddenLibraryIds", json!([2])),
    ];
    for (name, value) in changes {
        let mut d = w::WallpaperListData::default();
        w::restore_selection(&mut d, &x);
        assert!(w::matches_selection(&d, &x), "до смены {name}");
        assert!(!w::set_property(&mut d, name, &value).unwrap().is_empty());
        assert!(!w::matches_selection(&d, &x), "после смены {name}");
    }
}

/// Восстановление ТЕКУЩЕЙ выборки — ни одного сигнала, то есть ни
/// перезагрузки, ни `selectionChanged`.
#[test]
fn restoring_current_selection_is_silent() {
    let x = w::encode_selection(&full_selection());
    let mut d = w::WallpaperListData::default();
    w::restore_selection(&mut d, &x);
    assert!(w::restore_selection(&mut d, &x).is_empty());
    // Пустая выборка поверх пустого состояния — тоже.
    let mut fresh = w::WallpaperListData::default();
    let empty = w::encode_selection(&w::selection_of(&fresh));
    assert!(w::restore_selection(&mut fresh, &empty).is_empty());
}

/// Порядок сигналов `restoreSelection` — обычные сеттеры в порядке
/// оригинала: `replaceFilterState`, `sorts`, …, `hiddenLibraryIds`.
#[test]
fn restore_selection_uses_upstream_setter_order() {
    let mut d = w::WallpaperListData::default();
    let signals = w::restore_selection(&mut d, &w::encode_selection(&full_selection()));
    assert_eq!(
        signals,
        vec![
            prop("filters"),
            prop("filterLogics"),
            prop("filterState"),
            prop("hasOtherFilters"),
            prop("hasActiveFilters"),
            prop("sorts"),
            prop("searchText"),
            prop("wpType"),
            prop("skipTypes"),
            prop("filterTags"),
            prop("skipContentRatings"),
            prop("hiddenFilter"),
            prop("hiddenLibraryIds"),
        ]
    );
}

/// Не выборка — ничего: у пустой выборки prost-код пуст, и без метки
/// пустая строка или `null` из QML «совпали» бы с пустым состоянием.
#[test]
fn untagged_bytes_are_not_a_selection() {
    let mut d = w::WallpaperListData::default();
    assert!(w::decode_selection(b"").is_none());
    assert!(!w::matches_selection(&d, b""));
    let bare = full_selection().encode_to_vec();
    assert!(w::decode_selection(&bare).is_none());
    assert!(w::restore_selection(&mut d, &bare).is_empty());
    let empty = w::encode_selection(&pb::WallpaperSelection::default());
    assert_eq!(empty, w::SELECTION_TAG);
    assert!(w::matches_selection(&d, &empty));
    // Метка есть, код битый — тоже не выборка.
    let mut broken = w::SELECTION_TAG.to_vec();
    broken.extend([0xff, 0xff, 0xff]);
    assert!(w::decode_selection(&broken).is_none());
}

/// Запрос списка и выборка — одни и те же поля, и запрос несёт их как
/// есть: незаполненное правило не отбрасывается (оригинал его тоже шлёт).
#[test]
fn list_request_is_selection() {
    let mut d = w::WallpaperListData::default();
    w::restore_selection(&mut d, &w::encode_selection(&full_selection()));
    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    let s = w::selection_of(&d);
    assert_eq!(s, full_selection());
    assert_eq!(req.filters.len(), 4);
    assert_eq!(req.filters, s.filters);
    assert_eq!(req.filter_logics, s.filter_logics);
    assert_eq!(req.sorts, s.sorts);
    assert_eq!(req.search_text, s.search_text);
    assert_eq!(req.wp_type, s.wp_type);
    assert_eq!(req.skip_types, s.skip_types);
    assert_eq!(req.filter_tags, s.filter_tags);
    assert_eq!(req.skip_content_ratings, s.skip_content_ratings);
    assert_eq!(req.hidden_filter, s.hidden_filter);
    assert_eq!(
        req.library_visibility.unwrap().hidden_library_ids,
        s.hidden_library_ids
    );
}

// ── waywallen 0.5.0: страницы ───────────────────────────────────────────

fn entries(from: usize, n: usize) -> Vec<pb::WallpaperEntry> {
    (from..from + n)
        .map(|i| pb::WallpaperEntry {
            id: format!("wp-{i}"),
            ..Default::default()
        })
        .collect()
}

fn decode_list(bytes: &[u8]) -> pb::WallpaperListRequest {
    pb::WallpaperListRequest::decode(bytes).unwrap()
}

/// QueryList: reload — страница 0 размером `limit`, fetchMore — страница
/// `offset + 1` размером `limit`, строки дописываются, `noMore` — когда
/// строк столько, сколько подходит под выборку.
#[test]
fn pages_follow_query_list_base() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData {
        limit: 50,
        offset: 3,
        no_more: true,
        ..Default::default()
    };
    assert_eq!(
        w::begin_reload(&mut d),
        vec![prop("offset"), prop("noMore")]
    );
    let req = decode_list(&w::encode_list_request(&d));
    assert_eq!((req.page, req.page_size), (0, 50));

    st.status = q::STATUS_QUERYING;
    let signals = w::apply_wallpaper_list(&mut st, &mut d, &list_frame(entries(0, 50), 120)).unwrap();
    assert!(signals.contains(&prop("items")));
    assert!(!signals.contains(&prop("noMore")), "ещё есть страницы");
    assert!(!d.no_more);

    let req = decode_list(&w::encode_fetch_more_request(&d, st.status).unwrap());
    assert_eq!((req.page, req.page_size), (1, 50));
    st.status = q::STATUS_QUERYING;
    let signals =
        w::apply_wallpaper_page(&mut st, &mut d, &list_frame(entries(50, 50), 120), 1).unwrap();
    assert_eq!(&signals[..2], &[prop("itemsAppended"), prop("offset")]);
    assert_eq!(d.items.len(), 100);
    assert_eq!(d.offset, 1);
    assert_eq!(d.items[50]["id_proto"], json!("wp-50"));

    let req = decode_list(&w::encode_fetch_more_request(&d, st.status).unwrap());
    assert_eq!((req.page, req.page_size), (2, 50));
    let signals =
        w::apply_wallpaper_page(&mut st, &mut d, &list_frame(entries(100, 20), 120), 2).unwrap();
    assert!(signals.contains(&prop("noMore")));
    assert!(d.no_more);
    assert_eq!(d.items.len(), 120);
    assert!(w::encode_fetch_more_request(&d, st.status).is_none());
}

/// Пустая страница — конец, даже если `count` обещает больше.
#[test]
fn empty_page_ends_pages() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData {
        limit: 10,
        ..Default::default()
    };
    w::apply_wallpaper_list(&mut st, &mut d, &list_frame(entries(0, 10), 40)).unwrap();
    assert!(!d.no_more);
    let signals = w::apply_wallpaper_page(&mut st, &mut d, &list_frame(vec![], 40), 1).unwrap();
    assert!(!signals.contains(&prop("itemsAppended")));
    assert!(d.no_more);
}

/// Когда fetchMore не догружает: `noMore`, висящая перезагрузка, запрос в
/// полёте и — наше условие — `limit = 0` (иначе страница 1 размером 0 —
/// это ВЕСЬ список ещё раз, в хвост).
#[test]
fn fetch_more_is_refused_when_upstream_would_skip_or_double() {
    let d = w::WallpaperListData {
        limit: 50,
        ..Default::default()
    };
    assert!(w::encode_fetch_more_request(&d, q::STATUS_FINISHED).is_some());
    assert!(w::encode_fetch_more_request(&d, q::STATUS_QUERYING).is_none());
    let pending = w::WallpaperListData {
        reload_pending: true,
        ..d.clone()
    };
    assert!(w::encode_fetch_more_request(&pending, q::STATUS_FINISHED).is_none());
    let done = w::WallpaperListData {
        no_more: true,
        ..d.clone()
    };
    assert!(w::encode_fetch_more_request(&done, q::STATUS_FINISHED).is_none());
    let all = w::WallpaperListData::default();
    assert!(w::encode_fetch_more_request(&all, q::STATUS_UNINITIALIZED).is_none());
}

/// Главная страница (`limit = 0`): весь список одним ответом, после него
/// `noMore` — страниц нет.
#[test]
fn whole_list_answer_has_no_more_pages() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    let req = decode_list(&w::encode_list_request(&d));
    assert_eq!((req.page, req.page_size), (0, 0));
    let signals = w::apply_wallpaper_list(&mut st, &mut d, &list_frame(entries(0, 3), 3)).unwrap();
    assert!(signals.contains(&prop("noMore")));
    assert!(d.no_more);
}

/// `invalidateList`: `selectionChanged`, сброс страниц, перезагрузка в пути.
#[test]
fn invalidate_list_resets_pages() {
    let mut d = w::WallpaperListData {
        offset: 2,
        ..Default::default()
    };
    assert_eq!(
        w::invalidate_list(&mut d),
        vec![prop("selection"), prop("offset")]
    );
    assert_eq!(d.offset, 0);
    assert!(d.reload_pending);
    assert_eq!(w::invalidate_list(&mut d), vec![prop("selection")]);
}

/// `limit`: до первого запуска только запоминается (+`selectionChanged`,
/// как у оригинала), после — перезагрузка, как `limitChanged →
/// invalidateList`.
#[test]
fn limit_reloads_only_after_first_start() {
    let mut d = w::WallpaperListData::default();
    let (signals, reload) = w::set_limit(&mut d, 50, q::STATUS_UNINITIALIZED);
    assert_eq!(signals, vec![prop("limit"), prop("selection")]);
    assert!(!reload);
    assert!(!d.reload_pending);
    assert_eq!(w::set_limit(&mut d, 50, q::STATUS_FINISHED), (vec![], false));
    d.offset = 1;
    let (signals, reload) = w::set_limit(&mut d, 100, q::STATUS_FINISHED);
    assert_eq!(signals, vec![prop("limit"), prop("selection"), prop("offset")]);
    assert!(reload);
    assert!(d.reload_pending);
}
