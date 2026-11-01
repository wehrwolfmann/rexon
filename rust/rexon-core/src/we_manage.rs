//! Разбор ответов «управляющих» запросов вкладки Wallpaper Engine:
//! плагины, плейлисты, источники и библиотеки, подписки источников,
//! действия плагинов и правка их настроек.
//!
//! Здесь только Qt-свободная часть: кадр демона → карты/списки + список сигналов
//! в порядке оригинала. Оболочки `#[qobject]` живут в `rexon-gui`.
//!
//! ⚠️ Эталон — ОРИГИНАЛ (waywallen `ui/src/query/` на
//! коммите вендора `e5626c1`, он же v0.2.6+9), а не наш Python-мост.
//! Расхождения, которые перенос «как было» законсервировал бы, чинятся
//! здесь и перечислены у каждой функции.
//!
//! Отдельно про сортировку списка плагинов: оригинал сравнивает имена
//! `QString::localeAwareCompare`, то есть коллацией ICU. В ядре Qt нет и
//! быть не должно, поэтому сравнение идёт по `to_lowercase()` — ровно
//! то же, что делал Python-мост. Для латинских имён плагинов
//! (`org.waywallen.*`) результат совпадает; расхождение возможно только
//! на именах с диакритикой, и тащить ради него ICU в ядро дороже пользы.

use serde_json::{json, Map, Value};

use crate::proto::control as pb;
use crate::we_query::{
    self, finish, take_response, QuerySignal, QueryState, SettingSchema, STATUS_ERROR,
    STATUS_QUERYING,
};

/// Карта `SettingSchema` для QML.
///
/// ⚠️ Ключи SNAKE_CASE — `SettingField.qml` написан против
/// `default_value`/`label_key`/`description_key` оригинала.
fn setting_schema_map(s: &SettingSchema) -> Value {
    json!({
        "key": s.key,
        "type": s.value_type,
        "default_value": s.default_value,
        "identity": s.identity,
        "label_key": s.label_key,
        "description_key": s.description_key,
        // Переводимые подписи каталога плагина: `SettingField.qml` читает
        // `label`/`description`, заголовок группы — `group_label`.
        "label": s.label,
        "description": s.description,
        "group_label": s.group_label,
        "min": s.min,
        "max": s.max,
        "step": s.step,
        "choices": s.choices,
        "group": s.group,
        "order": s.order,
    })
}

/// `RendererPluginInfo` → карта (`renderer_to_map` оригинала + наши
/// `bin`/`priority`/`pluginId`; лишние ключи QML не мешают).
fn renderer_plugin_map(r: pb::RendererPluginInfo) -> Value {
    let settings: Vec<Value> = r
        .settings
        .into_iter()
        .map(|s| setting_schema_map(&SettingSchema::from(s)))
        .collect();
    json!({
        "name": r.name,
        "bin": r.bin,
        "types": r.types,
        "priority": r.priority,
        "version": r.version,
        "settings": settings,
        "pluginId": r.plugin_id,
    })
}

/// `PluginUpdateInfo` → карта (`plugin_update_to_map` оригинала).
fn plugin_update_map(u: &pb::PluginUpdateInfo) -> Value {
    json!({
        "pluginId": u.plugin_id,
        "state": u.state,
        "latestVersion": u.latest_version,
        "zipUrl": u.zip_url,
        "sha256": u.sha256,
        "error": u.error,
        "checkedAtMs": u.checked_at_ms,
    })
}

/// `PluginCompat` → карта (`plugin_compat_to_map` оригинала, waywallen
/// 0.4.2): годится ли установленный плагин этому демону и, если нет, —
/// почему, словами самого демона. Как у оригинала, отсутствие сообщения
/// читается умолчаниями protobuf (`compatible: false`, пустая причина).
fn plugin_compat_map(c: &pb::PluginCompat) -> Value {
    json!({
        "compatible": c.compatible,
        "reason": c.reason,
    })
}

// ════════════════════════════════════════════════════════════════════════
// PluginListQuery
// ════════════════════════════════════════════════════════════════════════

/// Свойства `PluginListQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginListData {
    pub plugins: Vec<Value>,
    pub inactive_system: Vec<String>,
    pub inactive_user: Vec<String>,
}

/// Ключ сортировки оригинала: пользовательские раньше системных, внутри —
/// по имени (пустое имя заменяется идентификатором).
fn plugin_sort_key(m: &Value) -> (bool, String) {
    let system = m.get("system").and_then(Value::as_bool).unwrap_or(false);
    let name = m.get("name").and_then(Value::as_str).unwrap_or_default();
    let id = m.get("id").and_then(Value::as_str).unwrap_or_default();
    let label = if name.is_empty() { id } else { name };
    (system, label.to_lowercase())
}

/// Разобрать ответ `plugin_list`.
pub fn apply_plugin_list(
    st: &mut QueryState,
    data: &mut PluginListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::PluginList(pl)) = rsp.payload {
        let mut items: Vec<Value> = pl
            .plugins
            .into_iter()
            .map(|p| {
                let update_info = plugin_update_map(&p.update_info.unwrap_or_default());
                let renderers: Vec<Value> =
                    p.renderers.into_iter().map(renderer_plugin_map).collect();
                json!({
                    "id": p.id,
                    "name": p.name,
                    "version": p.version,
                    "update": p.update,
                    "hasSource": p.has_source,
                    "system": p.system,
                    "section": if p.system { "system" } else { "user" },
                    "updateInfo": update_info,
                    "compat": plugin_compat_map(&p.compat.unwrap_or_default()),
                    "renderers": renderers,
                })
            })
            .collect();
        items.sort_by(|a, b| plugin_sort_key(a).cmp(&plugin_sort_key(b)));
        // Один сигнал на три свойства — так у оригинала (`pluginsChanged`
        // объявлен NOTIFY для всех трёх).
        let changed = data.plugins != items
            || data.inactive_system != pl.inactive_system
            || data.inactive_user != pl.inactive_user;
        data.plugins = items;
        data.inactive_system = pl.inactive_system;
        data.inactive_user = pl.inactive_user;
        if changed {
            out.push(QuerySignal::Property("plugins"));
        }
    }
    Ok(finish(st, out))
}

// ════════════════════════════════════════════════════════════════════════
// PluginInstallQuery / PluginInspectQuery / PluginDeleteQuery
// ════════════════════════════════════════════════════════════════════════

/// Свойства `PluginInstallQuery` и `PluginDeleteQuery` — они одинаковы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginIdResult {
    pub plugin_id: String,
    pub needs_restart: bool,
}

/// Разобрать ответ `plugin_install`.
///
/// Сигналы: `resultChanged`, затем `installed(pluginId, needsRestart)` —
/// оба БЕЗУСЛОВНЫЕ, как у оригинала: они означают «демон ответил», а не
/// «значение стало другим».
pub fn apply_plugin_install(
    st: &mut QueryState,
    data: &mut PluginIdResult,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::PluginInstall(r)) = rsp.payload {
        data.plugin_id = r.plugin_id;
        data.needs_restart = r.needs_restart;
        out.push(QuerySignal::Property("result"));
        out.push(QuerySignal::Property("installed"));
    }
    Ok(finish(st, out))
}

/// Разобрать ответ `plugin_delete`.
pub fn apply_plugin_delete(
    st: &mut QueryState,
    data: &mut PluginIdResult,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::PluginDelete(r)) = rsp.payload {
        data.plugin_id = r.plugin_id;
        data.needs_restart = r.needs_restart;
        out.push(QuerySignal::Property("result"));
        out.push(QuerySignal::Property("deleted"));
    }
    Ok(finish(st, out))
}

/// Свойства `PluginInspectQuery` — весь ответ целиком.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginInspectData {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub update: String,
    pub has_source: bool,
    pub renderers: Vec<String>,
    pub overwrite: bool,
    pub existing_version: String,
    pub existing_name: String,
    pub existing_system: bool,
}

/// Разобрать ответ `plugin_inspect`.
pub fn apply_plugin_inspect(
    st: &mut QueryState,
    data: &mut PluginInspectData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::PluginInspect(r)) = rsp.payload {
        *data = PluginInspectData {
            plugin_id: r.plugin_id,
            name: r.name,
            version: r.version,
            update: r.update,
            has_source: r.has_source,
            renderers: r.renderers,
            overwrite: r.overwrite,
            existing_version: r.existing_version,
            existing_name: r.existing_name,
            existing_system: r.existing_system,
        };
        out.push(QuerySignal::Property("result"));
        out.push(QuerySignal::Property("inspected"));
    }
    Ok(finish(st, out))
}

