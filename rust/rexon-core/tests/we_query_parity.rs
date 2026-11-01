//! Сверка порта `we_query.rs` с оригиналом waywallen: `qextra::QAsyncResult`
//! (`async.cpp`), `qextra::Query` (`query.cpp`) и запросы
//! `ui/src/query/{health,renderer,tag}_query.cpp`.
//!
//! Проверяется то, ради чего база вообще существует: ПОРЯДОК сигналов при
//! смене статуса, молчание при повторе того же значения, ветка ошибки
//! демона и глобальная пересылка по `forwardError`. Кадры собираются тем
//! же prost'ом (схема общая с демоном) — живой демон не дёргается.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_query as q;

/// Кадр ответа: статус OK и полезная нагрузка.
fn ok_frame(payload: pb::response::Payload) -> Vec<u8> {
    frame(pb::Status::Ok as i32, String::new(), Some(payload))
}

fn frame(status: i32, message: String, payload: Option<pb::response::Payload>) -> Vec<u8> {
    pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 7,
            status,
            message,
            error_code: 0,
            payload,
        })),
    }
    .encode_to_vec()
}

// ── база: автомат статуса ──────────────────────────────────────────────

#[test]
fn fresh_state_matches_qasyncresult_defaults() {
    let st = q::QueryState::default();
    // async.cpp: m_status(Status::Uninitialized), m_error пустой,
    // m_forward_error(true).
    assert_eq!(st.status, q::STATUS_UNINITIALIZED);
    assert!(st.error.is_empty());
    assert!(st.forward_error);
}

#[test]
fn status_change_order_is_status_then_querying_then_finished() {
    let mut st = q::QueryState::default();
    // Uninitialized → Querying: querying стало true.
    assert_eq!(
        q::set_status(&mut st, q::STATUS_QUERYING),
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_QUERYING),
            q::QuerySignal::QueryingChanged,
        ]
    );
    // Querying → Finished: querying стало false, следом finished().
    assert_eq!(
        q::set_status(&mut st, q::STATUS_FINISHED),
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
}

#[test]
fn same_status_is_silent() {
    // Биндабельное свойство уведомляет только при изменении значения.
    let mut st = q::QueryState::default();
    q::set_status(&mut st, q::STATUS_FINISHED);
    assert!(q::set_status(&mut st, q::STATUS_FINISHED).is_empty());
}

#[test]
fn error_status_emits_error_occurred_then_forwards_to_app() {
    let mut st = q::QueryState::default();
    q::set_error(&mut st, "демон упал");
    // async.cpp: сначала errorOccurred(error()), потом — если стоит
    // forwardError — глобальный обработчик.
    assert_eq!(
        q::set_status(&mut st, q::STATUS_ERROR),
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_ERROR),
            q::QuerySignal::ErrorOccurred("демон упал".into()),
            q::QuerySignal::AppErrorOccurred("демон упал".into()),
        ]
    );
}

#[test]
fn forward_error_false_keeps_error_local() {
    // Страницы, показывающие ошибку сами (DiscoverPage, SettingsPage,
    // WallpaperDetailPanel), ставят forwardError: false.
    let mut st = q::QueryState::default();
    assert_eq!(
        q::set_forward_error(&mut st, false),
        vec![q::QuerySignal::ForwardErrorChanged]
    );
    assert!(q::set_forward_error(&mut st, false).is_empty());
    q::set_error(&mut st, "нет источника");
    let sig = q::set_status(&mut st, q::STATUS_ERROR);
    assert!(!sig.contains(&q::QuerySignal::AppErrorOccurred("нет источника".into())));
    assert!(sig.contains(&q::QuerySignal::ErrorOccurred("нет источника".into())));
}

#[test]
fn set_error_notifies_only_on_change() {
    let mut st = q::QueryState::default();
    assert_eq!(
        q::set_error(&mut st, "сбой"),
        vec![q::QuerySignal::ErrorChanged]
    );
    assert!(q::set_error(&mut st, "сбой").is_empty());
}

