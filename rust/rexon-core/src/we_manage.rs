//! Parsing responses of the «management» requests of the Wallpaper Engine tab:
//! plugins, playlists, sources and libraries, source subscriptions,
//! plugin actions and editing their settings.
//!
//! Only the Qt-free part lives here: daemon frame → maps/lists + a list of signals
//! in the original's order. The `#[qobject]` wrappers live in `rexon-gui`.
//!
//! ⚠️ The reference is the ORIGINAL (waywallen `ui/src/query/` at
//! vendor commit `e5626c1`, a.k.a. v0.2.6+9), not our Python bridge.
//! Deviations that an «as it was» port would have preserved are fixed
//! here and listed at each function.
//!
//! A separate note on sorting the plugin list: the original compares names
//! with `QString::localeAwareCompare`, i.e. with ICU collation. The core has no Qt and
//! must not have it, so the comparison goes by `to_lowercase()` — exactly
//! what the Python bridge did. For Latin plugin names
//! (`org.waywallen.*`) the result is the same; a difference is possible only
//! on names with diacritics, and pulling ICU into the core for that costs more than it gives.

use serde_json::{json, Map, Value};

use crate::proto::control as pb;
use crate::we_query::{
    self, finish, take_response, QuerySignal, QueryState, SettingSchema, STATUS_ERROR,
    STATUS_QUERYING,
};

/// `SettingSchema` map for QML.
///
/// ⚠️ SNAKE_CASE keys — `SettingField.qml` is written against the original's
/// `default_value`/`label_key`/`description_key`.
fn setting_schema_map(s: &SettingSchema) -> Value {
    json!({
        "key": s.key,
        "type": s.value_type,
        "default_value": s.default_value,
        "identity": s.identity,
        "label_key": s.label_key,
        "description_key": s.description_key,
        // Translatable labels of the plugin catalog: `SettingField.qml` reads
        // `label`/`description`, the group header — `group_label`.
        "label": s.label,
        "description": s.description,
        "group_label": s.group_label,
        "min": s.min,
        "max": s.max,
        "step": s.step,
        "choices": s.choices,
        // waywallen 0.6.0: option labels (`choiceLabels`, camelCase —
        // that is how `source_query.cpp` writes the key).
        "choiceLabels": s.choice_labels,
        "group": s.group,
        "order": s.order,
    })
}

/// `RendererPluginInfo` → map (the original's `renderer_to_map` + our
/// `bin`/`priority`/`pluginId`; extra keys do not bother QML).
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

/// `PluginUpdateInfo` → map (the original's `plugin_update_to_map`).
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

/// `PluginCompat` → map (the original's `plugin_compat_to_map`, waywallen
/// 0.4.2): whether the installed plugin suits this daemon and, if not —
/// why, in the daemon's own words. As in the original, a missing message
/// is read as protobuf defaults (`compatible: false`, empty reason).
fn plugin_compat_map(c: &pb::PluginCompat) -> Value {
    json!({
        "compatible": c.compatible,
        "reason": c.reason,
    })
}

// ════════════════════════════════════════════════════════════════════════
// PluginListQuery
// ════════════════════════════════════════════════════════════════════════

/// Properties of `PluginListQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginListData {
    pub plugins: Vec<Value>,
    pub inactive_system: Vec<String>,
    pub inactive_user: Vec<String>,
}

/// The original's sort key: user plugins before system ones, within each —
/// by name (an empty name is replaced by the id).
fn plugin_sort_key(m: &Value) -> (bool, String) {
    let system = m.get("system").and_then(Value::as_bool).unwrap_or(false);
    let name = m.get("name").and_then(Value::as_str).unwrap_or_default();
    let id = m.get("id").and_then(Value::as_str).unwrap_or_default();
    let label = if name.is_empty() { id } else { name };
    (system, label.to_lowercase())
}

/// Parse the `plugin_list` response.
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
        // One signal for three properties — as in the original (`pluginsChanged`
        // is declared NOTIFY for all three).
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