// ════════════════════════════════════════════════════════════════════════
// Прогресс длинных задач (ProgressQuery оригинала)
// ════════════════════════════════════════════════════════════════════════
//
// ⚠️ Починка при порте (правило #16). У оригинала проверка и установка
// обновлений плагина наследуются от `ProgressQuery`: RPC лишь ПРИНИМАЕТ
// задачу и отдаёт `query_id`, а идёт она дальше событиями `task_progress`
// демона — оттуда и `progress`/`progressing`, и настоящий конец работы.
// Наш Python-мост этого класса не имел вовсе:
//
//   * свойств `queryId`/`progress`/`progressing` не было, а вендорный QML
//     их читает (`PluginManagePage.qml:499,501`) — привязки индикатора
//     обновления были мёртвыми;
//   * `installed`/`checked` слались по ПРИЁМУ задачи, поэтому тост
//     «Обновлён "X"» появлялся сразу, а не когда обновление встало;
//   * событие `task_progress` не разбиралось ни в `we_notify`, ни где-либо
//     ещё — весь канал прогресса пропадал в никуда.

/// Снимок прогресса одной задачи демона (`TaskProgressSnapshot`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskProgressSnapshot {
    pub progress: f64,
    pub progressing: bool,
    pub ended: bool,
    pub error: bool,
    pub message: String,
    /// Монотонный номер: снимок, пришедший ДО начала запроса, не имеет к
    /// нему отношения и должен игнорироваться.
    pub sequence: u64,
}

/// Склад снимков прогресса — порт полей `m_task_progress`/
/// `m_task_progress_sequence` синглтона `Notify` оригинала.
///
/// Живёт в ядре, а не в оболочке: он нужен и разбору события, и запросам,
/// а второй копии быть не должно (правило #12).
pub mod progress_store {
    use super::TaskProgressSnapshot;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    #[derive(Default)]
    struct Store {
        sequence: u64,
        items: HashMap<String, TaskProgressSnapshot>,
    }

    fn store() -> &'static Mutex<Store> {
        static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(Store::default()))
    }

    /// Записать снимок, выдав ему следующий номер. Возвращает записанное.
    pub fn record(
        query_id: &str,
        progress: f64,
        progressing: bool,
        ended: bool,
        error: bool,
        message: &str,
    ) -> TaskProgressSnapshot {
        let mut s = store().lock().expect("склад прогресса не отравлен");
        s.sequence += 1;
        let snapshot = TaskProgressSnapshot {
            progress,
            progressing,
            ended,
            error,
            message: message.to_string(),
            sequence: s.sequence,
        };
        s.items.insert(query_id.to_string(), snapshot.clone());
        snapshot
    }

    /// Текущий номер (порт `taskProgressSequence()`).
    pub fn sequence() -> u64 {
        store()
            .lock()
            .expect("склад прогресса не отравлен")
            .sequence
    }

    /// Снимок по идентификатору задачи (порт `taskProgressSnapshot()`).
    pub fn snapshot(query_id: &str) -> Option<TaskProgressSnapshot> {
        store()
            .lock()
            .expect("склад прогресса не отравлен")
            .items
            .get(query_id)
            .cloned()
    }

    /// Забыть всё — нужно только проверкам, чтобы случаи не влияли друг
    /// на друга.
    pub fn reset() {
        let mut s = store().lock().expect("склад прогресса не отравлен");
        s.sequence = 0;
        s.items.clear();
    }
}

/// Состояние `ProgressQuery` поверх состояния базы `Query`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressState {
    pub query_id: String,
    pub progress: f64,
    pub progressing: bool,
    /// Номер склада на момент `beginProgressQuery` — всё, что было раньше,
    /// к этому запросу не относится.
    pub begin_sequence: u64,
    /// Текст последнего `progressEnded`. Отдельно от `QueryState::error`:
    /// у оригинала сигнал несёт `snapshot.message`, а он приходит и при
    /// успешном завершении, где ошибки нет вовсе.
    pub last_message: String,
}

/// `progressEnded(error, message)` — отдельным вариантом его не выразить,
/// поэтому оболочка получает его как `Property("progressEnded")`, а текст
/// берёт из `ProgressState::last_message`, признак ошибки — из статуса.
pub const PROGRESS_ENDED: &str = "progressEnded";

fn set_progress_value(ps: &mut ProgressState, value: f64, out: &mut Vec<QuerySignal>) {
    let value = value.clamp(0.0, 1.0);
    // qFuzzyCompare(m_progress + 1.0, progress + 1.0) оригинала — то же
    // самое сравнение с поправкой на около-нулевые значения.
    if (ps.progress - value).abs() <= f64::EPSILON * 1e5 {
        return;
    }
    ps.progress = value;
    out.push(QuerySignal::Property("progress"));
}

fn set_progressing_value(ps: &mut ProgressState, value: bool, out: &mut Vec<QuerySignal>) {
    if ps.progressing == value {
        return;
    }
    ps.progressing = value;
    out.push(QuerySignal::Property("progressing"));
}

/// `beginProgressQuery()`: запомнить номер склада, обнулить прогресс и
/// перевести базу в Querying.
pub fn begin_progress(st: &mut QueryState, ps: &mut ProgressState) -> Vec<QuerySignal> {
    let mut out = Vec::new();
    ps.begin_sequence = progress_store::sequence();
    if !ps.query_id.is_empty() {
        ps.query_id.clear();
        out.push(QuerySignal::Property("queryId"));
    }
    out.extend(we_query::set_error(st, ""));
    set_progress_value(ps, 0.0, &mut out);
    set_progressing_value(ps, true, &mut out);
    out.extend(we_query::set_status(st, STATUS_QUERYING));
    out
}

/// `failProgressQuery(message)`.
pub fn fail_progress(st: &mut QueryState, ps: &mut ProgressState, message: &str) -> Vec<QuerySignal> {
    let mut out = Vec::new();
    set_progressing_value(ps, false, &mut out);
    ps.last_message = message.to_string();
    out.extend(we_query::set_error(st, message));
    out.extend(we_query::set_status(st, STATUS_ERROR));
    out.push(QuerySignal::Property(PROGRESS_ENDED));
    out
}

/// `applyTaskProgress(snapshot)`.
pub fn apply_task_progress(
    st: &mut QueryState,
    ps: &mut ProgressState,
    snapshot: &TaskProgressSnapshot,
) -> Vec<QuerySignal> {
    let mut out = Vec::new();
    set_progress_value(ps, snapshot.progress, &mut out);
    set_progressing_value(ps, snapshot.progressing, &mut out);
    if !snapshot.ended {
        return out;
    }
    ps.last_message = snapshot.message.clone();
    if snapshot.error {
        out.extend(we_query::set_error(st, &snapshot.message));
        out.extend(we_query::set_status(st, STATUS_ERROR));
    } else {
        out.extend(we_query::set_status(st, we_query::STATUS_FINISHED));
    }
    out.push(QuerySignal::Property(PROGRESS_ENDED));
    out
}

/// `acceptProgressQuery(queryId)`: пустой идентификатор — сразу отказ;
/// иначе запомнить и, если снимок уже пришёл (демон мог опередить ответ),
/// применить его немедленно.
pub fn accept_progress(
    st: &mut QueryState,
    ps: &mut ProgressState,
    query_id: &str,
) -> Vec<QuerySignal> {
    if query_id.is_empty() {
        return fail_progress(st, ps, "empty progress query id");
    }
    let mut out = Vec::new();
    if ps.query_id != query_id {
        ps.query_id = query_id.to_string();
        out.push(QuerySignal::Property("queryId"));
    }
    if let Some(snapshot) = progress_store::snapshot(query_id) {
        if snapshot.sequence > ps.begin_sequence {
            out.extend(apply_task_progress(st, ps, &snapshot));
        }
    }
    out
}

/// Событие `task_progress` пришло — относится ли оно к этому запросу.
///
/// Порт условия лямбды конструктора `ProgressQuery`: пустой `queryId`,
/// чужой `queryId` и снимок старше начала запроса пропускаются.
pub fn progress_matches(ps: &ProgressState, query_id: &str, sequence: u64) -> bool {
    !ps.query_id.is_empty() && ps.query_id == query_id && sequence > ps.begin_sequence
}

