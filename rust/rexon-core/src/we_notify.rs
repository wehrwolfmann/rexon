//! The `Notify` QML singleton of the Wallpaper Engine tab — port of waywallen's
//! `ui/src/notify.cpp` (MIT, (c) hypengw; attribution in NOTICE).
//!
//! The singleton relays daemon events to QML: it keeps a state snapshot
//! (whether a scan is running, how many tasks, the daemon phase, the output
//! backend) and turns `GlobalEvent`s from the wire into Qt signals. ALL the
//! deciding part is here: parsing the frame (prost, the daemon schema is
//! already in [`crate::proto`]), the state transition and the list of signals
//! to emit. The Python shim stays a QObject shell: it got bytes — handed them
//! here — broadcast the resulting signals.
//!
//! This is the first module where the daemon protobuf is parsed in Rust rather
//! than with Python protobuf: `Notify` does not read response fields
//! (`pb.Response`), only events, so the boundary could be moved for it
//! separately from the other bridge classes.
//!
//! ⚠️ Fixes during the port (rule #16):
//!   1. `displayConnectionFailed` — the signal WAS declared in our Python but
//!      was NEVER emitted: the handler had no branch for the event. The daemon
//!      uses this event to report that a foreign client could not connect
//!      (protocol/version mismatch) — the message was silently lost.
//!   2. `statusChanged` was emitted on EVERY `StatusSync`, even when no field
//!      changed (the daemon sends a snapshot on any change of its state,
//!      including unrelated ones). The original compares the snapshot with the
//!      current one and stays silent if there is no difference; same here —
//!      needless re-evaluation of QML bindings is gone.
//!   3. The reset on a WebSocket drop also emitted the signal unconditionally;
//!      now — only if the state really changed (as in the original).
//!
//! v0.2.6: the `QrLoginProgress` event was added (QR-code login in the source
//! plugin dialog) — the vendored QML subscribes to it.
//!
//! Deliberately NOT ported: `taskProgress` — the daemon and fresh upstream
//! have the event, but the vendored QML v0.2.5 does not use it (checked by
//! grep over `ui/qml`), and there is no reason to drag an unused signal into
//! the bridge. When the vendor update brings it — the branch is added here.

use crate::proto::control as pb;
use prost::Message;
use serde::{ser::SerializeMap, Serialize, Serializer};

/// `DAEMON_PHASE_READY` of the daemon protocol — taken FROM THE SCHEMA, not as
/// a literal: the enum has `Unspecified = 0`, so Ready = 2, and a hand-written
/// one would quietly leave the UI in the Starting phase (caught by the
/// roundtrip test on the very first run).
const DAEMON_PHASE_READY: i32 = pb::DaemonPhase::Ready as i32;

/// Daemon phase in terms of the QML enum `Notify.DaemonPhase`.
pub const PHASE_STARTING: i32 = 0;
/// The daemon is ready.
pub const PHASE_READY: i32 = 1;

/// A `DisplayBackendStatus` snapshot in the form QML reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DisplayBackend {
    pub name: String,
    pub state: String,
    pub desktop: String,
    pub binary: String,
    pub reason: String,
    /// ⚠️ Goes out as `flatpakId` — that is how both QML and the shim read it.
    #[serde(rename = "flatpakId")]
    pub flatpak_id: String,
}

/// Singleton state (the properties `scanInProgress`/`activeTaskCount`/
/// `daemonPhase`/`displayBackend`/`globalPaused`/`globalMuted`/
/// `globalStopped`).
///
/// ⚠️ The three global switches arrived in `StatusSync` together with protocol
/// v0.3.5, and until now NOBODY read them: batch 7 brought the commands
/// `GlobalPauseSetQuery`/`GlobalMuteSetQuery`/`GlobalStopSetQuery`, but the
/// state was left out. The vendor `StatusPage.qml` reads exactly
/// `W.Notify.globalPaused`/`globalMuted`/`globalStopped` — on non-existent
/// properties the three labels «Pause all»/«Mute all»/«Stop all» never showed
/// whether the mode was on («Unable to assign [undefined] to bool»). Rule
/// #12: a command and its state are one entity, added together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotifyState {
    pub scan_in_progress: bool,
    pub active_task_count: u32,
    pub daemon_phase: i32,
    pub display_backend: DisplayBackend,
    pub global_paused: bool,
    pub global_muted: bool,
    pub global_stopped: bool,
}

