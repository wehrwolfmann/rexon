//! База `Query` и простые запросы вкладки Wallpaper Engine — порт
//! `qextra::QAsyncResult`/`qextra::Query` + `ui/src/query/{health,renderer,
//! tag}_query.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//!
//! База — конечный автомат асинхронного результата: статус
//! (Uninitialized/Querying/Finished/Error), текст ошибки, флаг пересылки
//! ошибки в глобальный обработчик. Вся решающая часть здесь: переход
//! состояния и ПОРЯДОК сигналов Qt, которые обязан разослать шим.
//! Наследники добавляют разбор своего ответа: сырые байты кадра →
//! prost → новые значения свойств + сигналы `<имя>Changed`.
//!
//! Почему не cxx-qt: он не умеет наследование Rust-QObject от
//! Rust-QObject, а вся иерархия запросов вкладки — именно наследование
//! от одной базы. Поэтому состояние базы ВСТРОЕНО структурой
//! ([`QueryState`]) в каждый запрос, а QObject'ами остаются
//! PySide6-оболочки в `qml_ui/bridge.py` — ровно как у `Notify`/`Util`.
//!
//! ⚠️ Расхождения с оригиналом (осознанные, правило #16):
//!   1. Сигналы свойств (`serviceChanged`, `tagsChanged`, …) шлются
//!      ТОЛЬКО при реальном изменении значения. Оригинал шлёт их
//!      безусловно на каждый успешный ответ (`Q_EMIT self->tagsChanged()`
//!      сразу после присваивания) — то есть перевычисляет привязки QML
//!      даже когда демон вернул тот же список. Тот же класс, что был
//!      починен в `Notify` (`statusChanged` на каждый StatusSync).
//!      Наблюдаемое поведение не меняется: ни один обработчик
//!      `on<Свойство>Changed` в вендоренном QML на эти сигналы не
//!      подписан (проверено grep'ом по `ui/qml`), они питают только
//!      привязки свойств, а привязке нужен сигнал лишь при изменении.
//!   2. `HealthQuery` в нашем прежнем Python-мосту УЖЕ сравнивал
//!      значения, а `TagListQuery`/`RendererListQuery` — нет. Теперь
//!      правило одно на все запросы (правило #12: чиним движок, а не
//!      отдельные места).
//!
//! Отличие транспорта: у оригинала `reload()` сначала ставит Querying и
//! лишь потом узнаёт об ошибке отправки; у нас соединение проверяется
//! ДО отправки ([`begin_call`]), поэтому при мёртвом демоне статус идёт
//! сразу в Error, минуя Querying. Это наша ветка (у оригинала её нет —
//! он всегда «отправляет» в backend), сохранено как было в мосту.

use crate::proto::control as pb;
use crate::we_notify::{plugin_text, PluginText};
use prost::Message;
use serde::{ser::SerializeMap, Serialize, Serializer};

/// `qextra::QAsyncResult::Status` — значения перечисления Qt как есть
/// (QML читает их числами через `Query.Status`).
pub const STATUS_UNINITIALIZED: i32 = 0;
/// Запрос отправлен, ответа нет.
pub const STATUS_QUERYING: i32 = 1;
/// Ответ получен и разобран.
pub const STATUS_FINISHED: i32 = 2;
/// Ошибка соединения, транспорта или демона.
pub const STATUS_ERROR: i32 = 3;

/// Текст, которым мост объясняет отсутствие соединения с демоном.
///
/// Английский msgid каталога; на экран идёт перевод (`tr`) в [`set_error`]-
/// вызове ниже.
pub const NO_CONNECTION: &str = crate::i18n::n_("no connection to the daemon");

/// Состояние базы `Query`, встроенное в каждый запрос.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryState {
    pub status: i32,
    pub error: String,
    /// `forwardError` — дублировать ли ошибку в глобальный обработчик
    /// (`App.errorOccurred` → тост в Window.qml). Оригинал: `true` по
    /// умолчанию, страницы с собственным показом ошибки ставят `false`.
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

