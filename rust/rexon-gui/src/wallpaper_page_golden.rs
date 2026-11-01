// Голден страницы «Обои»: разбор эталона, сборка кадров и реплей ЧИСТОГО слоя.
//
// Эталон — tests/golden/wallpaper_page_cases.json, снятый генератором рядом с
// ЖИВОГО питоновского моста (37 случаев, 564 записи сигналов). Здесь он
// превращается в записи журнала порта и проигрывается двумя способами:
//
//   * ЧИСТЫЙ слой — `cargo test` (низ этого файла). Он проигрывает решения
//     ЯДРА (we_wallpaper/we_query) и чистую модель `WallpaperRows`, а
//     рассылку повторяет по тем же правилам, что и запрос
//     (`LIST_RELOAD_ON` берётся у него же, второй копии списка нет). Только
//     здесь видны СТРУКТУРНЫЕ сигналы модели (`modelReset`): в Qt они
//     приватные (QPrivateSignal), из Rust на них не подписаться.
//     ⚠️ Раз рассылку он ПОВТОРЯЕТ, а не вызывает, порядок «модель
//     пересобрана ДО itemsChanged» доказывает не он, а стенд: там на том же
//     месте стоит `countChanged` ЖИВОЙ модели, и перестановка в порте
//     немедленно валит сверку (проверено укусом).
//   * Qt-слой — стенд `wallpaper_page_selftest` на ЖИВЫХ QObject'ах со всеми
//     восемью запросами страницы и НАСТОЯЩИМ таймером дебаунса.
//
// Почему два прогона, а не один: `#[cfg(test)]` в файлах-мостах cxx-qt не
// работает (build script гоняется один раз на lib и на test), поэтому Qt-слой
// проверяется бинарём-стендом. Так же выкрутилась партия 3 (discover_queries).
//
// ⚠️ Расхождения с эталоном НЕ подгоняются. Порт местами ведёт себя иначе,
// чем Python, — но потому, что Python расходился с ОРИГИНАЛОМ
// (waywallen `ui/src/query/wallpaper_query.cpp`), а порт сделан
// по оригиналу. Каждая такая правка эталона перечислена в [`FIXUPS`], несёт
// причину и ОБЯЗАНА сработать: если правка перестала быть нужной, прогон
// ругается — так протухшая поблажка не переживёт починку.

use prost::Message;
use serde_json::{Map, Value};

use rexon_core::proto::control as pb;
use rexon_core::we_query::{self, QuerySignal, QueryState};
use rexon_core::we_settings::VMap;
use rexon_core::we_wallpaper;

use crate::discover_queries::golden::{bool_at, i64_at, str_at, strings_at};
use crate::wallpaper_list_model::{ModelEvent, WallpaperRows};

/// Голден в исходниках крейта — тот самый файл, который печатает
/// tests/golden/wallpaper_page_cases.py с живого Python-моста.
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

// ── Журнал ──────────────────────────────────────────────────────────────────

/// Одна наблюдаемая запись: кто испустил и что именно.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Имя объекта: list / get / apply / remove / scan / propertySet /
    /// layout / portal / list.model.
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
    // ── база Query ──
    Status(i32),
    Querying,
    ErrorChanged,
    ForwardErrorChanged,
    Finished,
    ErrorOccurred(String),
    /// Пересылка в глобальный обработчик. В голдене она помечена `q = "app"`
    /// (её шлёт синглтон App); в порте App ещё нет, и её шлёт сам запрос
    /// сигналом `appErrorOccurred`, поэтому запись приписывается тому
    /// запросу, чей `errorOccurred` только что прошёл. Порядок не меняется:
    /// у оригинала пересылка идёт сразу за собственным сигналом.
    AppError(String),
    /// `<имя>Changed()` — имя закрытым списком, чтобы опечатка в голдене
    /// роняла разбор, а не молча теряла сигнал.
    Prop(&'static str),
    /// Кадр наружу: токен, поле полезной нагрузки и ВСЕ поля запроса.
    Sent {
        token: i64,
        field: String,
        request: Value,
    },
    /// Удалён один предмет (`removed` оригинала).
    Removed(String),
    /// Удалён набор: что просили и сколько демон снял.
    RemovedMany { ids: Vec<String>, count: i32 },
    /// Возврат инвокабла (`replaceFilterState`).
    Result { method: String, value: bool },
    /// Сброс модели библиотеки — begin/endResetModel.
    ModelReset,
    /// countChanged модели библиотеки.
    ModelCount,
    /// selectionChanged модели: у Python его не было вовсе, поэтому в
    /// эталоне он не встречается и любое его появление — расхождение.
    ModelSelection,
    ModelSelectedCount,
    /// dataChanged по роли выбора.
    ModelSelectedRange { first: i32, last: i32 },
}

