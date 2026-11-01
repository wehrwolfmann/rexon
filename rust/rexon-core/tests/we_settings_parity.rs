//! Сверка порта `we_settings.rs` с оригиналом waywallen —
//! `ui/src/query/settings_query.cpp` (`global_to_map`/`map_to_global`,
//! `plugins_to_map`/`map_to_plugins`, `SettingsGetQuery::reload`).
//!
//! Главное, что проверяется, — КРУГ: демон обрабатывает `settings_set` как
//! полную замену, поэтому карта, полученная из `settings_get`, обязана
//! собраться обратно в то же самое сообщение. Плюс набор ключей 1-в-1 с
//! оригиналом (QML читает именно их) и два дефекта прежнего Python-моста,
//! починенные этим портом: плоские `renderer.*` и плоская карта плагинов.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_query as q;
use rexon_core::we_settings as s;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Полностью заполненные настройки: круг проверяется на них, чтобы ни одно
/// поле сообщения не осталось без покрытия.
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
            // Правило без payload — законное состояние свежей строки
            // редактора фильтров.
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
            // waywallen 0.4.4: в layout_defaults отражение не используется.
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
            // waywallen 0.4.2: повод GameMode и охват пауз по окнам.
            any_window_scope: pb::AutoScope::AllDisplays as i32,
            focused_scope: pb::AutoScope::CurrentDisplay as i32,
            maximized_scope: pb::AutoScope::AllDisplays as i32,
            fullscreen_scope: pb::AutoScope::AllDisplays as i32,
            gamemode: pb::AutoAction::Pause as i32,
            // waywallen 0.5.0: пауза при питании от батареи.
            on_battery: pb::AutoAction::Stop as i32,
            // waywallen 0.4.4: исключения окон — все четыре списка.
            window_exclusions: Some(pb::WindowExclusions {
                application_ids: vec!["org.kde.konsole".into()],
                titles: vec!["Просмотр видео".into()],
                application_id_patterns: vec!["steam_app_*".into()],
                title_patterns: vec!["*— Firefox".into(), "Игра?".into()],
            }),
        }),
        // waywallen 0.4.4: показывать скрытые вместе с остальными.
        wallpaper_hidden_filter: pb::WallpaperHiddenFilter::Include as i32,
        // waywallen 0.5.0: скрытые библиотеки страницы «Обои».
        wallpaper_library_visibility: Some(pb::WallpaperLibraryVisibility {
            hidden_library_ids: vec![4, 9],
        }),
        audio_fade_ms: 750,
        disable_plugin_update_notifications: true,
        duplicate_renderers_for_same_wallpaper: true,
        renderer: Some(pb::GlobalRendererSettings {
            enable_audio: Some(false),
            volume: Some(37),
        }),
        mute_when_other_audio: Some(true),
        audio_capture_enabled: true,
        pause_effect: Some(pb::PauseEffectConfig {
            kind: pb::PauseEffectKind::Blur as i32,
            blur: Some(pb::BlurEffectConfig { radius: 42 }),
        }),
        pointer_forwarding_enabled: true,
        hide_tray_icon: true,
        // waywallen 0.4.2: анимация смены обоев.
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

// ── набор ключей карты ───────────────────────────────────────────────────

#[test]
fn global_map_keys_match_upstream_global_to_map() {
    let m = s::global_to_map(&full_global());
    let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
    keys.sort_unstable();
    // Ровно список из global_to_map (settings_query.cpp; ключ
    // `debugLoggingEnabled` добавлен вендором в v0.3.8, `transition` — в
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
        "wallpaperSkipTypes",
        "wallpaperFilterTags",
        "wallpaperSkipContentRatings",
        // v0.4.4
        "wallpaperHiddenFilter",
        // v0.5.0 (только когда демон прислал поле)
        "wallpaperHiddenLibraryIds",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected);
}