/// Свойства `PluginUpdateCheckQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginUpdateCheckData {
    pub updates: Vec<Value>,
}

/// Разобрать ответ `plugin_update_check`: это ПРИЁМ задачи, а не её конец.
pub fn apply_plugin_update_check(
    st: &mut QueryState,
    ps: &mut ProgressState,
    data: &mut PluginUpdateCheckData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        // Оригинал на отказе зовёт failProgressQuery — статус Error уже
        // выставлен take_response, осталось снять признак работы и
        // сообщить о конце.
        let mut ended = Vec::new();
        set_progressing_value(ps, false, &mut ended);
        ps.last_message = st.error.clone();
        ended.push(QuerySignal::Property(PROGRESS_ENDED));
        out.extend(ended);
        return Ok(out);
    };
    let mut query_id = String::new();
    if let Some(pb::response::Payload::PluginUpdateCheck(r)) = rsp.payload {
        let updates: Vec<Value> = r.updates.iter().map(plugin_update_map).collect();
        if data.updates != updates {
            data.updates = updates;
        }
        // Сигнал безусловный — так у оригинала.
        out.push(QuerySignal::Property("updates"));
        query_id = r.query_id;
    }
    out.extend(accept_progress(st, ps, &query_id));
    Ok(out)
}

/// Разобрать ответ `plugin_update_install` — тоже приём задачи.
pub fn apply_plugin_update_install(
    st: &mut QueryState,
    ps: &mut ProgressState,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        let mut ended = Vec::new();
        set_progressing_value(ps, false, &mut ended);
        ps.last_message = st.error.clone();
        ended.push(QuerySignal::Property(PROGRESS_ENDED));
        out.extend(ended);
        return Ok(out);
    };
    let query_id = match rsp.payload {
        Some(pb::response::Payload::PluginUpdateInstall(r)) => r.query_id,
        _ => String::new(),
    };
    out.extend(accept_progress(st, ps, &query_id));
    Ok(out)
}

/// Разобрать событие демона: если это `task_progress` — записать снимок.
///
/// Возвращает `(query_id, снимок)`; `None` — кадр не про прогресс.
pub fn take_task_progress(frame_bytes: &[u8]) -> Option<(String, TaskProgressSnapshot)> {
    use prost::Message;
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let pb::server_frame::Kind::Event(event) = frame.kind? else {
        return None;
    };
    let pb::event::Payload::TaskProgress(p) = event.payload? else {
        return None;
    };
    let snapshot = progress_store::record(
        &p.query_id,
        p.progress as f64,
        p.progressing,
        p.ended,
        p.error,
        &p.message,
    );
    Some((p.query_id, snapshot))
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistListQuery / PlaylistMutationQuery
// ════════════════════════════════════════════════════════════════════════

/// `PlaylistSummary` → карта в терминах QML (`playlist_to_map` оригинала).
///
/// ⚠️ Ключ `sourceKind` — наш, у оригинала его в карте нет. Оставлен:
/// лишний ключ вендорному QML не мешает, а страница плейлистов Rexon
/// по нему отличает «умные» плейлисты от обычных.
///
/// Одна точка на весь файл (правило #12): по этой же карте живут и список
/// плейлистов, и подробности одного — расходиться им нельзя, иначе
/// `revision` доехал бы до одной страницы и потерялся у другой.
pub fn playlist_summary_map(p: &pb::PlaylistSummary) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "sourceKind": p.source_kind,
        "mode": p.mode,
        "intervalSecs": p.interval_secs,
        "itemCount": p.item_count,
        "entryIds": p.entry_ids,
        // Поля протокола 0.3.7. `revision` — сторож гонки правок: страница
        // возвращает его демону в `expected_revision`. `synchronizedSelection`
        // — переключатель формы правки плейлиста.
        "revision": p.revision,
        "synchronizedSelection": p.synchronized_selection,
    })
}

/// Ключ карты плейлиста с выборкой умного плейлиста (waywallen 0.5.0,
/// `playlist_to_map` оригинала: ключ есть только у умного плейлиста).
pub const SMART_SELECTION_KEY: &str = "smartSelection";

/// Выборка умного плейлиста из его сводки; `None` — плейлист курируемый
/// (или демон старше 0.5.0 и источника не прислал).
fn smart_selection_of(p: &pb::PlaylistSummary) -> Option<pb::WallpaperSelection> {
    match p.source.as_ref()?.kind.as_ref()? {
        pb::playlist_source::Kind::Smart(sel) => Some(sel.clone()),
        pb::playlist_source::Kind::Curated(_) => None,
    }
}

/// Плейлист для QML: карта [`playlist_summary_map`] и выборка умного
/// плейлиста.
///
/// Выборка лежит отдельно от карты потому, что в JSON ей места нет: в
/// QML она уходит тем же непрозрачным значением (байты, в JS —
/// `ArrayBuffer`), что `WallpaperListQuery.selection`
/// ([`crate::we_wallpaper::encode_selection`]), — `PlaylistEditPage` 0.5.0
/// отдаёт `playlist.smartSelection` прямо в `restoreSelection` и
/// `matchesSelection`. Под ключ [`SMART_SELECTION_KEY`] её кладёт мост.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistView {
    pub map: Value,
    /// Закодированная выборка; `None` — ключа в карте нет.
    pub smart_selection: Option<Vec<u8>>,
}

/// `PlaylistSummary` → [`PlaylistView`]. Выборка — как её прислал демон,
/// без правки: страница сравнивает с ней выборку превью, а демон хранит
/// уже очищенную (её очищает [`playlist_save_smart_request`]).
pub fn playlist_view(p: &pb::PlaylistSummary) -> PlaylistView {
    PlaylistView {
        map: playlist_summary_map(p),
        smart_selection: smart_selection_of(p)
            .map(|sel| crate::we_wallpaper::encode_selection(&sel)),
    }
}

/// Источники «умных» плейлистов, увиденные в ответах демона.
///
/// waywallen 0.5.0 различает курируемый плейлист (явный список обоев) и
/// умный (выборка по фильтрам, `PlaylistSource.smart`). Форма правки
/// вендорного UI 0.4.4 о втором не знает и шлёт `PlaylistUpdate` без
/// `source` — демон читает это как курируемый из `entry_ids`. На умном
/// плейлисте 0.5.0 такой кадр отвергает («source cannot be converted»),
/// то есть имя, режим и интервал умного плейлиста из Rexon поменять было
/// нельзя, а у демона без этой проверки выборка превратилась бы в пустой
/// список.
///
/// Склад общий на процесс, как [`progress_store`]: ответы приходят
/// запросам списка и подробностей, а кадр правки собирает третий объект
/// (`PlaylistMutationQuery`), и протаскивать выборку через QML ради этого
/// не нужно.
pub mod playlist_source_store {
    use crate::proto::control as pb;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn store() -> &'static Mutex<HashMap<i64, pb::WallpaperSelection>> {
        static STORE: OnceLock<Mutex<HashMap<i64, pb::WallpaperSelection>>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    use super::smart_selection_of as smart_of;

    /// Запомнить (или забыть) источник одного плейлиста — ответ `playlist_get`.
    pub fn remember(p: &pb::PlaylistSummary) {
        let mut s = store().lock().expect("склад плейлистов не отравлен");
        match smart_of(p) {
            Some(sel) => {
                s.insert(p.id, sel);
            }
            None => {
                s.remove(&p.id);
            }
        }
    }

    /// Ответ `playlist_list` — полный список: удалённые плейлисты забываются.
    pub fn remember_all(list: &[pb::PlaylistSummary]) {
        let mut s = store().lock().expect("склад плейлистов не отравлен");
        s.clear();
        for p in list {
            if let Some(sel) = smart_of(p) {
                s.insert(p.id, sel);
            }
        }
    }

    /// Выборка умного плейлиста; `None` — плейлист курируемый или не виден.
    pub fn smart(id: i64) -> Option<pb::WallpaperSelection> {
        store()
            .lock()
            .expect("склад плейлистов не отравлен")
            .get(&id)
            .cloned()
    }
}