impl Default for NotifyState {
    fn default() -> Self {
        NotifyState {
            scan_in_progress: false,
            active_task_count: 0,
            daemon_phase: PHASE_STARTING,
            display_backend: DisplayBackend::default(),
            global_paused: false,
            global_muted: false,
            global_stopped: false,
        }
    }
}

/// A Qt signal the shim must emit (in return order).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "signal", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NotifySignal {
    /// `statusChanged()` — the state snapshot changed.
    StatusChanged,
    /// `daemonReady()` — the phase went Starting → Ready.
    DaemonReady,
    /// `wallpaperSyncFinished(count, error)`.
    WallpaperSyncFinished { count: u32, error: String },
    /// `wallpaperHiddenChanged(wallpaperIds, hidden)` — waywallen 0.4.4: the
    /// daemon hid wallpapers or brought them back (by any client).
    WallpaperHiddenChanged { wallpaper_ids: Vec<String>, hidden: bool },
    /// `librariesAdded(paths)`.
    LibrariesAdded { paths: Vec<String> },
    /// `settingsChanged()`.
    SettingsChanged,
    /// `displayConnectionFailed(name, protocolVersion, errorCode, reason)`.
    DisplayConnectionFailed {
        client_name: String,
        client_protocol_version: u32,
        error_code: u32,
        reason: String,
    },
    /// `remoteDownloadProgress(sourceId, id, state, error)`.
    RemoteDownloadProgress {
        source_id: String,
        id: String,
        state: i32,
        error: String,
        /// waywallen 0.6.0: transfer progress — `Some((received, total))` if
        /// the daemon sent it; «total» `0` — size unknown. The page signal
        /// does not carry it: it goes to the shared acquisition store
        /// (`setRemoteAcquisitionState` in the original).
        transfer: Option<(u64, u64)>,
    },
    /// `playlistChanged()`.
    PlaylistChanged,
    /// `pluginChanged()`.
    PluginChanged,
    /// `pluginUpdateChanged()`.
    PluginUpdateChanged,
    /// `pluginRestartFailed(pluginId, error)`.
    PluginRestartFailed { plugin_id: String, error: String },
    /// `pluginStateChanged()` — the state of a source plugin changed (v0.2.6;
    /// Discover re-reads the list of sources).
    PluginStateChanged,
    /// `qrLoginProgress(...)` — QR-code login (v0.2.6, QrLoginDialog).
    ///
    /// ⚠ waywallen 0.3.8: the dialog's four texts are translatable — a
    /// `PluginMessage` travels next to the former strings, and the vendor QML
    /// passes them through `I18n.tr`. So here it is [`PluginText`], not
    /// `String`: the string stayed a fallback inside it.
    QrLoginProgress {
        session_id: String,
        plugin_id: String,
        action_id: String,
        state: i32,
        qr_image: String,
        display_value: PluginText,
        error: PluginText,
        title: PluginText,
        instruction: PluginText,
    },
    /// `browserLoginProgress(...)` — sign-in through a browser window
    /// (waywallen 0.6.1, BrowserLoginDialog; e.g. the Pixiv source plugin).
    /// Same shape as [`Self::QrLoginProgress`] minus the QR image.
    BrowserLoginProgress {
        session_id: String,
        plugin_id: String,
        action_id: String,
        state: i32,
        display_value: PluginText,
        error: PluginText,
        title: PluginText,
        instruction: PluginText,
    },
}

/// A translatable plugin string: either a label from its catalog, or ready
/// text the daemon sent itself.
///
/// Empty `plugin_id`/`msgid` mean «no catalog» — then the bare `fallback`
/// string goes out, as the vendor's `pluginMessageFromPb` returns it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginText {
    pub plugin_id: String,
    pub msgid: String,
    pub arguments: Vec<String>,
    pub fallback: String,
}

impl PluginText {
    /// Whether the label is in the plugin's catalog (otherwise `fallback` goes out).
    pub fn has_catalog_key(&self) -> bool {
        !self.plugin_id.is_empty() && !self.msgid.is_empty()
    }
}

