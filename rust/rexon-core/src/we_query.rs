//! The `Query` base and simple requests of the Wallpaper Engine tab — port
//! of waywallen's `qextra::QAsyncResult`/`qextra::Query` + `ui/src/query/{health,renderer,
//! tag}_query.cpp` (MIT, (c) hypengw; attribution in NOTICE).
//!
//! The base is a state machine of an asynchronous result: status
//! (Uninitialized/Querying/Finished/Error), the error text, a flag for
//! forwarding the error to the global handler. All the deciding part lives
//! here: the state transition and the ORDER of Qt signals the shim must emit.
//! Descendants add parsing of their response: raw frame bytes →
//! prost → new property values + `<name>Changed` signals.
//!
//! Why not cxx-qt: it cannot inherit a Rust QObject from a
//! Rust QObject, and the whole request hierarchy of the tab is exactly
//! inheritance from one base. So the base state is EMBEDDED as a struct
//! ([`QueryState`]) into every request, while the QObjects remain the
//! PySide6 wrappers in `qml_ui/bridge.py` — just like `Notify`/`Util`.
//!
//! ⚠️ Deviations from the original (deliberate, rule #16):
//!   1. Property signals (`serviceChanged`, `tagsChanged`, …) are emitted
//!      ONLY when the value really changes. The original emits them
//!      unconditionally on every successful response (`Q_EMIT self->tagsChanged()`
//!      right after the assignment) — i.e. it re-evaluates QML bindings
//!      even when the daemon returned the same list. Same class of issue that
//!      was fixed in `Notify` (`statusChanged` on every StatusSync).
//!      Observable behaviour does not change: no `on<Property>Changed`
//!      handler in the vendored QML subscribes to these signals (checked
//!      by grep over `ui/qml`), they only feed property bindings, and a
//!      binding needs a signal only on a change.
//!   2. `HealthQuery` in our former Python bridge ALREADY compared
//!      values, while `TagListQuery`/`RendererListQuery` did not. Now
//!      there is one rule for all requests (rule #12: fix the engine, not
//!      individual places).
//!
//! Transport difference: the original's `reload()` first sets Querying and
//! only then learns about a send error; we check the connection
//! BEFORE sending ([`begin_call`]), so with a dead daemon the status goes
//! straight to Error, skipping Querying. This is our branch (the original
//! has none — it always "sends" to the backend), kept as it was in the bridge.

use crate::proto::control as pb;
use crate::we_notify::{plugin_text, PluginText};
use prost::Message;
use serde::{ser::SerializeMap, Serialize, Serializer};

/// `qextra::QAsyncResult::Status` — Qt enum values as they are
/// (QML reads them as numbers via `Query.Status`).
pub const STATUS_UNINITIALIZED: i32 = 0;
/// The request was sent, no response yet.
pub const STATUS_QUERYING: i32 = 1;
/// The response was received and parsed.
pub const STATUS_FINISHED: i32 = 2;
/// Connection, transport or daemon error.
pub const STATUS_ERROR: i32 = 3;

/// The text the bridge uses to explain a missing daemon connection.
///
/// An English catalog msgid; the screen gets the translation (`tr`) in the
/// [`set_error`] call below.
pub const NO_CONNECTION: &str = crate::i18n::n_("no connection to the daemon");

/// State of the `Query` base, embedded into every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryState {
    pub status: i32,
    pub error: String,
    /// `forwardError` — whether to duplicate the error to the global handler
    /// (`App.errorOccurred` → toast in Window.qml). Original: `true` by
    /// default, pages with their own error display set `false`.
    pub forward_error: bool,
}

impl Default for QueryState {
    fn default() -> Self {
        QueryState {
            status: STATUS_UNINITIALIZED,
            error: String::new(),
            forward_error: true,
        }
    }
}

/// A Qt signal the shim must emit (strictly in the returned order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySignal {
    /// `statusChanged(Status)` — the argument is read by QML handlers of the
    /// form `function onStatusChanged(status)`.
    StatusChanged(i32),
    /// `queryingChanged()` — the derived property `status == Querying`.
    QueryingChanged,
    /// `errorChanged()`.
    ErrorChanged,
    /// `forwardErrorChanged()`.
    ForwardErrorChanged,
    /// `finished()` — the status became Finished.
    Finished,
    /// `errorOccurred(error)` — the status became Error.
    ErrorOccurred(String),
    /// Global forwarding: `App.errorOccurred(error)`. A separate
    /// variant, because the signal is emitted NOT by the request but by the App singleton.
    AppErrorOccurred(String),
    /// `<name>Changed()` of a specific request (`service`, `tags`, …).
    Property(&'static str),
}