/// Сигнал Qt, который шим обязан отправить (строго в порядке возврата).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySignal {
    /// `statusChanged(Status)` — аргумент читают обработчики QML вида
    /// `function onStatusChanged(status)`.
    StatusChanged(i32),
    /// `queryingChanged()` — производное свойство `status == Querying`.
    QueryingChanged,
    /// `errorChanged()`.
    ErrorChanged,
    /// `forwardErrorChanged()`.
    ForwardErrorChanged,
    /// `finished()` — статус стал Finished.
    Finished,
    /// `errorOccurred(error)` — статус стал Error.
    ErrorOccurred(String),
    /// Глобальная пересылка: `App.errorOccurred(error)`. Отдельный
    /// вариант, потому что сигнал шлёт НЕ сам запрос, а синглтон App.
    AppErrorOccurred(String),
    /// `<имя>Changed()` конкретного запроса (`service`, `tags`, …).
    Property(&'static str),
}

/// Форма сигнала наружу — словарь `{signal: имя, ...аргументы}`, ровно тот,
/// что читает шим и вкладка.
///
/// ⚠️ `Serialize` написан РУКАМИ намеренно, и вот почему: у трёх вариантов
/// полезная нагрузка — голый скаляр (`StatusChanged(i32)`,
/// `ErrorOccurred(String)`, `Property(&str)`), а `#[serde(tag = "...")]`
/// с такими не работает вовсе — внутренний тег требует, чтобы содержимое
/// само сериализовалось картой. Переписывать перечисление на именованные
/// поля ради вывода — 126 мест правки в пятнадцати файлах, включая чужие;
/// цена не стоит выигрыша. Взамен форма объявлена ОДИН раз и рядом с самим
/// типом: новый вариант перечисления обязан появиться здесь же (компилятор
/// потребует ветку), а не в мосту за границей крейта.
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

/// Полезная нагрузка сигнала одного из двух видов (см. [`QuerySignal`]).
enum Arg<'a> {
    Int(i32),
    Str(&'a str),
}

/// Текст ошибки из ответа демона: `message`, а при пустом — «status N»
/// (так делает `backend.cpp` оригинала, чтобы UI не показывал пустоту).
pub fn response_error_text(status: i32, message: &str) -> String {
    if message.is_empty() {
        format!("status {status}")
    } else {
        message.to_string()
    }
}

/// `setError` базы: свойство биндабельное, сигнал только при изменении.
pub fn set_error(st: &mut QueryState, text: &str) -> Vec<QuerySignal> {
    if st.error == text {
        return Vec::new();
    }
    st.error = text.to_string();
    vec![QuerySignal::ErrorChanged]
}

/// `setForwardError` базы.
pub fn set_forward_error(st: &mut QueryState, value: bool) -> Vec<QuerySignal> {
    if st.forward_error == value {
        return Vec::new();
    }
    st.forward_error = value;
    vec![QuerySignal::ForwardErrorChanged]
}

/// `setStatus` базы — весь порядок рассылки оригинала в одном месте.
///
/// `QAsyncResult` в конструкторе вешает на `statusChanged` лямбду:
/// сначала `finished()`/`errorOccurred(error)`, и только затем, если
/// стоит `forwardError`, ошибка уходит в глобальный обработчик.
/// `querying` — производное свойство (`status == Querying`), его сигнал
/// летит лишь когда производное значение действительно изменилось.
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

/// Начало запроса: `(отправлять ли кадр, сигналы)`.
///
/// Нет живого соединения — кадр не отправляется, запрос сразу в Error с
/// текстом [`NO_CONNECTION`] (ветка нашего транспорта, см. заголовок).
pub fn begin_call(st: &mut QueryState, connected: bool) -> (bool, Vec<QuerySignal>) {
    if !connected {
        let mut out = set_error(st, &crate::i18n::tr(NO_CONNECTION));
        out.extend(set_status(st, STATUS_ERROR));
        return (false, out);
    }
    (true, set_status(st, STATUS_QUERYING))
}

