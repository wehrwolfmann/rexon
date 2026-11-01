//! Страница «Обои» вкладки Wallpaper Engine — порт
//! `ui/src/query/wallpaper_query.cpp` waywallen (MIT, (c) hypengw;
//! атрибуция в NOTICE).
//!
//! Здесь живут: конвертер записи библиотеки (`WallpaperEntry` → карта,
//! которую читает QML), состояние `WallpaperListQuery` со всеми его
//! свойствами (`sorts`/`filters`/`filterLogics`/`skipTypes`/`filterTags`/
//! `skipContentRatings`/`searchText`/`wpType`), сборка запроса
//! `wallpaper_list` и разбор ответа. Тот же конвертер записи обслуживает
//! `WallpaperGetQuery` — панель деталей читает ровно те же ключи.
//!
//! Отдельный модуль (а не хвост `we_query.rs`) по той же границе, что у
//! оригинала: `wallpaper_query.cpp` — своя единица трансляции, и это самая
//! нагруженная страница вкладки.
//!
//! Правила сортировки и фильтров конвертирует `we_settings`
//! (`sort_rule_to_map`/`map_to_sort_rule` и компания): те же самые правила
//! ездят в настройках демона, и движок обязан быть один (правило #12).
//!
//! ⚠️ Что чинит этот порт по сравнению с прежним Python-мостом
//! (правило #16 — баги при переводе исправляем, а не консервируем):
//!
//!   1. Карта записи теряла ТРИ поля: `wallpaperLayoutOverride`,
//!      `wallpaperLayoutOverrideSet` и `supportsItemUnsubscribe`.
//!      `WallpaperDetailPanel.qml` читает все три: без первых двух блок
//!      «Layout override» всегда показывал раскладку по умолчанию, а
//!      кнопка сброса была вечно серой даже у обои с сохранённым
//!      персональным оверрайдом; без третьего действие «Отписаться»
//!      никогда не включалось и не попадало в набор действий панели.
//!   2. Сеттеры `skipTypes`/`filterTags`/`skipContentRatings` не слали
//!      `hasActiveFiltersChanged`, хотя эти списки входят в
//!      `hasActiveFilters`. Подсветка кнопки «Фильтры» обновлялась только
//!      как побочный эффект успешного ответа демона (мост слал сигнал
//!      безусловно в обработчике ответа) — то есть с задержкой, а при
//!      ошибке запроса не обновлялась вовсе.
//!   3. `total` брался как длина полученного списка, а не из поля `count`
//!      ответа. При `page_size = 0` значения совпадают, но контракт у
//!      демона другой: `count` — число записей, подходящих под фильтр.
//!
//! Страницы (waywallen 0.5.0). База `QueryList` QExtra — `offset`/`limit`/
//! `noMore` и `fetchMore`: `reload` просит страницу 0 размером
//! `(offset + 1) * limit`, `fetchMore` — страницу `offset + 1` размером
//! `limit` и дописывает строки в хвост. Конструктор `WallpaperListQuery`
//! ставит `limit = 0`, а это значит «страниц нет, отдать всё»: так живёт
//! главная страница «Обои», и для неё ничего не изменилось. Постранично
//! грузятся предпросмотр умного плейлиста (`PlaylistEditPage`, `limit: 50`
//! и `fetchMore(50)` в конце прокрутки) и «Скрытые обои» (`limit: 100`,
//! догрузку просит сама сетка через `canFetchMore` модели). `sort`/`asc`
//! базы не портированы: `WallpaperListRequest` таких полей не имеет, и
//! `initReqForReload` их не ставит.
//!
//! Выборка (waywallen 0.5.0): `selection` — снимок всех полей, по которым
//! демон отбирает обои (`pb::WallpaperSelection`). QML её не разбирает, а
//! только хранит и отдаёт обратно (`restoreSelection`/`matchesSelection`,
//! позже — `saveSmart` умного плейлиста), поэтому наружу она уходит
//! непрозрачными байтами, см. [`encode_selection`].

use crate::proto::control as pb;
use crate::we_query::{self as q, QuerySignal, QueryState};
use crate::we_settings::{self as s, VMap};
use prost::Message;
use serde_json::Value;

// ── запись библиотеки: `WallpaperEntry` → карта для QML ─────────────────
//
// У оригинала конвертера нет вовсе: список несёт Qt-Protobuf-гаджеты
// `control::v1::WallpaperEntry`, и QML читает их свойства напрямую. Значит
// ключи здесь — имена свойств ГАДЖЕТА, а не поля proto: camelCase
// (`json_name`) плюс одно исключение — поле `id` генератор Qt Protobuf
// называет `id_proto` (`id` в Qt занято). Набор ключей сверен грепом по
// вендоренному `ui/qml`: панель деталей и карточка читают их все.