/// The outward shape of a signal — a map `{signal: name, ...arguments}`, exactly
/// the one the shim and the tab read.
///
/// ⚠️ `Serialize` is written BY HAND on purpose, and here is why: three variants
/// carry a bare scalar payload (`StatusChanged(i32)`,
/// `ErrorOccurred(String)`, `Property(&str)`), and `#[serde(tag = "...")]`
/// does not work with those at all — an internal tag requires the content
/// to serialize as a map itself. Rewriting the enum with named
/// fields just for output means 126 edits in fifteen files, including foreign ones;
/// the cost is not worth the gain. Instead the shape is declared ONCE, right next
/// to the type: a new enum variant has to appear here as well (the compiler
/// will demand a branch), not in the bridge beyond the crate boundary.
impl Serialize for QuerySignal {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let (name, arg): (&str, Option<(&str, Arg<'_>)>) = match self {
            QuerySignal::StatusChanged(v) => ("statusChanged", Some(("status", Arg::Int(*v)))),
            QuerySignal::QueryingChanged => ("queryingChanged", None),
            QuerySignal::ErrorChanged => ("errorChanged", None),
            QuerySignal::ForwardErrorChanged => ("forwardErrorChanged", None),
            QuerySignal::Finished => ("finished", None),
            QuerySignal::ErrorOccurred(t) => ("errorOccurred", Some(("error", Arg::Str(t)))),
            QuerySignal::AppErrorOccurred(t) => ("appErrorOccurred", Some(("error", Arg::Str(t)))),
            QuerySignal::Property(n) => ("property", Some(("name", Arg::Str(n)))),
        };
        let mut m = ser.serialize_map(Some(1 + usize::from(arg.is_some())))?;
        m.serialize_entry("signal", name)?;
        if let Some((key, value)) = arg {
            match value {
                Arg::Int(v) => m.serialize_entry(key, &v)?,
                Arg::Str(v) => m.serialize_entry(key, v)?,
            }
        }
        m.end()
    }
}

/// A signal payload of one of two kinds (see [`QuerySignal`]).
enum Arg<'a> {
    Int(i32),
    Str(&'a str),
}

/// Error text from a daemon response: `message`, and when empty — "status N"
/// (that is what the original's `backend.cpp` does, so the UI never shows emptiness).
pub fn response_error_text(status: i32, message: &str) -> String {
    if message.is_empty() {
        format!("status {status}")
    } else {
        message.to_string()
    }
}

/// The base's `setError`: a bindable property, signal only on change.
pub fn set_error(st: &mut QueryState, text: &str) -> Vec<QuerySignal> {
    if st.error == text {
        return Vec::new();
    }
    st.error = text.to_string();
    vec![QuerySignal::ErrorChanged]
}

/// The base's `setForwardError`.
pub fn set_forward_error(st: &mut QueryState, value: bool) -> Vec<QuerySignal> {
    if st.forward_error == value {
        return Vec::new();
    }
    st.forward_error = value;
    vec![QuerySignal::ForwardErrorChanged]
}

/// The base's `setStatus` — the whole emission order of the original in one place.
///
/// `QAsyncResult` attaches a lambda to `statusChanged` in its constructor:
/// first `finished()`/`errorOccurred(error)`, and only then, if
/// `forwardError` is set, the error goes to the global handler.
/// `querying` is a derived property (`status == Querying`), its signal
/// fires only when the derived value really changed.
pub fn set_status(st: &mut QueryState, value: i32) -> Vec<QuerySignal> {
    if st.status == value {
        return Vec::new();
    }
    let was_querying = st.status == STATUS_QUERYING;
    st.status = value;
    let mut out = vec![QuerySignal::StatusChanged(value)];
    if was_querying != (value == STATUS_QUERYING) {
        out.push(QuerySignal::QueryingChanged);
    }
    if value == STATUS_FINISHED {
        out.push(QuerySignal::Finished);
    } else if value == STATUS_ERROR {
        out.push(QuerySignal::ErrorOccurred(st.error.clone()));
        if st.forward_error {
            out.push(QuerySignal::AppErrorOccurred(st.error.clone()));
        }
    }
    out
}

/// Start of a request: `(whether to send the frame, signals)`.
///
/// No live connection — the frame is not sent, the request goes straight to
/// Error with the [`NO_CONNECTION`] text (our transport's branch, see the header).
pub fn begin_call(st: &mut QueryState, connected: bool) -> (bool, Vec<QuerySignal>) {
    if !connected {
        let mut out = set_error(st, &crate::i18n::tr(NO_CONNECTION));
        out.extend(set_status(st, STATUS_ERROR));
        return (false, out);
    }
    (true, set_status(st, STATUS_QUERYING))
}

