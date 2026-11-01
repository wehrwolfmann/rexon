// Списки тегов и возрастных рейтингов библиотеки — диалог фильтров обоев.
//
// Эталон — waywallen `ui/src/query/tag_query.cpp` @e5626c1.
// Разбор ответов уже был в ядре (`we_query::apply_tag_list`/
// `apply_content_rating_list`), здесь только оболочка.

/// Мост cxx-qt: два запроса-списка строк.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

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
        #[qproperty(QStringList, tags, READ = tags, NOTIFY = tags_changed)]
        type TagListQuery = super::TagListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QStringList, ratings, READ = ratings, NOTIFY = ratings_changed)]
        type ContentRatingListQuery = super::ContentRatingListQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut TagListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut TagListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut TagListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut TagListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut TagListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut TagListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut TagListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut TagListQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "tagsChanged"]
        fn tags_changed(self: Pin<&mut TagListQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut ContentRatingListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut ContentRatingListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut ContentRatingListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut ContentRatingListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut ContentRatingListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut ContentRatingListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut ContentRatingListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut ContentRatingListQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "ratingsChanged"]
        fn ratings_changed(self: Pin<&mut ContentRatingListQuery>);
    }

    extern "RustQt" {
        fn status(self: &TagListQuery) -> i32;
        fn error(self: &TagListQuery) -> QString;
        fn querying(self: &TagListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &TagListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut TagListQuery>, value: bool);
        fn tags(self: &TagListQuery) -> QStringList;

        fn status(self: &ContentRatingListQuery) -> i32;
        fn error(self: &ContentRatingListQuery) -> QString;
        fn querying(self: &ContentRatingListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &ContentRatingListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut ContentRatingListQuery>, value: bool);
        fn ratings(self: &ContentRatingListQuery) -> QStringList;
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut TagListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut TagListQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut ContentRatingListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut ContentRatingListQuery>, token: i64,
                            frame: &QByteArray);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QStringList};
use prost::Message;

use rexon_core::proto::control as pb;
use rexon_core::we_query::{self, QuerySignal, QueryState, StringListData};

use crate::impl_query_base;
use crate::query_base::decode_failed;

/// Список строк ядра → `QStringList` для QML.
fn to_qstringlist(items: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for item in items {
        list.append(QString::from(item));
    }
    list
}

// ════════════════════════════════════════════════════════════════════════
// TagListQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct TagListQueryRust {
    base: QueryState,
    data: StringListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(TagListQuery);

impl qobject::TagListQuery {
    pub fn tags(&self) -> QStringList {
        to_qstringlist(&self.data.items)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::TagListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("tag_list", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_query::apply_tag_list(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "tags" => self.as_mut().tags_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// ContentRatingListQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct ContentRatingListQueryRust {
    base: QueryState,
    data: StringListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(ContentRatingListQuery);

impl qobject::ContentRatingListQuery {
    pub fn ratings(&self) -> QStringList {
        to_qstringlist(&self.data.items)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::ContentRatingListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("content_rating_list", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_query::apply_content_rating_list(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "ratings" => self.as_mut().ratings_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