/// Имена сигналов-свойств, встречающиеся в эталоне и в порте.
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

/// Ожидаемый журнал случая ровно таким, каким его снял Python.
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

// ── Правки эталона: сознательные расхождения порта ──────────────────────────

/// Одна правка: чем эталон отличается от того, что ОБЯЗАН делать порт.
pub struct Fixup {
    /// Случай голдена, к которому она относится.
    pub case: &'static str,
    /// Короткое имя для отчёта.
    pub name: &'static str,
    /// Почему эталон здесь не эталон.
    pub why: &'static str,
}

/// Список сознательных расхождений. Всё, чего в нём нет, — баг порта.
///
/// Общее у всех: голден снят с ЖИВОГО Python, а Python по этим
/// пунктам расходился с оригиналом (`wallpaper_query.cpp`), и порт сделан по
/// ОРИГИНАЛУ (правило #16 — баги при переводе чиним, а не консервируем).
/// Подробный разбор — в шапке `wallpaper_actions.rs`.
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

/// Голден → журнал, который ОБЯЗАН выдать порт.
///
/// Возвращает исправленные записи и имена применённых правок. Правка, которая
/// ничего не изменила, — ошибка: значит, она протухла.
pub fn port_records(case: &Value) -> (Vec<Record>, Vec<&'static str>) {
    let name = str_at(case, "name");
    let mut records = python_records(case);
    let mut applied: Vec<&'static str> = Vec::new();

    match name.as_str() {
        "удаление: один id и пустышка в наборе" => {
            // Кадр: пустой id отсеян, единственный уезжает полем wallpaper_id.
            for record in records.iter_mut() {
                if let Sig::Sent { request, .. } = &mut record.sig {
                    request["wallpaper_id"] = Value::String("w1".into());
                    request["wallpaper_ids"] = Value::Array(Vec::new());
                    applied.push(FIXUPS[0].name);
                }
            }
            // Отчёт: набор без пустышки, число — из ответа демона.
            for record in records.iter_mut() {
                if let Sig::RemovedMany { ids, count } = &mut record.sig {
                    *ids = vec!["w1".to_string()];
                    *count = 1;
                    applied.push(FIXUPS[2].name);
                }
            }
            // И сверх набора — сигнал на единственный предмет.
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
            // Свойство обновляется в разборе ответа, то есть ДО перевода
            // статуса в Finished — как у оригинала.
            let at = records
                .iter()
                .position(|r| r.q == "apply" && matches!(r.sig, Sig::Status(2)))
                .expect("голден: в случае нет перехода в Finished");
            records.insert(at, Record::new("apply", Sig::Prop("rendererIdChanged")));
            applied.push(FIXUPS[5].name);
        }
        "удаление: reload() снимает обою из свойства wallpaperId" => {
            // Единственный случай, где записи ДОПИСЫВАЮТСЯ целиком: у
            // Python после reload() не происходило ничего, а порт обязан
            // отправить тот же кадр, что и `remove(["w1"])` — с id в поле
            // `wallpaper_id`, потому что предмет один.
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
            // У порта устаревший ответ отбрасывается целиком, поэтому весь
            // набор сигналов даёт СВЕЖИЙ ответ — ровно один раз. Python
            // разобрал оба, и второй разбор дописал лишний wallpaperChanged
            // в хвост.
            let last = records.pop().expect("голден: случай пуст");
            assert_eq!(
                last,
                Record::new("get", Sig::Prop("wallpaperChanged")),
                "голден: в хвосте случая не разбор устаревшего ответа"
            );
            applied.push(FIXUPS[8].name);
        }
        "применение: тот же рендерер вторым ответом — сигнал всё равно" => {
            // Сигнал летит на КАЖДЫЙ ответ демона, даже когда рендерер тот
            // же: он означает «демон ответил». Место то же, что и у случая
            // выше, — перед переводом в Finished.
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
            // Правка одна на случай, а не на вставку: она про ПРАВИЛО
            // «сигнал безусловный», а не про каждое его срабатывание.
            applied.push(FIXUPS[7].name);
        }
        "свойство обои: value и reset — разные ветки oneof" => {
            // Порядок правок случая: wallpaperId, propertyKey, propertyValue,
            // propertyValue (на пустое), reset.
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

/// Правки ИТОГОВЫХ свойств: то же расхождение, но видимое не в журнале, а в
/// значении свойства в конце случая.
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

/// Итоговое состояние запроса таким, каким его ОБЯЗАН показать порт.
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

/// Сколько правок обязано примениться на всём голдене — страховка от
/// протухшей поблажки: починили расхождение в порте, а правку забыли снять.
pub fn expected_fixups() -> usize {
    FIXUPS.len()
}

// ── Кадры демона: описание из сценария → настоящий protobuf ────────────────

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
        // waywallen 0.4.4: отражения в сценариях нет.
        flip: 0,
    }
}

/// Запись библиотеки из сценария. Зеркало `_fill_entry` генератора: обе
/// стороны собирают ОДИН и тот же protobuf из одного описания, поэтому
/// эталон не зависит от того, кто его читает.
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
        // waywallen 0.4.4: скрытых записей в сценариях нет.
        hidden: false,
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

/// Шаг «respond»/«respondError» → сырой кадр `ServerFrame`, ровно тот, что
/// клиент отдаёт ядру при `want_frame`.
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

// ── Кадр запроса → JSON в том же виде, в каком его печатает голден ─────────

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

/// Цели применения v0.3.5 в голдене: карта на цель, ОБЕ ветки `oneof`.
///
/// Голден печатает `_msg_fields` (tests/golden/wallpaper_page_cases.py:663) —
/// он проходит по ДЕСКРИПТОРУ и выкладывает каждое поле, в том числе
/// незаполненную ветку `oneof` с её значением по умолчанию. Так записаны и
/// соседи по этому же файлу: `wallpaper_property_set` несёт и `value`, и
/// `reset`, правило фильтра — все три ветки `payload`. Значит, цель-дисплей
/// в эталоне выглядит как `{display_id: 1, canvas_id: ""}`, а цель-холст —
/// как `{display_id: 0, canvas_id: "cnv"}`.
///
/// ⚠️ Это только форма ЗАПИСИ. На проводе ветка ровно одна: `oneof` не
/// умеет держать обе, и присвоение `canvas_id` стёрло бы `display_id`.
/// Кадр собирает `we_manage::presentation_targets` (порт вендорного
/// `presentationTargetsFromVariant`, ui/src/proto.cpp:8 на f7baecd), и
/// байты сверяет стенд — здесь же печатается человекочитаемый снимок.
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

/// Кадр запроса → карта ВСЕХ полей, включая значения по умолчанию.
///
/// Зеркало `_msg_fields` генератора: пустые списки, нули и `false` обязаны
/// быть видны — именно на них ловятся потерянные поля. Единственное
/// исключение — одиночное вложенное сообщение: у него важен сам факт наличия
/// (тело раскладки при сбросе отправляться НЕ должно), поэтому неустановленное
/// даёт `null`, а не карту нулей.
///
/// Разбирается ИМЕННО то, что уехало байтами: намерение отправить кадр
/// проверкой не считается.
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
                ("wallpaper_id", Value::String(r.wallpaper_id)),
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

// ── Воспроизведение ЧИСТЫМ слоем: запрос списка и его модель ───────────────

/// Стенд чистого слоя. Здесь живут только те решения, которые считает ядро,
/// плюс правила рассылки, записанные в `wallpaper_queries.rs`:
/// перезагрузка по списку [`crate::wallpaper_queries::LIST_RELOAD_ON`] и
/// пересборка модели ПЕРЕД `itemsChanged`.
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
                        // `invalidateList`, как у моста: новое поколение —
                        // ответ на запрос в полёте больше не наш.
                        self.pending.clear();
                        let _ = we_wallpaper::invalidate_list(&mut self.data);
                    }
                    // waywallen 0.5.0: выборка и страницы (`selection`,
                    // `offset`, `noMore`, `limit`) — свойств не было в эпоху
                    // эталона, живой стенд их не слушает. Сторожат их юниты
                    // ядра и проба QML (`qml_int64_probe.rs`).
                    if matches!(name, "selection" | "offset" | "noMore" | "limit") {
                        continue;
                    }
                    // waywallen 0.5.0: `hasOtherFiltersChanged` эталон эпохи
                    // Python не знает (свойства не было), и живой стенд его
                    // не слушает. Порядок и условие сигнала сторожат юниты
                    // ядра (`tests/we_wallpaper_parity.rs`); в эталоне без
                    // скрытых библиотек он идёт ровно в паре с
                    // `hasActiveFiltersChanged`, так что пропуск ничего не
                    // прячет.
                    if name == "hasOtherFilters" {
                        continue;
                    }
                    if name == "items" {
                        // Модель перестраивается ПЕРВОЙ: у моста пересборку
                        // вешал конструктор запроса, то есть она стояла
                        // первой в цепочке подписчиков, и QML на
                        // onItemsChanged обязан читать УЖЕ новые строки.
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
        // Новое поколение и сброс страниц — как `reload` моста.
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
        // Номер выдаётся ДО сборки кадра — реплика обязана повторять путь
        // продукта, а там `request_id` теперь один и лежит внутри кадра.
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

    /// Сработал дебаунс: правки свойств склеиваются в ОДНУ перезагрузку.
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
                    // Кадр не наш: в случае участвует запрос-действие.
                    return;
                }
                let frame = build_frame(step);
                let signals =
                    we_wallpaper::apply_wallpaper_list(&mut self.base, &mut self.data, &frame)
                        .expect("ответ демона разбирается");
                self.dispatch(signals);
            }
            "connected" => crate::discover_queries::set_connected(bool_at(step, "value")),
            // Шаги прочих запросов страницы чистому слою неинтересны: их
            // логика живёт в Qt-объектах и проверяется стендом.
            _ => {}
        }
    }
}

