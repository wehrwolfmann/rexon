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
        /// waywallen 0.6.2: the wallpaper the rows belong to; a new one unfolds
        /// every group again.
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId",
                    READ = wallpaper_id, WRITE = set_wallpaper_id,
                    NOTIFY = wallpaper_id_changed)]
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
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut UserPropertyListModel>);
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
        /// # Safety
        /// Requires a paired end_insert_rows.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut UserPropertyListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Paired with begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut UserPropertyListModel>);
        /// # Safety
        /// Requires a paired end_remove_rows.
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut UserPropertyListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Paired with begin_remove_rows.
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut UserPropertyListModel>);
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

        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &UserPropertyListModel) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut UserPropertyListModel>, value: &QString);

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

        /// The overrides snapshot (also the property setter). waywallen 0.6.2
        /// calls it directly after every successful read: the same snapshot
        /// still replaces optimistic edits, so it is re-parsed even when equal.
        #[qinvokable]
        #[cxx_name = "setOverridesJson"]
        fn set_overrides_json(self: Pin<&mut UserPropertyListModel>, value: &QString);

        /// waywallen 0.6.2: fold or unfold a group header.
        #[qinvokable]
        #[cxx_name = "setGroupExpanded"]
        fn set_group_expanded(self: Pin<&mut UserPropertyListModel>, key: &QString, expanded: bool);

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
    /// waywallen 0.6.2: the `group` row this property belongs to (empty — none);
    /// a group header carries its own key.
    pub group_key: String,
    /// waywallen 0.6.2: the visibility condition (`condition` of the schema).
    pub condition: Condition,
    /// waywallen 0.6.2: the schema default as JSON (`None` — absent), what a
    /// condition compares against while there is no override.
    pub default_value: Option<Value>,
    /// waywallen 0.6.2: the raw combo options — a condition sees the option's own
    /// value, not its wire string.
    pub options: Vec<Value>,
    /// waywallen 0.6.2: digits after the point for the slider number.
    pub precision: i32,
}

/// A group header or a text label: neither has a value to write or reset.
fn is_value_property(editor: &str) -> bool {
    editor != "group" && editor != "text"
}

/// Model state: the parsed schema + the user's overrides.
#[derive(Debug, Default, Clone)]
pub struct UserProperties {
    entries: Vec<Entry>,
    overrides: HashMap<String, String>,
    /// Groups the person folded (waywallen 0.6.2). Hidden rows keep their values
    /// and take part in resets.
    collapsed: std::collections::HashSet<String>,
    /// Model row → entry index: only the rows that are shown.
    visible: Vec<usize>,
}

/// One step of bringing the shown rows to a new set: the Qt layer wraps each in
/// begin/endRemoveRows or begin/endInsertRows, so delegates of rows that stay
/// (a slider being dragged) are not recreated.
#[derive(Debug, Clone, PartialEq)]
pub enum RowOp {
    Remove { first: usize, last: usize },
    Insert { first: usize, entries: Vec<usize> },
}

impl UserProperties {
    /// Rebuild the rows from the wallpaper's schema. Folded groups that are gone
    /// are forgotten, the rest stay folded.
    pub fn set_schema(&mut self, schema_json: &str) {
        self.entries = build_entries(schema_json);
        let groups: std::collections::HashSet<&str> = self
            .entries
            .iter()
            .filter(|e| e.kind_of_editor == "group")
            .map(|e| e.key.as_str())
            .collect();
        self.collapsed.retain(|k| groups.contains(k.as_str()));
        self.visible = self.compute_visible();
    }

    /// Shown rows → entry indices.
    pub fn visible(&self) -> &[usize] {
        &self.visible
    }

    /// Entry index of a model row.
    pub fn entry_of_row(&self, row: usize) -> Option<usize> {
        self.visible.get(row).copied()
    }

    /// Model row of a key, if shown.
    pub fn row_of_key(&self, key: &str) -> Option<usize> {
        self.visible.iter().position(|&i| self.entries[i].key == key)
    }

    /// Whether a group header is unfolded.
    pub fn expanded(&self, entry: usize) -> bool {
        self.entries
            .get(entry)
            .is_some_and(|e| e.kind_of_editor == "group" && !self.collapsed.contains(&e.key))
    }

    pub fn has_collapsed(&self) -> bool {
        !self.collapsed.is_empty()
    }

    /// A new wallpaper: every group is unfolded again. `true` — something was
    /// folded, the rows must be rebuilt (a model reset).
    pub fn clear_collapsed(&mut self) -> bool {
        if self.collapsed.is_empty() {
            return false;
        }
        self.collapsed.clear();
        self.visible = self.compute_visible();
        true
    }

    /// Fold or unfold a group. `false` — nothing to do (no such group, or it is
    /// already so).
    pub fn set_group_expanded(&mut self, key: &str, expanded: bool) -> bool {
        if expanded == !self.collapsed.contains(key) {
            return false;
        }
        if !self.entries.iter().any(|e| e.key == key && e.kind_of_editor == "group") {
            return false;
        }
        if expanded {
            self.collapsed.remove(key);
        } else {
            self.collapsed.insert(key.to_string());
        }
        true
    }