/// Снять `Response` с сырого кадра сервера и обработать не-OK статус.
///
/// `None` в первом поле = демон ответил ошибкой, состояние уже переведено
/// в Error и полезную нагрузку читать не нужно (так же `inspect_set`
/// оригинала зовёт лямбду разбора только на успехе).
///
/// Битый кадр — `Err`: шим пишет отладку и живёт дальше (демон может
/// быть новее нас, падать посреди приёма нельзя). Кадр с событием
/// вместо ответа для запроса невозможен (клиент маршрутизирует ответы по
/// `request_id`), но трактуется так же — как неразобранный.
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

/// Общий хвост успешного разбора: свойства уже обновлены — статус в
/// Finished.
pub fn finish(st: &mut QueryState, mut out: Vec<QuerySignal>) -> Vec<QuerySignal> {
    out.extend(set_status(st, STATUS_FINISHED));
    out
}

/// Разбор ответа на запрос-действие: полезной нагрузки нет, важен только
/// исход.
///
/// У оригинала это `inspect_set(result, [](const Response&) {})` — пустая
/// лямбда разбора: команда либо принята демоном (статус Finished), либо
/// отклонена (Error с его текстом). Одна точка на все такие запросы вкладки
/// (правило #12), а не по копии в каждом.
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

/// Присвоить значение и отметить сигнал, если оно изменилось.
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

// ── HealthQuery (страница «Статус», Window.qml) ─────────────────────────

/// Свойства `HealthQuery`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthData {
    pub service: String,
    pub state: String,
    /// Система, на которой работает демон (v0.3.5). Вендорная страница
    /// «Статус» показывает строку и прячет ярлык, пока она пуста
    /// (`StatusPage.qml:222`).
    pub os_name: String,
}

/// Разобрать ответ `health`.
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

// ── DaemonLogQuery (диалог «Журнал демона», v0.3.8) ─────────────────────

/// Свойства `DaemonLogQuery`.
///
/// waywallen 0.3.8 научился отдавать свой журнал одним запросом
/// (`log_read`), а страница «Статус» — показывать его в окне. Полей три, и
/// все три идут прямо из ответа: путь файла, его содержимое и признак, что
/// демон отдал не весь файл, а хвост.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonLogData {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

/// Разобрать ответ `log_read`.
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

// ── RendererListQuery (страница «Статус») ───────────────────────────────

/// `RendererInstance` в виде, в котором его читает QML (ключи карты
/// `_renderer_info_dict` моста: `id` вместо `renderer_id`, остальное
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

/// Одно `RuntimeCondition` в виде, в котором его читает QML: строковые
/// `kind`/`origin` вместо чисел перечисления.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCondition {
    pub kind: String,
    pub origin: String,
    pub reason: String,
    pub related_renderer_id: String,
    pub related_display_id: u64,
}

/// Пара `key`/`value` из `RendererRuntimeTag`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeTag {
    pub key: String,
    pub value: String,
}

/// Строка `status` рендерера из дерева `RendererState` (v0.3.5).
///
/// До v0.3.5 демон присылал готовую строку в `RendererInstance.status`
/// (поле 3). В v0.3.5 поле снесено в `reserved`, а состояние приезжает
/// деревом `RendererState`; вендорный QML при этом по-прежнему читает
/// `status` и `running` (`StatusPage.qml:344/347/374`). Значит строку
/// собираем сами — ОДНОЙ функцией на всех потребителей (ответ
/// `renderer_list` здесь и живые модели `rexon-gui`), правило #12.
///
/// Соответствие один в один с `Renderer::status` вендора
/// (`ui/src/objmodel/renderer.cpp`), включая пустую строку на неизвестное
/// состояние: на ней вендорный QML прячет ярлык состояния целиком.
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

/// `running` рендерера — порт `Renderer::running`: живым считается только
/// работающий процесс (играет, на паузе или без звука).
pub fn renderer_running(status: &str) -> bool {
    matches!(status, "playing" | "paused" | "muted")
}