#[test]
fn begin_call_without_connection_goes_straight_to_error() {
    let mut st = q::QueryState::default();
    let (send, sig) = q::begin_call(&mut st, false);
    assert!(!send, "кадр не должен уходить в мёртвый сокет");
    assert_eq!(st.status, q::STATUS_ERROR);
    assert_eq!(st.error, rexon_core::i18n::tr(q::NO_CONNECTION));
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::ErrorChanged,
            q::QuerySignal::StatusChanged(q::STATUS_ERROR),
            q::QuerySignal::ErrorOccurred(rexon_core::i18n::tr(q::NO_CONNECTION)),
            q::QuerySignal::AppErrorOccurred(rexon_core::i18n::tr(q::NO_CONNECTION)),
        ]
    );
}

#[test]
fn begin_call_connected_switches_to_querying() {
    let mut st = q::QueryState::default();
    let (send, sig) = q::begin_call(&mut st, true);
    assert!(send);
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_QUERYING),
            q::QuerySignal::QueryingChanged,
        ]
    );
}

#[test]
fn daemon_error_text_falls_back_to_status_number() {
    // backend.cpp: пустой message → "status N", иначе сам message.
    assert_eq!(q::response_error_text(3, ""), "status 3");
    assert_eq!(q::response_error_text(3, "не найдено"), "не найдено");
}

#[test]
fn non_ok_response_sets_error_and_skips_payload() {
    let mut st = q::QueryState::default();
    let mut data = q::HealthData::default();
    q::begin_call(&mut st, true);
    let f = frame(
        pb::Status::NotFound as i32,
        String::new(),
        Some(pb::response::Payload::Health(pb::HealthResponse {
            service: "waywallen".into(),
            state: "ok".into(),
            ..Default::default()
        })),
    );
    let sig = q::apply_health(&mut st, &mut data, &f).unwrap();
    // Полезная нагрузка при не-OK не читается (inspect_set оригинала
    // зовёт лямбду только на успехе).
    assert_eq!(data, q::HealthData::default());
    assert_eq!(st.status, q::STATUS_ERROR);
    assert_eq!(st.error, "status 3");
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::ErrorChanged,
            q::QuerySignal::StatusChanged(q::STATUS_ERROR),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::ErrorOccurred("status 3".into()),
            q::QuerySignal::AppErrorOccurred("status 3".into()),
        ]
    );
}

#[test]
fn broken_frame_is_error_not_panic() {
    let mut st = q::QueryState::default();
    let mut data = q::HealthData::default();
    assert!(q::apply_health(&mut st, &mut data, &[0xff, 0xff, 0xff]).is_err());
    // Состояние не тронуто — шим пишет отладку и живёт дальше.
    assert_eq!(st, q::QueryState::default());
}

#[test]
fn event_frame_is_rejected_as_undecodable() {
    let mut st = q::QueryState::default();
    let mut data = q::HealthData::default();
    let f = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Event(pb::Event { payload: None })),
    }
    .encode_to_vec();
    assert!(q::apply_health(&mut st, &mut data, &f).is_err());
}

// ── HealthQuery ────────────────────────────────────────────────────────

#[test]
fn health_fills_properties_and_repeats_silently() {
    let mut st = q::QueryState::default();
    let mut data = q::HealthData::default();
    q::begin_call(&mut st, true);
    let f = ok_frame(pb::response::Payload::Health(pb::HealthResponse {
        service: "waywallen".into(),
        state: "running".into(),
        os_name: "CachyOS".into(),
    }));
    let sig = q::apply_health(&mut st, &mut data, &f).unwrap();
    assert_eq!(data.service, "waywallen");
    assert_eq!(data.state, "running");
    assert_eq!(data.os_name, "CachyOS");
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::Property("service"),
            q::QuerySignal::Property("state"),
            q::QuerySignal::Property("osName"),
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
    // Повтор того же ответа: свойства молчат (расхождение №1 с
    // оригиналом — он эмитил бы serviceChanged/stateChanged снова),
    // статус тоже не менялся.
    q::begin_call(&mut st, true);
    let sig = q::apply_health(&mut st, &mut data, &f).unwrap();
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
}

