//! Settings of the Wallpaper Engine tab — port of waywallen's
//! `ui/src/query/settings_query.cpp` (MIT, (c) hypengw; attribution in NOTICE).
//!
//! The CONVERTER between the protocol's `GlobalSettings`/`PluginSettings` and
//! the value map QML reads and writes lives here. It is one for both
//! directions: `settings_get` unfolds the proto into a map, `settings_set`
//! assembles the proto back from the map. The directions must be mirror
//! images — the daemon handles `settings_set` as a FULL replacement, and any
//! field lost on the round trip is erased at the user's.
//!
//! A separate module (rather than the tail of `we_query.rs`) along the same
//! boundary as the original: its `settings_query.cpp` is a separate
//! translation unit too, and the converter serves not only settings requests
//! but also the «Wallpapers» page (sort/filter rules in `WallpaperListQuery`).
//!
//! ⚠️ The former Python bridge built the map by REFLECTION over the protobuf
//! descriptor (`_pb_to_variant`: all fields under `json_name`). That is not
//! the same as the original's `global_to_map`, and gave TWO live defects —
//! both fixed by this port (rule #16):
//!
//!   1. `renderer` went out as a nested map `{enableAudio, volume}`, while
//!      `SettingsPage.qml` reads and writes the FLAT keys
//!      `renderer.enable_audio` / `renderer.volume`. Result: the «renderer
//!      sound» checkbox and the volume slider showed default values and were
//!      not saved at all (the reverse `_js_to_pb` did not see the flat keys,
//!      and took the nested map from a stale snapshot).
//!   2. `plugins[name]` went out as `{"values": {key: value}}` instead of a
//!      flat value map. `PluginSettingsPage.qml` reads `currentValues[key]` and
//!      sends back a flat map — so plugin settings were not read, and «Apply»
//!      sent the daemon an EMPTY table (full replacement → the plugin's values
//!      were erased).
//!
//! Values are `serde_json::Value` (a map = `Map<String, Value>`): exactly the
//! types that survive the QVariant boundary (bool / number / string / list /
//! map). JSON here is the in-memory representation and the golden format; on
//! the wire settings travel as protobuf.

use crate::proto::control as pb;
use crate::we_query::{self as q, QuerySignal, QueryState};
use prost::Message;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// A value map in QML terms (`QVariantMap` in the original).
pub type VMap = Map<String, Value>;

// ── reading map values (QVariant::toInt/toBool/toString semantics) ───────

/// `QVariant::toString()` over one value: a non-string is converted to a
/// string, everything else (a map, a list, `null`) is an empty string.
pub(crate) fn value_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// `QVariant::toBool()` over one value: a number is «non-zero», an empty
/// string and `null` are false.
pub(crate) fn value_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty() && s != "false",
        _ => false,
    }
}

/// `QVariant::toInt()` over one value: numbers from QML arrive as doubles, so
/// a fractional one is rounded.
pub(crate) fn value_i64(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().map(|f| f.round() as i64).unwrap_or(0)),
        Value::Bool(b) => i64::from(*b),
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

/// The same in the width of Qt's `int`.
pub(crate) fn value_i32(v: &Value) -> i32 {
    value_i64(v) as i32
}

/// `QVariant::toString()`: no key — an empty string.
fn get_str(m: &VMap, key: &str) -> String {
    m.get(key).map(value_str).unwrap_or_default()
}

/// `QVariant::toInt()`: no key — 0.
fn get_i64(m: &VMap, key: &str) -> i64 {
    m.get(key).map(value_i64).unwrap_or(0)
}

fn get_i32(m: &VMap, key: &str) -> i32 {
    get_i64(m, key) as i32
}

/// `QVariant::toUInt()`: we do not turn a negative into an unsigned — take 0
/// (in the original this is a UB-like zone, QML never gets there: both volume
/// and rotation seconds are limited to non-negative values).
fn get_u32(m: &VMap, key: &str) -> u32 {
    let v = get_i64(m, key);
    if v < 0 {
        0
    } else {
        v as u32
    }
}

/// `QVariant::value(key, fallback).toUInt()`: no key — take the fallback
/// number, not zero.
fn get_u32_or(m: &VMap, key: &str, fallback: u32) -> u32 {
    if m.contains_key(key) {
        get_u32(m, key)
    } else {
        fallback
    }
}

/// `QVariant::toBool()`: no key — false.
fn get_bool(m: &VMap, key: &str) -> bool {
    m.get(key).map(value_bool).unwrap_or(false)
}

fn get_list<'a>(m: &'a VMap, key: &str) -> &'a [Value] {
    match m.get(key) {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

fn get_map<'a>(m: &'a VMap, key: &str) -> Option<&'a VMap> {
    match m.get(key) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}

/// A list element → a map (skipping non-maps: in the original
/// `value<Gadget>()` on a foreign type returns an empty gadget, ours — an
/// empty map).
pub(crate) fn as_map(v: &Value) -> VMap {
    match v {
        Value::Object(o) => o.clone(),
        _ => VMap::new(),
    }
}

