// Типы waywallen v0.2.6: держатель одного элемента выдачи, отмена входа по
// QR-коду, правка настроек источника, подписки источников (в том числе
// Мастерской Steam) и отписка от обои.
//
// Эталон — waywallen `ui/src/query/{remote,qr_login,
// wallpaper}_query.cpp` @e5626c1 (вендорный коммит v0.2.6+9).
//
// Что порт чинит (правило #16):
//
//   1. `RemoteSettingsPatchQuery`: оригинал шлёт `completed(..., accepted =
//      true, "")` СРАЗУ после `inspect_set`, не глядя на статус ответа —
//      отклонённая демоном правка докладывалась странице как принятая.
//      Здесь при отказе идёт `accepted = false` с текстом ошибки (так же
//      делал наш Python-мост).
//   2. `WallpaperUnsubscribeQuery`: у оригинала перед `unsubscribed` стоит
//      проверка «тот ли предмет» (`m_wallpaper_id != wallpaper_id →
//      выход`), а наш Python её растерял — при быстром переключении
//      карточек панель деталей получала сигнал про ЧУЖУЮ обоину.
//
// ⚠️ Своего пути в Steam здесь НЕТ и не должно быть. `bridge.py` держал
// ветки `_run_bg` (синхронная сеть Steam из фонового потока: подписка
// личным токеном, чтение `appworkshop_431960.acf`, сторож загрузки), но
// включались они только при `WORKSHOP_SERVED_LOCALLY = True`, а флаг стоит
// в `False` — РЕШЕНИЕМ Wehrwolfmann: «вкладка идёт вплотную за апстримом,
// каталог Мастерской снова отдаёт плагин демона». То есть весь этот код в
// продукте недостижим, и переносить его сюда значило бы законсервировать
// уже отменённое поведение. Подписки идут ровно тем же путём, что у
// оригинала, — RPC демону. Если решение когда-нибудь пересмотрят, ветку
// придётся писать заново: своего клиента Мастерской Steam в дереве нет.

