// UserPropertyListModel — the wallpaper property editor in the details
// panel of the Wallpaper Engine tab, ported from Python to Rust/cxx-qt.
// A model without Qt Protobuf gadgets and without network — the input is
// just two JSON strings (the wallpaper's `userPropertiesSchema` and
// `userPropertyOverrides`), the output is rows for the vendored QML.
//
// The original is rexon/qt_wallpaper_engine/qml_ui/bridge.py:4180 (itself a
// port of waywallen's model/user_property_model.cpp @v0.2.5). We do NOT
// touch the vendored QML (page/WallpaperDetailPanel.qml), so the names of
// the type, properties, signals, roles and section strings must match
// character for character: the section is compared in QML as a string
// (since waywallen 0.3.8 it is "property"/"user", before that
// "Properties"/"User properties").
//
// The layer split here is the same as in the rest of the crate: all
// parsing and all state live in the plain `UserProperties` struct without
// Qt (that is what the golden test checks against the LIVE Python model),
// while the QObject only lays the ready results out by roles and sends
// signals.

/// cxx-qt bridge: a QAbstractListModel subclass.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// The base Qt type: cxx-qt-lib has no QAbstractListModel wrapper, the
        /// bridge declares it itself.
        ///
        /// Warning: here it is declared WITHOUT `#[qobject]`. The reason was found by
        /// the linker: for every `#[qobject]` type in `extern "C++Qt"` cxx-qt emits a
        /// pair of cast shims `cxx_qt_ffi_<Type>_upcastPtr/downcastPtr`, named after
        /// the TYPE, not the bridge. A second bridge in the same crate declaring the
        /// same base gives a `duplicate symbol` on
        /// `cxx_qt_ffi_QAbstractListModel_upcastPtr`. Without `#[qobject]` the shims
        /// are not emitted, while `#[base = QAbstractListModel]`, `#[inherit]` and
        /// `#[cxx_override]` work: they hang on the subclass.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// Set of roles for dataChanged (in Qt6 QVector<int> == QList<int>).
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("rexon_register_we.h");
        /// Translating a plugin label (the schema's `localized_text`) with the
        /// `PluginTranslations` singleton — the same one the original reaches
        /// through `PluginTranslationStore::instance()`. An empty string means "no
        /// translation", and then the label from the schema is shown.
        #[cxx_name = "rexon_we_plugin_translate"]
        fn plugin_translate(message_json: &QString) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // The schema and the overrides come from QML through bindings to the
        // wallpaper's properties — hence WRITE. In 0.9 the flags are written in
        // CAPITALS, and if at least one is given, READ is required explicitly.
        #[qproperty(QString, schema_json, cxx_name = "schemaJson",
                    READ = schema_json, WRITE = set_schema_json,
                    NOTIFY = schema_json_changed)]
        #[qproperty(QString, overrides_json, cxx_name = "overridesJson",
                    READ = overrides_json, WRITE = set_overrides_json,
                    NOTIFY = overrides_json_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        // Both "there is something to reset" flags hang on ONE signal — just as in
        // the original: the section header disables the reset button by them.
        #[qproperty(bool, has_predefined_property_overrides,
                    cxx_name = "hasPredefinedPropertyOverrides",
                    READ = has_predefined_property_overrides,
                    NOTIFY = override_state_changed)]
        #[qproperty(bool, has_user_property_overrides,
                    cxx_name = "hasUserPropertyOverrides",
                    READ = has_user_property_overrides,
                    NOTIFY = override_state_changed)]
        type UserPropertyListModel = super::UserPropertyListModelRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "schemaJsonChanged"]
        fn schema_json_changed(self: Pin<&mut UserPropertyListModel>);
        #[qsignal]
        #[cxx_name = "overridesJsonChanged"]
        fn overrides_json_changed(self: Pin<&mut UserPropertyListModel>);
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut UserPropertyListModel>);
        #[qsignal]
        #[cxx_name = "overrideStateChanged"]
        fn override_state_changed(self: Pin<&mut UserPropertyListModel>);

        /// The user changed a property value. Emitted ONLY by user actions
        /// (setValue): QML debounces it with a timer and sends
        /// WallpaperPropertySetQuery. Setting `overridesJson` programmatically does
        /// NOT raise this signal — otherwise the daemon's answer would loop back
        /// into a write.
        #[qsignal]
        #[cxx_name = "valueChanged"]
        fn value_changed(self: Pin<&mut UserPropertyListModel>, key: QString, value: QString);

        /// Resetting a property to its default value. A separate signal rather than
        /// `valueChanged` with an empty string: since v0.2.5 an empty string is a
        /// VALID value of text properties and must not reset.
        #[qsignal]
        #[cxx_name = "resetRequested"]
        fn reset_requested(self: Pin<&mut UserPropertyListModel>, key: QString);

        /// dataChanged of the base class — a targeted row update.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut UserPropertyListModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    // ── Inherited methods of the base class ───────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Requires a paired end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut UserPropertyListModel>);
        /// # Safety
        /// Paired with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut UserPropertyListModel>);
    }

    // index() is safe to call, but cxx-qt requires safe #[inherit] functions to
    // live in an unsafe block (unsafe here is about matching the declaration to
    // the C++ signature, not about the call being dangerous).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &UserPropertyListModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── QAbstractListModel overrides ──────────────────────────────────────
    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures that moc makes for default arguments, and
    // rowCount() without arguments from JS fails with "Insufficient arguments".
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &UserPropertyListModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &UserPropertyListModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &UserPropertyListModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Property reading/writing ──────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "schemaJson"]
        fn schema_json(self: &UserPropertyListModel) -> QString;
        #[cxx_name = "setSchemaJson"]
        fn set_schema_json(self: Pin<&mut UserPropertyListModel>, value: &QString);

        #[cxx_name = "overridesJson"]
        fn overrides_json(self: &UserPropertyListModel) -> QString;
        #[cxx_name = "setOverridesJson"]
        fn set_overrides_json(self: Pin<&mut UserPropertyListModel>, value: &QString);

        fn count(self: &UserPropertyListModel) -> i32;

        #[cxx_name = "hasPredefinedPropertyOverrides"]
        fn has_predefined_property_overrides(self: &UserPropertyListModel) -> bool;
        #[cxx_name = "hasUserPropertyOverrides"]
        fn has_user_property_overrides(self: &UserPropertyListModel) -> bool;
    }

    // ── API for QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// A value written by the user. An empty string is an ordinary value; it
        /// does not remove the override (reset* exists for that).
        #[qinvokable]
        #[cxx_name = "setValue"]
        fn set_value(self: Pin<&mut UserPropertyListModel>, key: &QString, value: &QString);

        /// Reset all properties to their default values.
        #[qinvokable]
        #[cxx_name = "resetAll"]
        fn reset_all(self: Pin<&mut UserPropertyListModel>);

        /// Reset only the predefined properties (the "Properties" section).
        #[qinvokable]
        #[cxx_name = "resetPredefinedProperties"]
        fn reset_predefined_properties(self: Pin<&mut UserPropertyListModel>);

        /// Reset only the wallpaper's user properties.
        #[qinvokable]
        #[cxx_name = "resetUserProperties"]
        fn reset_user_properties(self: Pin<&mut UserPropertyListModel>);

        /// Role number by name (-1 if there is no such role) — so that the bench
        /// can check which role exactly arrived in dataChanged.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &UserPropertyListModel, name: &QString) -> i32;

        /// A role's value by row number — checks without a delegate.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &UserPropertyListModel, row: i32, name: &QString) -> QVariant;
    }

    impl cxx_qt::Initialize for UserPropertyListModel {}
    // Subscribing to the arrival of label catalogues goes through
    // `CxxQtThread::queue`, and the thread trait must be declared IN THE
    // BRIDGE: cxx-qt emits both `qt_thread()` itself and its type alias from
    // this line.
    impl cxx_qt::Threading for UserPropertyListModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QModelIndex, QString, QStringList, QVariant, QVector};

