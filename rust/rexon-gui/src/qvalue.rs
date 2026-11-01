// Перевод значений между JSON ядра и QVariant, которым живёт QML.
//
// Модуль общий на весь крейт намеренно (правило #12 — движок один, а не
// копия в каждом мосте): одни и те же карты ядра читают и модели выдачи, и
// модель библиотеки обоев, и запросы. Разъехавшиеся копии конвертера дали бы
// разное поведение на одном и том же значении, и заметно это стало бы не на
// сборке, а в интерфейсе.

use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use serde_json::Value;

/// JSON → QVariant. Вложенные объекты уезжают QVariantMap, массивы —
/// QVariantList: ровно то, что вендорный QML читает точкой
/// (`source.sorts[0].key`).
pub fn json_to_variant(value: &Value) -> QVariant {
    match value {
        Value::Null => QVariant::default(),
        Value::Bool(b) => QVariant::from(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                // i32 хватает всем полям протокола, а QML читает его как
                // число без потерь.
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
/// Разбирает не Rust, а Qt: `QVariant::value<T>()` в cxx-qt-lib проверяет
/// `canConvert`, а он у Qt разрешающий — строка «конвертируется» и в bool, и
/// в список посимвольно, поэтому определить НАСТОЯЩИЙ тип значения им
/// нельзя. `QJsonDocument::fromVariant` делает это правилами самого Qt
/// (шим `cpp/rexon_gui_factory.cpp`).
///
/// ⚠️ Объявление C++-функции живёт в мосте партии 3 и переиспользуется
/// отсюда: второе объявление ТОЙ ЖЕ функции в другом мосте крейта даёт
/// `duplicate symbol` на линковке (cxx печатает шим по имени C++-функции).
pub fn variant_to_json(value: &QVariant) -> Value {
    let bytes = crate::discover_queries::qobject::rexon_variant_to_json(value);
    serde_json::from_slice(bytes.as_slice()).unwrap_or(Value::Null)
}

// ════════════════════════════════════════════════════════════════════════
// 64-битные целые наружу в QML
// ════════════════════════════════════════════════════════════════════════
//
// ⚠️ ЗАЧЕМ ЭТИ ЧЕТЫРЕ ФУНКЦИИ. cxx-qt печатает `i64`/`u64` как
// `::std::int64_t` / `::std::uint64_t`, а на 64-битном Linux это `long` /
// `unsigned long` — ОТДЕЛЬНЫЕ метатипы Qt, не те, что `qlonglong`. Движок
// QML их не знает, и литеральная декларативная привязка в такое свойство
// («displayId: 7») ОТКАЗЫВАЕТ: «Cannot assign value 7 to property
// displayId», причём вместе со всем документом — страница не создаётся
// вовсе. Замерено живым движком 31.08.2026, разбор и повторяемый стенд —
// `src/qml_int64_probe.rs`.
//
// Поэтому наружу в QML 64-битное целое НЕ выставляется никогда: свойство,
// параметр инвокабла и параметр сигнала объявляются `f64` (в QML —
// `double`), а поле внутри остаётся точным `i64`/`u64`. Так же держит эти
// значения и сам вендорный QML: `required property double playlistId`.
//
// ⚠️ ГРАНИЦА ТОЧНОСТИ: `double` держит целые без потерь до 2^53
// (9 007 199 254 740 992). Все значения, которые ходят этим путём, —
// номера строк базы демона, счётчики правок и поколений процессов, размеры
// кэша в байтах: до потолка им далеко. Новое значение, которому нужны все
// 64 бита (например, идентификатор Steam Workshop или отметка времени в
// наносекундах), этим путём выпускать НЕЛЬЗЯ — его место в `QString`.
// Сторож `qml_int64` в `src/lib.rs` не даст завести такое свойство молча.

/// Целое со знаком наружу в QML.
pub fn i64_to_qml(value: i64) -> f64 {
    value as f64
}

/// Целое без знака наружу в QML.
pub fn u64_to_qml(value: u64) -> f64 {
    value as f64
}

/// Обратно: число из QML в точное целое со знаком. Дробную часть
/// отбрасываем — QML присваивает своё `double`, а номер целый.
pub fn qml_to_i64(value: f64) -> i64 {
    value as i64
}

/// Обратно: число из QML в точное целое без знака.
pub fn qml_to_u64(value: f64) -> u64 {
    value as u64
}
