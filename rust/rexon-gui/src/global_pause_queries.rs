// Глобальные переключатели демона: пауза, звук, полная остановка.
//
// Первая
// порция того, что вендор добавил в v0.3.5 и чего у нас не было ВООБЩЕ:
// эти четыре типа не регистрировал ни C++-слой, ни Python-мост, и
// `Window.qml` со `StatusPage.qml` падали на «is not a type».
//
// Эталон — `ui/src/query/global_pause_query.cpp` вендора на теге v0.3.5
// (f7baecd), тем же тегом вендорен наш QML. Разбор ответа — в ядре
// (`rexon_core::we_manage::apply_global_flag`), здесь только Qt-сторона.
//
// Три «Set»-запроса и «Toggle» отличаются ровно двумя вещами: именем поля
// протокола и тем, что Toggle не имеет сеттера (демон сам решает, во что
// переключиться) и всегда шлёт сигнал `toggled`. Всё остальное общее и
// печатается одним макросом ниже — правило #12, движок один.

/// Мост cxx-qt: четыре глобальных переключателя.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        // Только чтение: значение называет демон в ответе.
        #[qproperty(bool, paused, READ = paused, NOTIFY = paused_changed)]
        type GlobalPauseToggleQuery = super::GlobalPauseToggleQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(bool, paused, READ = paused, WRITE = set_paused,
                    NOTIFY = paused_changed)]
        type GlobalPauseSetQuery = super::GlobalPauseSetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(bool, muted, READ = muted, WRITE = set_muted, NOTIFY = muted_changed)]
        type GlobalMuteSetQuery = super::GlobalMuteSetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(bool, stopped, READ = stopped, WRITE = set_stopped,
                    NOTIFY = stopped_changed)]
        type GlobalStopSetQuery = super::GlobalStopSetQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut GlobalPauseToggleQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut GlobalPauseToggleQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut GlobalPauseToggleQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut GlobalPauseToggleQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut GlobalPauseToggleQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut GlobalPauseToggleQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut GlobalPauseToggleQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut GlobalPauseToggleQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "pausedChanged"]
        fn paused_changed(self: Pin<&mut GlobalPauseToggleQuery>);
        #[qsignal]
        fn toggled(self: Pin<&mut GlobalPauseToggleQuery>, paused: bool);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut GlobalPauseSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut GlobalPauseSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut GlobalPauseSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut GlobalPauseSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut GlobalPauseSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut GlobalPauseSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut GlobalPauseSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut GlobalPauseSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "pausedChanged"]
        fn paused_changed(self: Pin<&mut GlobalPauseSetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut GlobalMuteSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut GlobalMuteSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut GlobalMuteSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut GlobalMuteSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut GlobalMuteSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut GlobalMuteSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut GlobalMuteSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut GlobalMuteSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "mutedChanged"]
        fn muted_changed(self: Pin<&mut GlobalMuteSetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut GlobalStopSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut GlobalStopSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut GlobalStopSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut GlobalStopSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut GlobalStopSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut GlobalStopSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut GlobalStopSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut GlobalStopSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "stoppedChanged"]
        fn stopped_changed(self: Pin<&mut GlobalStopSetQuery>);
    }

    extern "RustQt" {
        fn status(self: &GlobalPauseToggleQuery) -> i32;
        fn error(self: &GlobalPauseToggleQuery) -> QString;
        fn querying(self: &GlobalPauseToggleQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &GlobalPauseToggleQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut GlobalPauseToggleQuery>, value: bool);

        fn status(self: &GlobalPauseSetQuery) -> i32;
        fn error(self: &GlobalPauseSetQuery) -> QString;
        fn querying(self: &GlobalPauseSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &GlobalPauseSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut GlobalPauseSetQuery>, value: bool);

        fn status(self: &GlobalMuteSetQuery) -> i32;
        fn error(self: &GlobalMuteSetQuery) -> QString;
        fn querying(self: &GlobalMuteSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &GlobalMuteSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut GlobalMuteSetQuery>, value: bool);

        fn status(self: &GlobalStopSetQuery) -> i32;
        fn error(self: &GlobalStopSetQuery) -> QString;
        fn querying(self: &GlobalStopSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &GlobalStopSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut GlobalStopSetQuery>, value: bool);

        fn paused(self: &GlobalPauseToggleQuery) -> bool;
        fn paused(self: &GlobalPauseSetQuery) -> bool;
        #[cxx_name = "setPaused"]
        fn set_paused(self: Pin<&mut GlobalPauseSetQuery>, value: bool);
        fn muted(self: &GlobalMuteSetQuery) -> bool;
        #[cxx_name = "setMuted"]
        fn set_muted(self: Pin<&mut GlobalMuteSetQuery>, value: bool);
        fn stopped(self: &GlobalStopSetQuery) -> bool;
        #[cxx_name = "setStopped"]
        fn set_stopped(self: Pin<&mut GlobalStopSetQuery>, value: bool);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut GlobalPauseToggleQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut GlobalPauseToggleQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut GlobalPauseSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut GlobalPauseSetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut GlobalMuteSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut GlobalMuteSetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut GlobalStopSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut GlobalStopSetQuery>, token: i64,
                            frame: &QByteArray);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::QByteArray;
use prost::Message;

use rexon_core::proto::control as pb;
use rexon_core::we_manage::{apply_global_flag, GlobalFlag};
use rexon_core::we_query::{QuerySignal, QueryState};

