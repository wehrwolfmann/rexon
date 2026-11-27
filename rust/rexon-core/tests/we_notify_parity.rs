//! Check of the `we_notify.rs` port against the original waywallen
//! `ui/src/notify.cpp`.
//!
//! Frames are built with the same prost (the daemon schema is shared), fed to
//! `apply_frame`, and the state transition + signal list are compared.
//! Separate tests pin the fixes: the missed `displayConnectionFailed` event
//! and `statusChanged` without changes.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_notify as n;

/// A translatable string without a catalog key — what the daemon sent before 0.3.8.
fn plain(text: &str) -> n::PluginText {
    n::PluginText { fallback: text.to_string(), ..Default::default() }
}

fn frame(payload: pb::event::Payload) -> Vec<u8> {
    let f = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Event(pb::Event {
            payload: Some(payload),
        })),
    };
    f.encode_to_vec()
}

/// `phase` here is a value FROM THE SCHEMA (Unspecified=0, Starting=1,
/// Ready=2), not the QML enum Notify.DaemonPhase (Starting=0, Ready=1).
fn status(
    scan: bool,
    tasks: u32,
    phase: i32,
    backend: Option<pb::DisplayBackendStatus>,
) -> Vec<u8> {
    frame(pb::event::Payload::StatusSync(pb::StatusSync {
        scan_in_progress: scan,
        active_task_count: tasks,
        phase,
        display_backend: backend,
        ..Default::default()
    }))
}

#[test]
fn status_sync_updates_state_and_signals_ready_edge() {
    let mut st = n::NotifyState::default();
    // start: the daemon is not ready yet, but a task appeared → only statusChanged
    let sig = n::apply_frame(&mut st, &status(true, 2, pb::DaemonPhase::Starting as i32, None)).unwrap();
    assert_eq!(sig, vec![n::NotifySignal::StatusChanged]);
    assert!(st.scan_in_progress && st.active_task_count == 2);
    assert_eq!(st.daemon_phase, n::PHASE_STARTING);

    // transition to Ready → statusChanged + daemonReady (exactly in this order)
    let sig = n::apply_frame(&mut st, &status(false, 0, pb::DaemonPhase::Ready as i32, None)).unwrap();
    assert_eq!(
        sig,
        vec![
            n::NotifySignal::StatusChanged,
            n::NotifySignal::DaemonReady
        ]
    );
    assert_eq!(st.daemon_phase, n::PHASE_READY);

    // repeated Ready with the same snapshot → NOT A SINGLE signal (fix #2/#3)
    let sig = n::apply_frame(&mut st, &status(false, 0, pb::DaemonPhase::Ready as i32, None)).unwrap();
    assert!(sig.is_empty(), "{sig:?}");
}

#[test]
fn display_backend_snapshot_reaches_state() {
    let mut st = n::NotifyState::default();
    let backend = pb::DisplayBackendStatus {
        name: "kde".into(),
        state: "running".into(),
        desktop: "KDE".into(),
        binary: "/usr/bin/waywallen".into(),
        reason: String::new(),
        flatpak_id: String::new(),
    };
    let sig = n::apply_frame(&mut st, &status(false, 0, pb::DaemonPhase::Ready as i32, Some(backend))).unwrap();
    assert_eq!(sig.len(), 2);
    assert_eq!(st.display_backend.name, "kde");
    assert_eq!(st.display_backend.binary, "/usr/bin/waywallen");
    // the same backend again — silence
    let same = pb::DisplayBackendStatus {
        name: "kde".into(),
        state: "running".into(),
        desktop: "KDE".into(),
        binary: "/usr/bin/waywallen".into(),
        reason: String::new(),
        flatpak_id: String::new(),
    };
    assert!(n::apply_frame(&mut st, &status(false, 0, pb::DaemonPhase::Ready as i32, Some(same)))
        .unwrap()
        .is_empty());
    // one field changed — statusChanged again, without daemonReady
    let moved = pb::DisplayBackendStatus {
        name: "kde".into(),
        state: "stopped".into(),
        desktop: "KDE".into(),
        binary: "/usr/bin/waywallen".into(),
        reason: "нет вывода".into(),
        flatpak_id: String::new(),
    };
    assert_eq!(
        n::apply_frame(&mut st, &status(false, 0, pb::DaemonPhase::Ready as i32, Some(moved))).unwrap(),
        vec![n::NotifySignal::StatusChanged]
    );
}