/// Take the `Response` off a raw server frame and handle a non-OK status.
///
/// `None` in the first field = the daemon answered with an error, the state is
/// already switched to Error and the payload need not be read (likewise the
/// original's `inspect_set` calls the parsing lambda only on success).
///
/// A broken frame is `Err`: the shim logs debug output and carries on (the daemon
/// may be newer than us, crashing in the middle of receiving is not allowed). A
/// frame with an event instead of a response is impossible for a request (the
/// client routes responses by `request_id`), but is treated the same — as unparsed.
pub(crate) fn take_response(
    st: &mut QueryState,
    frame_bytes: &[u8],
) -> Result<(Option<pb::Response>, Vec<QuerySignal>), prost::DecodeError> {
    let frame = pb::ServerFrame::decode(frame_bytes)?;
    let Some(pb::server_frame::Kind::Response(rsp)) = frame.kind else {
        return Err(prost::DecodeError::new("кадр сервера без ответа"));
    };
    if rsp.status != pb::Status::Ok as i32 {
        let text = response_error_text(rsp.status, &rsp.message);
        let mut out = set_error(st, &text);
        out.extend(set_status(st, STATUS_ERROR));
        return Ok((None, out));
    }
    Ok((Some(rsp), Vec::new()))
}

/// Common tail of a successful parse: properties are already updated — status to
/// Finished.
pub fn finish(st: &mut QueryState, mut out: Vec<QuerySignal>) -> Vec<QuerySignal> {
    out.extend(set_status(st, STATUS_FINISHED));
    out
}

/// Parse a response to an action request: there is no payload, only the
/// outcome matters.
///
/// In the original this is `inspect_set(result, [](const Response&) {})` — an empty
/// parsing lambda: the command is either accepted by the daemon (status Finished)
/// or rejected (Error with its text). One point for all such requests of the tab
/// (rule #12), not a copy in each.
pub fn apply_ack(
    st: &mut QueryState,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, out) = take_response(st, frame_bytes)?;
    if answer.is_none() {
        return Ok(out);
    }
    Ok(finish(st, out))
}

/// Assign a value and mark the signal if it changed.
pub fn assign<T: PartialEq>(
    slot: &mut T,
    value: T,
    name: &'static str,
    out: &mut Vec<QuerySignal>,
) {
    if *slot != value {
        *slot = value;
        out.push(QuerySignal::Property(name));
    }
}

// ── HealthQuery (the «Status» page, Window.qml) ─────────────────────────

/// Properties of `HealthQuery`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthData {
    pub service: String,
    pub state: String,
    /// The system the daemon runs on (v0.3.5). The vendor «Status» page
    /// shows the line and hides the label while it is empty
    /// (`StatusPage.qml:222`).
    pub os_name: String,
}

/// Parse the `health` response.
pub fn apply_health(
    st: &mut QueryState,
    data: &mut HealthData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::Health(h)) = rsp.payload {
        assign(&mut data.service, h.service, "service", &mut out);
        assign(&mut data.state, h.state, "state", &mut out);
        assign(&mut data.os_name, h.os_name, "osName", &mut out);
    }
    Ok(finish(st, out))
}

// ── DaemonLogQuery (the «Daemon log» dialog, v0.3.8) ────────────────────

/// Properties of `DaemonLogQuery`.
///
/// waywallen 0.3.8 learned to hand out its log in one request
/// (`log_read`), and the «Status» page to show it in a window. There are three
/// fields, all three straight from the response: the file path, its contents and
/// a flag that the daemon returned not the whole file but its tail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonLogData {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

/// Parse the `log_read` response.
pub fn apply_log_read(
    st: &mut QueryState,
    data: &mut DaemonLogData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::LogRead(l)) = rsp.payload {
        assign(&mut data.path, l.path, "path", &mut out);
        assign(&mut data.content, l.content, "content", &mut out);
        assign(&mut data.truncated, l.truncated, "truncated", &mut out);
    }
    Ok(finish(st, out))
}

// ── RendererListQuery (the «Status» page) ───────────────────────────────

/// `RendererInstance` in the form QML reads it (map keys of the
/// bridge's `_renderer_info_dict`: `id` instead of `renderer_id`, the rest
/// camelCase).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererInstance {
    pub id: String,
    pub fps: u32,
    pub status: String,
    pub running: bool,
    pub keep: bool,
    pub name: String,
    pub pid: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    pub drm_render_major: u32,
    pub drm_render_minor: u32,
    pub last_exit_reason: String,
    pub process_generation: u64,
    #[serde(rename = "runtimeConditions")]
    pub conditions: Vec<RuntimeCondition>,
    pub runtime_tags: Vec<RuntimeTag>,
}

