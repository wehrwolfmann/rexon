//! Check of the `we_wallpaper.rs` port against the waywallen original —
//! `ui/src/query/wallpaper_query.cpp` (`WallpaperListQuery`,
//! `WallpaperGetQuery`) and the vendored QML of the «Wallpapers» page.
//!
//! Checked: the key set of the entry map (QML reads exactly those — the list
//! was verified by grep over `ui/qml`), the order of setter signals, silence
//! on a repeat of the same value, the daemon error branch, a broken frame and
//! the ROUND TRIP — filter/sort rules go into the request the same as they
//! came from the daemon settings. Frames are built by prost, the live daemon
//! is not poked.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_query as q;
use rexon_core::we_settings as s;
use rexon_core::we_wallpaper as w;
use serde_json::{json, Value};
use std::collections::HashMap;

/// A response frame: status OK and a payload.
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

/// A fully filled entry: it checks that no `WallpaperEntry` field got lost on
/// the way to QML.
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
        // waywallen 0.4.4: the «hidden» flag. It does not go into the QML map — the
        // v0.4.3 markup does not read it; it will come with accepting the v0.4.4 markup.
        hidden: false,
        update_available: false,
        sub_type: 0,
    }
}

// ── library entry map ─────────────────────────────────────────────

#[test]
fn entry_map_keys_match_qt_protobuf_gadget_properties() {
    let m = w::entry_to_map(&full_entry());
    let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
    keys.sort_unstable();
    // All properties of the `control::v1::WallpaperEntry` gadget: the proto
    // fields' json_name, except `id` — the Qt Protobuf generator calls it `id_proto`.
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
        // waywallen 0.4.4: the «hidden» flag.
        "hidden",
        // waywallen 0.6.0: update at the source and the entry subtype.
        "updateAvailable",
        "subType",
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

/// A defect of the former Python bridge: the details panel reads these three
/// keys, but the map did not carry them — the «Layout override» block showed
/// the default layout, the reset button and the «Unsubscribe» action were
/// forever grey.
#[test]
fn entry_map_has_layout_override_and_unsubscribe_flags() {
    let m = w::entry_to_map(&full_entry());
    assert_eq!(m["wallpaperLayoutOverrideSet"], json!(true));
    assert_eq!(m["supportsItemUnsubscribe"], json!(true));
    assert_eq!(m["supportsItemRemove"], json!(true));
    // The nested layout keys are the same as in the settings' layout_to_map,
    // plus `flip` (v0.4.4): WallpaperDetailPanel reads
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
    // The Qt-Protobuf gadget returns an unset message with default fields, and
    // QML takes `wallpaperLayoutOverride` unconditionally — the map shape must
    // not jump.
    let m = w::entry_to_map(&pb::WallpaperEntry::default());
    assert_eq!(m["wallpaperLayoutOverrideSet"], json!(false));
    let mut want = s::layout_to_map(&pb::LayoutPrefs::default());
    want.insert("flip".into(), json!(0));
    assert_eq!(m["wallpaperLayoutOverride"], Value::Object(want));
}

#[test]
fn entry_map_metadata_order_is_stable() {
    // The proto HashMap order is non-deterministic: without sorting the same
    // entry would look «changed» and trigger a model reset.
    let mut e = pb::WallpaperEntry::default();
    for i in 0..32 {
        e.metadata.insert(format!("k{i}"), format!("v{i}"));
    }
    let first = w::entry_to_map(&e);
    for _ in 0..8 {
        assert_eq!(w::entry_to_map(&e), first);
    }
}

// ── property setters ─────────────────────────────────────────────────────

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
    // A repeat of the same value is silent (the original's setters compare).
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
    // The list of names the shim reads is the very same.
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
    // The original's setFilters: filtersChanged → filterStateChanged →
    // hasOtherFiltersChanged (0.5.0, subscribed to filterStateChanged) →
    // hasActiveFiltersChanged (both only when the flag flipped).
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
    // A repeat — complete silence, including filterStateChanged (otherwise the
    // page would reload the list for nothing).
    assert!(w::set_property(&mut d, "filters", &rules)
        .unwrap()
        .is_empty());
    // Removing the last rule flips the flag again.
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
    // The original's hasActiveFilters does not take group logic into account.
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

/// A defect of the former Python bridge: quick lists changed
/// `hasActiveFilters` but sent no signal — the «Filters» button highlight was
/// updated only as a side effect of a successful daemon response, and was not
/// updated on an error.
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
        // A second identical list — silence.
        assert!(w::set_property(&mut d, name, &value).unwrap().is_empty());
        // Back to empty — the flag flipped back.
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
    // The flag is already raised by skipping types — a second signal is needed
    // neither when adding another quick filter nor when editing the same list.
    assert_eq!(
        w::set_property(&mut d, "filterTags", &json!(["море"])).unwrap(),
        vec![prop("filterTags")]
    );
    assert_eq!(
        w::set_property(&mut d, "skipTypes", &json!(["video", "web"])).unwrap(),
        vec![prop("skipTypes")]
    );
    // And removing one of the two does not flip the flag either.
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
    // A repeat — «nothing changed», the page does not reload on false.
    let (changed, sig) = w::replace_filter_state(&mut d, &filters, &logics);
    assert!(!changed);
    assert!(sig.is_empty());
}

