//! QML-синглтон `Notify` вкладки Wallpaper Engine — порт `ui/src/notify.cpp`
//! waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//!
//! Синглтон — ретранслятор событий демона в QML: держит снимок состояния
//! (идёт ли скан, сколько задач, фаза демона, бэкенд вывода) и превращает
//! `GlobalEvent` с провода в сигналы Qt. Здесь — ВСЯ решающая часть:
//! разбор кадра (prost, схема демона уже в [`crate::proto`]), переход
//! состояния и список сигналов к отправке. Python-шим остаётся оболочкой
//! QObject: получил байты — отдал их сюда — разослал полученные сигналы.
//!
//! Это первый модуль, где протобуф демона разбирается на Rust, а не
//! Python-протобуфом: `Notify` не читает поля ответов (`pb.Response`),
//! только события, поэтому границу можно двигать по нему отдельно от
//! остальных классов моста.
//!
//! ⚠️ Починки при порте (правило #16):
//!   1. `displayConnectionFailed` — сигнал в нашем Python БЫЛ объявлен, но
//!      не эмитился НИКОГДА: ветки события в обработчике не было. Демон
//!      сообщает этим событием, что чужой клиент не смог подключиться
//!      (несовпадение протокола/версии) — сообщение молча терялось.
//!   2. `statusChanged` эмитился на КАЖДЫЙ `StatusSync`, даже когда ни одно
//!      поле не изменилось (демон шлёт снимок на любое изменение своего
//!      состояния, в т.ч. постороннее). Оригинал сравнивает снимок с
//!      текущим и молчит, если разницы нет; здесь так же — лишние
//!      пересчёты привязок QML убраны.
//!   3. Сброс при обрыве WebSocket тоже эмитил сигнал безусловно; теперь —
//!      только если состояние действительно менялось (как в оригинале).
//!
//! v0.2.6: добавлено событие `QrLoginProgress` (вход по QR-коду в
//! диалоге плагина-источника) — вендоренный QML на него подписан.
//!
//! НЕ портировано намеренно: `taskProgress` — событие есть у демона и в
//! свежем апстриме, но вендоренный QML v0.2.5 его не использует (проверено
//! grep'ом по `ui/qml`), а тащить в мост неиспользуемый сигнал незачем.
//! Появится при обновлении вендора — ветка добавляется сюда.

use crate::proto::control as pb;
use prost::Message;
use serde::{ser::SerializeMap, Serialize, Serializer};

/// `DAEMON_PHASE_READY` протокола демона — берётся ИЗ СХЕМЫ, а не
/// литералом: в enum есть `Unspecified = 0`, поэтому Ready = 2, и
/// написанная от руки единица тихо оставляла бы UI в фазе Starting
/// (поймано roundtrip-тестом при первом же прогоне).
const DAEMON_PHASE_READY: i32 = pb::DaemonPhase::Ready as i32;

/// Фаза демона в терминах QML-перечисления `Notify.DaemonPhase`.
pub const PHASE_STARTING: i32 = 0;
/// Демон готов.
pub const PHASE_READY: i32 = 1;

/// Снимок `DisplayBackendStatus` в виде, в котором его читает QML.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DisplayBackend {
    pub name: String,
    pub state: String,
    pub desktop: String,
    pub binary: String,
    pub reason: String,
    /// ⚠️ Наружу уходит как `flatpakId` — так его читает и QML, и шим.
    #[serde(rename = "flatpakId")]
    pub flatpak_id: String,
}

/// Состояние синглтона (свойства `scanInProgress`/`activeTaskCount`/
/// `daemonPhase`/`displayBackend`/`globalPaused`/`globalMuted`/
/// `globalStopped`).
///
/// ⚠️ Три глобальных переключателя приехали в `StatusSync` вместе с
/// протоколом v0.3.5, и до сих пор их не читал НИКТО: партия 7 привезла
/// команды `GlobalPauseSetQuery`/`GlobalMuteSetQuery`/`GlobalStopSetQuery`,
/// а состояние осталось за бортом. Вендорная `StatusPage.qml` читает
/// ровно `W.Notify.globalPaused`/`globalMuted`/`globalStopped` — на
/// несуществующих свойствах три ярлыка «Pause all»/«Mute all»/«Stop all»
/// никогда не показывали, включён режим или нет («Unable to assign
/// [undefined] to bool»). Правило #12: команда и её состояние — одна
/// сущность, добавляются вместе.
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