/// A list of strings (the original's `QStringList`): elements are converted to strings.
pub(crate) fn string_list(items: &[Value]) -> Vec<String> {
    items.iter().map(value_str).collect()
}

fn strings_value(items: &[String]) -> Value {
    Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
}

fn maps_value<T>(items: &[T], to_map: fn(&T) -> VMap) -> Value {
    Value::Array(items.iter().map(|i| Value::Object(to_map(i))).collect())
}

// ── sort and filter rules (the «Wallpapers» page) ────────────────────────
//
// In the original these three fields go into the map AS Qt-Protobuf GADGETS
// (`QVariant::fromValue(rule)`), a gadget can both read and write
// properties. We have no gadgets on this side: the map is read by QML
// (`r.key`, `r.direction`) and by the Rust filter model, which converts the
// map to a gadget itself (`rexon_coerce_gadget`). So the keys here are
// Qt-Protobuf property names (`json_name`, camelCase), enum values are numbers.

/// `WallpaperSortRule` → a map.
pub fn sort_rule_to_map(r: &pb::WallpaperSortRule) -> VMap {
    let mut m = VMap::new();
    m.insert("key".into(), Value::from(r.key));
    m.insert("direction".into(), Value::from(r.direction));
    m
}

/// A map → `WallpaperSortRule`.
pub fn map_to_sort_rule(m: &VMap) -> pb::WallpaperSortRule {
    pb::WallpaperSortRule {
        key: get_i32(m, "key"),
        direction: get_i32(m, "direction"),
    }
}

/// `FilterLogic` → a map.
pub fn filter_logic_to_map(l: &pb::FilterLogic) -> VMap {
    let mut m = VMap::new();
    m.insert("op".into(), Value::from(l.op));
    m.insert("groupA".into(), Value::from(l.group_a));
    m.insert("groupB".into(), Value::from(l.group_b));
    m
}

/// A map → `FilterLogic`.
pub fn map_to_filter_logic(m: &VMap) -> pb::FilterLogic {
    pb::FilterLogic {
        op: get_i32(m, "op"),
        group_a: get_i32(m, "groupA"),
        group_b: get_i32(m, "groupB"),
    }
}

