// The whole "Status" page: daemon health, the list of live renderers, the
// list of renderer plugins and killing a renderer.
//
// The last four types that the tab took from the Python bridge.
// Reference: waywallen `ui/src/query/{health,renderer}_query.cpp`; response
// parsing already lived in the core (`we_query::apply_health` /
// `apply_renderer_list` / `apply_renderer_plugin_list`); here there is
// only the Qt side.
//
// What the port fixes along the way (rule #16 — we do not preserve the
// original's bugs):
//
//   1. The bridge had NO `HealthQuery.osName`. The property came with the
//      v0.3.5 protocol, and the vendored `StatusPage.qml:222` reads it
//      directly: `visible: healthQuery.osName.length > 0`. On a
//      non-existent property this is `undefined.length` — a TypeError in
//      the binding, and the "daemon system" line never appeared. The core
//      parsed the field (`HealthData.os_name`); only the bridge did not
//      expose it.
//
//   2. Synchronizing the `RendererManager` with the `renderer_list` answer
//      went through JSON MAPS: the bridge serialized the maps parsed by the
//      core, and `we_objmodel` parsed them back with a second, "mirror"
//      parser. Now the manager receives the same RAW frame the query
//      parsed, and there is one normalizer for both paths — the daemon
//      event and the answer (rule #12). The mirror parser is removed.

/// cxx-qt bridge: four queries of the "Status" page.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

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
        #[qproperty(QString, service, READ = service, NOTIFY = service_changed)]
        #[qproperty(QString, state, READ = state, NOTIFY = state_changed)]
        #[qproperty(QString, os_name, cxx_name = "osName", READ = os_name,
                    NOTIFY = os_name_changed)]
        type HealthQuery = super::HealthQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// Path of the daemon log file.
        #[qproperty(QString, path, READ = path, NOTIFY = path_changed)]
        /// Log content (the daemon returns the tail, not the whole file).
        #[qproperty(QString, content, READ = content, NOTIFY = content_changed)]
        /// The daemon truncated the log — not all of it is shown.
        #[qproperty(bool, truncated, READ = truncated, NOTIFY = truncated_changed)]
        type DaemonLogQuery = super::DaemonLogQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QStringList, renderers, READ = renderers, NOTIFY = renderers_changed)]
        #[qproperty(QVariant, instances, READ = instances, NOTIFY = instances_changed)]
        type RendererListQuery = super::RendererListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, renderers, READ = renderers, NOTIFY = renderers_changed)]
        #[qproperty(QStringList, supported_types, cxx_name = "supportedTypes",
                    READ = supported_types, NOTIFY = supported_types_changed)]
        /// waywallen 0.4.2: `type key → label` (a string or a key of the plugin
        /// catalogue).
        #[qproperty(QVariant, type_labels, cxx_name = "typeLabels",
                    READ = type_labels, NOTIFY = type_labels_changed)]
        type RendererPluginListQuery = super::RendererPluginListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, renderer_id, cxx_name = "rendererId", READ = renderer_id,
                    WRITE = set_renderer_id, NOTIFY = renderer_id_changed)]
        type RendererKillQuery = super::RendererKillQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut HealthQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut HealthQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut HealthQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut HealthQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut HealthQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut HealthQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut HealthQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut HealthQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "serviceChanged"]
        fn service_changed(self: Pin<&mut HealthQuery>);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut HealthQuery>);
        #[qsignal]
        #[cxx_name = "osNameChanged"]
        fn os_name_changed(self: Pin<&mut HealthQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DaemonLogQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut DaemonLogQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut DaemonLogQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut DaemonLogQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut DaemonLogQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut DaemonLogQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut DaemonLogQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut DaemonLogQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "pathChanged"]
        fn path_changed(self: Pin<&mut DaemonLogQuery>);
        #[qsignal]
        #[cxx_name = "contentChanged"]
        fn content_changed(self: Pin<&mut DaemonLogQuery>);
        #[qsignal]
        #[cxx_name = "truncatedChanged"]
        fn truncated_changed(self: Pin<&mut DaemonLogQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RendererListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RendererListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RendererListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RendererListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RendererListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RendererListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RendererListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut RendererListQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "renderersChanged"]
        fn renderers_changed(self: Pin<&mut RendererListQuery>);
        #[qsignal]
        #[cxx_name = "instancesChanged"]
        fn instances_changed(self: Pin<&mut RendererListQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RendererPluginListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RendererPluginListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RendererPluginListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RendererPluginListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RendererPluginListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RendererPluginListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RendererPluginListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut RendererPluginListQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "renderersChanged"]
        fn renderers_changed(self: Pin<&mut RendererPluginListQuery>);
        #[qsignal]
        #[cxx_name = "supportedTypesChanged"]
        fn supported_types_changed(self: Pin<&mut RendererPluginListQuery>);
        #[qsignal]
        #[cxx_name = "typeLabelsChanged"]
        fn type_labels_changed(self: Pin<&mut RendererPluginListQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RendererKillQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RendererKillQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RendererKillQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RendererKillQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RendererKillQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RendererKillQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RendererKillQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut RendererKillQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "rendererIdChanged"]
        fn renderer_id_changed(self: Pin<&mut RendererKillQuery>);
    }

    extern "RustQt" {
        fn status(self: &HealthQuery) -> i32;
        fn error(self: &HealthQuery) -> QString;
        fn querying(self: &HealthQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &HealthQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut HealthQuery>, value: bool);
        fn service(self: &HealthQuery) -> QString;
        fn state(self: &HealthQuery) -> QString;
        #[cxx_name = "osName"]
        fn os_name(self: &HealthQuery) -> QString;

        fn status(self: &DaemonLogQuery) -> i32;
        fn error(self: &DaemonLogQuery) -> QString;
        fn querying(self: &DaemonLogQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &DaemonLogQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut DaemonLogQuery>, value: bool);
        fn path(self: &DaemonLogQuery) -> QString;
        fn content(self: &DaemonLogQuery) -> QString;
        fn truncated(self: &DaemonLogQuery) -> bool;

        fn status(self: &RendererListQuery) -> i32;
        fn error(self: &RendererListQuery) -> QString;
        fn querying(self: &RendererListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RendererListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RendererListQuery>, value: bool);
        fn renderers(self: &RendererListQuery) -> QStringList;
        fn instances(self: &RendererListQuery) -> QVariant;

        fn status(self: &RendererPluginListQuery) -> i32;
        fn error(self: &RendererPluginListQuery) -> QString;
        fn querying(self: &RendererPluginListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RendererPluginListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RendererPluginListQuery>, value: bool);
        fn renderers(self: &RendererPluginListQuery) -> QVariant;
        #[cxx_name = "supportedTypes"]
        fn supported_types(self: &RendererPluginListQuery) -> QStringList;
        #[cxx_name = "typeLabels"]
        fn type_labels(self: &RendererPluginListQuery) -> QVariant;

        fn status(self: &RendererKillQuery) -> i32;
        fn error(self: &RendererKillQuery) -> QString;
        fn querying(self: &RendererKillQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RendererKillQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RendererKillQuery>, value: bool);
        #[cxx_name = "rendererId"]
        fn renderer_id(self: &RendererKillQuery) -> QString;
        #[cxx_name = "setRendererId"]
        fn set_renderer_id(self: Pin<&mut RendererKillQuery>, value: &QString);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut HealthQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut HealthQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut DaemonLogQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut DaemonLogQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut RendererListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RendererListQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut RendererPluginListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RendererPluginListQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut RendererKillQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RendererKillQuery>, token: i64, frame: &QByteArray);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QStringList, QVariant};