#[test]
fn global_map_schema_covers_every_key() {
    // Схема для шима и карта из global_to_map обязаны совпадать ключ в
    // ключ: шим по схеме достаёт значения из QJSValue, пропущенный ключ
    // молча потерялся бы на записи (полная замена → настройка стёрта).
    let m = s::global_to_map(&full_global());
    let mut from_map: Vec<&str> = m.keys().map(String::as_str).collect();
    let mut from_schema: Vec<&str> = s::GLOBAL_MAP_SCHEMA.iter().map(|(k, _)| *k).collect();
    from_map.sort_unstable();
    from_schema.sort_unstable();
    assert_eq!(from_schema, from_map);
}

#[test]
fn unset_message_fields_are_absent_like_upstream() {
    // hasLayoutDefaults()/hasAutoReplay() ложны → ключей нет вовсе
    // (QML отличает «не пришло» от «пришло пустым»).
    let m = s::global_to_map(&pb::GlobalSettings::default());
    assert!(!m.contains_key("layoutDefaults"));
    assert!(!m.contains_key("autoReplay"));
    // v0.4.2: `hasTransition()` ложно — ключа нет, как у оригинала.
    assert!(!m.contains_key("transition"));
    // …а скалярные ключи есть всегда, со значениями по умолчанию.
    assert_eq!(m["queueMode"], json!(""));
    assert_eq!(m["rotationSecs"], json!(0));
    assert_eq!(m["muteWhenOtherAudio"], json!(false));
    assert_eq!(m["wallpaperFilters"], json!([]));
}

// ── задержка возобновления (поле v0.3.9) ─────────────────────────────────

#[test]
fn auto_replay_resume_delay_matches_upstream_default() {
    // Поле необязательное, и демон 0.3.8 его не пришлёт вовсе: у вендора
    // `hasResumeDelayMs()` ложен → в карту уходит 250, а не ноль.
    let m = s::auto_replay_to_map(&pb::AutoReplayPolicy::default());
    assert_eq!(
        m["resumeDelayMs"],
        json!(s::DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS)
    );
    // Присланное число доезжает как есть — включая ноль («без задержки»).
    let m = s::auto_replay_to_map(&pb::AutoReplayPolicy {
        resume_delay_ms: Some(0),
        ..Default::default()
    });
    assert_eq!(m["resumeDelayMs"], json!(0));
    // …и собирается обратно в сообщение: `settings_set` — полная замена,
    // потерянный ключ стёр бы настройку.
    assert_eq!(s::map_to_auto_replay(&m).resume_delay_ms, Some(0));
    // Ключа нет вовсе (страница из прошлого выпуска) → снова 250,
    // как `m.value("resumeDelayMs", 250)` оригинала.
    let mut bare = s::auto_replay_to_map(&pb::AutoReplayPolicy::default());
    bare.remove("resumeDelayMs");
    assert_eq!(
        s::map_to_auto_replay(&bare).resume_delay_ms,
        Some(s::DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS)
    );
}

// ── дефект 1: плоские ключи renderer.* ───────────────────────────────────

#[test]
fn renderer_settings_are_flat_keys_with_upstream_defaults() {
    // Нет сообщения renderer → звук включён, громкость 100.
    let m = s::global_to_map(&pb::GlobalSettings::default());
    assert_eq!(m[s::KEY_RENDERER_ENABLE_AUDIO], json!(true));
    assert_eq!(m[s::KEY_RENDERER_VOLUME], json!(100));
    // Сообщение есть, но поля не установлены → те же значения.
    let g = pb::GlobalSettings {
        renderer: Some(pb::GlobalRendererSettings::default()),
        ..Default::default()
    };
    let m = s::global_to_map(&g);
    assert_eq!(m[s::KEY_RENDERER_ENABLE_AUDIO], json!(true));
    assert_eq!(m[s::KEY_RENDERER_VOLUME], json!(100));
    // Установленные значения приезжают как есть, вложенной карты нет.
    let m = s::global_to_map(&full_global());
    assert_eq!(m[s::KEY_RENDERER_ENABLE_AUDIO], json!(false));
    assert_eq!(m[s::KEY_RENDERER_VOLUME], json!(37));
    assert!(!m.contains_key("renderer"));
}