/// Прогнать случай через ЧИСТЫЙ слой запроса списка.
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
    // Генератор в конце случая ещё раз крутит цикл событий: незакрытый
    // дебаунс обязан сработать и там.
    harness.settle();
    harness
}

/// Строки модели по РОЛЯМ в том же JSON-виде, что печатает голден.
///
/// Роль `selected` из сверки исключена намеренно: у питоновской модели её не
/// было вовсе (шапка `wallpaper_list_model.rs`, пункт 1), поэтому в эталоне
/// её нет и быть не может. Её наличие проверяется отдельно.
pub fn rows_json(rows: &[VMap]) -> Value {
    Value::Array(rows.iter().map(|m| Value::Object(m.clone())).collect())
}

/// Ожидаемые строки голдена, дополненные полями waywallen 0.4.4.
///
/// Голден снят с живого Python до v0.4.4, и двух новых ключей записи в нём
/// нет: `hidden` (обоина скрыта) и `flip` внутри
/// `wallpaperLayoutOverride` (отражение). В сценариях голдена ни скрытых
/// обоев, ни отражения нет (`entry_of` ставит `hidden: false`, `flip: 0`),
/// поэтому дописываются ровно эти умолчания — остальное сверяется как было.
/// Одна ожидаемая запись (панель деталей) — то же дополнение, что у
/// [`expected_rows_v044`]. Пустая карта («записи нет») остаётся пустой.
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
                    if let Some(Value::Object(layout)) = map.get_mut("wallpaperLayoutOverride") {
                        layout.entry("flip").or_insert(Value::from(0));
                    }
                }
                row
            })
            .collect(),
    )
}