/// `WallpaperEntry` → карта в терминах QML.
pub fn entry_to_map(e: &pb::WallpaperEntry) -> VMap {
    let mut m = VMap::new();
    m.insert("id_proto".into(), Value::String(e.id.clone()));
    m.insert("name".into(), Value::String(e.name.clone()));
    m.insert("wpType".into(), Value::String(e.wp_type.clone()));
    m.insert("resource".into(), Value::String(e.resource.clone()));
    m.insert("preview".into(), Value::String(e.preview.clone()));
    // Порядок карты proto недетерминирован — сортируем, иначе одна и та же
    // запись то и дело выглядела бы «изменившейся» и дёргала сброс модели.
    let mut meta = VMap::new();
    let mut keys: Vec<&String> = e.metadata.keys().collect();
    keys.sort();
    for k in keys {
        meta.insert(k.clone(), Value::String(e.metadata[k].clone()));
    }
    m.insert("metadata".into(), Value::Object(meta));
    m.insert("size".into(), Value::from(e.size));
    m.insert("width".into(), Value::from(e.width));
    m.insert("height".into(), Value::from(e.height));
    m.insert(
        "contentRating".into(),
        Value::String(e.content_rating.clone()),
    );
    m.insert(
        "tags".into(),
        Value::Array(e.tags.iter().map(|t| Value::String(t.clone())).collect()),
    );
    m.insert(
        "userPropertiesSchema".into(),
        Value::String(e.user_properties_schema.clone()),
    );
    m.insert(
        "userPropertyOverrides".into(),
        Value::String(e.user_property_overrides.clone()),
    );
    m.insert("description".into(), Value::String(e.description.clone()));
    m.insert("externalId".into(), Value::String(e.external_id.clone()));
    // v0.3.5: страница обоины на сайте источника («открыть в браузере»).
    // У локальных обоев пусто.
    m.insert("webUrl".into(), Value::String(e.web_url.clone()));
    // Гаджет отдаёт вложенное сообщение всегда (неустановленное — с полями
    // по умолчанию), и QML полагается на это: он берёт `wallpaperLayout`
    // безусловно, а решает по `wallpaperLayoutOverrideSet`. Поэтому ключ
    // кладём всегда, форма карты не должна прыгать.
    // waywallen 0.4.4: у гаджета раскладки появилось `flip` (отражение),
    // панель деталей читает его как `wallpaperLayout.flip`. Общий
    // `layout_to_map` его не кладёт намеренно: у оригинала
    // `settings_query.cpp` тоже возит раскладку настроек без отражения.
    let mut layout = s::layout_to_map(
        e.wallpaper_layout_override.as_ref().unwrap_or(&DEFAULT_LAYOUT),
    );
    layout.insert(
        "flip".into(),
        Value::from(
            e.wallpaper_layout_override
                .as_ref()
                .map_or(0, |l| l.flip),
        ),
    );
    m.insert("wallpaperLayoutOverride".into(), Value::Object(layout));
    m.insert(
        "wallpaperLayoutOverrideSet".into(),
        Value::Bool(e.wallpaper_layout_override_set),
    );
    m.insert(
        "supportsItemRemove".into(),
        Value::Bool(e.supports_item_remove),
    );
    m.insert(
        "supportsItemUnsubscribe".into(),
        Value::Bool(e.supports_item_unsubscribe),
    );
    // waywallen 0.4.4: обоина скрыта из библиотеки (`wallpaper_hide`).
    // Читают карточка (значок «скрыто»), панель деталей и счётчик
    // `hiddenSelectedCount` хранилища выделения.
    m.insert("hidden".into(), Value::Bool(e.hidden));
    m
}

/// Пустая раскладка для записи без персонального оверрайда.
const DEFAULT_LAYOUT: pb::LayoutPrefs = pb::LayoutPrefs {
    fillmode: 0,
    align: 0,
    rotation: 0,
    location_x: 0,
    location_y: 0,
    location_set: false,
    // waywallen 0.4.4: FLIP_UNSPECIFIED — отражения нет и не задано.
    flip: 0,
};

// ── WallpaperListQuery ──────────────────────────────────────────────────

/// Состояние `WallpaperListQuery`: свойства-фильтры + результат ответа.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WallpaperListData {
    pub wp_type: String,
    pub search_text: String,
    pub sorts: Vec<pb::WallpaperSortRule>,
    pub filters: Vec<pb::WallpaperFilterRule>,
    pub filter_logics: Vec<pb::FilterLogic>,
    pub skip_types: Vec<String>,
    pub filter_tags: Vec<String>,
    pub skip_content_ratings: Vec<String>,
    /// waywallen 0.4.4: `WallpaperHiddenFilter` — 0 прятать скрытые
    /// (умолчание демона), 1 только скрытые, 2 все.
    pub hidden_filter: i32,
    /// waywallen 0.5.0: номера СКРЫТЫХ библиотек (`hiddenLibraryIds`) —
    /// всегда по возрастанию и без повторов, как держит оригинал. Пустой
    /// список — «показать все».
    pub hidden_library_ids: Vec<i64>,
    /// `count` последнего ответа демона (свойство `total`).
    pub total: i32,
    /// `limit` базы `QueryList`: размер страницы. 0 — страниц нет, демон
    /// отдаёт весь список (умолчание конструктора оригинала).
    pub limit: i32,
    /// `offset` базы: номер последней загруженной страницы (0 — только
    /// первая). Сбрасывается любой сменой выборки и каждым `reload`.
    pub offset: i32,
    /// `noMore` базы: дальше страниц нет. Ставит разбор ответа.
    pub no_more: bool,
    /// `m_reload_pending` оригинала: выборка сменилась, отложенный `reload`
    /// ещё не ушёл. Пока он висит, `fetchMore` не догружает — страница
    /// старой выборки легла бы в хвост новой.
    pub reload_pending: bool,
    /// Строки модели библиотеки — карты [`entry_to_map`].
    pub items: Vec<VMap>,
}