use prost::Message;
use serde_json::{json, Value};

use rexon_core::proto::control as pb;
use rexon_core::we_query::{
    self, DaemonLogData, HealthData, QuerySignal, QueryState, RendererInstance, RendererListData,
    RendererPlugin, RendererPluginListData, SettingSchema,
};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::json_to_variant;

/// Core string list → `QStringList` for QML.
fn to_qstringlist(items: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for item in items {
        list.append(QString::from(item));
    }
    list
}

macro_rules! on_decode_error {
    ($self:ident) => {{
        let signals = decode_failed(&mut $self.as_mut().rust_mut().base);
        $self.dispatch(signals);
    }};
}

// ════════════════════════════════════════════════════════════════════════
// HealthQuery — daemon health (Window.qml and StatusPage.qml)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct HealthQueryRust {
    base: QueryState,
    data: HealthData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(HealthQuery);

impl qobject::HealthQuery {
    pub fn service(&self) -> QString {
        QString::from(&self.data.service)
    }

    pub fn state(&self) -> QString {
        QString::from(&self.data.state)
    }

    pub fn os_name(&self) -> QString {
        QString::from(&self.data.os_name)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::HealthRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("health", payload) {
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
            we_query::apply_health(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "service" => self.as_mut().service_changed(),
            "state" => self.as_mut().state_changed(),
            "osName" => self.as_mut().os_name_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// DaemonLogQuery — daemon log (dialog from the "Status" page, v0.3.8)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct DaemonLogQueryRust {
    base: QueryState,
    data: DaemonLogData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(DaemonLogQuery);

impl qobject::DaemonLogQuery {
    pub fn path(&self) -> QString {
        QString::from(&self.data.path)
    }

    pub fn content(&self) -> QString {
        QString::from(&self.data.content)
    }

    pub fn truncated(&self) -> bool {
        self.data.truncated
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::LogReadRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("log_read", payload) {
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
            we_query::apply_log_read(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "path" => self.as_mut().path_changed(),
            "content" => self.as_mut().content_changed(),
            "truncated" => self.as_mut().truncated_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// RendererListQuery — live renderers (StatusPage.qml)
// ════════════════════════════════════════════════════════════════════════

/// `RendererInstance` → a QML map (the keys of the vendor's
/// `renderer_to_map` plus our `keep`/`lastExitReason`/`processGeneration`).
fn renderer_instance_map(i: &RendererInstance) -> Value {
    json!({
        "id": i.id,
        "fps": i.fps,
        "status": i.status,
        "running": i.running,
        "keep": i.keep,
        "lastExitReason": i.last_exit_reason,
        "processGeneration": i.process_generation,
        "runtimeConditions": i.conditions.iter().map(|c| json!({
            "kind": c.kind,
            "origin": c.origin,
            "reason": c.reason,
            "relatedRendererId": c.related_renderer_id,
            "relatedDisplayId": c.related_display_id,
        })).collect::<Vec<Value>>(),
        "runtimeTags": i.runtime_tags.iter().map(|t| json!({
            "key": t.key,
            "value": t.value,
        })).collect::<Vec<Value>>(),
        "name": i.name,
        "pid": i.pid,
        "textureWidth": i.texture_width,
        "textureHeight": i.texture_height,
        "drmRenderMajor": i.drm_render_major,
        "drmRenderMinor": i.drm_render_minor,
    })
}

#[derive(Default)]
pub struct RendererListQueryRust {
    base: QueryState,
    data: RendererListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(RendererListQuery);

impl qobject::RendererListQuery {
    pub fn renderers(&self) -> QStringList {
        to_qstringlist(&self.data.renderers)
    }

    pub fn instances(&self) -> QVariant {
        json_to_variant(&Value::Array(
            self.data.instances.iter().map(renderer_instance_map).collect(),
        ))
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::RendererListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("renderer_list", payload) {
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
            we_query::apply_renderer_list(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => {
                // The live model `App.rendererManager` must see the answer the same way it
                // sees events: in the original the synchronization is done by the query
                // itself (`RendererListQuery::reload` → `replaceAll`). We hand over the
                // frame RAW — there is one normalizer.
                crate::we_singletons::renderers_replace_frame(frame.as_slice());
                self.dispatch(signals);
            }
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "renderers" => self.as_mut().renderers_changed(),
            "instances" => self.as_mut().instances_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// RendererPluginListQuery — renderer plugins (StatusPage, "Wallpapers", details)
// ════════════════════════════════════════════════════════════════════════

/// `SettingSchema` → a map.
///
/// Warning: the keys are SNAKE_CASE: `SettingField.qml` is written against
/// the original's `default_value`/`label_key`/`description_key`; it would
/// not read camelCase.
fn setting_schema_map(s: &SettingSchema) -> Value {
    json!({
        "key": s.key,
        "type": s.value_type,
        "default_value": s.default_value,
        "identity": s.identity,
        "label_key": s.label_key,
        "description_key": s.description_key,
        // Translatable labels of the plugin catalogue: `SettingField.qml` reads
        // `label`/`description`, and the group title is `group_label`.
        "label": s.label,
        "description": s.description,
        "group_label": s.group_label,
        "min": s.min,
        "max": s.max,
        "step": s.step,
        "choices": s.choices,
        "group": s.group,
        "order": s.order,
    })
}

/// `RendererPluginInfo` → a map (the original's `renderer_plugin_to_map`
/// plus our `bin`/`priority`/`pluginId`; extra keys do not bother QML).
fn renderer_plugin_map(p: &RendererPlugin) -> Value {
    json!({
        "name": p.name,
        "bin": p.bin,
        "types": p.types,
        "priority": p.priority,
        "version": p.version,
        "settings": p.settings.iter().map(setting_schema_map).collect::<Vec<Value>>(),
        "pluginId": p.plugin_id,
    })
}

#[derive(Default)]
pub struct RendererPluginListQueryRust {
    base: QueryState,
    data: RendererPluginListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(RendererPluginListQuery);

impl qobject::RendererPluginListQuery {
    pub fn renderers(&self) -> QVariant {
        json_to_variant(&Value::Array(
            self.data.renderers.iter().map(renderer_plugin_map).collect(),
        ))
    }

    pub fn supported_types(&self) -> QStringList {
        to_qstringlist(&self.data.supported_types)
    }

    pub fn type_labels(&self) -> QVariant {
        json_to_variant(&serde_json::to_value(&self.data.type_labels).unwrap_or_default())
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::RendererPluginListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("renderer_plugin_list", payload) {
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
            we_query::apply_renderer_plugin_list(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "renderers" => self.as_mut().renderers_changed(),
            "supportedTypes" => self.as_mut().supported_types_changed(),
            "typeLabels" => self.as_mut().type_labels_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// RendererKillQuery — "Kill renderer" (StatusPage.qml)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct RendererKillQueryRust {
    base: QueryState,
    renderer_id: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(RendererKillQuery);

impl qobject::RendererKillQuery {
    pub fn renderer_id(&self) -> QString {
        QString::from(&self.renderer_id)
    }

    pub fn set_renderer_id(mut self: Pin<&mut Self>, value: &QString) {
        let next = value.to_string();
        if self.renderer_id == next {
            return;
        }
        self.as_mut().rust_mut().renderer_id = next;
        self.renderer_id_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let renderer_id = self.renderer_id.clone();
        if renderer_id.is_empty() {
            return;
        }
        // Success is reflected by the `renderer_removed` event → RendererManager;
        // here there is only sending and the status for the UI.
        let payload = pb::RendererKillRequest { renderer_id }.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("renderer_kill", payload) {
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
        let _ = self;
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
