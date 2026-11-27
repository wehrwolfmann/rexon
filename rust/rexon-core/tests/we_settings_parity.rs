//! Check of the `we_settings.rs` port against the waywallen original —
//! `ui/src/query/settings_query.cpp` (`global_to_map`/`map_to_global`,
//! `plugins_to_map`/`map_to_plugins`, `SettingsGetQuery::reload`).
//!
//! The main thing checked is the ROUND TRIP: the daemon treats `settings_set`
//! as a full replacement, so the map obtained from `settings_get` must build
//! back into the very same message. Plus the key set 1:1 with the original
//! (QML reads exactly those) and two defects of the former Python bridge fixed
//! by this port: flat `renderer.*` and the flat plugin map.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_query as q;
use rexon_core::we_settings as s;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Fully filled settings: the round trip is checked on them, so that no field
/// of the message is left uncovered.
fn full_global() -> pb::GlobalSettings {
    pb::GlobalSettings {
        wallpaper_filters: vec![
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
            // A rule without payload is a legitimate state of a fresh row of the filter
            // editor.
            pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Unspecified as i32,
                group: 3,
                payload: None,
            },
        ],
        wallpaper_filter_logics: vec![pb::FilterLogic {
            op: pb::LogicOp::Or as i32,
            group_a: 0,
            group_b: 1,
        }],
        wallpaper_sorts: vec![pb::WallpaperSortRule {
            key: pb::WallpaperSortKey::LastModified as i32,
            direction: pb::SortDirection::Desc as i32,
        }],
        layout_defaults: Some(pb::LayoutPrefs {
            fillmode: 2,
            align: 3,
            rotation: 1,
            location_x: 40,
            location_y: 60,
            location_set: true,
            // waywallen 0.4.4: mirroring is not used in layout_defaults.
            flip: 0,
        }),
        queue_mode: "shuffle".into(),
        rotation_secs: 900,
        wallpaper_skip_types: vec!["video".into(), "web".into()],
        wallpaper_filter_tags: vec!["природа".into()],
        wallpaper_skip_content_ratings: vec!["mature".into()],
        auto_replay: Some(pb::AutoReplayPolicy {
            any_window: pb::AutoAction::Mute as i32,
            focused: pb::AutoAction::Pause as i32,
            maximized: pb::AutoAction::Stop as i32,
            fullscreen: pb::AutoAction::Pause as i32,
            session_locked: pb::AutoAction::Stop as i32,
            session_inactive: pb::AutoAction::None as i32,
            resume_delay_ms: Some(400),
            // waywallen 0.4.2: the GameMode reason and the per-window pause scope.
            any_window_scope: pb::AutoScope::AllDisplays as i32,
            focused_scope: pb::AutoScope::CurrentDisplay as i32,
            maximized_scope: pb::AutoScope::AllDisplays as i32,
            fullscreen_scope: pb::AutoScope::AllDisplays as i32,
            gamemode: pb::AutoAction::Pause as i32,
            // waywallen 0.5.0: pause on battery power.
            on_battery: pb::AutoAction::Stop as i32,
            // waywallen 0.4.4: window exceptions — all four lists.
            window_exclusions: Some(pb::WindowExclusions {
                application_ids: vec!["org.kde.konsole".into()],
                titles: vec!["Просмотр видео".into()],
                application_id_patterns: vec!["steam_app_*".into()],
                title_patterns: vec!["*— Firefox".into(), "Игра?".into()],
            }),
        }),
        // waywallen 0.4.4: show hidden ones together with the rest.
        wallpaper_hidden_filter: pb::WallpaperHiddenFilter::Include as i32,
        // waywallen 0.5.0: hidden libraries of the «Wallpapers» page.
        wallpaper_library_visibility: Some(pb::WallpaperLibraryVisibility {
            hidden_library_ids: vec![4, 9],
        }),
        audio_fade_ms: 750,
        disable_plugin_update_notifications: true,
        duplicate_renderers_for_same_wallpaper: true,
        renderer: Some(pb::GlobalRendererSettings {
            enable_audio: Some(false),
            volume: Some(37),
            audio_output: Some("alsa_output.usb-headset".into()),
        }),
        mute_when_other_audio: Some(true),
        audio_capture_enabled: true,
        pause_effect: Some(pb::PauseEffectConfig {
            kind: pb::PauseEffectKind::Blur as i32,
            blur: Some(pb::BlurEffectConfig { radius: 42 }),
        }),
        pointer_forwarding_enabled: true,
        hide_tray_icon: true,
        // waywallen 0.4.2: wallpaper change animation.
        transition: Some(pb::TransitionConfig {
            kind: pb::TransitionKind::Wipe as i32,
            duration_ms: 1200,
            angle: 90,
            origin_x: 25,
            origin_y: 75,
        }),
        ..Default::default()
    }
}

