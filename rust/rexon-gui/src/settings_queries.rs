// Daemon settings: reading (`SettingsGetQuery`) and writing
// (`SettingsSetQuery`).
//
// Reference: waywallen `ui/src/query/settings_query.cpp`.
// The `global` map and the plugin table are assembled by the core
// (`we_settings::global_to_map`/`plugins_to_map`); here there is only the
// Qt side.
//
// Warning: the keys of the `global` map are CUSTOM, not the protocol's
// json_name: `pluginUpdateNotifications` = !disable_plugin_update_notifications
// (INVERSION), `duplicateRenderers` = duplicate_renderers_for_same_wallpaper
// (renaming). Both sides of the aliases live in the core precisely so that
// the "read ↔ write" round trip cannot drift apart: the daemon treats
// `settings_set` as a FULL replacement, and a one-sided alias would wipe
// settings.
//
// The `global` property of a write arrives from QML as a map containing
// Qt Protobuf gadgets (`wallpaperSorts` is written by the Rust model of
// the filter editor). Because of them the Python bridge held it as a
// `QJSValue` and parsed it with a schema by descriptor: QVariantMap
// conversion of the gadgets segfaulted PySide6. In C++/Rust there is no
// such trouble — the map arrives as is, and the gadgets are unwrapped by
// the shared converter `qvalue::variant_to_json`, the same one used by all
// other "QML → core" paths.
//
// Warning: the daemon's `settings_set` is a FULL replacement. The page must
// send a copy of the whole `global` map from `settings_get` with the edit,
// and all plugins; the "do not write before the first read" gate lives in
// QML (`_persistGlobalChange`), and it must not be duplicated here —
// otherwise a legitimate write of empty settings would become impossible.

/// cxx-qt bridge: reading and writing the daemon settings.
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
        #[qproperty(QVariant, global, READ = global, NOTIFY = global_changed)]
        #[qproperty(QVariant, plugins, READ = plugins, NOTIFY = plugins_changed)]
        /// waywallen 0.3.8: the daemon's log directory; the settings page opens it
        /// with the "Open" button.
        #[qproperty(QString, log_dir, cxx_name = "logDir", READ = log_dir,
                    NOTIFY = log_dir_changed)]
        /// waywallen 0.3.8: the log level is set from outside (`WW_LOG`) — the
        /// debug log switch is disabled and explains why.
        #[qproperty(bool, ww_log_active, cxx_name = "wwLogActive",
                    READ = ww_log_active, NOTIFY = ww_log_active_changed)]
        type SettingsGetQuery = super::SettingsGetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, global, READ = global, WRITE = set_global,
                    NOTIFY = global_changed)]
        #[qproperty(QVariant, plugins, READ = plugins, WRITE = set_plugins,
                    NOTIFY = plugins_changed)]
        type SettingsSetQuery = super::SettingsSetQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut SettingsGetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut SettingsGetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut SettingsGetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut SettingsGetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "globalChanged"]
        fn global_changed(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        #[cxx_name = "pluginsChanged"]
        fn plugins_changed(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        #[cxx_name = "logDirChanged"]
        fn log_dir_changed(self: Pin<&mut SettingsGetQuery>);
        #[qsignal]
        #[cxx_name = "wwLogActiveChanged"]
        fn ww_log_active_changed(self: Pin<&mut SettingsGetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut SettingsSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut SettingsSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut SettingsSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut SettingsSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut SettingsSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut SettingsSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut SettingsSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut SettingsSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "globalChanged"]
        fn global_changed(self: Pin<&mut SettingsSetQuery>);
        #[qsignal]
        #[cxx_name = "pluginsChanged"]
        fn plugins_changed(self: Pin<&mut SettingsSetQuery>);
    }

    extern "RustQt" {
        fn status(self: &SettingsGetQuery) -> i32;
        fn error(self: &SettingsGetQuery) -> QString;
        fn querying(self: &SettingsGetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &SettingsGetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut SettingsGetQuery>, value: bool);
        fn global(self: &SettingsGetQuery) -> QVariant;
        fn plugins(self: &SettingsGetQuery) -> QVariant;
        #[cxx_name = "logDir"]
        fn log_dir(self: &SettingsGetQuery) -> QString;
        #[cxx_name = "wwLogActive"]
        fn ww_log_active(self: &SettingsGetQuery) -> bool;

        fn status(self: &SettingsSetQuery) -> i32;
        fn error(self: &SettingsSetQuery) -> QString;
        fn querying(self: &SettingsSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &SettingsSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut SettingsSetQuery>, value: bool);
        fn global(self: &SettingsSetQuery) -> QVariant;
        #[cxx_name = "setGlobal"]
        fn set_global(self: Pin<&mut SettingsSetQuery>, value: &QVariant);
        fn plugins(self: &SettingsSetQuery) -> QVariant;
        #[cxx_name = "setPlugins"]
        fn set_plugins(self: Pin<&mut SettingsSetQuery>, value: &QVariant);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut SettingsGetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut SettingsGetQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut SettingsSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut SettingsSetQuery>, token: i64, frame: &QByteArray);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QVariant};