/// `WallpaperFilterRule` → a map. Only the SET branch of `oneof payload` is
/// put — the gadget reader tells «no payload» by the absence of the key (in
/// Qt-Protobuf that is `hasStringFilter` and company).
pub fn filter_rule_to_map(r: &pb::WallpaperFilterRule) -> VMap {
    use pb::wallpaper_filter_rule::Payload;
    let mut m = VMap::new();
    m.insert("type".into(), Value::from(r.r#type));
    m.insert("group".into(), Value::from(r.group));
    match &r.payload {
        Some(Payload::StringFilter(f)) => {
            let mut s = VMap::new();
            s.insert("value".into(), Value::String(f.value.clone()));
            s.insert("condition".into(), Value::from(f.condition));
            m.insert("stringFilter".into(), Value::Object(s));
        }
        Some(Payload::IntFilter(f)) => {
            let mut s = VMap::new();
            s.insert("value".into(), Value::from(f.value));
            s.insert("condition".into(), Value::from(f.condition));
            m.insert("intFilter".into(), Value::Object(s));
        }
        Some(Payload::TagFilter(f)) => {
            let mut s = VMap::new();
            s.insert("values".into(), strings_value(&f.values));
            s.insert("condition".into(), Value::from(f.condition));
            m.insert("tagFilter".into(), Value::Object(s));
        }
        None => {}
    }
    m
}

/// A map → `WallpaperFilterRule`.
pub fn map_to_filter_rule(m: &VMap) -> pb::WallpaperFilterRule {
    use pb::wallpaper_filter_rule::Payload;
    let payload = if let Some(f) = get_map(m, "stringFilter") {
        Some(Payload::StringFilter(pb::WallpaperStringFilter {
            value: get_str(f, "value"),
            condition: get_i32(f, "condition"),
        }))
    } else if let Some(f) = get_map(m, "intFilter") {
        Some(Payload::IntFilter(pb::WallpaperIntFilter {
            value: get_i64(f, "value"),
            condition: get_i32(f, "condition"),
        }))
    } else {
        get_map(m, "tagFilter").map(|f| {
            Payload::TagFilter(pb::WallpaperTagFilter {
                values: string_list(get_list(f, "values")),
                condition: get_i32(f, "condition"),
            })
        })
    };
    pb::WallpaperFilterRule {
        r#type: get_i32(m, "type"),
        group: get_i32(m, "group"),
        payload,
    }
}

// ── LayoutPrefs / AutoReplayPolicy ───────────────────────────────────────

/// The original's `layout_to_map`.
pub fn layout_to_map(l: &pb::LayoutPrefs) -> VMap {
    let mut m = VMap::new();
    m.insert("fillmode".into(), Value::from(l.fillmode));
    m.insert("align".into(), Value::from(l.align));
    m.insert("locationSet".into(), Value::Bool(l.location_set));
    m.insert("locationX".into(), Value::from(l.location_x));
    m.insert("locationY".into(), Value::from(l.location_y));
    m.insert("rotation".into(), Value::from(l.rotation));
    m
}

/// The original's `map_to_layout`.
pub fn map_to_layout(m: &VMap) -> pb::LayoutPrefs {
    pb::LayoutPrefs {
        fillmode: get_i32(m, "fillmode"),
        align: get_i32(m, "align"),
        rotation: get_i32(m, "rotation"),
        location_x: get_u32(m, "locationX"),
        location_y: get_u32(m, "locationY"),
        location_set: get_bool(m, "locationSet"),
        // waywallen 0.4.4: mirroring is not used in
        // `GlobalSettings.layout_defaults` (control.proto), the original's
        // `map_to_layout` does not set it — FLIP_UNSPECIFIED.
        flip: 0,
    }
}

/// Default resume delay when the daemon did not send it. The vendor number
/// is the same in three places of release v0.3.9: `settings_query.cpp` (both
/// sides of the map) and `SettingsPage.qml`.
pub const DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS: u32 = 250;

/// The original's `auto_replay_to_map`.
pub fn auto_replay_to_map(p: &pb::AutoReplayPolicy) -> VMap {
    let mut m = VMap::new();
    m.insert("anyWindow".into(), Value::from(p.any_window));
    m.insert("focused".into(), Value::from(p.focused));
    m.insert("maximized".into(), Value::from(p.maximized));
    m.insert("fullscreen".into(), Value::from(p.fullscreen));
    m.insert("sessionLocked".into(), Value::from(p.session_locked));
    m.insert("sessionInactive".into(), Value::from(p.session_inactive));
    // waywallen 0.4.2: the fifth pause reason — an active GameMode (the
    // `AutoAction` enum, like the other reasons).
    m.insert("gamemode".into(), Value::from(p.gamemode));
    // waywallen 0.5.0: the sixth pause reason — running on battery
    // (`AutoAction`). The settings page has no row for it yet, but the map
    // must carry it: `settings_set` replaces the policy entirely, and
    // without the key every save from Rexon would reset the choice made in
    // another client. The key is the same as in the 0.5.0 vendor UI's
    // `auto_replay_to_map` (`settings_query.cpp`: `onBattery`).
    m.insert("onBattery".into(), Value::from(p.on_battery));
    // The field is optional: «not sent» turns into 250 at the vendor, not 0
    // — otherwise the page slider would sit at zero and the very first
    // settings write would cancel a delay the person never touched.
    m.insert(
        "resumeDelayMs".into(),
        Value::from(
            p.resume_delay_ms
                .unwrap_or(DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS),
        ),
    );
    // waywallen 0.4.2: which monitors the window-based pause applies to —
    // `AutoScope` (0 = only the monitor with the window; 1 = all monitors).
    // Zero is both the protocol default and the daemon's behaviour before 0.4.2.
    m.insert("anyWindowScope".into(), Value::from(p.any_window_scope));
    m.insert("focusedScope".into(), Value::from(p.focused_scope));
    m.insert("maximizedScope".into(), Value::from(p.maximized_scope));
    m.insert("fullscreenScope".into(), Value::from(p.fullscreen_scope));
    // waywallen 0.4.4: window exceptions for the pause (by app id and title,
    // exactly or by a `*`/`?` pattern). The page does not edit them yet, but
    // the map must carry them: `settings_set` is a full replacement, and
    // `auto_replay` without them would erase exceptions set by the person.
    let ex = p.window_exclusions.clone().unwrap_or_default();
    m.insert(
        "excludedApplicationIds".into(),
        strings_value(&ex.application_ids),
    );
    m.insert("excludedWindowTitles".into(), strings_value(&ex.titles));
    m.insert(
        "excludedApplicationIdPatterns".into(),
        strings_value(&ex.application_id_patterns),
    );
    m.insert(
        "excludedWindowTitlePatterns".into(),
        strings_value(&ex.title_patterns),
    );
    m
}

/// The original's `map_to_auto_replay`.
pub fn map_to_auto_replay(m: &VMap) -> pb::AutoReplayPolicy {
    pb::AutoReplayPolicy {
        any_window: get_i32(m, "anyWindow"),
        focused: get_i32(m, "focused"),
        maximized: get_i32(m, "maximized"),
        fullscreen: get_i32(m, "fullscreen"),
        session_locked: get_i32(m, "sessionLocked"),
        session_inactive: get_i32(m, "sessionInactive"),
        // The original's `m.value("resumeDelayMs", 250).toUInt()`: no key —
        // 250, not 0.
        resume_delay_ms: Some(get_u32_or(
            m,
            "resumeDelayMs",
            DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS,
        )),
        // waywallen 0.4.2 (`settings_query.cpp` v0.4.2): no key — 0, i.e.
        // `AUTO_ACTION_NONE` for GameMode and `AUTO_SCOPE_CURRENT_DISPLAY` for
        // the scope: exactly how the daemon behaved before the fields appeared.
        gamemode: get_i32(m, "gamemode"),
        // waywallen 0.5.0: no key — 0 (`AUTO_ACTION_NONE`), the daemon's default
        // for this reason.
        on_battery: get_i32(m, "onBattery"),
        any_window_scope: get_i32(m, "anyWindowScope"),
        focused_scope: get_i32(m, "focusedScope"),
        maximized_scope: get_i32(m, "maximizedScope"),
        fullscreen_scope: get_i32(m, "fullscreenScope"),
        // waywallen 0.4.4 (`settings_query.cpp` v0.4.4): the exceptions are
        // always set, no key — an empty list, as in the original.
        window_exclusions: Some(pb::WindowExclusions {
            application_ids: string_list(get_list(m, "excludedApplicationIds")),
            titles: string_list(get_list(m, "excludedWindowTitles")),
            application_id_patterns: string_list(get_list(m, "excludedApplicationIdPatterns")),
            title_patterns: string_list(get_list(m, "excludedWindowTitlePatterns")),
        }),
    }
}

/// Default transition duration when the map does not have it
/// (`map_to_transition` of release v0.4.2: `m.value("durationMs", 500)`).
pub const DEFAULT_TRANSITION_DURATION_MS: u32 = 500;
/// Default «grow» center, percent of the surface (same place: 50/50).
pub const DEFAULT_TRANSITION_ORIGIN: u32 = 50;

/// The original's `transition_to_map` (waywallen 0.4.2): the wallpaper change animation.
pub fn transition_to_map(p: &pb::TransitionConfig) -> VMap {
    let mut m = VMap::new();
    m.insert("kind".into(), Value::from(p.kind));
    m.insert("durationMs".into(), Value::from(p.duration_ms));
    m.insert("angle".into(), Value::from(p.angle));
    m.insert("originX".into(), Value::from(p.origin_x));
    m.insert("originY".into(), Value::from(p.origin_y));
    m
}

/// The original's `map_to_transition`: no key — the vendor defaults (kind 0 =
/// no transition, 500 ms, angle 0, center 50/50), not zeros.
pub fn map_to_transition(m: &VMap) -> pb::TransitionConfig {
    pb::TransitionConfig {
        kind: get_i32(m, "kind"),
        duration_ms: get_u32_or(m, "durationMs", DEFAULT_TRANSITION_DURATION_MS),
        angle: get_u32(m, "angle"),
        origin_x: get_u32_or(m, "originX", DEFAULT_TRANSITION_ORIGIN),
        origin_y: get_u32_or(m, "originY", DEFAULT_TRANSITION_ORIGIN),
    }
}

// ── GlobalSettings ───────────────────────────────────────────────────────

/// Key of the flat «renderer sound» pair (`GlobalRendererSettings.enable_audio`).
pub const KEY_RENDERER_ENABLE_AUDIO: &str = "renderer.enable_audio";
/// Key of the flat «renderer volume» pair (`GlobalRendererSettings.volume`).
pub const KEY_RENDERER_VOLUME: &str = "renderer.volume";
/// Key of the «renderer output device» pair (`GlobalRendererSettings.audio_output`,
/// waywallen 0.6.2): a sink name from `AudioOutputList`, empty — the system default.
pub const KEY_RENDERER_AUDIO_OUTPUT: &str = "renderer.audio_output";
/// Default volume when the daemon did not send it (original: 100).
pub const DEFAULT_RENDERER_VOLUME: u32 = 100;

/// Default blur radius on pause — as on the vendor settings page
/// (`SettingsPage.qml:289`).
pub const DEFAULT_BLUR_RADIUS: u32 = 30;

/// The original's `global_to_map`: `GlobalSettings` → a map for QML.
///
/// The key set is exactly the original's — it covers ALL fields of the
/// message, so the reverse [`map_to_global`] loses nothing on a full
/// replacement. Two keys are renamed (as in the original, QML knows only
/// them): `pluginUpdateNotifications` — the INVERSE of
/// `disable_plugin_update_notifications`, `duplicateRenderers` —
/// `duplicate_renderers_for_same_wallpaper`. Renderer settings go as a flat
/// `renderer.*` pair, not a nested map.
pub fn global_to_map(g: &pb::GlobalSettings) -> VMap {
    let mut m = VMap::new();
    m.insert(
        "wallpaperFilters".into(),
        maps_value(&g.wallpaper_filters, filter_rule_to_map),
    );
    m.insert(
        "wallpaperFilterLogics".into(),
        maps_value(&g.wallpaper_filter_logics, filter_logic_to_map),
    );
    m.insert(
        "wallpaperSorts".into(),
        maps_value(&g.wallpaper_sorts, sort_rule_to_map),
    );
    if let Some(l) = &g.layout_defaults {
        m.insert("layoutDefaults".into(), Value::Object(layout_to_map(l)));
    }
    if let Some(p) = &g.auto_replay {
        m.insert("autoReplay".into(), Value::Object(auto_replay_to_map(p)));
    }
    m.insert("queueMode".into(), Value::String(g.queue_mode.clone()));
    m.insert("rotationSecs".into(), Value::from(g.rotation_secs));
    m.insert("audioFadeMs".into(), Value::from(g.audio_fade_ms));
    // Original: hasMuteWhenOtherAudio() && muteWhenOtherAudio() — «not
    // sent» and «sent false» are the same thing for the UI.
    m.insert(
        "muteWhenOtherAudio".into(),
        Value::Bool(g.mute_when_other_audio.unwrap_or(false)),
    );
    m.insert(
        "audioCaptureEnabled".into(),
        Value::Bool(g.audio_capture_enabled),
    );
    // v0.3.5. The pause effect arrives as a nested message; the map is put
    // ALWAYS and in full — the page edits a copy and sends it back whole,
    // and the daemon's `settings_set` is a full replacement: a missing key
    // would erase the setting.
    m.insert(
        "pauseEffect".into(),
        Value::Object(pause_effect_to_map(g.pause_effect.as_ref())),
    );
    // waywallen 0.4.2: as in the original — only if the daemon sent it. The
    // page normalizes an empty one itself (`_normalizedTransition`), and the
    // reverse path puts the field only when the key is present.
    if let Some(t) = &g.transition {
        m.insert("transition".into(), Value::Object(transition_to_map(t)));
    }
    m.insert(
        "pointerForwardingEnabled".into(),
        Value::Bool(g.pointer_forwarding_enabled),
    );
    m.insert("hideTrayIcon".into(), Value::Bool(g.hide_tray_icon));
    // waywallen 0.3.8: debug log level of the daemon and renderer.
    m.insert(
        "debugLoggingEnabled".into(),
        Value::Bool(g.debug_logging_enabled),
    );
    m.insert(
        "pluginUpdateNotifications".into(),
        Value::Bool(!g.disable_plugin_update_notifications),
    );
    m.insert(
        "duplicateRenderers".into(),
        Value::Bool(g.duplicate_renderers_for_same_wallpaper),
    );
    // Both `GlobalRendererSettings` fields have a presence flag: no message
    // or no field → default values (sound on, volume 100), as in the
    // original.
    let r = g.renderer.as_ref();
    m.insert(
        KEY_RENDERER_ENABLE_AUDIO.into(),
        Value::Bool(r.is_none_or(|r| r.enable_audio.unwrap_or(true))),
    );
    m.insert(
        KEY_RENDERER_VOLUME.into(),
        Value::from(r.and_then(|r| r.volume).unwrap_or(DEFAULT_RENDERER_VOLUME)),
    );
    m.insert(
        KEY_RENDERER_AUDIO_OUTPUT.into(),
        Value::String(r.and_then(|r| r.audio_output.clone()).unwrap_or_default()),
    );
    m.insert(
        "wallpaperSkipTypes".into(),
        strings_value(&g.wallpaper_skip_types),
    );
    m.insert(
        "wallpaperFilterTags".into(),
        strings_value(&g.wallpaper_filter_tags),
    );
    m.insert(
        "wallpaperSkipContentRatings".into(),
        strings_value(&g.wallpaper_skip_content_ratings),
    );
    // waywallen 0.4.4: how the library shows hidden wallpapers
    // (`WallpaperHiddenFilter`, 0 = hide — the daemon's default).
    m.insert(
        "wallpaperHiddenFilter".into(),
        Value::from(g.wallpaper_hidden_filter),
    );
    // waywallen 0.5.0: hidden libraries of the «Wallpapers» page. As in the
    // original — the key exists ONLY when the daemon sent the message: its
    // absence in `settings_set` means «do not touch», while a sent empty
    // list means «show all». Substituting an empty list for «not sent»
    // would erase the hiding at the very first settings write.
    if let Some(v) = &g.wallpaper_library_visibility {
        m.insert(
            KEY_WALLPAPER_HIDDEN_LIBRARY_IDS.into(),
            Value::Array(v.hidden_library_ids.iter().map(|id| Value::from(*id)).collect()),
        );
    }
    m
}

/// Key of hidden libraries in the `global` map (waywallen 0.5.0,
/// `settings_query.cpp`).
pub const KEY_WALLPAPER_HIDDEN_LIBRARY_IDS: &str = "wallpaperHiddenLibraryIds";

/// `PauseEffectConfig` → a QML map. The shape is like `_defaultPauseEffect`
/// of the vendor settings page (`SettingsPage.qml:286`): the effect kind plus
/// a nested blur map, default radius 30.
fn pause_effect_to_map(p: Option<&pb::PauseEffectConfig>) -> VMap {
    let mut blur = VMap::new();
    blur.insert(
        "radius".into(),
        Value::from(
            p.and_then(|p| p.blur.as_ref())
                .map(|b| b.radius)
                .filter(|r| *r > 0)
                .unwrap_or(DEFAULT_BLUR_RADIUS),
        ),
    );
    let mut out = VMap::new();
    out.insert("kind".into(), Value::from(p.map(|p| p.kind).unwrap_or(0)));
    out.insert("blur".into(), Value::Object(blur));
    out
}

/// A QML map → `PauseEffectConfig`.
fn map_to_pause_effect(m: &VMap) -> pb::PauseEffectConfig {
    let radius = as_map(m.get("blur").unwrap_or(&Value::Null))
        .get("radius")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_BLUR_RADIUS as u64) as u32;
    pb::PauseEffectConfig {
        kind: m.get("kind").and_then(Value::as_i64).unwrap_or(0) as i32,
        blur: Some(pb::BlurEffectConfig { radius }),
    }
}

/// The original's `map_to_global`: a map from QML → `GlobalSettings`.
///
/// Rule lists are ALWAYS set (an empty list is a legitimate «no filters»),
/// the rest — only when the key exists: the page edits a copy of the full map
/// from `settings_get`, and the absence of a key means «the field did not
/// come from the daemon». The only exception is `audioCaptureEnabled`: the
/// original writes it unconditionally (no key → false), kept as is.
pub fn map_to_global(m: &VMap) -> pb::GlobalSettings {
    let mut g = pb::GlobalSettings {
        wallpaper_filters: get_list(m, "wallpaperFilters")
            .iter()
            .map(|v| map_to_filter_rule(&as_map(v)))
            .collect(),
        wallpaper_filter_logics: get_list(m, "wallpaperFilterLogics")
            .iter()
            .map(|v| map_to_filter_logic(&as_map(v)))
            .collect(),
        wallpaper_sorts: get_list(m, "wallpaperSorts")
            .iter()
            .map(|v| map_to_sort_rule(&as_map(v)))
            .collect(),
        audio_capture_enabled: get_bool(m, "audioCaptureEnabled"),
        pause_effect: m.get("pauseEffect").map(|v| map_to_pause_effect(&as_map(v))),
        pointer_forwarding_enabled: get_bool(m, "pointerForwardingEnabled"),
        hide_tray_icon: get_bool(m, "hideTrayIcon"),
        debug_logging_enabled: get_bool(m, "debugLoggingEnabled"),
        ..Default::default()
    };
    // Round-tripping layout_defaults: the page does not edit them, but must
    // return them to the daemon — otherwise a full replacement would erase the
    // layout.
    if let Some(l) = get_map(m, "layoutDefaults") {
        g.layout_defaults = Some(map_to_layout(l));
    }
    if let Some(p) = get_map(m, "autoReplay") {
        g.auto_replay = Some(map_to_auto_replay(p));
    }
    if let Some(t) = get_map(m, "transition") {
        g.transition = Some(map_to_transition(t));
    }
    if m.contains_key("queueMode") {
        g.queue_mode = get_str(m, "queueMode");
    }
    if m.contains_key("rotationSecs") {
        g.rotation_secs = get_u32(m, "rotationSecs");
    }
    if m.contains_key("audioFadeMs") {
        g.audio_fade_ms = get_u32(m, "audioFadeMs");
    }
    if m.contains_key("muteWhenOtherAudio") {
        g.mute_when_other_audio = Some(get_bool(m, "muteWhenOtherAudio"));
    }
    if m.contains_key("pluginUpdateNotifications") {
        g.disable_plugin_update_notifications = !get_bool(m, "pluginUpdateNotifications");
    }
    if m.contains_key("duplicateRenderers") {
        g.duplicate_renderers_for_same_wallpaper = get_bool(m, "duplicateRenderers");
    }
    if m.contains_key(KEY_RENDERER_ENABLE_AUDIO)
        || m.contains_key(KEY_RENDERER_VOLUME)
        || m.contains_key(KEY_RENDERER_AUDIO_OUTPUT)
    {
        let mut r = pb::GlobalRendererSettings::default();
        if m.contains_key(KEY_RENDERER_ENABLE_AUDIO) {
            r.enable_audio = Some(get_bool(m, KEY_RENDERER_ENABLE_AUDIO));
        }
        if m.contains_key(KEY_RENDERER_VOLUME) {
            r.volume = Some(get_u32(m, KEY_RENDERER_VOLUME));
        }
        if m.contains_key(KEY_RENDERER_AUDIO_OUTPUT) {
            r.audio_output = Some(get_str(m, KEY_RENDERER_AUDIO_OUTPUT));
        }
        g.renderer = Some(r);
    }
    if m.contains_key("wallpaperSkipTypes") {
        g.wallpaper_skip_types = string_list(get_list(m, "wallpaperSkipTypes"));
    }
    if m.contains_key("wallpaperFilterTags") {
        g.wallpaper_filter_tags = string_list(get_list(m, "wallpaperFilterTags"));
    }
    if m.contains_key("wallpaperSkipContentRatings") {
        g.wallpaper_skip_content_ratings = string_list(get_list(m, "wallpaperSkipContentRatings"));
    }
    if m.contains_key("wallpaperHiddenFilter") {
        g.wallpaper_hidden_filter = get_i32(m, "wallpaperHiddenFilter");
    }
    // waywallen 0.5.0: the key exists — the field is set (an empty list is
    // legitimate and means «show all»), no key — the field is not set, and
    // the daemon keeps the previous hiding. The order of numbers is as the
    // page sent it: `WallpaperListQuery` normalizes them, not the settings.
    if m.contains_key(KEY_WALLPAPER_HIDDEN_LIBRARY_IDS) {
        g.wallpaper_library_visibility = Some(pb::WallpaperLibraryVisibility {
            hidden_library_ids: get_list(m, KEY_WALLPAPER_HIDDEN_LIBRARY_IDS)
                .iter()
                .map(value_i64)
                .collect(),
        });
    }
    g
}

/// Schema of the `global` map: key → value kind.
///
/// Needed by exactly one consumer — the PySide6 shim, which extracts values
/// from a `QJSValue` (the map comes from QML and may carry Qt-Protobuf
/// gadgets). Only this side knows the field types, so the key list lives
/// here, next to [`global_to_map`], and is not duplicated in Python (rule
/// #12). The test `global_map_schema_covers_every_key` makes sure they do not
/// diverge.
pub const GLOBAL_MAP_SCHEMA: &[(&str, GlobalValueKind)] = &[
    ("wallpaperFilters", GlobalValueKind::FilterRuleList),
    ("wallpaperFilterLogics", GlobalValueKind::FilterLogicList),
    ("wallpaperSorts", GlobalValueKind::SortRuleList),
    ("layoutDefaults", GlobalValueKind::LayoutPrefs),
    ("autoReplay", GlobalValueKind::AutoReplayPolicy),
    ("queueMode", GlobalValueKind::String),
    ("rotationSecs", GlobalValueKind::Int),
    ("audioFadeMs", GlobalValueKind::Int),
    ("muteWhenOtherAudio", GlobalValueKind::Bool),
    ("audioCaptureEnabled", GlobalValueKind::Bool),
    ("pauseEffect", GlobalValueKind::PauseEffectConfig),
    ("transition", GlobalValueKind::TransitionConfig),
    ("pointerForwardingEnabled", GlobalValueKind::Bool),
    ("hideTrayIcon", GlobalValueKind::Bool),
    ("debugLoggingEnabled", GlobalValueKind::Bool),
    ("pluginUpdateNotifications", GlobalValueKind::Bool),
    ("duplicateRenderers", GlobalValueKind::Bool),
    (KEY_RENDERER_ENABLE_AUDIO, GlobalValueKind::Bool),
    (KEY_RENDERER_VOLUME, GlobalValueKind::Int),
    (KEY_RENDERER_AUDIO_OUTPUT, GlobalValueKind::String),
    ("wallpaperSkipTypes", GlobalValueKind::StringList),
    ("wallpaperFilterTags", GlobalValueKind::StringList),
    ("wallpaperSkipContentRatings", GlobalValueKind::StringList),
    ("wallpaperHiddenFilter", GlobalValueKind::Int),
    (KEY_WALLPAPER_HIDDEN_LIBRARY_IDS, GlobalValueKind::IntList),
];

/// Value kind of a `global` map key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalValueKind {
    Bool,
    Int,
    String,
    StringList,
    /// waywallen 0.5.0: a list of integers (library numbers).
    IntList,
    LayoutPrefs,
    AutoReplayPolicy,
    PauseEffectConfig,
    TransitionConfig,
    FilterRuleList,
    FilterLogicList,
    SortRuleList,
}

