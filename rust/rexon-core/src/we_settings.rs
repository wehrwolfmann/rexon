//! Настройки вкладки Wallpaper Engine — порт `ui/src/query/settings_query.cpp`
//! waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//!
//! Здесь живёт КОНВЕРТЕР между `GlobalSettings`/`PluginSettings` протокола и
//! картой значений, которую читает и пишет QML. Он один на оба направления:
//! `settings_get` разворачивает proto в карту, `settings_set` собирает из
//! карты proto обратно. Направления обязаны быть зеркальны — демон
//! обрабатывает `settings_set` как ПОЛНУЮ замену, и любое поле, потерянное
//! на круге, у пользователя стирается.
//!
//! Отдельный модуль (а не хвост `we_query.rs`) по той же границе, что у
//! оригинала: у него `settings_query.cpp` тоже отдельная единица трансляции,
//! а конвертер обслуживает не только запросы настроек, но и страницу «Обои»
//! (правила сортировки/фильтров в `WallpaperListQuery`).
//!
//! ⚠️ Прежний Python-мост строил карту РЕФЛЕКСИЕЙ по дескриптору protobuf
//! (`_pb_to_variant`: все поля под `json_name`). Это не то же самое, что
//! `global_to_map` оригинала, и давало ДВА живых дефекта — оба чинятся
//! этим портом (правило #16):
//!
//!   1. `renderer` уезжал вложенной картой `{enableAudio, volume}`, а
//!      `SettingsPage.qml` читает и пишет ПЛОСКИЕ ключи
//!      `renderer.enable_audio` / `renderer.volume`. Итог: галка «звук
//!      рендереров» и ползунок громкости показывали значения по умолчанию
//!      и не сохранялись вовсе (обратный `_js_to_pb` плоские ключи не
//!      видел, а вложенную карту брал из устаревшего снимка).
//!   2. `plugins[имя]` уезжал как `{"values": {ключ: значение}}` вместо
//!      плоской карты значений. `PluginSettingsPage.qml` читает
//!      `currentValues[ключ]` и шлёт обратно плоскую карту — то есть
//!      настройки плагина не читались, а «Применить» отправляло демону
//!      ПУСТУЮ таблицу (полная замена → значения плагина стирались).
//!
//! Значения — `serde_json::Value` (карта = `Map<String, Value>`): ровно те
//! типы, что переживают границу QVariant (bool / число / строка / список /
//! карта). JSON здесь — представление в памяти и формат голденов, по
//! проводу настройки ходят protobuf'ом.

use crate::proto::control as pb;
use crate::we_query::{self as q, QuerySignal, QueryState};
use prost::Message;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Карта значений в терминах QML (`QVariantMap` у оригинала).
pub type VMap = Map<String, Value>;

// ── чтение значений карты (семантика QVariant::toInt/toBool/toString) ────

/// `QVariant::toString()` над одним значением: не-строка приводится к
/// строке, всё прочее (карта, список, `null`) — пустая строка.
pub(crate) fn value_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// `QVariant::toBool()` над одним значением: число — «не ноль», пустая
/// строка и `null` — ложь.
pub(crate) fn value_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty() && s != "false",
        _ => false,
    }
}

/// `QVariant::toInt()` над одним значением: числа из QML приходят
/// double'ами, поэтому дробное округляется.
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

/// То же в разрядности `int` Qt.
pub(crate) fn value_i32(v: &Value) -> i32 {
    value_i64(v) as i32
}

/// `QVariant::toString()`: нет ключа — пустая строка.
fn get_str(m: &VMap, key: &str) -> String {
    m.get(key).map(value_str).unwrap_or_default()
}

/// `QVariant::toInt()`: нет ключа — 0.
fn get_i64(m: &VMap, key: &str) -> i64 {
    m.get(key).map(value_i64).unwrap_or(0)
}

fn get_i32(m: &VMap, key: &str) -> i32 {
    get_i64(m, key) as i32
}