use serde_json::{Map, Value};
use std::collections::HashMap;

// ============================================================================
// Pure layer: parsing the schema and the override state (no Qt here)
// ============================================================================

/// Key of the predefined "scheme colour" property — it always exists, even
/// when the wallpaper's schema is empty.
pub const SCHEME_COLOR_KEY: &str = "waywallen.scheme_color";
/// Key of the predefined "sound" property — the row appears only if the key
/// is in the schema.
pub const ENABLE_AUDIO_KEY: &str = "waywallen.enable_audio";
/// Key of the predefined "playback speed" property — like sound, the row
/// appears only when the key is in the schema.
pub const PLAYBACK_SPEED_KEY: &str = "waywallen.playback_speed";
/// Key of the predefined "mouse parallax" property (waywallen 0.4.2) — the
/// row appears only when the key is in the schema.
pub const MOUSE_PARALLAX_KEY: &str = "waywallen.mouse_parallax";

/// All predefined keys: parsing of the user properties skips them,
/// otherwise they would end up in the second section twice.
const PREDEFINED_KEYS: [&str; 4] = [
    SCHEME_COLOR_KEY,
    ENABLE_AUDIO_KEY,
    PLAYBACK_SPEED_KEY,
    MOUSE_PARALLAX_KEY,
];

/// Section names. The vendored QML compares the section with a string and
/// chooses the header by it too (`WallpaperDetailPanel.qml`), so the strings
/// are technical and must NOT be translated.
///
/// Warning: waywallen 0.3.8 renamed them from "Properties"/"User properties"
/// to "property"/"user" — exactly the values of
/// [`KIND_PREDEFINED`]/[`KIND_USER`]. The vendor's reason is the same as ours
/// would be: these strings are compared, while the translation
/// `qsTr("Properties")` is shown, and a human-readable string as a key would
/// mean that translation breaks the comparison.
pub const SECTION_PREDEFINED: &str = KIND_PREDEFINED;
pub const SECTION_USER: &str = KIND_USER;
/// Record kind: a predefined property of the daemon or a property of the
/// wallpaper itself.
pub const KIND_PREDEFINED: &str = "property";
pub const KIND_USER: &str = "user";

/// Default scheme colour when the schema has none or it is empty.
const DEFAULT_SCHEME_COLOR: &str = "0.0000 0.0000 0.0000 1.0000";
/// Default playback speed (percent) — the original's fallback value when
/// the schema gave none.
const DEFAULT_PLAYBACK_SPEED: &str = "100";

/// Types for which the details panel has an editor.
const SUPPORTED_TYPES: [&str; 5] = ["color", "slider", "bool", "textinput", "text"];

/// A property editor row. Everything except the current value is computed
/// once when parsing the schema: data() is called on every delegate
/// repaint.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub label: String,
    pub kind_of_editor: String,
    pub section: &'static str,
    pub kind: &'static str,
    pub supported: bool,
    pub min_val: f64,
    pub max_val: f64,
    /// Slider step from the schema (`step`). Zero means "no step": the markup
    /// picks one itself by the size of the maximum.
    pub step_val: f64,
    /// Suffix for the slider number from the schema (`suffix`), for example
    /// "%".
    pub value_suffix: String,
    /// The default value from the schema, converted to wire form.
    pub default_wire: String,
    pub option_labels: Vec<String>,
    pub option_values: Vec<String>,
    /// The plugin label (`localized_text`) as raw JSON: it is translated by the
    /// `PluginTranslations` singleton, not by the pure layer.
    pub localized_label: String,
    /// The same for combo option labels (`localized_label` of an option).
    pub localized_option_labels: Vec<String>,
    /// Sort order of the user properties.
    pub order: f64,
}

/// Model state: the parsed schema + the user's overrides.
#[derive(Debug, Default, Clone)]
pub struct UserProperties {
    entries: Vec<Entry>,
    overrides: HashMap<String, String>,
}

impl UserProperties {
    /// Rebuild the rows from the wallpaper's schema.
    pub fn set_schema(&mut self, schema_json: &str) {
        self.entries = build_entries(schema_json);
    }