/// Кадр `PlaylistUpdate` формы правки плейлиста.
///
/// Курируемый плейлист — как прежде: состав в `entry_ids`, `source` не
/// шлётся (так его читает и демон до 0.5.0). Умный — `source` с его
/// же выборкой и пустым `entry_ids`: демон 0.5.0 отвергает кадр, где
/// заданы оба, а ручного состава у умного плейлиста не бывает. Так имя,
/// режим, интервал и синхронный выбор правятся, а выборка остаётся.
///
/// Форма 0.5.0 умный плейлист сохраняет не этим кадром, а `saveSmart`
/// ([`playlist_save_smart_request`]) — с выборкой со страницы; подстановка
/// здесь нужна только `update()` от формы, которая о выборке не знает.
#[allow(clippy::too_many_arguments)]
pub fn playlist_update_request(
    id: i64,
    name: &str,
    mode: i32,
    interval_secs: u32,
    synchronized_selection: bool,
    entry_ids: Vec<String>,
    expected_revision: i64,
) -> pb::PlaylistUpdateRequest {
    let (entry_ids, source) = match playlist_source_store::smart(id) {
        Some(sel) => (
            Vec::new(),
            Some(pb::PlaylistSource {
                kind: Some(pb::playlist_source::Kind::Smart(sel)),
            }),
        ),
        None => (entry_ids, None),
    };
    pb::PlaylistUpdateRequest {
        id,
        name: name.to_string(),
        mode,
        interval_secs,
        expected_revision,
        entry_ids,
        synchronized_selection,
        source,
    }
}

/// Кадр `saveSmart` — создать умный плейлист или обновить существующий.
#[derive(Debug, Clone, PartialEq)]
pub enum SmartPlaylistSave {
    Create(pb::PlaylistCreateRequest),
    Update(pb::PlaylistUpdateRequest),
}

impl SmartPlaylistSave {
    /// Поле `Request` протокола.
    pub fn field(&self) -> &'static str {
        match self {
            Self::Create(_) => "playlist_create",
            Self::Update(_) => "playlist_update",
        }
    }

    /// Ждать ли в ответе номер созданного плейлиста (`captureCreate`).
    pub fn captures_create(&self) -> bool {
        matches!(self, Self::Create(_))
    }

    pub fn encode_to_vec(&self) -> Vec<u8> {
        use prost::Message;
        match self {
            Self::Create(r) => r.encode_to_vec(),
            Self::Update(r) => r.encode_to_vec(),
        }
    }
}

/// `PlaylistMutationQuery::saveSmart` оригинала (waywallen 0.5.0).
///
/// `id > 0` — `PlaylistUpdate`, иначе `PlaylistCreate` (страница создания
/// умного плейлиста зовёт с нулём). Источник — выборка превью страницы
/// как есть, как у оригинала и как у запроса списка: демон получает ровно
/// ту выборку, по которой страница показала превью, а негодную отвергает
/// сам (его ошибка уходит в `error`). `entry_ids` пустой: кадр, где заданы и состав, и источник,
/// демон 0.5.0 отвергает.
///
/// `None` — `selection` не выборка (у оригинала `canConvert` ложен):
/// ничего не шлётся, ответ — [`playlist_smart_invalid`].
#[allow(clippy::too_many_arguments)]
pub fn playlist_save_smart_request(
    id: i64,
    name: &str,
    mode: i32,
    interval_secs: u32,
    synchronized_selection: bool,
    selection: &[u8],
    expected_revision: i64,
) -> Option<SmartPlaylistSave> {
    let sel = crate::we_wallpaper::decode_selection(selection)?;
    let source = Some(pb::PlaylistSource {
        kind: Some(pb::playlist_source::Kind::Smart(sel)),
    });
    Some(if id > 0 {
        SmartPlaylistSave::Update(pb::PlaylistUpdateRequest {
            id,
            name: name.to_string(),
            mode,
            interval_secs,
            expected_revision,
            entry_ids: Vec::new(),
            synchronized_selection,
            source,
        })
    } else {
        SmartPlaylistSave::Create(pb::PlaylistCreateRequest {
            name: name.to_string(),
            mode,
            interval_secs,
            entry_ids: Vec::new(),
            synchronized_selection,
            source,
        })
    })
}

/// `saveSmart` с невыборкой: ошибка, статус Error и `done()` — в порядке
/// оригинала. Текст здесь, а не в мосте: подписи для человека живут в
/// ядре.
pub fn playlist_smart_invalid(st: &mut QueryState) -> Vec<QuerySignal> {
    // Английский — дословно строка оригинала (`playlist_query.cpp`).
    let mut out = we_query::set_error(st, &crate::i18n::tr("invalid smart playlist selection"));
    out.extend(we_query::set_status(st, STATUS_ERROR));
    out.push(QuerySignal::Property("done"));
    out
}

/// Свойства `PlaylistListQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistListData {
    pub playlists: Vec<PlaylistView>,
}

/// Свойства `PlaylistDetailQuery` (страница правки плейлиста, 0.3.7).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistDetailData {
    /// Плейлист — свойство `playlist`.
    pub playlist: PlaylistView,
    /// Номер правки — свойство `revision`.
    pub revision: i64,
    /// Состав по порядку — свойство `entryIds`.
    pub entry_ids: Vec<String>,
    /// Строки модели `data` — карты `we_wallpaper::entry_to_map`.
    pub items: Vec<crate::we_settings::VMap>,
}

/// Разобрать ответ `playlist_get`.
///
/// Порядок сигналов — как у оригинала (`PlaylistDetailQuery::reload`):
/// сначала перестраивается модель обоей, потом летит `playlistChanged`.
/// Страница по нему пересобирает черновик и обязана увидеть УЖЕ новый
/// состав.
pub fn apply_playlist_get(
    st: &mut QueryState,
    data: &mut PlaylistDetailData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::PlaylistGet(detail)) = rsp.payload {
        let summary = detail.playlist.unwrap_or_default();
        playlist_source_store::remember(&summary);
        data.playlist = playlist_view(&summary);
        data.revision = summary.revision;
        data.entry_ids = summary.entry_ids;
        data.items = detail
            .wallpapers
            .iter()
            .map(crate::we_wallpaper::entry_to_map)
            .collect();
        out.push(QuerySignal::Property("items"));
        out.push(QuerySignal::Property("playlist"));
    }
    Ok(finish(st, out))
}

/// Разобрать ответ `playlist_list`. Плейлист строит [`playlist_view`].
pub fn apply_playlist_list(
    st: &mut QueryState,
    data: &mut PlaylistListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::PlaylistList(pl)) = rsp.payload {
        playlist_source_store::remember_all(&pl.playlists);
        data.playlists = pl.playlists.iter().map(playlist_view).collect();
        // Сигнал безусловный — так у оригинала.
        out.push(QuerySignal::Property("playlists"));
    }
    Ok(finish(st, out))
}

/// Разобрать ответ мутации плейлиста.
///
/// `capture_create` — брать ли из ответа номер созданного плейлиста.
/// Сигнал `done()` у оригинала летит ВСЕГДА, в том числе при отказе
/// демона: QML после него читает `.status`.
pub fn apply_playlist_mutation(
    st: &mut QueryState,
    created_id: &mut i64,
    capture_create: bool,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        out.push(QuerySignal::Property("done"));
        return Ok(out);
    };
    if capture_create {
        if let Some(pb::response::Payload::PlaylistCreate(r)) = rsp.payload {
            *created_id = r.id;
            out.push(QuerySignal::Property("createdId"));
        }
    }
    let mut out = finish(st, out);
    out.push(QuerySignal::Property("done"));
    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════
// SourceListQuery / LibraryAutoDetectQuery
// ════════════════════════════════════════════════════════════════════════

/// Свойства `SourceListQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceListData {
    pub sources: Vec<Value>,
}

/// Разобрать ответ `source_list`.
///
/// ⚠️ Починка при порте (правило #16): наш Python отдавал только пять
/// ключей (`name`/`version`/`libraryLabel`/`libraryHint`/`types`), а
/// оригинал на этом вендоре кладёт ещё `pluginId` и полную схему
/// `settings` (`source_query.cpp` @e5626c1). Без них страница настроек
/// источника не знала, какому плагину принадлежит источник и какие у него
/// настройки вообще есть.
pub fn apply_source_list(
    st: &mut QueryState,
    data: &mut SourceListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::SourceList(sl)) = rsp.payload {
        data.sources = sl
            .sources
            .into_iter()
            .map(|s| {
                let settings: Vec<Value> = s
                    .settings
                    .into_iter()
                    .map(|ss| setting_schema_map(&SettingSchema::from(ss)))
                    .collect();
                json!({
                    "name": s.name,
                    "version": s.version,
                    "libraryLabel": s.library_label,
                    "libraryHint": s.library_hint,
                    "types": s.types,
                    "pluginId": s.plugin_id,
                    "settings": settings,
                })
            })
            .collect();
        out.push(QuerySignal::Property("sources"));
    }
    Ok(finish(st, out))
}

