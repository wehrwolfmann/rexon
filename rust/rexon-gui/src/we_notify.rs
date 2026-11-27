// Notify — the QML singleton of daemon events of the Wallpaper Engine tab.
//
// A port of waywallen's `ui/src/notify.cpp` (MIT, (c) hypengw; attribution
// in NOTICE).
//
// There is no deciding part here: parsing the `ServerFrame` frame, the
// state transition and the LIST of signals to send are computed by the core
// (`rexon_core::we_notify`, covered by `we_notify_parity.rs`). The bridge
// only has to hand out the resulting list.
//
// Two upstream fixes live in the core and come with it:
// `displayConnectionFailed` is now really emitted (there was no event branch
// at all), and `statusChanged` stays silent if the snapshot did not change.
//
// Warning: in the original `displayBackend` is a Qt Protobuf gadget
// (`control::v1::DisplayBackendStatus`), for us a QVariantMap. A gadget
// cannot be expressed in cxx-qt 0.9, and QML reads only its fields with a
// dot — on a map this works exactly the same.
//
// NOT ported deliberately (as in the core): `taskProgress`. The daemon has
// the event, but the vendored QML v0.2.6+9 does not use it once — checked by
// grep over `waywallen/ui/qml`.

/// cxx-qt bridge: the `Notify` singleton.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    // Warning: `type QObject` is NOT declared here: Notify has no base, and
    // cxx-qt emits the declaration itself (cxx-qt-gen: add_qobject_import). A
    // second one gives «the type QObject is defined multiple times».
    unsafe extern "C++" {
        include!("rexon_we_singletons.h");
        /// Register as a receiver of daemon frames.
        ///
        /// # Safety
        /// `notify` is a live QObject (a pointer to itself).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_register_notify(notify: *mut QObject);
    }

    /// The daemon phase. The values come from the original (`notify.cppm`),
    /// QML reads them as `Notify.DaemonPhase.Ready`.
    #[qenum(Notify)]
    enum DaemonPhase {
        Starting,
        Ready,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, scan_in_progress, cxx_name = "scanInProgress",
                    READ = scan_in_progress, NOTIFY = status_changed)]
        #[qproperty(u32, active_task_count, cxx_name = "activeTaskCount",
                    READ = active_task_count, NOTIFY = status_changed)]
        #[qproperty(bool, global_paused, cxx_name = "globalPaused",
                    READ = global_paused, NOTIFY = status_changed)]
        #[qproperty(bool, global_muted, cxx_name = "globalMuted",
                    READ = global_muted, NOTIFY = status_changed)]
        #[qproperty(bool, global_stopped, cxx_name = "globalStopped",
                    READ = global_stopped, NOTIFY = status_changed)]
        #[qproperty(i32, daemon_phase, cxx_name = "daemonPhase",
                    READ = daemon_phase, NOTIFY = status_changed)]
        #[qproperty(QMap_QString_QVariant, display_backend, cxx_name = "displayBackend",
                    READ = display_backend, NOTIFY = status_changed)]
        type Notify = super::NotifyRust;
    }

    // ── signals (names and signatures come from the original) ─────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut Notify>);

        #[qsignal]
        #[cxx_name = "daemonReady"]
        fn daemon_ready(self: Pin<&mut Notify>);

        #[qsignal]
        #[cxx_name = "wallpaperSyncFinished"]
        fn wallpaper_sync_finished(self: Pin<&mut Notify>, count: u32, error: QString);

        /// waywallen 0.4.4: wallpapers were hidden or restored.
        #[qsignal]
        #[cxx_name = "wallpaperHiddenChanged"]
        fn wallpaper_hidden_changed(self: Pin<&mut Notify>, wallpaper_ids: QStringList,
                                    hidden: bool);

        #[qsignal]
        #[cxx_name = "librariesAdded"]
        fn libraries_added(self: Pin<&mut Notify>, paths: QStringList);

        #[qsignal]
        #[cxx_name = "settingsChanged"]
        fn settings_changed(self: Pin<&mut Notify>);

        /// Warning: a signal that our Python bridge declared but never emitted:
        /// the event branch did not exist (fixed in the core).
        #[qsignal]
        #[cxx_name = "displayConnectionFailed"]
        fn display_connection_failed(
            self: Pin<&mut Notify>,
            client_name: QString,
            client_protocol_version: u32,
            error_code: u32,
            reason: QString,
        );

        #[qsignal]
        #[cxx_name = "remoteDownloadProgress"]
        fn remote_download_progress(
            self: Pin<&mut Notify>,
            source_id: QString,
            id: QString,
            state: i32,
            error: QString,
        );

        #[qsignal]
        #[cxx_name = "playlistChanged"]
        fn playlist_changed(self: Pin<&mut Notify>);

        #[qsignal]
        #[cxx_name = "pluginChanged"]
        fn plugin_changed(self: Pin<&mut Notify>);

        #[qsignal]
        #[cxx_name = "pluginUpdateChanged"]
        fn plugin_update_changed(self: Pin<&mut Notify>);

        #[qsignal]
        #[cxx_name = "pluginRestartFailed"]
        fn plugin_restart_failed(self: Pin<&mut Notify>, plugin_id: QString, error: QString);

        #[qsignal]
        #[cxx_name = "pluginStateChanged"]
        fn plugin_state_changed(self: Pin<&mut Notify>);

        #[qsignal]
        #[cxx_name = "qrLoginProgress"]
        fn qr_login_progress(
            self: Pin<&mut Notify>,
            session_id: QString,
            plugin_id: QString,
            action_id: QString,
            state: i32,
            qr_image: QString,
            // waywallen 0.3.8: the four texts of the dialog are translatable —
            // either a `PluginMessage` map or a ready string goes out, exactly
            // as with the vendor's `pluginMessageFromPb`. QML passes them through
            // `I18n.tr`, which understands both forms.
            display_value: QVariant,
            error: QVariant,
            title: QVariant,
            instruction: QVariant,
        );

        /// waywallen 0.6.1: sign-in through a browser window
        /// (BrowserLoginDialog). Same texts as `qrLoginProgress`, no QR image.
        #[qsignal]
        #[cxx_name = "browserLoginProgress"]
        fn browser_login_progress(
            self: Pin<&mut Notify>,
            session_id: QString,
            plugin_id: QString,
            action_id: QString,
            state: i32,
            display_value: QVariant,
            error: QVariant,
            title: QVariant,
            instruction: QVariant,
        );
    }

    extern "RustQt" {
        #[cxx_name = "scanInProgress"]
        fn scan_in_progress(self: &Notify) -> bool;
        #[cxx_name = "activeTaskCount"]
        fn active_task_count(self: &Notify) -> u32;
        #[cxx_name = "globalPaused"]
        fn global_paused(self: &Notify) -> bool;
        #[cxx_name = "globalMuted"]
        fn global_muted(self: &Notify) -> bool;
        #[cxx_name = "globalStopped"]
        fn global_stopped(self: &Notify) -> bool;
        #[cxx_name = "daemonPhase"]
        fn daemon_phase(self: &Notify) -> i32;
        #[cxx_name = "displayBackend"]
        fn display_backend(self: &Notify) -> QMap_QString_QVariant;

        /// A raw `ServerFrame` frame from the daemon. Called by the shim
        /// (`rexon_we_do_deliver_event`), not by QML: the name must match what
        /// `QMetaObject::invokeMethod` looks for.
        #[qinvokable]
        #[cxx_name = "deliverEvent"]
        fn deliver_event(self: Pin<&mut Notify>, frame: &QByteArray);

        /// WebSocket disconnection: go back to the pessimistic state so that the
        /// UI does not keep a stale «daemon is ready».
        #[qinvokable]
        #[cxx_name = "transportDisconnected"]
        fn transport_disconnected(self: Pin<&mut Notify>);
    }

    impl cxx_qt::Initialize for Notify {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use rexon_core::we_notify::{self, NotifySignal, NotifyState};

/// A translatable plugin string → a value for QML.
///
/// The form is the same as the vendor's `pluginMessageFromPb` and the `*Text`
/// maps in a source description: a `{pluginId, msgid, arguments}` map when
/// the label has a catalogue key, and a bare string when it does not. The
/// vendored `I18n.tr` understands both.
fn plugin_text_variant(text: &we_notify::PluginText) -> QVariant {
    if !text.has_catalog_key() {
        return QVariant::from(&QString::from(&text.fallback));
    }
    let mut map = serde_json::Map::new();
    map.insert(
        "pluginId".into(),
        serde_json::Value::String(text.plugin_id.clone()),
    );
    map.insert("msgid".into(), serde_json::Value::String(text.msgid.clone()));
    map.insert(
        "arguments".into(),
        serde_json::Value::Array(
            text.arguments
                .iter()
                .map(|a| serde_json::Value::String(a.clone()))
                .collect(),
        ),
    );
    // The fallback text goes alongside: `plugin_l10n::resolve` takes it if the
    // plugin's catalogue is not on the machine.
    map.insert(
        "rexonFallback".into(),
        serde_json::Value::String(text.fallback.clone()),
    );
    crate::qvalue::json_to_variant(&serde_json::Value::Object(map))
}

/// The singleton's state belongs entirely to the core.
#[derive(Default)]
pub struct NotifyRust {
    state: NotifyState,
}

impl cxx_qt::Initialize for qobject::Notify {
    fn initialize(self: Pin<&mut Self>) {
        // Register as a receiver AT ONCE, in post-construction: the daemon sends
        // the initial `status_sync` once per WS connect, and a singleton created
        // later will lose the transition to Ready. The original has exactly the
        // same reason to set up Notify before loading QML (`App::init`).
        let this: *mut qobject::Notify = unsafe { self.get_unchecked_mut() };
        unsafe { qobject::rexon_we_register_notify(this.cast()) };
    }
}

impl qobject::Notify {
    pub fn scan_in_progress(&self) -> bool {
        self.rust().state.scan_in_progress
    }

    pub fn global_paused(&self) -> bool {
        self.rust().state.global_paused
    }

    pub fn global_muted(&self) -> bool {
        self.rust().state.global_muted
    }

    pub fn global_stopped(&self) -> bool {
        self.rust().state.global_stopped
    }

    pub fn active_task_count(&self) -> u32 {
        self.rust().state.active_task_count
    }

    pub fn daemon_phase(&self) -> i32 {
        self.rust().state.daemon_phase
    }

    pub fn display_backend(&self) -> qobject::QMap_QString_QVariant {
        let backend = &self.rust().state.display_backend;
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in [
            ("name", &backend.name),
            ("state", &backend.state),
            ("desktop", &backend.desktop),
            ("binary", &backend.binary),
            ("reason", &backend.reason),
            ("flatpakId", &backend.flatpak_id),
        ] {
            map.insert(QString::from(key), QVariant::from(&QString::from(value)));
        }
        map
    }

    pub fn deliver_event(mut self: Pin<&mut Self>, frame: &QByteArray) {
        let mut state = self.rust().state.clone();
        // A broken frame must not bring the tab down: the daemon may be newer than us.
        let Ok(signals) = we_notify::apply_frame(&mut state, frame.as_slice()) else {
            return;
        };
        self.as_mut().rust_mut().state = state;
        self.emit_all(&signals);
    }

    pub fn transport_disconnected(mut self: Pin<&mut Self>) {
        let mut state = self.rust().state.clone();
        let signals = we_notify::on_disconnected(&mut state);
        self.as_mut().rust_mut().state = state;
        self.emit_all(&signals);
    }

    /// Broadcast the list of signals computed by the core. The order is kept —
    /// it is part of the contract (`we_notify_parity.rs`).
    fn emit_all(mut self: Pin<&mut Self>, signals: &[NotifySignal]) {
        for signal in signals {
            match signal {
                NotifySignal::StatusChanged => self.as_mut().status_changed(),
                NotifySignal::DaemonReady => self.as_mut().daemon_ready(),
                NotifySignal::WallpaperSyncFinished { count, error } => self
                    .as_mut()
                    .wallpaper_sync_finished(*count, QString::from(error)),
                NotifySignal::WallpaperHiddenChanged { wallpaper_ids, hidden } => {
                    let list = QStringList::from_iter(wallpaper_ids.iter().map(QString::from));
                    self.as_mut().wallpaper_hidden_changed(list, *hidden);
                }
                NotifySignal::LibrariesAdded { paths } => {
                    let mut list = QStringList::default();
                    for path in paths {
                        list.append(QString::from(path));
                    }
                    self.as_mut().libraries_added(list);
                }
                NotifySignal::SettingsChanged => self.as_mut().settings_changed(),
                NotifySignal::DisplayConnectionFailed {
                    client_name,
                    client_protocol_version,
                    error_code,
                    reason,
                } => self.as_mut().display_connection_failed(
                    QString::from(client_name),
                    *client_protocol_version,
                    *error_code,
                    QString::from(reason),
                ),
                NotifySignal::RemoteDownloadProgress {
                    source_id,
                    id,
                    state,
                    error,
                    transfer,
                } => {
                    // Like the original's `Notify`: the state and the transfer
                    // progress go first to the shared store (the result card and
                    // the details panel), then the signal to the page.
                    let transfer = transfer.map_or_else(Default::default, |(received, total)| {
                        crate::remote_list_model::acquisition::Transfer {
                            active: true,
                            received,
                            total,
                        }
                    });
                    crate::remote_list_model::acquisition::set_progress(
                        source_id, id, *state, transfer,
                    );
                    self.as_mut().remote_download_progress(
                        QString::from(source_id),
                        QString::from(id),
                        *state,
                        QString::from(error),
                    )
                }
                NotifySignal::PlaylistChanged => self.as_mut().playlist_changed(),
                NotifySignal::PluginChanged => self.as_mut().plugin_changed(),
                NotifySignal::PluginUpdateChanged => self.as_mut().plugin_update_changed(),
                NotifySignal::PluginRestartFailed { plugin_id, error } => self
                    .as_mut()
                    .plugin_restart_failed(QString::from(plugin_id), QString::from(error)),
                NotifySignal::PluginStateChanged => self.as_mut().plugin_state_changed(),
                NotifySignal::QrLoginProgress {
                    session_id,
                    plugin_id,
                    action_id,
                    state,
                    qr_image,
                    display_value,
                    error,
                    title,
                    instruction,
                } => self.as_mut().qr_login_progress(
                    QString::from(session_id),
                    QString::from(plugin_id),
                    QString::from(action_id),
                    *state,
                    QString::from(qr_image),
                    plugin_text_variant(display_value),
                    plugin_text_variant(error),
                    plugin_text_variant(title),
                    plugin_text_variant(instruction),
                ),
                NotifySignal::BrowserLoginProgress {
                    session_id,
                    plugin_id,
                    action_id,
                    state,
                    display_value,
                    error,
                    title,
                    instruction,
                } => self.as_mut().browser_login_progress(
                    QString::from(session_id),
                    QString::from(plugin_id),
                    QString::from(action_id),
                    *state,
                    plugin_text_variant(display_value),
                    plugin_text_variant(error),
                    plugin_text_variant(title),
                    plugin_text_variant(instruction),
                ),
            }
        }
    }
}
