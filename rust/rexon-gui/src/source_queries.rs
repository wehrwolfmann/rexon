// Источники и библиотеки обоев: список source-плагинов, добавление,
// переименование (waywallen 0.5.0) и удаление библиотеки, автоопределение.
//
// Эталон — waywallen `ui/src/query/{source,library}_query.cpp`
// @e5626c1 (вендорный коммит v0.2.6+9), а не наш Python-мост. Порт чинит два
// расхождения, которые перенос «как было» законсервировал бы (правило #16):
//
//   1. `SourceListQuery` отдавал карту из пяти ключей, а оригинал на этом
//      вендоре кладёт ещё `pluginId` и полную схему `settings` — без них
//      страница настроек источника не знает ни владельца источника, ни
//      того, какие у него настройки есть.
//   2. `LibraryAutoDetectQuery` выбрасывал ответ демона целиком
//      (`lambda rsp: None`), хотя у оригинала есть свойство `addedCount`:
//      сколько библиотек нашлось, показать было нечем.
//
// Разбор обоих ответов уехал в ядро (`rexon_core::we_manage`).

/// Мост cxx-qt: пять запросов источников и библиотек.
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
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, sources, READ = sources, NOTIFY = sources_changed)]
        type SourceListQuery = super::SourceListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, path, READ = path, WRITE = set_path, NOTIFY = path_changed)]
        /// waywallen 0.5.0: имя новой библиотеки (поле «Name» AddLibraryPage).
        /// Пустое — имя по умолчанию выбирает демон.
        #[qproperty(QString, name, READ = name, WRITE = set_name, NOTIFY = name_changed)]
        #[qproperty(QString, plugin_name, cxx_name = "pluginName", READ = plugin_name,
                    WRITE = set_plugin_name, NOTIFY = plugin_name_changed)]
        type LibraryAddQuery = super::LibraryAddQueryRust;

        /// waywallen 0.5.0: переименовать библиотеку (диалог «Rename library»
        /// SourceManagePage). Номер — `f64` по той же причине, что у
        /// `LibraryRemoveQuery` ниже.
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(f64, library_id, cxx_name = "libraryId", READ = library_id,
                    WRITE = set_library_id, NOTIFY = library_id_changed)]
        #[qproperty(QString, name, READ = name, WRITE = set_name, NOTIFY = name_changed)]
        type LibraryRenameQuery = super::LibraryRenameQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// ⚠️ `f64`, а не `i64`, — и это не небрежность. Номер библиотеки
        /// целый, но 64-битное целое наружу в QML выпускать нельзя:
        /// литеральная привязка «libraryId: 7» отказывает вместе со всем
        /// документом. Разбор и границы точности — `src/qvalue.rs`,
        /// живая проверка — `src/qml_int64_probe.rs`.
        #[qproperty(f64, library_id, cxx_name = "libraryId", READ = library_id,
                    WRITE = set_library_id, NOTIFY = library_id_changed)]
        type LibraryRemoveQuery = super::LibraryRemoveQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(i32, added_count, cxx_name = "addedCount", READ = added_count,
                    NOTIFY = added_count_changed)]
        type LibraryAutoDetectQuery = super::LibraryAutoDetectQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut SourceListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut SourceListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut SourceListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut SourceListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut SourceListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut SourceListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut SourceListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut SourceListQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut LibraryAddQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut LibraryAddQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut LibraryAddQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut LibraryAddQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut LibraryRenameQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut LibraryRenameQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LibraryRenameQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut LibraryRenameQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut LibraryRenameQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut LibraryRenameQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut LibraryRenameQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut LibraryRenameQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut LibraryRemoveQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut LibraryRemoveQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LibraryRemoveQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut LibraryRemoveQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut LibraryRemoveQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut LibraryRemoveQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut LibraryRemoveQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut LibraryRemoveQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut LibraryAutoDetectQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut LibraryAutoDetectQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LibraryAutoDetectQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut LibraryAutoDetectQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut LibraryAutoDetectQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut LibraryAutoDetectQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut LibraryAutoDetectQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut LibraryAutoDetectQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "sourcesChanged"]
        fn sources_changed(self: Pin<&mut SourceListQuery>);
        #[qsignal]
        #[cxx_name = "pathChanged"]
        fn path_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "nameChanged"]
        fn name_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "pluginNameChanged"]
        fn plugin_name_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "libraryIdChanged"]
        fn library_id_changed(self: Pin<&mut LibraryRenameQuery>);
        #[qsignal]
        #[cxx_name = "nameChanged"]
        fn name_changed(self: Pin<&mut LibraryRenameQuery>);
        #[qsignal]
        #[cxx_name = "libraryIdChanged"]
        fn library_id_changed(self: Pin<&mut LibraryRemoveQuery>);
        #[qsignal]
        #[cxx_name = "addedCountChanged"]
        fn added_count_changed(self: Pin<&mut LibraryAutoDetectQuery>);
    }

    extern "RustQt" {
        fn status(self: &SourceListQuery) -> i32;
        fn error(self: &SourceListQuery) -> QString;
        fn querying(self: &SourceListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &SourceListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut SourceListQuery>, value: bool);

        fn status(self: &LibraryAddQuery) -> i32;
        fn error(self: &LibraryAddQuery) -> QString;
        fn querying(self: &LibraryAddQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryAddQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryAddQuery>, value: bool);

        fn status(self: &LibraryRenameQuery) -> i32;
        fn error(self: &LibraryRenameQuery) -> QString;
        fn querying(self: &LibraryRenameQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryRenameQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryRenameQuery>, value: bool);

        fn status(self: &LibraryRemoveQuery) -> i32;
        fn error(self: &LibraryRemoveQuery) -> QString;
        fn querying(self: &LibraryRemoveQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryRemoveQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryRemoveQuery>, value: bool);

        fn status(self: &LibraryAutoDetectQuery) -> i32;
        fn error(self: &LibraryAutoDetectQuery) -> QString;
        fn querying(self: &LibraryAutoDetectQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryAutoDetectQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryAutoDetectQuery>, value: bool);

        fn sources(self: &SourceListQuery) -> QVariant;
        fn path(self: &LibraryAddQuery) -> QString;
        #[cxx_name = "setPath"]
        fn set_path(self: Pin<&mut LibraryAddQuery>, value: QString);
        fn name(self: &LibraryAddQuery) -> QString;
        #[cxx_name = "setName"]
        fn set_name(self: Pin<&mut LibraryAddQuery>, value: QString);
        #[cxx_name = "pluginName"]
        fn plugin_name(self: &LibraryAddQuery) -> QString;
        #[cxx_name = "setPluginName"]
        fn set_plugin_name(self: Pin<&mut LibraryAddQuery>, value: QString);
        #[cxx_name = "libraryId"]
        fn library_id(self: &LibraryRemoveQuery) -> f64;
        #[cxx_name = "setLibraryId"]
        fn set_library_id(self: Pin<&mut LibraryRemoveQuery>, value: f64);
        #[cxx_name = "libraryId"]
        fn library_id(self: &LibraryRenameQuery) -> f64;
        #[cxx_name = "setLibraryId"]
        fn set_library_id(self: Pin<&mut LibraryRenameQuery>, value: f64);
        fn name(self: &LibraryRenameQuery) -> QString;
        #[cxx_name = "setName"]
        fn set_name(self: Pin<&mut LibraryRenameQuery>, value: QString);
        #[cxx_name = "addedCount"]
        fn added_count(self: &LibraryAutoDetectQuery) -> i32;
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut SourceListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut SourceListQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut LibraryAddQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryAddQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut LibraryRenameQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryRenameQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut LibraryRemoveQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryRemoveQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut LibraryAutoDetectQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryAutoDetectQuery>, token: i64,
                            frame: &QByteArray);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QVariant};
