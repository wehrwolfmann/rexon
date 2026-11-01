//! Сверка порта `we_notify.rs` с оригиналом `ui/src/notify.cpp` waywallen.
//!
//! Кадры собираются тем же prost'ом (схема демона общая), скармливаются
//! `apply_frame` и сверяются переход состояния + список сигналов.
//! Отдельными тестами закреплены починки: пропущенное событие
//! `displayConnectionFailed` и `statusChanged` без изменений.

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_notify as n;

/// Переводимая строка без ключа каталога — то, что демон присылал до 0.3.8.
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

/// `phase` тут — значение ИЗ СХЕМЫ (Unspecified=0, Starting=1, Ready=2),
/// а не QML-перечисление Notify.DaemonPhase (Starting=0, Ready=1).
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
    // старт: демон ещё не готов, но появилась задача → только statusChanged
    let sig = n::apply_frame(&mut st, &status(true, 2, pb::DaemonPhase::Starting as i32, None)).unwrap();
    assert_eq!(sig, vec![n::NotifySignal::StatusChanged]);
    assert!(st.scan_in_progress && st.active_task_count == 2);
    assert_eq!(st.daemon_phase, n::PHASE_STARTING);

    // переход в Ready → statusChanged + daemonReady (именно в этом порядке)
    let sig = n::apply_frame(&mut st, &status(false, 0, pb::DaemonPhase::Ready as i32, None)).unwrap();
    assert_eq!(
        sig,
        vec![
            n::NotifySignal::StatusChanged,
            n::NotifySignal::DaemonReady
        ]
    );
    assert_eq!(st.daemon_phase, n::PHASE_READY);

    // повторный Ready тем же снимком → НИ ОДНОГО сигнала (починка №2/№3)
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
    // тот же бэкенд повторно — тишина
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
    // сменилось одно поле — снова statusChanged, без daemonReady
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
                },
            )),
            n::NotifySignal::RemoteDownloadProgress {
                source_id: "steam".into(),
                id: "123".into(),
                state: 2,
                error: String::new(),
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
                // waywallen 0.3.8: рядом с каждой строкой появилась её
                // переводимая пара; демон её здесь не прислал.
                ..Default::default()
            })),
            n::NotifySignal::QrLoginProgress {
                session_id: "s1".into(),
                plugin_id: "steam".into(),
                action_id: "login".into(),
                state: 1,
                qr_image: "data".into(),
                // waywallen 0.3.8: четыре текста диалога переводимы. Демон
                // прислал только строки — ключа каталога у них нет, и
                // наружу уходит запасной текст, как раньше.
                display_value: plain("код"),
                error: plain(""),
                title: plain("Вход"),
                instruction: plain("Отсканируйте"),
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
    // состояние снимка эти события не трогают
    assert_eq!(st, n::NotifyState::default());
}

/// ⚠️ Починка №1: событие было в протоколе и в сигнале нашего Python, но
/// ветки обработки не существовало — сообщение о том, что чужой клиент не
/// смог подключиться к дисплей-протоколу, молча терялось.
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
    // чистое состояние — сигнала нет (починка №3)
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
    // кадр с ОТВЕТОМ (их читает Python) — событий не даёт
    let resp = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 7,
            ..Default::default()
        })),
    }
    .encode_to_vec();
    assert!(n::apply_frame(&mut st, &resp).unwrap().is_empty());

    // событие без payload и событие чужой модели — тоже тишина
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
