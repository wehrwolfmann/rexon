// Стенд страницы «Обои»: тот же голден на ЖИВЫХ QObject'ах.
//
// Чистый слой запроса списка уже сверен с Python в `cargo test`
// (wallpaper_page_golden). Здесь проверяется ровно то, чего он проверить не
// может:
//
//   * все ВОСЕМЬ объектов страницы разом — запрос библиотеки и семь
//     запросов-действий. У действий чистого слоя нет вовсе: их решения живут
//     прямо в Qt-объектах, и другого способа их проиграть не существует;
//   * сигналы действительно долетают из C++, а не «мы решили их послать»;
//   * дебаунс — НАСТОЯЩИЙ QTimer: две правки свойства подряд склеиваются в
//     один кадр потому, что так работает Qt, а не потому что так написано в
//     стенде;
//   * кадр разбирается ОБРАТНО из байтов, уехавших наружу;
//   * модель библиотеки — ребёнок запроса, читается по РОЛЯМ через свой
//     `data()`, то есть ровно так, как её читает делегат QML.
//
// Чего стенд не видит: структурных сигналов модели (`modelReset`,
// `rowsInserted`). Они приватные (QPrivateSignal), из Rust на них не
// подписаться, поэтому их порядок сверяет чистый слой.
//
// Возврат: 0 — всё сошлось, 1 — есть расхождения. Молчаливого успеха по
// таймауту нет: стенд ничего не ждёт вслепую.