    /// Parse the override map. Non-string values are ignored, a broken JSON
    /// gives an empty map (as in the original — silently, without an error in
    /// the UI).
    pub fn set_overrides(&mut self, overrides_json: &str) {
        self.overrides.clear();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(overrides_json) else {
            return;
        };
        for (key, value) in map {
            if let Value::String(text) = value {
                self.overrides.insert(key, text);
            }
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current value of a row: the override if the key IS in the map (an empty
    /// string is a value too), otherwise the default value from the schema.
    pub fn current_value(&self, row: usize) -> &str {
        let Some(entry) = self.entries.get(row) else {
            return "";
        };
        match self.overrides.get(&entry.key) {
            Some(value) => value,
            None => &entry.default_wire,
        }
    }

    /// Whether the colour has an alpha channel — by the number of components of
    /// the wire string.
    pub fn has_alpha(&self, row: usize) -> bool {
        self.current_value(row).split_whitespace().count() >= 4
    }

    /// Whether records of the given kind have at least one override — the
    /// section headers' reset buttons are disabled by this.
    pub fn has_overrides_kind(&self, kind: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.kind == kind && self.overrides.contains_key(&e.key))
    }

    /// Write a value. Returns the number of the row to update; None means there
    /// is no such row (the value is remembered anyway, exactly as in the
    /// original: the override map is wider than the set of rows).
    pub fn set_value(&mut self, key: &str, value: &str) -> Option<usize> {
        self.overrides.insert(key.to_string(), value.to_string());
        self.row_of(key)
    }

    /// Remove the overrides of records of kind `kind` (None — of all).
    /// Returns "row, key" pairs in row order — the original sends dataChanged
    /// and resetRequested in the same order.
    pub fn reset_kind(&mut self, kind: Option<&str>) -> Vec<(usize, String)> {
        let mut reset = Vec::new();
        for (row, entry) in self.entries.iter().enumerate() {
            if let Some(kind) = kind {
                if entry.kind != kind {
                    continue;
                }
            }
            if self.overrides.remove(&entry.key).is_some() {
                reset.push((row, entry.key.clone()));
            }
        }
        reset
    }

    fn row_of(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }
}

/// Python truthiness of a JSON value: empty string/list/dict, zero, false and
/// null are false. The original relies on it (`v.get("text") or key`).
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The `%g` format (six significant digits, trailing zeros removed, an
/// exponent for very large and very small numbers) — the original prints
/// schema numbers with it, and the wire string depends on it: 1234567 must
/// become `1.23457e+06`, not `1234567`.
fn format_g(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    // Scientific notation with 5 digits after the point = exactly 6
    // significant digits; the exponent is taken from it AFTER rounding, as %g
    // requires.
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("научная запись всегда содержит e");
    let exponent: i32 = exponent.parse().unwrap_or(0);
    // The exponent range in which %g prints ordinary rather than scientific
    // notation: from -4 to 6 (exclusive).
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{}{:02}",
            trim_trailing_zeros(mantissa),
            sign,
            exponent.abs()
        )
    } else {
        let decimals = (5 - exponent).max(0) as usize;
        trim_trailing_zeros(&format!("{value:.decimals$}"))
    }
}

fn trim_trailing_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// Colour components are printed with four digits — that is how the daemon
/// expects them.
fn components_wire(items: &[Value]) -> String {
    items
        .iter()
        // The original calls float(x) and crashes on a non-numeric component (a
        // TypeError right in the model's data()). During the port we fix this: a
        // non-numeric component gives 0.0, and a schema with garbage is simply
        // drawn black.
        .map(|item| format!("{:.4}", item.as_f64().unwrap_or(0.0)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// JSON value → wire string (the original's `jsonValueToWireString`).
fn json_wire(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Number(n) => format_g(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => s.clone(),
        Value::Array(items) => components_wire(items),
        _ => String::new(),
    }
}

/// Default value from the schema → wire string taking the type into
/// account (the original's `coerceDefaultWireString`).
fn coerce_default_wire(value: &Value, type_name: &str) -> String {
    if type_name == "color" {
        match value {
            Value::Array(items) => return components_wire(items),
            Value::String(s) => return s.clone(),
            _ => {}
        }
    }
    if type_name == "bool" {
        return if truthy(value) { "true" } else { "false" }.to_string();
    }
    if type_name == "slider" {
        // The original converts to a number only int/float, everything else is 0.0
        // (bool in Python is a subclass of int, so true gives "1").
        let number = match value {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        };
        return format_g(number);
    }
    json_wire(value)
}

/// A number from a schema field with a fallback value. The original writes
/// `float(v.get("max", 1.0) or 1.0)`: a falsy value (zero included!) is
/// replaced with the fallback — a noticeable quirk, a maximum of `0` turns
/// into `1`.
fn number_or(value: Option<&Value>, fallback: f64) -> f64 {
    let Some(value) = value else {
        return fallback;
    };
    if !truthy(value) {
        return fallback;
    }
    match value {
        Value::Number(n) => n.as_f64().unwrap_or(fallback),
        Value::Bool(_) => 1.0,
        // The original calls float("...") and crashes on a non-numeric string;
        // during the port we take the fallback value instead of an exception.
        Value::String(s) => s.trim().parse::<f64>().unwrap_or(fallback),
        _ => fallback,
    }
}

/// Record label: the text from the schema, and if there is none, the key
/// itself.
fn label_or_key(fields: &Map<String, Value>, key: &str) -> String {
    match fields.get("text") {
        Some(value) if truthy(value) => match value {
            Value::String(s) => s.clone(),
            other => json_wire(other),
        },
        _ => key.to_string(),
    }
}

/// Property type in lower case (schemas contain "Slider").
fn type_name(fields: &Map<String, Value>) -> String {
    match fields.get("type") {
        None => String::new(),
        Some(Value::String(s)) => s.to_lowercase(),
        Some(other) => json_wire(other).to_lowercase(),
    }
}

/// An empty field map for schema records that turned out not to be a dict.
fn fields_of(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    }
}

/// Parse the wallpaper's schema into editor rows.
///
/// Row order: first the predefined properties (scheme colour, then sound),
/// then the user ones sorted by the `order` field. The sort is stable — with
/// equal `order` the order stays as the keys lie in the JSON (the
/// `preserve_order` feature of serde_json takes care of that).
pub fn build_entries(schema_json: &str) -> Vec<Entry> {
    let schema = match serde_json::from_str::<Value>(schema_json) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };

    let mut entries = predefined_entries(&schema);
    let mut users = Vec::new();

    for (key, value) in &schema {
        if PREDEFINED_KEYS.contains(&key.as_str()) {
            continue;
        }
        let fields = fields_of(value);
        let editor = type_name(&fields);

        let mut option_labels = Vec::new();
        let mut option_values = Vec::new();
        let mut localized_option_labels = Vec::new();
        if let Some(Value::Array(options)) = fields.get("options") {
            for option in options {
                let option = fields_of(option);
                let value = json_wire(option.get("value").unwrap_or(&Value::Null));
                let label = match option.get("label") {
                    Some(text) if truthy(text) => json_wire(text),
                    _ => value.clone(),
                };
                option_values.push(value);
                option_labels.push(label);
                localized_option_labels.push(localized_of(&option, "localized_label"));
            }
        }

        let supported = SUPPORTED_TYPES.contains(&editor.as_str())
            || (editor == "combo" && !option_values.is_empty());

        users.push(Entry {
            label: label_or_key(&fields, key),
            section: SECTION_USER,
            kind: KIND_USER,
            supported,
            min_val: number_or(fields.get("min"), 0.0),
            max_val: number_or(fields.get("max"), 1.0),
            default_wire: coerce_default_wire(
                fields.get("value").unwrap_or(&Value::Null),
                &editor,
            ),
            option_labels,
            option_values,
            localized_label: localized_of(&fields, "localized_text"),
            localized_option_labels,
            order: number_or(fields.get("order"), 0.0),
            step_val: step_of(&fields),
            value_suffix: text_of(&fields, "suffix"),
            kind_of_editor: editor,
            key: key.clone(),
        });
    }

    users.sort_by(|a, b| a.order.total_cmp(&b.order));
    entries.extend(users);
    entries
}

/// Predefined properties of the daemon — they come not from the wallpaper's
/// schema but from the protocol.
///
/// Warning: the numeric fields are taken FROM THE SCHEMA, not invented: for
/// "playback speed" the daemon sends `min: 10`, `max: 400`, `step: 10` and
/// `suffix: "%"`, and without them the slider would go from 0 to 1 without
/// percent.
fn predefined_entries(schema: &Map<String, Value>) -> Vec<Entry> {
    let make = |key: &str,
                fields: &Map<String, Value>,
                label: &str,
                editor: &str,
                default_wire: &str| {
        let mut kind_of_editor = type_name(fields);
        if kind_of_editor.is_empty() {
            kind_of_editor = editor.to_string();
        }
        let mut wire = coerce_default_wire(
            fields.get("value").unwrap_or(&Value::Null),
            &kind_of_editor,
        );
        if wire.is_empty() {
            wire = default_wire.to_string();
        }
        Entry {
            key: key.to_string(),
            label: label_or_key_default(fields, label),
            supported: SUPPORTED_TYPES.contains(&kind_of_editor.as_str()),
            kind_of_editor,
            section: SECTION_PREDEFINED,
            kind: KIND_PREDEFINED,
            min_val: number_or(fields.get("min"), 0.0),
            max_val: number_or(fields.get("max"), 1.0),
            step_val: step_of(fields),
            value_suffix: text_of(fields, "suffix"),
            default_wire: wire,
            option_labels: Vec::new(),
            option_values: Vec::new(),
            localized_label: localized_of(fields, "localized_text"),
            localized_option_labels: Vec::new(),
            order: 0.0,
        }
    };

    let scheme = fields_of(schema.get(SCHEME_COLOR_KEY).unwrap_or(&Value::Null));
    let mut entries = vec![make(
        SCHEME_COLOR_KEY,
        &scheme,
        "Scheme color",
        "color",
        DEFAULT_SCHEME_COLOR,
    )];

    // The sound row appears only if the key is in the schema: a wallpaper
    // without sound must not show a switch with nothing to switch off.
    if schema.contains_key(ENABLE_AUDIO_KEY) {
        let audio = fields_of(schema.get(ENABLE_AUDIO_KEY).unwrap_or(&Value::Null));
        entries.push(make(ENABLE_AUDIO_KEY, &audio, "Enable audio", "bool", "true"));
    }

    // Playback speed — in the same order and under the same conditions.
    if schema.contains_key(PLAYBACK_SPEED_KEY) {
        let speed = fields_of(schema.get(PLAYBACK_SPEED_KEY).unwrap_or(&Value::Null));
        entries.push(make(
            PLAYBACK_SPEED_KEY,
            &speed,
            "Playback speed",
            "slider",
            DEFAULT_PLAYBACK_SPEED,
        ));
    }

    // waywallen 0.4.2 (`user_property_model.cpp`): mouse parallax — a switch,
    // on by default, in the same order after the speed.
    if schema.contains_key(MOUSE_PARALLAX_KEY) {
        let parallax = fields_of(schema.get(MOUSE_PARALLAX_KEY).unwrap_or(&Value::Null));
        entries.push(make(MOUSE_PARALLAX_KEY, &parallax, "Mouse parallax", "bool", "true"));
    }
    entries
}

/// Slider step from the schema (`step`). A missing or non-numeric field
/// gives zero — the vendor's markup treats it as "no step".
fn step_of(fields: &Map<String, Value>) -> f64 {
    match fields.get("step") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// A string field of the schema. A non-string value is an empty string,
/// exactly as with the original's `QJsonValue::toString()`.
fn text_of(fields: &Map<String, Value>, name: &str) -> String {
    match fields.get(name) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The plugin label (`{pluginId, msgid}`) as raw JSON. Empty when the field
/// is missing or is not an object: then there is nothing to translate.
fn localized_of(fields: &Map<String, Value>, name: &str) -> String {
    match fields.get(name) {
        Some(value @ Value::Object(map)) if !map.is_empty() => value.to_string(),
        _ => String::new(),
    }
}

/// The same as label_or_key, but the fallback is not the key but the
/// protocol's ready label (the key of predefined properties is not shown to
/// the user).
fn label_or_key_default(fields: &Map<String, Value>, fallback: &str) -> String {
    match fields.get("text") {
        Some(value) if truthy(value) => match value {
            Value::String(s) => s.clone(),
            other => json_wire(other),
        },
        _ => fallback.to_string(),
    }
}

// ============================================================================
// Qt layer
// ============================================================================

/// Roles start at Qt::UserRole + 1 (Qt::UserRole == 256) — the same numbers
/// as in the Python model and in the original.
const ROLE_BASE: i32 = 256 + 1;

/// Role names in number order — the single source of truth for roleNames,
/// data() and roleNumber().
/// Warning: the role order is the same as the vendor's `Roles` enum
/// (`ui/src/model/user_property_model.cppm`), and it also sets their
/// numbers. The `stepVal` and `valueSuffix` roles appeared at the vendor
/// together with the "playback speed" slider; the details panel delegate
/// declares them as `required property`, and such a delegate without the
/// role is simply NOT CREATED — both property sections then disappear
/// entirely.
const ROLE_NAMES: [&str; 14] = [
    "key",
    "label",
    "type",
    "supported",
    "minVal",
    "maxVal",
    "stepVal",
    "valueSuffix",
    "currentValue",
    "hasAlpha",
    "optionLabels",
    "optionValues",
    "section",
    "kind",
];

/// Indices of the roles that depend on the override: exactly they go into
/// dataChanged when a value is edited. The other roles are taken from the
/// schema and do not change.
const ROLE_CURRENT_VALUE: usize = 8;
const ROLE_HAS_ALPHA: usize = 9;

/// Indices of the roles that depend on the LABEL CATALOGUE rather than the
/// schema: the row label and the combo option labels. Both are computed in
/// `data()` through `PluginTranslations`, so the arrival of catalogues must
/// update them.
const ROLE_LABEL: usize = 1;
const ROLE_OPTION_LABELS: usize = 10;

/// Rust state of the model.
#[derive(Default)]
pub struct UserPropertyListModelRust {
    schema_json: String,
    overrides_json: String,
    props: UserProperties,
    /// Subscription number for the plugin label catalogues; `0` means not
    /// subscribed.
    translations_watch: u64,
}

impl Drop for UserPropertyListModelRust {
    fn drop(&mut self) {
        crate::plugin_translations::subscribers::unwatch(self.translations_watch);
    }
}

/// Role set "the current value changed" — two roles, not an empty vector.
/// An empty one would mean "ALL roles changed", and QML would recompute the
/// whole delegate.
fn current_value_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_CURRENT_VALUE as i32);
    roles.append(ROLE_BASE + ROLE_HAS_ALPHA as i32);
    roles
}

/// Role set "a label catalogue arrived" — also two, and also not empty: the
/// property value is not touched on a language change, and there is no
/// reason to poke the whole delegate because of a label.
fn label_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_LABEL as i32);
    roles.append(ROLE_BASE + ROLE_OPTION_LABELS as i32);
    roles
}

