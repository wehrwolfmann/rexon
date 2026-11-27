// waywallen v0.2.6 types: the holder of one result item, cancelling a QR
// code login, editing source settings, source subscriptions (including the
// Steam Workshop) and unsubscribing from a wallpaper.
//
// Reference: waywallen `ui/src/query/{remote,qr_login,
// wallpaper}_query.cpp` @e5626c1 (vendor commit v0.2.6+9).
//
// What the port fixes (rule #16):
//
//   1. `RemoteSettingsPatchQuery`: the original sends `completed(...,
//      accepted = true, "")` RIGHT after `inspect_set` without looking at
//      the answer status — an edit rejected by the daemon was reported to
//      the page as accepted. Here on a refusal `accepted = false` goes with
//      the error text (as our Python bridge did too).
//   2. `WallpaperUnsubscribeQuery`: in the original there is an "is it the
//      same item" check before `unsubscribed` (`m_wallpaper_id !=
//      wallpaper_id → return`), and our Python lost it — when switching
//      cards quickly the details panel got the signal about SOMEONE ELSE's
//      wallpaper.
//
// Warning: there is NO path of our own to Steam here and there must not be.
// `bridge.py` had `_run_bg` branches (synchronous Steam networking from a
// background thread: subscribing with a personal token, reading
// `appworkshop_431960.acf`, a download watchdog), but they were enabled
// only with `WORKSHOP_SERVED_LOCALLY = True`, and the flag is `False` — BY
// Wehrwolfmann's DECISION: "the tab follows upstream closely, the Workshop
// catalogue is served by the daemon's plugin again". So all that code is
// unreachable in the product, and porting it here would mean preserving
// behaviour that was already cancelled. Subscriptions go exactly the same
// way as in the original — an RPC to the daemon. If the decision is ever
// revised, the branch will have to be written anew: there is no Steam
// Workshop client of our own in the tree.

/// cxx-qt bridge: the item holder and four v0.2.6 queries.
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
        /// Holder of ONE result item (`model::RemoteStoreItem`).
        ///
        /// Upstream it is a wrapper over the shared `kstore` storage with a change
        /// subscription; our model returns ready maps, so a value property is
        /// enough here — but a subscription to the shared acquisition state store
        /// must exist, otherwise the "Unsubscribe" button in the details panel
        /// changes only after the page is reloaded.
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

        /// waywallen 0.6.1: cancel a browser sign-in (BrowserLoginDialog).
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, session_id, cxx_name = "sessionId", READ = session_id,
                    WRITE = set_session_id, NOTIFY = session_id_changed)]
        type BrowserLoginCancelQuery = super::BrowserLoginCancelQueryRust;

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
        fn status_changed(self: Pin<&mut BrowserLoginCancelQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut BrowserLoginCancelQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut BrowserLoginCancelQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut BrowserLoginCancelQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut BrowserLoginCancelQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut BrowserLoginCancelQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut BrowserLoginCancelQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut BrowserLoginCancelQuery>, token: i64, field: QString,
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
        #[cxx_name = "sessionIdChanged"]
        fn session_id_changed(self: Pin<&mut BrowserLoginCancelQuery>);

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

        fn status(self: &BrowserLoginCancelQuery) -> i32;
        fn error(self: &BrowserLoginCancelQuery) -> QString;
        fn querying(self: &BrowserLoginCancelQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &BrowserLoginCancelQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut BrowserLoginCancelQuery>, value: bool);

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

        #[cxx_name = "sessionId"]
        fn session_id(self: &BrowserLoginCancelQuery) -> QString;
        #[cxx_name = "setSessionId"]
        fn set_session_id(self: Pin<&mut BrowserLoginCancelQuery>, value: QString);
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

        #[qinvokable]
        fn reload(self: Pin<&mut BrowserLoginCancelQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut BrowserLoginCancelQuery>, token: i64,
                            frame: &QByteArray);

        /// Empty in the original: started only by an explicit `patch()`.
        #[qinvokable]
        fn reload(self: Pin<&mut RemoteSettingsPatchQuery>);
        #[qinvokable]
        fn patch(self: Pin<&mut RemoteSettingsPatchQuery>, source_id: QString,
                 values: QVariant);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RemoteSettingsPatchQuery>, token: i64,
                            frame: &QByteArray);

        /// Empty in the original: started by `refresh()`/`setSubscribed()`.
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

/// The "checking…"/"working…" state of the shared acquisition store.
const ACQ_PENDING: i32 = 3;

macro_rules! on_decode_error {
    ($self:ident) => {{
        let signals = decode_failed(&mut $self.as_mut().rust_mut().base);
        $self.dispatch(signals);
    }};
}