use core::pin::Pin;
use cxx_qt::QMetaObjectConnectionGuard;
use cxx_qt_lib::{QByteArray, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::discover_queries::golden::{bool_at, i64_at, str_at, strings_at};
use crate::qvalue::{json_to_variant, variant_to_json};
use crate::remote_list_model::qobject::{rexon_delete_object, rexon_process_events};
use crate::wallpaper_actions::qobject::{
    WallpaperApplyQuery, WallpaperApplyViaPortalQuery, WallpaperGetQuery, WallpaperLayoutSetQuery,
    WallpaperHideQuery, WallpaperPropertySetQuery, WallpaperRemoveQuery, WallpaperScanQuery,
};
use crate::wallpaper_list_model::qobject::WallpaperListModel;
use crate::wallpaper_list_model::{ENTRY_KEYS, FIRST_ENTRY_ROLE, SELECTED_ROLE};
use crate::wallpaper_page_golden::{self as golden, Record, Sig};
use crate::wallpaper_queries::qobject::{
    rexon_new_wallpaper_apply_query, rexon_new_wallpaper_apply_via_portal_query,
    rexon_new_wallpaper_get_query, rexon_new_wallpaper_hide_query,
    rexon_new_wallpaper_layout_set_query,
    rexon_new_wallpaper_list_query, rexon_new_wallpaper_property_set_query,
    rexon_new_wallpaper_remove_query, rexon_new_wallpaper_scan_query, QObject,
    WallpaperListQuery,
};

pub const ARG: &str = "--wallpaper-selftest";

/// Журнал живых сигналов.
type Journal = Arc<Mutex<Vec<Record>>>;

fn push(journal: &Journal, q: &str, sig: Sig) {
    journal.lock().expect("журнал").push(Record::new(q, sig));
}

/// Живая страница: восемь запросов и модель библиотеки.
struct Page {
    list: *mut WallpaperListQuery,
    get: *mut WallpaperGetQuery,
    apply: *mut WallpaperApplyQuery,
    remove: *mut WallpaperRemoveQuery,
    scan: *mut WallpaperScanQuery,
    property_set: *mut WallpaperPropertySetQuery,
    layout: *mut WallpaperLayoutSetQuery,
    portal: *mut WallpaperApplyViaPortalQuery,
    model: *mut WallpaperListModel,
    raw: Vec<*mut QObject>,
}

/// Изменяемая ссылка из неизменяемой: объектами владеет Qt, а не Rust. Та же
/// схема, что у стендов партий 2-3.
macro_rules! pin_of {
    ($ptr:expr) => {
        unsafe { Pin::new_unchecked(&mut *$ptr) }
    };
}

impl Page {
    fn new() -> Self {
        let null = core::ptr::null_mut();
        let list = unsafe { rexon_new_wallpaper_list_query(null) };
        let get = unsafe { rexon_new_wallpaper_get_query(null) };
        let apply = unsafe { rexon_new_wallpaper_apply_query(null) };
        let remove = unsafe { rexon_new_wallpaper_remove_query(null) };
        let scan = unsafe { rexon_new_wallpaper_scan_query(null) };
        let property_set = unsafe { rexon_new_wallpaper_property_set_query(null) };
        let layout = unsafe { rexon_new_wallpaper_layout_set_query(null) };
        let portal = unsafe { rexon_new_wallpaper_apply_via_portal_query(null) };
        let raw = vec![list, get, apply, remove, scan, property_set, layout, portal];
        for object in &raw {
            assert!(!object.is_null(), "фабрика вернула nullptr");
        }
        let list: *mut WallpaperListQuery = list.cast();
        // Модель создаёт сам запрос в пост-конструкции и владеет ею как
        // ребёнком — отдельно её удалять нельзя.
        let model = unsafe { (*list).data() }.cast::<WallpaperListModel>();
        assert!(!model.is_null(), "запрос списка не создал модель");
        Page {
            list,
            get: get.cast(),
            apply: apply.cast(),
            remove: remove.cast(),
            scan: scan.cast(),
            property_set: property_set.cast(),
            layout: layout.cast(),
            portal: portal.cast(),
            model,
            raw,
        }
    }

    #[allow(clippy::mut_from_ref)]
    fn list(&self) -> Pin<&mut WallpaperListQuery> {
        pin_of!(self.list)
    }
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> Pin<&mut WallpaperGetQuery> {
        pin_of!(self.get)
    }
    #[allow(clippy::mut_from_ref)]
    fn apply(&self) -> Pin<&mut WallpaperApplyQuery> {
        pin_of!(self.apply)
    }
    #[allow(clippy::mut_from_ref)]
    fn remove(&self) -> Pin<&mut WallpaperRemoveQuery> {
        pin_of!(self.remove)
    }
    #[allow(clippy::mut_from_ref)]
    fn scan(&self) -> Pin<&mut WallpaperScanQuery> {
        pin_of!(self.scan)
    }
    #[allow(clippy::mut_from_ref)]
    fn property_set(&self) -> Pin<&mut WallpaperPropertySetQuery> {
        pin_of!(self.property_set)
    }
    #[allow(clippy::mut_from_ref)]
    fn layout(&self) -> Pin<&mut WallpaperLayoutSetQuery> {
        pin_of!(self.layout)
    }
    #[allow(clippy::mut_from_ref)]
    fn portal(&self) -> Pin<&mut WallpaperApplyViaPortalQuery> {
        pin_of!(self.portal)
    }

    fn destroy(self) {
        for object in self.raw {
            unsafe { rexon_delete_object(object) };
        }
    }
}

/// Подписка на сигналы базы Query. Макрос, потому что типы разные, а имена
/// методов одинаковые — ровно та же причина, что у `impl_action_base!`.
macro_rules! connect_base {
    ($guards:expr, $journal:expr, $ptr:expr, $ty:ty, $name:literal) => {{
        let raw: *mut $ty = $ptr;
        let pin = || unsafe { Pin::new_unchecked(&mut *raw) };

        let sink = Arc::clone($journal);
        $guards.push(pin().on_status_changed(move |_, status| {
            push(&sink, $name, Sig::Status(status));
        }));
        let sink = Arc::clone($journal);
        $guards.push(pin().on_querying_changed(move |_| push(&sink, $name, Sig::Querying)));
        let sink = Arc::clone($journal);
        $guards.push(pin().on_error_changed(move |_| push(&sink, $name, Sig::ErrorChanged)));
        let sink = Arc::clone($journal);
        $guards.push(
            pin().on_forward_error_changed(move |_| push(&sink, $name, Sig::ForwardErrorChanged)),
        );
        let sink = Arc::clone($journal);
        $guards.push(pin().on_finished(move |_| push(&sink, $name, Sig::Finished)));
        let sink = Arc::clone($journal);
        $guards.push(pin().on_error_occurred(move |_, text| {
            push(&sink, $name, Sig::ErrorOccurred(text.to_string()));
        }));
        let sink = Arc::clone($journal);
        $guards.push(pin().on_app_error_occurred(move |_, text| {
            push(&sink, $name, Sig::AppError(text.to_string()));
        }));
        // Кадр разбирается ОБРАТНО из байтов: стенд обязан проверять то, что
        // реально уехало, а не намерение это отправить.
        let sink = Arc::clone($journal);
        $guards.push(pin().on_send_request(move |_, token, _field, request| {
            let (field, request) = golden::request_json(request.as_slice());
            push(
                &sink,
                $name,
                Sig::Sent {
                    token,
                    field,
                    request,
                },
            );
        }));
    }};
}

/// Подписка на сигнал-свойство: одна строчка вместо четырёх на каждый.
macro_rules! connect_prop {
    ($guards:expr, $journal:expr, $pin:expr, $method:ident, $name:literal, $signal:literal) => {{
        let sink = Arc::clone($journal);
        $guards.push($pin.$method(move |_| push(&sink, $name, Sig::Prop($signal))));
    }};
}

/// Счётчики стенда.
struct Report {
    failures: Vec<String>,
    cases: usize,
    /// Имена случаев, на которых нашлось хоть одно расхождение. Приговор по
    /// СЛУЧАЯМ, а не по числу проваленных проверок: одно расхождение журнала
    /// тянет за собой десяток проваленных свойств, и «провалов 14» ничего не
    /// говорит о том, сколько сценариев на самом деле разошлось.
    diverged: Vec<String>,
    events: usize,
    fixups: Vec<&'static str>,
}

impl Report {
    fn check(&mut self, condition: bool, message: impl FnOnce() -> String) {
        if !condition {
            self.failures.push(message());
        }
    }

    /// Сверка значения: в отчёт уезжает И то, что в эталоне, И то, что выдал
    /// порт. Одно «не сошлось» без обеих сторон заставляет лезть в голден
    /// руками — а расхождений тут может быть сразу несколько.
    fn eq<T: PartialEq + std::fmt::Debug>(&mut self, what: String, actual: T, expected: T) {
        if actual != expected {
            self.failures
                .push(format!("{what}: в порте {actual:?}, в эталоне {expected:?}"));
        }
    }
}

/// Значение QVariant в JSON.
///
/// Через карту-обёртку намеренно: `QJsonDocument::fromVariant` держит на
/// верхнем уровне только массив или объект, а роли модели — это строки,
/// числа и признаки. Обёртка отдаёт разбор типов Qt, а не догадкам Rust.
fn variant_json(value: &QVariant) -> Value {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(QString::from("v"), value.clone());
    variant_to_json(&QVariant::from(&map))
        .get("v")
        .cloned()
        .unwrap_or(Value::Null)
}

fn strings_of(list: &QStringList) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn qstringlist(values: &[String]) -> QStringList {
    QStringList::from_iter(values.iter().map(QString::from))
}

/// Строки живой модели по РОЛЯМ в том же JSON-виде, что печатает голден.
///
/// Роль `selected` в эталоне отсутствует: у питоновской модели её не было
/// вовсе (шапка `wallpaper_list_model.rs`, пункт 1). Она проверяется
/// отдельно — что объявлена и что по ней читается `false`.
fn rows_json(model: &WallpaperListModel, name: &str, report: &mut Report) -> Value {
    let mut rows = Vec::new();
    for row in 0..model.count() {
        let mut object = serde_json::Map::new();
        for (i, key) in ENTRY_KEYS.iter().enumerate() {
            let role = model.role_number(&QString::from(*key));
            report.check(role == FIRST_ENTRY_ROLE + i as i32, || {
                format!("{name}: роль {key} = {role}, а должна быть {}", FIRST_ENTRY_ROLE + i as i32)
            });
            object.insert(
                (*key).to_string(),
                variant_json(&model.role_value(row, &QString::from(*key))),
            );
        }
        report.check(
            model.role_number(&QString::from("selected")) == SELECTED_ROLE,
            || format!("{name}: роли selected нет — делегат вендора её читает"),
        );
        report.check(
            variant_json(&model.role_value(row, &QString::from("selected"))) == Value::Bool(false),
            || format!("{name}: строка {row} оказалась выбранной"),
        );
        rows.push(Value::Object(object));
    }
    Value::Array(rows)
}

/// Прогон одного случая голдена на живых объектах.
fn run_case(case: &Value, report: &mut Report) {
    crate::discover_queries::reset_tokens();
    crate::discover_queries::set_connected(true);

    let name = str_at(case, "name");
    let failures_before = report.failures.len();
    let page = Page::new();
    let journal: Journal = Arc::new(Mutex::new(Vec::new()));
    let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();

    connect_base!(&mut guards, &journal, page.list, WallpaperListQuery, "list");
    connect_base!(&mut guards, &journal, page.get, WallpaperGetQuery, "get");
    connect_base!(&mut guards, &journal, page.apply, WallpaperApplyQuery, "apply");
    connect_base!(&mut guards, &journal, page.remove, WallpaperRemoveQuery, "remove");
    connect_base!(&mut guards, &journal, page.scan, WallpaperScanQuery, "scan");
    connect_base!(
        &mut guards,
        &journal,
        page.property_set,
        WallpaperPropertySetQuery,
        "propertySet"
    );
    connect_base!(&mut guards, &journal, page.layout, WallpaperLayoutSetQuery, "layout");
    connect_base!(
        &mut guards,
        &journal,
        page.portal,
        WallpaperApplyViaPortalQuery,
        "portal"
    );

    // ── собственные сигналы запросов ────────────────────────────────────
    connect_prop!(&mut guards, &journal, page.list(), on_wp_type_changed, "list", "wpTypeChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_search_text_changed, "list", "searchTextChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_sorts_changed, "list", "sortsChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_filters_changed, "list", "filtersChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_filter_logics_changed, "list", "filterLogicsChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_skip_types_changed, "list", "skipTypesChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_filter_tags_changed, "list", "filterTagsChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_skip_content_ratings_changed, "list", "skipContentRatingsChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_filter_state_changed, "list", "filterStateChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_has_active_filters_changed, "list", "hasActiveFiltersChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_total_changed, "list", "totalChanged");
    connect_prop!(&mut guards, &journal, page.list(), on_items_changed, "list", "itemsChanged");

    connect_prop!(&mut guards, &journal, page.get(), on_wallpaper_id_changed, "get", "wallpaperIdChanged");
    connect_prop!(&mut guards, &journal, page.get(), on_wallpaper_changed, "get", "wallpaperChanged");

    connect_prop!(&mut guards, &journal, page.apply(), on_wallpaper_changed, "apply", "wallpaperChanged");
    connect_prop!(&mut guards, &journal, page.apply(), on_targets_changed, "apply", "targetsChanged");
    connect_prop!(&mut guards, &journal, page.apply(), on_renderer_name_changed, "apply", "rendererNameChanged");
    connect_prop!(&mut guards, &journal, page.apply(), on_renderer_id_changed, "apply", "rendererIdChanged");

    connect_prop!(&mut guards, &journal, page.remove(), on_wallpaper_id_changed, "remove", "wallpaperIdChanged");
    connect_prop!(&mut guards, &journal, page.scan(), on_count_changed, "scan", "countChanged");

    connect_prop!(&mut guards, &journal, page.property_set(), on_wallpaper_id_changed, "propertySet", "wallpaperIdChanged");
    connect_prop!(&mut guards, &journal, page.property_set(), on_property_key_changed, "propertySet", "propertyKeyChanged");
    connect_prop!(&mut guards, &journal, page.property_set(), on_property_value_changed, "propertySet", "propertyValueChanged");
    connect_prop!(&mut guards, &journal, page.property_set(), on_reset_changed, "propertySet", "resetChanged");

    connect_prop!(&mut guards, &journal, page.layout(), on_params_changed, "layout", "paramsChanged");

    connect_prop!(&mut guards, &journal, page.portal(), on_wallpaper_id_changed, "portal", "wallpaperIdChanged");
    connect_prop!(&mut guards, &journal, page.portal(), on_uri_changed, "portal", "uriChanged");

    // Отчёт удаления — на нём висит тост страницы.
    let sink = Arc::clone(&journal);
    guards.push(page.remove().on_removed(move |_, id| {
        push(&sink, "remove", Sig::Removed(id.to_string()));
    }));
    let sink = Arc::clone(&journal);
    guards.push(page.remove().on_removed_many(move |_, ids, count| {
        push(
            &sink,
            "remove",
            Sig::RemovedMany {
                ids: strings_of(&ids),
                count,
            },
        );
    }));

    // Модель библиотеки: структурных сигналов из Rust не видно, зато виден
    // countChanged и сигналы выбора (последних в эталоне быть не может).
    let model_pin = || pin_of!(page.model);
    let sink = Arc::clone(&journal);
    guards.push(model_pin().on_count_changed(move |_| push(&sink, "list.model", Sig::ModelCount)));
    let sink = Arc::clone(&journal);
    guards.push(
        model_pin().on_selection_changed(move |_| push(&sink, "list.model", Sig::ModelSelection)),
    );
    let sink = Arc::clone(&journal);
    guards.push(model_pin().on_selected_count_changed(move |_| {
        push(&sink, "list.model", Sig::ModelSelectedCount)
    }));

    // ── шаги сценария ───────────────────────────────────────────────────
    for step in case
        .get("steps")
        .and_then(Value::as_array)
        .expect("голден: у случая нет шагов")
    {
        match str_at(step, "op").as_str() {
            "set" => {
                let q = str_at(step, "q");
                let prop = str_at(step, "name");
                let value = step.get("value").cloned().unwrap_or(Value::Null);
                let text = || QString::from(value.as_str().unwrap_or_default());
                let number = || value.as_i64().unwrap_or_default() as i32;
                let flag = || value.as_bool().unwrap_or(false);
                let list_of = || {
                    value
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|v| v.as_str().unwrap_or_default().to_string())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                };
                match (q.as_str(), prop.as_str()) {
                    ("list", "wpType") => page.list().set_wp_type(text()),
                    ("list", "searchText") => page.list().set_search_text(text()),
                    ("list", "sorts") => page.list().set_sorts(json_to_variant(&value)),
                    ("list", "filters") => page.list().set_filters(json_to_variant(&value)),
                    ("list", "filterLogics") => {
                        page.list().set_filter_logics(json_to_variant(&value))
                    }
                    ("list", "skipTypes") => page.list().set_skip_types(qstringlist(&list_of())),
                    ("list", "filterTags") => page.list().set_filter_tags(qstringlist(&list_of())),
                    ("list", "skipContentRatings") => {
                        page.list().set_skip_content_ratings(qstringlist(&list_of()))
                    }
                    ("list", "forwardError") => page.list().set_forward_error(flag()),
                    ("get", "wallpaperId") => page.get().set_wallpaper_id(text()),
                    ("apply", "wallpaper") => page.apply().set_wallpaper(json_to_variant(&value)),
                    ("apply", "targets") => page.apply().set_targets(json_to_variant(&value)),
                    ("apply", "rendererName") => page.apply().set_renderer_name(text()),
                    ("remove", "wallpaperId") => page.remove().set_wallpaper_id(text()),
                    ("propertySet", "wallpaperId") => page.property_set().set_wallpaper_id(text()),
                    ("propertySet", "propertyKey") => page.property_set().set_property_key(text()),
                    ("propertySet", "propertyValue") => {
                        page.property_set().set_property_value(text())
                    }
                    ("propertySet", "reset") => page.property_set().set_reset(flag()),
                    ("layout", "wallpaperId") => page.layout().set_wallpaper_id(text()),
                    ("layout", "clear") => page.layout().set_clear(flag()),
                    ("layout", "fillmode") => page.layout().set_fillmode(number()),
                    ("layout", "rotation") => page.layout().set_rotation(number()),
                    ("layout", "locationX") => page.layout().set_location_x(number()),
                    ("layout", "locationY") => page.layout().set_location_y(number()),
                    ("portal", "wallpaperId") => page.portal().set_wallpaper_id(text()),
                    (other_q, other_p) => {
                        report.check(false, || format!("{name}: нет свойства {other_q}.{other_p}"))
                    }
                }
            }
            "call" => {
                let q = str_at(step, "q");
                let method = str_at(step, "method");
                match (q.as_str(), method.as_str()) {
                    ("list", "reload") => page.list().reload(),
                    ("list", "replaceFilterState") => {
                        let filters = step.get("filters").cloned().unwrap_or(Value::Null);
                        let logics = step.get("logics").cloned().unwrap_or(Value::Null);
                        let changed = page.list().replace_filter_state(
                            json_to_variant(&filters),
                            json_to_variant(&logics),
                        );
                        push(
                            &journal,
                            "list",
                            Sig::Result {
                                method: "replaceFilterState".into(),
                                value: changed,
                            },
                        );
                    }
                    ("get", "reload") => page.get().reload(),
                    ("apply", "reload") => page.apply().reload(),
                    ("remove", "remove") => {
                        page.remove().remove(qstringlist(&strings_at(step, "ids")))
                    }
                    ("remove", "reload") => page.remove().reload(),
                    ("scan", "reload") => page.scan().reload(),
                    ("propertySet", "reload") => page.property_set().reload(),
                    ("layout", "reload") => page.layout().reload(),
                    ("portal", "reload") => page.portal().reload(),
                    (other_q, other_m) => {
                        report.check(false, || format!("{name}: нет метода {other_q}.{other_m}"))
                    }
                }
            }
            // Дебаунс — НАСТОЯЩИЙ таймер: крутим цикл событий дольше его
            // интервала. Не склеивались бы правки — пришло бы несколько
            // кадров, и сверка журнала это поймала бы.
            "settle" => rexon_process_events(i64_at(step, "ms").max(300) as i32),
            "respond" | "respondError" => {
                let token = i64_at(step, "token");
                let frame = QByteArray::from(&golden::build_frame(step)[..]);
                // Кому отдать ответ — известно из уже записанного кадра.
                let owner = journal
                    .lock()
                    .expect("журнал")
                    .iter()
                    .find_map(|record| match &record.sig {
                        Sig::Sent { token: t, .. } if *t == token => Some(record.q.clone()),
                        _ => None,
                    });
                match owner.as_deref() {
                    Some("list") => page.list().deliver_response(token, &frame),
                    Some("get") => page.get().deliver_response(token, &frame),
                    Some("apply") => page.apply().deliver_response(token, &frame),
                    Some("remove") => page.remove().deliver_response(token, &frame),
                    Some("scan") => page.scan().deliver_response(token, &frame),
                    Some("propertySet") => page.property_set().deliver_response(token, &frame),
                    Some("layout") => page.layout().deliver_response(token, &frame),
                    Some("portal") => page.portal().deliver_response(token, &frame),
                    _ => report.check(false, || format!("{name}: некому отдать ответ {token}")),
                }
            }
            "connected" => crate::discover_queries::set_connected(bool_at(step, "value")),
            other => report.check(false, || format!("{name}: неизвестное действие {other}")),
        }
    }
    // Незакрытый дебаунс обязан сработать и в конце случая.
    rexon_process_events(300);

    // ── сверка журнала ──────────────────────────────────────────────────
    let actual = journal.lock().expect("журнал").clone();
    report.events += actual.len();
    let (expected, fixups) = golden::port_records(case);
    report.fixups.extend(fixups);
    // Структурные сигналы модели стенду не видны — их проверяет чистый слой.
    let expected: Vec<Record> = expected
        .into_iter()
        .filter(|r| r.sig != Sig::ModelReset)
        .collect();
    report.check(actual == expected, || {
        format!("{name}: журнал разошёлся\n{}", diff(&actual, &expected))
    });

    // ── сверка свойств у ЖИВЫХ объектов ─────────────────────────────────
    let want = golden::port_final(case, "list", &mut report.fixups);
    let list = page.list();
    report.eq(
        format!("{name}: список.status"),
        list.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: список.error"),
        list.error().to_string(),
        str_at(&want, "error"),
    );
    report.eq(
        format!("{name}: список.forwardError"),
        list.forward_error(),
        bool_at(&want, "forwardError"),
    );
    report.eq(
        format!("{name}: список.wpType"),
        list.wp_type().to_string(),
        str_at(&want, "wpType"),
    );
    report.eq(
        format!("{name}: список.searchText"),
        list.search_text().to_string(),
        str_at(&want, "searchText"),
    );
    report.eq(
        format!("{name}: список.skipTypes"),
        strings_of(&list.skip_types()),
        strings_at(&want, "skipTypes"),
    );
    report.eq(
        format!("{name}: список.filterTags"),
        strings_of(&list.filter_tags()),
        strings_at(&want, "filterTags"),
    );
    report.eq(
        format!("{name}: список.skipContentRatings"),
        strings_of(&list.skip_content_ratings()),
        strings_at(&want, "skipContentRatings"),
    );
    report.eq(
        format!("{name}: список.hasActiveFilters"),
        list.has_active_filters(),
        bool_at(&want, "hasActiveFilters"),
    );
    report.eq(
        format!("{name}: список.total"),
        list.total(),
        i64_at(&want, "total") as i32,
    );
    // Правила уезжают в QML списком карт — проверяем ровно то, что он видит.
    for (prop, actual) in [
        ("sorts", variant_json(&list.sorts())),
        ("filters", variant_json(&list.filters())),
        ("filterLogics", variant_json(&list.filter_logics())),
    ] {
        report.eq(format!("{name}: список.{prop}"), actual, want[prop].clone());
    }

    let model = unsafe { &*page.model };
    report.eq(
        format!("{name}: строк в модели"),
        model.count(),
        i64_at(&want, "count") as i32,
    );
    let rows = rows_json(model, &name, report);
    report.eq(
        format!("{name}: строки модели"),
        rows,
        crate::wallpaper_page_golden::expected_rows_v044(&want["rows"]),
    );

    let want = golden::port_final(case, "get", &mut report.fixups);
    let get = page.get();
    report.eq(
        format!("{name}: деталь.status"),
        get.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: деталь.error"),
        get.error().to_string(),
        str_at(&want, "error"),
    );
    report.eq(
        format!("{name}: деталь.wallpaperId"),
        get.wallpaper_id().to_string(),
        str_at(&want, "wallpaperId"),
    );
    report.eq(
        format!("{name}: деталь.wallpaper"),
        variant_json(&get.wallpaper()),
        crate::wallpaper_page_golden::expected_entry_v044(&want["wallpaper"]),
    );

    let want = golden::port_final(case, "apply", &mut report.fixups);
    let apply = page.apply();
    report.eq(
        format!("{name}: применение.status"),
        apply.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: применение.error"),
        apply.error().to_string(),
        str_at(&want, "error"),
    );
    report.eq(
        format!("{name}: применение.rendererName"),
        apply.renderer_name().to_string(),
        str_at(&want, "rendererName"),
    );
    report.eq(
        format!("{name}: применение.rendererId"),
        apply.renderer_id().to_string(),
        str_at(&want, "rendererId"),
    );

    let want = golden::port_final(case, "remove", &mut report.fixups);
    let remove = page.remove();
    report.eq(
        format!("{name}: удаление.status"),
        remove.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: удаление.error"),
        remove.error().to_string(),
        str_at(&want, "error"),
    );

    let want = golden::port_final(case, "scan", &mut report.fixups);
    let scan = page.scan();
    report.eq(
        format!("{name}: пересканирование.status"),
        scan.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: пересканирование.error"),
        scan.error().to_string(),
        str_at(&want, "error"),
    );
    report.eq(
        format!("{name}: пересканирование.count"),
        scan.count(),
        i64_at(&want, "count") as i32,
    );

    let want = golden::port_final(case, "propertySet", &mut report.fixups);
    let property_set = page.property_set();
    report.eq(
        format!("{name}: свойство.status"),
        property_set.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: свойство.error"),
        property_set.error().to_string(),
        str_at(&want, "error"),
    );

    let want = golden::port_final(case, "layout", &mut report.fixups);
    let layout = page.layout();
    report.eq(
        format!("{name}: раскладка.status"),
        layout.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: раскладка.error"),
        layout.error().to_string(),
        str_at(&want, "error"),
    );
    report.eq(
        format!("{name}: раскладка.wallpaperId"),
        layout.wallpaper_id().to_string(),
        str_at(&want, "wallpaperId"),
    );
    report.eq(
        format!("{name}: раскладка.clear"),
        layout.clear(),
        bool_at(&want, "clear"),
    );
    for (prop, actual) in [
        ("fillmode", layout.fillmode()),
        ("locationX", layout.location_x()),
        ("locationY", layout.location_y()),
        ("rotation", layout.rotation()),
    ] {
        report.eq(
            format!("{name}: раскладка.{prop}"),
            actual,
            i64_at(&want, prop) as i32,
        );
    }

    let want = golden::port_final(case, "portal", &mut report.fixups);
    let portal = page.portal();
    report.eq(
        format!("{name}: портал.status"),
        portal.status(),
        i64_at(&want, "status") as i32,
    );
    report.eq(
        format!("{name}: портал.error"),
        portal.error().to_string(),
        str_at(&want, "error"),
    );
    report.eq(
        format!("{name}: портал.wallpaperId"),
        portal.wallpaper_id().to_string(),
        str_at(&want, "wallpaperId"),
    );
    report.eq(
        format!("{name}: портал.uri"),
        portal.uri().to_string(),
        str_at(&want, "uri"),
    );

    if report.failures.len() > failures_before {
        report.diverged.push(name);
    }

    drop(guards);
    page.destroy();
}