#[test]
fn replace_filter_state_reports_only_changed_half() {
    let mut d = w::WallpaperListData::default();
    let filters = json!([{"type": 1, "group": 0, "stringFilter": {"value": "a", "condition": 1}}]);
    w::replace_filter_state(&mut d, &filters, &json!([]));
    // Only the group logic changes: the rules signal does not fire, the active
    // filters flag did not flip.
    let (changed, sig) = w::replace_filter_state(
        &mut d,
        &filters,
        &json!([{"op": 2, "groupA": 0, "groupB": 1}]),
    );
    assert!(changed);
    assert_eq!(sig, vec![prop("filterLogics"), prop("filterState")]);
}

// ── request ──────────────────────────────────────────────────────────────

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
    // WE tab boundaries: hidden types (gltf) are appended to the frame after the person's choice.
    assert_eq!(req.skip_types, vec!["video".to_string(), "gltf".to_string()]);
    assert_eq!(req.filter_tags, vec!["природа".to_string()]);
    assert_eq!(req.skip_content_ratings, vec!["mature".to_string()]);
    assert_eq!(req.sorts.len(), 1);
    assert_eq!(req.filters.len(), 2);
    assert_eq!(req.filter_logics.len(), 1);
    // initReqForReload with limit = 0: page zero, page size 0 — by protocol
    // «no paging, return everything».
    assert_eq!(req.page, 0);
    assert_eq!(req.page_size, 0);
}

/// ROUND TRIP: rules come to the page from the daemon settings (`settings_get`
/// → `wallpaperFilters`/`wallpaperSorts`) and go into the list request. The
/// map is the only representation between them, and it must carry the rules
/// without losses.
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
        // A fresh filter editor row — a rule without payload.
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

    // Settings → maps (exactly what QML gives our setters).
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

    // The map carried the rules without losses — together with the fresh row…
    assert_eq!(d.filters, filters);
    let req = pb::WallpaperListRequest::decode(&w::encode_list_request(&d)[..]).unwrap();
    assert_eq!(req.sorts, sorts);
    // …and they ALL go onto the wire, as in the original: the 0.5.0 daemon
    // itself rejects an unfilled rule, and the page shows its error.
    assert_eq!(req.filters, filters);
    assert_eq!(req.filter_logics, logics);
}