/// Разобрать ответ `library_auto_detect`.
///
/// ⚠️ Починка при порте (правило #16): у оригинала есть свойство
/// `addedCount` (сколько библиотек нашлось), а наш Python выбрасывал ответ
/// целиком (`lambda rsp: None`), и число добавленных библиотек показать
/// было нечем.
pub fn apply_library_auto_detect(
    st: &mut QueryState,
    added_count: &mut i32,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::LibraryAutoDetect(r)) = rsp.payload {
        we_query::assign(
            added_count,
            r.added.len() as i32,
            "addedCount",
            &mut out,
        );
    }
    Ok(finish(st, out))
}

// ════════════════════════════════════════════════════════════════════════
// Подписки источников (Мастерская Steam и прочие)
// ════════════════════════════════════════════════════════════════════════

/// Что именно должен разослать шим после ответа на `subscription_status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionStateResult {
    pub state: i32,
    pub error: String,
}

/// Разобрать ответ `subscription_status` для одного предмета.
///
/// Возвращает `(сигналы базы, исход)`. Исход есть всегда: и на успехе, и
/// на отказе — оригинал в обеих ветках шлёт `stateLoaded`, разница только
/// в значениях (на отказе состояние 0 и текст ошибки запроса).
pub fn apply_subscription_status(
    st: &mut QueryState,
    item_id: &str,
    frame_bytes: &[u8],
) -> Result<(Vec<QuerySignal>, SubscriptionStateResult), prost::DecodeError> {
    let (answer, out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        let error = st.error.clone();
        return Ok((out, SubscriptionStateResult { state: 0, error }));
    };
    let mut result = SubscriptionStateResult {
        state: 0,
        error: String::new(),
    };
    if let Some(pb::response::Payload::SubscriptionStatus(status)) = rsp.payload {
        for entry in &status.items {
            if entry.id == item_id {
                result.state = entry.state;
                break;
            }
        }
        result.error = status.error;
    }
    Ok((finish(st, out), result))
}

/// Исход `subscription_set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionSetResult {
    pub accepted: bool,
    pub error: String,
    /// Состояние, которое нужно положить в общий склад приобретения.
    pub state: i32,
}

/// Разобрать ответ `subscription_set`.
///
/// Состояния — как у оригинала: принято → 2 при подписке и 1 при отписке,
/// отклонено → наоборот.
pub fn apply_subscription_set(
    st: &mut QueryState,
    subscribed: bool,
    frame_bytes: &[u8],
) -> Result<(Vec<QuerySignal>, SubscriptionSetResult), prost::DecodeError> {
    let (answer, out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        let error = st.error.clone();
        return Ok((
            out,
            SubscriptionSetResult {
                accepted: false,
                error,
                state: if subscribed { 1 } else { 2 },
            },
        ));
    };
    let mut result = SubscriptionSetResult {
        accepted: false,
        error: String::new(),
        state: if subscribed { 1 } else { 2 },
    };
    if let Some(pb::response::Payload::SubscriptionSet(r)) = rsp.payload {
        result.accepted = r.accepted;
        result.error = r.error;
        result.state = if r.accepted {
            if subscribed {
                2
            } else {
                1
            }
        } else if subscribed {
            1
        } else {
            2
        };
    }
    Ok((finish(st, out), result))
}

// ════════════════════════════════════════════════════════════════════════
// Действия плагинов и правка настроек источника
// ════════════════════════════════════════════════════════════════════════

/// Исход `plugin_action`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginActionResult {
    pub accepted: bool,
    pub error: String,
    pub session_id: String,
}

/// Разобрать ответ `plugin_action`.
///
/// ⚠️ Расхождение с оригиналом СОЗНАТЕЛЬНОЕ и в нашу пользу: у оригинала
/// `completed` шлётся только в двух ветках из трёх — при обрыве связи и
/// при успехе. Если демон ответил, но со статусом-отказом, `inspect_set`
/// лямбду не зовёт, и `completed` не летит вовсе: диалог формы действия
/// зависает открытым навсегда. Здесь `completed(false, error, "")` идёт и
/// в этой ветке — так же, как это делал наш Python-мост.
pub fn apply_plugin_action(
    st: &mut QueryState,
    result: &mut PluginActionResult,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        *result = PluginActionResult {
            accepted: false,
            error: st.error.clone(),
            session_id: String::new(),
        };
        out.push(QuerySignal::Property("completed"));
        return Ok(out);
    };
    *result = PluginActionResult::default();
    if let Some(pb::response::Payload::PluginAction(a)) = rsp.payload {
        result.accepted = a.accepted;
        result.error = a.error;
        result.session_id = a.session_id;
    }
    out.push(QuerySignal::Property("completed"));
    Ok(finish(st, out))
}

/// Исход `remote_settings_patch`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchResult {
    pub accepted: bool,
    pub error: String,
}

/// Разобрать ответ `remote_settings_patch`.
///
/// ⚠️ Расхождение с оригиналом СОЗНАТЕЛЬНОЕ и в нашу пользу: оригинал
/// шлёт `completed(..., accepted = true, "")` СРАЗУ после `inspect_set`,
/// не глядя на статус ответа, — то есть отклонённая демоном правка
/// докладывалась странице как принятая, и та молча считала настройки
/// сохранёнными. Здесь при отказе идёт `accepted = false` с текстом
/// ошибки, как это делал наш Python-мост.
pub fn apply_settings_patch(
    st: &mut QueryState,
    result: &mut PatchResult,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(_rsp) = answer else {
        *result = PatchResult {
            accepted: false,
            error: st.error.clone(),
        };
        out.push(QuerySignal::Property("completed"));
        return Ok(out);
    };
    *result = PatchResult {
        accepted: true,
        error: String::new(),
    };
    let mut out = finish(st, out);
    out.push(QuerySignal::Property("completed"));
    Ok(out)
}

/// Карта `QVariantMap` из QML → карта строк протокола.
///
/// Порт цикла `wire_values` оригинала: значение любого типа приводится к
/// строке, `null` становится пустой строкой.
pub fn values_to_wire(values: &Value) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    if let Value::Object(map) = values {
        for (key, value) in map {
            out.insert(key.clone(), value_to_wire_string(value));
        }
    }
    out
}

fn value_to_wire_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            // Qt печатает bool как "true"/"false" — QVariant::toString().
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        other => other.to_string(),
    }
}

/// Список `QVariantList` из QML → список строк протокола.
pub fn list_to_strings(values: &Value) -> Vec<String> {
    match values {
        Value::Array(items) => items.iter().map(value_to_wire_string).collect(),
        _ => Vec::new(),
    }
}