/// Мост cxx-qt: держатель элемента и четыре запроса v0.2.6.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        /// Держатель ОДНОГО элемента выдачи (`model::RemoteStoreItem`).
        ///
        /// У апстрима это обёртка над общим хранилищем `kstore` с подпиской
        /// на изменения; у нас модель отдаёт готовые карты, поэтому здесь
        /// достаточно свойства-значения — но подписка на общий склад
        /// состояний приобретения обязана быть, иначе кнопка «Отписаться»
        /// в панели деталей меняется только после перезагрузки страницы.
        #[qobject]
        #[qml_element]
        #[qproperty(QVariant, item, READ = item, WRITE = set_item, NOTIFY = item_changed)]
        type RemoteStoreItem = super::RemoteStoreItemRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, session_id, cxx_name = "sessionId", READ = session_id,
                    WRITE = set_session_id, NOTIFY = session_id_changed)]
        type QrLoginCancelQuery = super::QrLoginCancelQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        type RemoteSettingsPatchQuery = super::RemoteSettingsPatchQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        type RemoteSubscriptionQuery = super::RemoteSubscriptionQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = wallpaper_id_changed)]
        type WallpaperUnsubscribeQuery = super::WallpaperUnsubscribeQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "itemChanged"]
        fn item_changed(self: Pin<&mut RemoteStoreItem>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut QrLoginCancelQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut QrLoginCancelQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut QrLoginCancelQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut QrLoginCancelQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut QrLoginCancelQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut QrLoginCancelQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut QrLoginCancelQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut QrLoginCancelQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RemoteSettingsPatchQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RemoteSettingsPatchQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RemoteSettingsPatchQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RemoteSettingsPatchQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RemoteSettingsPatchQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RemoteSettingsPatchQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RemoteSettingsPatchQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut RemoteSettingsPatchQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RemoteSubscriptionQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RemoteSubscriptionQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RemoteSubscriptionQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RemoteSubscriptionQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RemoteSubscriptionQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RemoteSubscriptionQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RemoteSubscriptionQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut RemoteSubscriptionQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperUnsubscribeQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperUnsubscribeQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperUnsubscribeQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperUnsubscribeQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperUnsubscribeQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperUnsubscribeQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperUnsubscribeQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperUnsubscribeQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "sessionIdChanged"]
        fn session_id_changed(self: Pin<&mut QrLoginCancelQuery>);

        #[qsignal]
        fn completed(self: Pin<&mut RemoteSettingsPatchQuery>, source_id: QString,
                     values: QVariant, accepted: bool, error: QString);

        #[qsignal]
        #[cxx_name = "stateLoaded"]
        fn state_loaded(self: Pin<&mut RemoteSubscriptionQuery>, source_id: QString,
                        id: QString, state: i32, error: QString);
        #[qsignal]
        #[cxx_name = "setFinished"]
        fn set_finished(self: Pin<&mut RemoteSubscriptionQuery>, source_id: QString,
                        id: QString, subscribed: bool, accepted: bool, error: QString);

        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperUnsubscribeQuery>);
        #[qsignal]
        fn unsubscribed(self: Pin<&mut WallpaperUnsubscribeQuery>, wallpaper_id: QString);
    }

    extern "RustQt" {
        fn item(self: &RemoteStoreItem) -> QVariant;
        #[cxx_name = "setItem"]
        fn set_item(self: Pin<&mut RemoteStoreItem>, value: QVariant);

        fn status(self: &QrLoginCancelQuery) -> i32;
        fn error(self: &QrLoginCancelQuery) -> QString;
        fn querying(self: &QrLoginCancelQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &QrLoginCancelQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut QrLoginCancelQuery>, value: bool);

        fn status(self: &RemoteSettingsPatchQuery) -> i32;
        fn error(self: &RemoteSettingsPatchQuery) -> QString;
        fn querying(self: &RemoteSettingsPatchQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RemoteSettingsPatchQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RemoteSettingsPatchQuery>, value: bool);

        fn status(self: &RemoteSubscriptionQuery) -> i32;
        fn error(self: &RemoteSubscriptionQuery) -> QString;
        fn querying(self: &RemoteSubscriptionQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RemoteSubscriptionQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RemoteSubscriptionQuery>, value: bool);

        fn status(self: &WallpaperUnsubscribeQuery) -> i32;
        fn error(self: &WallpaperUnsubscribeQuery) -> QString;
        fn querying(self: &WallpaperUnsubscribeQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperUnsubscribeQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperUnsubscribeQuery>, value: bool);

        #[cxx_name = "sessionId"]
        fn session_id(self: &QrLoginCancelQuery) -> QString;
        #[cxx_name = "setSessionId"]
        fn set_session_id(self: Pin<&mut QrLoginCancelQuery>, value: QString);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperUnsubscribeQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperUnsubscribeQuery>, value: QString);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut QrLoginCancelQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut QrLoginCancelQuery>, token: i64,
                            frame: &QByteArray);

        /// У оригинала пустой: запускается только явным `patch()`.
        #[qinvokable]
        fn reload(self: Pin<&mut RemoteSettingsPatchQuery>);
        #[qinvokable]
        fn patch(self: Pin<&mut RemoteSettingsPatchQuery>, source_id: QString,
                 values: QVariant);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RemoteSettingsPatchQuery>, token: i64,
                            frame: &QByteArray);

        /// У оригинала пустой: запускается `refresh()`/`setSubscribed()`.
        #[qinvokable]
        fn reload(self: Pin<&mut RemoteSubscriptionQuery>);
        #[qinvokable]
        fn refresh(self: Pin<&mut RemoteSubscriptionQuery>, source_id: QString, id: QString);
        #[qinvokable]
        #[cxx_name = "setSubscribed"]
        fn set_subscribed(self: Pin<&mut RemoteSubscriptionQuery>, source_id: QString,
                          id: QString, subscribed: bool);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RemoteSubscriptionQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperUnsubscribeQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperUnsubscribeQuery>, token: i64,
                            frame: &QByteArray);
    }

    impl cxx_qt::Threading for RemoteStoreItem {}
    impl cxx_qt::Constructor<()> for RemoteStoreItem {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QByteArray, QString, QVariant};
use prost::Message;
use serde_json::{Map, Value};