/// Properties of `PluginInstallQuery` and `PluginDeleteQuery` — they are the same.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginIdResult {
    pub plugin_id: String,
    pub needs_restart: bool,
}

/// Parse the `plugin_install` response.
///
/// Signals: `resultChanged`, then `installed(pluginId, needsRestart)` —
/// both UNCONDITIONAL, as in the original: they mean «the daemon answered», not
/// «the value became different».
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

/// Parse the `plugin_delete` response.
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

/// Properties of `PluginInspectQuery` — the whole response.
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

/// Parse the `plugin_inspect` response.
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
// Progress of long tasks (the original's ProgressQuery)
// ════════════════════════════════════════════════════════════════════════
//
// ⚠️ Fixed during the port (rule #16). In the original, checking and installing
// plugin updates inherit from `ProgressQuery`: the RPC only ACCEPTS the
// task and returns a `query_id`, and the task then proceeds through the daemon's
// `task_progress` events — that is where `progress`/`progressing` and the real end of the work come from.
// Our Python bridge did not have this class at all:
//
//   * there were no `queryId`/`progress`/`progressing` properties, while the vendor QML
//     reads them (`PluginManagePage.qml:499,501`) — the update indicator
//     bindings were dead;
//   * `installed`/`checked` were emitted on task ACCEPTANCE, so the toast
//     «Updated "X"» appeared immediately, not when the update was in place;
//   * the `task_progress` event was parsed neither in `we_notify` nor anywhere
//     else — the whole progress channel went nowhere.

/// Progress snapshot of one daemon task (`TaskProgressSnapshot`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskProgressSnapshot {
    pub progress: f64,
    pub progressing: bool,
    pub ended: bool,
    pub error: bool,
    pub message: String,
    /// Monotonic number: a snapshot that arrived BEFORE the request started has nothing
    /// to do with it and must be ignored.
    pub sequence: u64,
}

/// Store of progress snapshots — port of the `m_task_progress`/
/// `m_task_progress_sequence` fields of the original's `Notify` singleton.
///
/// Lives in the core, not in the wrapper: both event parsing and the requests
/// need it, and there must not be a second copy (rule #12).
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

    /// Record a snapshot, giving it the next number. Returns what was recorded.
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

    /// Current number (port of `taskProgressSequence()`).
    pub fn sequence() -> u64 {
        store()
            .lock()
            .expect("склад прогресса не отравлен")
            .sequence
    }

    /// Snapshot by task id (port of `taskProgressSnapshot()`).
    pub fn snapshot(query_id: &str) -> Option<TaskProgressSnapshot> {
        store()
            .lock()
            .expect("склад прогресса не отравлен")
            .items
            .get(query_id)
            .cloned()
    }

    /// Forget everything — needed only by tests, so cases do not affect each
    /// other.
    pub fn reset() {
        let mut s = store().lock().expect("склад прогресса не отравлен");
        s.sequence = 0;
        s.items.clear();
    }
}

/// `ProgressQuery` state on top of the `Query` base state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressState {
    pub query_id: String,
    pub progress: f64,
    pub progressing: bool,
    /// Store number at the moment of `beginProgressQuery` — everything earlier
    /// does not belong to this request.
    pub begin_sequence: u64,
    /// Text of the last `progressEnded`. Separate from `QueryState::error`:
    /// in the original the signal carries `snapshot.message`, and it also arrives on
    /// successful completion, where there is no error at all.
    pub last_message: String,
}

/// `progressEnded(error, message)` cannot be expressed as a separate variant,
/// so the wrapper receives it as `Property("progressEnded")`, takes the text
/// from `ProgressState::last_message`, and the error flag from the status.
pub const PROGRESS_ENDED: &str = "progressEnded";