/// Развёрнутое расхождение журналов: первая разошедшаяся запись и хвосты.
/// Простыня из двух списков по полсотни записей нечитаема, а разбирать
/// расхождение всё равно приходится с первой точки расхождения.
fn diff(actual: &[Record], expected: &[Record]) -> String {
    let at = (0..actual.len().max(expected.len()))
        .find(|i| actual.get(*i) != expected.get(*i))
        .unwrap_or(0);
    let window = |records: &[Record]| {
        records
            .iter()
            .skip(at.saturating_sub(2))
            .take(6)
            .map(|r| format!("    {} {:?}", r.q, r.sig))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "  разошлось на записи {at} (всего получено {}, ожидалось {})\n  получено:\n{}\n  ожидалось:\n{}",
        actual.len(),
        expected.len(),
        window(actual),
        window(expected)
    )
}

/// Проверки waywallen 0.4.4, которых в голдене нет: он снят с Python-моста
/// до 0.4.4. Скрытие обоев, фильтр «скрытые» у списка, отражение `flip` в
/// раскладке. Сверяется то, что реально уехало байтами, и то, что вернулось
/// в QML сигналами, — на тех же живых объектах, что и голден.
fn run_v044(report: &mut Report) {
    use prost::Message;
    use rexon_core::proto::control as pb;

    crate::discover_queries::reset_tokens();
    crate::discover_queries::set_connected(true);
    report.cases += 1;
    let failures_before = report.failures.len();

    type Frames = Arc<Mutex<Vec<(i64, pb::request::Payload)>>>;
    let decode = |bytes: &[u8]| {
        pb::Request::decode(bytes)
            .ok()
            .and_then(|r| r.payload)
            .expect("кадр запроса разбирается")
    };
    let respond = |payload: pb::response::Payload| {
        let frame = pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(pb::Response {
                status: pb::Status::Ok as i32,
                payload: Some(payload),
                ..Default::default()
            })),
        }
        .encode_to_vec();
        QByteArray::from(&frame[..])
    };

    // ── WallpaperHideQuery ──────────────────────────────────────────────
    let raw_hide = unsafe { rexon_new_wallpaper_hide_query(core::ptr::null_mut()) };
    assert!(!raw_hide.is_null(), "фабрика скрытия вернула nullptr");
    let hide_ptr: *mut WallpaperHideQuery = raw_hide.cast();
    let hide = || pin_of!(hide_ptr);
    let frames: Frames = Arc::new(Mutex::new(Vec::new()));
    let updates: Arc<Mutex<Vec<(Vec<String>, bool, u32)>>> = Arc::new(Mutex::new(Vec::new()));
    let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();
    let sink = Arc::clone(&frames);
    guards.push(hide().on_send_request(move |_, token, _field, request| {
        sink.lock().expect("кадры").push((token, decode(request.as_slice())));
    }));
    let sink = Arc::clone(&updates);
    guards.push(hide().on_updated(move |_, ids, hidden, count| {
        sink.lock().expect("отчёты").push((strings_of(&ids), hidden, count));
    }));

    report.eq("v0.4.4: скрытие.hidden по умолчанию".into(), hide().hidden(), true);
    hide().set_wallpaper_id(QString::from("wp-1"));
    hide().reload();
    hide().set_hidden_for_ids(qstringlist(&["a".into(), String::new(), "b".into()]), false);
    let sent = frames.lock().expect("кадры").clone();
    let want = vec![
        pb::request::Payload::WallpaperHide(pb::WallpaperHideRequest {
            wallpaper_id: "wp-1".into(),
            wallpaper_ids: Vec::new(),
            hidden: true,
        }),
        pb::request::Payload::WallpaperHide(pb::WallpaperHideRequest {
            wallpaper_id: String::new(),
            wallpaper_ids: vec!["a".into(), "b".into()],
            hidden: false,
        }),
    ];
    report.eq(
        "v0.4.4: кадры скрытия".into(),
        format!("{:?}", sent.iter().map(|(_, p)| p).collect::<Vec<_>>()),
        format!("{:?}", want.iter().collect::<Vec<_>>()),
    );
    // Ответы в обратном порядке: отчёт обязан нести то, что просили в
    // ЭТОМ запросе, а не текущие свойства.
    for (token, count) in sent.iter().map(|(t, _)| *t).rev().zip([2u32, 1]) {
        hide().deliver_response(
            token,
            &respond(pb::response::Payload::WallpaperHide(pb::WallpaperHideResponse {
                updated_count: count,
            })),
        );
    }
    report.eq(
        "v0.4.4: отчёты скрытия".into(),
        updates.lock().expect("отчёты").clone(),
        vec![
            (vec!["a".to_string(), "b".to_string()], false, 2),
            (vec!["wp-1".to_string()], true, 1),
        ],
    );
    report.eq("v0.4.4: скрытие.status".into(), hide().status(), 2);
    drop(guards);
    unsafe { rexon_delete_object(raw_hide) };

    // ── отражение в раскладке ───────────────────────────────────────────
    let raw_layout = unsafe { rexon_new_wallpaper_layout_set_query(core::ptr::null_mut()) };
    let layout_ptr: *mut WallpaperLayoutSetQuery = raw_layout.cast();
    let layout = || pin_of!(layout_ptr);
    let frames: Frames = Arc::new(Mutex::new(Vec::new()));
    let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();
    let sink = Arc::clone(&frames);
    guards.push(layout().on_send_request(move |_, token, _field, request| {
        sink.lock().expect("кадры").push((token, decode(request.as_slice())));
    }));
    report.eq(
        "v0.4.4: раскладка.flip по умолчанию (FLIP_NONE)".into(),
        layout().flip(),
        pb::Flip::None as i32,
    );
    layout().set_wallpaper_id(QString::from("wp-2"));
    layout().set_flip(pb::Flip::Horizontal as i32);
    layout().reload();
    let flips: Vec<i32> = frames
        .lock()
        .expect("кадры")
        .iter()
        .filter_map(|(_, p)| match p {
            pb::request::Payload::WallpaperLayoutSet(r) => r.layout.as_ref().map(|l| l.flip),
            _ => None,
        })
        .collect();
    report.eq(
        "v0.4.4: flip в кадре раскладки".into(),
        flips,
        vec![pb::Flip::Horizontal as i32],
    );
    drop(guards);
    unsafe { rexon_delete_object(raw_layout) };

    // ── фильтр «скрытые» у списка и delayReload ─────────────────────────
    let raw_list = unsafe { rexon_new_wallpaper_list_query(core::ptr::null_mut()) };
    let list_ptr: *mut WallpaperListQuery = raw_list.cast();
    let list = || pin_of!(list_ptr);
    let frames: Frames = Arc::new(Mutex::new(Vec::new()));
    let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();
    let sink = Arc::clone(&frames);
    guards.push(list().on_send_request(move |_, token, _field, request| {
        sink.lock().expect("кадры").push((token, decode(request.as_slice())));
    }));
    let hidden_filters = || {
        frames
            .lock()
            .expect("кадры")
            .iter()
            .filter_map(|(_, p)| match p {
                pb::request::Payload::WallpaperList(r) => Some(r.hidden_filter),
                _ => None,
            })
            .collect::<Vec<i32>>()
    };
    list().set_limit(100);
    report.eq("v0.4.4: список.limit хранится".into(), list().limit(), 100);
    report.eq("v0.4.4: limit сам список не грузит".into(), hidden_filters(), Vec::new());
    list().set_hidden_filter(pb::WallpaperHiddenFilter::Only as i32);
    list().delay_reload();
    list().delay_reload();
    rexon_process_events(300);
    report.eq(
        "v0.4.4: hiddenFilter + два delayReload = один кадр «только скрытые»".into(),
        hidden_filters(),
        vec![pb::WallpaperHiddenFilter::Only as i32],
    );
    drop(guards);
    unsafe { rexon_delete_object(raw_list) };

    if report.failures.len() > failures_before {
        report.diverged.push("v0.4.4: скрытие/отражение".into());
    }
}