impl WallpaperListData {
    /// `hasActiveFilters` оригинала: скрытые библиотеки ИЛИ прочие фильтры
    /// (waywallen 0.5.0). Логика групп в него НЕ входит — сама по себе она
    /// ничего не отсекает.
    pub fn has_active_filters(&self) -> bool {
        !self.hidden_library_ids.is_empty() || self.has_other_filters()
    }

    /// `hasOtherFilters` оригинала (waywallen 0.5.0): всё, кроме скрытых
    /// библиотек. Страница подсвечивает им кнопку «Фильтры», а скрытые
    /// библиотеки — отдельно кнопкой «Менеджер библиотек».
    pub fn has_other_filters(&self) -> bool {
        !self.filters.is_empty()
            || !self.skip_types.is_empty()
            || !self.filter_tags.is_empty()
            || !self.skip_content_ratings.is_empty()
            || self.hidden_filter != 0
    }
}

/// Номера библиотек из значения QML: числа (из QML они приходят
/// double'ами) или строки с числом; остальное отбрасывается. Результат —
/// по возрастанию и без повторов, как `setHiddenLibraryIds` оригинала.
pub fn library_ids(value: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = match value {
        Value::Array(items) => items
            .iter()
            .filter(|v| v.is_number() || v.as_str().is_some_and(|t| t.trim().parse::<i64>().is_ok()))
            .map(s::value_i64)
            .collect(),
        _ => Vec::new(),
    };
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Имена свойств, которые принимает [`set_property`].
pub const LIST_PROPERTIES: &[&str] = &[
    "wpType",
    "searchText",
    "sorts",
    "filters",
    "filterLogics",
    "skipTypes",
    "filterTags",
    "skipContentRatings",
    "hiddenFilter",
    // waywallen 0.5.0
    "hiddenLibraryIds",
];

fn rules<T>(value: &Value, from_map: fn(&VMap) -> T) -> Vec<T> {
    match value {
        Value::Array(items) => items.iter().map(|v| from_map(&s::as_map(v))).collect(),
        _ => Vec::new(),
    }
}

fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => s::string_list(items),
        _ => Vec::new(),
    }
}

/// Записать свойство `WallpaperListQuery` — сеттеры оригинала целиком.
///
/// `None` — имени нет среди [`LIST_PROPERTIES`]. Порядок сигналов ровно
/// как у оригинала: сначала сигнал самого свойства, потом
/// `filterStateChanged` (он один запускает перезагрузку списка на оба
/// изменения — правила и логика групп), и только затем
/// `hasActiveFiltersChanged`, если признак «фильтры включены» перевернулся.
///
/// `setOffset(0)` и `selectionChanged` оригинала здесь не шлются: оба —
/// следствие `invalidateList`, который у оригинала подписан на сигналы
/// свойств выборки. У порта его исполняет мост по тем же сигналам
/// ([`invalidate_list`]), чтобы порядок сигналов самих сеттеров остался
/// прежним.
pub fn set_property(
    d: &mut WallpaperListData,
    name: &str,
    value: &Value,
) -> Option<Vec<QuerySignal>> {
    let mut out = Vec::new();
    let had_other = d.has_other_filters();
    match name {
        "wpType" => q::assign(&mut d.wp_type, s::value_str(value), "wpType", &mut out),
        "searchText" => q::assign(
            &mut d.search_text,
            s::value_str(value),
            "searchText",
            &mut out,
        ),
        "sorts" => q::assign(
            &mut d.sorts,
            rules(value, s::map_to_sort_rule),
            "sorts",
            &mut out,
        ),
        "filters" => {
            let had = d.has_active_filters();
            q::assign(
                &mut d.filters,
                rules(value, s::map_to_filter_rule),
                "filters",
                &mut out,
            );
            if !out.is_empty() {
                out.push(QuerySignal::Property("filterState"));
                push_other_flip(d, had_other, &mut out);
                if had != d.has_active_filters() {
                    out.push(QuerySignal::Property("hasActiveFilters"));
                }
            }
        }
        "filterLogics" => {
            q::assign(
                &mut d.filter_logics,
                rules(value, s::map_to_filter_logic),
                "filterLogics",
                &mut out,
            );
            if !out.is_empty() {
                out.push(QuerySignal::Property("filterState"));
            }
        }
        "skipTypes" | "filterTags" | "skipContentRatings" => {
            let had = d.has_active_filters();
            let (slot, signal) = match name {
                "skipTypes" => (&mut d.skip_types, "skipTypes"),
                "filterTags" => (&mut d.filter_tags, "filterTags"),
                _ => (&mut d.skip_content_ratings, "skipContentRatings"),
            };
            q::assign(slot, strings(value), signal, &mut out);
            if !out.is_empty() {
                push_other_flip(d, had_other, &mut out);
            }
            if !out.is_empty() && had != d.has_active_filters() {
                out.push(QuerySignal::Property("hasActiveFilters"));
            }
        }
        // waywallen 0.4.4: `setHiddenFilter` — тот же порядок, что у
        // списков выше: сигнал свойства, затем `hasActiveFilters`.
        "hiddenFilter" => {
            let had = d.has_active_filters();
            q::assign(&mut d.hidden_filter, s::value_i32(value), "hiddenFilter", &mut out);
            if !out.is_empty() {
                push_other_flip(d, had_other, &mut out);
            }
            if !out.is_empty() && had != d.has_active_filters() {
                out.push(QuerySignal::Property("hasActiveFilters"));
            }
        }
        // waywallen 0.5.0: `setHiddenLibraryIds` — список нормализуется
        // (по возрастанию, без повторов) ДО сравнения, поэтому [3,1,3]
        // поверх [1,3] — молчание. `hasOtherFilters` скрытые библиотеки
        // не трогают по определению.
        "hiddenLibraryIds" => {
            let had = d.has_active_filters();
            q::assign(
                &mut d.hidden_library_ids,
                library_ids(value),
                "hiddenLibraryIds",
                &mut out,
            );
            if !out.is_empty() && had != d.has_active_filters() {
                out.push(QuerySignal::Property("hasActiveFilters"));
            }
        }
        _ => return None,
    }
    Some(out)
}