fn set_progress_value(ps: &mut ProgressState, value: f64, out: &mut Vec<QuerySignal>) {
    let value = value.clamp(0.0, 1.0);
    // The original's qFuzzyCompare(m_progress + 1.0, progress + 1.0) — the same
    // comparison, corrected for near-zero values.
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

/// `beginProgressQuery()`: remember the store number, zero the progress and
/// switch the base to Querying.
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

/// `acceptProgressQuery(queryId)`: an empty id — immediate failure;
/// otherwise remember it and, if the snapshot has already arrived (the daemon may have
/// beaten the response), apply it right away.
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

/// A `task_progress` event arrived — does it belong to this request.
///
/// Port of the condition in the `ProgressQuery` constructor lambda: an empty `queryId`,
/// a foreign `queryId` and a snapshot older than the request start are skipped.
pub fn progress_matches(ps: &ProgressState, query_id: &str, sequence: u64) -> bool {
    !ps.query_id.is_empty() && ps.query_id == query_id && sequence > ps.begin_sequence
}

/// Properties of `PluginUpdateCheckQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginUpdateCheckData {
    pub updates: Vec<Value>,
}

/// Parse the `plugin_update_check` response: this is task ACCEPTANCE, not its end.
pub fn apply_plugin_update_check(
    st: &mut QueryState,
    ps: &mut ProgressState,
    data: &mut PluginUpdateCheckData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        // On a refusal the original calls failProgressQuery — the Error status is already
        // set by take_response, what is left is to clear the working flag and
        // report the end.
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
        // The signal is unconditional — as in the original.
        out.push(QuerySignal::Property("updates"));
        query_id = r.query_id;
    }
    out.extend(accept_progress(st, ps, &query_id));
    Ok(out)
}

/// Parse the `plugin_update_install` response — also task acceptance.
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

/// Parse a daemon event: if it is `task_progress` — record the snapshot.
///
/// Returns `(query_id, snapshot)`; `None` — the frame is not about progress.
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

/// `PlaylistSummary` → map in QML terms (the original's `playlist_to_map`).
///
/// ⚠️ The `sourceKind` key is ours, the original's map does not have it. Kept:
/// an extra key does not bother the vendor QML, and Rexon's playlists page
/// uses it to tell «smart» playlists from regular ones.
///
/// One point for the whole file (rule #12): both the playlist list and the details
/// of one playlist live on this same map — they must not diverge, otherwise
/// `revision` would reach one page and get lost on the other.
pub fn playlist_summary_map(p: &pb::PlaylistSummary) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "sourceKind": p.source_kind,
        "mode": p.mode,
        "intervalSecs": p.interval_secs,
        "itemCount": p.item_count,
        "entryIds": p.entry_ids,
        // Protocol 0.3.7 fields. `revision` is the edit race guard: the page
        // returns it to the daemon in `expected_revision`. `synchronizedSelection`
        // is a toggle of the playlist edit form.
        "revision": p.revision,
        "synchronizedSelection": p.synchronized_selection,
    })
}

/// Playlist map key holding the smart playlist selection (waywallen 0.5.0,
/// the original's `playlist_to_map`: only a smart playlist has the key).
pub const SMART_SELECTION_KEY: &str = "smartSelection";

/// Smart playlist selection from its summary; `None` — the playlist is curated
/// (or the daemon is older than 0.5.0 and sent no source).
fn smart_selection_of(p: &pb::PlaylistSummary) -> Option<pb::WallpaperSelection> {
    match p.source.as_ref()?.kind.as_ref()? {
        pb::playlist_source::Kind::Smart(sel) => Some(sel.clone()),
        pb::playlist_source::Kind::Curated(_) => None,
    }
}

/// A playlist for QML: the [`playlist_summary_map`] map and the smart playlist
/// selection.
///
/// The selection lies apart from the map because there is no place for it in JSON:
/// it goes to QML as the same opaque value (bytes, in JS an
/// `ArrayBuffer`) as `WallpaperListQuery.selection`
/// ([`crate::we_wallpaper::encode_selection`]) — `PlaylistEditPage` 0.5.0
/// hands `playlist.smartSelection` straight to `restoreSelection` and
/// `matchesSelection`. The bridge puts it under the [`SMART_SELECTION_KEY`] key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistView {
    pub map: Value,
    /// Encoded selection; `None` — the key is not in the map.
    pub smart_selection: Option<Vec<u8>>,
}