/// Список `QVariantList` из QML → список беззнаковых номеров экранов.
pub fn list_to_u64(values: &Value) -> Vec<u64> {
    match values {
        Value::Array(items) => items
            .iter()
            .filter_map(|v| match v {
                Value::Number(n) => n.as_u64(),
                Value::String(s) => s.parse::<u64>().ok(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Список целей применения v0.3.5 из того, что присвоил QML.
///
/// Порт `presentationTargetsFromVariant` (`ui/src/proto.cpp` вендора):
/// каждая запись — карта ЛИБО с `canvasId`, ЛИБО с `displayId`; непустой
/// `canvasId` побеждает, а запись без пригодного идентификатора
/// выбрасывается (у вендора `display_id == 0` — «значения нет»).
///
/// Точка сборки целей ровно одна на всю вкладку (правило #12): с v0.3.5
/// поля `display_ids` у `wallpaper_apply` и у обеих команд плейлиста
/// сменили ТИП, сохранив номер, — кадр старой формы демон роняет молча,
/// поэтому собирать цели «ещё где-то рядом» нельзя.
pub fn presentation_targets(values: &Value) -> Vec<pb::PresentationTarget> {
    let Value::Array(items) = values else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let canvas_id = item.get("canvasId").and_then(Value::as_str).unwrap_or("");
        if !canvas_id.is_empty() {
            out.push(pb::PresentationTarget {
                target: Some(pb::presentation_target::Target::CanvasId(
                    canvas_id.to_string(),
                )),
            });
            continue;
        }
        let display_id = match item.get("displayId") {
            Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
            Some(Value::String(s)) => s.parse::<u64>().unwrap_or(0),
            _ => 0,
        };
        if display_id == 0 {
            continue;
        }
        out.push(pb::PresentationTarget {
            target: Some(pb::presentation_target::Target::DisplayId(display_id)),
        });
    }
    out
}

/// Пустая карта — чтобы оболочка не собирала её вручную.
pub fn empty_map() -> Map<String, Value> {
    Map::new()
}

// ════════════════════════════════════════════════════════════════════════
// Партия 7: глобальные пауза/звук/стоп и холсты (v0.3.5)
// ════════════════════════════════════════════════════════════════════════
//
// Эталон — вендорные `ui/src/query/global_pause_query.cpp` и
// `ui/src/query/display_query.cpp` на теге v0.3.5 (f7baecd), тот же тег, с
// которого вендорен наш QML.

/// Какой из четырёх глобальных переключателей отвечает демон.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalFlag {
    /// `global_pause_toggle` — демон сам решает, во что переключиться.
    PauseToggle,
    /// `global_pause_set`.
    PauseSet,
    /// `global_mute_set`.
    MuteSet,
    /// `global_stop_set`.
    StopSet,
}

impl GlobalFlag {
    /// Достать значение из ответа демона, если в нём ожидаемая нагрузка.
    fn read(self, payload: &pb::response::Payload) -> Option<bool> {
        match (self, payload) {
            (GlobalFlag::PauseToggle, pb::response::Payload::GlobalPauseToggle(r)) => {
                Some(r.paused)
            }
            (GlobalFlag::PauseSet, pb::response::Payload::GlobalPauseSet(r)) => Some(r.paused),
            (GlobalFlag::MuteSet, pb::response::Payload::GlobalMuteSet(r)) => Some(r.muted),
            (GlobalFlag::StopSet, pb::response::Payload::GlobalStopSet(r)) => Some(r.stopped),
            _ => None,
        }
    }
}

/// Разбор ответа на любой из четырёх глобальных переключателей.
///
/// Возвращает вместе с сигналами то значение, которое НАЗВАЛ демон — оно
/// нужно `GlobalPauseToggleQuery`: у оригинала сигнал `toggled(paused)`
/// летит на каждом успешном ответе, даже если свойство не изменилось
/// (кнопка в интерфейсе показывает тост «Приостановлено»/«Продолжено»
/// независимо от того, совпало ли значение с прежним).
///
/// `None` во втором поле — ответ не про нас (демон ошибся полем) или демон
/// отказал: тогда ни свойство, ни сигнал трогать нельзя.
pub fn apply_global_flag(
    st: &mut QueryState,
    flag: GlobalFlag,
    slot: &mut bool,
    property: &'static str,
    frame_bytes: &[u8],
) -> Result<(Vec<QuerySignal>, Option<bool>), prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok((out, None));
    };
    let value = rsp.payload.as_ref().and_then(|p| flag.read(p));
    if let Some(value) = value {
        we_query::assign(slot, value, property, &mut out);
    }
    Ok((finish(st, out), value))
}

/// Чем кончилась правка холста — ровно три исхода оригинала.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasMutation {
    /// `canvasCreated(id)`.
    Created(String),
    /// `canvasUpdated(id, revision)`.
    Updated(String, u64),
    /// `canvasDeleted(id)`. Номер холста демон в ответе не повторяет —
    /// его помнит сам запрос с момента отправки кадра.
    Deleted(String),
    /// Демон отказал либо ответил не тем полем: сигналов нет.
    None,
}

/// Разбор ответа на создание/правку/удаление холста.
///
/// `deleted_id` — номер холста из ОТПРАВЛЕННОГО кадра удаления (у
/// оригинала он тоже захватывается в замыкание до отправки).
pub fn apply_canvas_mutation(
    st: &mut QueryState,
    deleted_id: &str,
    frame_bytes: &[u8],
) -> Result<(Vec<QuerySignal>, CanvasMutation), prost::DecodeError> {
    let (answer, out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok((out, CanvasMutation::None));
    };
    let outcome = match rsp.payload {
        Some(pb::response::Payload::CanvasCreate(r)) => match r.canvas {
            Some(canvas) => CanvasMutation::Created(canvas.canvas_id),
            None => CanvasMutation::None,
        },
        Some(pb::response::Payload::CanvasUpdate(r)) => match r.canvas {
            Some(canvas) => CanvasMutation::Updated(canvas.canvas_id, r.revision),
            None => CanvasMutation::None,
        },
        Some(pb::response::Payload::CanvasDelete(_)) => {
            CanvasMutation::Deleted(deleted_id.to_string())
        }
        _ => CanvasMutation::None,
    };
    Ok((finish(st, out), outcome))
}