/// One `RuntimeCondition` in the form QML reads it: string
/// `kind`/`origin` instead of enum numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCondition {
    pub kind: String,
    pub origin: String,
    pub reason: String,
    pub related_renderer_id: String,
    pub related_display_id: u64,
}

/// A `key`/`value` pair from `RendererRuntimeTag`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeTag {
    pub key: String,
    pub value: String,
}

/// The renderer's `status` string from the `RendererState` tree (v0.3.5).
///
/// Before v0.3.5 the daemon sent a ready string in `RendererInstance.status`
/// (field 3). In v0.3.5 the field moved to `reserved`, and the state arrives
/// as a `RendererState` tree; the vendor QML still reads
/// `status` and `running` (`StatusPage.qml:344/347/374`). So we build the
/// string ourselves — with ONE function for all consumers (the
/// `renderer_list` response here and the live models of `rexon-gui`), rule #12.
///
/// One-to-one match with the vendor's `Renderer::status`
/// (`ui/src/objmodel/renderer.cpp`), including the empty string for an unknown
/// state: on it the vendor QML hides the state label entirely.
pub fn renderer_status(info: &pb::RendererInstance) -> &'static str {
    let Some(kind) = info.state.as_ref().and_then(|s| s.kind.as_ref()) else {
        return "";
    };
    match kind {
        pb::renderer_state::Kind::Starting(_) => "starting",
        pb::renderer_state::Kind::Running(r) => {
            match pb::RendererActivity::try_from(r.activity) {
                Ok(pb::RendererActivity::Playing) => "playing",
                Ok(pb::RendererActivity::Paused) => "paused",
                Ok(pb::RendererActivity::Muted) => "muted",
                _ => "",
            }
        }
        pb::renderer_state::Kind::Stopping(_) => "stopping",
        pb::renderer_state::Kind::Stopped(_) => "stopped",
        pb::renderer_state::Kind::Killed(_) => "killed",
        pb::renderer_state::Kind::Failed(_) => "failed",
    }
}

/// The renderer's `running` — port of `Renderer::running`: only a working
/// process counts as alive (playing, paused or muted).
pub fn renderer_running(status: &str) -> bool {
    matches!(status, "playing" | "paused" | "muted")
}

/// `keep` — port of `Renderer::keep`: the process is held after stopping.
pub fn renderer_keep(info: &pb::RendererInstance) -> bool {
    match info.state.as_ref().and_then(|s| s.kind.as_ref()) {
        Some(pb::renderer_state::Kind::Stopping(s)) => s.keep,
        Some(pb::renderer_state::Kind::Stopped(s)) => s.keep,
        Some(pb::renderer_state::Kind::Killed(s)) => s.keep,
        _ => false,
    }
}

/// `processGeneration` — port of the vendor property of the same name.
pub fn renderer_process_generation(info: &pb::RendererInstance) -> u64 {
    match info.state.as_ref().and_then(|s| s.kind.as_ref()) {
        Some(pb::renderer_state::Kind::Starting(s)) => s.generation,
        Some(pb::renderer_state::Kind::Running(s)) => s.generation,
        Some(pb::renderer_state::Kind::Stopping(s)) => s.generation,
        _ => 0,
    }
}

/// `lastExitReason` — port of the vendor property of the same name.
pub fn renderer_last_exit_reason(info: &pb::RendererInstance) -> String {
    let reason = |exit: Option<&pb::RendererExit>| {
        exit.map(|e| e.reason.clone()).unwrap_or_default()
    };
    match info.state.as_ref().and_then(|s| s.kind.as_ref()) {
        Some(pb::renderer_state::Kind::Stopped(s)) => reason(s.last_exit.as_ref()),
        Some(pb::renderer_state::Kind::Killed(s)) => reason(s.last_exit.as_ref()),
        Some(pb::renderer_state::Kind::Failed(s)) => reason(s.failure.as_ref()),
        _ => String::new(),
    }
}

/// `RuntimeCondition` → QML shape — port of the vendor's `runtimeConditionsFromPb`
/// (`ui/src/proto.cpp`). Unknown enum values
/// degrade to `issue`/`unknown`, as in the original.
pub fn runtime_conditions(list: &[pb::RuntimeCondition]) -> Vec<RuntimeCondition> {
    list.iter()
        .map(|c| RuntimeCondition {
            kind: match pb::RuntimeConditionKind::try_from(c.kind) {
                Ok(pb::RuntimeConditionKind::RuntimeConditionLoading) => "loading",
                Ok(pb::RuntimeConditionKind::RuntimeConditionWaiting) => "waiting",
                Ok(pb::RuntimeConditionKind::RuntimeConditionHang) => "hang",
                _ => "issue",
            }
            .to_string(),
            origin: match pb::RuntimeConditionOrigin::try_from(c.origin) {
                Ok(pb::RuntimeConditionOrigin::Renderer) => "renderer",
                Ok(pb::RuntimeConditionOrigin::Display) => "display",
                Ok(pb::RuntimeConditionOrigin::Release) => "release",
                _ => "unknown",
            }
            .to_string(),
            reason: c.reason.clone(),
            related_renderer_id: c.related_renderer_id.clone(),
            related_display_id: c.related_display_id,
        })
        .collect()
}