// ── map key set ───────────────────────────────────────────────────

#[test]
fn global_map_keys_match_upstream_global_to_map() {
    let m = s::global_to_map(&full_global());
    let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
    keys.sort_unstable();
    // Exactly the list from global_to_map (settings_query.cpp; the key
    // `debugLoggingEnabled` was added by the vendor in v0.3.8, `transition` in
    // v0.4.2).
    let mut expected = vec![
        "debugLoggingEnabled",
        "transition",
        "wallpaperFilters",
        "wallpaperFilterLogics",
        "wallpaperSorts",
        "layoutDefaults",
        "autoReplay",
        "queueMode",
        "rotationSecs",
        "audioFadeMs",
        "muteWhenOtherAudio",
        "audioCaptureEnabled",
        "pauseEffect",
        "pointerForwardingEnabled",
        "hideTrayIcon",
        "pluginUpdateNotifications",
        "duplicateRenderers",
        "renderer.enable_audio",
        "renderer.volume",
        // waywallen 0.6.2
        "renderer.audio_output",
        "wallpaperSkipTypes",
        "wallpaperFilterTags",
        "wallpaperSkipContentRatings",
        // v0.4.4
        "wallpaperHiddenFilter",
        // v0.5.0 (only when the daemon sent the field)
        "wallpaperHiddenLibraryIds",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected);
}

#[test]
fn global_map_schema_covers_every_key() {
    // The schema for the shim and the map from global_to_map must match key for
    // key: the shim uses the schema to take values out of a QJSValue, a missing
    // key would be silently lost on write (full replacement → setting erased).
    let m = s::global_to_map(&full_global());
    let mut from_map: Vec<&str> = m.keys().map(String::as_str).collect();
    let mut from_schema: Vec<&str> = s::GLOBAL_MAP_SCHEMA.iter().map(|(k, _)| *k).collect();
    from_map.sort_unstable();
    from_schema.sort_unstable();
    assert_eq!(from_schema, from_map);
}

#[test]
fn unset_message_fields_are_absent_like_upstream() {
    // hasLayoutDefaults()/hasAutoReplay() are false → no keys at all
    // (QML distinguishes «did not arrive» from «arrived empty»).
    let m = s::global_to_map(&pb::GlobalSettings::default());
    assert!(!m.contains_key("layoutDefaults"));
    assert!(!m.contains_key("autoReplay"));
    // v0.4.2: `hasTransition()` is false — no key, as in the original.
    assert!(!m.contains_key("transition"));
    // …while scalar keys are always present, with default values.
    assert_eq!(m["queueMode"], json!(""));
    assert_eq!(m["rotationSecs"], json!(0));
    assert_eq!(m["muteWhenOtherAudio"], json!(false));
    assert_eq!(m["wallpaperFilters"], json!([]));
}

// ── resume delay (field of v0.3.9) ─────────────────────────────────

#[test]
fn auto_replay_resume_delay_matches_upstream_default() {
    // The field is optional, and the 0.3.8 daemon will not send it at all: for
    // the vendor `hasResumeDelayMs()` is false → 250 goes into the map, not zero.
    let m = s::auto_replay_to_map(&pb::AutoReplayPolicy::default());
    assert_eq!(
        m["resumeDelayMs"],
        json!(s::DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS)
    );
    // A sent number arrives as is — including zero («no delay»).
    let m = s::auto_replay_to_map(&pb::AutoReplayPolicy {
        resume_delay_ms: Some(0),
        ..Default::default()
    });
    assert_eq!(m["resumeDelayMs"], json!(0));
    // …and builds back into the message: `settings_set` is a full replacement, a
    // lost key would erase the setting.
    assert_eq!(s::map_to_auto_replay(&m).resume_delay_ms, Some(0));
    // No key at all (a page from the previous release) → 250 again, like the
    // original's `m.value("resumeDelayMs", 250)`.
    let mut bare = s::auto_replay_to_map(&pb::AutoReplayPolicy::default());
    bare.remove("resumeDelayMs");
    assert_eq!(
        s::map_to_auto_replay(&bare).resume_delay_ms,
        Some(s::DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS)
    );
}