/// Ключи записи библиотеки и их ПОРЯДОК — так, как их печатает голден
/// (`_WALLPAPER_KEYS` моста).
///
/// Порядок здесь не косметика: роли модели считаются от него
/// (`FIRST_ENTRY_ROLE + индекс`), и перестановка двух ключей молча съезжает
/// нумерацию ролей — сверка строк по ИМЕНАМ такого не видит.
///
/// Голден снят до waywallen 0.4.4, и ключа `hidden` в нём нет: он добавлен
/// в КОНЕЦ списка (роли прежних ключей не сдвинулись) — так его и
/// дописываем к эталону (см. [`expected_rows_v044`]).
pub fn entry_keys(doc: &Value) -> Vec<String> {
    let mut keys = strings_at(doc, "ключи_записи");
    if !keys.iter().any(|k| k == "hidden") {
        keys.push("hidden".to_string());
    }
    keys
}

/// Ожидаемое итоговое состояние одного запроса из голдена.
pub fn final_of<'a>(case: &'a Value, q: &str) -> &'a Value {
    case.get("final")
        .and_then(|f| f.get(q))
        .unwrap_or_else(|| panic!("голден: нет итогового состояния {q}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Голден снят с ЖИВЫХ Python-запросов страницы «Обои». Здесь через
    /// чистый слой проигрывается его половина — запрос библиотеки и его
    /// модель: какой кадр уходит демону (со всеми полями), в каком порядке
    /// летят сигналы запроса и модели и какими оказываются свойства и строки.
    ///
    /// Вторая половина (семь запросов-действий) живёт в Qt-объектах, у неё
    /// чистого слоя нет — её проверяет стенд `wallpaper_page_selftest`.
    ///
    /// Один тест на всё, а не тридцать три: чистый слой пользуется общим на
    /// мост счётчиком токенов (в Python это была модульная переменная), и два
    /// теста, идущие параллельно, толкались бы за него.
    #[test]
    fn golden_parity_with_python_list_query() {
        let _tokens = crate::discover_queries::token_test_lock();
        // Язык — исходный: тексты ошибок и подписи источников с 30.09 —
        // msgid каталога (русский — переводом), а язык процесса общий и
        // соседи крейта ставят именно исходный. Эталон хранит английские
        // msgid там, где Python-эпоха писала русский.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let doc = load();
        let cases = doc["cases"].as_array().expect("голден: cases");
        assert!(!cases.is_empty(), "голден пуст");

        // Ключи записи и их порядок — от них считаются номера ролей.
        assert_eq!(
            crate::wallpaper_list_model::ENTRY_KEYS.to_vec(),
            entry_keys(&doc),
            "порядок ключей записи разошёлся с эталоном — поехали номера ролей"
        );

        let mut records = 0usize;
        for case in cases {
            let name = str_at(case, "name");
            let harness = replay_pure(case);
            // Ожидание — только записи запроса списка и его модели.
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
        // Страховка от «сравнили пустоту с пустотой».
        assert!(
            records > 200,
            "проиграно всего {records} сигналов — эталон подозрительно пуст"
        );
    }

    /// Правки эталона обязаны быть ЖИВЫМИ: каждая — сработать ровно там, где
    /// заявлена. Правка, которая ничего не меняет, означает, что расхождение
    /// починили, а поблажку снять забыли.
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