/// `PlaylistSummary` → [`PlaylistView`]. The selection is as the daemon sent it,
/// unedited: the page compares the preview selection with it, and the daemon stores
/// an already cleaned one (cleaned by [`playlist_save_smart_request`]).
pub fn playlist_view(p: &pb::PlaylistSummary) -> PlaylistView {
    PlaylistView {
        map: playlist_summary_map(p),
        smart_selection: smart_selection_of(p)
            .map(|sel| crate::we_wallpaper::encode_selection(&sel)),
    }
}

/// Sources of «smart» playlists seen in daemon responses.
///
/// waywallen 0.5.0 distinguishes a curated playlist (an explicit wallpaper list) and
/// a smart one (a selection by filters, `PlaylistSource.smart`). The edit form of the
/// vendor UI 0.4.4 does not know about the second and sends `PlaylistUpdate` without
/// `source` — the daemon reads that as curated from `entry_ids`. On a smart
/// playlist 0.5.0 rejects such a frame («source cannot be converted»),
/// i.e. the name, mode and interval of a smart playlist could not be changed from Rexon,
/// and a daemon without this check would have turned the selection into an empty
/// list.
///
/// The store is process-wide, like [`progress_store`]: responses come to the
/// list and details requests, while the edit frame is built by a third object
/// (`PlaylistMutationQuery`), and dragging the selection through QML for that
/// is not needed.
pub mod playlist_source_store {
    use crate::proto::control as pb;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn store() -> &'static Mutex<HashMap<i64, pb::WallpaperSelection>> {
        static STORE: OnceLock<Mutex<HashMap<i64, pb::WallpaperSelection>>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    use super::smart_selection_of as smart_of;

    /// Remember (or forget) the source of one playlist — the `playlist_get` response.
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

    /// The `playlist_list` response — the full list: deleted playlists are forgotten.
    pub fn remember_all(list: &[pb::PlaylistSummary]) {
        let mut s = store().lock().expect("склад плейлистов не отравлен");
        s.clear();
        for p in list {
            if let Some(sel) = smart_of(p) {
                s.insert(p.id, sel);
            }
        }
    }

    /// Smart playlist selection; `None` — the playlist is curated or not seen.
    pub fn smart(id: i64) -> Option<pb::WallpaperSelection> {
        store()
            .lock()
            .expect("склад плейлистов не отравлен")
            .get(&id)
            .cloned()
    }
}

/// `PlaylistUpdate` frame of the playlist edit form.
///
/// A curated playlist — as before: the contents in `entry_ids`, `source` is not
/// sent (that is how a daemon before 0.5.0 reads it too). A smart one — `source` with its
/// own selection and an empty `entry_ids`: daemon 0.5.0 rejects a frame where
/// both are set, and a smart playlist never has manual contents. This way the name,
/// mode, interval and synchronized selection are edited, while the selection stays.
///
/// Form 0.5.0 saves a smart playlist not with this frame but with `saveSmart`
/// ([`playlist_save_smart_request`]) — with the selection from the page; the substitution
/// here is only needed by `update()` from a form that does not know about the selection.
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

/// `saveSmart` frame — create a smart playlist or update an existing one.
#[derive(Debug, Clone, PartialEq)]
pub enum SmartPlaylistSave {
    Create(pb::PlaylistCreateRequest),
    Update(pb::PlaylistUpdateRequest),
}

impl SmartPlaylistSave {
    /// Protocol `Request` field.
    pub fn field(&self) -> &'static str {
        match self {
            Self::Create(_) => "playlist_create",
            Self::Update(_) => "playlist_update",
        }
    }

    /// Whether to expect the created playlist's id in the response (`captureCreate`).
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