// ── defect 1: flat renderer.* keys ───────────────────────────────────

#[test]
fn renderer_settings_are_flat_keys_with_upstream_defaults() {
    // No renderer message → sound on, volume 100.
    let m = s::global_to_map(&pb::GlobalSettings::default());
    assert_eq!(m[s::KEY_RENDERER_ENABLE_AUDIO], json!(true));
    assert_eq!(m[s::KEY_RENDERER_VOLUME], json!(100));
    // The message is there, but the fields are not set → the same values.
    let g = pb::GlobalSettings {
        renderer: Some(pb::GlobalRendererSettings::default()),
        ..Default::default()
    };
    let m = s::global_to_map(&g);
    assert_eq!(m[s::KEY_RENDERER_ENABLE_AUDIO], json!(true));
    assert_eq!(m[s::KEY_RENDERER_VOLUME], json!(100));
    // Set values arrive as is, there is no nested map.
    let m = s::global_to_map(&full_global());
    assert_eq!(m[s::KEY_RENDERER_ENABLE_AUDIO], json!(false));
    assert_eq!(m[s::KEY_RENDERER_VOLUME], json!(37));
    assert!(!m.contains_key("renderer"));
}

#[test]
fn renderer_flat_keys_write_back_into_message() {
    // Exactly what SettingsPage.qml does: a copy of the map + an edit.
    let mut m = s::global_to_map(&pb::GlobalSettings::default());
    m.insert(s::KEY_RENDERER_ENABLE_AUDIO.into(), json!(false));
    m.insert(s::KEY_RENDERER_VOLUME.into(), json!(45));
    let g = s::map_to_global(&m);
    let r = g.renderer.expect("renderer должен быть установлен");
    assert_eq!(r.enable_audio, Some(false));
    assert_eq!(r.volume, Some(45));
    // Not a single renderer.* key → the message is not set at all
    // (original: a gate on contains() of both keys).
    let mut m = s::global_to_map(&pb::GlobalSettings::default());
    m.remove(s::KEY_RENDERER_ENABLE_AUDIO);
    m.remove(s::KEY_RENDERER_VOLUME);
    m.remove(s::KEY_RENDERER_AUDIO_OUTPUT);
    assert!(s::map_to_global(&m).renderer.is_none());
    // waywallen 0.6.2: the output device alone is enough to send the message.
    m.insert(s::KEY_RENDERER_AUDIO_OUTPUT.into(), json!("alsa_output.hdmi"));
    let r = s::map_to_global(&m).renderer.expect("renderer with the device");
    assert_eq!((r.enable_audio, r.volume, r.audio_output.as_deref()), (None, None, Some("alsa_output.hdmi")));
}

// ── defect 2: flat plugin map ─────────────────────────────────────

#[test]
fn plugins_map_is_flat_without_values_level() {
    let mut plugins = HashMap::new();
    plugins.insert(
        "mpv".to_string(),
        pb::PluginSettings {
            values: HashMap::from([
                ("fps".to_string(), "30".to_string()),
                ("volume".to_string(), "80".to_string()),
            ]),
        },
    );
    let m = s::plugins_to_map(&plugins);
    assert_eq!(m["mpv"], json!({"fps": "30", "volume": "80"}));
    // Round trip: a map from QML (flat, as PluginSettingsPage builds it).
    let back = s::map_to_plugins(&m);
    assert_eq!(back, plugins);
}

#[test]
fn plugin_values_survive_apply_from_page() {
    // PluginSettingsPage.apply(): a copy of all plugins + the flat map of the
    // changed one. Formerly an empty table went here (the plugin's values were
    // erased by the full replacement).
    let mut m = s::VMap::new();
    m.insert("mpv".into(), json!({"fps": "60"}));
    m.insert("web".into(), json!({"zoom": "1.5"}));
    let out = s::map_to_plugins(&m);
    assert_eq!(out["mpv"].values["fps"], "60");
    assert_eq!(out["web"].values["zoom"], "1.5");
    assert_eq!(out.len(), 2);
}