impl cxx_qt::Initialize for qobject::UserPropertyListModel {
    /// Subscription to the plugin label catalogues.
    ///
    /// ⭐ Without it the property panel showed the customer "Enable audio" and
    /// "Playback speed" in English in a Russian window: the daemon gives the
    /// labels of these predefined properties as a `{pluginId, msgid}` pair, the
    /// translation is looked up in the catalogues, and the catalogues are loaded
    /// LAZILY — the panel delegate itself orders them with its very first
    /// question. The answer arrives later, the `label` role has already been
    /// read by then, and without a push the English words stayed on screen
    /// until the model was rebuilt.
    fn initialize(mut self: Pin<&mut Self>) {
        let watch = crate::plugin_translations::subscribers::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::UserPropertyListModel::on_translations_changed,
        );
        self.as_mut().rust_mut().translations_watch = watch;
    }
}

impl qobject::UserPropertyListModel {
    // ── Properties ────────────────────────────────────────────────────────
    fn schema_json(&self) -> QString {
        QString::from(&self.schema_json)
    }

    fn overrides_json(&self) -> QString {
        QString::from(&self.overrides_json)
    }

    fn count(&self) -> i32 {
        self.props.len() as i32
    }

    fn has_predefined_property_overrides(&self) -> bool {
        self.props.has_overrides_kind(KIND_PREDEFINED)
    }