/// `QVariant::toUInt()`: отрицательное в беззнаковое не превращаем — берём 0
/// (у оригинала это UB-подобная зона, до неё QML не доводит: и громкость, и
/// секунды ротации ограничены неотрицательными).
fn get_u32(m: &VMap, key: &str) -> u32 {
    let v = get_i64(m, key);
    if v < 0 {
        0
    } else {
        v as u32
    }
}

/// `QVariant::value(key, fallback).toUInt()`: ключа нет — берём запасное
/// число, а не ноль.
fn get_u32_or(m: &VMap, key: &str, fallback: u32) -> u32 {
    if m.contains_key(key) {
        get_u32(m, key)
    } else {
        fallback
    }
}

/// `QVariant::toBool()`: нет ключа — false.
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

/// Элемент списка → карта (мимо не-карт: у оригинала `value<Gadget>()`
/// на чужом типе отдаёт пустой гаджет, у нас — пустая карта).
pub(crate) fn as_map(v: &Value) -> VMap {
    match v {
        Value::Object(o) => o.clone(),
        _ => VMap::new(),
    }
}

/// Список строк (`QStringList` оригинала): элементы приводятся к строке.
pub(crate) fn string_list(items: &[Value]) -> Vec<String> {
    items.iter().map(value_str).collect()
}

fn strings_value(items: &[String]) -> Value {
    Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
}

fn maps_value<T>(items: &[T], to_map: fn(&T) -> VMap) -> Value {
    Value::Array(items.iter().map(|i| Value::Object(to_map(i))).collect())
}

// ── правила сортировки и фильтров (страница «Обои») ──────────────────────
//
// У оригинала эти три поля кладутся в карту КАК ГАДЖЕТЫ Qt-Protobuf
// (`QVariant::fromValue(rule)`), гаджет умеет и читать, и писать свойства.
// У нас гаджетов на этой стороне нет: карту читает QML (`r.key`,
// `r.direction`) и Rust-модель фильтров, которая приводит карту к гаджету
// сама (`rexon_coerce_gadget`). Поэтому ключи здесь — Qt-Protobuf'овские
// имена свойств (`json_name`, camelCase), значения перечислений — числа.

/// `WallpaperSortRule` → карта.
pub fn sort_rule_to_map(r: &pb::WallpaperSortRule) -> VMap {
    let mut m = VMap::new();
    m.insert("key".into(), Value::from(r.key));
    m.insert("direction".into(), Value::from(r.direction));
    m
}

/// Карта → `WallpaperSortRule`.
pub fn map_to_sort_rule(m: &VMap) -> pb::WallpaperSortRule {
    pb::WallpaperSortRule {
        key: get_i32(m, "key"),
        direction: get_i32(m, "direction"),
    }
}

/// `FilterLogic` → карта.
pub fn filter_logic_to_map(l: &pb::FilterLogic) -> VMap {
    let mut m = VMap::new();
    m.insert("op".into(), Value::from(l.op));
    m.insert("groupA".into(), Value::from(l.group_a));
    m.insert("groupB".into(), Value::from(l.group_b));
    m
}

/// Карта → `FilterLogic`.
pub fn map_to_filter_logic(m: &VMap) -> pb::FilterLogic {
    pb::FilterLogic {
        op: get_i32(m, "op"),
        group_a: get_i32(m, "groupA"),
        group_b: get_i32(m, "groupB"),
    }
}

/// `WallpaperFilterRule` → карта. Ветка `oneof payload` кладётся ТОЛЬКО
/// установленная — гаджет-читатель отличает «нет payload» по отсутствию
/// ключа (у Qt-Protobuf это `hasStringFilter` и компания).
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

/// Карта → `WallpaperFilterRule`.
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

/// `layout_to_map` оригинала.
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