#[test]
fn transient_events_map_one_to_one() {
    let mut st = n::NotifyState::default();
    let cases: Vec<(Vec<u8>, n::NotifySignal)> = vec![
        (
            frame(pb::event::Payload::WallpaperSyncFinished(
                pb::WallpaperSyncFinished { count: 42, error: "ошибка".into() },
            )),
            n::NotifySignal::WallpaperSyncFinished { count: 42, error: "ошибка".into() },
        ),
        (
            frame(pb::event::Payload::LibrariesAdded(pb::LibrariesAdded {
                paths: vec!["/дом/обои".into(), "/srv/wp".into()],
            })),
            n::NotifySignal::LibrariesAdded {
                paths: vec!["/дом/обои".into(), "/srv/wp".into()],
            },
        ),
        (
            frame(pb::event::Payload::SettingsChanged(pb::SettingsChanged::default())),
            n::NotifySignal::SettingsChanged,
        ),
        (
            frame(pb::event::Payload::RemoteDownloadProgress(
                pb::RemoteDownloadProgress {
                    source_id: "steam".into(),
                    id: "123".into(),
                    state: 2,
                    error: String::new(),
                    transfer: None,
                },
            )),
            n::NotifySignal::RemoteDownloadProgress {
                source_id: "steam".into(),
                id: "123".into(),
                state: 2,
                error: String::new(),
                transfer: None,
            },
        ),
        // waywallen 0.6.0: transfer progress reaches the store; without a full
        // size, «total» is 0.
        (
            frame(pb::event::Payload::RemoteDownloadProgress(
                pb::RemoteDownloadProgress {
                    source_id: "steam".into(),
                    id: "124".into(),
                    state: 2,
                    error: String::new(),
                    transfer: Some(pb::RemoteDownloadTransfer {
                        received_bytes: 4096,
                        total_bytes: None,
                    }),
                },
            )),
            n::NotifySignal::RemoteDownloadProgress {
                source_id: "steam".into(),
                id: "124".into(),
                state: 2,
                error: String::new(),
                transfer: Some((4096, 0)),
            },
        ),
        (
            frame(pb::event::Payload::PlaylistChanged(pb::PlaylistChanged::default())),
            n::NotifySignal::PlaylistChanged,
        ),
        (
            frame(pb::event::Payload::PluginChanged(pb::Empty {})),
            n::NotifySignal::PluginChanged,
        ),
        (
            frame(pb::event::Payload::PluginStateChanged(pb::Empty {})),
            n::NotifySignal::PluginStateChanged,
        ),
        (
            frame(pb::event::Payload::QrLoginProgress(pb::QrLoginProgress {
                session_id: "s1".into(),
                plugin_id: "steam".into(),
                action_id: "login".into(),
                state: 1,
                qr_image: "data".into(),
                display_value: "код".into(),
                error: String::new(),
                title: "Вход".into(),
                instruction: "Отсканируйте".into(),
                // waywallen 0.3.8: each string got its translatable pair next to it;
                // the daemon did not send it here.
                ..Default::default()
            })),
            n::NotifySignal::QrLoginProgress {
                session_id: "s1".into(),
                plugin_id: "steam".into(),
                action_id: "login".into(),
                state: 1,
                qr_image: "data".into(),
                // waywallen 0.3.8: the four dialog texts are translatable. The daemon
                // sent only strings — they have no catalog key, and the fallback text
                // goes out, as before.
                display_value: plain("код"),
                error: plain(""),
                title: plain("Вход"),
                instruction: plain("Отсканируйте"),
            },
        ),
        (
            // waywallen 0.6.1: browser sign-in (Pixiv). Same texts as the QR
            // flow, no image.
            frame(pb::event::Payload::BrowserLoginProgress(pb::BrowserLoginProgress {
                session_id: "b1".into(),
                plugin_id: "org.waywallen.pixiv".into(),
                action_id: "login".into(),
                state: 2,
                title: "Sign in".into(),
                instruction: "Finish in the browser window".into(),
                ..Default::default()
            })),
            n::NotifySignal::BrowserLoginProgress {
                session_id: "b1".into(),
                plugin_id: "org.waywallen.pixiv".into(),
                action_id: "login".into(),
                state: 2,
                display_value: plain(""),
                error: plain(""),
                title: plain("Sign in"),
                instruction: plain("Finish in the browser window"),
            },
        ),
        (
            frame(pb::event::Payload::PluginUpdateChanged(pb::Empty {})),
            n::NotifySignal::PluginUpdateChanged,
        ),
        (
            frame(pb::event::Payload::PluginRestartFailed(
                pb::PluginRestartFailed {
                    plugin_id: "scene".into(),
                    error: "не поднялся".into(),
                },
            )),
            n::NotifySignal::PluginRestartFailed {
                plugin_id: "scene".into(),
                error: "не поднялся".into(),
            },
        ),
    ];
    for (bytes, want) in cases {
        let got = n::apply_frame(&mut st, &bytes).unwrap();
        assert_eq!(got, vec![want.clone()], "событие {want:?}");
    }
    // these events do not touch the snapshot state
    assert_eq!(st, n::NotifyState::default());
}