    fn has_user_property_overrides(&self) -> bool {
        self.props.has_overrides_kind(KIND_USER)
    }

    /// A schema change = a full replacement of the content, hence a reset
    /// rather than insert/remove: the number of rows may become anything.
    fn set_schema_json(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value == self.schema_json {
            return;
        }
        self.as_mut().rust_mut().schema_json = value.clone();
        self.as_mut().schema_json_changed();

        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().props.set_schema(&value);
            self.as_mut().end_reset_model();
        }
        self.as_mut().count_changed();
        self.as_mut().override_state_changed();
    }

    /// The overrides come from the daemon as a whole in one string. Rows are not
    /// added or removed — only the two value-dependent fields change, so a
    /// reset is not needed, dataChanged over a range is enough.
    fn set_overrides_json(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value == self.overrides_json {
            return;
        }
        {
            let mut state = self.as_mut().rust_mut();
            state.overrides_json = value.clone();
        }
        self.as_mut().overrides_json_changed();
        self.as_mut().rust_mut().props.set_overrides(&value);

        let rows = self.props.len();
        if rows > 0 {
            let roles = current_value_roles();
            let first = self.index(0, 0, &QModelIndex::default());
            let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
            self.as_mut().data_changed(&first, &last, &roles);
        }
        self.as_mut().override_state_changed();
    }

    /// Label catalogues arrived (or the language changed) — re-read the labels.
    ///
    /// The rows are the same and in the same order, only the text of two roles
    /// changes, so a reset is not needed: `dataChanged` over the whole range is
    /// enough.
    pub fn on_translations_changed(mut self: Pin<&mut Self>) {
        let rows = self.props.len();
        if rows == 0 {
            return;
        }
        let roles = label_roles();
        let first = self.index(0, 0, &QModelIndex::default());
        let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
        self.as_mut().data_changed(&first, &last, &roles);
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // In a flat list only the root has children.
        if parent.is_valid() {
            0
        } else {
            self.props.len() as i32
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        for (i, name) in ROLE_NAMES.iter().enumerate() {
            names.insert(ROLE_BASE + i as i32, QByteArray::from(*name));
        }
        names
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if !index.is_valid() || row < 0 || row as usize >= self.props.len() {
            return QVariant::default();
        }
        let row = row as usize;
        let entry = &self.props.entries()[row];
        let Ok(slot) = usize::try_from(role - ROLE_BASE) else {
            return QVariant::default();
        };
        match ROLE_NAMES.get(slot).copied() {
            Some("key") => QVariant::from(&QString::from(&entry.key)),
            Some("label") => {
                let translated = plugin_label(&entry.localized_label);
                QVariant::from(&QString::from(if translated.is_empty() {
                    &entry.label
                } else {
                    &translated
                }))
            }
            Some("type") => QVariant::from(&QString::from(&entry.kind_of_editor)),
            Some("supported") => QVariant::from(&entry.supported),
            Some("minVal") => QVariant::from(&entry.min_val),
            Some("maxVal") => QVariant::from(&entry.max_val),
            Some("stepVal") => QVariant::from(&entry.step_val),
            Some("valueSuffix") => QVariant::from(&QString::from(&entry.value_suffix)),
            Some("currentValue") => {
                QVariant::from(&QString::from(self.props.current_value(row)))
            }
            Some("hasAlpha") => QVariant::from(&self.props.has_alpha(row)),
            Some("optionLabels") => {
                let mut labels = entry.option_labels.clone();
                for (index, label) in labels.iter_mut().enumerate() {
                    let Some(localized) = entry.localized_option_labels.get(index) else {
                        break;
                    };
                    let translated = plugin_label(localized);
                    if !translated.is_empty() {
                        *label = translated;
                    }
                }
                QVariant::from(&string_list(&labels))
            }
            Some("optionValues") => QVariant::from(&string_list(&entry.option_values)),
            Some("section") => QVariant::from(&QString::from(entry.section)),
            Some("kind") => QVariant::from(&QString::from(entry.kind)),
            _ => QVariant::default(),
        }
    }

    // ── API for QML ───────────────────────────────────────────────────────
    fn set_value(mut self: Pin<&mut Self>, key: &QString, value: &QString) {
        let key = key.to_string();
        let value = value.to_string();
        let row = self.as_mut().rust_mut().props.set_value(&key, &value);
        if let Some(row) = row {
            self.as_mut().notify_current_value(row);
        }
        self.as_mut().override_state_changed();
        // valueChanged goes last: QML reads the already updated model state in the
        // handler.
        self.as_mut()
            .value_changed(QString::from(&key), QString::from(&value));
    }

    fn reset_all(self: Pin<&mut Self>) {
        self.reset_kind(None);
    }

    fn reset_predefined_properties(self: Pin<&mut Self>) {
        self.reset_kind(Some(KIND_PREDEFINED));
    }

    fn reset_user_properties(self: Pin<&mut Self>) {
        self.reset_kind(Some(KIND_USER));
    }

    fn role_number(&self, name: &QString) -> i32 {
        let name = name.to_string();
        ROLE_NAMES
            .iter()
            .position(|n| *n == name)
            .map(|i| ROLE_BASE + i as i32)
            .unwrap_or(-1)
    }

    fn role_value(&self, row: i32, name: &QString) -> QVariant {
        let role = self.role_number(name);
        if role < 0 {
            return QVariant::default();
        }
        let index = self.index(row, 0, &QModelIndex::default());
        self.data(&index, role)
    }

    // ── Internals ─────────────────────────────────────────────────────────
    /// A targeted notification about a change of one row's value.
    fn notify_current_value(mut self: Pin<&mut Self>, row: usize) {
        let roles = current_value_roles();
        let index = self.index(row as i32, 0, &QModelIndex::default());
        self.as_mut().data_changed(&index, &index, &roles);
    }

    fn reset_kind(mut self: Pin<&mut Self>, kind: Option<&str>) {
        let reset = self.as_mut().rust_mut().props.reset_kind(kind);
        for (row, key) in reset {
            self.as_mut().notify_current_value(row);
            // The reset goes out as a separate signal: QML sends the daemon reset=true,
            // not a write of an empty string.
            self.as_mut().reset_requested(QString::from(&key));
        }
        self.as_mut().override_state_changed();
    }
}