    /// The value a condition sees for an entry — the original's
    /// `conditionValueFor_`.
    fn condition_value(&self, e: &Entry) -> CondValue {
        let Some(wire) = self.overrides.get(&e.key) else {
            return e.default_value.as_ref().map_or(CondValue::Undefined, CondValue::from_json);
        };
        if e.kind_of_editor == "combo" {
            for option in &e.options {
                let value = option.get("value").cloned().unwrap_or(Value::Null);
                if json_wire(&value) == *wire {
                    return CondValue::from_json(&value);
                }
            }
        }
        if e.kind_of_editor == "bool" || matches!(e.default_value, Some(Value::Bool(_))) {
            return match wire.as_str() {
                "true" => CondValue::Bool(true),
                "false" => CondValue::Bool(false),
                _ => CondValue::Undefined,
            };
        }
        if e.kind_of_editor == "slider" || matches!(e.default_value, Some(Value::Number(_))) {
            return match wire.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => CondValue::Num(n),
                _ => CondValue::Undefined,
            };
        }
        CondValue::Str(wire.clone())
    }

    /// The rows to show — the original's `visibleRows_`: a row whose condition
    /// fails is hidden; a group is shown only if it passes and has a shown child;
    /// children of a hidden or folded group are hidden.
    pub fn compute_visible(&self) -> Vec<usize> {
        let values: HashMap<String, CondValue> =
            self.entries.iter().map(|e| (e.key.clone(), self.condition_value(e))).collect();
        let included: Vec<bool> = self.entries.iter().map(|e| e.condition.matches(&values)).collect();
        let mut shown_groups = std::collections::HashSet::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.kind_of_editor != "group" || !included[i] {
                continue;
            }
            for (child, c) in self.entries.iter().enumerate().skip(i + 1) {
                if c.kind_of_editor == "group" {
                    break;
                }
                if included[child] {
                    shown_groups.insert(e.key.clone());
                    break;
                }
            }
        }
        let mut rows = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if !included[i] {
                continue;
            }
            if e.kind_of_editor == "group" {
                if !shown_groups.contains(&e.key) {
                    continue;
                }
            } else if !e.group_key.is_empty()
                && (!shown_groups.contains(&e.group_key) || self.collapsed.contains(&e.group_key))
            {
                continue;
            }
            rows.push(i);
        }
        rows
    }

    /// Steps from the shown rows to [`compute_visible`](Self::compute_visible) —
    /// the original's `updateVisibleRows_`: removals from the end first, then
    /// insertions. Applying each step to `visible()` as it is returned is the
    /// caller's job ([`apply_row_op`](Self::apply_row_op)).
    pub fn visible_ops(&self) -> Vec<RowOp> {
        row_ops(&self.visible, &self.compute_visible())
    }

    pub fn apply_row_op(&mut self, op: &RowOp) {
        match op {
            RowOp::Remove { first, last } => {
                self.visible.drain(*first..=*last);
            }
            RowOp::Insert { first, entries } => {
                for (k, e) in entries.iter().enumerate() {
                    self.visible.insert(first + k, *e);
                }
            }
        }
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

    /// Number of SHOWN rows.
    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    /// Current value of an entry (by entry index): the override if the key IS in
    /// the map (an empty string is a value too), otherwise the default value from
    /// the schema. A group header has no value.
    pub fn current_value(&self, row: usize) -> &str {
        let Some(entry) = self.entries.get(row) else {
            return "";
        };
        if entry.kind_of_editor == "group" {
            return "";
        }
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
        self.entries.iter().any(|e| {
            is_value_property(&e.kind_of_editor) && e.kind == kind && self.overrides.contains_key(&e.key)
        })
    }

    /// Write a value. `false` — there is no such value property (since waywallen
    /// 0.6.2 the write is then dropped, without signals: a group header or a
    /// text label has nothing to write).
    pub fn set_value(&mut self, key: &str, value: &str) -> bool {
        if !self.entries.iter().any(|e| e.key == key && is_value_property(&e.kind_of_editor)) {
            return false;
        }
        self.overrides.insert(key.to_string(), value.to_string());
        true
    }

    /// Remove the overrides of value properties of kind `kind` (None — of all).
    /// Returns the keys in entry order — the original sends dataChanged and
    /// resetRequested in the same order, AFTER the shown rows are updated.
    pub fn reset_kind(&mut self, kind: Option<&str>) -> Vec<String> {
        let mut reset = Vec::new();
        for entry in &self.entries {
            if !is_value_property(&entry.kind_of_editor) || kind.is_some_and(|k| entry.kind != k) {
                continue;
            }
            if self.overrides.remove(&entry.key).is_some() {
                reset.push(entry.key.clone());
            }
        }
        reset
    }
}

