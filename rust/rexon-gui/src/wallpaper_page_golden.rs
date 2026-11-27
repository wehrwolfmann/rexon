// Golden file of the "Wallpapers" page: parsing the reference, building
// frames and replaying the PURE layer.
//
// The reference is tests/golden/wallpaper_page_cases.json, captured by the
// generator next to it from the LIVE Python bridge (37 cases, 564 signal
// records). Here it is turned into the port's log records and played back
// in two ways:
//
//   * the PURE layer — `cargo test` (the bottom of this file). It replays
//     the CORE's decisions (we_wallpaper/we_query) and the pure model
//     `WallpaperRows`, and repeats the dispatch by the same rules as the
//     query (`LIST_RELOAD_ON` is taken from it, there is no second copy of
//     the list). Only here are the model's STRUCTURAL signals (`modelReset`)
//     visible: in Qt they are private (QPrivateSignal), and Rust cannot
//     subscribe to them.
//     Warning: since it REPEATS the dispatch rather than calling it, the order
//     "the model is rebuilt BEFORE itemsChanged" is proven not by it but by
//     the bench: there the LIVE model's `countChanged` stands in the same
//     place, and a swap in the port immediately fails the comparison
//     (verified with a bite).
//   * the Qt layer — the `wallpaper_page_selftest` bench on LIVE QObjects
//     with all eight queries of the page and a REAL debounce timer.
//
// Why two runs and not one: `#[cfg(test)]` does not work in cxx-qt bridge
// files (the build script runs once for lib and for test), so the Qt layer
// is checked by a bench binary. Batch 3 (discover_queries) got around it
// the same way.
//
// Warning: divergences from the reference are NOT fudged. In places the port
// behaves differently from Python — but because Python diverged from the
// ORIGINAL (waywallen `ui/src/query/wallpaper_query.cpp`), and the port is
// made after the original. Every such adjustment of the reference is listed
// in [`FIXUPS`], carries a reason and MUST fire: if an adjustment stops
// being needed, the run complains — so a stale allowance will not survive
// a fix.

use prost::Message;
use serde_json::{Map, Value};

use rexon_core::proto::control as pb;
use rexon_core::we_query::{self, QuerySignal, QueryState};
use rexon_core::we_settings::VMap;
use rexon_core::we_wallpaper;

use crate::discover_queries::golden::{bool_at, i64_at, str_at, strings_at};
use crate::wallpaper_list_model::{ModelEvent, WallpaperRows};

/// The golden file in the crate sources — the very file printed by
/// tests/golden/wallpaper_page_cases.py from the live Python bridge.
pub const GOLDEN_JSON: &str = include_str!("../tests/golden/wallpaper_page_cases.json");

pub fn load() -> Value {
    serde_json::from_str(GOLDEN_JSON).expect("голден читается")
}

fn array_at<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

// ── Log ─────────────────────────────────────────────────────────────────────

/// One observed record: who emitted it and what exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Object name: list / get / apply / remove / scan / propertySet / layout /
    /// portal / list.model.
    pub q: String,
    pub sig: Sig,
}