// ════════════════════════════════════════════════════════════════════════
// RemoteStoreItem — one result item for the details panel
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct RemoteStoreItemRust {
    item: Map<String, Value>,
    /// Entry number in the shared acquisition store; 0 means not registered.
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
        // Neither unwrap() nor a preceding is_destroyed(): between the check and
        // the queueing the object may die.
        let id = acquisition::register(Box::new(move |source_id, item_id, state, transfer| {
            let source = source_id.to_string();
            let item = item_id.to_string();
            match handle.queue(move |object| {
                object.apply_acquisition(&source, &item, state, transfer);
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

    /// The store reported an acquisition state. Whether it is our item and
    /// whether the value really changed is decided here (a port of
    /// `_on_acquisition`).
    ///
    /// waywallen 0.6.0: together with the state comes the transfer progress —
    /// the details panel shows the received bytes (`downloadReceivedBytes` and
    /// others).
    pub fn apply_acquisition(
        mut self: Pin<&mut Self>,
        source_id: &str,
        item_id: &str,
        state: i32,
        transfer: acquisition::Transfer,
    ) {
        let same = self.item.get("sourceId").and_then(Value::as_str) == Some(source_id)
            && self.item.get("itemId").and_then(Value::as_str) == Some(item_id);
        if !same {
            return;
        }
        let next = [
            ("acquisitionState", Value::from(state)),
            ("downloadTransferActive", Value::Bool(transfer.active)),
            ("downloadReceivedBytes", Value::from(transfer.received)),
            ("downloadTotalBytes", Value::from(transfer.total)),
        ];
        let changed = next.iter().any(|(key, value)| match self.item.get(*key) {
            // Numbers from QML arrive fractional — compare them as numbers.
            Some(Value::Number(have)) => have.as_f64() != value.as_f64(),
            Some(have) => have != value,
            // The previous row has no transfer progress key — an empty progress is no
            // news.
            None => *key == "acquisitionState" || value != &Value::Bool(false) && value != &Value::from(0),
        });
        if !changed {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            for (key, value) in next {
                rust.item.insert(key.to_string(), value);
            }
        }
        self.item_changed();
    }
}

// ════════════════════════════════════════════════════════════════════════
// QrLoginCancelQuery — cancelling a QR code login
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
// BrowserLoginCancelQuery — cancel a browser sign-in (waywallen 0.6.1)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct BrowserLoginCancelQueryRust {
    base: QueryState,
    session_id: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(BrowserLoginCancelQuery);

impl qobject::BrowserLoginCancelQuery {
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
        let payload = pb::BrowserLoginCancelRequest {
            session_id: self.session_id.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("browser_login_cancel", payload) {
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
// RemoteSettingsPatchQuery — a targeted edit of source settings
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct RemoteSettingsPatchQueryRust {
    base: QueryState,
    result: we_manage::PatchResult,
    /// token → what exactly was sent: the page gets it back in `completed` to
    /// understand whose edit was confirmed.
    pending: std::collections::BTreeMap<i64, (String, Map<String, Value>)>,
    /// Arguments of the current `completed`.
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
        // Original: an empty source or an empty map means return without a
        // request.
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
        // No connection: the page counts the edits as "in flight" and without
        // `completed` would stay in the saving state forever.
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
// RemoteSubscriptionQuery — subscription state and toggling it
// ════════════════════════════════════════════════════════════════════════

/// What we are waiting for on this token.
enum Await {
    /// `subscription_status`: source, item, generation number.
    Status(String, String, u64),
    /// `subscription_set`: source, item, what it was toggled to.
    Set(String, String, bool),
}

#[derive(Default)]
pub struct RemoteSubscriptionQueryRust {
    base: QueryState,
    /// A port of `m_refresh_generation`: a late answer to an earlier request
    /// must not override a state requested later.
    generation: u64,
    pending: std::collections::BTreeMap<i64, Await>,
    /// Arguments of the signal that is about to go out.
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
        // Shared store: "checking…" — before sending, as in the original.
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
        // The frame did not go out: in the original this is the `!result` branch —
        // state 0 and `stateLoaded` with the error text.
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
        // "Working…" — before sending.
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
                // The answer to a stale request touches nothing, not even the store.
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
// WallpaperUnsubscribeQuery — unsubscribing from a wallpaper in the details panel
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperUnsubscribeQueryRust {
    base: QueryState,
    wallpaper_id: String,
    /// token → which wallpaper was unsubscribed with it. The original compares
    /// it with the current one and silently returns on a mismatch; our Python
    /// lost this check.
    pending: std::collections::BTreeMap<i64, String>,
    /// Argument of the `unsubscribed` signal.
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
        // Warning: protection against a stale answer: while it was on its way, the
        // panel may have switched to another wallpaper.
        if wanted != self.wallpaper_id {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            we_query::apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(mut signals) => {
                // `unsubscribed` only on success: apply_ack brings only an accepted
                // command to Finished.
                if self.base.status == we_query::STATUS_FINISHED {
                    self.as_mut().rust_mut().unsubscribed_id = wanted;
                    // Before the status, as in the original: the signal is sent by the parsing
                    // lambda, and Finished is set by it right after.
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
