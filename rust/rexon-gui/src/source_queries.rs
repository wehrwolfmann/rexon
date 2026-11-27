// Wallpaper sources and libraries: the list of source plugins, adding,
// editing (waywallen 0.6.0: name and settings; in 0.5.0 — only renaming)
// and removing a library, auto-detection.
//
// Reference: waywallen `ui/src/query/{source,library}_query.cpp` @e5626c1
// (vendor commit v0.2.6+9), not our Python bridge. The port fixes two
// divergences that porting "as it was" would have preserved (rule #16):
//
//   1. `SourceListQuery` returned a map of five keys, while the original at
//      this vendor revision also puts `pluginId` and the full `settings`
//      schema — without them the source settings page knows neither the
//      source's owner nor what settings it has.
//   2. `LibraryAutoDetectQuery` threw away the daemon's answer entirely
//      (`lambda rsp: None`), although the original has an `addedCount`
//      property: how many libraries were found had nowhere to be shown.
//
// Parsing of both answers moved to the core (`rexon_core::we_manage`).

/// cxx-qt bridge: five source and library queries.
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
        /// waywallen 0.5.0: name of the new library (the "Name" field of
        /// AddLibraryPage). Empty means the daemon picks the default name.
        #[qproperty(QString, name, READ = name, WRITE = set_name, NOTIFY = name_changed)]
        #[qproperty(QString, plugin_name, cxx_name = "pluginName", READ = plugin_name,
                    WRITE = set_plugin_name, NOTIFY = plugin_name_changed)]
        /// waywallen 0.6.0: setting values of the new library (the plugin's
        /// `librarySettings`, `LibrarySettingsFields`) — a map.
        #[qproperty(QVariant, settings, READ = settings, WRITE = set_settings,
                    NOTIFY = settings_changed)]
        type LibraryAddQuery = super::LibraryAddQueryRust;

        /// waywallen 0.6.0: editing a library — name and settings
        /// (`EditLibraryPage`; in 0.5.0 this was the `LibraryRenameQuery` of the
        /// "Rename library" dialog). The number is `f64` for the same reason as for
        /// `LibraryRemoveQuery` below.
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
        #[qproperty(QVariant, settings, READ = settings, WRITE = set_settings,
                    NOTIFY = settings_changed)]
        type LibraryEditQuery = super::LibraryEditQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// Warning: `f64`, not `i64` — and this is not carelessness. The library
        /// number is an integer, but a 64-bit integer must not be exposed to QML: a
        /// literal binding "libraryId: 7" fails together with the whole document.
        /// The analysis and precision limits are in `src/qvalue.rs`, the live check
        /// in `src/qml_int64_probe.rs`.
        #[qproperty(f64, library_id, cxx_name = "libraryId", READ = library_id,
                    WRITE = set_library_id, NOTIFY = library_id_changed)]
        type LibraryRemoveQuery = super::LibraryRemoveQueryRust;

        /// waywallen 0.6.1: move a library to another folder
        /// (SourceManagePage, "Change library folder").
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// Warning: `f64`, not `i64` — and this is not carelessness. The library
        /// number is an integer, but a 64-bit integer must not be exposed to QML: a
        /// literal binding "libraryId: 7" fails together with the whole document.
        /// The analysis and precision limits are in `src/qvalue.rs`, the live check
        /// in `src/qml_int64_probe.rs`.
        #[qproperty(f64, library_id, cxx_name = "libraryId", READ = library_id,
                    WRITE = set_library_id, NOTIFY = library_id_changed)]
        #[qproperty(QString, path, READ = path, WRITE = set_path, NOTIFY = path_changed)]
        type LibraryRelocateQuery = super::LibraryRelocateQueryRust;

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
        fn status_changed(self: Pin<&mut LibraryEditQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut LibraryEditQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut LibraryEditQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut LibraryEditQuery>, token: i64, field: QString,
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
        fn status_changed(self: Pin<&mut LibraryRelocateQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut LibraryRelocateQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LibraryRelocateQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut LibraryRelocateQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut LibraryRelocateQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut LibraryRelocateQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut LibraryRelocateQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut LibraryRelocateQuery>, token: i64, field: QString,
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
        #[cxx_name = "settingsChanged"]
        fn settings_changed(self: Pin<&mut LibraryAddQuery>);
        #[qsignal]
        #[cxx_name = "settingsChanged"]
        fn settings_changed(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        #[cxx_name = "libraryIdChanged"]
        fn library_id_changed(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        #[cxx_name = "nameChanged"]
        fn name_changed(self: Pin<&mut LibraryEditQuery>);
        #[qsignal]
        #[cxx_name = "libraryIdChanged"]
        fn library_id_changed(self: Pin<&mut LibraryRemoveQuery>);
        #[qsignal]
        #[cxx_name = "libraryIdChanged"]
        fn library_id_changed(self: Pin<&mut LibraryRelocateQuery>);
        #[qsignal]
        #[cxx_name = "pathChanged"]
        fn path_changed(self: Pin<&mut LibraryRelocateQuery>);
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

        fn status(self: &LibraryEditQuery) -> i32;
        fn error(self: &LibraryEditQuery) -> QString;
        fn querying(self: &LibraryEditQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryEditQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryEditQuery>, value: bool);

        fn status(self: &LibraryRemoveQuery) -> i32;
        fn error(self: &LibraryRemoveQuery) -> QString;
        fn querying(self: &LibraryRemoveQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryRemoveQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryRemoveQuery>, value: bool);

        fn status(self: &LibraryRelocateQuery) -> i32;
        fn error(self: &LibraryRelocateQuery) -> QString;
        fn querying(self: &LibraryRelocateQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &LibraryRelocateQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut LibraryRelocateQuery>, value: bool);

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
        fn settings(self: &LibraryAddQuery) -> QVariant;
        #[cxx_name = "setSettings"]
        fn set_settings(self: Pin<&mut LibraryAddQuery>, value: QVariant);
        fn settings(self: &LibraryEditQuery) -> QVariant;
        #[cxx_name = "setSettings"]
        fn set_settings(self: Pin<&mut LibraryEditQuery>, value: QVariant);
        #[cxx_name = "libraryId"]
        fn library_id(self: &LibraryRemoveQuery) -> f64;
        #[cxx_name = "setLibraryId"]
        fn set_library_id(self: Pin<&mut LibraryRemoveQuery>, value: f64);
        #[cxx_name = "libraryId"]
        fn library_id(self: &LibraryRelocateQuery) -> f64;
        #[cxx_name = "setLibraryId"]
        fn set_library_id(self: Pin<&mut LibraryRelocateQuery>, value: f64);
        fn path(self: &LibraryRelocateQuery) -> QString;
        #[cxx_name = "setPath"]
        fn set_path(self: Pin<&mut LibraryRelocateQuery>, value: QString);
        #[cxx_name = "libraryId"]
        fn library_id(self: &LibraryEditQuery) -> f64;
        #[cxx_name = "setLibraryId"]
        fn set_library_id(self: Pin<&mut LibraryEditQuery>, value: f64);
        fn name(self: &LibraryEditQuery) -> QString;
        #[cxx_name = "setName"]
        fn set_name(self: Pin<&mut LibraryEditQuery>, value: QString);
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
        fn reload(self: Pin<&mut LibraryEditQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryEditQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut LibraryRemoveQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryRemoveQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut LibraryRelocateQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut LibraryRelocateQuery>, token: i64, frame: &QByteArray);

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
use crate::qvalue::{json_to_variant, variant_to_json};

/// Emit the "frame did not parse" signals — the same for all five.
macro_rules! on_decode_error {
    ($self:ident) => {{
        let signals = decode_failed(&mut $self.as_mut().rust_mut().base);
        $self.dispatch(signals);
    }};
}

// ════════════════════════════════════════════════════════════════════════
// SourceListQuery — list of source plugins (SourceManagePage/AddLibraryPage)
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
// LibraryAddQuery — add a library path under a specific plugin
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryAddQueryRust {
    base: QueryState,
    path: String,
    name: String,
    plugin_name: String,
    /// Library settings map as the page set it (JSON).
    settings: Value,
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

    pub fn settings(&self) -> QVariant {
        json_to_variant(&self.settings)
    }

    pub fn set_settings(mut self: Pin<&mut Self>, value: QVariant) {
        let next = variant_to_json(&value);
        if self.settings == next {
            return;
        }
        self.as_mut().rust_mut().settings = next;
        self.settings_changed();
    }

    /// The original silently returns if BOTH fields are not set. The name
    /// (0.5.0) is trimmed, an empty one is not sent; the settings (0.6.0) go as
    /// strings (`QVariant::toString`). The decision is in the core
    /// (`we_client::library_add_request`).
    pub fn reload(mut self: Pin<&mut Self>) {
        let Some(request) = we_client::library_add_request(
            &self.path,
            &self.plugin_name,
            &self.name,
            we_client::library_settings(&self.settings),
        ) else {
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
        // There is no useful payload: only the outcome matters (`inspect_set` with
        // an empty lambda in the original). The shared point for all such requests
        // is in the core.
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
// LibraryEditQuery — library name and settings (waywallen 0.6.0)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryEditQueryRust {
    base: QueryState,
    library_id: i64,
    name: String,
    settings: Value,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(LibraryEditQuery);

impl qobject::LibraryEditQuery {
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

    pub fn settings(&self) -> QVariant {
        json_to_variant(&self.settings)
    }

    pub fn set_settings(mut self: Pin<&mut Self>, value: QVariant) {
        let next = variant_to_json(&value);
        if self.settings == next {
            return;
        }
        self.as_mut().rust_mut().settings = next;
        self.settings_changed();
    }

    /// The number is not positive or the name is empty after trimming spaces —
    /// the original silently returns, the status is not touched
    /// (`we_client::library_edit_request`).
    pub fn reload(mut self: Pin<&mut Self>) {
        let Some(request) = we_client::library_edit_request(
            self.library_id,
            &self.name,
            we_client::library_settings(&self.settings),
        ) else {
            return;
        };
        let payload = request.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("library_edit", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        // The daemon's answer is `Empty`: only the outcome matters, as for
        // removal.
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
// LibraryRemoveQuery — remove a library by number
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
// LibraryRelocateQuery — move a library to another folder (waywallen 0.6.1)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct LibraryRelocateQueryRust {
    base: QueryState,
    library_id: i64,
    path: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(LibraryRelocateQuery);

impl qobject::LibraryRelocateQuery {
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

    pub fn path(&self) -> QString {
        QString::from(&self.path)
    }

    pub fn set_path(mut self: Pin<&mut Self>, value: QString) {
        let value = value.to_string();
        if self.path == value {
            return;
        }
        self.as_mut().rust_mut().path = value;
        self.path_changed();
    }

    /// Same guard as upstream: nothing is sent without an id and a path.
    pub fn reload(mut self: Pin<&mut Self>) {
        if self.library_id <= 0 || self.path.is_empty() {
            return;
        }
        let payload = pb::LibraryRelocateRequest {
            id: self.library_id,
            path: self.path.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("library_relocate", payload) {
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
        debug_assert!(false, "no signal for property {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// LibraryAutoDetectQuery — searching for libraries on disk
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