/// Сигнал Qt, который шим обязан отправить (в порядке возврата).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "signal", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NotifySignal {
    /// `statusChanged()` — изменился снимок состояния.
    StatusChanged,
    /// `daemonReady()` — фаза перешла Starting → Ready.
    DaemonReady,
    /// `wallpaperSyncFinished(count, error)`.
    WallpaperSyncFinished { count: u32, error: String },
    /// `wallpaperHiddenChanged(wallpaperIds, hidden)` — waywallen 0.4.4:
    /// демон скрыл обои или вернул их (любым клиентом).
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
    },
    /// `playlistChanged()`.
    PlaylistChanged,
    /// `pluginChanged()`.
    PluginChanged,
    /// `pluginUpdateChanged()`.
    PluginUpdateChanged,
    /// `pluginRestartFailed(pluginId, error)`.
    PluginRestartFailed { plugin_id: String, error: String },
    /// `pluginStateChanged()` — изменилось состояние плагина-источника
    /// (v0.2.6; Discover перечитывает список источников).
    PluginStateChanged,
    /// `qrLoginProgress(...)` — вход по QR-коду (v0.2.6, QrLoginDialog).
    ///
    /// ⚠ waywallen 0.3.8: четыре текста диалога переводимы — рядом с
    /// прежними строками едет `PluginMessage`, и вендорный QML пропускает
    /// их через `I18n.tr`. Поэтому здесь [`PluginText`], а не `String`:
    /// строка осталась запаской внутри него.
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
}

/// Переводимая строка плагина: либо подпись из его каталога, либо готовый
/// текст, который демон прислал сам.
///
/// Пустые `plugin_id`/`msgid` означают «каталога нет» — тогда наружу уходит
/// голая строка `fallback`, как её отдаёт `pluginMessageFromPb` вендора.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginText {
    pub plugin_id: String,
    pub msgid: String,
    pub arguments: Vec<String>,
    pub fallback: String,
}

impl PluginText {
    /// Есть ли подпись в каталоге плагина (иначе наружу идёт `fallback`).
    pub fn has_catalog_key(&self) -> bool {
        !self.plugin_id.is_empty() && !self.msgid.is_empty()
    }
}

/// Форма наружу — ОДНА для всех потребителей: словарь
/// `{pluginId, msgid, arguments, rexonFallback}`, когда у подписи есть ключ
/// каталога, и голая строка, когда его нет (так же отдаёт `pluginMessageFromPb`
/// вендора и так же читает вендорный QML).
///
/// ⚠️ Здесь `Serialize` написан РУКАМИ намеренно: тип разнотипный (строка либо
/// словарь), а `#[serde(untagged)]` бывает только у перечислений. Взамен форма
/// объявлена ОДИН раз и рядом с самим типом — переписывать её в мосту на
/// каждое поле, где встретился `PluginText`, больше не нужно.
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

/// `PluginMessage` протокола + прежнее строковое поле → [`PluginText`].
///
/// Публичен нарочно: это порт `pluginMessageFromPb` вендора, и он нужен не
/// только оповещениям — схемы настроек плагинов (`we_query::SettingSchema`)
/// собирают свои подписи ТЕМ ЖЕ правилом. Вторая копия правила разошлась бы
/// с этой молча (правило #12).
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

/// Разбор кадра сервера: событие или ничего.
///
/// На вход идут СЫРЫЕ байты `ServerFrame` с WebSocket — ровно то, что
/// пришло от демона. Кадр с ответом (`Response`) событий не даёт: ответы
/// по-прежнему разбирает Python (их читают колбэки запросов).
pub fn parse_event(frame_bytes: &[u8]) -> Result<Option<pb::Event>, prost::DecodeError> {
    let frame = pb::ServerFrame::decode(frame_bytes)?;
    Ok(match frame.kind {
        Some(pb::server_frame::Kind::Event(e)) => Some(e),
        _ => None,
    })
}

/// Применить кадр к состоянию и получить список сигналов.
///
/// Битый кадр — не ошибка вызывающего: демон может быть новее нас, и
/// падать посреди приёма нельзя. Возвращается `Err`, шим пишет отладку и
/// живёт дальше (так же ведёт себя Python-эталон).
pub fn apply_frame(
    state: &mut NotifyState,
    frame_bytes: &[u8],
) -> Result<Vec<NotifySignal>, prost::DecodeError> {
    let Some(event) = parse_event(frame_bytes)? else {
        return Ok(Vec::new());
    };
    Ok(apply_event(state, &event))
}

/// Тот же переход состояния от уже разобранного события.
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
        // Снимки/изменения дисплеев, рендереров и библиотек Notify не
        // касаются — у них свои модели (RendererManager/DisplayManager/
        // LibraryManager), они читают события сами.
        _ => {}
    }
    out
}

/// Обрыв WebSocket: вернуться к пессимистичному состоянию, чтобы UI не
/// держал устаревшее «демон готов», пока демона нет.
pub fn on_disconnected(state: &mut NotifyState) -> Vec<NotifySignal> {
    let fresh = NotifyState::default();
    if *state == fresh {
        return Vec::new();
    }
    *state = fresh;
    vec![NotifySignal::StatusChanged]
}