use rexon_core::proto::control as pb;
use rexon_core::we_manage;
use rexon_core::we_query::{self, QuerySignal, QueryState};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::{json_to_variant, variant_to_json};
use crate::remote_list_model::acquisition;

/// Состояние «уточняем…»/«выполняем…» общего склада приобретения.
const ACQ_PENDING: i32 = 3;

macro_rules! on_decode_error {
    ($self:ident) => {{
        let signals = decode_failed(&mut $self.as_mut().rust_mut().base);
        $self.dispatch(signals);
    }};
}

// ════════════════════════════════════════════════════════════════════════
// RemoteStoreItem — один элемент выдачи для панели деталей
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct RemoteStoreItemRust {
    item: Map<String, Value>,
    /// Номер записи в общем складе приобретения; 0 — не зарегистрирован.
    store_id: u64,
}

impl Drop for RemoteStoreItemRust {
    fn drop(&mut self) {
        if self.store_id != 0 {
            acquisition::unregister(self.store_id);
        }
    }
}

impl cxx_qt::Initialize for qobject::RemoteStoreItem {
    fn initialize(mut self: Pin<&mut Self>) {
        let handle = self.as_ref().get_ref().qt_thread();
        // Ни unwrap(), ни предварительного is_destroyed(): между проверкой
        // и постановкой объект успевает умереть.
        let id = acquisition::register(Box::new(move |source_id, item_id, state| {
            let source = source_id.to_string();
            let item = item_id.to_string();
            match handle.queue(move |object| {
                object.apply_acquisition(&source, &item, state);
            }) {
                Ok(()) => Ok(()),
                Err(ThreadingQueueError::ObjectDestroyed) => Err(()),
                Err(_) => Ok(()),
            }
        }));
        self.as_mut().rust_mut().store_id = id;
    }
}

impl qobject::RemoteStoreItem {
    pub fn item(&self) -> QVariant {
        json_to_variant(&Value::Object(self.item.clone()))
    }

    pub fn set_item(mut self: Pin<&mut Self>, value: QVariant) {
        let next = match variant_to_json(&value) {
            Value::Object(map) => map,
            _ => we_manage::empty_map(),
        };
        if self.item == next {
            return;
        }
        self.as_mut().rust_mut().item = next;
        self.item_changed();
    }

    /// Склад сообщил состояние приобретения. Наш ли это предмет и правда ли
    /// значение изменилось — решается здесь (порт `_on_acquisition`).
    pub fn apply_acquisition(mut self: Pin<&mut Self>, source_id: &str, item_id: &str, state: i32) {
        let same = self.item.get("sourceId").and_then(Value::as_str) == Some(source_id)
            && self.item.get("itemId").and_then(Value::as_str) == Some(item_id);
        if !same {
            return;
        }
        if self.item.get("acquisitionState").and_then(Value::as_i64) == Some(state as i64) {
            return;
        }
        self.as_mut()
            .rust_mut()
            .item
            .insert("acquisitionState".to_string(), Value::from(state));
        self.item_changed();
    }
}