// ── RendererListQuery ──────────────────────────────────────────────────

#[test]
fn renderer_list_maps_instance_fields() {
    let mut st = q::QueryState::default();
    let mut data = q::RendererListData::default();
    let f = ok_frame(pb::response::Payload::RendererList(
        pb::RendererListResponse {
            renderers: vec!["wescene".into(), "web".into()],
            instances: vec![pb::RendererInstance {
                renderer_id: "r-1".into(),
                fps: 60,
                // v0.3.5: строки `status` на проводе больше нет, состояние
                // приезжает деревом — строку собирает ядро.
                state: Some(pb::RendererState {
                    kind: Some(pb::renderer_state::Kind::Running(
                        pb::RendererRunningState {
                            generation: 3,
                            activity: pb::RendererActivity::Playing as i32,
                        },
                    )),
                }),
                name: "wescene".into(),
                pid: 4242,
                drm_render_major: 226,
                drm_render_minor: 128,
                texture_width: 1920,
                texture_height: 1080,
                ..Default::default()
            }],
        },
    ));
    let sig = q::apply_renderer_list(&mut st, &mut data, &f).unwrap();
    assert_eq!(data.renderers, vec!["wescene", "web"]);
    let i = &data.instances[0];
    // renderer_id → id (ключ карты моста), остальное 1-в-1.
    assert_eq!(i.id, "r-1");
    assert_eq!(i.fps, 60);
    assert_eq!(i.status, "playing");
    assert!(i.running);
    assert!(!i.keep);
    assert_eq!(i.process_generation, 3);
    assert_eq!(i.name, "wescene");
    assert_eq!(i.pid, 4242);
    assert_eq!((i.texture_width, i.texture_height), (1920, 1080));
    assert_eq!((i.drm_render_major, i.drm_render_minor), (226, 128));
    assert_eq!(sig[0], q::QuerySignal::Property("renderers"));
    assert_eq!(sig[1], q::QuerySignal::Property("instances"));
}

#[test]
fn renderer_list_reports_only_changed_property() {
    let mut st = q::QueryState::default();
    let mut data = q::RendererListData::default();
    let base = |fps: u32| {
        ok_frame(pb::response::Payload::RendererList(
            pb::RendererListResponse {
                renderers: vec!["wescene".into()],
                instances: vec![pb::RendererInstance {
                    renderer_id: "r-1".into(),
                    fps,
                    ..Default::default()
                }],
            },
        ))
    };
    q::apply_renderer_list(&mut st, &mut data, &base(30)).unwrap();
    let sig = q::apply_renderer_list(&mut st, &mut data, &base(60)).unwrap();
    // Список движков тот же — сигнал только по instances.
    assert_eq!(sig, vec![q::QuerySignal::Property("instances")]);
    assert_eq!(data.instances[0].fps, 60);
}

// ── RendererPluginListQuery ────────────────────────────────────────────

#[test]
fn renderer_plugin_list_carries_setting_schema() {
    let mut st = q::QueryState::default();
    let mut data = q::RendererPluginListData::default();
    let f = ok_frame(pb::response::Payload::RendererPluginList(
        pb::RendererPluginListResponse {
            renderers: vec![pb::RendererPluginInfo {
                name: "wescene".into(),
                bin: "/usr/bin/wescene-renderer".into(),
                types: vec!["scene".into()],
                priority: 10,
                version: "0.2.6".into(),
                settings: vec![pb::SettingSchema {
                    key: "resolution".into(),
                    r#type: pb::SettingValueType::String as i32,
                    default_value: "auto".into(),
                    identity: true,
                    label_key: "settings.resolution".into(),
                    description_key: "settings.resolution.desc".into(),
                    min: String::new(),
                    max: String::new(),
                    step: String::new(),
                    choices: vec!["auto".into(), "native".into()],
                    group: "render".into(),
                    order: 3,
                    // Локализуемые подписи схемы — протокол 0.3.7.
                    ..Default::default()
                }],
                plugin_id: "io.waywallen.wescene".into(),
            }],
            supported_types: vec!["scene".into(), "video".into()],
            // Подписи типов — протокол 0.4.2; этот случай их не касается.
            ..Default::default()
        },
    ));
    q::apply_renderer_plugin_list(&mut st, &mut data, &f).unwrap();
    let p = &data.renderers[0];
    assert_eq!(p.name, "wescene");
    assert_eq!(p.plugin_id, "io.waywallen.wescene");
    assert_eq!(p.priority, 10);
    let s = &p.settings[0];
    assert_eq!(s.key, "resolution");
    assert_eq!(s.value_type, pb::SettingValueType::String as i32);
    assert_eq!(s.default_value, "auto");
    assert!(s.identity);
    assert_eq!(s.choices, vec!["auto", "native"]);
    assert_eq!(s.order, 3);
    assert_eq!(data.supported_types, vec!["scene", "video"]);
}