// ── renamed keys ────────────────────────────────────────────────

#[test]
fn renamed_keys_keep_upstream_inversion() {
    let g = pb::GlobalSettings {
        disable_plugin_update_notifications: false,
        duplicate_renderers_for_same_wallpaper: true,
        ..Default::default()
    };
    let m = s::global_to_map(&g);
    assert_eq!(m["pluginUpdateNotifications"], json!(true)); // INVERSION
    assert_eq!(m["duplicateRenderers"], json!(true));
    assert!(!m.contains_key("disablePluginUpdateNotifications"));
    assert!(!m.contains_key("duplicateRenderersForSameWallpaper"));

    let back = s::map_to_global(&m);
    assert!(!back.disable_plugin_update_notifications);
    assert!(back.duplicate_renderers_for_same_wallpaper);
}

#[test]
fn mute_when_other_audio_collapses_presence_like_upstream() {
    // hasMuteWhenOtherAudio() && muteWhenOtherAudio()
    let m = s::global_to_map(&pb::GlobalSettings::default());
    assert_eq!(m["muteWhenOtherAudio"], json!(false));
    let g = pb::GlobalSettings {
        mute_when_other_audio: Some(false),
        ..Default::default()
    };
    assert_eq!(s::global_to_map(&g)["muteWhenOtherAudio"], json!(false));
    // On the way back the key is always present → the field is set explicitly.
    assert_eq!(
        s::map_to_global(&m).mute_when_other_audio,
        Some(false),
        "ключ есть в карте — оригинал пишет поле"
    );
}

// ── round trip (full replacement loses nothing) ────────────────────────────────

#[test]
fn full_global_survives_round_trip() {
    let g = full_global();
    let back = s::map_to_global(&s::global_to_map(&g));
    assert_eq!(back, g);
}

/// waywallen 0.4.4: `settings_set` is a full replacement, and an `auto_replay`
/// without window exceptions would erase them at the daemon. The page does not
/// edit exceptions, but the map must carry them back; no key (a page of the
/// previous release) — empty lists, like the original's `toStringList()`.
#[test]
fn window_exclusions_and_hidden_filter_survive_settings_set() {
    let g = full_global();
    let m = s::global_to_map(&g);
    let replay = m["autoReplay"].as_object().expect("autoReplay — карта");
    assert_eq!(replay["excludedApplicationIdPatterns"], json!(["steam_app_*"]));
    assert_eq!(m["wallpaperHiddenFilter"], json!(2));
    let back = s::map_to_global(&m);
    assert_eq!(
        back.auto_replay.and_then(|p| p.window_exclusions),
        g.auto_replay.and_then(|p| p.window_exclusions)
    );
    assert_eq!(back.wallpaper_hidden_filter, 2);

    let mut bare = s::auto_replay_to_map(&pb::AutoReplayPolicy::default());
    for k in [
        "excludedApplicationIds",
        "excludedWindowTitles",
        "excludedApplicationIdPatterns",
        "excludedWindowTitlePatterns",
    ] {
        assert_eq!(bare.remove(k), Some(json!([])), "ключ {k}");
    }
    assert_eq!(
        s::map_to_auto_replay(&bare).window_exclusions,
        Some(pb::WindowExclusions::default())
    );
}

#[test]
fn default_global_survives_round_trip() {
    // Empty settings: the round trip must not «invent» either layout_defaults or
    // auto_replay, but it must set renderer (the renderer.* keys are always in
    // the map) — with the same values the reader substituted.
    let g = pb::GlobalSettings::default();
    let back = s::map_to_global(&s::global_to_map(&g));
    assert!(back.layout_defaults.is_none());
    assert!(back.auto_replay.is_none());
    assert_eq!(
        back.renderer,
        Some(pb::GlobalRendererSettings {
            enable_audio: Some(true),
            volume: Some(100),
            // waywallen 0.6.2: the reader always gives the device, empty = the system
            // default, and it goes back as such (the original does the same).
            audio_output: Some(String::new()),
        }),
        "значения по умолчанию читателя уходят обратно явно — \
         наблюдаемое поведение то же (звук включён, громкость 100)"
    );
    // The same with mute_when_other_audio: the key is always in the map, so
    // «not sent» goes back as an explicit false (the value is the same).
    assert_eq!(back.mute_when_other_audio, Some(false));
    // And with the pause effect: the map always carries it, and in full (the
    // page sends settings as a full replacement), so «not sent» goes back as an
    // explicit «no effect, default radius» — the observable behaviour is the
    // same as in the original.
    assert_eq!(
        back.pause_effect,
        Some(pb::PauseEffectConfig {
            kind: pb::PauseEffectKind::None as i32,
            blur: Some(pb::BlurEffectConfig {
                radius: s::DEFAULT_BLUR_RADIUS,
            }),
        })
    );
    assert_eq!(
        pb::GlobalSettings {
            renderer: None,
            mute_when_other_audio: None,
            pause_effect: None,
            ..back
        },
        g,
        "остальные поля круг не меняет"
    );
}