/// The original's `PlaylistMutationQuery::saveSmart` (waywallen 0.5.0).
///
/// `id > 0` — `PlaylistUpdate`, otherwise `PlaylistCreate` (the smart playlist
/// creation page calls it with zero). The source is the page's preview selection
/// as is, as in the original and as in the list request: the daemon gets exactly
/// the selection the page showed the preview by, and rejects an unusable one
/// itself (its error goes to `error`). `entry_ids` is empty: daemon 0.5.0 rejects a frame
/// where both contents and source are set.
///
/// `None` — `selection` is not a selection (the original's `canConvert` is false):
/// nothing is sent, the response is [`playlist_smart_invalid`].
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

/// `saveSmart` with a non-selection: error, Error status and `done()` — in the
/// original's order. The text is here, not in the bridge: human-facing labels live in the
/// core.
pub fn playlist_smart_invalid(st: &mut QueryState) -> Vec<QuerySignal> {
    // The English is verbatim the original's string (`playlist_query.cpp`).
    let mut out = we_query::set_error(st, &crate::i18n::tr("invalid smart playlist selection"));
    out.extend(we_query::set_status(st, STATUS_ERROR));
    out.push(QuerySignal::Property("done"));
    out
}

/// Properties of `PlaylistListQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistListData {
    pub playlists: Vec<PlaylistView>,
}

/// Properties of `PlaylistDetailQuery` (the playlist edit page, 0.3.7).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistDetailData {
    /// The playlist — the `playlist` property.
    pub playlist: PlaylistView,
    /// Edit number — the `revision` property.
    pub revision: i64,
    /// Contents in order — the `entryIds` property.
    pub entry_ids: Vec<String>,
    /// Rows of the `data` model — `we_wallpaper::entry_to_map` maps.
    pub items: Vec<crate::we_settings::VMap>,
}

/// Parse the `playlist_get` response.
///
/// Signal order — as in the original (`PlaylistDetailQuery::reload`):
/// first the wallpaper model is rebuilt, then `playlistChanged` fires.
/// The page rebuilds its draft on it and must ALREADY see the new
/// contents.
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

/// Parse the `playlist_list` response. The playlist is built by [`playlist_view`].
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
        // The signal is unconditional — as in the original.
        out.push(QuerySignal::Property("playlists"));
    }
    Ok(finish(st, out))
}

/// Parse a playlist mutation response.
///
/// `capture_create` — whether to take the created playlist's id from the response.
/// In the original the `done()` signal fires ALWAYS, including on a daemon
/// refusal: QML reads `.status` after it.
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

/// Properties of `SourceListQuery`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceListData {
    pub sources: Vec<Value>,
}

/// Parse the `source_list` response.
///
/// ⚠️ Fixed during the port (rule #16): our Python returned only five
/// keys (`name`/`version`/`libraryLabel`/`libraryHint`/`types`), while
/// the original on this vendor commit also puts `pluginId` and the full
/// `settings` schema (`source_query.cpp` @e5626c1). Without them the source settings
/// page did not know which plugin the source belongs to and which settings it
/// has at all.
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
        // WE tab boundaries: a library source that has only hidden
        // types (glTF folders) does not get into the «Add library» list.
        data.sources = sl
            .sources
            .into_iter()
            .filter(|s| we_query::we_tab_keeps_plugin(&s.types))
            .map(|s| {
                let schemas = |list: Vec<pb::SettingSchema>| -> Vec<Value> {
                    list.into_iter()
                        .map(|ss| setting_schema_map(&SettingSchema::from(ss)))
                        .collect()
                };
                json!({
                    "name": s.name,
                    "version": s.version,
                    "libraryLabel": s.library_label,
                    "libraryHint": s.library_hint,
                    "types": we_query::we_tab_types(s.types),
                    "pluginId": s.plugin_id,
                    "settings": schemas(s.settings),
                    // waywallen 0.6.0: settings of the library itself
                    // (`AddLibraryPage`/`EditLibraryPage`).
                    "librarySettings": schemas(s.library_settings),
                })
            })
            .collect();
        out.push(QuerySignal::Property("sources"));
    }
    Ok(finish(st, out))
}