/// `hasOtherFiltersChanged` — сразу за сигналом самого свойства (и за
/// `filterStateChanged`), до `hasActiveFiltersChanged`: у оригинала он
/// подписан на сигналы свойств, а `hasActiveFiltersChanged` сеттер шлёт
/// последним. Оригинал шлёт его на КАЖДОЕ изменение этих свойств; порт —
/// только когда признак перевернулся (как `hasActiveFilters` и остальные
/// свойства партии): QML читает его привязкой, и холостой сигнал ей не
/// нужен.
fn push_other_flip(d: &WallpaperListData, had_other: bool, out: &mut Vec<QuerySignal>) {
    if had_other != d.has_other_filters() {
        out.push(QuerySignal::Property("hasOtherFilters"));
    }
}

/// `toggleLibraryHidden` оригинала (waywallen 0.5.0): номер был скрыт —
/// снять, не был — добавить; дальше обычный сеттер `hiddenLibraryIds`.
pub fn toggle_library_hidden(d: &mut WallpaperListData, id: i64) -> Vec<QuerySignal> {
    let mut ids = d.hidden_library_ids.clone();
    match ids.iter().position(|v| *v == id) {
        Some(at) => {
            ids.remove(at);
        }
        None => ids.push(id),
    }
    let value = Value::Array(ids.into_iter().map(Value::from).collect());
    set_property(d, "hiddenLibraryIds", &value).unwrap_or_default()
}

/// `replaceFilterState` оригинала: правила и логика групп меняются разом,
/// то есть страница получает ОДИН `filterStateChanged` и одну
/// перезагрузку списка вместо двух. Возвращает `(изменилось ли, сигналы)`.
pub fn replace_filter_state(
    d: &mut WallpaperListData,
    filters: &Value,
    logics: &Value,
) -> (bool, Vec<QuerySignal>) {
    let new_filters = rules(filters, s::map_to_filter_rule);
    let new_logics = rules(logics, s::map_to_filter_logic);
    let filters_changed = d.filters != new_filters;
    let logics_changed = d.filter_logics != new_logics;
    if !filters_changed && !logics_changed {
        return (false, Vec::new());
    }
    let had = d.has_active_filters();
    let had_other = d.has_other_filters();
    d.filters = new_filters;
    d.filter_logics = new_logics;
    let mut out = Vec::new();
    if filters_changed {
        out.push(QuerySignal::Property("filters"));
    }
    if logics_changed {
        out.push(QuerySignal::Property("filterLogics"));
    }
    out.push(QuerySignal::Property("filterState"));
    push_other_flip(d, had_other, &mut out);
    if had != d.has_active_filters() {
        out.push(QuerySignal::Property("hasActiveFilters"));
    }
    (true, out)
}

// ── Выборка (waywallen 0.5.0) ───────────────────────────────────────────

/// `selection()` оригинала: все поля, по которым демон отбирает обои, —
/// ровно в том виде, в каком они лежат в свойствах (без отбора негодных
/// правил: незаполненная строка редактора тоже часть черновика, и
/// `restoreSelection` обязан вернуть её на место).
pub fn selection_of(d: &WallpaperListData) -> pb::WallpaperSelection {
    pb::WallpaperSelection {
        filters: d.filters.clone(),
        filter_logics: d.filter_logics.clone(),
        sorts: d.sorts.clone(),
        search_text: d.search_text.clone(),
        wp_type: d.wp_type.clone(),
        skip_types: d.skip_types.clone(),
        filter_tags: d.filter_tags.clone(),
        skip_content_ratings: d.skip_content_ratings.clone(),
        hidden_filter: d.hidden_filter,
        hidden_library_ids: d.hidden_library_ids.clone(),
    }
}