/// The outward form is ONE for all consumers: the dictionary
/// `{pluginId, msgid, arguments, rexonFallback}` when the label has a catalog
/// key, and a bare string when it does not (the vendor's `pluginMessageFromPb`
/// returns it the same way and the vendor QML reads it the same way).
///
/// ⚠️ `Serialize` is written BY HAND here on purpose: the type is mixed (a
/// string or a dictionary), and `#[serde(untagged)]` exists only for enums. In
/// return the form is declared ONCE and next to the type itself — there is no
/// longer any need to rewrite it in the bridge for every field where a
/// `PluginText` occurs.
impl Serialize for PluginText {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        if !self.has_catalog_key() {
            return ser.serialize_str(&self.fallback);
        }
        let mut m = ser.serialize_map(Some(4))?;
        m.serialize_entry("pluginId", &self.plugin_id)?;
        m.serialize_entry("msgid", &self.msgid)?;
        m.serialize_entry("arguments", &self.arguments)?;
        m.serialize_entry("rexonFallback", &self.fallback)?;
        m.end()
    }
}

/// The protocol's `PluginMessage` + the former string field → [`PluginText`].
///
/// Public on purpose: it is a port of the vendor's `pluginMessageFromPb`, and
/// not only notifications need it — plugin settings schemas
/// (`we_query::SettingSchema`) assemble their labels by THE SAME rule. A second
/// copy of the rule would silently diverge from this one (rule #12).
pub fn plugin_text(message: Option<&pb::PluginMessage>, fallback: &str) -> PluginText {
    let (plugin_id, msgid, arguments) = match message {
        Some(m) => (m.plugin_id.clone(), m.msgid.clone(), m.arguments.clone()),
        None => (String::new(), String::new(), Vec::new()),
    };
    PluginText {
        plugin_id,
        msgid,
        arguments,
        fallback: fallback.to_string(),
    }
}

/// Parsing a server frame: an event or nothing.
///
/// The input is RAW `ServerFrame` bytes from the WebSocket — exactly what came
/// from the daemon. A frame with a response (`Response`) gives no events:
/// responses are still parsed by Python (request callbacks read them).
pub fn parse_event(frame_bytes: &[u8]) -> Result<Option<pb::Event>, prost::DecodeError> {
    let frame = pb::ServerFrame::decode(frame_bytes)?;
    Ok(match frame.kind {
        Some(pb::server_frame::Kind::Event(e)) => Some(e),
        _ => None,
    })
}

/// Apply a frame to the state and get the list of signals.
///
/// A broken frame is not the caller's error: the daemon may be newer than us,
/// and crashing in the middle of receiving is not allowed. `Err` is returned,
/// the shim writes debug output and carries on (the Python reference behaves
/// the same way).
pub fn apply_frame(
    state: &mut NotifyState,
    frame_bytes: &[u8],
) -> Result<Vec<NotifySignal>, prost::DecodeError> {
    let Some(event) = parse_event(frame_bytes)? else {
        return Ok(Vec::new());
    };
    Ok(apply_event(state, &event))
}