#[test]
fn setting_schema_carries_translatable_labels() {
    // `pluginMessageFromPb` вендора: есть ключ каталога — наружу карта,
    // нет — голая строка запаски. `SettingField.qml` читает `label` и
    // `description`, заголовок группы — `group_label`
    // (`PluginSettingsPage.qml`), и без них человек видел бы ключ.
    let schema = q::SettingSchema::from(pb::SettingSchema {
        key: "hwdec".into(),
        label_key: "settings.video.hwdec".into(),
        description_key: "settings.video.hwdec.desc".into(),
        group: "video".into(),
        label: Some(pb::PluginMessage {
            plugin_id: "org.waywallen.video".into(),
            msgid: "Hardware decoding".into(),
            arguments: vec![],
        }),
        ..Default::default()
    });
    let v = serde_json::to_value(&schema).unwrap();
    assert_eq!(v["label"]["pluginId"], "org.waywallen.video");
    assert_eq!(v["label"]["msgid"], "Hardware decoding");
    // Каталога у описания и группы нет — наружу идут прежние строки, а не
    // пустая карта: у вендора это тоже строка.
    assert_eq!(v["description"], "settings.video.hwdec.desc");
    assert_eq!(v["group_label"], "video");
}

// ── TagListQuery / ContentRatingListQuery ──────────────────────────────

#[test]
fn tag_and_rating_lists_fill_their_own_property() {
    let mut st = q::QueryState::default();
    let mut tags = q::StringListData::default();
    let f = ok_frame(pb::response::Payload::TagList(pb::TagListResponse {
        tags: vec!["anime".into(), "nature".into()],
    }));
    let sig = q::apply_tag_list(&mut st, &mut tags, &f).unwrap();
    assert_eq!(tags.items, vec!["anime", "nature"]);
    assert_eq!(sig[0], q::QuerySignal::Property("tags"));

    let mut st = q::QueryState::default();
    let mut ratings = q::StringListData::default();
    let f = ok_frame(pb::response::Payload::ContentRatingList(
        pb::ContentRatingListResponse {
            ratings: vec!["everyone".into(), "mature".into()],
        },
    ));
    let sig = q::apply_content_rating_list(&mut st, &mut ratings, &f).unwrap();
    assert_eq!(ratings.items, vec!["everyone", "mature"]);
    // ⚠️ Именно этот список был всегда пуст до 12234d9: живой класс
    // моста был заглушкой, затиравшей рабочую реализацию.
    assert_eq!(sig[0], q::QuerySignal::Property("ratings"));
}

#[test]
fn empty_payload_still_finishes() {
    // Демон вправе ответить OK без нагрузки — запрос обязан закончиться
    // Finished, а не зависнуть в Querying.
    let mut st = q::QueryState::default();
    let mut tags = q::StringListData::default();
    q::begin_call(&mut st, true);
    let f = frame(pb::Status::Ok as i32, String::new(), None);
    let sig = q::apply_tag_list(&mut st, &mut tags, &f).unwrap();
    assert_eq!(st.status, q::STATUS_FINISHED);
    assert!(tags.items.is_empty());
    assert_eq!(
        sig,
        vec![
            q::QuerySignal::StatusChanged(q::STATUS_FINISHED),
            q::QuerySignal::QueryingChanged,
            q::QuerySignal::Finished,
        ]
    );
}