#[test]
fn filter_rule_payload_variants_survive_round_trip() {
    for rule in full_global().wallpaper_filters {
        let back = s::map_to_filter_rule(&s::filter_rule_to_map(&rule));
        assert_eq!(back, rule);
    }
}

#[test]
fn sort_rule_map_uses_keys_read_by_qml() {
    // WallpaperPage.restoreSortFromSettings reads r.key / r.direction.
    let r = pb::WallpaperSortRule {
        key: pb::WallpaperSortKey::Size as i32,
        direction: pb::SortDirection::Asc as i32,
    };
    let m = s::sort_rule_to_map(&r);
    assert_eq!(m["key"], json!(3));
    assert_eq!(m["direction"], json!(1));
    assert_eq!(s::map_to_sort_rule(&m), r);
}

#[test]
fn filter_logic_map_uses_keys_read_by_rust_model() {
    // rexon-qt-filter reads op / groupA / groupB.
    let l = pb::FilterLogic {
        op: pb::LogicOp::And as i32,
        group_a: 2,
        group_b: 3,
    };
    let m = s::filter_logic_to_map(&l);
    assert_eq!(m["op"], json!(1));
    assert_eq!(m["groupA"], json!(2));
    assert_eq!(m["groupB"], json!(3));
    assert_eq!(s::map_to_filter_logic(&m), l);
}

// ── map_to_global semantics on an incomplete map ────────────────────────────

#[test]
fn missing_keys_leave_proto_defaults() {
    // A map without keys: empty lists, default fields, messages are not set. The
    // original behaves the same way (contains() gates).
    let g = s::map_to_global(&s::VMap::new());
    assert_eq!(g, pb::GlobalSettings::default());
}

#[test]
fn audio_capture_enabled_is_written_unconditionally() {
    // The only field the original writes without contains(): no key → false.
    // Kept as is (a quirk of the original).
    let g = s::map_to_global(&s::VMap::new());
    assert!(!g.audio_capture_enabled);
}

#[test]
fn numbers_from_qml_are_coerced_like_qvariant() {
    // QML gives numbers as doubles; string lists may arrive with numbers inside
    // (Object.assign copies whatever it was given).
    let mut m = s::VMap::new();
    m.insert("rotationSecs".into(), json!(600.0));
    m.insert(s::KEY_RENDERER_VOLUME.into(), json!(72.6));
    m.insert("wallpaperSkipTypes".into(), json!(["video", 3]));
    let g = s::map_to_global(&m);
    assert_eq!(g.rotation_secs, 600);
    assert_eq!(g.renderer.unwrap().volume, Some(73));
    assert_eq!(g.wallpaper_skip_types, vec!["video", "3"]);
}

// ── SettingsGetQuery: response parsing and signals ────────────────────────────

fn frame(payload: Option<pb::response::Payload>, status: i32, message: &str) -> Vec<u8> {
    pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 3,
            status,
            message: message.to_string(),
            error_code: 0,
            payload,
        })),
    }
    .encode_to_vec()
}

fn settings_frame(global: Option<pb::GlobalSettings>) -> Vec<u8> {
    frame(
        Some(pb::response::Payload::SettingsGet(pb::SettingsGetResponse {
            global,
            plugins: HashMap::from([(
                "mpv".to_string(),
                pb::PluginSettings {
                    values: HashMap::from([("fps".to_string(), "30".to_string())]),
                },
            )]),
            // waywallen 0.3.8: the response also carries the log directory and the
            // WW_LOG flag; they are not set here.
            ..Default::default()
        })),
        pb::Status::Ok as i32,
        "",
    )
}