/// Метка непрозрачного значения выборки.
///
/// Значение уходит в QML байтами (в JS — `ArrayBuffer`): метка + prost-код
/// `pb::WallpaperSelection`. Метка нужна потому, что у пустой выборки
/// prost-код — ноль байт, и без неё «пустая выборка» не отличалась бы от
/// пустой строки или `null`, которые QML охотно приводит к массиву байтов.
/// У оригинала эту роль играет тип гаджета: `canConvert<WallpaperSelection>`
/// ложен для всего, что не выборка, — `matchesSelection(undefined)` даёт
/// false, `restoreSelection(undefined)` ничего не делает.
pub const SELECTION_TAG: &[u8] = b"wwsel1:";

/// Выборка → непрозрачное значение для QML.
pub fn encode_selection(s: &pb::WallpaperSelection) -> Vec<u8> {
    let mut out = SELECTION_TAG.to_vec();
    s.encode(&mut out).expect("Vec<u8> растёт сам");
    out
}

/// Непрозрачное значение из QML → выборка. `None` — это не выборка (нет
/// метки или код не разбирается).
pub fn decode_selection(bytes: &[u8]) -> Option<pb::WallpaperSelection> {
    let body = bytes.strip_prefix(SELECTION_TAG)?;
    pb::WallpaperSelection::decode(body).ok()
}

/// `matchesSelection` оригинала: сравнение СООБЩЕНИЙ, а не байтов — у
/// одного сообщения может быть больше одного кода.
pub fn matches_selection(d: &WallpaperListData, bytes: &[u8]) -> bool {
    decode_selection(bytes).is_some_and(|s| s == selection_of(d))
}

/// `restoreSelection` оригинала: разложить выборку по свойствам через
/// ОБЫЧНЫЕ сеттеры в его порядке (`replaceFilterState`, `sorts`,
/// `searchText`, `wpType`, `skipTypes`, `filterTags`, `skipContentRatings`,
/// `hiddenFilter`, `hiddenLibraryIds`). Совпавшее поле молчит, поэтому
/// восстановление текущей выборки не шлёт ни одного сигнала и не
/// перезагружает список. Не выборка — ничего не делать.
///
/// Значения идут через карты — тем же путём, каким их пишет QML: другого
/// пути «значение → свойство» у порта нет и быть не должно.
pub fn restore_selection(d: &mut WallpaperListData, bytes: &[u8]) -> Vec<QuerySignal> {
    let Some(sel) = decode_selection(bytes) else {
        return Vec::new();
    };
    let maps = |items: Vec<VMap>| Value::Array(items.into_iter().map(Value::Object).collect());
    let texts = |items: &[String]| Value::Array(items.iter().cloned().map(Value::String).collect());
    let filters = maps(sel.filters.iter().map(s::filter_rule_to_map).collect());
    let logics = maps(sel.filter_logics.iter().map(s::filter_logic_to_map).collect());
    let (_, mut out) = replace_filter_state(d, &filters, &logics);
    let steps: [(&str, Value); 8] = [
        ("sorts", maps(sel.sorts.iter().map(s::sort_rule_to_map).collect())),
        ("searchText", Value::String(sel.search_text.clone())),
        ("wpType", Value::String(sel.wp_type.clone())),
        ("skipTypes", texts(&sel.skip_types)),
        ("filterTags", texts(&sel.filter_tags)),
        ("skipContentRatings", texts(&sel.skip_content_ratings)),
        ("hiddenFilter", Value::from(sel.hidden_filter)),
        (
            "hiddenLibraryIds",
            Value::Array(sel.hidden_library_ids.iter().copied().map(Value::from).collect()),
        ),
    ];
    for (name, value) in steps {
        out.extend(set_property(d, name, &value).unwrap_or_default());
    }
    out
}

/// `invalidateList` оригинала — то, что делает ЛЮБАЯ смена выборки:
/// `selectionChanged`, сброс страниц (`setOffset(0)` сеттеров) и отметка
/// «перезагрузка в пути». Поколение и отмену запроса в полёте исполняет
/// мост (у него токены кадров), отложенную перезагрузку — его дебаунс.
pub fn invalidate_list(d: &mut WallpaperListData) -> Vec<QuerySignal> {
    let mut out = vec![QuerySignal::Property("selection")];
    q::assign(&mut d.offset, 0, "offset", &mut out);
    d.reload_pending = true;
    out
}