/// `RendererRuntimeTag` → QML shape — port of the vendor's `runtimeTagsFromPb`.
pub fn runtime_tags(list: &[pb::RendererRuntimeTag]) -> Vec<RuntimeTag> {
    list.iter()
        .map(|t| RuntimeTag { key: t.key.clone(), value: t.value.clone() })
        .collect()
}

impl From<pb::RendererInstance> for RendererInstance {
    fn from(i: pb::RendererInstance) -> Self {
        let status = renderer_status(&i);
        RendererInstance {
            id: i.renderer_id.clone(),
            fps: i.fps,
            status: status.to_string(),
            running: renderer_running(status),
            keep: renderer_keep(&i),
            name: i.name.clone(),
            pid: i.pid,
            texture_width: i.texture_width,
            texture_height: i.texture_height,
            drm_render_major: i.drm_render_major,
            drm_render_minor: i.drm_render_minor,
            last_exit_reason: renderer_last_exit_reason(&i),
            process_generation: renderer_process_generation(&i),
            conditions: runtime_conditions(&i.conditions),
            runtime_tags: runtime_tags(&i.runtime_tags),
        }
    }
}

/// Properties of `RendererListQuery`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererListData {
    pub renderers: Vec<String>,
    pub instances: Vec<RendererInstance>,
}

/// Parse the `renderer_list` response.
///
/// Syncing `RendererManager` stays with the shim: the manager holds the
/// live renderer QObjects, their life cycle is the Qt part. The shim
/// feeds it the same `instances` list.
pub fn apply_renderer_list(
    st: &mut QueryState,
    data: &mut RendererListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::RendererList(rl)) = rsp.payload {
        assign(&mut data.renderers, rl.renderers, "renderers", &mut out);
        let instances: Vec<RendererInstance> =
            rl.instances.into_iter().map(RendererInstance::from).collect();
        assign(&mut data.instances, instances, "instances", &mut out);
    }
    Ok(finish(st, out))
}

// ── RendererPluginListQuery («Status» page, wallpaper details, «Wallpapers») ──

/// `SettingSchema` — the schema of one renderer plugin setting.
///
/// ⚠️ Map keys in the bridge are SNAKE_CASE, not camelCase: `SettingField.qml`
/// is written against the original's `default_value`/`label_key`/`description_key`
/// (`plugin_query.cpp`), it would not read camelCase.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SettingSchema {
    pub key: String,
    /// ⚠️ Exposed under the name `type`: that is how `SettingField.qml` reads it.
    #[serde(rename = "type")]
    pub value_type: i32,
    pub default_value: String,
    pub identity: bool,
    pub label_key: String,
    pub description_key: String,
    pub min: String,
    pub max: String,
    pub step: String,
    pub choices: Vec<String>,
    pub group: String,
    pub order: i32,
    /// Translatable label from the plugin catalog (`SettingSchema.label`,
    /// protocol field 13). Fallback text — `label_key`, as in the vendor's
    /// `pluginMessageFromPb`.
    ///
    /// ⚠️ Without these three fields the plugin settings page showed the
    /// user a TECHNICAL key instead of a label: `SettingField.qml` reads
    /// `schema.label`/`schema.description`, and the group header —
    /// `schema.group_label`. Since release v0.3.9 the loss became more visible:
    /// the vendor stopped printing an unresolved key as the label
    /// (`isUnresolvedKey`), and the description disappeared altogether.
    pub label: PluginText,
    /// Translatable description (`SettingSchema.description`, field 14);
    /// fallback text — `description_key`.
    pub description: PluginText,
    /// Translatable group header (`SettingSchema.group_label`,
    /// field 15); fallback text — `group`.
    pub group_label: PluginText,
    /// waywallen 0.6.0: translatable labels of choice options (field 16), one
    /// per `choices` entry. No label — `SettingField.qml` shows the value
    /// itself.
    pub choice_labels: Vec<PluginText>,
}