use prost::Message;
use serde_json::Value;

use rexon_core::proto::control as pb;
use rexon_core::we_query::{self as we_query, QuerySignal, QueryState};
use rexon_core::we_settings::{self, SettingsGetData, VMap};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::{json_to_variant, variant_to_json};

#[derive(Default)]
pub struct SettingsGetQueryRust {
    base: QueryState,
    data: SettingsGetData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(SettingsGetQuery);

impl qobject::SettingsGetQuery {
    pub fn global(&self) -> QVariant {
        json_to_variant(&Value::Object(self.data.global.clone()))
    }

    pub fn plugins(&self) -> QVariant {
        json_to_variant(&Value::Object(self.data.plugins.clone()))
    }

    pub fn log_dir(&self) -> QString {
        QString::from(&self.data.log_dir)
    }

    pub fn ww_log_active(&self) -> bool {
        self.data.ww_log_active
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::SettingsGetRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("settings_get", payload) {
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
            we_settings::apply_settings_get(&mut rust.base, &mut rust.data, frame.as_slice())
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
            "global" => self.as_mut().global_changed(),
            "plugins" => self.as_mut().plugins_changed(),
            "logDir" => self.as_mut().log_dir_changed(),
            "wwLogActive" => self.as_mut().ww_log_active_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// SettingsSetQuery — writing settings (SettingsPage, "Wallpapers", plugins)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct SettingsSetQueryRust {
    base: QueryState,
    /// `None` means the property has not been written yet. It must be told
    /// apart from an empty map: the page gates writing until the first read,
    /// and a request without an assigned `global` must not go to the daemon at
    /// all.
    global: Option<VMap>,
    plugins: VMap,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(SettingsSetQuery);

/// Value from QML → a map. Not an object — an empty map.
fn variant_to_map(value: &QVariant) -> VMap {
    match variant_to_json(value) {
        Value::Object(map) => map,
        _ => VMap::new(),
    }
}

impl qobject::SettingsSetQuery {
    pub fn global(&self) -> QVariant {
        // Mirror for debugging: exactly what will go to the daemon.
        json_to_variant(&Value::Object(self.global.clone().unwrap_or_default()))
    }

    pub fn set_global(mut self: Pin<&mut Self>, value: &QVariant) {
        let next = variant_to_map(value);
        self.as_mut().rust_mut().global = Some(next);
        self.global_changed();
    }

    pub fn plugins(&self) -> QVariant {
        json_to_variant(&Value::Object(self.plugins.clone()))
    }

    pub fn set_plugins(mut self: Pin<&mut Self>, value: &QVariant) {
        let next = variant_to_map(value);
        self.as_mut().rust_mut().plugins = next;
        self.plugins_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let Some(global) = self.global.clone() else {
            // Nothing to write: the page has not read the settings yet. A full
            // replacement with an empty map would wipe everything the daemon has.
            let signals = we_query::set_status(
                &mut self.as_mut().rust_mut().base,
                we_query::STATUS_FINISHED,
            );
            self.dispatch(signals);
            return;
        };
        let plugins = self.plugins.clone();
        let payload = we_settings::encode_settings_set(&global, &plugins);
        if let Some(token) = self.as_mut().send_frame("settings_set", payload) {
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
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "global" => self.as_mut().global_changed(),
            "plugins" => self.as_mut().plugins_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