/// The original's removal-then-insertion walk from `current` to `next` (both
/// ascending entry indices).
pub fn row_ops(current: &[usize], next: &[usize]) -> Vec<RowOp> {
    let keep: std::collections::HashSet<usize> = next.iter().copied().collect();
    let mut rows = current.to_vec();
    let mut ops = Vec::new();
    let mut last = rows.len() as isize - 1;
    while last >= 0 {
        if keep.contains(&rows[last as usize]) {
            last -= 1;
            continue;
        }
        let mut first = last;
        while first > 0 && !keep.contains(&rows[first as usize - 1]) {
            first -= 1;
        }
        ops.push(RowOp::Remove { first: first as usize, last: last as usize });
        rows.drain(first as usize..=last as usize);
        last = first - 1;
    }
    let mut row = 0;
    while row < next.len() {
        if row < rows.len() && rows[row] == next[row] {
            row += 1;
            continue;
        }
        let first = row;
        while row < next.len() && (first >= rows.len() || next[row] != rows[first]) {
            row += 1;
        }
        let entries = next[first..row].to_vec();
        for (k, e) in entries.iter().enumerate() {
            rows.insert(first + k, *e);
        }
        ops.push(RowOp::Insert { first, entries });
    }
    ops
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

/// `%.<digits>g` — since waywallen 0.6.2 the wire string of a number is
/// `QString::number(v, 'g', 17)`, so a slider value survives the round trip.
fn format_g17(value: f64) -> String {
    format_g_digits(value, 17)
}

fn format_g_digits(value: f64, digits: usize) -> String {
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
    let scientific = format!("{value:.*e}", digits - 1);
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("научная запись всегда содержит e");
    let exponent: i32 = exponent.parse().unwrap_or(0);
    // The exponent range in which %g prints ordinary rather than scientific
    // notation: from -4 to 6 (exclusive).
    if !(-4..digits as i32).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{}{:02}",
            trim_trailing_zeros(mantissa),
            sign,
            exponent.abs()
        )
    } else {
        let decimals = (digits as i32 - 1 - exponent).max(0) as usize;
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
        Value::Number(n) => format_g17(n.as_f64().unwrap_or(0.0)),
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
        return format_g17(number);
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
        let mut editor = type_name(&fields);
        // waywallen 0.6.2: a property without a type is a text label.
        if editor.is_empty() {
            editor = "text".to_string();
        }

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

        let supported = editor == "group"
            || SUPPORTED_TYPES.contains(&editor.as_str())
            || (editor == "combo" && !option_values.is_empty());
        let max_val = number_or(fields.get("max"), 1.0);
        let (step_val, precision) = if editor == "slider" {
            slider_presentation(&fields, max_val)
        } else {
            (step_of(&fields), 0)
        };
        let condition = Condition::compile(&text_of(&fields, "condition"));
        if !condition.is_valid() {
            rexon_core::log_warn!("we-props", "unsupported user property condition: {key}");
        }
        // The original: `order`, else `index`, else 0 (`toDouble(default)`: only a
        // JSON number counts).
        let order = fields
            .get("order")
            .and_then(Value::as_f64)
            .or_else(|| fields.get("index").and_then(Value::as_f64))
            .unwrap_or(0.0);

        users.push(Entry {
            label: label_or_key(&fields, key),
            section: SECTION_USER,
            kind: KIND_USER,
            supported,
            min_val: number_or(fields.get("min"), 0.0),
            max_val,
            default_wire: coerce_default_wire(
                fields.get("value").unwrap_or(&Value::Null),
                &editor,
            ),
            option_labels,
            option_values,
            localized_label: localized_of(&fields, "localized_text"),
            localized_option_labels,
            order,
            step_val,
            precision,
            value_suffix: text_of(&fields, "suffix"),
            group_key: String::new(),
            condition,
            default_value: fields.get("value").cloned(),
            options: match fields.get("options") {
                Some(Value::Array(options)) => options.clone(),
                _ => Vec::new(),
            },
            kind_of_editor: editor,
            key: key.clone(),
        });
    }

    users.sort_by(|a, b| a.order.total_cmp(&b.order));
    // Every row after a `group` header belongs to it until the next header.
    let mut group = String::new();
    for entry in &mut users {
        if entry.kind_of_editor == "group" {
            group = entry.key.clone();
        }
        entry.group_key = group.clone();
    }
    entries.extend(users);
    entries
}

