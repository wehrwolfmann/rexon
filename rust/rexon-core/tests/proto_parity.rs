//! Golden check of the protobuf port (prost) against the original `control_pb2.py`/
//! `filter_pb2.py` (google.protobuf).
//!
//! The golden `golden/proto_cases.json` was captured from Python itself: the same messages
//! were serialized with `SerializeToString(deterministic=True)`, the hex was written to
//! the fixture. For EVERY case Rust:
//!   1) decodes the golden bytes into a prost struct and checks the fields
//!      (a structural check — that the decoding is semantically correct, not "bytes as
//!      bytes");
//!   2) encodes again and, for messages WITHOUT a `map`, compares the bytes one-to-one with
//!      the golden (round-trip). In proto3 both implementations write fields in
//!      ascending number order and omit defaults, so the bytes match.
//!
//! ⚠️ `map` fields (`renderer_spawn_map`) are NOT byte-deterministic: prost keeps
//! a map in a `HashMap`, the key order on the wire is arbitrary. For them — only
//! the structural check (map contents by key), the byte check is NOT applied.

use prost::Message;
use rexon_core::proto::control::{self as pb, FillMode, Rotation, Align, Status, ErrorCode};
use rexon_core::proto::control::{PlaylistMode};
use rexon_core::proto::control as filt; // filter.proto is in the control.v1 package too
use serde_json::Value;
use std::collections::BTreeMap;

/// All hex goldens from the fixture.
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

/// Check: decode → the fields are correct AND re-encoding gives the same bytes (round-trip).
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
        // Protocol 0.3.7 fields (revision and the shared wallpaper choice) are
        // zero in the golden: the fixture was captured from Python before they appeared, and proto3 does not
        // write such fields to the wire — the round-trip must stay byte-for-byte.
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

/// A map message: the byte check is NOT applied (the key order is arbitrary),
/// only the structural one — that decoding restored all fields and the map contents.
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
    // The round-trip stays valid (decodes into the same thing again), even though
    // the bytes of the map field may be reordered.
    let re = pb::RendererSpawnRequest::decode(&decoded.encode_to_vec()[..]).unwrap();
    assert_eq!(re, decoded);
}

// ============================================================================
// LIVE daemon frames (golden/we_live_frames.json)
// ============================================================================
//
// The cases above are built from synthetic messages: they check the codec on
// individual field types, but not on what the daemon really sends (deep
// nesting, repeated messages of hundreds of bytes, fields of a newer protocol version
// unknown to us). Here are frames captured from LIVE waywallen
// 0.2.6 over WebSocket with READ-ONLY requests (the user's wallpapers were not
// touched; no apply, renderer_kill, scan or settings_set).
//
// The golden is twofold: `frame_hex` is the bytes from the wire, `reserialized_hex` is their
// reassembly by `google.protobuf` after parsing (`deterministic=True`). Comparing
// prost with exactly the SECOND one is the cross-implementation parity: if from the
// same input bytes both implementations assemble the same thing, then both parsed
// the frame the same way. The `byte_stable` flag in the fixture says whether the
// reassembly matched the wire (it does not for `settings_get` — there is a `map`).

/// Frames of the live daemon.
fn live() -> Value {
    serde_json::from_str(include_str!("golden/we_live_frames.json")).unwrap()
}

/// The common check of one live frame: parsing by prost → reassembly →
/// comparison with the google.protobuf reassembly (with `byte_stable` — also with
/// the wire bytes).
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
            // `payload: null` in the fixture = an answer without a payload
            // (for us this is a real daemon error on autostart_get).
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
        // A map field: the key order is arbitrary, we compare structurally.
        let a = pb::ServerFrame::decode(&re[..]).unwrap();
        let b = pb::ServerFrame::decode(&expect_re[..]).unwrap();
        assert_eq!(a, b, "{name}: структурная сверка с google.protobuf");
        assert_eq!(a, frame, "{name}: round-trip не воспроизводит кадр");
    }
}

/// All live daemon ANSWERS are parsed and reassembled as in Python.
#[test]
fn живые_ответы_демона_паритетны() {
    let g = live();
    let cases = g["responses"].as_object().unwrap();
    // The fixture must be non-empty and cover the tab's read requests —
    // otherwise the test is "green about nothing".
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

/// Server-push `Event`s — the daemon sends them by itself, without a request.
#[test]
fn живые_события_демона_паритетны() {
    let g = live();
    let events = g["events"].as_array().unwrap();
    assert!(!events.is_empty(), "фикстура без событий");
    for (i, case) in events.iter().enumerate() {
        assert_live_frame(&format!("event[{i}]"), case);
    }
}

/// A live ERROR frame of the daemon: `status`/`error_code` are read, there is no payload.
/// Such an answer comes for `autostart_get` (the daemon outside Flatpak does not
/// support it) — the "error without payload" branch in answer parsing must be
/// covered by a real frame, not a made-up one.
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

/// The live REQUESTS with which the answers were captured: prost assembles the same bytes.
/// This is also a check of the field number table on the real wire — the
/// `subscription_status` frame (field 70, added upstream after the handwritten
/// table) was accepted and answered by the daemon.
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