/// Translating a plugin label by its raw JSON. An empty input gives an
/// empty output: most properties have no `localized_text` in the schema at
/// all.
fn plugin_label(localized_json: &str) -> String {
    if localized_json.is_empty() {
        return String::new();
    }
    qobject::plugin_translate(&QString::from(localized_json)).to_string()
}

/// A string list for list roles. A separate model for combo options is not
/// needed: QML reads them as an ordinary array (`optionValues[index]`).
fn string_list(items: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for item in items {
        list.append(QString::from(item));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden file was captured from the LIVE Python model (see
    /// tests/golden/user_property_cases.py). Here it is run through the port's
    /// pure layer: rows, role values, override flags and — above all — signals
    /// (which rows and in what order get into dataChanged/resetRequested).
    #[derive(serde::Deserialize)]
    struct Golden {
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        #[serde(rename = "schemaJson")]
        schema_json: String,
        #[serde(rename = "overridesJson")]
        overrides_json: String,
        actions: Vec<Action>,
        count: usize,
        rows: Vec<Row>,
        #[serde(rename = "hasPredefinedPropertyOverrides")]
        has_predefined: bool,
        #[serde(rename = "hasUserPropertyOverrides")]
        has_user: bool,
        #[serde(rename = "dataChanged")]
        data_changed: Vec<DataChanged>,
        #[serde(rename = "valueChanged")]
        value_changed: Vec<ValueChanged>,
        #[serde(rename = "resetRequested")]
        reset_requested: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct Action {
        op: String,
        #[serde(default)]
        key: String,
        #[serde(default)]
        value: String,
    }

    #[derive(serde::Deserialize)]
    struct Row {
        key: String,
        label: String,
        #[serde(rename = "type")]
        editor: String,
        supported: bool,
        #[serde(rename = "minVal")]
        min_val: f64,
        #[serde(rename = "maxVal")]
        max_val: f64,
        #[serde(rename = "currentValue")]
        current_value: String,
        #[serde(rename = "hasAlpha")]
        has_alpha: bool,
        #[serde(rename = "optionLabels")]
        option_labels: Vec<String>,
        #[serde(rename = "optionValues")]
        option_values: Vec<String>,
        section: String,
        kind: String,
    }

    #[derive(serde::Deserialize)]
    struct DataChanged {
        first: usize,
        last: usize,
        roles: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct ValueChanged {
        key: String,
        value: String,
    }

    /// What the port "would have emitted" as signals: the same events the
    /// golden file captures from the Python model, but assembled from the pure
    /// layer's return values.
    #[derive(Default)]
    struct Emitted {
        data_changed: Vec<(usize, usize, Vec<String>)>,
        value_changed: Vec<(String, String)>,
        reset_requested: Vec<String>,
    }

    const VALUE_ROLES: [&str; 2] = ["currentValue", "hasAlpha"];

    /// Section name of the Python-era reference → the waywallen 0.3.8 section
    /// name.
    fn section_of(golden: &str) -> &'static str {
        match golden {
            "Properties" => SECTION_PREDEFINED,
            "User properties" => SECTION_USER,
            other => panic!("незнакомая секция эталона: {other}"),
        }
    }

    fn replay(case: &Case) -> (UserProperties, Emitted) {
        let mut props = UserProperties::default();
        let mut out = Emitted::default();

        // Both assignments go through the "value did not change" check — it lives
        // in set_schema_json/set_overrides_json and is visible in the golden file:
        // in the case with an empty schema the model stays without any rows at
        // all, because an empty string equals the initial property value.
        if !case.schema_json.is_empty() {
            props.set_schema(&case.schema_json);
        }
        // The golden file captures signals starting with setting the overrides:
        // building the schema goes through a model reset, not through dataChanged.
        if !case.overrides_json.is_empty() {
            props.set_overrides(&case.overrides_json);
            if !props.is_empty() {
                out.data_changed.push((
                    0,
                    props.len() - 1,
                    VALUE_ROLES.iter().map(|s| s.to_string()).collect(),
                ));
            }
        }

        for action in &case.actions {
            match action.op.as_str() {
                "setValue" => {
                    if let Some(row) = props.set_value(&action.key, &action.value) {
                        out.data_changed.push((
                            row,
                            row,
                            VALUE_ROLES.iter().map(|s| s.to_string()).collect(),
                        ));
                    }
                    out.value_changed
                        .push((action.key.clone(), action.value.clone()));
                }
                op => {
                    let kind = match op {
                        "resetAll" => None,
                        "resetPredefinedProperties" => Some(KIND_PREDEFINED),
                        "resetUserProperties" => Some(KIND_USER),
                        other => panic!("неизвестное действие голдена: {other}"),
                    };
                    for (row, key) in props.reset_kind(kind) {
                        out.data_changed.push((
                            row,
                            row,
                            VALUE_ROLES.iter().map(|s| s.to_string()).collect(),
                        ));
                        out.reset_requested.push(key);
                    }
                }
            }
        }
        (props, out)
    }

    #[test]
    fn golden_parity_with_python_model() {
        let golden: Golden =
            serde_json::from_str(include_str!("../tests/golden/user_property_cases.json"))
                .expect("голден читается");
        assert!(!golden.cases.is_empty());

        for case in &golden.cases {
            let name = &case.name;
            let (props, emitted) = replay(case);

            assert_eq!(props.len(), case.count, "{name}: число строк");
            assert_eq!(props.len(), case.rows.len(), "{name}: длина голдена строк");

            for (row, expected) in case.rows.iter().enumerate() {
                let entry = &props.entries()[row];
                let at = format!("{name}: строка {row} ({})", expected.key);
                assert_eq!(entry.key, expected.key, "{at}: key");
                assert_eq!(entry.label, expected.label, "{at}: label");
                assert_eq!(entry.kind_of_editor, expected.editor, "{at}: type");
                assert_eq!(entry.supported, expected.supported, "{at}: supported");
                assert_eq!(entry.min_val, expected.min_val, "{at}: minVal");
                assert_eq!(entry.max_val, expected.max_val, "{at}: maxVal");
                // Warning: the reference was captured from the LIVE Python model of the
                // waywallen ≤0.3.7 era, where the section was named with a human-readable
                // string. Since 0.3.8 the vendor renamed it to a technical key and learned
                // to translate the header itself — so the reference is converted to the new
                // names rather than rewritten by hand.
                assert_eq!(
                    entry.section,
                    section_of(&expected.section),
                    "{at}: section"
                );
                assert_eq!(entry.kind, expected.kind, "{at}: kind");
                assert_eq!(
                    entry.option_labels, expected.option_labels,
                    "{at}: optionLabels"
                );
                assert_eq!(
                    entry.option_values, expected.option_values,
                    "{at}: optionValues"
                );
                assert_eq!(
                    props.current_value(row),
                    expected.current_value,
                    "{at}: currentValue"
                );
                assert_eq!(props.has_alpha(row), expected.has_alpha, "{at}: hasAlpha");
            }

            assert_eq!(
                props.has_overrides_kind(KIND_PREDEFINED),
                case.has_predefined,
                "{name}: hasPredefinedPropertyOverrides"
            );
            assert_eq!(
                props.has_overrides_kind(KIND_USER),
                case.has_user,
                "{name}: hasUserPropertyOverrides"
            );

            let expected_dc: Vec<(usize, usize, Vec<String>)> = case
                .data_changed
                .iter()
                .map(|d| (d.first, d.last, d.roles.clone()))
                .collect();
            assert_eq!(emitted.data_changed, expected_dc, "{name}: dataChanged");

            let expected_vc: Vec<(String, String)> = case
                .value_changed
                .iter()
                .map(|v| (v.key.clone(), v.value.clone()))
                .collect();
            assert_eq!(emitted.value_changed, expected_vc, "{name}: valueChanged");
            assert_eq!(
                emitted.reset_requested, case.reset_requested,
                "{name}: resetRequested"
            );
        }
    }

    #[test]
    fn format_g_matches_python() {
        // The values below are the output of `"%g" % v` in Python (checked by hand
        // on the same numbers that appear in wallpaper schemas).
        for (value, expected) in [
            (0.0, "0"),
            (0.5, "0.5"),
            (1.0, "1"),
            (100.0, "100"),
            (2.5, "2.5"),
            (1234567.0, "1.23457e+06"),
            (0.00001, "1e-05"),
            (0.0001, "0.0001"),
            (-0.25, "-0.25"),
            (123456.0, "123456"),
            (1.0 / 3.0, "0.333333"),
        ] {
            assert_eq!(format_g(value), expected, "%g для {value}");
        }
    }

    #[test]
    fn empty_override_is_a_value_not_a_reset() {
        // Regression of v0.2.5: an empty string is a valid value of a text
        // property, and it must NOT mean "return to the default value".
        let mut props = UserProperties::default();
        props.set_schema(r#"{"caption": {"type": "textinput", "value": "по умолчанию"}}"#);
        let row = props.entries().len() - 1;
        assert_eq!(props.current_value(row), "по умолчанию");
        props.set_value("caption", "");
        assert_eq!(props.current_value(row), "");
        assert!(props.has_overrides_kind(KIND_USER));
        props.reset_kind(Some(KIND_USER));
        assert_eq!(props.current_value(row), "по умолчанию");
    }

    /// Guard of the contract with the vendored markup.
    ///
    /// WHY. The details panel delegate declares its fields as `required
    /// property`, and a role the model does not have is not "an empty value"
    /// but a refusal to CREATE the delegate: both property sections disappear
    /// from the screen entirely, silently. That is exactly what happened with
    /// `stepVal`/`valueSuffix`: they came with the vendored QML v0.3.5, the
    /// model did not learn about them, and the property panel was empty since
    /// 18.08.2026. The check takes the list straight from the vendored file, so
    /// the next such field will be caught by `cargo test`, not on a live window.
    #[test]
    fn delegate_required_properties_are_covered_by_roles() {
        const PANEL_QML: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/qml/page/",
            "WallpaperDetailPanel.qml"
        ));

        // The property model's delegate is recognized by its `id`: the substring
        // `delegate: ColumnLayout` also occurs in the SECTION delegate
        // (`section.delegate:`), while that declaration exists exactly once.
        let start = PANEL_QML
            .find("id: m_prop_delegate")
            .expect("в панели деталей обязан быть делегат свойств");
        let block = &PANEL_QML[start..];
        // waywallen 0.6.0: the delegate gets a single `required property
        // MD.LazyRow row` (the source is `MD.ItemModelSource` over our model) and
        // reads the roles as `row.value.<role>`. Before 0.6.0 every role was its
        // own `required property`. We understand both forms.
        let mut required: Vec<&str> = Vec::new();
        for line in block.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("required property ") {
                let name = rest
                    .split_whitespace()
                    .nth(1)
                    .expect("у объявления есть тип и имя");
                if name != "row" {
                    required.push(name);
                }
                continue;
            }
            if let Some((_, tail)) = line.split_once(": row.value.") {
                let name = tail
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    required.push(name);
                }
                continue;
            }
            if !required.is_empty() && !line.is_empty() && !line.starts_with("readonly property") {
                // The declarations come in one piece: the very first line after them
                // closes the list.
                break;
            }
        }
        assert!(
            required.len() >= 10,
            "объявлений required property найдено {} — разбор разъехался с \
             разметкой",
            required.len()
        );
        for name in required {
            assert!(
                ROLE_NAMES.contains(&name),
                "делегат вендора требует роль `{name}`, а модель её не отдаёт: \
                 делегат не создастся и секции свойств исчезнут"
            );
        }
    }

    /// The three predefined properties of the daemon — the same as the vendor's
    /// (`isPredefinedKey`): scheme colour, sound, playback speed.
    #[test]
    fn playback_speed_is_a_predefined_row_with_schema_limits() {
        let mut props = UserProperties::default();
        props.set_schema(
            r#"{"waywallen.scheme_color": {"type": "color", "value": "0.1 0.2 0.3"},
                "waywallen.enable_audio": {"type": "bool", "value": true},
                "waywallen.playback_speed": {"text": "Playback speed",
                                             "type": "slider", "value": 100,
                                             "min": 10, "max": 400, "step": 10,
                                             "suffix": "%"},
                "brightness": {"type": "slider", "value": 0.5, "order": 1}}"#,
        );
        let keys: Vec<&str> = props.entries().iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                SCHEME_COLOR_KEY,
                ENABLE_AUDIO_KEY,
                PLAYBACK_SPEED_KEY,
                "brightness"
            ],
            "предопределённые идут первыми и не дублируются в секции обоины"
        );
        let speed = &props.entries()[2];
        assert_eq!(speed.kind, KIND_PREDEFINED);
        assert_eq!(speed.min_val, 10.0);
        assert_eq!(speed.max_val, 400.0);
        assert_eq!(speed.step_val, 10.0);
        assert_eq!(speed.value_suffix, "%");
        assert_eq!(props.current_value(2), "100");
    }

    #[test]
    fn role_names_are_unique_and_indexed() {
        let mut seen = std::collections::HashSet::new();
        for name in ROLE_NAMES {
            assert!(seen.insert(name), "роль {name} объявлена дважды");
        }
        assert_eq!(ROLE_NAMES[ROLE_CURRENT_VALUE], "currentValue");
        assert_eq!(ROLE_NAMES[ROLE_HAS_ALPHA], "hasAlpha");
    }
}