/// Записать `limit`. Возвращает `(сигналы, перезагрузить ли список)`.
///
/// У оригинала `limitChanged → invalidateList`: смена размера страницы
/// шлёт `selectionChanged` и перезагружает список отложенно. Порт делает
/// это не всегда — осознанное отличие. Вендорный QML пишет `limit`
/// декларативно (`limit: 50` предпросмотра умного плейлиста, `limit: 100`
/// «Скрытых обоев»), то есть В КОНСТРУКТОРЕ, когда список ещё ни разу не
/// грузился. Оригинал на этом отправляет запрос, которого страница не
/// просила: «Скрытые обои» всё равно зовут `delayReload` из
/// `Component.onCompleted`, а редактор плейлиста сам решает, когда
/// грузить (`reload` после `restoreSelection`), — и лишний запрос
/// приходит с ПУСТОЙ выборкой, то есть показывает на миг всю библиотеку.
/// Поэтому до первого запуска (`status == Uninitialized`) смена `limit`
/// только запоминается и шлёт `selectionChanged`, а после — ведёт себя
/// как оригинал: список уже показан, и новая страница должна прийти с
/// новым размером.
pub fn set_limit(
    d: &mut WallpaperListData,
    value: i32,
    status: i32,
) -> (Vec<QuerySignal>, bool) {
    let mut out = Vec::new();
    q::assign(&mut d.limit, value, "limit", &mut out);
    if out.is_empty() {
        return (out, false);
    }
    if status == q::STATUS_UNINITIALIZED {
        out.push(QuerySignal::Property("selection"));
        return (out, false);
    }
    out.extend(invalidate_list(d));
    (out, true)
}

// ── Страницы: reload и fetchMore ────────────────────────────────────────

/// Собрать `WallpaperListRequest` на страницу `page` размером `page_size`.
///
/// Поля выборки — [`selection_of`] как есть, ровно как у `listRequest`
/// оригинала: поля перечислены в проекте ОДИН раз (правило #12), и умный
/// плейлист (`saveSmart`) отправляет ту же выборку.
///
/// ⛔ Правила здесь НЕ отбираются. С 24.10.2026 до этой правки негодные
/// правила молча выкидывались — и когда шим гаджетов портил нагрузку
/// (см. `rexon_gadget_shim.cpp`, `isPointerProperty`), выкидывались ВСЕ:
/// предпросмотр показывал все обои, а плейлист сохранялся без фильтров.
/// Незаполненное правило демон 0.5.0 отвергает (`InvalidArgument`,
/// «invalid wallpaper selection»), и страница показывает его ошибку — так
/// же, как у оригинала.
fn list_request(d: &WallpaperListData, page: u32, page_size: u32) -> pb::WallpaperListRequest {
    let s = selection_of(d);
    pb::WallpaperListRequest {
        wp_type: s.wp_type,
        page,
        page_size,
        filters: s.filters,
        filter_logics: s.filter_logics,
        sorts: s.sorts,
        search_text: s.search_text,
        skip_types: s.skip_types,
        filter_tags: s.filter_tags,
        skip_content_ratings: s.skip_content_ratings,
        // waywallen 0.4.4: какие обои показывать по признаку «скрыта».
        hidden_filter: s.hidden_filter,
        // waywallen 0.5.0: скрытые библиотеки. Сообщение кладётся ВСЕГДА,
        // как у оригинала (`listRequest`), — пустой список значит «показать
        // все». Демон до 0.5.0 поля 14 не знает и пропускает его.
        library_visibility: Some(pb::WallpaperLibraryVisibility {
            hidden_library_ids: s.hidden_library_ids,
        }),
    }
}

/// Начало `reload()`: поколение сменилось, отложенная перезагрузка ушла,
/// страницы сброшены (`setOffset(0)`, `setNoMore(false)` оригинала).
pub fn begin_reload(d: &mut WallpaperListData) -> Vec<QuerySignal> {
    let mut out = Vec::new();
    d.reload_pending = false;
    q::assign(&mut d.offset, 0, "offset", &mut out);
    q::assign(&mut d.no_more, false, "noMore", &mut out);
    out
}

/// Собрать `WallpaperListRequest` для `reload()` и сериализовать.
///
/// `page = 0`, `page_size = (offset + 1) * limit` из `initReqForReload`
/// базы. После [`begin_reload`] `offset` всегда 0, так что размер — ровно
/// `limit`; у главной страницы `limit = 0`, и это «отдать всё» по
/// протоколу.
pub fn encode_list_request(d: &WallpaperListData) -> Vec<u8> {
    let size = (d.offset.max(0) + 1).saturating_mul(d.limit.max(0));
    list_request(d, 0, size as u32).encode_to_vec()
}

/// Собрать кадр `fetchMore()`: страница `offset + 1` размером `limit`
/// (`initReqForFetchMore` базы). `None` — догружать нельзя, запрос не
/// начинается.
///
/// Условия оригинала: дальше страниц нет (`noMore`), смена выборки ждёт
/// своей перезагрузки, или запрос уже в полёте. Четвёртое условие —
/// наше: `limit <= 0`. У оригинала `fetchMore` до первого ответа
/// (`noMore` ещё false) при `limit = 0` просит страницу 1 размером 0, то
/// есть ВЕСЬ список ещё раз, и дописывает его в хвост — библиотека
/// удваивается. Страниц нет — догружать нечего.
pub fn encode_fetch_more_request(d: &WallpaperListData, status: i32) -> Option<Vec<u8>> {
    if d.no_more || d.reload_pending || status == q::STATUS_QUERYING || d.limit <= 0 {
        return None;
    }
    let page = (d.offset.max(0) as u32).saturating_add(1);
    Some(list_request(d, page, d.limit as u32).encode_to_vec())
}