impl From<pb::SettingSchema> for SettingSchema {
    fn from(s: pb::SettingSchema) -> Self {
        SettingSchema {
            key: s.key,
            value_type: s.r#type,
            default_value: s.default_value,
            identity: s.identity,
            label: plugin_text(s.label.as_ref(), &s.label_key),
            description: plugin_text(s.description.as_ref(), &s.description_key),
            group_label: plugin_text(s.group_label.as_ref(), &s.group),
            label_key: s.label_key,
            description_key: s.description_key,
            min: s.min,
            max: s.max,
            step: s.step,
            choices: s.choices,
            choice_labels: s.choice_labels.iter().map(|m| plugin_text(Some(m), "")).collect(),
            group: s.group,
            order: s.order,
        }
    }
}

// ── Boundaries of the «Wallpaper Engine» tab ────────────────────────────
//
// Wehrwolfmann's decision (06.11.2026): the tab is a copy of the original
// Wallpaper Engine. It holds the Steam Workshop (the OWE plugin), local videos and
// pictures, playlists, and nothing beyond that. The waywallen daemon carries more:
// the Wallhaven catalog (already in v0.5.0), Sketchfab and the «gltf» wallpaper type (in main
// after v0.5.0). Wallhaven moves to «Appearance → HD wallpapers», 3D wallpapers
// also belong outside this tab.
//
// The filtering happens here, in our port of the requests, not as a patch to the
// vendor QML: a patch conflicts on every vendor update, while these functions
// are not touched by `rexon-vendor-update`.

/// Plugins whose «Discover» catalogs the tab shows (`owner_plugin_id`
/// from `RemoteSourceInfo`).
///
/// ⭐ An allow list, not a deny list: a catalog upstream adds tomorrow
/// does NOT get into the tab by default. An empty owner does not pass either:
/// it is unknown whose catalog it is.
pub const WE_TAB_REMOTE_OWNERS: &[&str] = &[crate::engine_update::OWE_PLUGIN_ID];

/// Wallpaper types the tab does not have: neither in the type filter, nor in the
/// renderer list, nor among library sources, nor in the library itself.
///
/// Here the list is a deny list, and that is deliberate: the original's wallpaper
/// types («scene», «video», «web», «application», «image» and whatever else OWE
/// hands out) are not fully enumerated, and an allow list would hide the user's
/// wallpapers at the first unfamiliar type. Add any new upstream type beyond WE
/// here when updating the vendor.
pub const WE_TAB_HIDDEN_TYPES: &[&str] = &["gltf"];

/// Whether the tab shows the «Discover» catalog of this owner plugin.
pub fn we_tab_shows_remote_owner(owner_plugin_id: &str) -> bool {
    WE_TAB_REMOTE_OWNERS.contains(&owner_plugin_id)
}

/// Whether the tab shows the wallpaper type.
pub fn we_tab_shows_type(wp_type: &str) -> bool {
    !WE_TAB_HIDDEN_TYPES.contains(&wp_type)
}

/// Types without the hidden ones, order preserved.
pub fn we_tab_types(types: Vec<String>) -> Vec<String> {
    types.into_iter().filter(|t| we_tab_shows_type(t)).collect()
}

/// Whether a plugin with this set of types stays in the tab (a renderer or a
/// library source): yes, if it has at least one visible type. An empty
/// set is no reason to hide — nothing is known from it.
pub fn we_tab_keeps_plugin(types: &[String]) -> bool {
    types.is_empty() || types.iter().any(|t| we_tab_shows_type(t))
}

/// `skip_types` of the library request: the user's choice plus the hidden types.
///
/// The hidden ones are added only to the frame, the `skipTypes` property stays the
/// user's choice: it goes to the settings and to the quick filter.
pub fn we_tab_skip_types(user: &[String]) -> Vec<String> {
    let mut out = user.to_vec();
    for t in WE_TAB_HIDDEN_TYPES {
        if !out.iter().any(|u| u == t) {
            out.push((*t).to_string());
        }
    }
    out
}

/// `RendererPluginInfo` as a bridge map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererPlugin {
    pub name: String,
    pub bin: String,
    pub types: Vec<String>,
    pub priority: u32,
    pub version: String,
    pub settings: Vec<SettingSchema>,
    pub plugin_id: String,
}

impl From<pb::RendererPluginInfo> for RendererPlugin {
    fn from(p: pb::RendererPluginInfo) -> Self {
        RendererPlugin {
            name: p.name,
            bin: p.bin,
            types: p.types,
            priority: p.priority,
            version: p.version,
            settings: p.settings.into_iter().map(SettingSchema::from).collect(),
            plugin_id: p.plugin_id,
        }
    }
}

/// Properties of `RendererPluginListQuery`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererPluginListData {
    pub renderers: Vec<RendererPlugin>,
    pub supported_types: Vec<String>,
    /// waywallen 0.4.2: wallpaper type labels `key → label` — only for
    /// the keys whose renderer declared a label. No key — the page
    /// shows the key itself, as before the field appeared.
    pub type_labels: std::collections::BTreeMap<String, PluginText>,
}