impl GlobalValueKind {
    /// Kind name for the shim (it is also the key of the «kind → protobuf
    /// message» mapping table on the Python side).
    pub fn name(self) -> &'static str {
        match self {
            GlobalValueKind::Bool => "bool",
            GlobalValueKind::Int => "int",
            GlobalValueKind::String => "string",
            GlobalValueKind::StringList => "strings",
            GlobalValueKind::IntList => "ints",
            GlobalValueKind::LayoutPrefs => "layoutPrefs",
            GlobalValueKind::AutoReplayPolicy => "autoReplayPolicy",
            GlobalValueKind::PauseEffectConfig => "pauseEffectConfig",
            GlobalValueKind::TransitionConfig => "transitionConfig",
            GlobalValueKind::FilterRuleList => "filterRuleList",
            GlobalValueKind::FilterLogicList => "filterLogicList",
            GlobalValueKind::SortRuleList => "sortRuleList",
        }
    }
}

// ── PluginSettings ───────────────────────────────────────────────────────

/// The original's `plugins_to_map`: `name → PluginSettings` → `name → {key:
/// value}`. The `values` level of the proto is REMOVED — QML reads plugin
/// values directly (`currentValues[key]`).
pub fn plugins_to_map(plugins: &HashMap<String, pb::PluginSettings>) -> VMap {
    let mut out = VMap::new();
    // The proto map order is non-deterministic — we sort so the map is
    // comparable (the `pluginsChanged` signal is sent only on a real
    // change) and so the goldens do not drift.
    let mut names: Vec<&String> = plugins.keys().collect();
    names.sort();
    for name in names {
        let mut inner = VMap::new();
        let mut keys: Vec<&String> = plugins[name].values.keys().collect();
        keys.sort();
        for k in keys {
            inner.insert(k.clone(), Value::String(plugins[name].values[k].clone()));
        }
        out.insert(name.clone(), Value::Object(inner));
    }
    out
}