/// `keep` — порт `Renderer::keep`: процесс придержан после остановки.
pub fn renderer_keep(info: &pb::RendererInstance) -> bool {
    match info.state.as_ref().and_then(|s| s.kind.as_ref()) {
        Some(pb::renderer_state::Kind::Stopping(s)) => s.keep,
        Some(pb::renderer_state::Kind::Stopped(s)) => s.keep,
        Some(pb::renderer_state::Kind::Killed(s)) => s.keep,
        _ => false,
    }
}

/// `processGeneration` — порт одноимённого свойства вендора.
pub fn renderer_process_generation(info: &pb::RendererInstance) -> u64 {
    match info.state.as_ref().and_then(|s| s.kind.as_ref()) {
        Some(pb::renderer_state::Kind::Starting(s)) => s.generation,
        Some(pb::renderer_state::Kind::Running(s)) => s.generation,
        Some(pb::renderer_state::Kind::Stopping(s)) => s.generation,
        _ => 0,
    }
}

/// `lastExitReason` — порт одноимённого свойства вендора.
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

/// `RuntimeCondition` → форма QML — порт `runtimeConditionsFromPb`
/// вендора (`ui/src/proto.cpp`). Неизвестные значения перечислений
/// вырождаются в `issue`/`unknown`, как у оригинала.
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

/// `RendererRuntimeTag` → форма QML — порт `runtimeTagsFromPb` вендора.
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

/// Свойства `RendererListQuery`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererListData {
    pub renderers: Vec<String>,
    pub instances: Vec<RendererInstance>,
}

/// Разобрать ответ `renderer_list`.
///
/// Синхронизация `RendererManager` остаётся за шимом: менеджер держит
/// живые QObject'ы рендереров, их жизненный цикл — Qt-часть. Шим
/// кормит его тем же списком `instances`.
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

// ── RendererPluginListQuery (страница «Статус», детали обои, «Обои») ────

/// `SettingSchema` — схема одной настройки плагина-рендерера.
///
/// ⚠️ Ключи карты в мосту SNAKE_CASE, а не camelCase: `SettingField.qml`
/// написан против `default_value`/`label_key`/`description_key`
/// оригинала (`plugin_query.cpp`), camelCase он бы не прочитал.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SettingSchema {
    pub key: String,
    /// ⚠️ Наружу — под именем `type`: так его читает `SettingField.qml`.
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
    /// Переводимая подпись из каталога плагина (`SettingSchema.label`,
    /// поле 13 протокола). Запасной текст — `label_key`, как у
    /// `pluginMessageFromPb` вендора.
    ///
    /// ⚠️ Без этих трёх полей страница настроек плагина показывала
    /// человеку ТЕХНИЧЕСКИЙ ключ вместо подписи: `SettingField.qml` читает
    /// `schema.label`/`schema.description`, а заголовок группы —
    /// `schema.group_label`. С выпуска v0.3.9 потеря стала заметнее:
    /// вендор перестал печатать неразрешённый ключ как подпись
    /// (`isUnresolvedKey`), и описание пропадало вовсе.
    pub label: PluginText,
    /// Переводимое описание (`SettingSchema.description`, поле 14);
    /// запасной текст — `description_key`.
    pub description: PluginText,
    /// Переводимый заголовок группы (`SettingSchema.group_label`,
    /// поле 15); запасной текст — `group`.
    pub group_label: PluginText,
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
            group: s.group,
            order: s.order,
        }
    }
}

/// `RendererPluginInfo` в виде карты моста.
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

/// Свойства `RendererPluginListQuery`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererPluginListData {
    pub renderers: Vec<RendererPlugin>,
    pub supported_types: Vec<String>,
    /// waywallen 0.4.2: подписи типов обоев `ключ → подпись` — только для
    /// тех ключей, чей рендерер подпись объявил. Нет ключа — страница
    /// показывает сам ключ, как до появления поля.
    pub type_labels: std::collections::BTreeMap<String, PluginText>,
}