/// Parse the `renderer_plugin_list` response.
pub fn apply_renderer_plugin_list(
    st: &mut QueryState,
    data: &mut RendererPluginListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::RendererPluginList(pl)) = rsp.payload {
        // Tab boundaries: a renderer that supports only hidden types (gltf)
        // is not shown; the others have the hidden types struck out.
        let renderers: Vec<RendererPlugin> = pl
            .renderers
            .into_iter()
            .filter(|r| we_tab_keeps_plugin(&r.types))
            .map(|r| {
                let mut r = RendererPlugin::from(r);
                r.types = we_tab_types(r.types);
                r
            })
            .collect();
        assign(&mut data.renderers, renderers, "renderers", &mut out);
        assign(
            &mut data.supported_types,
            we_tab_types(pl.supported_types),
            "supportedTypes",
            &mut out,
        );
        // Port of `renderer_query.cpp` v0.4.2: an empty key is skipped,
        // the label is `pluginMessageFromPb(label_text, label)`.
        let type_labels = pl
            .type_labels
            .iter()
            .filter(|t| !t.r#type.is_empty() && we_tab_shows_type(&t.r#type))
            .map(|t| (t.r#type.clone(), plugin_text(t.label_text.as_ref(), &t.label)))
            .collect();
        assign(&mut data.type_labels, type_labels, "typeLabels", &mut out);
    }
    Ok(finish(st, out))
}

// ── Schema parity check (the hidden «Developer» page) ───────────────────

/// Parity check properties: the report on the last
/// `renderer_plugin_list` response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParitySchemaData {
    /// Report of the last check; `None` — no check has run yet.
    pub report: Option<crate::we_parity::ParityReport>,
}

/// Parse the `renderer_plugin_list` response and IMMEDIATELY reconcile the schema
/// with Rexon's UI registry.
///
/// A separate entry, not a parse on top of [`apply_renderer_plugin_list`]: for
/// the «Developer» page what matters is not the renderer list but the check
/// verdict, and assembling it from data already laid out into QML maps would
/// mean a second parser of the same response (rule #12).
pub fn apply_parity_schema(
    st: &mut QueryState,
    data: &mut ParitySchemaData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::RendererPluginList(pl)) = rsp.payload {
        let report = crate::we_parity::compare_schema(&pl);
        if data.report.as_ref() != Some(&report) {
            data.report = Some(report);
            out.push(QuerySignal::Property("report"));
        }
    }
    Ok(finish(st, out))
}

// ── TagListQuery / ContentRatingListQuery (the filter dialog) ───────────

/// A string-list property (`tags` of one request, `ratings` of the other).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringListData {
    pub items: Vec<String>,
}

/// Parse the `tag_list` response (the `tags` property).
pub fn apply_tag_list(
    st: &mut QueryState,
    data: &mut StringListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::TagList(tl)) = rsp.payload {
        assign(&mut data.items, tl.tags, "tags", &mut out);
    }
    Ok(finish(st, out))
}

/// Parse the `content_rating_list` response (the `ratings` property).
pub fn apply_content_rating_list(
    st: &mut QueryState,
    data: &mut StringListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::ContentRatingList(cl)) = rsp.payload {
        assign(&mut data.items, cl.ratings, "ratings", &mut out);
    }
    Ok(finish(st, out))
}

// ── GPUs (`App.gpuManager`: «State», «Monitors», settings) ──────────────

/// The GPU list from the `gpu_list` response.
///
/// `None` — the frame is not about GPUs: it did not parse, it is an event, the
/// daemon refused or answered with another payload (the connection-drop frame
/// the transport uses to close pending requests looks like that too). The
/// previous list must not be touched then — an empty `Some` means "no cards", not
/// "no response".
pub fn gpu_list_from_frame(frame_bytes: &[u8]) -> Option<Vec<pb::GpuInfo>> {
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let Some(pb::server_frame::Kind::Response(rsp)) = frame.kind else {
        return None;
    };
    if rsp.status != pb::Status::Ok as i32 {
        return None;
    }
    match rsp.payload {
        Some(pb::response::Payload::GpuList(list)) => Some(list.gpus),
        _ => None,
    }
}

// ── Current wallpaper showings (`App.presentationManager`, rail card) ───