#[test]
fn renderer_flat_keys_write_back_into_message() {
    // Ровно то, что делает SettingsPage.qml: копия карты + правка.
    let mut m = s::global_to_map(&pb::GlobalSettings::default());
    m.insert(s::KEY_RENDERER_ENABLE_AUDIO.into(), json!(false));
    m.insert(s::KEY_RENDERER_VOLUME.into(), json!(45));
    let g = s::map_to_global(&m);
    let r = g.renderer.expect("renderer должен быть установлен");
    assert_eq!(r.enable_audio, Some(false));
    assert_eq!(r.volume, Some(45));
    // Нет ни одного ключа renderer.* → сообщение не ставится вовсе
    // (оригинал: gate на contains() обоих ключей).
    let mut m = s::global_to_map(&pb::GlobalSettings::default());
    m.remove(s::KEY_RENDERER_ENABLE_AUDIO);
    m.remove(s::KEY_RENDERER_VOLUME);
    assert!(s::map_to_global(&m).renderer.is_none());
}

// ── дефект 2: плоская карта плагинов ─────────────────────────────────────

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
    // Круг: карта из QML (плоская, как её собирает PluginSettingsPage).
    let back = s::map_to_plugins(&m);
    assert_eq!(back, plugins);
}

#[test]
fn plugin_values_survive_apply_from_page() {
    // PluginSettingsPage.apply(): копия всех плагинов + плоская карта
    // изменённого. Раньше сюда уезжала пустая таблица (значения плагина
    // стирались полной заменой).
    let mut m = s::VMap::new();
    m.insert("mpv".into(), json!({"fps": "60"}));
    m.insert("web".into(), json!({"zoom": "1.5"}));
    let out = s::map_to_plugins(&m);
    assert_eq!(out["mpv"].values["fps"], "60");
    assert_eq!(out["web"].values["zoom"], "1.5");
    assert_eq!(out.len(), 2);
}

// ── переименованные ключи ────────────────────────────────────────────────

#[test]
fn renamed_keys_keep_upstream_inversion() {
    let g = pb::GlobalSettings {
        disable_plugin_update_notifications: false,
        duplicate_renderers_for_same_wallpaper: true,
        ..Default::default()
    };
    let m = s::global_to_map(&g);
    assert_eq!(m["pluginUpdateNotifications"], json!(true)); // ИНВЕРСИЯ
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
    // Обратно ключ всегда присутствует → поле ставится явно.
    assert_eq!(
        s::map_to_global(&m).mute_when_other_audio,
        Some(false),
        "ключ есть в карте — оригинал пишет поле"
    );
}

// ── круг (полная замена ничего не теряет) ────────────────────────────────

#[test]
fn full_global_survives_round_trip() {
    let g = full_global();
    let back = s::map_to_global(&s::global_to_map(&g));
    assert_eq!(back, g);
}

/// waywallen 0.4.4: `settings_set` — полная замена, и `auto_replay` без
/// исключений окон стёр бы их у демона. Страница исключения не правит,
/// но карта обязана донести их обратно; ключа нет (страница прошлого
/// выпуска) — пустые списки, как `toStringList()` оригинала.
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
    // Пустые настройки: круг не должен «выдумывать» ни layout_defaults,
    // ни auto_replay, но обязан выставить renderer (ключи renderer.* в
    // карте есть всегда) — с теми же значениями, что подставил читатель.
    let g = pb::GlobalSettings::default();
    let back = s::map_to_global(&s::global_to_map(&g));
    assert!(back.layout_defaults.is_none());
    assert!(back.auto_replay.is_none());
    assert_eq!(
        back.renderer,
        Some(pb::GlobalRendererSettings {
            enable_audio: Some(true),
            volume: Some(100),
        }),
        "значения по умолчанию читателя уходят обратно явно — \
         наблюдаемое поведение то же (звук включён, громкость 100)"
    );
    // То же с mute_when_other_audio: ключ в карте есть всегда, значит
    // «не прислано» уходит обратно явным false (значение то же).
    assert_eq!(back.mute_when_other_audio, Some(false));
    // И с эффектом на паузе: карта его несёт всегда и полной (страница
    // шлёт настройки полной заменой), поэтому «не прислано» уходит назад
    // явным «эффекта нет, радиус по умолчанию» — наблюдаемое поведение то
    // же, что у оригинала.
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
    // WallpaperPage.restoreSortFromSettings читает r.key / r.direction.
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
    // rexon-qt-filter читает op / groupA / groupB.
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