/// The same state transition from an already parsed event.
pub fn apply_event(state: &mut NotifyState, event: &pb::Event) -> Vec<NotifySignal> {
    use pb::event::Payload;
    let Some(payload) = event.payload.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match payload {
        Payload::WallpaperSyncFinished(done) => {
            out.push(NotifySignal::WallpaperSyncFinished {
                count: done.count,
                error: done.error.clone(),
            });
        }
        Payload::WallpaperHiddenChanged(changed) => {
            out.push(NotifySignal::WallpaperHiddenChanged {
                wallpaper_ids: changed.wallpaper_ids.clone(),
                hidden: changed.hidden,
            });
        }
        Payload::LibrariesAdded(added) => {
            out.push(NotifySignal::LibrariesAdded {
                paths: added.paths.clone(),
            });
        }
        Payload::StatusSync(sync) => {
            let backend = sync
                .display_backend
                .as_ref()
                .map(|b| DisplayBackend {
                    name: b.name.clone(),
                    state: b.state.clone(),
                    desktop: b.desktop.clone(),
                    binary: b.binary.clone(),
                    reason: b.reason.clone(),
                    flatpak_id: b.flatpak_id.clone(),
                })
                .unwrap_or_default();
            let phase = if sync.phase == DAEMON_PHASE_READY {
                PHASE_READY
            } else {
                PHASE_STARTING
            };
            let ready_edge = phase == PHASE_READY && state.daemon_phase != PHASE_READY;
            let changed = sync.scan_in_progress != state.scan_in_progress
                || sync.active_task_count != state.active_task_count
                || phase != state.daemon_phase
                || backend != state.display_backend
                || sync.global_paused != state.global_paused
                || sync.global_muted != state.global_muted
                || sync.global_stopped != state.global_stopped;
            if changed {
                state.scan_in_progress = sync.scan_in_progress;
                state.active_task_count = sync.active_task_count;
                state.daemon_phase = phase;
                state.display_backend = backend;
                state.global_paused = sync.global_paused;
                state.global_muted = sync.global_muted;
                state.global_stopped = sync.global_stopped;
                out.push(NotifySignal::StatusChanged);
            }
            if ready_edge {
                out.push(NotifySignal::DaemonReady);
            }
        }
        Payload::SettingsChanged(_) => out.push(NotifySignal::SettingsChanged),
        Payload::DisplayConnectionFailed(f) => {
            out.push(NotifySignal::DisplayConnectionFailed {
                client_name: f.client_name.clone(),
                client_protocol_version: f.client_protocol_version,
                error_code: f.error_code,
                reason: f.reason.clone(),
            });
        }
        Payload::RemoteDownloadProgress(p) => {
            out.push(NotifySignal::RemoteDownloadProgress {
                source_id: p.source_id.clone(),
                id: p.id.clone(),
                state: p.state,
                error: p.error.clone(),
                transfer: p
                    .transfer
                    .as_ref()
                    .map(|t| (t.received_bytes, t.total_bytes.unwrap_or(0))),
            });
        }
        Payload::PlaylistChanged(_) => out.push(NotifySignal::PlaylistChanged),
        Payload::PluginChanged(_) => out.push(NotifySignal::PluginChanged),
        Payload::PluginUpdateChanged(_) => out.push(NotifySignal::PluginUpdateChanged),
        Payload::PluginRestartFailed(f) => {
            out.push(NotifySignal::PluginRestartFailed {
                plugin_id: f.plugin_id.clone(),
                error: f.error.clone(),
            });
        }
        Payload::PluginStateChanged(_) => out.push(NotifySignal::PluginStateChanged),
        Payload::QrLoginProgress(p) => {
            out.push(NotifySignal::QrLoginProgress {
                session_id: p.session_id.clone(),
                plugin_id: p.plugin_id.clone(),
                action_id: p.action_id.clone(),
                state: p.state,
                qr_image: p.qr_image.clone(),
                display_value: plugin_text(
                    p.display_value_text.as_ref(),
                    &p.display_value,
                ),
                error: plugin_text(p.error_text.as_ref(), &p.error),
                title: plugin_text(p.title_text.as_ref(), &p.title),
                instruction: plugin_text(p.instruction_text.as_ref(), &p.instruction),
            });
        }
        Payload::BrowserLoginProgress(p) => {
            out.push(NotifySignal::BrowserLoginProgress {
                session_id: p.session_id.clone(),
                plugin_id: p.plugin_id.clone(),
                action_id: p.action_id.clone(),
                state: p.state,
                display_value: plugin_text(
                    p.display_value_text.as_ref(),
                    &p.display_value,
                ),
                error: plugin_text(p.error_text.as_ref(), &p.error),
                title: plugin_text(p.title_text.as_ref(), &p.title),
                instruction: plugin_text(p.instruction_text.as_ref(), &p.instruction),
            });
        }
        // Snapshots/changes of displays, renderers and libraries do not
        // concern Notify — they have their own models (RendererManager/
        // DisplayManager/LibraryManager), which read the events themselves.
        _ => {}
    }
    out
}

/// WebSocket drop: go back to the pessimistic state so the UI does not keep
/// a stale «daemon ready» while there is no daemon.
pub fn on_disconnected(state: &mut NotifyState) -> Vec<NotifySignal> {
    let fresh = NotifyState::default();
    if *state == fresh {
        return Vec::new();
    }
    *state = fresh;
    vec![NotifySignal::StatusChanged]
}
