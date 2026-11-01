// Notify — QML-синглтон событий демона вкладки Wallpaper Engine.
//
// Порт
// `ui/src/notify.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//
// Решающей части здесь нет: разбор кадра `ServerFrame`, переход состояния и
// СПИСОК сигналов к отправке считает ядро (`rexon_core::we_notify`, прикрыто
// `we_notify_parity.rs`). Мосту остаётся раздать полученный список.
//
// Две починки апстрима живут в ядре и приезжают вместе с ним:
// `displayConnectionFailed` теперь действительно эмитится (ветки события не
// было вовсе), а `statusChanged` молчит, если снимок не изменился.
//
// ⚠️ `displayBackend` у оригинала — гаджет Qt-Protobuf
// (`control::v1::DisplayBackendStatus`), у нас — QVariantMap. Гаджет в
// cxx-qt 0.9 не выразим, а QML читает у него только поля
// точкой — на карте это работает ровно так же.
//
// НЕ портировано намеренно (как и в ядре): `taskProgress`. Событие у демона
// есть, но вендорный QML v0.2.6+9 его не использует ни разу — проверено
// grep'ом по `waywallen/ui/qml`.

/// Мост cxx-qt: синглтон `Notify`.
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

    // ⚠️ `type QObject` здесь НЕ объявляется: у Notify нет базы, и cxx-qt
    // печатает объявление сам (cxx-qt-gen: add_qobject_import). Второе даёт
    // «the type QObject is defined multiple times».
    unsafe extern "C++" {
        include!("rexon_we_singletons.h");
        /// Записаться получателем кадров демона.
        ///
        /// # Safety
        /// `notify` — живой QObject (указатель на самого себя).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_register_notify(notify: *mut QObject);
    }

    /// Фаза демона. Значения — из оригинала (`notify.cppm`), QML читает их
    /// как `Notify.DaemonPhase.Ready`.
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

    // ── сигналы (имена и сигнатуры — из оригинала) ────────────────────────
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

        /// waywallen 0.4.4: обои скрыты или возвращены.
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

        /// ⚠️ Сигнал, который у нашего Python-моста был объявлен, но не
        /// эмитился ни разу: ветки события не существовало (починено в ядре).
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
            // waywallen 0.3.8: четыре текста диалога переводимы — наружу
            // едет либо карта `PluginMessage`, либо готовая строка, ровно
            // как у `pluginMessageFromPb` вендора. QML пропускает их через
            // `I18n.tr`, который понимает обе формы.
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

        /// Сырой кадр `ServerFrame` от демона. Зовётся шимом
        /// (`rexon_we_do_deliver_event`), а не QML: имя обязано совпадать с
        /// тем, что ищет `QMetaObject::invokeMethod`.
        #[qinvokable]
        #[cxx_name = "deliverEvent"]
        fn deliver_event(self: Pin<&mut Notify>, frame: &QByteArray);

        /// Обрыв WebSocket: вернуться к пессимистичному состоянию, чтобы UI
        /// не держал устаревшее «демон готов».
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

/// Переводимая строка плагина → значение для QML.
///
/// Форма та же, что у `pluginMessageFromPb` вендора и у карт `*Text` в
/// описании источника: карта `{pluginId, msgid, arguments}`, когда у
/// подписи есть ключ каталога, и голая строка, когда его нет. Вендорный
/// `I18n.tr` понимает обе.
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
    // Запасной текст рядом: `plugin_l10n::resolve` берёт его, если каталога
    // плагина на машине нет.
    map.insert(
        "rexonFallback".into(),
        serde_json::Value::String(text.fallback.clone()),
    );
    crate::qvalue::json_to_variant(&serde_json::Value::Object(map))
}

/// Состояние синглтона целиком принадлежит ядру.
#[derive(Default)]
pub struct NotifyRust {
    state: NotifyState,
}

impl cxx_qt::Initialize for qobject::Notify {
    fn initialize(self: Pin<&mut Self>) {
        // Записаться получателем СРАЗУ, в пост-конструкции: демон шлёт
        // стартовый `status_sync` один раз на WS-connect, и синглтон,
        // созданный позже, переход в Ready потеряет. У оригинала ровно та же
        // причина завести Notify до загрузки QML (`App::init`).
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
        // Битый кадр не должен ронять вкладку: демон может быть новее нас.
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

    /// Разослать список сигналов, посчитанный ядром. Порядок сохраняется —
    /// он часть контракта (`we_notify_parity.rs`).
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
                } => self.as_mut().remote_download_progress(
                    QString::from(source_id),
                    QString::from(id),
                    *state,
                    QString::from(error),
                ),
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
            }
        }
    }
}