/// Разобрать ответ `wallpaper_list` на `reload()`.
///
/// Сигнал `items` — наш: у оригинала список уезжает в модель через
/// `t->sync(items)` (умная синхронизация хранилища), а у нас модель
/// Qt живёт в шиме, и ей нужен явный повод пересобраться. Шлём его
/// только при РЕАЛЬНОМ изменении списка: сброс модели теряет положение
/// прокрутки и выделение, а демон присылает тот же список после каждого
/// пересканирования библиотеки.
pub fn apply_wallpaper_list(
    st: &mut QueryState,
    data: &mut WallpaperListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::WallpaperList(wl)) = rsp.payload {
        let items: Vec<VMap> = wl.wallpapers.iter().map(entry_to_map).collect();
        q::assign(&mut data.items, items, "items", &mut out);
        q::assign(&mut data.total, wl.count as i32, "total", &mut out);
        let more = more_pages(data, wl.wallpapers.is_empty());
        q::assign(&mut data.no_more, !more, "noMore", &mut out);
    }
    Ok(q::finish(st, out))
}

/// Разобрать ответ `wallpaper_list` на `fetchMore()`: строки страницы
/// дописываются в хвост (сигнал `itemsAppended` — мост дописывает их в
/// модель вставкой, не сбросом: прокрутка и выделение остаются на месте),
/// `offset` становится номером этой страницы.
pub fn apply_wallpaper_page(
    st: &mut QueryState,
    data: &mut WallpaperListData,
    frame_bytes: &[u8],
    page: i32,
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::WallpaperList(wl)) = rsp.payload {
        if !wl.wallpapers.is_empty() {
            data.items.extend(wl.wallpapers.iter().map(entry_to_map));
            out.push(QuerySignal::Property("itemsAppended"));
        }
        q::assign(&mut data.offset, page, "offset", &mut out);
        q::assign(&mut data.total, wl.count as i32, "total", &mut out);
        let more = more_pages(data, wl.wallpapers.is_empty());
        q::assign(&mut data.no_more, !more, "noMore", &mut out);
    }
    Ok(q::finish(st, out))
}

/// `more` оригинала: строк меньше, чем подходит под выборку, и последняя
/// страница не пустая (пустая значит, что демон дальше не даст ничего,
/// даже если `count` говорит иное).
fn more_pages(d: &WallpaperListData, page_empty: bool) -> bool {
    (d.items.len() as i64) < i64::from(d.total) && !page_empty
}

// ── WallpaperGetQuery (панель деталей) ──────────────────────────────────

/// Свойство `wallpaper` запроса `WallpaperGetQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WallpaperGetData {
    pub wallpaper: VMap,
}

/// Разобрать ответ `wallpaper_get`.
///
/// Ответ без записи трактуем как оригинал — записью с полями по умолчанию:
/// у Qt-Protobuf чтение неустановленного сообщения отдаёт именно её. Для
/// панели это значит «нет свежих данных» — она сама падает обратно на
/// строку списка (`id_proto` пустой).
pub fn apply_wallpaper_get(
    st: &mut QueryState,
    data: &mut WallpaperGetData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::WallpaperGet(wg)) = rsp.payload {
        let m = entry_to_map(&wg.entry.unwrap_or_default());
        q::assign(&mut data.wallpaper, m, "wallpaper", &mut out);
    }
    Ok(q::finish(st, out))
}

// ── WallpaperApplyQuery: плейлисты, остановленные применением ──────────

/// Список плейлистов, которые демон остановил, применяя обою, — в том
/// виде, в каком его читает панель деталей (`WallpaperDetailPanel.qml`,
/// `toastStoppedPlaylists`): по карте на плейлист с ключами `playlistId`,
/// `playlistName`, `allDisplays` и `displayIds`.
///
/// Применение одной обои на дисплей, где шёл плейлист, плейлист снимает;
/// молча это делать нельзя — человек увидел бы, что смена обоев по
/// расписанию просто перестала работать. Пустой список — «ничего не
/// остановлено».
pub fn stopped_playlists(rsp: &pb::WallpaperApplyResponse) -> Value {
    Value::Array(
        rsp.stopped_playlists
            .iter()
            .map(|p| {
                serde_json::json!({
                    "playlistId": p.playlist_id,
                    "playlistName": p.playlist_name,
                    "allDisplays": p.all_displays,
                    "displayIds": p.display_ids,
                })
            })
            .collect(),
    )
}

// ── WallpaperLayoutSetQuery (раскладка отдельной обои) ──────────────────