/// ⚠️ Fix #1: the event existed in the protocol and in our Python signal, but
/// there was no handling branch — the message that a foreign client failed to
/// connect to the display protocol was silently lost.
#[test]
fn fix_display_connection_failed_is_delivered() {
    let mut st = n::NotifyState::default();
    let bytes = frame(pb::event::Payload::DisplayConnectionFailed(
        pb::DisplayConnectionFailed {
            client_name: "чужой".into(),
            client_protocol_version: 3,
            error_code: 2,
            reason: "VERSION_UNSUPPORTED".into(),
        },
    ));
    assert_eq!(
        n::apply_frame(&mut st, &bytes).unwrap(),
        vec![n::NotifySignal::DisplayConnectionFailed {
            client_name: "чужой".into(),
            client_protocol_version: 3,
            error_code: 2,
            reason: "VERSION_UNSUPPORTED".into(),
        }]
    );
}

#[test]
fn disconnect_resets_only_when_dirty() {
    let mut st = n::NotifyState::default();
    // clean state — no signal (fix #3)
    assert!(n::on_disconnected(&mut st).is_empty());

    n::apply_frame(&mut st, &status(true, 3, pb::DaemonPhase::Ready as i32, None)).unwrap();
    assert_eq!(
        n::on_disconnected(&mut st),
        vec![n::NotifySignal::StatusChanged]
    );
    assert_eq!(st, n::NotifyState::default());
}

#[test]
fn response_frames_and_unknown_payloads_are_silent() {
    let mut st = n::NotifyState::default();
    // a frame with a RESPONSE (Python reads those) — yields no events
    let resp = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 7,
            ..Default::default()
        })),
    }
    .encode_to_vec();
    assert!(n::apply_frame(&mut st, &resp).unwrap().is_empty());

    // an event without payload and an event of a foreign model — silence too
    let empty = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Event(pb::Event { payload: None })),
    }
    .encode_to_vec();
    assert!(n::apply_frame(&mut st, &empty).unwrap().is_empty());
    let displays = frame(pb::event::Payload::DisplaySnapshot(pb::DisplaySnapshot {
        displays: Vec::new(),
    }));
    assert!(n::apply_frame(&mut st, &displays).unwrap().is_empty());
    assert_eq!(st, n::NotifyState::default());
}

#[test]
fn broken_frame_is_an_error_not_a_panic() {
    let mut st = n::NotifyState::default();
    let err = n::apply_frame(&mut st, &[0xff, 0xff, 0xff, 0xff]);
    assert!(err.is_err());
    assert_eq!(st, n::NotifyState::default());
}