/// The original's `map_to_plugins` (the reverse direction).
pub fn map_to_plugins(m: &VMap) -> HashMap<String, pb::PluginSettings> {
    let mut out = HashMap::new();
    for (name, cfg) in m {
        let mut values = HashMap::new();
        if let Value::Object(inner) = cfg {
            for (k, v) in inner {
                values.insert(
                    k.clone(),
                    match v {
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => String::new(),
                    },
                );
            }
        }
        out.insert(name.clone(), pb::PluginSettings { values });
    }
    out
}

// ── SettingsGetQuery ─────────────────────────────────────────────────────

/// Properties of `SettingsGetQuery` (`global` and `plugins` in QML terms).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsGetData {
    pub global: VMap,
    pub plugins: VMap,
    /// waywallen 0.3.8: the daemon's log directory (read only).
    pub log_dir: String,
    /// waywallen 0.3.8: `WW_LOG` is set in the daemon's environment — the log
    /// level is set from outside, and the switch in settings does not work.
    pub ww_log_active: bool,
}

/// Parse the `settings_get` response.
///
/// Like the other batch 3 requests, property signals are sent only on a real
/// change (the original sends them unconditionally, see the header of
/// `we_query.rs`).
pub fn apply_settings_get(
    st: &mut QueryState,
    data: &mut SettingsGetData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::SettingsGet(sg)) = rsp.payload {
        // The original calls global_to_map(get_rsp.global()) without a
        // presence check: in Qt-Protobuf reading an unset message returns a
        // message with default fields.
        let g = global_to_map(&sg.global.unwrap_or_default());
        q::assign(&mut data.global, g, "global", &mut out);
        let p = plugins_to_map(&sg.plugins);
        q::assign(&mut data.plugins, p, "plugins", &mut out);
        q::assign(&mut data.log_dir, sg.log_dir, "logDir", &mut out);
        q::assign(
            &mut data.ww_log_active,
            sg.ww_log_active,
            "wwLogActive",
            &mut out,
        );
    }
    Ok(q::finish(st, out))
}