// ── видеокарты ─────────────────────────────────────────────────────────

/// Ответ `gpu_list` живого демона 0.4.4 на машине с двумя картами
/// (тот же кадр лежит в `golden/we_live_frames.json`).
const LIVE_GPU_LIST_HEX: &str = "0abd0208061001ca02b5020a790a132f6465762f6472692f72656e64657244313238120e2f6465762f6472692f636172643118e20120800128e20130013a0c303030303a30363a30302e3040822048bf2b5206616d646770755a2150686f656e69783120e2809420616d64677075203078313030323a307831356266620850686f656e6978310ab7010a132f6465762f6472692f72656e64657244313239120e2f6465762f6472692f636172643018e20120810128e2013a0c303030303a30313a30302e3040de2148a05052066e76696469615a4141443130364d205b4765466f726365205254582034303730204d61782d51202f204d6f62696c655d20e28094206e7669646961203078313064653a307832383230622841443130364d205b4765466f726365205254582034303730204d61782d51202f204d6f62696c655d";

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn gpu_list_reads_live_daemon_frame() {
    let gpus = q::gpu_list_from_frame(&unhex(LIVE_GPU_LIST_HEX)).expect("ответ gpu_list");
    assert_eq!(gpus.len(), 2);

    let amd = &gpus[0];
    assert_eq!(amd.render_node, "/dev/dri/renderD128");
    assert_eq!(amd.primary_node, "/dev/dri/card1");
    assert_eq!((amd.render_major, amd.render_minor), (226, 128));
    assert_eq!((amd.primary_major, amd.primary_minor), (226, 1));
    assert_eq!(amd.pci_bdf, "0000:06:00.0");
    assert_eq!((amd.vendor_id, amd.device_id), (0x1002, 0x15bf));
    assert_eq!(amd.driver, "amdgpu");
    assert_eq!(amd.name, "Phoenix1");

    let nvidia = &gpus[1];
    assert_eq!(nvidia.render_node, "/dev/dri/renderD129");
    assert_eq!((nvidia.render_major, nvidia.render_minor), (226, 129));
    assert_eq!((nvidia.vendor_id, nvidia.device_id), (0x10de, 0x2820));
    assert_eq!(nvidia.driver, "nvidia");
    assert!(nvidia.name.contains("RTX 4070"));
}

#[test]
fn gpu_is_found_by_render_or_primary_node() {
    let gpus = q::gpu_list_from_frame(&unhex(LIVE_GPU_LIST_HEX)).unwrap();
    // Узел отрисовки — его несёт рисовальщик.
    assert_eq!(q::find_gpu(&gpus, 226, 128), Some(0));
    assert_eq!(q::find_gpu(&gpus, 226, 129), Some(1));
    // Основной узел той же карты.
    assert_eq!(q::find_gpu(&gpus, 226, 1), Some(0));
    assert_eq!(q::find_gpu(&gpus, 226, 0), Some(1));
    // Чужой узел и «узел неизвестен».
    assert_eq!(q::find_gpu(&gpus, 226, 130), None);
    assert_eq!(q::find_gpu(&gpus, 0, 0), None);
    assert_eq!(q::find_gpu(&gpus, 0, 128), None);
}

#[test]
fn gpu_list_ignores_frames_that_are_not_its_answer() {
    // Отказ демона и кадр обрыва соединения: статус не OK.
    let refused = frame(pb::Status::Internal as i32, "нет".into(), None);
    assert_eq!(q::gpu_list_from_frame(&refused), None);
    // OK, но чужая нагрузка.
    let other = ok_frame(pb::response::Payload::TagList(pb::TagListResponse::default()));
    assert_eq!(q::gpu_list_from_frame(&other), None);
    // Мусор вместо кадра.
    assert_eq!(q::gpu_list_from_frame(&[0xff, 0xff, 0xff]), None);
    // Пустой список — это ответ «карт нет», а не «ответа нет».
    let empty = ok_frame(pb::response::Payload::GpuList(pb::GpuListResponse::default()));
    assert_eq!(q::gpu_list_from_frame(&empty), Some(Vec::new()));
}