// ════════════════════════════════════════════════════════════════════════
// QrLoginCancelQuery — отмена входа по QR-коду
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct QrLoginCancelQueryRust {
    base: QueryState,
    session_id: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(QrLoginCancelQuery);

impl qobject::QrLoginCancelQuery {
    pub fn session_id(&self) -> QString {
        QString::from(&self.session_id)
    }

    pub fn set_session_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.session_id == next {
            return;
        }
        self.as_mut().rust_mut().session_id = next;
        self.session_id_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.session_id.is_empty() {
            return;
        }
        let payload = pb::QrLoginCancelRequest {
            session_id: self.session_id.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("qr_login_cancel", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            we_query::apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// RemoteSettingsPatchQuery — точечная правка настроек источника
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct RemoteSettingsPatchQueryRust {
    base: QueryState,
    result: we_manage::PatchResult,
    /// token → что именно отправляли: страница получает это обратно в
    /// `completed`, чтобы понять, чью правку подтвердили.
    pending: std::collections::BTreeMap<i64, (String, Map<String, Value>)>,
    /// Аргументы текущего `completed`.
    completed_args: (String, Map<String, Value>),
}

impl_query_base!(RemoteSettingsPatchQuery);

impl qobject::RemoteSettingsPatchQuery {
    pub fn reload(self: Pin<&mut Self>) {
        let _ = self;
    }

    pub fn patch(mut self: Pin<&mut Self>, source_id: QString, values: QVariant) {
        let source = source_id.to_string();
        let sent = match variant_to_json(&values) {
            Value::Object(map) => map,
            _ => we_manage::empty_map(),
        };
        // Оригинал: пустой источник или пустая карта — выход без запроса.
        if source.is_empty() || sent.is_empty() {
            return;
        }
        let payload = pb::RemoteSettingsPatchRequest {
            source_id: source.clone(),
            values: we_manage::values_to_wire(&Value::Object(sent.clone())),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("remote_settings_patch", payload) {
            self.as_mut()
                .rust_mut()
                .pending
                .insert(token, (source, sent));
            return;
        }
        // Соединения нет: страница считает правки «в полёте» и без
        // `completed` навсегда останется в состоянии сохранения.
        let error = self.base.error.clone();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.result = we_manage::PatchResult {
                accepted: false,
                error,
            };
            rust.completed_args = (source_id.to_string(), sent);
        }
        self.dispatch(vec![QuerySignal::Property("completed")]);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(args) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        self.as_mut().rust_mut().completed_args = args;
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_settings_patch(&mut rust.base, &mut rust.result, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "completed" => {
                let source = QString::from(&self.completed_args.0);
                let values = json_to_variant(&Value::Object(self.completed_args.1.clone()));
                let accepted = self.result.accepted;
                let error = QString::from(&self.result.error);
                self.as_mut().completed(source, values, accepted, error);
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// RemoteSubscriptionQuery — состояние подписки и её переключение
// ════════════════════════════════════════════════════════════════════════

/// Чего ждём по этому токену.
enum Await {
    /// `subscription_status`: источник, предмет, номер поколения.
    Status(String, String, u64),
    /// `subscription_set`: источник, предмет, куда переключали.
    Set(String, String, bool),
}

#[derive(Default)]
pub struct RemoteSubscriptionQueryRust {
    base: QueryState,
    /// Порт `m_refresh_generation`: поздний ответ прошлого запроса не
    /// должен перебивать состояние, запрошенное позже.
    generation: u64,
    pending: std::collections::BTreeMap<i64, Await>,
    /// Аргументы сигнала, который вот-вот уйдёт.
    state_loaded: (String, String, i32, String),
    set_finished: (String, String, bool, bool, String),
}

impl_query_base!(RemoteSubscriptionQuery);

impl qobject::RemoteSubscriptionQuery {
    pub fn reload(self: Pin<&mut Self>) {
        let _ = self;
    }

    pub fn refresh(mut self: Pin<&mut Self>, source_id: QString, id: QString) {
        let source = source_id.to_string();
        let item = id.to_string();
        if source.is_empty() || item.is_empty() {
            return;
        }
        // Общий склад: «уточняем…» — до отправки, как у оригинала.
        acquisition::set(&source, &item, ACQ_PENDING);
        let generation = {
            let mut rust = self.as_mut().rust_mut();
            rust.generation += 1;
            rust.generation
        };
        let payload = pb::SubscriptionStatusRequest {
            source_id: source.clone(),
            item_ids: vec![item.clone()],
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("subscription_status", payload) {
            self.as_mut()
                .rust_mut()
                .pending
                .insert(token, Await::Status(source, item, generation));
            return;
        }
        // Кадр не ушёл: у оригинала это ветка `!result` — состояние 0 и
        // `stateLoaded` с текстом ошибки.
        let error = self.base.error.clone();
        acquisition::set(&source, &item, 0);
        self.as_mut().rust_mut().state_loaded = (source, item, 0, error);
        self.dispatch(vec![QuerySignal::Property("stateLoaded")]);
    }

    pub fn set_subscribed(
        mut self: Pin<&mut Self>,
        source_id: QString,
        id: QString,
        subscribed: bool,
    ) {
        let source = source_id.to_string();
        let item = id.to_string();
        if source.is_empty() || item.is_empty() {
            return;
        }
        // «Выполняем…» — до отправки.
        acquisition::set(&source, &item, ACQ_PENDING);
        let payload = pb::SubscriptionSetRequest {
            source_id: source.clone(),
            item_id: item.clone(),
            subscribed,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("subscription_set", payload) {
            self.as_mut()
                .rust_mut()
                .pending
                .insert(token, Await::Set(source, item, subscribed));
            return;
        }
        let error = self.base.error.clone();
        let state = if subscribed { 1 } else { 2 };
        acquisition::set(&source, &item, state);
        self.as_mut().rust_mut().set_finished = (source, item, subscribed, false, error);
        self.dispatch(vec![QuerySignal::Property("setFinished")]);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(awaited) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        match awaited {
            Await::Status(source, item, generation) => {
                // Ответ устаревшего запроса не трогает ничего, даже склад.
                if generation != self.generation {
                    return;
                }
                let parsed = {
                    let mut rust = self.as_mut().rust_mut();
                    we_manage::apply_subscription_status(&mut rust.base, &item, frame.as_slice())
                };
                match parsed {
                    Ok((mut signals, result)) => {
                        acquisition::set(&source, &item, result.state);
                        self.as_mut().rust_mut().state_loaded =
                            (source, item, result.state, result.error);
                        signals.push(QuerySignal::Property("stateLoaded"));
                        self.dispatch(signals);
                    }
                    Err(_) => on_decode_error!(self),
                }
            }
            Await::Set(source, item, subscribed) => {
                let parsed = {
                    let mut rust = self.as_mut().rust_mut();
                    we_manage::apply_subscription_set(&mut rust.base, subscribed, frame.as_slice())
                };
                match parsed {
                    Ok((mut signals, result)) => {
                        acquisition::set(&source, &item, result.state);
                        self.as_mut().rust_mut().set_finished = (
                            source,
                            item,
                            subscribed,
                            result.accepted,
                            result.error,
                        );
                        signals.push(QuerySignal::Property("setFinished"));
                        self.dispatch(signals);
                    }
                    Err(_) => on_decode_error!(self),
                }
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "stateLoaded" => {
                let (source, item, state, error) = self.state_loaded.clone();
                self.as_mut().state_loaded(
                    QString::from(&source),
                    QString::from(&item),
                    state,
                    QString::from(&error),
                );
            }
            "setFinished" => {
                let (source, item, subscribed, accepted, error) = self.set_finished.clone();
                self.as_mut().set_finished(
                    QString::from(&source),
                    QString::from(&item),
                    subscribed,
                    accepted,
                    QString::from(&error),
                );
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperUnsubscribeQuery — отписка от обои из панели деталей
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperUnsubscribeQueryRust {
    base: QueryState,
    wallpaper_id: String,
    /// token → какую обоину им отписывали. Оригинал сверяет её с текущей и
    /// молча выходит при расхождении; наш Python эту проверку растерял.
    pending: std::collections::BTreeMap<i64, String>,
    /// Аргумент сигнала `unsubscribed`.
    unsubscribed_id: String,
}

impl_query_base!(WallpaperUnsubscribeQuery);

impl qobject::WallpaperUnsubscribeQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    pub fn set_wallpaper_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wallpaper_id == next {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = next;
        self.wallpaper_id_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.wallpaper_id.is_empty() {
            return;
        }
        let wanted = self.wallpaper_id.clone();
        let payload = pb::WallpaperUnsubscribeRequest {
            wallpaper_id: wanted.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("wallpaper_unsubscribe", payload) {
            self.rust_mut().pending.insert(token, wanted);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(wanted) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        // ⚠️ Защита от устаревшего ответа: пока он шёл, панель могла
        // переключиться на другую обоину.
        if wanted != self.wallpaper_id {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            we_query::apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(mut signals) => {
                // `unsubscribed` только на успехе: apply_ack доводит до
                // Finished лишь принятую команду.
                if self.base.status == we_query::STATUS_FINISHED {
                    self.as_mut().rust_mut().unsubscribed_id = wanted;
                    // Перед статусом, как у оригинала: сигнал шлёт лямбда
                    // разбора, а Finished выставляется ею же следом.
                    signals.insert(0, QuerySignal::Property("unsubscribed"));
                }
                self.dispatch(signals);
            }
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "unsubscribed" => {
                let id = QString::from(&self.unsubscribed_id);
                self.as_mut().unsubscribed(id);
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