/// Wallpapers from the `wallpaper_lookup` response.
///
/// `None` — the frame is not about them (did not parse, an event, a daemon refusal,
/// another payload, a connection-drop frame): already known wallpapers stay as
/// they are.
pub fn wallpaper_lookup_from_frame(frame_bytes: &[u8]) -> Option<Vec<pb::WallpaperEntry>> {
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let Some(pb::server_frame::Kind::Response(rsp)) = frame.kind else {
        return None;
    };
    if rsp.status != pb::Status::Ok as i32 {
        return None;
    }
    match rsp.payload {
        Some(pb::response::Payload::WallpaperLookup(found)) => Some(found.wallpapers),
        _ => None,
    }
}

/// Wallpaper ids of a showings snapshot — each once, in snapshot order:
/// they are asked from the daemon in one request.
pub fn presentation_wallpaper_ids(presentations: &[pb::WallpaperPresentationInfo]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for presentation in presentations {
        if !ids.contains(&presentation.wallpaper_id) {
            ids.push(presentation.wallpaper_id.clone());
        }
    }
    ids
}

/// Label of the showing targets: display and canvas names joined with « · »,
/// duplicates removed, order as in the snapshot.
pub fn presentation_target_summary(labels: &[String]) -> String {
    let mut unique: Vec<&str> = Vec::new();
    for label in labels {
        if !unique.contains(&label.as_str()) {
            unique.push(label);
        }
    }
    unique.join(" · ")
}

/// Which card a DRM node `major:minor` belongs to.
///
/// The number fits both the render node (`renderD128`) and the primary one (`card1`):
/// the renderer carries the first, the display carries its consumer's node, and both
/// look up the card with one call. `major == 0` — the node is unknown, no card.
pub fn find_gpu(gpus: &[pb::GpuInfo], major: u32, minor: u32) -> Option<usize> {
    if major == 0 {
        return None;
    }
    gpus.iter().position(|g| {
        (g.render_major == major && g.render_minor == minor)
            || (g.primary_major == major && g.primary_minor == minor)
    })
}

#[cfg(test)]
mod we_tab_scope_tests {
    use super::*;

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

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn remote_owners_are_a_whitelist() {
        assert!(we_tab_shows_remote_owner("org.waywallen.open-wallpaper-engine"));
        assert!(!we_tab_shows_remote_owner("org.waywallen.wallhaven"));
        assert!(!we_tab_shows_remote_owner("org.waywallen.sketchfab"));
        // A catalog upstream has not invented yet, and a catalog without an owner.
        assert!(!we_tab_shows_remote_owner("org.waywallen.some-future-catalog"));
        assert!(!we_tab_shows_remote_owner(""));
    }

    #[test]
    fn hidden_types_and_plugins() {
        assert!(!we_tab_shows_type("gltf"));
        for t in ["scene", "video", "web", "image", "application"] {
            assert!(we_tab_shows_type(t), "{t}");
        }
        assert_eq!(we_tab_types(strings(&["scene", "gltf", "video"])), ["scene", "video"]);
        assert!(!we_tab_keeps_plugin(&strings(&["gltf"])));
        assert!(we_tab_keeps_plugin(&strings(&["gltf", "image"])));
        assert!(we_tab_keeps_plugin(&[]));
    }

    #[test]
    fn skip_types_add_hidden_once() {
        assert_eq!(we_tab_skip_types(&[]), ["gltf"]);
        assert_eq!(we_tab_skip_types(&strings(&["web"])), ["web", "gltf"]);
        assert_eq!(we_tab_skip_types(&strings(&["gltf", "web"])), ["gltf", "web"]);
    }

    #[test]
    fn renderer_plugin_list_drops_gltf() {
        let renderer = |name: &str, types: &[&str]| pb::RendererPluginInfo {
            name: name.into(),
            types: strings(types),
            ..Default::default()
        };
        let label = |t: &str| pb::WallpaperTypeInfo {
            r#type: t.into(),
            label: t.to_uppercase(),
            ..Default::default()
        };
        let payload = pb::response::Payload::RendererPluginList(pb::RendererPluginListResponse {
            renderers: vec![
                renderer("owe", &["scene", "web"]),
                renderer("gltf", &["gltf"]),
                renderer("mixed", &["image", "gltf"]),
            ],
            supported_types: strings(&["scene", "web", "gltf", "image"]),
            type_labels: vec![label("scene"), label("gltf")],
        });
        let mut st = QueryState::default();
        let mut data = RendererPluginListData::default();
        apply_renderer_plugin_list(&mut st, &mut data, &frame(payload)).expect("кадр разобран");
        let names: Vec<&str> = data.renderers.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["owe", "mixed"]);
        assert_eq!(data.renderers[1].types, ["image"]);
        assert_eq!(data.supported_types, ["scene", "web", "image"]);
        assert_eq!(data.type_labels.keys().collect::<Vec<_>>(), ["scene"]);
    }
}
