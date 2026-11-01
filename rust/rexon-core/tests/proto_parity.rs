//! Golden-сверка protobuf-порта (prost) с оригиналом `control_pb2.py`/
//! `filter_pb2.py` (google.protobuf).
//!
//! Эталон `golden/proto_cases.json` снят с самого Python: те же сообщения
//! сериализованы `SerializeToString(deterministic=True)`, hex записан в
//! фикстуру. Для КАЖДОГО кейса Rust:
//!   1) декодирует эталонные байты в prost-структуру и проверяет поля
//!      (структурная сверка — что декод семантически верен, а не «байты как
//!      байты»);
//!   2) заново кодирует и для сообщений БЕЗ `map` сверяет байты один-в-один с
//!      эталоном (round-trip). У proto3 обе реализации пишут поля по
//!      возрастанию номера и опускают дефолты, поэтому байты совпадают.
//!
//! ⚠️ `map`-поля (`renderer_spawn_map`) НЕ байт-детерминированы: prost хранит
//! map в `HashMap`, порядок ключей на проводе произволен. Для них — только
//! структурная сверка (содержимое map по ключам), байтовая НЕ применяется.

use prost::Message;
use rexon_core::proto::control::{self as pb, FillMode, Rotation, Align, Status, ErrorCode};
use rexon_core::proto::control::{PlaylistMode};
use rexon_core::proto::control as filt; // filter.proto тоже в пакете control.v1
use serde_json::Value;
use std::collections::BTreeMap;

/// Все hex-эталоны из фикстуры.
fn golden() -> BTreeMap<String, Vec<u8>> {
    let v: Value = serde_json::from_str(include_str!("golden/proto_cases.json")).unwrap();
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, hex)| (k.clone(), hex_bytes(hex.as_str().unwrap())))
        .collect()
}

fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn bytes(name: &str) -> Vec<u8> {
    golden().get(name).unwrap_or_else(|| panic!("нет кейса {name}")).clone()
}

/// Проверка: декод → поля верны И перекодирование даёт те же байты (round-trip).
fn assert_roundtrip<M: Message + Default + PartialEq>(name: &str, expect: &M) {
    let raw = bytes(name);
    let decoded = M::decode(&raw[..]).expect("prost декодирует эталонные байты");
    assert_eq!(&decoded, expect, "поля декода расходятся: {name}");
    assert_eq!(decoded.encode_to_vec(), raw, "перекодирование не байт-в-байт: {name}");
}

#[test]
fn renderer_fps_scalar() {
    assert_roundtrip(
        "renderer_fps",
        &pb::RendererFpsRequest { renderer_id: "mon0".into(), fps: 60 },
    );
}

#[test]
fn request_oneof_nested() {
    let expect = pb::Request {
        request_id: 42,
        payload: Some(pb::request::Payload::RendererFps(pb::RendererFpsRequest {
            renderer_id: "disp-1".into(),
            fps: 30,
        })),
    };
    assert_roundtrip("request_renderer_fps", &expect);
}

fn sample_layout() -> pb::LayoutPrefs {
    pb::LayoutPrefs {
        fillmode: FillMode::PreserveAspectCrop as i32,
        align: Align::Center as i32,
        rotation: Rotation::Cw90 as i32,
        location_x: 50,
        location_y: 25,
        location_set: true,
        flip: 0,
    }
}

#[test]
fn layout_prefs_enums() {
    assert_roundtrip("layout_prefs", &sample_layout());
}

#[test]
fn wallpaper_entry_nested_repeated() {
    let expect = pb::WallpaperEntry {
        id: "wp1".into(),
        name: "Закат".into(),
        wp_type: "scene".into(),
        size: 123456,
        width: 1920,
        height: 1080,
        tags: vec!["a".into(), "b".into()],
        wallpaper_layout_override: Some(sample_layout()),
        wallpaper_layout_override_set: true,
        ..Default::default()
    };
    assert_roundtrip("wallpaper_entry", &expect);
}

#[test]
fn filter_rule_oneof() {
    let expect = filt::WallpaperFilterRule {
        r#type: filt::WallpaperFilterType::Name as i32,
        group: 2,
        payload: Some(filt::wallpaper_filter_rule::Payload::StringFilter(
            filt::WallpaperStringFilter {
                value: "dark".into(),
                condition: filt::StringCondition::Is as i32,
            },
        )),
    };
    assert_roundtrip("filter_rule", &expect);
}

#[test]
fn server_frame_response() {
    let expect = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 7,
            status: Status::Ok as i32,
            message: "done".into(),
            error_code: ErrorCode::Ok as i32,
            payload: None,
        })),
    };
    assert_roundtrip("server_frame_response", &expect);
}