#[test]
fn settings_get_fills_properties_and_signals_once() {
    let mut st = q::QueryState::default();
    let mut data = s::SettingsGetData::default();
    let (_, _) = q::begin_call(&mut st, true);

    let sig = s::apply_settings_get(&mut st, &mut data, &settings_frame(Some(full_global())))
        .expect("кадр разобран");
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::Property("global"),
            q::QuerySignal::Property("plugins"),
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
    assert_eq!(data.global["queueMode"], json!("shuffle"));
    assert_eq!(data.plugins["mpv"], json!({"fps": "30"}));

    // The same response again — properties did not change, no property signals
    // (the divergence from the original is documented in we_query.rs).
    let (_, _) = q::begin_call(&mut st, true);
    let sig = s::apply_settings_get(&mut st, &mut data, &settings_frame(Some(full_global())))
        .expect("кадр разобран");
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
fn settings_get_without_global_reads_message_defaults() {
    // The original calls global_to_map(rsp.global()) without a presence check.
    let mut st = q::QueryState::default();
    let mut data = s::SettingsGetData::default();
    s::apply_settings_get(&mut st, &mut data, &settings_frame(None)).expect("кадр разобран");
    assert_eq!(data.global["queueMode"], json!(""));
    assert_eq!(data.global[s::KEY_RENDERER_VOLUME], json!(100));
}

#[test]
fn settings_get_daemon_error_leaves_properties_untouched() {
    let mut st = q::QueryState::default();
    let mut data = s::SettingsGetData::default();
    let bad = frame(None, pb::Status::Internal as i32, "демон не смог");
    let sig = s::apply_settings_get(&mut st, &mut data, &bad).expect("кадр разобран");
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::ErrorChanged,
            q::QuerySignal::StatusChanged(q::STATUS_ERROR),
            q::QuerySignal::ErrorOccurred("демон не смог".into()),
            q::QuerySignal::AppErrorOccurred("демон не смог".into()),
        ]
    );
    assert!(data.global.is_empty());
    assert!(data.plugins.is_empty());
}

// ── SettingsSetQuery: building the request ─────────────────────────────────────

#[test]
fn settings_set_request_round_trips_through_wire() {
    let g = full_global();
    let global = s::global_to_map(&g);
    let mut plugins = s::VMap::new();
    plugins.insert("mpv".into(), json!({"fps": "60"}));

    let bytes = s::encode_settings_set(&global, &plugins);
    let req = pb::SettingsSetRequest::decode(bytes.as_slice()).expect("запрос разобран");
    assert_eq!(req.global.as_ref(), Some(&g));
    assert_eq!(req.plugins["mpv"].values["fps"], "60");
}

#[test]
fn settings_set_keeps_empty_plugin_table() {
    // No plugins — send an empty map (the «do not write before the first read»
    // gate lives in QML, the core does not second-guess).
    let bytes = s::encode_settings_set(&s::VMap::new(), &s::VMap::new());
    let req = pb::SettingsSetRequest::decode(bytes.as_slice()).expect("запрос разобран");
    assert!(req.plugins.is_empty());
    assert_eq!(req.global, Some(pb::GlobalSettings::default()));
}

#[test]
fn non_map_list_items_do_not_panic() {
    // Insurance against garbage from JS: a list element is not a map.
    let mut m = s::VMap::new();
    m.insert("wallpaperSorts".into(), Value::Array(vec![json!(7)]));
    let g = s::map_to_global(&m);
    assert_eq!(g.wallpaper_sorts, vec![pb::WallpaperSortRule::default()]);
}

// ── waywallen 0.4.2: pause scope, GameMode, change animation ───────────────

#[test]
fn auto_replay_scopes_and_gamemode_round_trip() {
    let g = full_global();
    let m = s::auto_replay_to_map(g.auto_replay.as_ref().unwrap());
    assert_eq!(m["gamemode"], json!(pb::AutoAction::Pause as i32));
    assert_eq!(m["anyWindowScope"], json!(pb::AutoScope::AllDisplays as i32));
    assert_eq!(m["focusedScope"], json!(pb::AutoScope::CurrentDisplay as i32));
    assert_eq!(&s::map_to_auto_replay(&m), g.auto_replay.as_ref().unwrap());
    // A page of the previous release does not know the keys → 0: GameMode does
    // not touch the wallpaper, pause only on its own monitor — the former daemon behaviour.
    let mut bare = m.clone();
    for k in ["gamemode", "anyWindowScope", "focusedScope", "maximizedScope", "fullscreenScope"] {
        bare.remove(k);
    }
    let p = s::map_to_auto_replay(&bare);
    assert_eq!(p.gamemode, pb::AutoAction::None as i32);
    assert_eq!(p.any_window_scope, pb::AutoScope::CurrentDisplay as i32);
}