/// `map_to_layout` оригинала.
pub fn map_to_layout(m: &VMap) -> pb::LayoutPrefs {
    pb::LayoutPrefs {
        fillmode: get_i32(m, "fillmode"),
        align: get_i32(m, "align"),
        rotation: get_i32(m, "rotation"),
        location_x: get_u32(m, "locationX"),
        location_y: get_u32(m, "locationY"),
        location_set: get_bool(m, "locationSet"),
        // waywallen 0.4.4: в `GlobalSettings.layout_defaults` отражение не
        // используется (control.proto), `map_to_layout` оригинала его не
        // ставит — FLIP_UNSPECIFIED.
        flip: 0,
    }
}

/// Задержка возобновления по умолчанию, когда демон её не прислал.
/// Вендорное число одно и то же в трёх местах выпуска v0.3.9:
/// `settings_query.cpp` (обе стороны карты) и `SettingsPage.qml`.
pub const DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS: u32 = 250;

/// `auto_replay_to_map` оригинала.
pub fn auto_replay_to_map(p: &pb::AutoReplayPolicy) -> VMap {
    let mut m = VMap::new();
    m.insert("anyWindow".into(), Value::from(p.any_window));
    m.insert("focused".into(), Value::from(p.focused));
    m.insert("maximized".into(), Value::from(p.maximized));
    m.insert("fullscreen".into(), Value::from(p.fullscreen));
    m.insert("sessionLocked".into(), Value::from(p.session_locked));
    m.insert("sessionInactive".into(), Value::from(p.session_inactive));
    // waywallen 0.4.2: пятый повод паузы — активный GameMode (перечисление
    // `AutoAction`, как и остальные поводы).
    m.insert("gamemode".into(), Value::from(p.gamemode));
    // waywallen 0.5.0: шестой повод паузы — питание от батареи
    // (`AutoAction`). Строки на странице настроек под него пока нет, но
    // карта обязана его нести: `settings_set` заменяет политику целиком, и
    // без ключа каждое сохранение из Rexon сбрасывало бы выбор, сделанный
    // в другом клиенте. Ключ — как у `auto_replay_to_map` вендорного UI
    // 0.5.0 (`settings_query.cpp`: `onBattery`).
    m.insert("onBattery".into(), Value::from(p.on_battery));
    // Поле необязательное: «не прислано» у вендора превращается в 250, а
    // не в 0 — иначе ползунок страницы вставал бы на ноль и первая же
    // запись настроек отменяла бы задержку, которой человек не касался.
    m.insert(
        "resumeDelayMs".into(),
        Value::from(
            p.resume_delay_ms
                .unwrap_or(DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS),
        ),
    );
    // waywallen 0.4.2: на какие мониторы действует пауза по окнам —
    // `AutoScope` (0 = только монитор, где окно; 1 = все мониторы). Ноль —
    // и умолчание протокола, и прежнее поведение демона до 0.4.2.
    m.insert("anyWindowScope".into(), Value::from(p.any_window_scope));
    m.insert("focusedScope".into(), Value::from(p.focused_scope));
    m.insert("maximizedScope".into(), Value::from(p.maximized_scope));
    m.insert("fullscreenScope".into(), Value::from(p.fullscreen_scope));
    // waywallen 0.4.4: исключения окон для паузы (по id приложения и
    // заголовку, точно и шаблоном `*`/`?`). Страница их пока не правит, но
    // карта обязана их нести: `settings_set` — полная замена, и
    // `auto_replay` без них стёр бы исключения, заданные человеком.
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

/// `map_to_auto_replay` оригинала.
pub fn map_to_auto_replay(m: &VMap) -> pb::AutoReplayPolicy {
    pb::AutoReplayPolicy {
        any_window: get_i32(m, "anyWindow"),
        focused: get_i32(m, "focused"),
        maximized: get_i32(m, "maximized"),
        fullscreen: get_i32(m, "fullscreen"),
        session_locked: get_i32(m, "sessionLocked"),
        session_inactive: get_i32(m, "sessionInactive"),
        // `m.value("resumeDelayMs", 250).toUInt()` оригинала: ключа нет —
        // 250, а не 0.
        resume_delay_ms: Some(get_u32_or(
            m,
            "resumeDelayMs",
            DEFAULT_AUTO_REPLAY_RESUME_DELAY_MS,
        )),
        // waywallen 0.4.2 (`settings_query.cpp` v0.4.2): ключа нет — 0, то
        // есть `AUTO_ACTION_NONE` для GameMode и `AUTO_SCOPE_CURRENT_DISPLAY`
        // для охвата: ровно то, как демон вёл себя до появления полей.
        gamemode: get_i32(m, "gamemode"),
        // waywallen 0.5.0: ключа нет — 0 (`AUTO_ACTION_NONE`), умолчание
        // демона для этого повода.
        on_battery: get_i32(m, "onBattery"),
        any_window_scope: get_i32(m, "anyWindowScope"),
        focused_scope: get_i32(m, "focusedScope"),
        maximized_scope: get_i32(m, "maximizedScope"),
        fullscreen_scope: get_i32(m, "fullscreenScope"),
        // waywallen 0.4.4 (`settings_query.cpp` v0.4.4): исключения ставятся
        // всегда, нет ключа — пустой список, как у оригинала.
        window_exclusions: Some(pb::WindowExclusions {
            application_ids: string_list(get_list(m, "excludedApplicationIds")),
            titles: string_list(get_list(m, "excludedWindowTitles")),
            application_id_patterns: string_list(get_list(m, "excludedApplicationIdPatterns")),
            title_patterns: string_list(get_list(m, "excludedWindowTitlePatterns")),
        }),
    }
}

/// Длительность перехода по умолчанию, когда в карте её нет
/// (`map_to_transition` выпуска v0.4.2: `m.value("durationMs", 500)`).
pub const DEFAULT_TRANSITION_DURATION_MS: u32 = 500;
/// Центр «роста» по умолчанию, проценты поверхности (там же: 50/50).
pub const DEFAULT_TRANSITION_ORIGIN: u32 = 50;

/// `transition_to_map` оригинала (waywallen 0.4.2): анимация смены обоев.
pub fn transition_to_map(p: &pb::TransitionConfig) -> VMap {
    let mut m = VMap::new();
    m.insert("kind".into(), Value::from(p.kind));
    m.insert("durationMs".into(), Value::from(p.duration_ms));
    m.insert("angle".into(), Value::from(p.angle));
    m.insert("originX".into(), Value::from(p.origin_x));
    m.insert("originY".into(), Value::from(p.origin_y));
    m
}

/// `map_to_transition` оригинала: нет ключа — умолчания вендора (вид 0 =
/// без перехода, 500 мс, угол 0, центр 50/50), а не нули.
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

/// Ключ плоской пары «звук рендереров» (`GlobalRendererSettings.enable_audio`).
pub const KEY_RENDERER_ENABLE_AUDIO: &str = "renderer.enable_audio";
/// Ключ плоской пары «громкость рендереров» (`GlobalRendererSettings.volume`).
pub const KEY_RENDERER_VOLUME: &str = "renderer.volume";
/// Громкость по умолчанию, когда демон её не прислал (оригинал: 100).
pub const DEFAULT_RENDERER_VOLUME: u32 = 100;

/// Радиус размытия на паузе по умолчанию — как у вендорной страницы
/// настроек (`SettingsPage.qml:289`).
pub const DEFAULT_BLUR_RADIUS: u32 = 30;

/// `global_to_map` оригинала: `GlobalSettings` → карта для QML.
///
/// Набор ключей ровно как у оригинала — он покрывает ВСЕ поля сообщения,
/// поэтому обратный [`map_to_global`] ничего не теряет при полной замене.
/// Два ключа переименованы (так у оригинала, QML знает только их):
/// `pluginUpdateNotifications` — ИНВЕРСИЯ `disable_plugin_update_notifications`,
/// `duplicateRenderers` — `duplicate_renderers_for_same_wallpaper`.
/// Настройки рендерера — плоской парой `renderer.*`, а не вложенной картой.
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
    // Оригинал: hasMuteWhenOtherAudio() && muteWhenOtherAudio() — «не
    // прислано» и «прислано false» для UI одно и то же.
    m.insert(
        "muteWhenOtherAudio".into(),
        Value::Bool(g.mute_when_other_audio.unwrap_or(false)),
    );
    m.insert(
        "audioCaptureEnabled".into(),
        Value::Bool(g.audio_capture_enabled),
    );
    // v0.3.5. Эффект на паузе приезжает вложенным сообщением; карту кладём
    // ВСЕГДА и полной — страница правит копию и шлёт её назад целиком, а
    // `settings_set` у демона — полная замена: пропавший ключ стёр бы
    // настройку.
    m.insert(
        "pauseEffect".into(),
        Value::Object(pause_effect_to_map(g.pause_effect.as_ref())),
    );
    // waywallen 0.4.2: как у оригинала — только если демон прислал.
    // Страница сама нормализует пустое (`_normalizedTransition`), а
    // обратный путь кладёт поле лишь при наличии ключа.
    if let Some(t) = &g.transition {
        m.insert("transition".into(), Value::Object(transition_to_map(t)));
    }
    m.insert(
        "pointerForwardingEnabled".into(),
        Value::Bool(g.pointer_forwarding_enabled),
    );
    m.insert("hideTrayIcon".into(), Value::Bool(g.hide_tray_icon));
    // waywallen 0.3.8: отладочный уровень журнала демона и рендерера.
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
    // Оба поля `GlobalRendererSettings` — с признаком присутствия: нет
    // сообщения либо нет поля → значения по умолчанию (звук включён,
    // громкость 100), как у оригинала.
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
    // waywallen 0.4.4: как библиотека показывает скрытые обои
    // (`WallpaperHiddenFilter`, 0 = прятать — умолчание демона).
    m.insert(
        "wallpaperHiddenFilter".into(),
        Value::from(g.wallpaper_hidden_filter),
    );
    // waywallen 0.5.0: скрытые библиотеки страницы «Обои». Как у
    // оригинала — ключ есть ТОЛЬКО когда демон прислал сообщение: его
    // отсутствие в `settings_set` значит «не трогать», а присланный пустой
    // список — «показать все». Подставить пустой список вместо «не
    // прислано» значило бы стереть скрытие при первой же записи настроек.
    if let Some(v) = &g.wallpaper_library_visibility {
        m.insert(
            KEY_WALLPAPER_HIDDEN_LIBRARY_IDS.into(),
            Value::Array(v.hidden_library_ids.iter().map(|id| Value::from(*id)).collect()),
        );
    }
    m
}