/// Parse the `library_auto_detect` response.
///
/// ⚠️ Fixed during the port (rule #16): the original has an `addedCount`
/// property (how many libraries were found), while our Python threw the response
/// away entirely (`lambda rsp: None`), and there was nothing to show the number of
/// added libraries with.
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
// Source subscriptions (Steam Workshop and others)
// ════════════════════════════════════════════════════════════════════════

/// What exactly the shim must emit after the `subscription_status` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionStateResult {
    pub state: i32,
    pub error: String,
}

/// Parse the `subscription_status` response for one item.
///
/// Returns `(base signals, outcome)`. There is always an outcome: both on success and
/// on refusal — the original emits `stateLoaded` in both branches, the difference is only
/// in the values (on refusal state 0 and the request's error text).
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

/// Outcome of `subscription_set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionSetResult {
    pub accepted: bool,
    pub error: String,
    /// The state to put into the shared acquisition store.
    pub state: i32,
}

/// Parse the `subscription_set` response.
///
/// States — as in the original: accepted → 2 on subscribe and 1 on unsubscribe,
/// rejected → the other way round.
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
// Plugin actions and editing source settings
// ════════════════════════════════════════════════════════════════════════

/// Outcome of `plugin_action`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginActionResult {
    pub accepted: bool,
    pub error: String,
    pub session_id: String,
}

/// Parse the `plugin_action` response.
///
/// ⚠️ The deviation from the original is DELIBERATE and in our favour: in the original
/// `completed` is emitted only in two branches out of three — on a connection drop and
/// on success. If the daemon answered, but with a refusal status, `inspect_set`
/// does not call the lambda, and `completed` does not fire at all: the action form dialog
/// hangs open forever. Here `completed(false, error, "")` is emitted in
/// this branch too — just as our Python bridge did.
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

/// Outcome of `remote_settings_patch`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchResult {
    pub accepted: bool,
    pub error: String,
}

/// Parse the `remote_settings_patch` response.
///
/// ⚠️ The deviation from the original is DELIBERATE and in our favour: the original
/// emits `completed(..., accepted = true, "")` RIGHT after `inspect_set`,
/// without looking at the response status — i.e. an edit rejected by the daemon
/// was reported to the page as accepted, and the page silently considered the settings
/// saved. Here a refusal gives `accepted = false` with the error
/// text, as our Python bridge did.
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

/// A `QVariantMap` map from QML → a protocol string map.
///
/// Port of the original's `wire_values` loop: a value of any type is converted to
/// a string, `null` becomes an empty string.
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
            // Qt prints bool as "true"/"false" — QVariant::toString().
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        other => other.to_string(),
    }
}

/// A `QVariantList` list from QML → a protocol string list.
pub fn list_to_strings(values: &Value) -> Vec<String> {
    match values {
        Value::Array(items) => items.iter().map(value_to_wire_string).collect(),
        _ => Vec::new(),
    }
}

/// A `QVariantList` list from QML → a list of unsigned screen numbers.
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

/// The v0.3.5 list of apply targets from what QML assigned.
///
/// Port of `presentationTargetsFromVariant` (the vendor's `ui/src/proto.cpp`):
/// each entry is a map EITHER with `canvasId` OR with `displayId`; a non-empty
/// `canvasId` wins, and an entry without a usable id
/// is dropped (at the vendor `display_id == 0` means «no value»).
///
/// There is exactly one point of building targets for the whole tab (rule #12): since v0.3.5
/// the `display_ids` fields of `wallpaper_apply` and of both playlist commands
/// changed TYPE while keeping the number — the daemon drops an old-shape frame silently,
/// so targets must not be built «somewhere else nearby».
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

/// An empty map — so the wrapper does not build it by hand.
pub fn empty_map() -> Map<String, Value> {
    Map::new()
}