/// Разобрать ответ `renderer_plugin_list`.
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
        let renderers: Vec<RendererPlugin> =
            pl.renderers.into_iter().map(RendererPlugin::from).collect();
        assign(&mut data.renderers, renderers, "renderers", &mut out);
        assign(
            &mut data.supported_types,
            pl.supported_types,
            "supportedTypes",
            &mut out,
        );
        // Порт `renderer_query.cpp` v0.4.2: пустой ключ пропускается,
        // подпись — `pluginMessageFromPb(label_text, label)`.
        let type_labels = pl
            .type_labels
            .iter()
            .filter(|t| !t.r#type.is_empty())
            .map(|t| (t.r#type.clone(), plugin_text(t.label_text.as_ref(), &t.label)))
            .collect();
        assign(&mut data.type_labels, type_labels, "typeLabels", &mut out);
    }
    Ok(finish(st, out))
}

// ── Сверка паритета схемы (скрытая страница «Разработчик») ──────────────

/// Свойства сверки паритета: отчёт по последнему ответу
/// `renderer_plugin_list`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParitySchemaData {
    /// Отчёт последней сверки; `None` — сверка ещё не проходила.
    pub report: Option<crate::we_parity::ParityReport>,
}

/// Разобрать ответ `renderer_plugin_list` и СРАЗУ свести схему с реестром
/// UI Rexon.
///
/// Отдельный вход, а не разбор поверх [`apply_renderer_plugin_list`]: для
/// страницы «Разработчик» ценен не список рендереров, а вердикт сверки, и
/// собрать его из уже разложенных по QML-картам данных значило бы завести
/// второй разборщик того же ответа (правило #12).
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

// ── TagListQuery / ContentRatingListQuery (диалог фильтров) ─────────────

/// Свойство-список строк (`tags` у одного запроса, `ratings` у другого).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringListData {
    pub items: Vec<String>,
}

/// Разобрать ответ `tag_list` (свойство `tags`).
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

/// Разобрать ответ `content_rating_list` (свойство `ratings`).
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

// ── Видеокарты (`App.gpuManager`: «Состояние», «Мониторы», настройки) ───

/// Список видеокарт из ответа `gpu_list`.
///
/// `None` — кадр не про видеокарты: не разобрался, это событие, демон
/// отказал или ответил другой нагрузкой (так выглядит и кадр обрыва
/// соединения, которым транспорт закрывает ждущие запросы). Прежний список
/// при этом трогать нельзя — пустой `Some` значит «карт нет», а не «ответа
/// нет».
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

// ── Текущие показы обоев (`App.presentationManager`, карточка рейки) ────

/// Обоины из ответа `wallpaper_lookup`.
///
/// `None` — кадр не про них (не разобрался, событие, отказ демона, другая
/// нагрузка, кадр обрыва соединения): уже известные обоины остаются как
/// есть.
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

/// Идентификаторы обоев снимка показов — по одному разу, в порядке снимка:
/// их спрашивают у демона одним запросом.
pub fn presentation_wallpaper_ids(presentations: &[pb::WallpaperPresentationInfo]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for presentation in presentations {
        if !ids.contains(&presentation.wallpaper_id) {
            ids.push(presentation.wallpaper_id.clone());
        }
    }
    ids
}

/// Подпись целей показа: имена дисплеев и холстов через « · », повторы
/// сняты, порядок — как в снимке.
pub fn presentation_target_summary(labels: &[String]) -> String {
    let mut unique: Vec<&str> = Vec::new();
    for label in labels {
        if !unique.contains(&label.as_str()) {
            unique.push(label);
        }
    }
    unique.join(" · ")
}

/// Какой карте принадлежит узел DRM `major:minor`.
///
/// Номер подходит и узла отрисовки (`renderD128`), и основного (`card1`):
/// рисовальщик несёт первый, а дисплей — узел своего потребителя, и оба
/// ищут карту одним вызовом. `major == 0` — узел неизвестен, карты нет.
pub fn find_gpu(gpus: &[pb::GpuInfo], major: u32, minor: u32) -> Option<usize> {
    if major == 0 {
        return None;
    }
    gpus.iter().position(|g| {
        (g.render_major == major && g.render_minor == minor)
            || (g.primary_major == major && g.primary_minor == minor)
    })
}