// ── AudioOutputListQuery (waywallen 0.6.2) ──────────────────────────────

/// Parse the `audio_output_list` answer: `outputs` as the vendor's maps
/// `{name, description, isDefault}`, `available` — whether a sound server was
/// reached. The vendor sends `outputsChanged` on every answer, so the signal is
/// unconditional here too (the page re-reads the list each time it opens).
pub fn apply_audio_output_list(
    st: &mut QueryState,
    outputs: &mut Vec<Value>,
    available: &mut bool,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::AudioOutputList(list)) = rsp.payload {
        *outputs = list
            .outputs
            .iter()
            .map(|o| {
                serde_json::json!({
                    "name": o.name,
                    "description": o.description,
                    "isDefault": o.is_default,
                })
            })
            .collect();
        *available = list.available;
        out.push(QuerySignal::Property("outputs"));
    }
    Ok(q::finish(st, out))
}

// ── SettingsSetQuery ─────────────────────────────────────────────────────

/// Assemble a `SettingsSetRequest` from QML maps and serialize it.
///
/// The daemon treats the request as a FULL replacement: the `global` map must
/// be a copy of the whole map from `settings_get` with the edit, and `plugins`
/// — all plugins, not just the one changed. The «do not write before the
/// first read» gate lives in QML (`_persistGlobalChange`), it must not be
/// duplicated here — otherwise a legitimate write of empty settings would
/// become impossible.
pub fn encode_settings_set(global: &VMap, plugins: &VMap) -> Vec<u8> {
    pb::SettingsSetRequest {
        global: Some(map_to_global(global)),
        plugins: map_to_plugins(plugins),
    }
    .encode_to_vec()
}