/// Ключ скрытых библиотек в карте `global` (waywallen 0.5.0,
/// `settings_query.cpp`).
pub const KEY_WALLPAPER_HIDDEN_LIBRARY_IDS: &str = "wallpaperHiddenLibraryIds";

/// `PauseEffectConfig` → карта QML. Форма — как у `_defaultPauseEffect`
/// вендорной страницы настроек (`SettingsPage.qml:286`): вид эффекта плюс
/// вложенная карта размытия, радиус по умолчанию 30.
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

/// Карта QML → `PauseEffectConfig`.
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

/// `map_to_global` оригинала: карта из QML → `GlobalSettings`.
///
/// Списки правил ставятся ВСЕГДА (пустой список — законное «фильтров нет»),
/// остальное — только когда ключ есть: страница правит копию полной карты
/// из `settings_get`, и отсутствие ключа значит «поле не пришло от демона».
/// Единственное исключение — `audioCaptureEnabled`: оригинал пишет его
/// безусловно (нет ключа → false), сохранено как есть.
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
    // Круговой перенос layout_defaults: страница их не редактирует, но
    // обязана вернуть демону — иначе полная замена стёрла бы раскладку.
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
    if m.contains_key(KEY_RENDERER_ENABLE_AUDIO) || m.contains_key(KEY_RENDERER_VOLUME) {
        let mut r = pb::GlobalRendererSettings::default();
        if m.contains_key(KEY_RENDERER_ENABLE_AUDIO) {
            r.enable_audio = Some(get_bool(m, KEY_RENDERER_ENABLE_AUDIO));
        }
        if m.contains_key(KEY_RENDERER_VOLUME) {
            r.volume = Some(get_u32(m, KEY_RENDERER_VOLUME));
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
    // waywallen 0.5.0: ключ есть — поле ставится (пустой список законен и
    // значит «показать все»), ключа нет — поле не ставится, и демон
    // сохраняет прежнее скрытие. Порядок номеров — как прислала страница:
    // нормализует их `WallpaperListQuery`, а не настройки.
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

/// Схема карты `global`: ключ → вид значения.
///
/// Нужна ровно одному потребителю — шиму PySide6, который достаёт значения
/// из `QJSValue` (карта приходит из QML и может нести Qt-Protobuf-гаджеты).
/// Типы полей знает только эта сторона, поэтому список ключей живёт здесь,
/// рядом с [`global_to_map`], а не дублируется в Python (правило #12).
/// Тест `global_map_schema_covers_every_key` следит, чтобы они не разошлись.
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
    ("wallpaperSkipTypes", GlobalValueKind::StringList),
    ("wallpaperFilterTags", GlobalValueKind::StringList),
    ("wallpaperSkipContentRatings", GlobalValueKind::StringList),
    ("wallpaperHiddenFilter", GlobalValueKind::Int),
    (KEY_WALLPAPER_HIDDEN_LIBRARY_IDS, GlobalValueKind::IntList),
];

/// Вид значения ключа карты `global`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalValueKind {
    Bool,
    Int,
    String,
    StringList,
    /// waywallen 0.5.0: список целых (номера библиотек).
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
    /// Имя вида для шима (он же — ключ таблицы соответствия
    /// «вид → сообщение protobuf» на стороне Python).
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

/// `plugins_to_map` оригинала: `имя → PluginSettings` → `имя → {ключ:
/// значение}`. Уровень `values` из proto СНИМАЕТСЯ — QML читает значения
/// плагина напрямую (`currentValues[ключ]`).
pub fn plugins_to_map(plugins: &HashMap<String, pb::PluginSettings>) -> VMap {
    let mut out = VMap::new();
    // Порядок карты proto недетерминирован — сортируем, чтобы карта была
    // сравнимой (сигнал `pluginsChanged` шлём только при реальном
    // изменении) и чтобы голдены не плавали.
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

/// `map_to_plugins` оригинала (обратное направление).
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

/// Свойства `SettingsGetQuery` (`global` и `plugins` в терминах QML).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsGetData {
    pub global: VMap,
    pub plugins: VMap,
    /// waywallen 0.3.8: каталог журнала демона (только чтение).
    pub log_dir: String,
    /// waywallen 0.3.8: в окружении демона задан `WW_LOG` — уровень
    /// журнала выставлен снаружи, и переключатель в настройках не работает.
    pub ww_log_active: bool,
}

/// Разобрать ответ `settings_get`.
///
/// Как и остальные запросы партии 3, сигналы свойств шлём только при
/// реальном изменении (оригинал шлёт их безусловно, см. заголовок
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
        // Оригинал зовёт global_to_map(get_rsp.global()) без проверки
        // присутствия: у Qt-Protobuf чтение неустановленного сообщения
        // отдаёт сообщение с полями по умолчанию.
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

// ── SettingsSetQuery ─────────────────────────────────────────────────────

/// Собрать `SettingsSetRequest` из карт QML и сериализовать.
///
/// Демон трактует запрос как ПОЛНУЮ замену: карта `global` должна быть
/// копией всей карты из `settings_get` с правкой, а `plugins` — всеми
/// плагинами, а не одним изменённым. Гейт «не писать до первого чтения»
/// живёт в QML (`_persistGlobalChange`), здесь его дублировать нельзя —
/// иначе законная запись пустых настроек стала бы невозможной.
pub fn encode_settings_set(global: &VMap, plugins: &VMap) -> Vec<u8> {
    pb::SettingsSetRequest {
        global: Some(map_to_global(global)),
        plugins: map_to_plugins(plugins),
    }
    .encode_to_vec()
}