/// waywallen 0.5.0: страницы и поколения `WallpaperListQuery` на живых
/// объектах — так, как их гоняют `PlaylistEditPage` (`limit: 50`) и сетка
/// «Скрытых обоев» (догрузку просит сама модель через `fetchMore`).
fn run_v050(report: &mut Report) {
    use prost::Message;
    use rexon_core::proto::control as pb;

    crate::discover_queries::reset_tokens();
    crate::discover_queries::set_connected(true);
    report.cases += 1;
    let failures_before = report.failures.len();

    type Frames = Arc<Mutex<Vec<(i64, pb::WallpaperListRequest)>>>;
    let respond = |from: usize, n: usize, count: u32| {
        let wallpapers = (from..from + n)
            .map(|i| pb::WallpaperEntry {
                id: format!("wp-{i}"),
                ..Default::default()
            })
            .collect();
        let frame = pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(pb::Response {
                status: pb::Status::Ok as i32,
                payload: Some(pb::response::Payload::WallpaperList(pb::WallpaperListResponse {
                    wallpapers,
                    count,
                })),
                ..Default::default()
            })),
        }
        .encode_to_vec();
        QByteArray::from(&frame[..])
    };

    let raw_list = unsafe { rexon_new_wallpaper_list_query(core::ptr::null_mut()) };
    let list_ptr: *mut WallpaperListQuery = raw_list.cast();
    let list = || pin_of!(list_ptr);
    let model_ptr: *mut WallpaperListModel = list().data().cast();
    let model = || pin_of!(model_ptr);
    let frames: Frames = Arc::new(Mutex::new(Vec::new()));
    let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();
    let sink = Arc::clone(&frames);
    guards.push(list().on_send_request(move |_, token, _field, request| {
        if let Some(pb::request::Payload::WallpaperList(r)) =
            pb::Request::decode(request.as_slice()).ok().and_then(|r| r.payload)
        {
            sink.lock().expect("кадры").push((token, r));
        }
    }));
    let pages = || {
        frames
            .lock()
            .expect("кадры")
            .iter()
            .map(|(_, r)| (r.page, r.page_size))
            .collect::<Vec<_>>()
    };
    let last_token = || frames.lock().expect("кадры").last().map(|(t, _)| *t).unwrap_or(-1);

    list().set_limit(50);
    rexon_process_events(300);
    report.eq("v0.5.0: limit до первого запуска не грузит".into(), pages(), vec![]);
    list().reload();
    report.eq("v0.5.0: reload — страница 0 размером limit".into(), pages(), vec![(0, 50)]);
    list().deliver_response(last_token(), &respond(0, 50, 120));
    report.eq("v0.5.0: первая страница в модели".into(), model().count(), 50);
    report.eq("v0.5.0: noMore после первой страницы".into(), list().no_more(), false);

    // Как сетка: модель сама просит догрузку, запрос шлёт страницу 1.
    model().fetch_more(&cxx_qt_lib::QModelIndex::default());
    // Пока страница в полёте, повторная просьба — ни модели, ни запросу.
    model().fetch_more(&cxx_qt_lib::QModelIndex::default());
    list().fetch_more(50);
    report.eq(
        "v0.5.0: fetchMore модели — страница 1, повтор в полёте молчит".into(),
        pages(),
        vec![(0, 50), (1, 50)],
    );
    list().deliver_response(last_token(), &respond(50, 50, 120));
    report.eq("v0.5.0: страница дописана вставкой".into(), model().count(), 100);
    report.eq("v0.5.0: offset после догрузки".into(), list().offset(), 1);
    list().fetch_more(50);
    list().deliver_response(last_token(), &respond(100, 20, 120));
    report.eq("v0.5.0: последняя страница".into(), model().count(), 120);
    report.eq("v0.5.0: noMore на последней странице".into(), list().no_more(), true);
    list().fetch_more(50);
    report.eq("v0.5.0: после noMore догрузки нет".into(), pages().len(), 3);

    // Поколение: смена выборки отменяет ответ в полёте, отложенная
    // перезагрузка уходит с новой выборкой и первой страницей.
    list().reload();
    let stale = last_token();
    list().set_search_text(QString::from("море"));
    list().deliver_response(stale, &respond(0, 3, 3));
    report.eq("v0.5.0: устаревший ответ не перебивает список".into(), model().count(), 120);
    rexon_process_events(300);
    let last = frames.lock().expect("кадры").last().cloned();
    report.eq(
        "v0.5.0: перезагрузка с новой выборкой".into(),
        last.map(|(_, r)| (r.search_text, r.page, r.page_size)),
        Some(("море".to_string(), 0, 50)),
    );
    drop(guards);
    unsafe { rexon_delete_object(raw_list) };

    if report.failures.len() > failures_before {
        report.diverged.push("v0.5.0: страницы и поколения".into());
    }
}

