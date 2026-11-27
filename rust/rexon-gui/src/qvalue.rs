// Converting values between the core's JSON and the QVariant that QML
// lives on.
//
// The module is shared across the whole crate on purpose (rule #12 — one
// engine, not a copy in every bridge): the same core maps are read by the
// result models, the wallpaper library model and the queries. Diverging
// copies of the converter would behave differently on the same value, and
// that would show up not at build time but in the interface.

use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use serde_json::Value;

/// JSON → QVariant. Nested objects become QVariantMap, arrays QVariantList:
/// exactly what the vendored QML reads with a dot (`source.sorts[0].key`).
pub fn json_to_variant(value: &Value) -> QVariant {
    match value {
        Value::Null => QVariant::default(),
        Value::Bool(b) => QVariant::from(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                // i32 is enough for all protocol fields, and QML reads it as a number
                // without loss.
                match i32::try_from(i) {
                    Ok(v) => QVariant::from(&v),
                    Err(_) => QVariant::from(&i),
                }
            } else {
                QVariant::from(&n.as_f64().unwrap_or_default())
            }
        }
        Value::String(s) => QVariant::from(&QString::from(s)),
        Value::Array(items) => {
            let mut list = QList::<QVariant>::default();
            for item in items {
                list.append(json_to_variant(item));
            }
            QVariant::from(&list)
        }
        Value::Object(map) => {
            let mut out = QMap::<QMapPair_QString_QVariant>::default();
            for (key, item) in map {
                out.insert(QString::from(key), json_to_variant(item));
            }
            QVariant::from(&out)
        }
    }
}

/// QVariant → JSON.
///
/// It is parsed not by Rust but by Qt: `QVariant::value<T>()` in cxx-qt-lib
/// checks `canConvert`, and in Qt that is permissive — a string "converts"
/// both to bool and to a character list, so the REAL type of the value
/// cannot be determined with it. `QJsonDocument::fromVariant` does it by
/// Qt's own rules (the `cpp/rexon_gui_factory.cpp` shim).
///
/// Warning: the declaration of the C++ function lives in the batch 3 bridge
/// and is reused from here: a second declaration of THE SAME function in
/// another bridge of the crate gives a `duplicate symbol` at link time (cxx
/// emits the shim by the C++ function name).
pub fn variant_to_json(value: &QVariant) -> Value {
    let bytes = crate::discover_queries::qobject::rexon_variant_to_json(value);
    serde_json::from_slice(bytes.as_slice()).unwrap_or(Value::Null)
}

// ════════════════════════════════════════════════════════════════════════
// 64-bit integers exposed to QML
// ════════════════════════════════════════════════════════════════════════
//
// Warning: WHY THESE FOUR FUNCTIONS. cxx-qt emits `i64`/`u64` as
// `::std::int64_t` / `::std::uint64_t`, and on 64-bit Linux these are
// `long` / `unsigned long` — SEPARATE Qt metatypes, not the same as
// `qlonglong`. The QML engine does not know them, and a literal
// declarative binding to such a property ("displayId: 7") FAILS: "Cannot
// assign value 7 to property displayId", and with it the whole document —
// the page is not created at all. Measured on a live engine on 31.08.2026;
// the analysis and a reproducible test bench are in
// `src/qml_int64_probe.rs`.
//
// So a 64-bit integer is NEVER exposed to QML: a property, an invokable
// parameter and a signal parameter are declared `f64` (in QML — `double`),
// while the field inside stays an exact `i64`/`u64`. The vendored QML
// itself holds these values the same way: `required property double
// playlistId`.
//
// Warning: THE PRECISION LIMIT: `double` holds integers without loss up to
// 2^53 (9 007 199 254 740 992). All values that travel this path are row
// numbers of the daemon's database, edit counters and process generations,
// cache sizes in bytes: they are far from the ceiling. A new value that
// needs all 64 bits (for example, a Steam Workshop identifier or a
// nanosecond timestamp) must NOT be exposed this way — its place is a
// `QString`. The `qml_int64` guard in `src/lib.rs` will not let such a
// property be added silently.

/// A signed integer exposed to QML.
pub fn i64_to_qml(value: i64) -> f64 {
    value as f64
}

/// An unsigned integer exposed to QML.
pub fn u64_to_qml(value: u64) -> f64 {
    value as f64
}

/// Back: a number from QML into an exact signed integer. The fractional
/// part is dropped — QML assigns its `double`, and the number is an
/// integer.
pub fn qml_to_i64(value: f64) -> i64 {
    value as i64
}

/// Back: a number from QML into an exact unsigned integer.
pub fn qml_to_u64(value: f64) -> u64 {
    value as u64
}