#[test]
fn response_enum_fields() {
    let expect = pb::Response {
        request_id: 9,
        status: Status::NotFound as i32,
        message: "missing".into(),
        error_code: ErrorCode::WallpaperNotFound as i32,
        payload: None,
    };
    assert_roundtrip("response_error", &expect);
}

#[test]
fn playlist_summary_int64_enum() {
    let expect = pb::PlaylistSummary {
        id: 5,
        name: "Fav".into(),
        source_kind: "curated".into(),
        mode: PlaylistMode::Shuffle as i32,
        interval_secs: 300,
        item_count: 3,
        entry_ids: vec!["x".into(), "y".into(), "z".into()],
        // Поля протокола 0.3.7 (ревизия и общий выбор обоев) в эталоне
        // нулевые: фикстура снята с Python до их появления, и proto3 такие
        // поля на провод не пишет — round-trip обязан остаться байт-в-байт.
        ..Default::default()
    };
    assert_roundtrip("playlist_summary", &expect);
}

#[test]
fn gpu_info_many_fields() {
    let expect = pb::GpuInfo {
        render_node: "/dev/dri/renderD128".into(),
        primary_node: "/dev/dri/card0".into(),
        render_major: 226,
        render_minor: 128,
        primary_major: 226,
        primary_minor: 0,
        pci_bdf: "0000:03:00.0".into(),
        vendor_id: 0x1002,
        device_id: 0x73df,
        driver: "amdgpu".into(),
        description: "AMD Radeon".into(),
        ..Default::default()
    };
    assert_roundtrip("gpu_info", &expect);
}

/// map-сообщение: байт-сверка НЕ применяется (порядок ключей произволен),
/// только структурная — что декод восстановил все поля и содержимое map.
#[test]
fn renderer_spawn_map_structural_only() {
    let raw = bytes("renderer_spawn_map");
    let decoded = pb::RendererSpawnRequest::decode(&raw[..]).unwrap();
    assert_eq!(decoded.wp_type, "scene");
    assert_eq!(decoded.width, 800);
    assert_eq!(decoded.height, 600);
    assert_eq!(decoded.fps, 24);
    assert_eq!(decoded.metadata.get("k1").map(String::as_str), Some("v1"));
    assert_eq!(decoded.metadata.get("k2").map(String::as_str), Some("v2"));
    assert_eq!(decoded.metadata.len(), 2);
    // Round-trip остаётся валидным (снова декодируется в то же), даже если
    // байты map-поля могут переставиться.
    let re = pb::RendererSpawnRequest::decode(&decoded.encode_to_vec()[..]).unwrap();
    assert_eq!(re, decoded);
}

// ============================================================================
// ЖИВЫЕ кадры демона (golden/we_live_frames.json)
// ============================================================================
//
// Кейсы выше собраны из синтетических сообщений: они проверяют кодек на
// отдельных типах полей, но не на том, что реально шлёт демон (глубокая
// вложенность, повторяющиеся сообщения на сотни байт, незнакомые нам поля
// более новой версии протокола). Здесь — кадры, снятые с ЖИВОГО waywallen
// 0.2.6 по WebSocket ТОЛЬКО читающими запросами (обои пользователя не
// тронуты; ни apply, ни renderer_kill, ни scan, ни settings_set).
//
// Эталон двойной: `frame_hex` — байты с провода, `reserialized_hex` — их
// пересборка `google.protobuf` после разбора (`deterministic=True`). Сверка
// prost'а именно со ВТОРЫМ и есть кросс-реализационный паритет: если из тех
// же входных байт обе реализации собирают одно и то же, значит обе разобрали
// кадр одинаково. Флаг `byte_stable` в фикстуре говорит, совпала ли
// пересборка с проводом (не совпадает у `settings_get` — там `map`).

/// Кадры живого демона.
fn live() -> Value {
    serde_json::from_str(include_str!("golden/we_live_frames.json")).unwrap()
}