/// Свойства `WallpaperLayoutSetQuery`.
///
/// Раскладка задаётся ОДНОЙ обои и перекрывает общую (`layout_defaults`
/// настроек). Панель деталей пишет сюда положение, вписывание и поворот,
/// а сброс шлёт отдельным признаком `clear` — тогда тело раскладки не
/// отправляется вовсе и демон снимает персональный оверрайд.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutSetData {
    pub wallpaper_id: String,
    pub clear: bool,
    pub fillmode: i32,
    pub location_x: i32,
    pub location_y: i32,
    pub rotation: i32,
    /// waywallen 0.4.4: отражение (`Flip`). Умолчание оригинала —
    /// `FLIP_NONE` (1); его ставит конструктор Qt-обёртки.
    pub flip: i32,
}

/// Имена свойств, которые принимает [`set_layout_property`].
pub const LAYOUT_SET_PROPERTIES: &[&str] = &[
    "wallpaperId",
    "clear",
    "fillmode",
    "locationX",
    "locationY",
    "rotation",
    "flip",
];

/// Записать свойство `WallpaperLayoutSetQuery`.
///
/// У оригинала все шесть сеттеров — один макрос `WW_SET`: присвоить и, если
/// значение ДЕЙСТВИТЕЛЬНО изменилось, послать общий `paramsChanged`.
/// Отдельных сигналов на свойство нет, поэтому имя сигнала здесь одно —
/// `params`. `None` — имени нет среди [`LAYOUT_SET_PROPERTIES`].
pub fn set_layout_property(
    d: &mut LayoutSetData,
    name: &str,
    value: &Value,
) -> Option<Vec<QuerySignal>> {
    let mut out = Vec::new();
    match name {
        "wallpaperId" => q::assign(
            &mut d.wallpaper_id,
            s::value_str(value),
            "params",
            &mut out,
        ),
        "clear" => q::assign(&mut d.clear, s::value_bool(value), "params", &mut out),
        "fillmode" => q::assign(&mut d.fillmode, s::value_i32(value), "params", &mut out),
        "locationX" => q::assign(&mut d.location_x, s::value_i32(value), "params", &mut out),
        "locationY" => q::assign(&mut d.location_y, s::value_i32(value), "params", &mut out),
        "rotation" => q::assign(&mut d.rotation, s::value_i32(value), "params", &mut out),
        "flip" => q::assign(&mut d.flip, s::value_i32(value), "params", &mut out),
        _ => return None,
    }
    Some(out)
}

/// Собрать `WallpaperLayoutSetRequest`.
///
/// `None` — обои не выбрана: оригинал в этом случае выходит из `reload()`
/// ДО перевода статуса, то есть запрос не начинается вовсе.
///
/// Тело раскладки уходит только при `clear == false`. `location_set`
/// всегда `true`: положение задаётся вместе с остальным, отдельного
/// переключателя «своё положение» в панели деталей нет. Проценты режем в
/// 0…100 — как оригинал, дублируя ограничение QML (страница может
/// прислать что угодно).
///
/// `align` не заполняем: у оригинала его тут нет, и в панели нет органа
/// управления выравниванием. Поле остаётся значением по умолчанию.
pub fn encode_layout_set(d: &LayoutSetData) -> Option<Vec<u8>> {
    if d.wallpaper_id.is_empty() {
        return None;
    }
    let layout = if d.clear {
        None
    } else {
        Some(pb::LayoutPrefs {
            fillmode: d.fillmode,
            rotation: d.rotation,
            location_x: d.location_x.clamp(0, 100) as u32,
            location_y: d.location_y.clamp(0, 100) as u32,
            location_set: true,
            flip: d.flip,
            ..Default::default()
        })
    };
    Some(
        pb::WallpaperLayoutSetRequest {
            wallpaper_id: d.wallpaper_id.clone(),
            clear: d.clear,
            layout,
        }
        .encode_to_vec(),
    )
}


// ── WallpaperHideQuery (скрыть обои, waywallen 0.4.4) ───────────────────

/// Непустые идентификаторы в исходном порядке — `setHiddenForIds`
/// оригинала отсеивает пустые до всего остального.
pub fn non_empty_ids(ids: &[String]) -> Vec<String> {
    ids.iter().filter(|id| !id.is_empty()).cloned().collect()
}

/// Собрать `WallpaperHideRequest`.
///
/// `None` — после отсева пустых не осталось ни одного идентификатора:
/// оригинал в этом случае выходит ДО перевода статуса, запроса нет вовсе.
/// Единственный идентификатор уходит в `wallpaper_id`, набор — в
/// `wallpaper_ids`, ровно как у оригинала.
pub fn encode_hide_request(ids: &[String], hidden: bool) -> Option<Vec<u8>> {
    let ids = non_empty_ids(ids);
    let request = match ids.as_slice() {
        [] => return None,
        [one] => pb::WallpaperHideRequest {
            wallpaper_id: one.clone(),
            hidden,
            ..Default::default()
        },
        _ => pb::WallpaperHideRequest {
            wallpaper_ids: ids,
            hidden,
            ..Default::default()
        },
    };
    Some(request.encode_to_vec())
}