// ════════════════════════════════════════════════════════════════════════
// Batch 7: global pause/sound/stop and canvases (v0.3.5)
// ════════════════════════════════════════════════════════════════════════
//
// The reference is the vendor's `ui/src/query/global_pause_query.cpp` and
// `ui/src/query/display_query.cpp` at tag v0.3.5 (f7baecd), the same tag
// our QML is vendored from.

/// Which of the four global toggles the daemon is answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalFlag {
    /// `global_pause_toggle` — the daemon decides itself what to switch to.
    PauseToggle,
    /// `global_pause_set`.
    PauseSet,
    /// `global_mute_set`.
    MuteSet,
    /// `global_stop_set`.
    StopSet,
}

impl GlobalFlag {
    /// Extract the value from the daemon response if it holds the expected payload.
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

/// Parsing the response to any of the four global toggles.
///
/// Returns, along with the signals, the value the daemon NAMED — it is
/// needed by `GlobalPauseToggleQuery`: in the original the `toggled(paused)` signal
/// fires on every successful response, even if the property did not change
/// (the button in the interface shows the «Paused»/«Resumed» toast
/// regardless of whether the value matched the previous one).
///
/// `None` in the second field — the response is not about us (the daemon got the field wrong) or the daemon
/// refused: then neither the property nor the signal may be touched.
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

/// How a canvas edit ended — exactly the original's three outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasMutation {
    /// `canvasCreated(id)`.
    Created(String),
    /// `canvasUpdated(id, revision)`.
    Updated(String, u64),
    /// `canvasDeleted(id)`. The daemon does not repeat the canvas id in the response —
    /// the request itself remembers it from the moment the frame was sent.
    Deleted(String),
    /// The daemon refused or answered with the wrong field: no signals.
    None,
}

/// Parsing the response to creating/editing/deleting a canvas.
///
/// `deleted_id` — the canvas id from the SENT delete frame (in the
/// original it is also captured into the closure before sending).
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

/// A canvas member rectangle from a QML map (`{settingsKey, x, y,
/// width, height}`) — that is how `CanvasEditorState.members` puts it.
///
/// The keys are camelCase precisely: the map is built by the vendor QML, not by the protocol.
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
            // The member's aspect lock — a protocol 0.3.7 field. Taken from
            // the same QML map; the current vendor QML does not put the key, and
            // then it is `false` here — exactly the current frame (a zero bool in
            // proto3 is not serialized).
            aspect_locked: item
                .get("aspectLocked")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