/// Slider step and digits after the point — the original's
/// `sliderPresentation` (waywallen 0.6.2): `fraction: false` means integers, an
/// explicit `precision` wins, otherwise the step's own decimals; no step at all is
/// `1` for integer sliders and "free" (0) for fractional ones.
fn slider_presentation(fields: &Map<String, Value>, maximum: f64) -> (f64, i32) {
    let fraction = fields.get("fraction");
    let fraction_true = matches!(fraction, Some(Value::Bool(true)));
    let integer = matches!(fraction, Some(Value::Bool(false)));
    let mut step = fields.get("step").and_then(Value::as_f64).unwrap_or(0.0);
    let mut precision = if fraction_true || maximum <= 10.0 { 3 } else { 0 };
    if integer {
        precision = 0;
    } else if let Some(p) = fields.get("precision").and_then(Value::as_f64) {
        // `QJsonValue::toInt()`: a non-integral number gives 0.
        let p = if p.fract() == 0.0 && p.abs() < i32::MAX as f64 { p as i32 } else { 0 };
        precision = p.clamp(0, 15);
    } else if step > 0.0 {
        let decimal = format!("{step:.15}");
        let decimal = decimal.trim_end_matches('0');
        precision = match decimal.find('.') {
            Some(point) => (decimal.len() - point - 1) as i32,
            None => 0,
        };
        if step < 1.0 && precision == 0 {
            precision = 15;
        }
    }
    if step <= 0.0 {
        step = if integer || (precision == 0 && !fraction_true) { 1.0 } else { 0.0 };
    }
    (step, precision)
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
        let max_val = number_or(fields.get("max"), 1.0);
        let (step_val, precision) = if kind_of_editor == "slider" {
            slider_presentation(fields, max_val)
        } else {
            (step_of(fields), 0)
        };
        // The condition default: the schema value, else the one the row shows.
        let default_value = fields.get("value").cloned().or_else(|| {
            Some(match kind_of_editor.as_str() {
                "bool" => Value::Bool(wire == "true"),
                "slider" => serde_json::Number::from_f64(wire.trim().parse().unwrap_or(0.0))
                    .map_or(Value::Null, Value::Number),
                _ => Value::String(wire.clone()),
            })
        });
        Entry {
            key: key.to_string(),
            label: label_or_key_default(fields, label),
            supported: SUPPORTED_TYPES.contains(&kind_of_editor.as_str()),
            kind_of_editor,
            section: SECTION_PREDEFINED,
            kind: KIND_PREDEFINED,
            min_val: number_or(fields.get("min"), 0.0),
            max_val,
            step_val,
            precision,
            value_suffix: text_of(fields, "suffix"),
            default_wire: wire,
            option_labels: Vec::new(),
            option_values: Vec::new(),
            localized_label: localized_of(fields, "localized_text"),
            localized_option_labels: Vec::new(),
            order: 0.0,
            group_key: String::new(),
            condition: Condition::default(),
            default_value,
            options: Vec::new(),
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
// Visibility conditions (waywallen 0.6.2 `user_property_condition.cppm`)
// ============================================================================
//
// A schema may hide a property behind a Wallpaper Engine condition such as
// `showclock.value == true && style.value != 2`. The original compiles it once
// into a postfix program and evaluates it with three-valued logic: a value it
// cannot know is `Undefined`, and an undefined result SHOWS the row (a broken or
// unsupported expression must not hide a property for good).

/// A condition operand: `QJsonValue` with its `Undefined` state.
#[derive(Debug, Clone, PartialEq)]
pub enum CondValue {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// Arrays and objects: always true, equal only to the same value.
    Other(Value),
}

impl CondValue {
    pub fn from_json(v: &Value) -> Self {
        match v {
            Value::Null => CondValue::Null,
            Value::Bool(b) => CondValue::Bool(*b),
            Value::Number(n) => n.as_f64().map_or(CondValue::Undefined, CondValue::Num),
            Value::String(s) => CondValue::Str(s.clone()),
            other => CondValue::Other(other.clone()),
        }
    }

    fn same_type(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CondOp {
    Open,
    Value,
    Not,
    AssignmentResult,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
struct CondInstruction {
    op: CondOp,
    value: CondValue,
    key: String,
}

/// A compiled visibility condition. The default (an empty expression) always
/// matches.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    program: Vec<CondInstruction>,
    valid: bool,
}

impl Default for Condition {
    fn default() -> Self {
        Condition { program: Vec::new(), valid: true }
    }
}

const COND_OPERATORS: [(&str, CondOp); 10] = [
    ("===", CondOp::StrictEqual),
    ("!==", CondOp::StrictNotEqual),
    ("==", CondOp::Equal),
    ("!=", CondOp::NotEqual),
    ("<=", CondOp::LessEqual),
    (">=", CondOp::GreaterEqual),
    ("&&", CondOp::And),
    ("||", CondOp::Or),
    ("<", CondOp::Less),
    (">", CondOp::Greater),
];

fn cond_precedence(op: CondOp) -> i32 {
    match op {
        CondOp::Open => 0,
        CondOp::Or => 1,
        CondOp::And => 2,
        CondOp::Not => 5,
        CondOp::Less | CondOp::LessEqual | CondOp::Greater | CondOp::GreaterEqual => 4,
        _ => 3,
    }
}

/// `\b` after a keyword: the next character is not a word character.
fn word_end(chars: &[char], at: usize) -> bool {
    chars.get(at).is_none_or(|c| !(c.is_alphanumeric() || *c == '_'))
}

/// One operand at `at`: `(instruction, end)`; `None` — no operand there. The
/// order is the original's regex alternation: `<name>.value`, a keyword, a
/// number, a quoted string.
fn cond_atom(chars: &[char], at: usize) -> Option<Result<(CondInstruction, usize), ()>> {
    let value = |v: CondValue, end: usize| {
        Some(Ok((CondInstruction { op: CondOp::Value, value: v, key: String::new() }, end)))
    };
    let first = *chars.get(at)?;
    // <name> . value
    if first.is_alphabetic() || first == '_' || first == '$' {
        let mut end = at + 1;
        while chars.get(end).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '$') {
            end += 1;
        }
        let name: String = chars[at..end].iter().collect();
        let mut i = end;
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        if chars.get(i) == Some(&'.') {
            i += 1;
            while chars.get(i).is_some_and(|c| c.is_whitespace()) {
                i += 1;
            }
            let word: String = chars.iter().skip(i).take(5).collect();
            if word == "value" && word_end(chars, i + 5) {
                let ins = CondInstruction { op: CondOp::Value, value: CondValue::Undefined, key: name };
                return Some(Ok((ins, i + 5)));
            }
        }
    }
    for (word, v) in [("true", CondValue::Bool(true)), ("false", CondValue::Bool(false)), ("null", CondValue::Null)] {
        let n = word.chars().count();
        if chars.iter().skip(at).take(n).copied().eq(word.chars()) && word_end(chars, at + n) {
            return value(v, at + n);
        }
    }
    // [+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?
    let mut i = at;
    if matches!(chars.get(i), Some('+' | '-')) {
        i += 1;
    }
    let digits = |from: usize| {
        let mut j = from;
        while chars.get(j).is_some_and(char::is_ascii_digit) {
            j += 1;
        }
        j
    };
    let int_end = digits(i);
    let mut end = if int_end > i {
        if chars.get(int_end) == Some(&'.') { digits(int_end + 1) } else { int_end }
    } else if chars.get(i) == Some(&'.') && digits(i + 1) > i + 1 {
        digits(i + 1)
    } else {
        0
    };
    if end > 0 {
        if matches!(chars.get(end), Some('e' | 'E')) {
            let mut j = end + 1;
            if matches!(chars.get(j), Some('+' | '-')) {
                j += 1;
            }
            let k = digits(j);
            if k > j {
                end = k;
            }
        }
        let text: String = chars[at..end].iter().collect();
        return match text.parse::<f64>() {
            Ok(n) if n.is_finite() => value(CondValue::Num(n), end),
            _ => Some(Err(())),
        };
    }
    // "…" or '…' with backslash escapes
    if first == '"' || first == '\'' {
        let mut j = at + 1;
        while let Some(&c) = chars.get(j) {
            if c == '\\' {
                j += 2;
                continue;
            }
            if c == first {
                return match cond_string(&chars[at..=j]) {
                    Some(text) => value(CondValue::Str(text), j + 1),
                    None => Some(Err(())),
                };
            }
            j += 1;
        }
    }
    None
}

/// Unquote a string literal — the original's `string_`.
fn cond_string(quoted: &[char]) -> Option<String> {
    let mut out = String::new();
    let mut i = 1;
    while i + 1 < quoted.len() {
        let c = quoted[i];
        if c != '\\' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i + 1 >= quoted.len() {
            return None;
        }
        match quoted[i] {
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            '/' => out.push('/'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => {
                if i + 4 >= quoted.len() - 1 {
                    return None;
                }
                let hex: String = quoted[i + 1..i + 5].iter().collect();
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code)?);
                i += 4;
            }
            _ => return None,
        }
        i += 1;
    }
    Some(out)
}

fn cond_truth(v: &CondValue) -> Option<bool> {
    match v {
        CondValue::Undefined => None,
        CondValue::Null => Some(false),
        CondValue::Bool(b) => Some(*b),
        CondValue::Num(n) => Some(*n != 0.0),
        CondValue::Str(s) => Some(!s.is_empty()),
        CondValue::Other(_) => Some(true),
    }
}

fn cond_number(v: &CondValue) -> Option<f64> {
    match v {
        CondValue::Num(n) => Some(*n),
        CondValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        CondValue::Null => Some(0.0),
        CondValue::Str(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Some(0.0);
            }
            t.parse::<f64>().ok().filter(|n| n.is_finite())
        }
        _ => None,
    }
}

fn cond_binary(op: CondOp, left: &CondValue, right: &CondValue) -> CondValue {
    if matches!(op, CondOp::And | CondOp::Or) {
        let (a, b) = (cond_truth(left), cond_truth(right));
        if op == CondOp::And && (a == Some(false) || b == Some(false)) {
            return CondValue::Bool(false);
        }
        if op == CondOp::Or && (a == Some(true) || b == Some(true)) {
            return CondValue::Bool(true);
        }
        if a.is_none() || b.is_none() {
            return CondValue::Undefined;
        }
        return CondValue::Bool(op == CondOp::And);
    }
    if *left == CondValue::Undefined || *right == CondValue::Undefined {
        return CondValue::Undefined;
    }
    if op == CondOp::AssignmentResult {
        return right.clone();
    }
    if matches!(op, CondOp::Equal | CondOp::NotEqual | CondOp::StrictEqual | CondOp::StrictNotEqual) {
        let mut equal = left == right;
        if !left.same_type(right)
            && matches!(op, CondOp::Equal | CondOp::NotEqual)
            && *left != CondValue::Null
            && *right != CondValue::Null
        {
            equal = matches!((cond_number(left), cond_number(right)), (Some(a), Some(b)) if a == b);
        }
        let negate = matches!(op, CondOp::NotEqual | CondOp::StrictNotEqual);
        return CondValue::Bool(equal != negate);
    }
    if let (CondValue::Str(a), CondValue::Str(b)) = (left, right) {
        let c = a.cmp(b);
        return CondValue::Bool(match op {
            CondOp::Less => c.is_lt(),
            CondOp::LessEqual => c.is_le(),
            CondOp::Greater => c.is_gt(),
            _ => c.is_ge(),
        });
    }
    let (Some(a), Some(b)) = (cond_number(left), cond_number(right)) else {
        return CondValue::Bool(false);
    };
    CondValue::Bool(match op {
        CondOp::Less => a < b,
        CondOp::LessEqual => a <= b,
        CondOp::Greater => a > b,
        _ => a >= b,
    })
}

impl Condition {
    /// Compile an expression; an unsupported one is invalid and always matches.
    pub fn compile(expression: &str) -> Self {
        let mut c = Condition::default();
        c.valid = c.compile_into(expression);
        if !c.valid {
            c.program.clear();
        }
        c
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    fn compile_into(&mut self, expression: &str) -> bool {
        let chars: Vec<char> = expression.chars().collect();
        if expression.encode_utf16().count() > 4096 {
            return false;
        }
        if expression.trim().is_empty() {
            return true;
        }
        let mut pending: Vec<CondOp> = Vec::new();
        let mut operand = true;
        let mut offset = 0;
        while offset < chars.len() {
            if self.program.len() + pending.len() > 512 {
                return false;
            }
            let c = chars[offset];
            if c.is_whitespace() {
                offset += 1;
                continue;
            }
            if operand {
                let assigning_literal = pending.last() == Some(&CondOp::AssignmentResult);
                if c == '!' || c == '(' {
                    if assigning_literal {
                        return false;
                    }
                    pending.push(if c == '!' { CondOp::Not } else { CondOp::Open });
                    offset += 1;
                    continue;
                }
                let Some(Ok((ins, end))) = cond_atom(&chars, offset) else {
                    return false;
                };
                if assigning_literal
                    && (!ins.key.is_empty() || chars[end..].iter().any(|c| !c.is_whitespace()))
                {
                    return false;
                }
                self.program.push(ins);
                offset = end;
                operand = false;
            } else if c == ')' {
                while pending.last().is_some_and(|op| *op != CondOp::Open) {
                    let op = pending.pop().expect("checked");
                    self.program.push(CondInstruction { op, value: CondValue::Undefined, key: String::new() });
                }
                if pending.pop().is_none() {
                    return false;
                }
                offset += 1;
            } else {
                let rest: String = chars[offset..].iter().collect();
                // Legacy assignments to a literal contribute only their result.
                if c == '='
                    && !rest.starts_with("==")
                    && pending.is_empty()
                    && self.program.len() == 1
                    && !self.program[0].key.is_empty()
                {
                    pending.push(CondOp::AssignmentResult);
                    offset += 1;
                    operand = true;
                    continue;
                }
                let Some((text, op)) = COND_OPERATORS.iter().find(|(t, _)| rest.starts_with(t)) else {
                    return false;
                };
                while pending.last().is_some_and(|p| cond_precedence(*p) >= cond_precedence(*op)) {
                    let p = pending.pop().expect("checked");
                    self.program.push(CondInstruction { op: p, value: CondValue::Undefined, key: String::new() });
                }
                pending.push(*op);
                offset += text.chars().count();
                operand = true;
            }
        }
        if operand {
            return false;
        }
        while let Some(op) = pending.pop() {
            if op == CondOp::Open {
                return false;
            }
            self.program.push(CondInstruction { op, value: CondValue::Undefined, key: String::new() });
        }
        self.program.len() <= 512
    }

    /// Whether the row is shown with these property values (by key).
    pub fn matches(&self, values: &HashMap<String, CondValue>) -> bool {
        if !self.valid || self.program.is_empty() {
            return true;
        }
        let mut stack: Vec<CondValue> = Vec::new();
        for ins in &self.program {
            match ins.op {
                CondOp::Value => stack.push(if ins.key.is_empty() {
                    ins.value.clone()
                } else {
                    values.get(&ins.key).cloned().unwrap_or(CondValue::Undefined)
                }),
                CondOp::Not => {
                    let Some(v) = stack.pop() else {
                        return true;
                    };
                    stack.push(cond_truth(&v).map_or(CondValue::Undefined, |b| CondValue::Bool(!b)));
                }
                op => {
                    if stack.len() < 2 {
                        return true;
                    }
                    let right = stack.pop().expect("checked");
                    let left = stack.pop().expect("checked");
                    stack.push(cond_binary(op, &left, &right));
                }
            }
        }
        if stack.len() != 1 {
            return true;
        }
        cond_truth(&stack[0]).unwrap_or(true)
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
const ROLE_NAMES: [&str; 18] = [
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
    // waywallen 0.6.2
    "isGroup",
    "groupKey",
    "expanded",
    "precision",
];

/// The `expanded` role: a group header's fold state.
const ROLE_EXPANDED: usize = 16;

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
    wallpaper_id: String,
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

    fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    fn set_wallpaper_id(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value == self.wallpaper_id {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = value;
        if self.props.has_collapsed() {
            unsafe {
                self.as_mut().begin_reset_model();
                self.as_mut().rust_mut().props.clear_collapsed();
                self.as_mut().end_reset_model();
            }
            self.as_mut().count_changed();
        }
        self.wallpaper_id_changed();
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

    /// The overrides come from the daemon as a whole in one string. Rows change
    /// only where a condition flips (insert/remove); the value roles of the rest
    /// get one dataChanged over the range. `overridesJsonChanged` goes last and
    /// only when the text differs.
    fn set_overrides_json(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        let json_changed = value != self.overrides_json;
        self.as_mut().rust_mut().overrides_json = value.clone();
        self.as_mut().rust_mut().props.set_overrides(&value);
        self.as_mut().update_visible_rows();

        let rows = self.props.len();
        if rows > 0 {
            let roles = current_value_roles();
            let first = self.index(0, 0, &QModelIndex::default());
            let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
            self.as_mut().data_changed(&first, &last, &roles);
        }
        self.as_mut().override_state_changed();
        if json_changed {
            self.as_mut().overrides_json_changed();
        }
    }

    /// Bring the shown rows to what the conditions and folds say now, by
    /// removals and insertions (the original's `updateVisibleRows_`).
    fn update_visible_rows(mut self: Pin<&mut Self>) {
        let before = self.props.len();
        let root = QModelIndex::default();
        for op in self.props.visible_ops() {
            match &op {
                RowOp::Remove { first, last } => unsafe {
                    self.as_mut().begin_remove_rows(&root, *first as i32, *last as i32);
                    self.as_mut().rust_mut().props.apply_row_op(&op);
                    self.as_mut().end_remove_rows();
                },
                RowOp::Insert { first, entries } => unsafe {
                    let last = first + entries.len() - 1;
                    self.as_mut().begin_insert_rows(&root, *first as i32, last as i32);
                    self.as_mut().rust_mut().props.apply_row_op(&op);
                    self.as_mut().end_insert_rows();
                },
            }
        }
        if before != self.props.len() {
            self.as_mut().count_changed();
        }
    }

    fn set_group_expanded(mut self: Pin<&mut Self>, key: &QString, expanded: bool) {
        let key = key.to_string();
        if !self.as_mut().rust_mut().props.set_group_expanded(&key, expanded) {
            return;
        }
        self.as_mut().update_visible_rows();
        if let Some(row) = self.props.row_of_key(&key) {
            let mut roles = QVector::<i32>::default();
            roles.append(ROLE_BASE + ROLE_EXPANDED as i32);
            let index = self.index(row as i32, 0, &QModelIndex::default());
            self.as_mut().data_changed(&index, &index, &roles);
        }
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
        let Some(row) = self.props.entry_of_row(row as usize) else {
            return QVariant::default();
        };
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
            Some("isGroup") => QVariant::from(&(entry.kind_of_editor == "group")),
            Some("groupKey") => QVariant::from(&QString::from(&entry.group_key)),
            Some("expanded") => QVariant::from(&self.props.expanded(row)),
            Some("precision") => QVariant::from(&entry.precision),
            _ => QVariant::default(),
        }
    }

    // ── API for QML ───────────────────────────────────────────────────────
    fn set_value(mut self: Pin<&mut Self>, key: &QString, value: &QString) {
        let key = key.to_string();
        let value = value.to_string();
        if !self.as_mut().rust_mut().props.set_value(&key, &value) {
            return;
        }
        self.as_mut().update_visible_rows();
        self.as_mut().notify_current_value(&key);
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
    /// A targeted notification about a change of one key's value (if its row is
    /// shown).
    fn notify_current_value(mut self: Pin<&mut Self>, key: &str) {
        let Some(row) = self.props.row_of_key(key) else {
            return;
        };
        let roles = current_value_roles();
        let index = self.index(row as i32, 0, &QModelIndex::default());
        self.as_mut().data_changed(&index, &index, &roles);
    }

    fn reset_kind(mut self: Pin<&mut Self>, kind: Option<&str>) {
        let reset = self.as_mut().rust_mut().props.reset_kind(kind);
        self.as_mut().update_visible_rows();
        for key in reset {
            self.as_mut().notify_current_value(&key);
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

    /// `%g` (six significant digits) — the wire form before waywallen 0.6.2.
    fn format_g(value: f64) -> String {
        format_g_digits(value, 6)
    }

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
                    // waywallen 0.6.2: a key that is no value property is dropped
                    // without any signal (the Python-era reference still has the
                    // `valueChanged` of such a write; see `golden_parity_with_python_model`).
                    if !props.set_value(&action.key, &action.value) {
                        continue;
                    }
                    if let Some(row) = props.row_of_key(&action.key) {
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
                    for key in props.reset_kind(kind) {
                        let Some(row) = props.row_of_key(&key) else { continue };
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

    /// waywallen 0.6.2: Wallpaper Engine visibility conditions.
    #[test]
    fn conditions_follow_the_original() {
        let vals = |pairs: &[(&str, CondValue)]| -> HashMap<String, CondValue> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
        };
        let v = vals(&[("clock", CondValue::Bool(true)), ("style", CondValue::Num(2.0)),
                       ("name", CondValue::Str("b".into()))]);
        for (expr, want) in [
            ("clock.value", true),
            ("!clock.value", false),
            ("clock.value == true && style.value != 2", false),
            ("clock.value && (style.value == 1 || style.value == 2)", true),
            ("style.value == '2'", true),
            ("style.value === '2'", false),
            ("name.value > \"a\"", true),
            ("missing.value == 1", true),
            ("missing.value == 1 && false", false),
            ("clock.value = true", true),
            ("style.value >= 1.5e0", true),
        ] {
            let c = Condition::compile(expr);
            assert!(c.is_valid(), "{expr}");
            assert_eq!(c.matches(&v), want, "{expr}");
        }
        for broken in ["clock.value ==", "(clock.value", "clock", "clock.value = style.value", "1 +"] {
            let c = Condition::compile(broken);
            assert!(!c.is_valid(), "{broken}");
            assert!(c.matches(&v), "an invalid condition shows the row: {broken}");
        }
        assert!(Condition::compile("").matches(&v));
    }

    #[test]
    fn groups_fold_and_conditions_hide_rows() {
        let mut props = UserProperties::default();
        props.set_schema(r#"{
            "look": {"type": "group", "text": "Look", "order": 1},
            "clock": {"type": "bool", "value": false, "order": 2},
            "format": {"type": "combo", "value": 1, "order": 3, "condition": "clock.value == true",
                       "options": [{"label": "24h", "value": 1}, {"label": "12h", "value": 2}]},
            "other": {"type": "group", "order": 4},
            "hidden": {"type": "bool", "value": true, "order": 5, "condition": "clock.value"}
        }"#);
        let keys = |p: &UserProperties| -> Vec<String> {
            p.visible().iter().map(|&i| p.entries()[i].key.clone()).collect()
        };
        // "other" has no shown child, so the header is hidden too.
        assert_eq!(keys(&props), [SCHEME_COLOR_KEY, "look", "clock"]);
        assert!(props.set_value("clock", "true"));
        for op in props.visible_ops() {
            props.apply_row_op(&op);
        }
        assert_eq!(keys(&props), [SCHEME_COLOR_KEY, "look", "clock", "format", "other", "hidden"]);
        assert!(props.set_group_expanded("look", false));
        for op in props.visible_ops() {
            props.apply_row_op(&op);
        }
        assert_eq!(keys(&props), [SCHEME_COLOR_KEY, "look", "other", "hidden"]);
        assert!(!props.set_value("look", "x"), "a group header has no value");
        assert!(props.clear_collapsed());
        assert_eq!(props.visible(), props.compute_visible().as_slice());
    }

    #[test]
    fn row_ops_reach_the_next_set() {
        for (current, next) in [
            (vec![0, 1, 2, 3, 4], vec![0, 3]),
            (vec![0, 3], vec![0, 1, 2, 3, 4]),
            (vec![1, 2, 5], vec![0, 2, 3, 6]),
            (vec![], vec![1, 2]),
            (vec![4, 5], vec![]),
        ] {
            let mut p = UserProperties { visible: current.clone(), ..Default::default() };
            for op in row_ops(&current, &next) {
                p.apply_row_op(&op);
            }
            assert_eq!(p.visible, next, "{current:?} -> {next:?}");
        }
    }

    #[test]
    fn slider_presentation_matches_the_original() {
        let f = |json: &str, max: f64| {
            let Value::Object(m) = serde_json::from_str::<Value>(json).unwrap() else { unreachable!() };
            slider_presentation(&m, max)
        };
        assert_eq!(f(r#"{}"#, 100.0), (1.0, 0));
        assert_eq!(f(r#"{}"#, 1.0), (0.0, 3));
        assert_eq!(f(r#"{"fraction": false}"#, 1.0), (1.0, 0));
        assert_eq!(f(r#"{"step": 0.05}"#, 100.0), (0.05, 2));
        assert_eq!(f(r#"{"precision": 1}"#, 100.0), (0.0, 1));
        assert_eq!(f(r#"{"fraction": true, "step": 0}"#, 100.0), (0.0, 3));
        assert_eq!(format_g17(0.1), "0.10000000000000001");
        assert_eq!(format_g17(1234567.0), "1234567");
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