use prost::Message;
use serde_json::Value;

use rexon_core::proto::control as pb;
use rexon_core::we_client;
use rexon_core::we_manage;
use rexon_core::we_query::{QuerySignal, QueryState};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::json_to_variant;

/// Разослать сигналы «кадр не разобрался» — одинаково у всех пяти.
macro_rules! on_decode_error {
    ($self:ident) => {{
        let signals = decode_failed(&mut $self.as_mut().rust_mut().base);
        $self.dispatch(signals);
    }};
}

// ════════════════════════════════════════════════════════════════════════
// SourceListQuery — список source-плагинов (SourceManagePage/AddLibraryPage)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct SourceListQueryRust {
    base: QueryState,
    data: we_manage::SourceListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(SourceListQuery);

impl qobject::SourceListQuery {
    pub fn sources(&self) -> QVariant {
        json_to_variant(&Value::Array(self.data.sources.clone()))
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::SourceListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("source_list", payload) {
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
            we_manage::apply_source_list(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "sources" => self.as_mut().sources_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// LibraryAddQuery — добавить путь библиотеки под конкретный плагин
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryAddQueryRust {
    base: QueryState,
    path: String,
    name: String,
    plugin_name: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(LibraryAddQuery);

impl qobject::LibraryAddQuery {
    pub fn path(&self) -> QString {
        QString::from(&self.path)
    }

    pub fn set_path(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.path == next {
            return;
        }
        self.as_mut().rust_mut().path = next;
        self.path_changed();
    }

    pub fn name(&self) -> QString {
        QString::from(&self.name)
    }

    pub fn set_name(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.name == next {
            return;
        }
        self.as_mut().rust_mut().name = next;
        self.name_changed();
    }

    pub fn plugin_name(&self) -> QString {
        QString::from(&self.plugin_name)
    }

    pub fn set_plugin_name(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.plugin_name == next {
            return;
        }
        self.as_mut().rust_mut().plugin_name = next;
        self.plugin_name_changed();
    }

    /// Оригинал молча выходит, если не заданы ОБА поля. Имя (0.5.0)
    /// обрезается, пустое не шлётся — решение в ядре
    /// (`we_client::library_add_request`).
    pub fn reload(mut self: Pin<&mut Self>) {
        let Some(request) =
            we_client::library_add_request(&self.path, &self.plugin_name, &self.name)
        else {
            return;
        };
        let payload = request.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("library_add", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        // Полезной нагрузки нет: важен только исход (`inspect_set` с пустой
        // лямбдой у оригинала). Общая точка на все такие запросы — в ядре.
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            rexon_core::we_query::apply_ack(&mut rust.base, frame.as_slice())
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
// LibraryRenameQuery — новое имя библиотеки (waywallen 0.5.0)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryRenameQueryRust {
    base: QueryState,
    library_id: i64,
    name: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(LibraryRenameQuery);

impl qobject::LibraryRenameQuery {
    pub fn library_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.library_id)
    }

    pub fn set_library_id(mut self: Pin<&mut Self>, value: f64) {
        let value = crate::qvalue::qml_to_i64(value);
        if self.library_id == value {
            return;
        }
        self.as_mut().rust_mut().library_id = value;
        self.library_id_changed();
    }

    pub fn name(&self) -> QString {
        QString::from(&self.name)
    }

    pub fn set_name(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.name == next {
            return;
        }
        self.as_mut().rust_mut().name = next;
        self.name_changed();
    }

    /// Номер не положительный или имя пусто после среза пробелов — оригинал
    /// молча выходит, статус не трогается (`we_client::library_rename_request`).
    pub fn reload(mut self: Pin<&mut Self>) {
        let Some(request) = we_client::library_rename_request(self.library_id, &self.name)
        else {
            return;
        };
        let payload = request.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("library_rename", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        // Ответ демона — `Empty`: важен только исход, как у удаления.
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            rexon_core::we_query::apply_ack(&mut rust.base, frame.as_slice())
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
// LibraryRemoveQuery — убрать библиотеку по номеру
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryRemoveQueryRust {
    base: QueryState,
    library_id: i64,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(LibraryRemoveQuery);

impl qobject::LibraryRemoveQuery {
    pub fn library_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.library_id)
    }

    pub fn set_library_id(mut self: Pin<&mut Self>, value: f64) {
        let value = crate::qvalue::qml_to_i64(value);
        if self.library_id == value {
            return;
        }
        self.as_mut().rust_mut().library_id = value;
        self.library_id_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.library_id == 0 {
            return;
        }
        let payload = pb::LibraryRemoveRequest {
            id: self.library_id,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("library_remove", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            rexon_core::we_query::apply_ack(&mut rust.base, frame.as_slice())
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
// LibraryAutoDetectQuery — поиск библиотек на диске
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryAutoDetectQueryRust {
    base: QueryState,
    added_count: i32,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(LibraryAutoDetectQuery);

impl qobject::LibraryAutoDetectQuery {
    pub fn added_count(&self) -> i32 {
        self.added_count
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::LibraryAutoDetectRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("library_auto_detect", payload) {
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
            we_manage::apply_library_auto_detect(
                &mut rust.base,
                &mut rust.added_count,
                frame.as_slice(),
            )
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "addedCount" => self.as_mut().added_count_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