/// Общая проверка одного живого кадра: разбор prost'ом → пересборка →
/// сверка с пересборкой google.protobuf (при `byte_stable` — ещё и с
/// проводными байтами).
fn assert_live_frame(name: &str, case: &Value) {
    let raw = hex_bytes(case["frame_hex"].as_str().unwrap());
    let expect_re = hex_bytes(case["reserialized_hex"].as_str().unwrap());
    let frame = pb::ServerFrame::decode(&raw[..])
        .unwrap_or_else(|e| panic!("prost не разобрал живой кадр {name}: {e}"));
    let kind = frame.kind.as_ref().unwrap_or_else(|| {
        panic!("живой кадр {name}: пустой oneof kind")
    });
    match (case["kind"].as_str().unwrap(), kind) {
        ("response", pb::server_frame::Kind::Response(r)) => {
            assert_eq!(
                r.request_id,
                case["request_id"].as_u64().unwrap(),
                "{name}: request_id"
            );
            assert_eq!(
                i64::from(r.status),
                case["status"].as_i64().unwrap(),
                "{name}: status"
            );
            assert_eq!(
                i64::from(r.error_code),
                case["error_code"].as_i64().unwrap(),
                "{name}: error_code"
            );
            assert_eq!(
                r.message,
                case["message"].as_str().unwrap(),
                "{name}: message"
            );
            // `payload: null` в фикстуре = ответ без полезной нагрузки
            // (у нас это реальная ошибка демона на autostart_get).
            assert_eq!(
                r.payload.is_some(),
                !case["payload"].is_null(),
                "{name}: наличие payload"
            );
        }
        ("event", pb::server_frame::Kind::Event(e)) => {
            assert!(e.payload.is_some(), "{name}: событие без payload");
        }
        (want, got) => panic!("{name}: ожидался {want}, получен {got:?}"),
    }
    let re = frame.encode_to_vec();
    if case["byte_stable"].as_bool().unwrap() {
        assert_eq!(
            re, expect_re,
            "{name}: пересборка prost расходится с google.protobuf"
        );
        assert_eq!(re, raw, "{name}: пересборка расходится с проводом");
    } else {
        // map-поле: порядок ключей произволен, сверяем структурно.
        let a = pb::ServerFrame::decode(&re[..]).unwrap();
        let b = pb::ServerFrame::decode(&expect_re[..]).unwrap();
        assert_eq!(a, b, "{name}: структурная сверка с google.protobuf");
        assert_eq!(a, frame, "{name}: round-trip не воспроизводит кадр");
    }
}

/// Все живые ОТВЕТЫ демона разбираются и пересобираются как у Python.
#[test]
fn живые_ответы_демона_паритетны() {
    let g = live();
    let cases = g["responses"].as_object().unwrap();
    // Фикстура должна быть непустой и покрывать читающие запросы вкладки —
    // иначе тест «зелёный ни о чём».
    assert!(cases.len() >= 18, "мало живых кейсов: {}", cases.len());
    for want in [
        "health",
        "display_list",
        "settings_get",
        "renderer_list",
        "renderer_plugin_list",
        "gpu_list",
        "tag_list",
        "content_rating_list",
        "source_list",
        "library_list",
        "playlist_list",
        "playlist_status",
        "plugin_list",
        "remote_availability",
        "wallpaper_list",
        "wallpaper_get",
    ] {
        assert!(cases.contains_key(want), "нет живого кейса {want}");
    }
    for (name, case) in cases {
        assert_live_frame(name, case);
    }
}

/// Server-push `Event` — их демон присылает сам, без запроса.
#[test]
fn живые_события_демона_паритетны() {
    let g = live();
    let events = g["events"].as_array().unwrap();
    assert!(!events.is_empty(), "фикстура без событий");
    for (i, case) in events.iter().enumerate() {
        assert_live_frame(&format!("event[{i}]"), case);
    }
}

/// Живой кадр ОШИБКИ демона: `status`/`error_code` читаются, payload'а нет.
/// Такой ответ приходит на `autostart_get` (демон вне Flatpak его не
/// поддерживает) — ветка «ошибка без payload» в разборе ответов должна быть
/// покрыта настоящим кадром, а не выдуманным.
#[test]
fn живой_кадр_ошибки_демона() {
    let g = live();
    let case = &g["responses"]["autostart_get"];
    let raw = hex_bytes(case["frame_hex"].as_str().unwrap());
    let frame = pb::ServerFrame::decode(&raw[..]).unwrap();
    let Some(pb::server_frame::Kind::Response(r)) = frame.kind else {
        panic!("ожидался response");
    };
    assert_ne!(r.status, Status::Ok as i32, "кейс должен быть ошибкой");
    assert!(r.payload.is_none(), "у ошибки не должно быть payload");
    assert!(!r.message.is_empty(), "демон обязан объяснить ошибку");
}

/// Живые ЗАПРОСЫ, которыми снимались ответы: те же байты собирает и prost.
/// Заодно это проверка таблицы номеров полей на реальном проводе — кадр
/// `subscription_status` (поле 70, добавлено апстримом после рукописной
/// таблицы) демон принял и ответил.
#[test]
fn живые_запросы_собираются_prost() {
    let g = live();
    for (name, case) in g["requests"].as_object().unwrap() {
        let raw = hex_bytes(case["frame_hex"].as_str().unwrap());
        let req = pb::Request::decode(&raw[..])
            .unwrap_or_else(|e| panic!("не разобран запрос {name}: {e}"));
        assert!(req.payload.is_some(), "{name}: запрос без payload");
        assert_eq!(req.encode_to_vec(), raw, "{name}: не байт-в-байт");
    }
}