impl Record {
    pub fn new(q: &str, sig: Sig) -> Self {
        Record {
            q: q.to_string(),
            sig,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sig {
    // ── Query base ──
    Status(i32),
    Querying,
    ErrorChanged,
    ForwardErrorChanged,
    Finished,
    ErrorOccurred(String),
    /// Forwarding to the global handler. In the golden file it is marked
    /// `q = "app"` (it is sent by the App singleton); the port has no App yet,
    /// and the query itself sends it with the `appErrorOccurred` signal, so the
    /// record is attributed to the query whose `errorOccurred` has just passed.
    /// The order does not change: in the original the forwarding goes right
    /// after its own signal.
    AppError(String),
    /// `<name>Changed()` — the name from a closed list, so that a typo in the
    /// golden file fails the parsing rather than silently losing a signal.
    Prop(&'static str),
    /// Frame out: the token, the payload field and ALL the request fields.
    Sent {
        token: i64,
        field: String,
        request: Value,
    },
    /// One item was removed (the original's `removed`).
    Removed(String),
    /// A set was removed: what was asked for and how many the daemon removed.
    RemovedMany { ids: Vec<String>, count: i32 },
    /// Return value of an invokable (`replaceFilterState`).
    Result { method: String, value: bool },
    /// Reset of the library model — begin/endResetModel.
    ModelReset,
    /// countChanged of the library model.
    ModelCount,
    /// selectionChanged of the model: Python did not have it at all, so it does
    /// not occur in the reference, and any appearance of it is a divergence.
    ModelSelection,
    ModelSelectedCount,
    /// dataChanged for the selection role.
    ModelSelectedRange { first: i32, last: i32 },
}

/// Names of property signals occurring in the reference and in the port.
fn prop_of(sig: &str) -> &'static str {
    match sig {
        "wpTypeChanged" => "wpTypeChanged",
        "searchTextChanged" => "searchTextChanged",
        "sortsChanged" => "sortsChanged",
        "filtersChanged" => "filtersChanged",
        "filterLogicsChanged" => "filterLogicsChanged",
        "skipTypesChanged" => "skipTypesChanged",
        "filterTagsChanged" => "filterTagsChanged",
        "skipContentRatingsChanged" => "skipContentRatingsChanged",
        "hasActiveFiltersChanged" => "hasActiveFiltersChanged",
        "totalChanged" => "totalChanged",
        "filterStateChanged" => "filterStateChanged",
        "itemsChanged" => "itemsChanged",
        "wallpaperIdChanged" => "wallpaperIdChanged",
        "wallpaperChanged" => "wallpaperChanged",
        "targetsChanged" => "targetsChanged",
        "rendererNameChanged" => "rendererNameChanged",
        "rendererIdChanged" => "rendererIdChanged",
        "countChanged" => "countChanged",
        "paramsChanged" => "paramsChanged",
        "propertyKeyChanged" => "propertyKeyChanged",
        "propertyValueChanged" => "propertyValueChanged",
        "resetChanged" => "resetChanged",
        "uriChanged" => "uriChanged",
        other => panic!("голден: неизвестный сигнал запроса {other}"),
    }
}

/// The expected log of a case exactly as Python captured it.
pub fn python_records(case: &Value) -> Vec<Record> {
    let mut out = Vec::new();
    let mut last_error_owner = String::new();
    for event in case
        .get("events")
        .and_then(Value::as_array)
        .expect("голден: у случая нет событий")
    {
        let q = str_at(event, "q");
        let sig = str_at(event, "sig");
        if q == "app" {
            assert_eq!(sig, "errorOccurred", "голден: у App иной сигнал");
            out.push(Record::new(
                &last_error_owner,
                Sig::AppError(str_at(event, "error")),
            ));
            continue;
        }
        if q == "list.model" {
            let sig = match sig.as_str() {
                "modelReset" => Sig::ModelReset,
                "countChanged" => Sig::ModelCount,
                other => panic!("голден: неизвестный сигнал модели {other}"),
            };
            out.push(Record::new("list.model", sig));
            continue;
        }
        let kind = match sig.as_str() {
            "statusChanged" => Sig::Status(i64_at(event, "status") as i32),
            "queryingChanged" => Sig::Querying,
            "errorChanged" => Sig::ErrorChanged,
            "forwardErrorChanged" => Sig::ForwardErrorChanged,
            "finished" => Sig::Finished,
            "errorOccurred" => {
                last_error_owner = q.clone();
                Sig::ErrorOccurred(str_at(event, "error"))
            }
            "sent" => Sig::Sent {
                token: i64_at(event, "token"),
                field: str_at(event, "field"),
                request: event.get("request").cloned().unwrap_or(Value::Null),
            },
            "removed" => Sig::Removed(str_at(event, "wallpaperId")),
            "removedMany" => Sig::RemovedMany {
                ids: strings_at(event, "ids"),
                count: i64_at(event, "count") as i32,
            },
            "result" => Sig::Result {
                method: str_at(event, "method"),
                value: bool_at(event, "value"),
            },
            other => Sig::Prop(prop_of(other)),
        };
        out.push(Record::new(&q, kind));
    }
    out
}

// ── Reference adjustments: deliberate divergences of the port ───────────────

/// One adjustment: how the reference differs from what the port MUST do.
pub struct Fixup {
    /// The golden case it applies to.
    pub case: &'static str,
    /// A short name for the report.
    pub name: &'static str,
    /// Why the reference is not the reference here.
    pub why: &'static str,
}

/// The list of deliberate divergences. Everything not in it is a bug of the
/// port.
///
/// What they all have in common: the golden file was captured from LIVE
/// Python, and on these points Python diverged from the original
/// (`wallpaper_query.cpp`), while the port is made after the ORIGINAL (rule
/// #16 — bugs are fixed during porting, not preserved). The detailed
/// analysis is in the header of `wallpaper_actions.rs`.
pub const FIXUPS: &[Fixup] = &[
    Fixup {
        case: "удаление: один id и пустышка в наборе",
        name: "пустые id отсеиваются, единственный уезжает полем wallpaper_id",
        why: "wallpaper_query.cpp:334 — Python слал пустышку демону и всегда списком",
    },
    Fixup {
        case: "удаление: один id и пустышка в наборе",
        name: "removed(id) для единственного предмета",
        why: "вендорный QML на него подписан (WallpaperDetailPanel.qml:162), у Python он не эмитился никогда",
    },
    Fixup {
        case: "удаление: один id и пустышка в наборе",
        name: "removedMany несёт removed_count ОТВЕТА",
        why: "Python обещал «удалено N» по длине отправленного набора",
    },
    Fixup {
        case: "удаление: отказ демона — removedMany всё равно не летит",
        name: "единственный id уезжает полем wallpaper_id",
        why: "wallpaper_query.cpp:334 — Python всегда слал набор списком",
    },
    Fixup {
        case: "удаление: демон снёс меньше, чем просили",
        name: "removedMany несёт removed_count ОТВЕТА",
        why: "в ответе демона 1, а Python сообщал 2",
    },
    Fixup {
        case: "применение: id обои, экраны и рендерер уходят одним кадром",
        name: "rendererId из ответа демона",
        why: "WallpaperApplyQuery Python выбрасывал ответ (lambda rsp: None), свойство было вечно пустым",
    },
    Fixup {
        case: "свойство обои: value и reset — разные ветки oneof",
        name: "сигналы по одному на свойство",
        why: "у оригинала wallpaperIdChanged/propertyKeyChanged/propertyValueChanged раздельные, Python схлопывал их в paramsChanged",
    },
    Fixup {
        case: "применение: тот же рендерер вторым ответом — сигнал всё равно",
        name: "rendererIdChanged на КАЖДЫЙ ответ демона",
        why: "wallpaper_query.cpp:539 — сигнал безусловный; Python выбрасывал ответ, поэтому в эталоне его нет ни разу",
    },
    Fixup {
        case: "деталь: устаревший ответ не подменяет карточку",
        name: "устаревший ответ не производит НИ ОДНОГО сигнала",
        why: "wallpaper_query.cpp:311 — оригинал сверяет id и выходит; Python сверки не делал и разбирал оба ответа, отсюда лишний wallpaperChanged",
    },
    Fixup {
        case: "удаление: reload() снимает обою из свойства wallpaperId",
        name: "reload() удаления шлёт кадр на wallpaperId",
        why: "у оригинала reload() == remove([wallpaperId]); Python переопределения не сделал, сработала пустая заглушка базы — свойство было мёртвым",
    },
];

/// Golden → the log the port MUST produce.
///
/// Returns the adjusted records and the names of the applied adjustments. An
/// adjustment that changed nothing is an error: it means it has gone stale.
pub fn port_records(case: &Value) -> (Vec<Record>, Vec<&'static str>) {
    let name = str_at(case, "name");
    let mut records = python_records(case);
    let mut applied: Vec<&'static str> = Vec::new();

    match name.as_str() {
        "удаление: один id и пустышка в наборе" => {
            // Frame: the empty id is filtered out, the single one goes in the
            // wallpaper_id field.
            for record in records.iter_mut() {
                if let Sig::Sent { request, .. } = &mut record.sig {
                    request["wallpaper_id"] = Value::String("w1".into());
                    request["wallpaper_ids"] = Value::Array(Vec::new());
                    applied.push(FIXUPS[0].name);
                }
            }
            // Report: the set without the empty one, the number from the daemon's
            // answer.
            for record in records.iter_mut() {
                if let Sig::RemovedMany { ids, count } = &mut record.sig {
                    *ids = vec!["w1".to_string()];
                    *count = 1;
                    applied.push(FIXUPS[2].name);
                }
            }
            // And on top of the set — a signal for the single item.
            let at = records
                .iter()
                .position(|r| matches!(r.sig, Sig::RemovedMany { .. }))
                .expect("голден: в случае нет removedMany");
            records.insert(at, Record::new("remove", Sig::Removed("w1".into())));
            applied.push(FIXUPS[1].name);
        }
        "удаление: отказ демона — removedMany всё равно не летит" => {
            for record in records.iter_mut() {
                if let Sig::Sent { request, .. } = &mut record.sig {
                    request["wallpaper_id"] = Value::String("w1".into());
                    request["wallpaper_ids"] = Value::Array(Vec::new());
                    applied.push(FIXUPS[3].name);
                }
            }
        }
        "удаление: демон снёс меньше, чем просили" => {
            for record in records.iter_mut() {
                if let Sig::RemovedMany { count, .. } = &mut record.sig {
                    *count = 1;
                    applied.push(FIXUPS[4].name);
                }
            }
        }
        "применение: id обои, экраны и рендерер уходят одним кадром" => {
            // The property is updated while parsing the answer, that is, BEFORE the
            // status switches to Finished — as in the original.
            let at = records
                .iter()
                .position(|r| r.q == "apply" && matches!(r.sig, Sig::Status(2)))
                .expect("голден: в случае нет перехода в Finished");
            records.insert(at, Record::new("apply", Sig::Prop("rendererIdChanged")));
            applied.push(FIXUPS[5].name);
        }
        "удаление: reload() снимает обою из свойства wallpaperId" => {
            // The only case where records are APPENDED in full: in Python nothing
            // happened after reload(), while the port must send the same frame as
            // `remove(["w1"])` — with the id in the `wallpaper_id` field, because there
            // is one item.
            assert_eq!(records.len(), 1, "голден: у случая появились свои сигналы");
            records.push(Record::new("remove", Sig::Status(we_query::STATUS_QUERYING)));
            records.push(Record::new("remove", Sig::Querying));
            records.push(Record::new(
                "remove",
                Sig::Sent {
                    token: 1,
                    field: "wallpaper_remove".into(),
                    request: map(vec![
                        ("wallpaper_id", Value::String("w1".into())),
                        ("wallpaper_ids", Value::Array(Vec::new())),
                    ]),
                },
            ));
            applied.push(FIXUPS[9].name);
        }
        "деталь: устаревший ответ не подменяет карточку" => {
            // The port drops a stale answer entirely, so the whole set of signals comes
            // from the FRESH answer — exactly once. Python parsed both, and the second
            // parse appended an extra wallpaperChanged to the tail.
            let last = records.pop().expect("голден: случай пуст");
            assert_eq!(
                last,
                Record::new("get", Sig::Prop("wallpaperChanged")),
                "голден: в хвосте случая не разбор устаревшего ответа"
            );
            applied.push(FIXUPS[8].name);
        }
        "применение: тот же рендерер вторым ответом — сигнал всё равно" => {
            // The signal fires on EVERY daemon answer, even when the renderer is the
            // same: it means "the daemon answered". The place is the same as in the
            // case above — before switching to Finished.
            let at: Vec<usize> = records
                .iter()
                .enumerate()
                .filter(|(_, r)| r.q == "apply" && matches!(r.sig, Sig::Status(2)))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(at.len(), 2, "голден: ответов демона стало {}", at.len());
            for (shift, i) in at.into_iter().enumerate() {
                records.insert(i + shift, Record::new("apply", Sig::Prop("rendererIdChanged")));
            }
            // One adjustment per case, not per insertion: it is about the RULE "the
            // signal is unconditional", not about each of its firings.
            applied.push(FIXUPS[7].name);
        }
        "свойство обои: value и reset — разные ветки oneof" => {
            // Order of the case's edits: wallpaperId, propertyKey, propertyValue,
            // propertyValue (to empty), reset.
            const NAMES: &[&str] = &[
                "wallpaperIdChanged",
                "propertyKeyChanged",
                "propertyValueChanged",
                "propertyValueChanged",
                "resetChanged",
            ];
            let mut seen = 0usize;
            for record in records.iter_mut() {
                if record.q == "propertySet" && record.sig == Sig::Prop("paramsChanged") {
                    record.sig = Sig::Prop(NAMES[seen]);
                    seen += 1;
                }
            }
            assert_eq!(seen, NAMES.len(), "голден: правок свойства стало {seen}");
            applied.push(FIXUPS[6].name);
        }
        _ => {}
    }
    (records, applied)
}

/// Adjustments of FINAL properties: the same divergence, but visible not in
/// the log but in the property value at the end of the case.
pub const FINAL_FIXUPS: &[Fixup] = &[
    Fixup {
        case: "применение: id обои, экраны и рендерер уходят одним кадром",
        name: "rendererId из ответа демона (свойство)",
        why: "Python выбрасывал ответ, поэтому в эталоне свойство пустое",
    },
    Fixup {
        case: "применение: тот же рендерер вторым ответом — сигнал всё равно",
        name: "rendererId из ответа демона (свойство, повторный ответ)",
        why: "тот же выброшенный ответ: в эталоне свойство пустое после обоих ответов",
    },
    Fixup {
        case: "удаление: reload() снимает обою из свойства wallpaperId",
        name: "запрос удаления остался в Querying (свойство)",
        why: "кадр ушёл и ответа ещё нет; у Python reload() был пуст, поэтому в эталоне статус нулевой",
    },
];

/// The final state of a query as the port MUST show it.
pub fn port_final(case: &Value, q: &str, applied: &mut Vec<&'static str>) -> Value {
    let mut want = final_of(case, q).clone();
    match (str_at(case, "name").as_str(), q) {
        ("применение: id обои, экраны и рендерер уходят одним кадром", "apply") => {
            want["rendererId"] = Value::String("r-7".into());
            applied.push(FINAL_FIXUPS[0].name);
        }
        ("применение: тот же рендерер вторым ответом — сигнал всё равно", "apply") => {
            want["rendererId"] = Value::String("r-7".into());
            applied.push(FINAL_FIXUPS[1].name);
        }
        ("удаление: reload() снимает обою из свойства wallpaperId", "remove") => {
            want["status"] = Value::from(we_query::STATUS_QUERYING);
            applied.push(FINAL_FIXUPS[2].name);
        }
        _ => {}
    }
    want
}

/// How many adjustments must apply over the whole golden file — a safeguard
/// against a stale allowance: the divergence was fixed in the port, but the
/// adjustment was not removed.
pub fn expected_fixups() -> usize {
    FIXUPS.len()
}

// ── Daemon frames: a description from the scenario → real protobuf ────────

fn status_of(name: &str) -> i32 {
    match name {
        "OK" => pb::Status::Ok as i32,
        "INVALID_ARGUMENT" => pb::Status::InvalidArgument as i32,
        "NOT_FOUND" => pb::Status::NotFound as i32,
        "INTERNAL" => pb::Status::Internal as i32,
        "FAILED_PRECONDITION" => pb::Status::FailedPrecondition as i32,
        other => panic!("голден: неизвестный статус ответа {other}"),
    }
}

fn layout_of(value: &Value) -> pb::LayoutPrefs {
    pb::LayoutPrefs {
        fillmode: i64_at(value, "fillmode") as i32,
        align: i64_at(value, "align") as i32,
        rotation: i64_at(value, "rotation") as i32,
        location_x: i64_at(value, "locationX") as u32,
        location_y: i64_at(value, "locationY") as u32,
        location_set: bool_at(value, "locationSet"),
        // waywallen 0.4.4: the scenarios have no flip.
        flip: 0,
    }
}

/// A library record from the scenario. A mirror of the generator's
/// `_fill_entry`: both sides build THE SAME protobuf from one description,
/// so the reference does not depend on who reads it.
fn entry_of(value: &Value) -> pb::WallpaperEntry {
    let metadata = value
        .get("metadata")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    pb::WallpaperEntry {
        id: str_at(value, "id"),
        name: str_at(value, "name"),
        wp_type: str_at(value, "wpType"),
        resource: str_at(value, "resource"),
        preview: str_at(value, "preview"),
        metadata,
        size: i64_at(value, "size"),
        width: i64_at(value, "width") as u32,
        height: i64_at(value, "height") as u32,
        content_rating: str_at(value, "contentRating"),
        web_url: str_at(value, "webUrl"),
        tags: strings_at(value, "tags"),
        user_properties_schema: str_at(value, "userPropertiesSchema"),
        user_property_overrides: str_at(value, "userPropertyOverrides"),
        description: str_at(value, "description"),
        external_id: str_at(value, "externalId"),
        wallpaper_layout_override: value
            .get("wallpaperLayoutOverride")
            .filter(|v| !v.is_null())
            .map(layout_of),
        wallpaper_layout_override_set: bool_at(value, "wallpaperLayoutOverrideSet"),
        supports_item_remove: bool_at(value, "supportsItemRemove"),
        supports_item_unsubscribe: bool_at(value, "supportsItemUnsubscribe"),
        // waywallen 0.4.4: the scenarios have no hidden records.
        hidden: false,
        // waywallen 0.6.0: the scenarios were captured earlier — ordinary records.
        update_available: false,
        sub_type: 0,
    }
}

fn payload_of(payload: &Value) -> pb::response::Payload {
    match str_at(payload, "kind").as_str() {
        "wallpaper_list" => pb::response::Payload::WallpaperList(pb::WallpaperListResponse {
            wallpapers: array_at(payload, "wallpapers").iter().map(entry_of).collect(),
            count: i64_at(payload, "count") as u32,
        }),
        "wallpaper_get" => pb::response::Payload::WallpaperGet(pb::WallpaperGetResponse {
            entry: payload
                .get("entry")
                .filter(|v| !v.is_null())
                .map(entry_of),
        }),
        "wallpaper_layout_set" => {
            pb::response::Payload::WallpaperLayoutSet(pb::WallpaperLayoutSetResponse {
                entry: payload
                    .get("entry")
                    .filter(|v| !v.is_null())
                    .map(entry_of),
            })
        }
        "wallpaper_scan" => pb::response::Payload::WallpaperScan(pb::WallpaperScanResponse {
            count: i64_at(payload, "count") as u32,
        }),
        "wallpaper_apply" => pb::response::Payload::WallpaperApply(pb::WallpaperApplyResponse {
            renderer_id: str_at(payload, "rendererId"),
            wallpaper_id: str_at(payload, "wallpaperId"),
            wp_type: str_at(payload, "wpType"),
            name: str_at(payload, "name"),
            ..Default::default()
        }),
        "wallpaper_remove" => pb::response::Payload::WallpaperRemove(pb::WallpaperRemoveResponse {
            removed_count: i64_at(payload, "removedCount") as u32,
        }),
        "wallpaper_property_set" => pb::response::Payload::WallpaperPropertySet(
            pb::WallpaperPropertySetResponse::default(),
        ),
        "wallpaper_apply_via_portal" => pb::response::Payload::WallpaperApplyViaPortal(
            pb::WallpaperApplyViaPortalResponse {
                wallpaper_id: str_at(payload, "wallpaperId"),
                uri: str_at(payload, "uri"),
            },
        ),
        other => panic!("голден: неизвестная полезная нагрузка {other}"),
    }
}

/// A "respond"/"respondError" step → a raw `ServerFrame`, exactly the one
/// the client hands to the core with `want_frame`.
pub fn build_frame(step: &Value) -> Vec<u8> {
    let mut rsp = pb::Response {
        status: pb::Status::Ok as i32,
        ..Default::default()
    };
    if let Some(status) = step.get("status").and_then(Value::as_str) {
        rsp.status = status_of(status);
        rsp.message = str_at(step, "message");
    }
    if let Some(payload) = step.get("payload").filter(|v| !v.is_null()) {
        rsp.payload = Some(payload_of(payload));
    }
    pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(rsp)),
    }
    .encode_to_vec()
}

// ── Request frame → JSON in the same form the golden file prints ─────────

fn map(pairs: Vec<(&str, Value)>) -> Value {
    let mut out = Map::new();
    for (key, value) in pairs {
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

fn strings(list: &[String]) -> Value {
    Value::Array(list.iter().map(|s| Value::String(s.clone())).collect())
}

/// Apply targets of v0.3.5 in the golden file: a map per target, BOTH
/// branches of the `oneof`.
///
/// The golden file prints `_msg_fields`
/// (tests/golden/wallpaper_page_cases.py:663) — it walks the DESCRIPTOR and
/// lays out every field, including the unset `oneof` branch with its
/// default value. The neighbours in this same file are recorded the same
/// way: `wallpaper_property_set` carries both `value` and `reset`, a filter
/// rule all three `payload` branches. So a display target looks like
/// `{display_id: 1, canvas_id: ""}` in the reference, and a canvas target
/// like `{display_id: 0, canvas_id: "cnv"}`.
///
/// Warning: this is only the form of the RECORD. On the wire there is exactly
/// one branch: a `oneof` cannot hold both, and assigning `canvas_id` would
/// erase `display_id`. The frame is built by `we_manage::presentation_targets`
/// (a port of the vendor's `presentationTargetsFromVariant`,
/// ui/src/proto.cpp:8 at f7baecd), and the bench compares the bytes — while
/// a human-readable snapshot is printed here.
fn targets(list: &[pb::PresentationTarget]) -> Value {
    Value::Array(
        list.iter()
            .map(|t| {
                let (display_id, canvas_id) = match &t.target {
                    Some(pb::presentation_target::Target::DisplayId(id)) => (*id, String::new()),
                    Some(pb::presentation_target::Target::CanvasId(id)) => (0, id.clone()),
                    None => (0, String::new()),
                };
                map(vec![
                    ("display_id", Value::from(display_id)),
                    ("canvas_id", Value::String(canvas_id)),
                ])
            })
            .collect(),
    )
}

fn layout_json(l: &pb::LayoutPrefs) -> Value {
    map(vec![
        ("fillmode", Value::from(l.fillmode)),
        ("align", Value::from(l.align)),
        ("rotation", Value::from(l.rotation)),
        ("location_x", Value::from(l.location_x)),
        ("location_y", Value::from(l.location_y)),
        ("location_set", Value::from(l.location_set)),
    ])
}

fn filter_rule_json(r: &pb::WallpaperFilterRule) -> Value {
    use pb::wallpaper_filter_rule::Payload;
    let (string_filter, int_filter, tag_filter) = match &r.payload {
        Some(Payload::StringFilter(f)) => (
            map(vec![
                ("value", Value::String(f.value.clone())),
                ("condition", Value::from(f.condition)),
            ]),
            Value::Null,
            Value::Null,
        ),
        Some(Payload::IntFilter(f)) => (
            Value::Null,
            map(vec![
                ("value", Value::from(f.value)),
                ("condition", Value::from(f.condition)),
            ]),
            Value::Null,
        ),
        Some(Payload::TagFilter(f)) => (
            Value::Null,
            Value::Null,
            map(vec![
                ("values", strings(&f.values)),
                ("condition", Value::from(f.condition)),
            ]),
        ),
        None => (Value::Null, Value::Null, Value::Null),
    };
    map(vec![
        ("type", Value::from(r.r#type)),
        ("group", Value::from(r.group)),
        ("string_filter", string_filter),
        ("int_filter", int_filter),
        ("tag_filter", tag_filter),
    ])
}

/// Request frame → a map of ALL fields, including default values.
///
/// A mirror of the generator's `_msg_fields`: empty lists, zeros and `false`
/// must be visible — that is exactly where lost fields are caught. The only
/// exception is a single nested message: what matters there is the very
/// fact of its presence (the layout body must NOT be sent on a reset), so
/// an unset one gives `null`, not a map of zeros.
///
/// It is EXACTLY what went out as bytes that is parsed: the intention to send
/// a frame does not count as a check.
pub fn request_json(frame: &[u8]) -> (String, Value) {
    let request = pb::Request::decode(frame).expect("кадр запроса разбирается");
    let payload = request.payload.expect("кадр запроса без полезной нагрузки");
    match payload {
        pb::request::Payload::WallpaperList(r) => (
            "wallpaper_list".into(),
            map(vec![
                ("wp_type", Value::String(r.wp_type)),
                ("page", Value::from(r.page)),
                ("page_size", Value::from(r.page_size)),
                (
                    "filters",
                    Value::Array(r.filters.iter().map(filter_rule_json).collect()),
                ),
                (
                    "filter_logics",
                    Value::Array(
                        r.filter_logics
                            .iter()
                            .map(|l| {
                                map(vec![
                                    ("op", Value::from(l.op)),
                                    ("group_a", Value::from(l.group_a)),
                                    ("group_b", Value::from(l.group_b)),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "sorts",
                    Value::Array(
                        r.sorts
                            .iter()
                            .map(|s| {
                                map(vec![
                                    ("key", Value::from(s.key)),
                                    ("direction", Value::from(s.direction)),
                                ])
                            })
                            .collect(),
                    ),
                ),
                ("search_text", Value::String(r.search_text)),
                ("skip_types", strings(&r.skip_types)),
                ("filter_tags", strings(&r.filter_tags)),
                ("skip_content_ratings", strings(&r.skip_content_ratings)),
            ]),
        ),
        pb::request::Payload::WallpaperGet(r) => (
            "wallpaper_get".into(),
            map(vec![("wallpaper_id", Value::String(r.wallpaper_id))]),
        ),
        pb::request::Payload::WallpaperApply(r) => (
            "wallpaper_apply".into(),
            map(vec![
                (
                    "wallpaper_id",
                    Value::String(match r.wallpaper {
                        Some(pb::wallpaper_apply_request::Wallpaper::WallpaperId(id)) => id,
                        _ => String::new(),
                    }),
                ),
                ("targets", targets(&r.targets)),
                ("renderer_name", Value::String(r.renderer_name)),
            ]),
        ),
        pb::request::Payload::WallpaperRemove(r) => (
            "wallpaper_remove".into(),
            map(vec![
                ("wallpaper_id", Value::String(r.wallpaper_id)),
                ("wallpaper_ids", strings(&r.wallpaper_ids)),
            ]),
        ),
        pb::request::Payload::WallpaperScan(_) => ("wallpaper_scan".into(), map(Vec::new())),
        pb::request::Payload::WallpaperPropertySet(r) => {
            use pb::wallpaper_property_set_request::Operation;
            let (value, reset) = match r.operation {
                Some(Operation::Value(v)) => (v, false),
                Some(Operation::Reset(v)) => (String::new(), v),
                None => (String::new(), false),
            };
            (
                "wallpaper_property_set".into(),
                map(vec![
                    ("wallpaper_id", Value::String(r.wallpaper_id)),
                    ("key", Value::String(r.key)),
                    ("value", Value::String(value)),
                    ("reset", Value::from(reset)),
                ]),
            )
        }
        pb::request::Payload::WallpaperLayoutSet(r) => (
            "wallpaper_layout_set".into(),
            map(vec![
                ("wallpaper_id", Value::String(r.wallpaper_id)),
                ("clear", Value::from(r.clear)),
                (
                    "layout",
                    r.layout.as_ref().map(layout_json).unwrap_or(Value::Null),
                ),
            ]),
        ),
        pb::request::Payload::WallpaperApplyViaPortal(r) => (
            "wallpaper_apply_via_portal".into(),
            map(vec![("wallpaper_id", Value::String(r.wallpaper_id))]),
        ),
        other => panic!("голден: кадр не со страницы «Обои»: {other:?}"),
    }
}

// ── Replay with the PURE layer: the list query and its model ───────────────

/// The pure layer bench. Only the decisions computed by the core live here,
/// plus the dispatch rules written in `wallpaper_queries.rs`: a reload by the
/// list [`crate::wallpaper_queries::LIST_RELOAD_ON`] and rebuilding the model
/// BEFORE `itemsChanged`.
#[derive(Default)]
pub struct PureHarness {
    pub base: QueryState,
    pub data: we_wallpaper::WallpaperListData,
    pub rows: WallpaperRows,
    pub journal: Vec<Record>,
    pending: std::collections::BTreeSet<i64>,
    reload_pending: bool,
}

impl PureHarness {
    pub fn new() -> Self {
        PureHarness {
            rows: WallpaperRows::new(),
            ..Default::default()
        }
    }

    fn dispatch(&mut self, signals: Vec<QuerySignal>) {
        for signal in signals {
            let sig = match signal {
                QuerySignal::StatusChanged(v) => Sig::Status(v),
                QuerySignal::QueryingChanged => Sig::Querying,
                QuerySignal::ErrorChanged => Sig::ErrorChanged,
                QuerySignal::ForwardErrorChanged => Sig::ForwardErrorChanged,
                QuerySignal::Finished => Sig::Finished,
                QuerySignal::ErrorOccurred(text) => Sig::ErrorOccurred(text),
                QuerySignal::AppErrorOccurred(text) => Sig::AppError(text),
                QuerySignal::Property(name) => {
                    if crate::wallpaper_queries::LIST_RELOAD_ON.contains(&name) {
                        self.reload_pending = true;
                        // `invalidateList`, as in the bridge: a new generation — the answer to the
                        // request in flight is no longer ours.
                        self.pending.clear();
                        let _ = we_wallpaper::invalidate_list(&mut self.data);
                    }
                    // waywallen 0.5.0: selection and pages (`selection`, `offset`, `noMore`,
                    // `limit`) — these properties did not exist in the reference's era, and
                    // the live bench does not listen to them. They are guarded by core unit
                    // tests and the QML probe (`qml_int64_probe.rs`).
                    if matches!(name, "selection" | "offset" | "noMore" | "limit") {
                        continue;
                    }
                    // waywallen 0.5.0: `hasOtherFiltersChanged` is unknown to the reference of
                    // the Python era (the property did not exist), and the live bench does not
                    // listen to it. The signal's order and condition are guarded by core unit
                    // tests (`tests/we_wallpaper_parity.rs`); in a reference without hidden
                    // libraries it goes exactly in a pair with `hasActiveFiltersChanged`, so
                    // skipping it hides nothing.
                    if name == "hasOtherFilters" {
                        continue;
                    }
                    if name == "items" {
                        // The model is rebuilt FIRST: in the bridge the rebuild was attached by the
                        // query constructor, that is, it stood first in the chain of subscribers,
                        // and QML on onItemsChanged must read the ALREADY new rows.
                        let events = self.rows.reset_rows(self.data.items.clone());
                        self.record_model(events);
                        Sig::Prop("itemsChanged")
                    } else {
                        Sig::Prop(prop_of(&format!("{name}Changed")))
                    }
                }
            };
            self.journal.push(Record::new("list", sig));
        }
    }

    fn record_model(&mut self, events: Vec<ModelEvent>) {
        for event in events {
            let sig = match event {
                ModelEvent::Reset => Sig::ModelReset,
                ModelEvent::CountChanged => Sig::ModelCount,
                ModelEvent::SelectionChanged => Sig::ModelSelection,
                ModelEvent::SelectedCountChanged => Sig::ModelSelectedCount,
                ModelEvent::SelectedRange { first, last } => {
                    Sig::ModelSelectedRange { first, last }
                }
            };
            self.journal.push(Record::new("list.model", sig));
        }
    }

    fn reload(&mut self) {
        // A new generation and resetting the pages — as in the bridge's `reload`.
        self.pending.clear();
        let signals = we_wallpaper::begin_reload(&mut self.data);
        self.dispatch(signals);
        let connected = crate::discover_queries::connected();
        let (send, signals) = we_query::begin_call(&mut self.base, connected);
        self.dispatch(signals);
        if !send {
            return;
        }
        let payload = we_wallpaper::encode_list_request(&self.data);
        // The number is issued BEFORE the frame is built — the replica must repeat
        // the product's path, and there `request_id` is now one and lies inside the
        // frame.
        let token = crate::discover_queries::next_token();
        let bytes =
            rexon_core::we_client::request_bytes(token as u64, "wallpaper_list", &payload)
                .expect("поле wallpaper_list известно протоколу");
        self.pending.insert(token);
        let (field, request) = request_json(&bytes);
        self.journal.push(Record::new(
            "list",
            Sig::Sent {
                token,
                field,
                request,
            },
        ));
    }

    /// The debounce fired: property edits are glued into ONE reload.
    fn settle(&mut self) {
        if !std::mem::take(&mut self.reload_pending) {
            return;
        }
        self.reload();
    }

    pub fn step(&mut self, step: &Value) {
        match str_at(step, "op").as_str() {
            "set" if str_at(step, "q") == "list" => {
                let name = str_at(step, "name");
                let value = step.get("value").cloned().unwrap_or(Value::Null);
                if name == "forwardError" {
                    let signals =
                        we_query::set_forward_error(&mut self.base, value.as_bool().unwrap_or(false));
                    self.dispatch(signals);
                    return;
                }
                let signals = we_wallpaper::set_property(&mut self.data, &name, &value)
                    .unwrap_or_else(|| panic!("голден: нет свойства list.{name}"));
                self.dispatch(signals);
            }
            "call" if str_at(step, "q") == "list" => match str_at(step, "method").as_str() {
                "reload" => self.reload(),
                "replaceFilterState" => {
                    let filters = step.get("filters").cloned().unwrap_or(Value::Null);
                    let logics = step.get("logics").cloned().unwrap_or(Value::Null);
                    let (changed, signals) =
                        we_wallpaper::replace_filter_state(&mut self.data, &filters, &logics);
                    self.dispatch(signals);
                    self.journal.push(Record::new(
                        "list",
                        Sig::Result {
                            method: "replaceFilterState".into(),
                            value: changed,
                        },
                    ));
                }
                other => panic!("голден: нет метода list.{other}"),
            },
            "settle" => self.settle(),
            "respond" | "respondError" => {
                let token = i64_at(step, "token");
                if !self.pending.remove(&token) {
                    // The frame is not ours: an action query takes part in the case.
                    return;
                }
                let frame = build_frame(step);
                let signals =
                    we_wallpaper::apply_wallpaper_list(&mut self.base, &mut self.data, &frame)
                        .expect("ответ демона разбирается");
                self.dispatch(signals);
            }
            "connected" => crate::discover_queries::set_connected(bool_at(step, "value")),
            // Steps of the page's other queries are of no interest to the pure layer:
            // their logic lives in Qt objects and is checked by the bench.
            _ => {}
        }
    }
}

/// Run a case through the PURE layer of the list query.
pub fn replay_pure(case: &Value) -> PureHarness {
    crate::discover_queries::reset_tokens();
    crate::discover_queries::set_connected(true);
    let mut harness = PureHarness::new();
    for step in case
        .get("steps")
        .and_then(Value::as_array)
        .expect("голден: у случая нет шагов")
    {
        harness.step(step);
    }
    // At the end of a case the generator spins the event loop once more: an
    // unclosed debounce must fire there too.
    harness.settle();
    harness
}

/// Rows of the model by ROLES in the same JSON form the golden file prints.
///
/// The `selected` role is deliberately excluded from the comparison: the
/// Python model did not have it at all (the header of
/// `wallpaper_list_model.rs`, item 1), so it is not and cannot be in the
/// reference. Its presence is checked separately.
pub fn rows_json(rows: &[VMap]) -> Value {
    Value::Array(rows.iter().map(|m| Value::Object(m.clone())).collect())
}

/// The expected golden rows, completed with the waywallen 0.4.4 fields.
///
/// The golden file was captured from live Python before v0.4.4, and two new
/// record keys are not in it: `hidden` (the wallpaper is hidden) and `flip`
/// inside `wallpaperLayoutOverride` (the flip). The golden scenarios have
/// neither hidden wallpapers nor flips (`entry_of` sets `hidden: false`,
/// `flip: 0`), so exactly these defaults are added — the rest is compared as
/// before.
/// One expected record (the details panel) — the same completion as for
/// [`expected_rows_v044`]. An empty map ("no record") stays empty.
pub fn expected_entry_v044(entry: &Value) -> Value {
    match entry {
        Value::Object(map) if map.contains_key("id_proto") => {
            expected_rows_v044(&Value::Array(vec![entry.clone()]))[0].clone()
        }
        _ => entry.clone(),
    }
}

pub fn expected_rows_v044(rows: &Value) -> Value {
    let Value::Array(items) = rows else {
        return rows.clone();
    };
    Value::Array(
        items
            .iter()
            .map(|row| {
                let mut row = row.clone();
                if let Value::Object(map) = &mut row {
                    map.entry("hidden").or_insert(Value::Bool(false));
                    // waywallen 0.6.0: added at the end in the same way as `hidden`.
                    map.entry("updateAvailable").or_insert(Value::Bool(false));
                    map.entry("subType").or_insert(Value::from(0));
                    if let Some(Value::Object(layout)) = map.get_mut("wallpaperLayoutOverride") {
                        layout.entry("flip").or_insert(Value::from(0));
                    }
                }
                row
            })
            .collect(),
    )
}

/// Keys of a library record and their ORDER — as the golden file prints them
/// (the bridge's `_WALLPAPER_KEYS`).
///
/// The order here is not cosmetic: the model's roles are counted from it
/// (`FIRST_ENTRY_ROLE + index`), and swapping two keys silently shifts the
/// role numbering — comparing rows by NAMES does not see that.
///
/// The golden file was captured before waywallen 0.4.4, and the `hidden` key
/// is not in it: it was added at the END of the list (the roles of the
/// previous keys did not shift) — and that is how we add it to the
/// reference (see [`expected_rows_v044`]).
pub fn entry_keys(doc: &Value) -> Vec<String> {
    let mut keys = strings_at(doc, "ключи_записи");
    // waywallen 0.6.0: `updateAvailable` and `subType` — also at the end.
    for key in ["hidden", "updateAvailable", "subType"] {
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
        }
    }
    keys
}

/// The expected final state of one query from the golden file.
pub fn final_of<'a>(case: &'a Value, q: &str) -> &'a Value {
    case.get("final")
        .and_then(|f| f.get(q))
        .unwrap_or_else(|| panic!("голден: нет итогового состояния {q}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden file was captured from the LIVE Python queries of the
    /// "Wallpapers" page. Here half of it is replayed through the pure layer —
    /// the library query and its model: which frame goes to the daemon (with all
    /// fields), in what order the signals of the query and the model fire, and
    /// what the properties and rows turn out to be.
    ///
    /// The other half (seven action queries) lives in Qt objects and has no pure
    /// layer — it is checked by the `wallpaper_page_selftest` bench.
    ///
    /// One test for everything rather than thirty-three: the pure layer uses the
    /// token counter shared by the bridge (in Python it was a module variable),
    /// and two tests running in parallel would jostle for it.
    #[test]
    fn golden_parity_with_python_list_query() {
        let _tokens = crate::discover_queries::token_test_lock();
        // The language is the source one: since 30.09 error texts and source labels
        // are catalogue msgids (Russian comes as a translation), while the process
        // language is shared and the crate's neighbours set exactly the source
        // one. The reference stores English msgids where the Python era wrote
        // Russian.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let doc = load();
        let cases = doc["cases"].as_array().expect("голден: cases");
        assert!(!cases.is_empty(), "голден пуст");

        // Record keys and their order — the role numbers are counted from them.
        assert_eq!(
            crate::wallpaper_list_model::ENTRY_KEYS.to_vec(),
            entry_keys(&doc),
            "порядок ключей записи разошёлся с эталоном — поехали номера ролей"
        );

        let mut records = 0usize;
        for case in cases {
            let name = str_at(case, "name");
            let harness = replay_pure(case);
            // The expectation is only the records of the list query and its model.
            let (expected, _) = port_records(case);
            let expected: Vec<Record> = expected
                .into_iter()
                .filter(|r| r.q == "list" || r.q == "list.model")
                .collect();
            assert_eq!(harness.journal, expected, "{name}: последовательность сигналов");
            records += harness.journal.len();

            let want = final_of(case, "list");
            assert_eq!(
                harness.base.status as i64,
                i64_at(want, "status"),
                "{name}: статус списка"
            );
            assert_eq!(harness.base.error, str_at(want, "error"), "{name}: ошибка списка");
            assert_eq!(
                harness.base.forward_error,
                bool_at(want, "forwardError"),
                "{name}: forwardError списка"
            );
            assert_eq!(harness.data.wp_type, str_at(want, "wpType"), "{name}: тип обоев");
            assert_eq!(
                harness.data.search_text,
                str_at(want, "searchText"),
                "{name}: строка поиска"
            );
            assert_eq!(
                harness.data.skip_types,
                strings_at(want, "skipTypes"),
                "{name}: скрытые типы"
            );
            assert_eq!(
                harness.data.filter_tags,
                strings_at(want, "filterTags"),
                "{name}: теги фильтра"
            );
            assert_eq!(
                harness.data.skip_content_ratings,
                strings_at(want, "skipContentRatings"),
                "{name}: скрытые рейтинги"
            );
            assert_eq!(
                harness.data.has_active_filters(),
                bool_at(want, "hasActiveFilters"),
                "{name}: признак «фильтры включены»"
            );
            assert_eq!(
                harness.data.total as i64,
                i64_at(want, "total"),
                "{name}: total"
            );
            assert_eq!(
                harness.rows.len() as i64,
                i64_at(want, "count"),
                "{name}: строк в модели"
            );
            assert_eq!(
                rows_json(harness.rows.rows()),
                expected_rows_v044(&want["rows"]),
                "{name}: строки модели"
            );
        }
        // A safeguard against "compared emptiness with emptiness".
        assert!(
            records > 200,
            "проиграно всего {records} сигналов — эталон подозрительно пуст"
        );
    }

    /// Reference adjustments must be LIVE: each must fire exactly where it is
    /// declared. An adjustment that changes nothing means the divergence was
    /// fixed but the allowance was not removed.
    #[test]
    fn deliberate_fixups_are_all_used() {
        let doc = load();
        let mut applied = Vec::new();
        for case in doc["cases"].as_array().expect("голден: cases") {
            let (_, names) = port_records(case);
            applied.extend(names);
        }
        assert_eq!(
            applied.len(),
            expected_fixups(),
            "правок применилось {}, а объявлено {}: {applied:?}",
            applied.len(),
            expected_fixups()
        );
    }
}