// ── семантика map_to_global на неполной карте ────────────────────────────

#[test]
fn missing_keys_leave_proto_defaults() {
    // Карта без ключей: списки пустые, поля по умолчанию, сообщения не
    // ставятся. Оригинал ведёт себя так же (contains()-гейты).
    let g = s::map_to_global(&s::VMap::new());
    assert_eq!(g, pb::GlobalSettings::default());
}

#[test]
fn audio_capture_enabled_is_written_unconditionally() {
    // Единственное поле, которое оригинал пишет без contains(): нет
    // ключа → false. Сохранено как есть (квирк оригинала).
    let g = s::map_to_global(&s::VMap::new());
    assert!(!g.audio_capture_enabled);
}

#[test]
fn numbers_from_qml_are_coerced_like_qvariant() {
    // QML отдаёт числа double'ами; строковые списки могут прийти с
    // числами внутри (Object.assign копирует что дали).
    let mut m = s::VMap::new();
    m.insert("rotationSecs".into(), json!(600.0));
    m.insert(s::KEY_RENDERER_VOLUME.into(), json!(72.6));
    m.insert("wallpaperSkipTypes".into(), json!(["video", 3]));
    let g = s::map_to_global(&m);
    assert_eq!(g.rotation_secs, 600);
    assert_eq!(g.renderer.unwrap().volume, Some(73));
    assert_eq!(g.wallpaper_skip_types, vec!["video", "3"]);
}

// ── SettingsGetQuery: разбор ответа и сигналы ────────────────────────────

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
            // waywallen 0.3.8: ответ несёт ещё каталог журнала и признак
            // WW_LOG; здесь они не заданы.
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

    // Тот же ответ повторно — свойства не менялись, сигналов свойств нет
    // (расхождение с оригиналом задокументировано в we_query.rs).
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
    // Оригинал зовёт global_to_map(rsp.global()) без проверки присутствия.
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

// ── SettingsSetQuery: сборка запроса ─────────────────────────────────────

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
    // Плагинов нет — шлём пустую карту (гейт «не писать до первого
    // чтения» живёт в QML, ядро не додумывает).
    let bytes = s::encode_settings_set(&s::VMap::new(), &s::VMap::new());
    let req = pb::SettingsSetRequest::decode(bytes.as_slice()).expect("запрос разобран");
    assert!(req.plugins.is_empty());
    assert_eq!(req.global, Some(pb::GlobalSettings::default()));
}

#[test]
fn non_map_list_items_do_not_panic() {
    // Страховка от мусора из JS: элемент списка не карта.
    let mut m = s::VMap::new();
    m.insert("wallpaperSorts".into(), Value::Array(vec![json!(7)]));
    let g = s::map_to_global(&m);
    assert_eq!(g.wallpaper_sorts, vec![pb::WallpaperSortRule::default()]);
}

// ── waywallen 0.4.2: охват пауз, GameMode, анимация смены ───────────────