use crate::impl_query_base;
use crate::query_base::decode_failed;

/// Общая часть всех четырёх: состояние базы, один булев флаг и журнал
/// отправленных кадров.
macro_rules! global_flag_query {
    (
        $query:ident,
        rust = $rust:ident,
        flag = $flag:expr,
        field = $field:literal,
        property = $property:literal,
        getter = $getter:ident,
        changed = $changed:ident,
    ) => {
        #[derive(Default)]
        pub struct $rust {
            base: QueryState,
            value: bool,
            pending: std::collections::BTreeSet<i64>,
        }

        impl_query_base!($query);

        impl qobject::$query {
            pub fn $getter(&self) -> bool {
                self.value
            }

            pub fn reload(mut self: Pin<&mut Self>) {
                let payload = $crate::global_pause_queries::payload($flag, self.value);
                if let Some(token) = self.as_mut().send_frame($field, payload) {
                    self.rust_mut().pending.insert(token);
                }
            }

            pub fn deliver_response(
                mut self: Pin<&mut Self>,
                token: i64,
                frame: &QByteArray,
            ) {
                if !self.as_mut().rust_mut().pending.remove(&token) {
                    return;
                }
                let parsed = {
                    let mut rust = self.as_mut().rust_mut();
                    let rust = &mut *rust;
                    apply_global_flag(
                        &mut rust.base,
                        $flag,
                        &mut rust.value,
                        $property,
                        frame.as_slice(),
                    )
                };
                match parsed {
                    Ok((signals, answered)) => {
                        self.as_mut().dispatch(signals);
                        self.on_answer(answered);
                    }
                    Err(_) => {
                        let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                        self.dispatch(signals);
                    }
                }
            }

            pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
                match name {
                    $property => self.as_mut().$changed(),
                    other => debug_assert!(false, "нет сигнала для свойства {other}"),
                }
            }

            pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
                self.dispatch_base(signals);
            }
        }
    };
}

/// Кадр запроса. Toggle нагрузки не несёт вовсе, остальные — один булев
/// флаг, который клиент ХОЧЕТ выставить.
fn payload(flag: GlobalFlag, value: bool) -> Vec<u8> {
    match flag {
        GlobalFlag::PauseToggle => pb::GlobalPauseToggleRequest {}.encode_to_vec(),
        GlobalFlag::PauseSet => pb::GlobalPauseSetRequest { paused: value }.encode_to_vec(),
        GlobalFlag::MuteSet => pb::GlobalMuteSetRequest { muted: value }.encode_to_vec(),
        GlobalFlag::StopSet => pb::GlobalStopSetRequest { stopped: value }.encode_to_vec(),
    }
}

/// Сеттер свойства — общий у трёх «Set»-запросов. У Toggle сеттера нет:
/// значение называет демон.
macro_rules! global_flag_setter {
    ($query:ident, $setter:ident, $changed:ident) => {
        impl qobject::$query {
            pub fn $setter(mut self: Pin<&mut Self>, value: bool) {
                if self.value == value {
                    return;
                }
                self.as_mut().rust_mut().value = value;
                self.$changed();
            }
        }
    };
}

global_flag_query!(
    GlobalPauseToggleQuery,
    rust = GlobalPauseToggleQueryRust,
    flag = GlobalFlag::PauseToggle,
    field = "global_pause_toggle",
    property = "paused",
    getter = paused,
    changed = paused_changed,
);

global_flag_query!(
    GlobalPauseSetQuery,
    rust = GlobalPauseSetQueryRust,
    flag = GlobalFlag::PauseSet,
    field = "global_pause_set",
    property = "paused",
    getter = paused,
    changed = paused_changed,
);
global_flag_setter!(GlobalPauseSetQuery, set_paused, paused_changed);

global_flag_query!(
    GlobalMuteSetQuery,
    rust = GlobalMuteSetQueryRust,
    flag = GlobalFlag::MuteSet,
    field = "global_mute_set",
    property = "muted",
    getter = muted,
    changed = muted_changed,
);
global_flag_setter!(GlobalMuteSetQuery, set_muted, muted_changed);

global_flag_query!(
    GlobalStopSetQuery,
    rust = GlobalStopSetQueryRust,
    flag = GlobalFlag::StopSet,
    field = "global_stop_set",
    property = "stopped",
    getter = stopped,
    changed = stopped_changed,
);
global_flag_setter!(GlobalStopSetQuery, set_stopped, stopped_changed);

/// Хвост ответа: у трёх «Set»-запросов его нет, у Toggle — сигнал
/// `toggled(paused)` на каждом успешном ответе демона.
trait OnAnswer {
    fn on_answer(self: Pin<&mut Self>, answered: Option<bool>);
}

impl OnAnswer for qobject::GlobalPauseToggleQuery {
    fn on_answer(self: Pin<&mut Self>, answered: Option<bool>) {
        if let Some(paused) = answered {
            self.toggled(paused);
        }
    }
}

impl OnAnswer for qobject::GlobalPauseSetQuery {
    fn on_answer(self: Pin<&mut Self>, _answered: Option<bool>) {}
}

impl OnAnswer for qobject::GlobalMuteSetQuery {
    fn on_answer(self: Pin<&mut Self>, _answered: Option<bool>) {}
}

impl OnAnswer for qobject::GlobalStopSetQuery {
    fn on_answer(self: Pin<&mut Self>, _answered: Option<bool>) {}
}