/// Прямоугольник участника холста из карты QML (`{settingsKey, x, y,
/// width, height}`) — так его кладёт `CanvasEditorState.members`.
///
/// Ключи именно camelCase: карту собирает QML вендора, а не протокол.
pub fn canvas_members_from_json(values: &Value) -> Vec<pb::CanvasMemberInput> {
    let Some(items) = values.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| pb::CanvasMemberInput {
            settings_key: item
                .get("settingsKey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            rect: Some(pb::CanvasRect {
                x: json_i32(item.get("x")),
                y: json_i32(item.get("y")),
                width: json_i32(item.get("width")).max(0) as u32,
                height: json_i32(item.get("height")).max(0) as u32,
            }),
            // Замок пропорций участника — поле протокола 0.3.7. Берём из
            // той же карты QML; нынешний вендорный QML ключа не кладёт, и
            // тогда здесь `false` — ровно нынешний кадр (нулевой bool в
            // proto3 не сериализуется).
            aspect_locked: item
                .get("aspectLocked")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

/// Число из QML: оно приходит и целым, и дробным (JS не различает).
fn json_i32(value: Option<&Value>) -> i32 {
    match value {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        Some(Value::String(s)) => s.parse::<i32>().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    fn frame(payload: pb::response::Payload) -> Vec<u8> {
        let rsp = pb::Response {
            request_id: 1,
            status: pb::Status::Ok as i32,
            payload: Some(payload),
            ..Default::default()
        };
        pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(rsp)),
        }
        .encode_to_vec()
    }

    #[test]
    fn плагины_сортируются_пользовательские_раньше_системных() {
        let payload = pb::response::Payload::PluginList(pb::PluginListResponse {
            plugins: vec![
                pb::PluginInfo {
                    id: "org.a".into(),
                    name: "Zeta".into(),
                    system: true,
                    ..Default::default()
                },
                pb::PluginInfo {
                    id: "org.b".into(),
                    name: "Beta".into(),
                    system: false,
                    ..Default::default()
                },
                pb::PluginInfo {
                    id: "org.c".into(),
                    name: String::new(),
                    system: false,
                    ..Default::default()
                },
            ],
            inactive_system: vec![],
            inactive_user: vec![],
            ..Default::default()
        });
        let mut st = QueryState::default();
        let mut data = PluginListData::default();
        apply_plugin_list(&mut st, &mut data, &frame(payload)).expect("кадр разобран");
        let ids: Vec<&str> = data
            .plugins
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        // Beta < org.c (пустое имя заменяется id), системный Zeta — последним.
        assert_eq!(ids, ["org.b", "org.c", "org.a"]);
    }

    #[test]
    fn источник_несёт_pluginid_и_настройки() {
        let payload = pb::response::Payload::SourceList(pb::SourceListResponse {
            sources: vec![pb::SourcePluginInfo {
                name: "wallhaven".into(),
                plugin_id: "org.waywallen.wallhaven".into(),
                settings: vec![pb::SettingSchema {
                    key: "api_key".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        });
        let mut st = QueryState::default();
        let mut data = SourceListData::default();
        apply_source_list(&mut st, &mut data, &frame(payload)).expect("кадр разобран");
        assert_eq!(data.sources[0]["pluginId"], "org.waywallen.wallhaven");
        assert_eq!(data.sources[0]["settings"][0]["key"], "api_key");
    }

    #[test]
    fn автоопределение_считает_добавленные_библиотеки() {
        let payload = pb::response::Payload::LibraryAutoDetect(pb::LibraryAutoDetectResponse {
            added: vec![pb::LibraryInstance::default(); 3],
        });
        let mut st = QueryState::default();
        let mut added = 0;
        let signals = apply_library_auto_detect(&mut st, &mut added, &frame(payload))
            .expect("кадр разобран");
        assert_eq!(added, 3);
        assert!(signals.contains(&QuerySignal::Property("addedCount")));
    }

    #[test]
    fn отклонённая_правка_настроек_не_докладывается_принятой() {
        // Отказ демона: статус не OK.
        let rsp = pb::Response {
            request_id: 1,
            status: pb::Status::InvalidArgument as i32,
            message: "нет такого источника".into(),
            ..Default::default()
        };
        let bytes = pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(rsp)),
        }
        .encode_to_vec();
        let mut st = QueryState::default();
        let mut result = PatchResult::default();
        apply_settings_patch(&mut st, &mut result, &bytes).expect("кадр разобран");
        assert!(!result.accepted);
        assert_eq!(result.error, "нет такого источника");
    }

    #[test]
    fn прогресс_задачи_живёт_складом_и_номерами() {
        progress_store::reset();
        // Снимок ПРОШЛОЙ задачи с тем же идентификатором пришёл ДО начала
        // нашего запроса — он не должен его закрыть.
        let stale = progress_store::record("q1", 1.0, false, true, true, "старая ошибка");

        let mut st = QueryState::default();
        let mut ps = ProgressState::default();
        let begin = begin_progress(&mut st, &mut ps);
        assert!(begin.contains(&QuerySignal::Property("progressing")));
        assert_eq!(ps.begin_sequence, stale.sequence);

        // Приём задачи: старый снимок в складе есть, но он не новее начала.
        accept_progress(&mut st, &mut ps, "q1");
        assert_eq!(st.status, STATUS_QUERYING);
        assert!(!progress_matches(&ps, "q1", stale.sequence));
        let snapshot = progress_store::record("q1", 1.0, false, true, false, "");
        assert!(progress_matches(&ps, "q1", snapshot.sequence));
        let signals = apply_task_progress(&mut st, &mut ps, &snapshot);
        assert!(signals.contains(&QuerySignal::Property(PROGRESS_ENDED)));
        assert_eq!(st.status, we_query::STATUS_FINISHED);
    }

    // ── партия 7: глобальные переключатели и холсты ──────────────────

    #[test]
    fn глобальная_пауза_берёт_значение_от_демона_а_не_своё() {
        let mut st = QueryState::default();
        let mut value = false;
        // Клиент просил «пауза», демон ответил «не пауза» — побеждает демон.
        let bytes = frame(pb::response::Payload::GlobalPauseSet(
            pb::GlobalPauseSetResponse { paused: false },
        ));
        let (signals, answered) =
            apply_global_flag(&mut st, GlobalFlag::PauseSet, &mut value, "paused", &bytes)
                .expect("кадр разбирается");
        assert!(!value);
        assert_eq!(answered, Some(false));
        assert!(!signals
            .iter()
            .any(|s| matches!(s, QuerySignal::Property("paused"))));
        assert_eq!(st.status, we_query::STATUS_FINISHED);
    }

    #[test]
    fn переключатель_паузы_докладывает_значение_даже_без_смены_свойства() {
        let mut st = QueryState::default();
        let mut value = true;
        let bytes = frame(pb::response::Payload::GlobalPauseToggle(
            pb::GlobalPauseToggleResponse { paused: true },
        ));
        let (_, answered) = apply_global_flag(
            &mut st,
            GlobalFlag::PauseToggle,
            &mut value,
            "paused",
            &bytes,
        )
        .expect("кадр разбирается");
        // Свойство не менялось, но тост в интерфейсе показать надо.
        assert_eq!(answered, Some(true));
    }

    #[test]
    fn чужая_нагрузка_не_трогает_флаг() {
        let mut st = QueryState::default();
        let mut value = false;
        let bytes = frame(pb::response::Payload::GlobalMuteSet(
            pb::GlobalMuteSetResponse { muted: true },
        ));
        let (_, answered) =
            apply_global_flag(&mut st, GlobalFlag::StopSet, &mut value, "stopped", &bytes)
                .expect("кадр разбирается");
        assert_eq!(answered, None);
        assert!(!value);
    }

    #[test]
    fn правка_холста_различает_три_исхода() {
        let created = frame(pb::response::Payload::CanvasCreate(
            pb::CanvasCreateResponse {
                canvas: Some(pb::CanvasInfo {
                    canvas_id: "c1".into(),
                    ..Default::default()
                }),
                revision: 7,
            },
        ));
        let mut st = QueryState::default();
        let (_, outcome) =
            apply_canvas_mutation(&mut st, "", &created).expect("кадр разбирается");
        assert_eq!(outcome, CanvasMutation::Created("c1".into()));

        let updated = frame(pb::response::Payload::CanvasUpdate(
            pb::CanvasUpdateResponse {
                canvas: Some(pb::CanvasInfo {
                    canvas_id: "c2".into(),
                    ..Default::default()
                }),
                revision: 9,
            },
        ));
        let mut st = QueryState::default();
        let (_, outcome) =
            apply_canvas_mutation(&mut st, "", &updated).expect("кадр разбирается");
        assert_eq!(outcome, CanvasMutation::Updated("c2".into(), 9));

        // Номер удалённого холста демон не повторяет — его помнит запрос.
        let deleted = frame(pb::response::Payload::CanvasDelete(pb::Empty {}));
        let mut st = QueryState::default();
        let (_, outcome) =
            apply_canvas_mutation(&mut st, "c3", &deleted).expect("кадр разбирается");
        assert_eq!(outcome, CanvasMutation::Deleted("c3".into()));
    }

    #[test]
    fn отказ_демона_на_правке_холста_не_даёт_сигнала() {
        let rsp = pb::Response {
            request_id: 1,
            status: pb::Status::Internal as i32,
            message: "холст занят".into(),
            ..Default::default()
        };
        let bytes = pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(rsp)),
        }
        .encode_to_vec();
        let mut st = QueryState::default();
        let (_, outcome) =
            apply_canvas_mutation(&mut st, "c3", &bytes).expect("кадр разбирается");
        assert_eq!(outcome, CanvasMutation::None);
        assert_eq!(st.status, we_query::STATUS_ERROR);
    }

    #[test]
    fn участники_холста_читаются_из_карт_qml() {
        let members = serde_json::json!([
            { "settingsKey": "DP-1", "x": -100.0, "y": 0, "width": 1920.0, "height": 1080 },
            { "settingsKey": "HDMI-1" },
        ]);
        let got = canvas_members_from_json(&members);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].settings_key, "DP-1");
        let rect = got[0].rect.as_ref().expect("прямоугольник обязателен");
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (-100, 0, 1920, 1080));
        // Пропущенные ключи — нули, а не отказ разбора.
        let rect = got[1].rect.as_ref().expect("прямоугольник обязателен");
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (0, 0, 0, 0));
    }

    /// waywallen 0.5.0: «Сохранить» в форме правки умного плейлиста не
    /// должно ни превращать его в курируемый, ни ломаться отказом демона.
    /// Склад общий на процесс, поэтому весь сценарий — в одной проверке.
    #[test]
    fn smart_playlist_update_keeps_its_selection() {
        let selection = pb::WallpaperSelection {
            filters: vec![pb::WallpaperFilterRule {
                r#type: pb::WallpaperFilterType::Tag as i32,
                group: 0,
                payload: Some(pb::wallpaper_filter_rule::Payload::TagFilter(
                    pb::WallpaperTagFilter {
                        values: vec!["море".into()],
                        condition: pb::StringCondition::Is as i32,
                    },
                )),
            }],
            wp_type: "scene".into(),
            ..Default::default()
        };
        let smart = pb::PlaylistSummary {
            id: 7001,
            name: "Море".into(),
            source_kind: "smart".into(),
            item_count: 12,
            source: Some(pb::PlaylistSource {
                kind: Some(pb::playlist_source::Kind::Smart(selection.clone())),
            }),
            ..Default::default()
        };
        let curated = pb::PlaylistSummary {
            id: 7002,
            name: "Избранное".into(),
            source_kind: "curated".into(),
            entry_ids: vec!["5".into()],
            source: Some(pb::PlaylistSource {
                kind: Some(pb::playlist_source::Kind::Curated(pb::CuratedPlaylistSource {
                    entry_ids: vec!["5".into()],
                })),
            }),
            ..Default::default()
        };
        let mut st = QueryState::default();
        let mut data = PlaylistListData::default();
        let list = pb::response::Payload::PlaylistList(pb::PlaylistListResponse {
            playlists: vec![smart.clone(), curated.clone()],
        });
        apply_playlist_list(&mut st, &mut data, &frame(list)).expect("кадр разобран");
        assert_eq!(data.playlists[0].map["sourceKind"], "smart");

        // Умный: выборка уходит обратно, ручного состава нет (форма 0.4.4
        // шлёт пустой список — демон 0.5.0 отверг бы кадр с обоими полями).
        let req = playlist_update_request(7001, "Море 2", 3, 600, true, vec![], 99);
        assert!(req.entry_ids.is_empty());
        assert_eq!(
            req.source,
            Some(pb::PlaylistSource {
                kind: Some(pb::playlist_source::Kind::Smart(selection.clone())),
            })
        );
        assert_eq!((req.name.as_str(), req.mode, req.interval_secs), ("Море 2", 3, 600));
        assert_eq!(req.expected_revision, 99);

        // Курируемый — как прежде: состав в `entry_ids`, `source` не шлётся.
        let req = playlist_update_request(7002, "Избранное", 1, 0, false, vec!["5".into(), "6".into()], 0);
        assert_eq!(req.entry_ids, vec!["5".to_string(), "6".to_string()]);
        assert!(req.source.is_none());

        // Подробности одного плейлиста тоже обновляют склад: умный стал
        // курируемым в другом клиенте — больше не подставляем выборку.
        let mut detail = PlaylistDetailData::default();
        let get = pb::response::Payload::PlaylistGet(pb::PlaylistGetResponse {
            playlist: Some(pb::PlaylistSummary {
                source: None,
                ..smart.clone()
            }),
            wallpapers: vec![],
        });
        apply_playlist_get(&mut st, &mut detail, &frame(get)).expect("кадр разобран");
        assert!(playlist_update_request(7001, "Море", 1, 0, false, vec![], 0).source.is_none());

        // Плейлист пропал из полного списка — забыт.
        let list = pb::response::Payload::PlaylistList(pb::PlaylistListResponse {
            playlists: vec![curated],
        });
        apply_playlist_list(&mut st, &mut data, &frame(list)).expect("кадр разобран");
        assert!(playlist_source_store::smart(7001).is_none());
    }

    /// Выборка умного плейлиста с годным правилом и сортировкой и с
    /// незаполненным: правило без значения и сортировка без ключа. Их
    /// отвергает демон, а не Rexon, — до демона они обязаны доехать.
    fn smart_selection_with_junk() -> pb::WallpaperSelection {
        pb::WallpaperSelection {
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
            sorts: vec![
                pb::WallpaperSortRule {
                    key: pb::WallpaperSortKey::Unspecified as i32,
                    direction: 1,
                },
                pb::WallpaperSortRule {
                    key: pb::WallpaperSortKey::Name as i32,
                    direction: 2,
                },
            ],
            search_text: "море".into(),
            hidden_library_ids: vec![3],
            ..Default::default()
        }
    }

    /// waywallen 0.5.0, `saveSmart`: номер > 0 — `PlaylistUpdate`, иначе
    /// `PlaylistCreate` с захватом номера; источник — выборка как есть (ни
    /// одно правило не теряется), состав пустой; не выборка — кадра нет,
    /// ошибка и `done()`.
    #[test]
    fn save_smart_sends_selection_as_is_without_entries() {
        let raw = smart_selection_with_junk();
        let bytes = crate::we_wallpaper::encode_selection(&raw);
        let smart = Some(pb::PlaylistSource {
            kind: Some(pb::playlist_source::Kind::Smart(raw.clone())),
        });

        let save = playlist_save_smart_request(7101, "Закаты", 2, 900, false, &bytes, 41)
            .expect("выборка разобрана");
        assert_eq!(save.field(), "playlist_update");
        assert!(!save.captures_create());
        let SmartPlaylistSave::Update(r) = &save else { panic!("ждали PlaylistUpdate") };
        assert_eq!(
            r,
            &pb::PlaylistUpdateRequest {
                id: 7101,
                name: "Закаты".into(),
                mode: 2,
                interval_secs: 900,
                expected_revision: 41,
                entry_ids: vec![],
                synchronized_selection: false,
                source: smart.clone(),
            }
        );
        // Байты кадра — те же, что у prost-кода запроса.
        assert_eq!(save.encode_to_vec(), prost::Message::encode_to_vec(r));

        for id in [0, -5] {
            let save = playlist_save_smart_request(id, "Новый", 1, 0, true, &bytes, 0)
                .expect("выборка разобрана");
            assert_eq!(save.field(), "playlist_create");
            assert!(save.captures_create());
            assert_eq!(
                save,
                SmartPlaylistSave::Create(pb::PlaylistCreateRequest {
                    name: "Новый".into(),
                    mode: 1,
                    interval_secs: 0,
                    entry_ids: vec![],
                    synchronized_selection: true,
                    source: smart.clone(),
                })
            );
        }

        // Пустая выборка — тоже выборка (метка есть, кода нет).
        let empty = crate::we_wallpaper::encode_selection(&pb::WallpaperSelection::default());
        assert!(playlist_save_smart_request(0, "Все", 1, 0, true, &empty, 0).is_some());

        // Не выборка: пусто, строка, чужой код без метки.
        let foreign = prost::Message::encode_to_vec(&raw);
        for junk in [&b""[..], b"selection", &foreign[..]] {
            assert!(playlist_save_smart_request(7101, "x", 1, 0, true, junk, 0).is_none());
        }
        let mut st = QueryState::default();
        let signals = playlist_smart_invalid(&mut st);
        assert_eq!(st.status, we_query::STATUS_ERROR);
        assert!(!st.error.is_empty());
        assert_eq!(signals.last(), Some(&QuerySignal::Property("done")));
    }

    /// waywallen 0.5.0: карта умного плейлиста несёт его выборку тем же
    /// непрозрачным значением, что `WallpaperListQuery.selection`, — как
    /// прислал демон, без очистки; у курируемого выборки нет. Список и
    /// подробности строят плейлист одной функцией.
    #[test]
    fn smart_selection_rides_with_playlist_map() {
        let raw = smart_selection_with_junk();
        let smart = pb::PlaylistSummary {
            id: 7102,
            name: "Закаты".into(),
            source_kind: "smart".into(),
            item_count: 4,
            revision: 9,
            source: Some(pb::PlaylistSource {
                kind: Some(pb::playlist_source::Kind::Smart(raw.clone())),
            }),
            ..Default::default()
        };
        let view = playlist_view(&smart);
        assert_eq!(view.map, playlist_summary_map(&smart));
        assert!(view.map.get(SMART_SELECTION_KEY).is_none(), "байты кладёт мост");
        let bytes = view.smart_selection.clone().expect("у умного выборка есть");
        assert_eq!(bytes, crate::we_wallpaper::encode_selection(&raw));
        assert_eq!(crate::we_wallpaper::decode_selection(&bytes), Some(raw.clone()));

        // `restoreSelection(smartSelection)` превью даёт ту же выборку —
        // `matchesSelection(baseline)` сразу после восстановления истинно.
        let mut preview = crate::we_wallpaper::WallpaperListData::default();
        let _ = crate::we_wallpaper::restore_selection(&mut preview, &bytes);
        assert!(crate::we_wallpaper::matches_selection(&preview, &bytes));

        // Курируемый и плейлист без источника (демон до 0.5.0) — без ключа.
        for source in [
            Some(pb::PlaylistSource {
                kind: Some(pb::playlist_source::Kind::Curated(pb::CuratedPlaylistSource {
                    entry_ids: vec!["1".into()],
                })),
            }),
            None,
        ] {
            let p = pb::PlaylistSummary {
                id: 7103,
                source,
                ..Default::default()
            };
            assert_eq!(playlist_view(&p).smart_selection, None);
        }

        // Подробности: тот же плейлист.
        let mut st = QueryState::default();
        let mut detail = PlaylistDetailData::default();
        let get = pb::response::Payload::PlaylistGet(pb::PlaylistGetResponse {
            playlist: Some(smart.clone()),
            wallpapers: vec![],
        });
        apply_playlist_get(&mut st, &mut detail, &frame(get)).expect("кадр разобран");
        assert_eq!(detail.playlist, view);
        assert_eq!(detail.revision, 9);
    }
}