/// The request carries rules, logics and sorts AS IS — parity with the
/// original's `listRequest`. Invalid ones (a fresh editor row, the «any»
/// condition, a library with «greater», an orphan logic) are rejected by the
/// 0.5.0 daemon itself (`InvalidArgument` «invalid wallpaper selection»), and
/// the page shows its error. On 24.10.2026 Rexon silently dropped such rules
/// — and together with the gadget corruption in the shim this erased ALL
/// filters of a smart playlist without a single word.
#[test]
fn request_carries_rules_as_is_like_upstream() {
    let mut d = w::WallpaperListData::default();
    let filters = json!([
        {"type": 1, "group": 0, "stringFilter": {"value": "лес", "condition": 3}},
        // a fresh editor row: type 0, no payload
        {"type": 0, "group": 1},
        // the «any» condition (STRING_CONDITION_UNSPECIFIED)
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

// ── response parsing ───────────────────────────────────────────────────────

#[test]
fn list_response_sets_items_then_total_then_finishes() {
    let mut st = q::QueryState::default();
    let mut d = w::WallpaperListData::default();
    q::begin_call(&mut st, true);

    let sig = w::apply_wallpaper_list(&mut st, &mut d, &list_frame(vec![full_entry()], 1)).unwrap();
    // Original: first the model sync, then totalChanged, followed by `setNoMore`
    // (waywallen 0.5.0, the QueryList base: the whole list arrived — no more
    // pages), and only then the Finished status.
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

    // Rescanning the library without changes: the list is the same — there must
    // be no model reset (otherwise scrolling and selection are lost).
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
    // The daemon contract: count is how many entries match the filter,
    // regardless of the size of the returned page.
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
    // The original's inspect_set does not call parsing on an error — the properties are intact.
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

    // The same response a second time — silence.
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
    // The original assigns `rsp.wallpaperGet().entry()` without a presence
    // check: in Qt-Protobuf this is an entry with default fields. On an empty
    // `id_proto` the panel itself falls back to the list row.
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
// Check against the original's `WallpaperLayoutSetQuery::reload()` and the
// `WW_SET` macro (`ui/src/query/wallpaper_query.cpp`).

fn layout_of(bytes: &[u8]) -> pb::WallpaperLayoutSetRequest {
    use prost::Message;
    pb::WallpaperLayoutSetRequest::decode(bytes).expect("запрос разобран")
}

#[test]
fn layout_set_without_wallpaper_id_sends_nothing() {
    // Original: `if (m_wallpaper_id.isEmpty()) return;` — BEFORE setStatus, i.e.
    // the request does not start at all.
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
    // The original's std::clamp(v, 0, 100): the page already clamps, but the
    // request must be correct even without its help.
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
    // Reset: the daemon removes the personal layout, there is no body at all —
    // otherwise it would write an «empty» layout instead of removing the override.
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
    // WW_SET: a signal only on a real change, and it is one for all properties —
    // paramsChanged.
    assert_eq!(
        w::set_layout_property(&mut d, "fillmode", &json!(3)),
        Some(vec![q::QuerySignal::Property("params")])
    );
    assert_eq!(
        w::set_layout_property(&mut d, "fillmode", &json!(3)),
        Some(vec![])
    );
    assert_eq!(d.fillmode, 3);
    // Numbers from QML arrive as doubles.
    assert_eq!(
        w::set_layout_property(&mut d, "locationX", &json!(49.6)),
        Some(vec![q::QuerySignal::Property("params")])
    );
    assert_eq!(d.location_x, 50);
    // An unknown name — None (the shim falls back to the Python path).
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
    // v0.4.4: the seventh is `flip` (mirroring).
    assert_eq!(w::LAYOUT_SET_PROPERTIES.len(), 7);
}

#[test]
fn ack_response_finishes_or_reports_daemon_error() {
    // An action request: there is no payload, the outcome matters.
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
    q::set_forward_error(&mut st, false); // the panel shows the error itself
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

// ── waywallen 0.4.4: hidden wallpapers and mirroring ───────────────────────────

#[test]
fn hidden_filter_counts_as_active_filter_and_goes_into_request() {
    let mut d = w::WallpaperListData::default();
    // 1 = WALLPAPER_HIDDEN_FILTER_ONLY: the property signal, then the flip of
    // `hasActiveFilters` — the order of the original's `setHiddenFilter` setter.
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
    // 2 = INCLUDE: the filter flag does not flip — only the property.
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
    // waywallen 0.6.0: the update badge and the «timeline» subtype — as in the gadget.
    assert_eq!(m.get("updateAvailable"), Some(&Value::Bool(false)));
    assert_eq!(m.get("subType"), Some(&json!(0)));
    e.update_available = true;
    e.sub_type = pb::WallpaperSubType::Timeline as i32;
    let m = w::entry_to_map(&e);
    assert_eq!(m.get("updateAvailable"), Some(&Value::Bool(true)));
    assert_eq!(m.get("subType"), Some(&json!(1)));
    assert_eq!(
        m["wallpaperLayoutOverride"].get("flip"),
        Some(&json!(pb::Flip::Horizontal as i32))
    );
    // Without an override the key is still there — the map shape does not jump.
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
    // Empty ones are filtered out; nothing left — no request at all.
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

// ── WallpaperApplyQuery: stopped playlists ────────────────────────

/// The map keys are those read by the details panel's `toastStoppedPlaylists`:
/// should they diverge from the markup, the toast would silently show «Playlist #undefined».
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

/// A response without stopped playlists is an empty list, not null: the
/// markup takes its `length`.
#[test]
fn stopped_playlists_empty_response_is_empty_list() {
    assert_eq!(w::stopped_playlists(&pb::WallpaperApplyResponse::default()), json!([]));
}

// ── waywallen 0.5.0: hidden libraries ─────────────────────────────────

/// The original's `setHiddenLibraryIds`: the list is normalised (ascending,
/// without repeats) BEFORE comparison; the `hasActiveFilters` flag is raised
/// by hidden libraries too, while `hasOtherFilters` is not.
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
    // The same set in another order and with a repeat — silence.
    assert!(w::set_property(&mut d, "hiddenLibraryIds", &json!([3.0, 1.0]))
        .unwrap()
        .is_empty());
    // A string with a number (that is how the id comes from the map) — the same
    // id, garbage is dropped.
    assert_eq!(
        w::set_property(&mut d, "hiddenLibraryIds", &json!(["1", 3, "x", null])).unwrap(),
        Vec::<q::QuerySignal>::new()
    );
    // Not a list — «nothing hidden».
    assert_eq!(
        w::set_property(&mut d, "hiddenLibraryIds", &Value::Null).unwrap(),
        vec![prop("hiddenLibraryIds"), prop("hasActiveFilters")]
    );
    assert!(d.hidden_library_ids.is_empty());
    assert!(!d.has_active_filters());
}

/// The original's `toggleLibraryHidden`: was there — remove, was not — add.
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

/// The flag table: `hasOtherFilters` is everything except hidden libraries,
/// `hasActiveFilters` is hidden libraries OR the rest. With a library already
/// hidden a quick filter flips only `hasOtherFilters`.
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
    // Group logic is not a filter — it does not touch the flags.
    assert_eq!(
        w::set_property(&mut d, "filterLogics", &json!([{"op": 1, "groupA": 0, "groupB": 1}]))
            .unwrap(),
        vec![prop("filterLogics"), prop("filterState")]
    );
}

/// The frame ALWAYS carries `library_visibility`, as the original's
/// `listRequest`: an empty list means «show all».
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

// ── waywallen 0.5.0: selection ────────────────────────────────────────────

/// A selection with all 10 fields filled (library ids already ascending: the
/// setter normalises them). One rule is an unfilled editor row: it must be
/// kept in the selection.
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

/// selection(restore(x)) == x: all 10 fields make the round trip without losses.
#[test]
fn selection_survives_restore_round_trip() {
    let x = full_selection();
    let mut d = w::WallpaperListData::default();
    let signals = w::restore_selection(&mut d, &w::encode_selection(&x));
    assert!(!signals.is_empty());
    assert_eq!(w::selection_of(&d), x);
    assert_eq!(w::decode_selection(&w::encode_selection(&x)), Some(x));
}

/// `matchesSelection` is true right after `restoreSelection` and false after
/// changing ANY of the 10 fields.
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

/// Restoring the CURRENT selection — not a single signal, i.e. neither a
/// reload nor `selectionChanged`.
#[test]
fn restoring_current_selection_is_silent() {
    let x = w::encode_selection(&full_selection());
    let mut d = w::WallpaperListData::default();
    w::restore_selection(&mut d, &x);
    assert!(w::restore_selection(&mut d, &x).is_empty());
    // An empty selection over an empty state — likewise.
    let mut fresh = w::WallpaperListData::default();
    let empty = w::encode_selection(&w::selection_of(&fresh));
    assert!(w::restore_selection(&mut fresh, &empty).is_empty());
}

/// The signal order of `restoreSelection` — the regular setters in the
/// original's order: `replaceFilterState`, `sorts`, …, `hiddenLibraryIds`.
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

/// Not a selection — nothing: an empty selection's prost code is empty, and
/// without a tag an empty string or `null` from QML would «match» the empty state.
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
    // A tag is there, the code is broken — not a selection either.
    let mut broken = w::SELECTION_TAG.to_vec();
    broken.extend([0xff, 0xff, 0xff]);
    assert!(w::decode_selection(&broken).is_none());
}

/// The list request and the selection are the same fields, and the request
/// carries them as is: an unfilled rule is not dropped (the original sends it too).
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
    // The only divergence of the frame from the selection is the WE tab's hidden types.
    assert_eq!(req.skip_types, rexon_core::we_query::we_tab_skip_types(&s.skip_types));
    assert_eq!(req.filter_tags, s.filter_tags);
    assert_eq!(req.skip_content_ratings, s.skip_content_ratings);
    assert_eq!(req.hidden_filter, s.hidden_filter);
    assert_eq!(
        req.library_visibility.unwrap().hidden_library_ids,
        s.hidden_library_ids
    );
}

// ── waywallen 0.5.0: pages ───────────────────────────────────────────

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

/// QueryList: reload — page 0 of size `limit`, fetchMore — page `offset + 1`
/// of size `limit`, rows are appended, `noMore` — when there are as many rows
/// as match the selection.
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

/// An empty page is the end, even if `count` promises more.
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

/// When fetchMore does not load more: `noMore`, a pending reload, a request
/// in flight and — our condition — `limit = 0` (otherwise page 1 of size 0 is
/// the WHOLE list once more, at the tail).
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

/// The main page (`limit = 0`): the whole list in one response, after it
/// `noMore` — no pages.
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

/// `invalidateList`: `selectionChanged`, page reset, a reload on the way.
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

/// `limit`: before the first start it is only remembered (+`selectionChanged`,
/// as in the original), after — a reload, like `limitChanged → invalidateList`.
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