/// waywallen 0.5.0: умные плейлисты и поколения запросов плейлистов на
/// живых объектах — так, как их гоняет `PlaylistEditPage` 0.5.0: создание
/// умного плейлиста с `playlistId: 0`, `saveSmart` с выборкой превью,
/// `playlist.smartSelection` обратно в `restoreSelection`.
fn run_v050_playlists(report: &mut Report) {
    use crate::playlist_queries::qobject::{
        rexon_new_playlist_detail_query, rexon_new_playlist_list_query,
        rexon_new_playlist_mutation_query, PlaylistDetailQuery, PlaylistListQuery,
        PlaylistMutationQuery,
    };
    use cxx_qt_lib::QList;
    use prost::Message;
    use rexon_core::proto::control as pb;
    use rexon_core::we_wallpaper;

    crate::discover_queries::reset_tokens();
    crate::discover_queries::set_connected(true);
    report.cases += 1;
    let failures_before = report.failures.len();

    let response = |payload: pb::response::Payload| {
        let frame = pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(pb::Response {
                status: pb::Status::Ok as i32,
                payload: Some(payload),
                ..Default::default()
            })),
        }
        .encode_to_vec();
        QByteArray::from(&frame[..])
    };
    // Выборка с незаполненным правилом и сортировкой без ключа: до демона
    // она доезжает как есть (отвергает её он, как у оригинала).
    let raw = pb::WallpaperSelection {
        filters: vec![
            pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Tag as i32,
                group: 0,
                payload: Some(pb::wallpaper_filter_rule::Payload::TagFilter(
                    pb::WallpaperTagFilter {
                        values: vec!["закат".into()],
                        condition: pb::StringCondition::Is as i32,
                    },
                )),
            },
            pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Tag as i32,
                group: 1,
                payload: None,
            },
        ],
        sorts: vec![pb::WallpaperSortRule { key: 0, direction: 1 }],
        search_text: "море".into(),
        ..Default::default()
    };
    let clean = Some(pb::PlaylistSource {
        kind: Some(pb::playlist_source::Kind::Smart(raw.clone())),
    });
    let smart = pb::PlaylistSummary {
        id: 6,
        name: "Закаты".into(),
        source_kind: "smart".into(),
        revision: 3,
        source: Some(pb::PlaylistSource {
            kind: Some(pb::playlist_source::Kind::Smart(raw.clone())),
        }),
        ..Default::default()
    };
    let curated = pb::PlaylistSummary {
        id: 7,
        name: "Избранное".into(),
        source_kind: "curated".into(),
        entry_ids: vec!["wp-1".into()],
        ..Default::default()
    };
    // `smartSelection` карты: байты, если ключ есть.
    let smart_of = |map: &QVariant| {
        map.value::<QMap<QMapPair_QString_QVariant>>()
            .and_then(|m| m.get(&QString::from("smartSelection")))
            .map(|v| v.value::<QByteArray>().map(|b| b.as_slice().to_vec()))
    };
    let name_of = |map: &QVariant| {
        map.value::<QMap<QMapPair_QString_QVariant>>()
            .and_then(|m| m.get(&QString::from("name")))
            .and_then(|v| v.value::<QString>())
            .map(|v| v.to_string())
    };

    type Sent = Arc<Mutex<Vec<(i64, pb::request::Payload)>>>;
    let sent: Sent = Arc::new(Mutex::new(Vec::new()));
    let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();
    let sink = |sent: &Sent| {
        let sent = Arc::clone(sent);
        move |token: i64, request: &QByteArray| {
            if let Some(p) = pb::Request::decode(request.as_slice()).ok().and_then(|r| r.payload) {
                sent.lock().expect("кадры").push((token, p));
            }
        }
    };
    let count = || sent.lock().expect("кадры").len();
    let last = || sent.lock().expect("кадры").last().cloned();

    // ── Список: поколения и выборка в карте ───────────────────────────────
    let raw_list = unsafe { rexon_new_playlist_list_query(core::ptr::null_mut()) };
    let list_ptr: *mut PlaylistListQuery = raw_list.cast();
    let list = || pin_of!(list_ptr);
    let push = sink(&sent);
    guards.push(list().on_send_request(move |_, token, _, request| push(token, &request)));
    list().reload();
    let stale = last().map(|(t, _)| t).unwrap_or(-1);
    list().reload();
    let fresh = last().map(|(t, _)| t).unwrap_or(-1);
    let listing = |playlists: Vec<pb::PlaylistSummary>| {
        response(pb::response::Payload::PlaylistList(pb::PlaylistListResponse { playlists }))
    };
    list().deliver_response(fresh, &listing(vec![smart.clone(), curated.clone()]));
    list().deliver_response(stale, &listing(vec![]));
    let rows = list().playlists().value::<QList<QVariant>>().unwrap_or_default();
    report.eq("v0.5.0 плейлисты: устаревший ответ списка отброшен".into(), rows.len(), 2);
    let first = rows.get(0).cloned().unwrap_or_default();
    report.eq(
        "v0.5.0 плейлисты: smartSelection в карте списка — выборка демона".into(),
        smart_of(&first),
        Some(Some(we_wallpaper::encode_selection(&raw))),
    );
    report.eq(
        "v0.5.0 плейлисты: у курируемого smartSelection нет".into(),
        smart_of(&rows.get(1).cloned().unwrap_or_default()),
        None,
    );
    report.eq(
        "v0.5.0 плейлисты: прочие ключи карты на месте".into(),
        name_of(&first),
        Some("Закаты".to_string()),
    );

    // ── Подробности: молчание без номера, поколения ───────────────────────
    let raw_detail = unsafe { rexon_new_playlist_detail_query(core::ptr::null_mut()) };
    let detail_ptr: *mut PlaylistDetailQuery = raw_detail.cast();
    let detail = || pin_of!(detail_ptr);
    let push = sink(&sent);
    guards.push(detail().on_send_request(move |_, token, _, request| push(token, &request)));
    let error_signals = Arc::new(Mutex::new(0usize));
    let errors = Arc::clone(&error_signals);
    guards.push(detail().on_error_occurred(move |_, _| *errors.lock().expect("счёт") += 1));
    let before = count();
    detail().reload();
    rexon_process_events(300);
    report.eq(
        "v0.5.0 плейлисты: PlaylistDetailQuery с id 0 — ни кадра, ни ошибки, ни статуса".into(),
        (count() - before, detail().status(), detail().error().to_string(), *error_signals.lock().expect("счёт")),
        (0, 0, String::new(), 0),
    );
    let getting = |p: pb::PlaylistSummary| {
        response(pb::response::Payload::PlaylistGet(pb::PlaylistGetResponse {
            playlist: Some(p),
            wallpapers: vec![],
        }))
    };
    detail().set_playlist_id(7.0);
    rexon_process_events(300);
    let about_7 = last().map(|(t, _)| t).unwrap_or(-1);
    // Плейлист сменился, пока ответ про прежний летит: тот ответ не наш.
    detail().set_playlist_id(6.0);
    detail().deliver_response(about_7, &getting(curated.clone()));
    report.eq(
        "v0.5.0 плейлисты: ответ про прежний плейлист отброшен".into(),
        name_of(&detail().playlist()),
        None,
    );
    rexon_process_events(300);
    let sent_get = last();
    report.eq(
        "v0.5.0 плейлисты: отложенная перезагрузка — про новый плейлист".into(),
        sent_get.as_ref().map(|(_, p)| match p {
            pb::request::Payload::PlaylistGet(r) => r.id,
            _ => -1,
        }),
        Some(6),
    );
    let about_6 = sent_get.map(|(t, _)| t).unwrap_or(-1);
    // Повторный reload — новое поколение: ответ на первый не наш.
    detail().reload();
    let again = last().map(|(t, _)| t).unwrap_or(-1);
    detail().deliver_response(about_6, &getting(curated.clone()));
    report.eq(
        "v0.5.0 плейлисты: ответ на прежний reload подробностей отброшен".into(),
        name_of(&detail().playlist()),
        None,
    );
    detail().deliver_response(again, &getting(smart.clone()));
    report.eq(
        "v0.5.0 плейлисты: smartSelection в playlist подробностей".into(),
        (smart_of(&detail().playlist()), detail().status()),
        (Some(Some(we_wallpaper::encode_selection(&raw))), 2),
    );
    // Назад к нулю (страница создания): молча, статус прежний.
    let before = count();
    detail().set_playlist_id(0.0);
    rexon_process_events(300);
    report.eq(
        "v0.5.0 плейлисты: смена на id 0 — молча".into(),
        (count() - before, detail().status(), *error_signals.lock().expect("счёт")),
        (0, 2, 0),
    );

    // ── saveSmart ─────────────────────────────────────────────────────────
    let raw_mutation = unsafe { rexon_new_playlist_mutation_query(core::ptr::null_mut()) };
    let mutation_ptr: *mut PlaylistMutationQuery = raw_mutation.cast();
    let mutation = || pin_of!(mutation_ptr);
    let push = sink(&sent);
    guards.push(mutation().on_send_request(move |_, token, _, request| push(token, &request)));
    let done = Arc::new(Mutex::new(0usize));
    let tick = Arc::clone(&done);
    guards.push(mutation().on_done(move |_| *tick.lock().expect("счёт") += 1));
    let selection = QVariant::from(&QByteArray::from(&we_wallpaper::encode_selection(&raw)[..]));

    mutation().save_smart(0.0, QString::from("Новый"), 1, 300, true, &selection, 0.0);
    report.eq(
        "v0.5.0 плейлисты: saveSmart(0) — PlaylistCreate с очищенной выборкой, без состава".into(),
        last().map(|(_, p)| p),
        Some(pb::request::Payload::PlaylistCreate(pb::PlaylistCreateRequest {
            name: "Новый".into(),
            mode: 1,
            interval_secs: 300,
            entry_ids: vec![],
            synchronized_selection: true,
            source: clean.clone(),
        })),
    );
    let created = last().map(|(t, _)| t).unwrap_or(-1);
    mutation().deliver_response(
        created,
        &response(pb::response::Payload::PlaylistCreate(pb::PlaylistCreateResponse { id: 42 })),
    );
    report.eq(
        "v0.5.0 плейлисты: createdId и done() после создания".into(),
        (mutation().created_id(), *done.lock().expect("счёт"), mutation().status()),
        (42.0, 1, 2),
    );

    mutation().save_smart(6.0, QString::from("Закаты 2"), 2, 600, false, &selection, 3.0);
    report.eq(
        "v0.5.0 плейлисты: saveSmart(id) — PlaylistUpdate с выборкой и номером правки".into(),
        last().map(|(_, p)| p),
        Some(pb::request::Payload::PlaylistUpdate(pb::PlaylistUpdateRequest {
            id: 6,
            name: "Закаты 2".into(),
            mode: 2,
            interval_secs: 600,
            expected_revision: 3,
            entry_ids: vec![],
            synchronized_selection: false,
            source: clean.clone(),
        })),
    );

    let before = count();
    mutation().save_smart(6.0, QString::from("x"), 1, 0, true, &QVariant::default(), 0.0);
    report.eq(
        "v0.5.0 плейлисты: saveSmart не с выборкой — без кадра, ошибка и done()".into(),
        (
            count() - before,
            mutation().status(),
            mutation().error().to_string().is_empty(),
            *done.lock().expect("счёт"),
        ),
        (0, 3, false, 2),
    );

    drop(guards);
    unsafe {
        rexon_delete_object(raw_mutation);
        rexon_delete_object(raw_detail);
        rexon_delete_object(raw_list);
    }
    if report.failures.len() > failures_before {
        report.diverged.push("v0.5.0: умные плейлисты и поколения".into());
    }
}