#[test]
fn auto_replay_scopes_and_gamemode_round_trip() {
    let g = full_global();
    let m = s::auto_replay_to_map(g.auto_replay.as_ref().unwrap());
    assert_eq!(m["gamemode"], json!(pb::AutoAction::Pause as i32));
    assert_eq!(m["anyWindowScope"], json!(pb::AutoScope::AllDisplays as i32));
    assert_eq!(m["focusedScope"], json!(pb::AutoScope::CurrentDisplay as i32));
    assert_eq!(&s::map_to_auto_replay(&m), g.auto_replay.as_ref().unwrap());
    // Страница прошлого выпуска ключей не знает → 0: GameMode не трогает
    // обои, пауза только на своём мониторе — прежнее поведение демона.
    let mut bare = m.clone();
    for k in ["gamemode", "anyWindowScope", "focusedScope", "maximizedScope", "fullscreenScope"] {
        bare.remove(k);
    }
    let p = s::map_to_auto_replay(&bare);
    assert_eq!(p.gamemode, pb::AutoAction::None as i32);
    assert_eq!(p.any_window_scope, pb::AutoScope::CurrentDisplay as i32);
}

// ── waywallen 0.5.0: повод «питание от батареи» ─────────────────────────

#[test]
fn auto_replay_on_battery_survives_settings_round_trip() {
    // Строки под этот повод на странице нет, но значение, выставленное в
    // другом клиенте, обязано пережить сохранение из Rexon: `settings_set`
    // заменяет политику целиком.
    let g = full_global();
    let m = s::auto_replay_to_map(g.auto_replay.as_ref().unwrap());
    assert_eq!(m["onBattery"], json!(pb::AutoAction::Stop as i32));
    let p = s::map_to_auto_replay(&m);
    assert_eq!(p.on_battery, pb::AutoAction::Stop as i32);
    // Через полную карту настроек — тот же путь, что у кнопки «Сохранить».
    let back = s::map_to_global(&s::global_to_map(&g));
    assert_eq!(
        back.auto_replay.as_ref().unwrap().on_battery,
        pb::AutoAction::Stop as i32
    );
    // Ключа нет (карта прежнего выпуска) — 0, умолчание демона.
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
    // `map_to_transition` v0.4.2: нет ключа — 500 мс и центр 50/50.
    let t = s::map_to_transition(&s::VMap::new());
    assert_eq!(t.kind, pb::TransitionKind::None as i32);
    assert_eq!(t.duration_ms, s::DEFAULT_TRANSITION_DURATION_MS);
    assert_eq!(t.angle, 0);
    assert_eq!((t.origin_x, t.origin_y), (50, 50));
}

/// waywallen 0.5.0: скрытые библиотеки. Ключ есть только при присланном
/// поле (`hasWallpaperLibraryVisibility()` оригинала); без ключа поле не
/// ставится — `settings_set` от страницы, не знающей скрытых библиотек
/// (или от демона до 0.5.0), не стирает их. Присланный пустой список —
/// законное «показать все» и обязан доехать обратно пустым сообщением.
#[test]
fn hidden_library_ids_survive_settings_set_and_absent_means_untouched() {
    let g = full_global();
    let m = s::global_to_map(&g);
    assert_eq!(m["wallpaperHiddenLibraryIds"], json!([4, 9]));
    assert_eq!(
        s::map_to_global(&m).wallpaper_library_visibility,
        g.wallpaper_library_visibility
    );

    // Не прислано — ключа нет, и обратно поле не появляется.
    let bare = s::global_to_map(&pb::GlobalSettings::default());
    assert!(!bare.contains_key("wallpaperHiddenLibraryIds"));
    assert_eq!(s::map_to_global(&bare).wallpaper_library_visibility, None);

    // Прислано пустым — ключ есть, пустой список, обратно пустое сообщение.
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

    // Страница пишет номера double'ами (так их отдаёт QML) — целые.
    let mut from_qml = m.clone();
    from_qml.insert("wallpaperHiddenLibraryIds".into(), json!([3.0, 1.0]));
    assert_eq!(
        s::map_to_global(&from_qml).wallpaper_library_visibility,
        Some(pb::WallpaperLibraryVisibility { hidden_library_ids: vec![3, 1] })
    );
}