/// Сторож оповещений о карте: значок карты (`GpuTag.qml`) — привязка, и
/// обновится она только по сигналу. Пойман случай: рисовальщик стартует с
/// нулевым узлом DRM и называет карту следующим обновлением, а свойства
/// были постоянными — на открытой странице «Состояние» значок не появлялся.
#[test]
fn gpu_tag_inputs_announce_their_changes() {
    const OBJMODEL_RS: &str = include_str!("../../rexon-gui/src/we_objmodel.rs");
    const APP_RS: &str = include_str!("../../rexon-gui/src/we_app.rs");
    let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let objmodel = flat(OBJMODEL_RS);
    let app = flat(APP_RS);

    // У Renderer оба номера узла меняются с оповещением…
    let renderer_bridge = objmodel
        .split("type Renderer = super::RendererRust;")
        .next()
        .expect("объявление Renderer в мосте");
    for property in ["drm_render_major", "drm_render_minor"] {
        let declared = format!("READ = {property}, NOTIFY = drm_render_changed)]");
        assert!(
            renderer_bridge.contains(&declared),
            "Renderer.{property} обязан оповещать о смене (drmRenderChanged)"
        );
    }
    // …и обновление это оповещение шлёт.
    let update = objmodel
        .split("fn update_from(mut self: Pin<&mut Self>, info: RendererInfo)")
        .nth(1)
        .and_then(|rest| rest.split("// RendererManager").next())
        .expect("Renderer::update_from");
    assert!(
        update.contains("old.drm_render_major != new.drm_render_major")
            && update.contains("old.drm_render_minor != new.drm_render_minor")
            && update.contains("self.as_mut().drm_render_changed();"),
        "Renderer::update_from обязан слать drmRenderChanged при смене узла"
    );

    // Смена списка карт доходит до значков через свойство App.gpuManager.
    assert!(
        app.contains("READ = gpu_manager, NOTIFY = gpu_manager_changed)]"),
        "App.gpuManager обязан оповещать: значок ищет карту вызовом find в привязке"
    );
    assert!(
        app.contains("if changed {") && app.contains("self.as_mut().gpu_manager_changed();"),
        "смена списка карт обязана слать gpuManagerChanged"
    );
}

// ── Текущие показы обоев (карточка рейки) ───────────────────────────────

#[test]
fn wallpaper_lookup_answer_keeps_daemon_order_and_ignores_foreign_frames() {
    let entry = |id: &str| pb::WallpaperEntry {
        id: id.into(),
        name: format!("имя {id}"),
        ..Default::default()
    };
    let answer = ok_frame(pb::response::Payload::WallpaperLookup(pb::WallpaperLookupResponse {
        wallpapers: vec![entry("b"), entry("a")],
    }));
    let found = q::wallpaper_lookup_from_frame(&answer).expect("ответ wallpaper_lookup");
    assert_eq!(found.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);

    // Кадр обрыва соединения (статус не OK) и чужая нагрузка — не ответ.
    let refused = frame(pb::Status::Internal as i32, "нет".into(), None);
    assert_eq!(q::wallpaper_lookup_from_frame(&refused), None);
    let other = ok_frame(pb::response::Payload::TagList(pb::TagListResponse::default()));
    assert_eq!(q::wallpaper_lookup_from_frame(&other), None);
}

#[test]
fn presentation_ids_and_target_labels_drop_repeats_in_snapshot_order() {
    let shown = |id: &str| pb::WallpaperPresentationInfo {
        wallpaper_id: id.into(),
        ..Default::default()
    };
    assert_eq!(
        q::presentation_wallpaper_ids(&[shown("b"), shown("a"), shown("b")]),
        ["b", "a"]
    );
    let labels = ["DP-1".to_string(), "Холст".to_string(), "DP-1".to_string()];
    assert_eq!(q::presentation_target_summary(&labels), "DP-1 · Холст");
    assert_eq!(q::presentation_target_summary(&[]), "");
}