/// waywallen 0.5.0: правило редактора фильтров на НАСТОЯЩИХ гаджетах
/// Qt-Protobuf доезжает до ядра целым — для каждого вида правила.
///
/// Случай 24.10.2026: «Название / содержит / zzqxw» в умном плейлисте —
/// предпросмотр показывал все обои, в базу демона легло `filters: []`.
/// Шим гаджетов при развороте правила в JSON читал служебные свойства
/// `intFilter_p`/`tagFilter_p`, а их геттер ПИШЕТ: ставит в oneof пустую
/// ветку и стирает настоящую. Нагрузка любого правила уезжала пустой
/// (`condition: 0`, `value: ""`), и ядро молча выбрасывало его как
/// негодное. Чистый слой этого не видит: у него гаджетов нет. Здесь
/// правило строится ТЕМИ ЖЕ вызовами шима, что модель редактора, в том же
/// порядке, что QML: `applyType` (пустой субфильтр, затем тип числом JS),
/// потом `commitToFilter` (заполненный субфильтр).
fn run_v050_filter_gadgets(report: &mut Report) {
    use crate::wallpaper_queries::qobject::{
        rexon_coerce_gadget, rexon_default_gadget, rexon_gadget_set_by_role,
        rexon_gadget_set_int, rexon_role_names,
    };
    use rexon_core::proto::control as pb;
    use rexon_core::we_settings as ws;
    use serde_json::json;

    report.cases += 1;
    let failures_before = report.failures.len();
    const NS: &str = "waywallen::control::v1::";
    let rule_type = format!("{NS}WallpaperFilterRule");

    match crate::we_env::load_control_types() {
        Ok(so) => println!("REXON-WALLPAPER-SELFTEST типы гаджетов: {so}"),
        Err(why) => {
            // Без настоящих типов проверять нечего — это провал, а не пропуск.
            report.check(false, || format!("v0.5.0 гаджеты фильтров: {why}"));
            report.diverged.push("v0.5.0: правила фильтров на гаджетах".into());
            return;
        }
    }
    let role_of = |type_name: &str, prop: &str| -> i32 {
        rexon_role_names(type_name)
            .iter()
            .find(|(_, name)| name.as_slice() == prop.as_bytes())
            .map(|(role, _)| *role - 257)
            .unwrap_or(-1)
    };
    let field_role = |field: &str| role_of(&rule_type, field);
    let type_role = field_role("type");
    report.check(type_role >= 0, || "у правила нет свойства type".into());

    use pb::wallpaper_filter_rule::Payload;
    let string = |value: &str, condition: i32| {
        Payload::StringFilter(pb::WallpaperStringFilter { value: value.into(), condition })
    };
    let int = |value: i64, condition: i32| {
        Payload::IntFilter(pb::WallpaperIntFilter { value, condition })
    };
    use pb::WallpaperFilterType as T;
    let cases: Vec<(&str, T, Payload)> = vec![
        ("Name/contains", T::Name, string("zzqxw", pb::StringCondition::Contains as i32)),
        ("Type/is", T::WpType, string("scene", pb::StringCondition::Is as i32)),
        ("ContentRating/is not", T::ContentRating, string("Mature", pb::StringCondition::IsNot as i32)),
        ("Library/includes", T::Library, int(2, pb::IntCondition::Equal as i32)),
        ("Library/excludes", T::Library, int(5, pb::IntCondition::EqualNot as i32)),
        ("Width/>", T::Width, int(1920, pb::IntCondition::Greater as i32)),
        ("Height/<", T::Height, int(1080, pb::IntCondition::Less as i32)),
        ("Size/>=", T::Size, int(5_000_000_000, pb::IntCondition::GreaterEqual as i32)),
        (
            "Tag/is",
            T::Tag,
            Payload::TagFilter(pb::WallpaperTagFilter {
                values: vec!["закат".into(), "море".into()],
                condition: pb::StringCondition::Is as i32,
            }),
        ),
    ];
    for (label, kind, payload) in cases {
        let expected = pb::WallpaperFilterRule {
            r#type: kind as i32,
            group: 3,
            payload: Some(payload.clone()),
        };
        // Ветка и субфильтр в виде карты — ровно то, что лежит в гаджете.
        let map = ws::filter_rule_to_map(&expected);
        let (field, sub_map) = map
            .iter()
            .find(|(k, _)| k.ends_with("Filter"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .expect("у правила есть ветка");
        let sub_type = format!("{NS}Wallpaper{}{}", field[..1].to_uppercase(), &field[1..]);
        let empty_sub = rexon_default_gadget(&sub_type);
        let full_sub = rexon_coerce_gadget(&sub_type, &json_to_variant(&sub_map));

        let mut rule = rexon_default_gadget(&rule_type);
        rule = rexon_gadget_set_int(&rule, "group", 3);
        rule = rexon_gadget_set_by_role(&rule, field_role(&field), &empty_sub);
        rule = rexon_gadget_set_by_role(&rule, type_role, &QVariant::from(&(kind as i32 as f64)));
        rule = rexon_gadget_set_by_role(&rule, field_role(&field), &full_sub);

        let first = variant_to_json(&rule);
        let second = variant_to_json(&rule);
        let got = first.as_object().map(ws::map_to_filter_rule);
        report.eq(
            format!("v0.5.0 гаджет правила {label}: доехал до ядра ({first})"),
            got,
            Some(expected.clone()),
        );
        // Разворот не имеет права менять сам гаджет: второй раз — то же.
        report.eq(format!("v0.5.0 гаджет правила {label}: второй разворот"), second, first);

        // Обратный путь (`replaceState` из сохранённой выборки) — тоже целым.
        let back = rexon_coerce_gadget(&rule_type, &json_to_variant(&Value::Object(map.clone())));
        report.eq(
            format!("v0.5.0 гаджет правила {label}: обратный путь"),
            variant_to_json(&back).as_object().map(ws::map_to_filter_rule),
            Some(expected),
        );
    }
    // Без шума: значения карт — числами, служебных `*_p` наружу нет.
    let rule = rexon_coerce_gadget(
        &rule_type,
        &json_to_variant(&json!({"type": 1, "group": 1, "stringFilter": {"value": "a", "condition": 3}})),
    );
    report.eq(
        "v0.5.0 гаджет правила: карта без служебных свойств, числа — числами".into(),
        variant_to_json(&rule),
        json!({"type": 1, "group": 1, "stringFilter": {"value": "a", "condition": 3}}),
    );

    if report.failures.len() > failures_before {
        report.diverged.push("v0.5.0: правила фильтров на гаджетах".into());
    }
}

/// Точка входа стенда. Код возврата — приговор.
pub fn run() -> i32 {
    // Стенду хватает ядра приложения: ни окон, ни QML здесь нет.
    let mut app = cxx_qt_lib::QCoreApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_application_name(&QString::from("Rexon (стенд страницы «Обои»)"));
    }
    // Эталон — на исходном языке (см. `wallpaper_page_golden::tests`).
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));

    let doc = golden::load();
    let cases = doc["cases"].as_array().expect("голден: cases").clone();
    let mut report = Report {
        failures: Vec::new(),
        cases: cases.len(),
        diverged: Vec::new(),
        events: 0,
        fixups: Vec::new(),
    };
    // Ключи записи и их порядок — от них считаются номера ролей модели.
    let keys = golden::entry_keys(&doc);
    report.check(ENTRY_KEYS.to_vec() == keys, || {
        format!("порядок ключей записи разошёлся с эталоном: {ENTRY_KEYS:?} против {keys:?}")
    });

    // ⭐ Пустая выборка = провал, а не успех: голден без случаев прогнал бы
    // перебор НИ РАЗУ и объявил стенд зелёным, не проверив ничего. Тот же
    // класс, на котором попался QML-стенд: сценарий Мастерской шёл на пустой
    // модели и сверял ноль с нулём.
    report.check(!cases.is_empty(), || {
        "в эталоне нет ни одного случая — стенду нечего проверять".to_string()
    });
    for case in &cases {
        run_case(case, &mut report);
    }
    run_v044(&mut report);
    run_v050(&mut report);
    run_v050_playlists(&mut report);
    run_v050_filter_gadgets(&mut report);

    // Правки эталона обязаны быть живыми: если расхождение починили, а
    // поблажку снять забыли, стенд обязан это заметить.
    let want_fixups = golden::FIXUPS.len() + golden::FINAL_FIXUPS.len();
    let applied = report.fixups.clone();
    report.check(applied.len() == want_fixups, || {
        format!(
            "правок эталона применилось {}, а объявлено {want_fixups}: {applied:?}",
            applied.len()
        )
    });

    println!(
        "REXON-WALLPAPER-SELFTEST cases={} passed={} diverged={} events={} fixups={} failures={}",
        report.cases,
        report.cases - report.diverged.len(),
        report.diverged.len(),
        report.events,
        report.fixups.len(),
        report.failures.len()
    );
    for case in &report.diverged {
        println!("REXON-WALLPAPER-SELFTEST-CASE РАЗОШЁЛСЯ {case}");
    }
    for failure in &report.failures {
        println!("REXON-WALLPAPER-SELFTEST-FAIL {failure}");
    }
    let ok = report.failures.is_empty();
    println!(
        "REXON-WALLPAPER-SELFTEST-VERDICT {} diverged={} failures={}",
        if ok { "OK" } else { "FAIL" },
        report.diverged.len(),
        report.failures.len()
    );
    i32::from(!ok)
}