// ── waywallen 0.5.0: the «on battery power» reason ─────────────────────────

#[test]
fn auto_replay_on_battery_survives_settings_round_trip() {
    // There is no row for this reason on the page, but a value set in another
    // client must survive a save from Rexon: `settings_set` replaces the policy entirely.
    let g = full_global();
    let m = s::auto_replay_to_map(g.auto_replay.as_ref().unwrap());
    assert_eq!(m["onBattery"], json!(pb::AutoAction::Stop as i32));
    let p = s::map_to_auto_replay(&m);
    assert_eq!(p.on_battery, pb::AutoAction::Stop as i32);
    // Through the full settings map — the same path as the «Save» button.
    let back = s::map_to_global(&s::global_to_map(&g));
    assert_eq!(
        back.auto_replay.as_ref().unwrap().on_battery,
        pb::AutoAction::Stop as i32
    );
    // No key (a map of the previous release) — 0, the daemon default.
    let mut bare = m.clone();
    bare.remove("onBattery");
    assert_eq!(
        s::map_to_auto_replay(&bare).on_battery,
        pb::AutoAction::None as i32
    );
}

#[test]
fn transition_round_trips_and_defaults_like_upstream() {
    let g = full_global();
    let m = s::global_to_map(&g);
    assert_eq!(s::map_to_global(&m).transition, g.transition);
    // `map_to_transition` v0.4.2: no key — 500 ms and centre 50/50.
    let t = s::map_to_transition(&s::VMap::new());
    assert_eq!(t.kind, pb::TransitionKind::None as i32);
    assert_eq!(t.duration_ms, s::DEFAULT_TRANSITION_DURATION_MS);
    assert_eq!(t.angle, 0);
    assert_eq!((t.origin_x, t.origin_y), (50, 50));
}

/// waywallen 0.5.0: hidden libraries. The key exists only when the field was
/// sent (the original's `hasWallpaperLibraryVisibility()`); without the key
/// the field is not set — a `settings_set` from a page that does not know
/// hidden libraries (or from a daemon before 0.5.0) does not erase them. A
/// sent empty list is a legitimate «show all» and must arrive back as an
/// empty message.
#[test]
fn hidden_library_ids_survive_settings_set_and_absent_means_untouched() {
    let g = full_global();
    let m = s::global_to_map(&g);
    assert_eq!(m["wallpaperHiddenLibraryIds"], json!([4, 9]));
    assert_eq!(
        s::map_to_global(&m).wallpaper_library_visibility,
        g.wallpaper_library_visibility
    );

    // Not sent — no key, and the field does not appear on the way back.
    let bare = s::global_to_map(&pb::GlobalSettings::default());
    assert!(!bare.contains_key("wallpaperHiddenLibraryIds"));
    assert_eq!(s::map_to_global(&bare).wallpaper_library_visibility, None);

    // Sent empty — the key is there, an empty list, an empty message on the way back.
    let empty = pb::GlobalSettings {
        wallpaper_library_visibility: Some(pb::WallpaperLibraryVisibility::default()),
        ..Default::default()
    };
    let m = s::global_to_map(&empty);
    assert_eq!(m["wallpaperHiddenLibraryIds"], json!([]));
    assert_eq!(
        s::map_to_global(&m).wallpaper_library_visibility,
        Some(pb::WallpaperLibraryVisibility::default())
    );

    // The page writes numbers as doubles (QML gives them that way) — integers.
    let mut from_qml = m.clone();
    from_qml.insert("wallpaperHiddenLibraryIds".into(), json!([3.0, 1.0]));
    assert_eq!(
        s::map_to_global(&from_qml).wallpaper_library_visibility,
        Some(pb::WallpaperLibraryVisibility { hidden_library_ids: vec![3, 1] })
    );
}