/// A number from QML: it arrives both as an integer and as a fraction (JS does not distinguish).
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
        // Beta < org.c (an empty name is replaced by the id), the system Zeta goes last.
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

    /// WE tab boundaries: a source with only glTF does not get into «Add library»,
    /// a mixed one has the type itself struck out, a source without types
    /// stays.
    #[test]
    fn source_list_hides_gltf_only_sources() {
        let src = |name: &str, types: &[&str]| pb::SourcePluginInfo {
            name: name.into(),
            types: types.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        };
        let payload = pb::response::Payload::SourceList(pb::SourceListResponse {
            sources: vec![
                src("image", &["image"]),
                src("gltf", &["gltf"]),
                src("mixed", &["video", "gltf"]),
                src("untyped", &[]),
            ],
        });
        let mut st = QueryState::default();
        let mut data = SourceListData::default();
        apply_source_list(&mut st, &mut data, &frame(payload)).expect("кадр разобран");
        let names: Vec<&str> = data.sources.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["image", "mixed", "untyped"]);
        assert_eq!(data.sources[1]["types"], serde_json::json!(["video"]));
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
        // Daemon refusal: status is not OK.
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
        // A snapshot of a PREVIOUS task with the same id arrived BEFORE the start of
        // our request — it must not close it.
        let stale = progress_store::record("q1", 1.0, false, true, true, "старая ошибка");

        let mut st = QueryState::default();
        let mut ps = ProgressState::default();
        let begin = begin_progress(&mut st, &mut ps);
        assert!(begin.contains(&QuerySignal::Property("progressing")));
        assert_eq!(ps.begin_sequence, stale.sequence);

        // Task acceptance: an old snapshot is in the store, but it is not newer than the start.
        accept_progress(&mut st, &mut ps, "q1");
        assert_eq!(st.status, STATUS_QUERYING);
        assert!(!progress_matches(&ps, "q1", stale.sequence));
        let snapshot = progress_store::record("q1", 1.0, false, true, false, "");
        assert!(progress_matches(&ps, "q1", snapshot.sequence));
        let signals = apply_task_progress(&mut st, &mut ps, &snapshot);
        assert!(signals.contains(&QuerySignal::Property(PROGRESS_ENDED)));
        assert_eq!(st.status, we_query::STATUS_FINISHED);
    }

    // ── batch 7: global toggles and canvases ─────────────────────────

    #[test]
    fn глобальная_пауза_берёт_значение_от_демона_а_не_своё() {
        let mut st = QueryState::default();
        let mut value = false;
        // The client asked for «pause», the daemon answered «not paused» — the daemon wins.
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
        // The property did not change, but the toast in the interface must be shown.
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

        // The daemon does not repeat the deleted canvas id — the request remembers it.
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
        // Missing keys are zeros, not a parse failure.
        let rect = got[1].rect.as_ref().expect("прямоугольник обязателен");
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (0, 0, 0, 0));
    }

    /// waywallen 0.5.0: «Save» in the smart playlist edit form must
    /// neither turn it into a curated one nor break on a daemon refusal.
    /// The store is process-wide, so the whole scenario is in one test.
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

        // Smart: the selection goes back, no manual contents (form 0.4.4
        // sends an empty list — daemon 0.5.0 would reject a frame with both fields).
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

        // Curated — as before: contents in `entry_ids`, `source` is not sent.
        let req = playlist_update_request(7002, "Избранное", 1, 0, false, vec!["5".into(), "6".into()], 0);
        assert_eq!(req.entry_ids, vec!["5".to_string(), "6".to_string()]);
        assert!(req.source.is_none());

        // The details of one playlist also update the store: a smart one became
        // curated in another client — the selection is no longer substituted.
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

        // The playlist vanished from the full list — forgotten.
        let list = pb::response::Payload::PlaylistList(pb::PlaylistListResponse {
            playlists: vec![curated],
        });
        apply_playlist_list(&mut st, &mut data, &frame(list)).expect("кадр разобран");
        assert!(playlist_source_store::smart(7001).is_none());
    }

    /// A smart playlist selection with a usable rule and sort and with
    /// unfilled ones: a rule without a value and a sort without a key. They
    /// are rejected by the daemon, not by Rexon — they must reach the daemon.
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

    /// waywallen 0.5.0, `saveSmart`: id > 0 — `PlaylistUpdate`, otherwise
    /// `PlaylistCreate` with capturing the id; the source is the selection as is (not
    /// a single rule is lost), contents empty; not a selection — no frame,
    /// an error and `done()`.
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
        // The frame bytes are the same as the request's prost encoding.
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

        // An empty selection is a selection too (the tag is there, no encoding).
        let empty = crate::we_wallpaper::encode_selection(&pb::WallpaperSelection::default());
        assert!(playlist_save_smart_request(0, "Все", 1, 0, true, &empty, 0).is_some());

        // Not a selection: empty, a string, a foreign encoding without the tag.
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

    /// waywallen 0.5.0: a smart playlist map carries its selection as the same
    /// opaque value as `WallpaperListQuery.selection` — as the daemon
    /// sent it, without cleaning; a curated one has no selection. The list and the
    /// details build the playlist with one function.
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

        // `restoreSelection(smartSelection)` of the preview gives the same selection —
        // `matchesSelection(baseline)` right after restoring is true.
        let mut preview = crate::we_wallpaper::WallpaperListData::default();
        let _ = crate::we_wallpaper::restore_selection(&mut preview, &bytes);
        assert!(crate::we_wallpaper::matches_selection(&preview, &bytes));

        // Curated and a playlist without a source (daemon before 0.5.0) — without the key.
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

        // Details: the same playlist.
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
