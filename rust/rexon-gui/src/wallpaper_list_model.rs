// WallpaperListModel — the wallpaper library model of the "Wallpapers"
// page, ported from Python to Rust/cxx-qt.
//
// Warning: the reference here is NOT our Python. The Python model
// (bridge.py:3064) is a list of maps and nothing more: no selection, no
// keys, no `selected` role. The original is waywallen's
// `ui/src/model/list_models.cppm` on top of `kstore::QMetaListModel` from
// QExtra (`include/kstore/qt/meta_list_model.hpp`,
// `src/kstore/meta_list_model.cpp`), and the selection lives precisely IN
// THE MODEL: a set of SELECTED KEYS, not indices. A wallpaper's key is
// `id_proto` (`ui/src/msg/backend_msg.cppm`, `ItemTrait<Wallpaper>::key`).
//
// The difference is not theoretical, and the port must remove it (rule #16
// — bugs are fixed during porting, not preserved):
//
//   1. We did not have the `selected` role at all, while the vendor's
//      delegate reads it (`page/WallpaperPage.qml:1038`,
//      `selected: model.selected ?? false`). That is, highlighting of
//      selected cards never worked in Rexon.
//   2. Selection by indices drifts on any rebuild of the list: the library
//      was rescanned, the sorting changed — and SOMEONE ELSE's wallpapers
//      end up selected. With keys this does not happen: a wallpaper
//      disappeared — its selection disappeared too.
//   3. The original's `selectedKeys()` returns keys IN THE ORDER OF THE
//      MODEL'S ROWS, followed by the selected keys that are no longer in
//      the list (they are not thrown away: they will come back when the
//      filter stops hiding them).
//
// The layer split is the same as in batches 2-3: the state and the
// decisions "which signals and in what order" live in the plain
// `WallpaperRows` struct without Qt, while the QObject only executes the
// resulting list of events.

/// cxx-qt bridge: a QAbstractListModel subclass.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Warning: without `#[qobject]` — see remote_list_model.rs: for every
        /// `#[qobject]` type in `extern "C++Qt"` cxx-qt emits a pair of cast shims
        /// keyed by the TYPE name, and a second such bridge in the crate gives a
        /// `duplicate symbol` at link time.
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

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        /// The original's `selectedCount` — the size of the set of selected keys.
        #[qproperty(i32, selected_count, cxx_name = "selectedCount",
                    READ = selected_count, NOTIFY = selected_count_changed)]
        /// The original's `selectionEnabled`. The wallpaper model constructor sets
        /// it to true (`list_models.cpp:12`) and nobody touches it afterwards — the
        /// vendored QML does not write this property at all. Kept as a readable
        /// property so that the delegate can ask.
        #[qproperty(bool, selection_enabled, cxx_name = "selectionEnabled",
                    READ = selection_enabled, NOTIFY = selection_enabled_changed)]
        /// waywallen 0.6.0: the model's `hasMore` (QExtra's `QMetaListModel`) — the
        /// `WallpaperView` grid is subscribed to `hasMoreChanged` and on it asks for
        /// more again (`notifyFetchStateChanged`).
        #[qproperty(bool, has_more, cxx_name = "hasMore", READ = has_more,
                    NOTIFY = has_more_changed)]
        type WallpaperListModel = super::WallpaperListModelRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut WallpaperListModel>);

        #[qsignal]
        #[cxx_name = "selectedCountChanged"]
        fn selected_count_changed(self: Pin<&mut WallpaperListModel>);

        #[qsignal]
        #[cxx_name = "selectionEnabledChanged"]
        fn selection_enabled_changed(self: Pin<&mut WallpaperListModel>);

        /// Carries the value, like QExtra's `hasMoreChanged(bool)` and our Discover
        /// results model.
        #[qsignal]
        #[cxx_name = "hasMoreChanged"]
        fn has_more_changed(self: Pin<&mut WallpaperListModel>, value: bool);

        /// The original's `selectionChanged`: "the selected set became different".
        /// Separate from `selectedCountChanged`, because the composition may change
        /// without the size changing.
        #[qsignal]
        #[cxx_name = "selectionChanged"]
        fn selection_changed(self: Pin<&mut WallpaperListModel>);

        /// waywallen 0.5.0: the model asks for the next page (`reqFetchMore` of
        /// QExtra's `QMetaListModel`). It is caught by `WallpaperListQuery::fetchMore`
        /// — that is how the "Hidden wallpapers" grid loads more rows by itself. The
        /// argument is the number of rows, as in the original; the query does not
        /// read it.
        #[qsignal]
        #[cxx_name = "reqFetchMore"]
        fn req_fetch_more(self: Pin<&mut WallpaperListModel>, row: i32);

        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut WallpaperListModel>,
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
        unsafe fn begin_reset_model(self: Pin<&mut WallpaperListModel>);
        /// # Safety
        /// Paired with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut WallpaperListModel>);

        /// # Safety
        /// The range must be valid; a paired end_insert_rows is required.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut WallpaperListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Paired with begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut WallpaperListModel>);
    }

    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &WallpaperListModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── QAbstractListModel overrides ──────────────────────────────────────
    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures that moc makes for default arguments.
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &WallpaperListModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &WallpaperListModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &WallpaperListModel, index: &QModelIndex, role: i32) -> QVariant;

        /// waywallen 0.5.0: the infinite scrolling protocol — the view asks by
        /// itself (QExtra's `QMetaListModel::canFetchMore`/`fetchMore`).
        #[cxx_override]
        #[cxx_name = "canFetchMore"]
        fn can_fetch_more(self: &WallpaperListModel, parent: &QModelIndex) -> bool;

        #[cxx_override]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut WallpaperListModel>, parent: &QModelIndex);
    }

    extern "RustQt" {
        fn count(self: &WallpaperListModel) -> i32;
        #[cxx_name = "selectedCount"]
        fn selected_count(self: &WallpaperListModel) -> i32;
        #[cxx_name = "selectionEnabled"]
        fn selection_enabled(self: &WallpaperListModel) -> bool;
        #[cxx_name = "hasMore"]
        fn has_more(self: &WallpaperListModel) -> bool;
    }

    // ── Invokables (names from the original, must not be changed) ─────────
    extern "RustQt" {
        /// The whole row as a map — `WallpaperPage.qml` puts it into
        /// `root.selectedWallpaper`, and the details panel reads the fields with a
        /// dot.
        #[qinvokable]
        fn item(self: &WallpaperListModel, row: i32) -> QMap_QString_QVariant;

        /// Wallpaper size as a number. In the original the invokable is needed
        /// because Qt Protobuf marks `int64` properties as SCRIPTABLE false and QML
        /// cannot read `wallpaper.size` directly (`list_models.cppm:35`). Our rows
        /// are maps, but the name and the behaviour must match:
        /// `WallpaperDetailPanel.qml` calls `sizeOf(wp)`.
        #[qinvokable]
        #[cxx_name = "sizeOf"]
        fn size_of(self: &WallpaperListModel, wallpaper: &QMap_QString_QVariant) -> f64;

        #[qinvokable]
        #[cxx_name = "isSelected"]
        fn is_selected(self: &WallpaperListModel, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "setSelected"]
        fn set_selected(self: Pin<&mut WallpaperListModel>, row: i32, selected: bool) -> bool;

        #[qinvokable]
        #[cxx_name = "toggleSelected"]
        fn toggle_selected(self: Pin<&mut WallpaperListModel>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(
            self: Pin<&mut WallpaperListModel>,
            from: i32,
            to: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        #[cxx_name = "clearSelection"]
        fn clear_selection(self: Pin<&mut WallpaperListModel>);

        #[qinvokable]
        #[cxx_name = "setSelectedKeys"]
        fn set_selected_keys(self: Pin<&mut WallpaperListModel>, keys: &QStringList);

        #[qinvokable]
        #[cxx_name = "selectedKeys"]
        fn selected_keys(self: &WallpaperListModel) -> QStringList;

        /// Keys of the selected wallpapers that ALLOW removal (`supportsItemRemove`).
        /// The original computes this in
        /// `WallpaperSelectStorage::removableSelectedWallpaperIds` by iterating over
        /// `selectedItems()`; for us the flag lies in the model's row, so the model
        /// computes it — and the selection storage only asks.
        #[qinvokable]
        #[cxx_name = "removableSelectedKeys"]
        fn removable_selected_keys(self: &WallpaperListModel) -> QStringList;

        /// The selected rows in full — the original's `selectedItems()`.
        #[qinvokable]
        #[cxx_name = "selectedItems"]
        fn selected_items(self: &WallpaperListModel) -> QList_QVariant;

        /// A role by name — checks read rows with it without guessing numbers.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &WallpaperListModel, name: &QString) -> i32;

        /// A role's value by name (for checks from QML).
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &WallpaperListModel, row: i32, name: &QString) -> QVariant;
    }

    impl cxx_qt::Threading for WallpaperListModel {}
    impl cxx_qt::Constructor<()> for WallpaperListModel {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QModelIndex, QString, QStringList, QVariant, QVector};
use std::collections::BTreeSet;

use rexon_core::we_settings::VMap;
use serde_json::Value;

use crate::qvalue::{json_to_variant, variant_to_json};

// ============================================================================
// Plain layer: rows, selection and events — without a single Qt type
// ============================================================================

/// Row key — `id_proto` (the original's `ItemTrait<Wallpaper>::key`).
pub const KEY_FIELD: &str = "id_proto";

/// The "the wallpaper can be removed" flag — removableSelected* is computed
/// from it.
pub const REMOVABLE_FIELD: &str = "supportsItemRemove";
/// waywallen 0.4.4: the "wallpaper is hidden" flag in a library record.
pub const HIDDEN_FIELD: &str = "hidden";

/// Keys of a library record in the same order the core puts them
/// (`rexon_core::we_wallpaper::entry_to_map`). The roles are counted from
/// this order, so the list must match the core converter in composition;
/// the names are the original's GADGET properties (camelCase, `id` →
/// `id_proto`).
pub const ENTRY_KEYS: &[&str] = &[
    "id_proto",
    "name",
    "wpType",
    "resource",
    "preview",
    "size",
    "width",
    "height",
    "contentRating",
    "tags",
    "description",
    "externalId",
    "userPropertiesSchema",
    "userPropertyOverrides",
    "wallpaperLayoutOverride",
    "wallpaperLayoutOverrideSet",
    "supportsItemRemove",
    "supportsItemUnsubscribe",
    // v0.3.5: the wallpaper's page on the source's site.
    "webUrl",
    "metadata",
    // v0.4.4: the wallpaper is hidden (`wallpaper_hide`). At the END of the
    // list — the role numbers of the previous keys do not shift.
    "hidden",
    // v0.6.0: an update at the source and the record subtype (1 — timeline).
    // Also at the end — the role numbers of the previous keys do not shift.
    "updateAvailable",
    "subType",
];

/// `Qt::UserRole`. The original's selection role is exactly that
/// (`QMetaListModel::SelectedRole`), and the record fields follow.
pub const SELECTED_ROLE: i32 = 0x0100;

/// Role of the first record field.
pub const FIRST_ENTRY_ROLE: i32 = SELECTED_ROLE + 1;

/// waywallen 0.6.0 (`GridListModel`): the row key (`itemKey`) and the whole
/// row (`item`) — they are read by the grid's `MD.ItemModelSource`. The
/// numbers are the same as in the original (`Qt::UserRole + 0x10000` and the
/// next one).
pub const ITEM_KEY_ROLE: i32 = SELECTED_ROLE + 0x10000;
pub const ITEM_ROLE: i32 = ITEM_KEY_ROLE + 1;

/// What the model must tell Qt after a state change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelEvent {
    /// A full rebuild (begin/endResetModel around it).
    Reset,
    /// countChanged.
    CountChanged,
    /// dataChanged for the selection role over a range of rows.
    SelectedRange { first: i32, last: i32 },
    /// selectedCountChanged.
    SelectedCountChanged,
    /// selectionChanged.
    SelectionChanged,
}

/// Library rows and selection by keys.
#[derive(Debug, Default)]
pub struct WallpaperRows {
    rows: Vec<VMap>,
    /// Selected KEYS. A set, not indices: a rebuild of the list must not move
    /// the selection onto other wallpapers.
    selected: BTreeSet<String>,
    selection_enabled: bool,
    /// `hasMore` of `QMetaListModel`: the query has a next page. Set by the
    /// owner query from the daemon's answer (waywallen 0.5.0).
    has_more: bool,
}

impl WallpaperRows {
    /// The wallpaper model is created with selection enabled
    /// (`list_models.cpp:12`).
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            selected: BTreeSet::new(),
            selection_enabled: true,
            has_more: false,
        }
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn set_has_more(&mut self, value: bool) {
        self.has_more = value;
    }

    /// `canFetchMore`: in a flat list only the root has children.
    pub fn can_fetch_more(&self, root: bool) -> bool {
        root && self.has_more
    }

    /// `QMetaListModel::fetchMore`: the flag is cleared BEFORE the request —
    /// otherwise the view would keep asking for more while the page is in
    /// flight. `true` means send `reqFetchMore`.
    pub fn fetch_more(&mut self, root: bool) -> bool {
        if !self.can_fetch_more(root) {
            return false;
        }
        self.has_more = false;
        true
    }

    /// Append a page at the tail (loading more). The selection, as with a
    /// reset, is not touched: a wallpaper selected earlier that arrives with
    /// the page is highlighted at once.
    pub fn append_rows(&mut self, rows: Vec<VMap>) -> Vec<ModelEvent> {
        if rows.is_empty() {
            return Vec::new();
        }
        self.rows.extend(rows);
        let mut out = vec![ModelEvent::CountChanged];
        if !self.selected.is_empty() {
            out.push(ModelEvent::SelectionChanged);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[VMap] {
        &self.rows
    }

    pub fn row(&self, row: i32) -> Option<&VMap> {
        usize::try_from(row).ok().and_then(|r| self.rows.get(r))
    }

    pub fn selection_enabled(&self) -> bool {
        self.selection_enabled
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// Row key. An empty string means "no row or no key" — in the original this
    /// is `selectionKeyAt`, and it does not select by an empty key.
    pub fn key_at(&self, row: i32) -> String {
        self.row(row)
            .and_then(|m| m.get(KEY_FIELD))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn is_selected(&self, row: i32) -> bool {
        if !self.selection_enabled {
            return false;
        }
        let key = self.key_at(row);
        !key.is_empty() && self.selected.contains(&key)
    }

    /// A full replacement of the rows. The selection is NOT cleared: in the
    /// original it lives in keys and survives a rebuild of the list (the
    /// wallpaper came back — its highlight came back too).
    pub fn reset_rows(&mut self, rows: Vec<VMap>) -> Vec<ModelEvent> {
        let count_changed = self.rows.len() != rows.len();
        let selected_before = self.selected_count();
        self.rows = rows;
        let mut out = vec![ModelEvent::Reset];
        if count_changed {
            out.push(ModelEvent::CountChanged);
        }
        // The selection size does not change (the keys are the same), but the
        // composition of the VISIBLE selection may have shifted — the page
        // recomputes on selectionChanged.
        if selected_before > 0 {
            out.push(ModelEvent::SelectionChanged);
        }
        out
    }

    pub fn set_selected(&mut self, row: i32, selected: bool) -> (bool, Vec<ModelEvent>) {
        if !self.selection_enabled {
            return (false, Vec::new());
        }
        let key = self.key_at(row);
        if key.is_empty() {
            return (false, Vec::new());
        }
        if self.selected.contains(&key) == selected {
            return (false, Vec::new());
        }
        if selected {
            self.selected.insert(key);
        } else {
            self.selected.remove(&key);
        }
        (
            true,
            vec![
                ModelEvent::SelectedRange {
                    first: row,
                    last: row,
                },
                ModelEvent::SelectedCountChanged,
                ModelEvent::SelectionChanged,
            ],
        )
    }

    pub fn toggle_selected(&mut self, row: i32) -> (bool, Vec<ModelEvent>) {
        let next = !self.is_selected(row);
        self.set_selected(row, next)
    }

    /// A range of rows. The bounds are clamped to the list and swapped if they
    /// came in reverse — like the original's `std::clamp` + `swap`.
    pub fn select_range(&mut self, from: i32, to: i32, selected: bool) -> (bool, Vec<ModelEvent>) {
        if !self.selection_enabled || self.rows.is_empty() {
            return (false, Vec::new());
        }
        let last_row = self.rows.len() as i32 - 1;
        let mut first = from.clamp(0, last_row);
        let mut last = to.clamp(0, last_row);
        if first > last {
            std::mem::swap(&mut first, &mut last);
        }
        let mut changed = false;
        for row in first..=last {
            let key = self.key_at(row);
            if key.is_empty() || self.selected.contains(&key) == selected {
                continue;
            }
            changed = true;
            if selected {
                self.selected.insert(key);
            } else {
                self.selected.remove(&key);
            }
        }
        if !changed {
            return (false, Vec::new());
        }
        (
            true,
            vec![
                ModelEvent::SelectedRange { first, last },
                ModelEvent::SelectedCountChanged,
                ModelEvent::SelectionChanged,
            ],
        )
    }

    pub fn clear_selection(&mut self) -> Vec<ModelEvent> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.selected.clear();
        self.all_rows_events()
    }

    pub fn set_selected_keys(&mut self, keys: &[String]) -> Vec<ModelEvent> {
        let next: BTreeSet<String> = keys.iter().filter(|k| !k.is_empty()).cloned().collect();
        if next == self.selected {
            return Vec::new();
        }
        self.selected = next;
        self.all_rows_events()
    }

    /// Keys of the selection: first in the order of the model's rows, then the
    /// selected keys that are not in the list right now. The order is exactly
    /// as in the original; the tail is needed because a filter may temporarily
    /// hide selected items.
    pub fn selected_keys(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.selected.len());
        let mut seen = BTreeSet::new();
        for row in 0..self.rows.len() as i32 {
            let key = self.key_at(row);
            if !key.is_empty() && self.selected.contains(&key) {
                seen.insert(key.clone());
                out.push(key);
            }
        }
        for key in &self.selected {
            if !seen.contains(key) {
                out.push(key.clone());
            }
        }
        out
    }

    /// Keys of the selection that have `supportsItemRemove` set.
    ///
    /// Computed ONLY over visible rows: the flag lives in the record, and about
    /// a key that is not in the list we know nothing — asking the daemon to
    /// remove it blindly is not allowed.
    pub fn removable_selected_keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for row in 0..self.rows.len() as i32 {
            if !self.is_selected(row) {
                continue;
            }
            let removable = self
                .row(row)
                .and_then(|m| m.get(REMOVABLE_FIELD))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let key = self.key_at(row);
            if removable && !key.is_empty() {
                out.push(key);
            }
        }
        out
    }

    /// How many selected rows are hidden — the original's `hiddenSelectedCount`
    /// (waywallen 0.4.4). As in the original, it is counted over selected
    /// RECORDS: a key that is not in the list says nothing about itself.
    pub fn hidden_selected_count(&self) -> usize {
        self.selected_items()
            .into_iter()
            .filter(|m| m.get(HIDDEN_FIELD).and_then(Value::as_bool).unwrap_or(false))
            .count()
    }

    /// The selected rows in full, in model order.
    pub fn selected_items(&self) -> Vec<&VMap> {
        (0..self.rows.len() as i32)
            .filter(|row| self.is_selected(*row))
            .filter_map(|row| self.row(row))
            .collect()
    }

    fn all_rows_events(&self) -> Vec<ModelEvent> {
        let mut out = Vec::new();
        if !self.rows.is_empty() {
            out.push(ModelEvent::SelectedRange {
                first: 0,
                last: self.rows.len() as i32 - 1,
            });
        }
        out.push(ModelEvent::SelectedCountChanged);
        out.push(ModelEvent::SelectionChanged);
        out
    }
}

/// Role number by name. `selected` is the selection role, the others are
/// record fields.
pub fn role_of(name: &str) -> Option<i32> {
    match name {
        "selected" => return Some(SELECTED_ROLE),
        "itemKey" => return Some(ITEM_KEY_ROLE),
        "item" => return Some(ITEM_ROLE),
        _ => {}
    }
    ENTRY_KEYS
        .iter()
        .position(|k| *k == name)
        .map(|i| FIRST_ENTRY_ROLE + i as i32)
}

/// Record field name by role number.
pub fn key_of(role: i32) -> Option<&'static str> {
    usize::try_from(role - FIRST_ENTRY_ROLE)
        .ok()
        .and_then(|i| ENTRY_KEYS.get(i).copied())
}

// ============================================================================
// Qt layer
// ============================================================================

/// Rust state of the model.
pub struct WallpaperListModelRust {
    rows: WallpaperRows,
}

impl Default for WallpaperListModelRust {
    fn default() -> Self {
        Self {
            rows: WallpaperRows::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::WallpaperListModel {
    fn initialize(self: Pin<&mut Self>) {
        // The default state is already correct (selection enabled); the method is
        // needed because cxx-qt constructs #[base] objects through Initialize.
        let _ = self;
    }
}

fn role_vector(role: i32) -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(role);
    roles
}

impl qobject::WallpaperListModel {
    pub fn count(&self) -> i32 {
        self.rows.len() as i32
    }

    pub fn selected_count(&self) -> i32 {
        self.rows.selected_count() as i32
    }

    pub fn selection_enabled(&self) -> bool {
        self.rows.selection_enabled()
    }

    pub fn has_more(&self) -> bool {
        self.rows.has_more()
    }

    pub fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            0
        } else {
            self.count()
        }
    }

    pub fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        // `selected` is declared only with selection enabled — just as in the
        // original (`meta_list_model.cpp:278`).
        if self.rows.selection_enabled() {
            names.insert(SELECTED_ROLE, cxx_qt_lib::QByteArray::from("selected"));
        }
        for (i, key) in ENTRY_KEYS.iter().enumerate() {
            names.insert(
                FIRST_ENTRY_ROLE + i as i32,
                cxx_qt_lib::QByteArray::from(*key),
            );
        }
        names.insert(ITEM_KEY_ROLE, cxx_qt_lib::QByteArray::from("itemKey"));
        names.insert(ITEM_ROLE, cxx_qt_lib::QByteArray::from("item"));
        names
    }

    pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if !index.is_valid() || self.rows.row(row).is_none() {
            return QVariant::default();
        }
        if role == SELECTED_ROLE {
            return QVariant::from(&self.rows.is_selected(row));
        }
        if role == ITEM_KEY_ROLE {
            // The key must be a STRING: otherwise `ItemModelSource` rejects the whole
            // model ("Key and reuse type must be strings").
            let key = self.rows.row(row).and_then(|m| m.get(KEY_FIELD));
            let text = match key {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            return QVariant::from(&QString::from(&text));
        }
        if role == ITEM_ROLE {
            return QVariant::from(&self.item(row));
        }
        let Some(key) = key_of(role) else {
            return QVariant::default();
        };
        match self.rows.row(row).and_then(|m| m.get(key)) {
            Some(value) => json_to_variant(value),
            None => QVariant::default(),
        }
    }

    pub fn item(&self, row: i32) -> qobject::QMap_QString_QVariant {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        if let Some(map) = self.rows.row(row) {
            for (key, value) in map {
                out.insert(QString::from(key), json_to_variant(value));
            }
        }
        out
    }

    pub fn size_of(&self, wallpaper: &qobject::QMap_QString_QVariant) -> f64 {
        // The map comes from `item()`, so the value is already a number; but QML
        // may hand over a string too (the details panel calls sizeOf on what it
        // put into a property), so both forms are parsed.
        let Some(value) = wallpaper.get(&QString::from("size")) else {
            return 0.0;
        };
        match variant_to_json(&value) {
            Value::Number(n) => n.as_f64().unwrap_or_default(),
            Value::String(s) => s.parse::<f64>().unwrap_or_default(),
            _ => 0.0,
        }
    }

    pub fn is_selected(&self, row: i32) -> bool {
        self.rows.is_selected(row)
    }

    pub fn set_selected(mut self: Pin<&mut Self>, row: i32, selected: bool) -> bool {
        let (changed, events) = self.as_mut().rust_mut().rows.set_selected(row, selected);
        self.emit_events(&events);
        changed
    }

    pub fn toggle_selected(mut self: Pin<&mut Self>, row: i32) -> bool {
        let (changed, events) = self.as_mut().rust_mut().rows.toggle_selected(row);
        self.emit_events(&events);
        changed
    }

    pub fn select_range(mut self: Pin<&mut Self>, from: i32, to: i32, selected: bool) -> bool {
        let (changed, events) = self
            .as_mut()
            .rust_mut()
            .rows
            .select_range(from, to, selected);
        self.emit_events(&events);
        changed
    }

    pub fn clear_selection(mut self: Pin<&mut Self>) {
        let events = self.as_mut().rust_mut().rows.clear_selection();
        self.emit_events(&events);
    }

    pub fn set_selected_keys(mut self: Pin<&mut Self>, keys: &QStringList) {
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        let events = self.as_mut().rust_mut().rows.set_selected_keys(&keys);
        self.emit_events(&events);
    }

    pub fn selected_keys(&self) -> QStringList {
        QStringList::from_iter(self.rows.selected_keys().iter().map(QString::from))
    }

    pub fn removable_selected_keys(&self) -> QStringList {
        QStringList::from_iter(self.rows.removable_selected_keys().iter().map(QString::from))
    }

    pub fn selected_items(&self) -> qobject::QList_QVariant {
        let mut out = qobject::QList_QVariant::default();
        for map in self.rows.selected_items() {
            out.append(json_to_variant(&Value::Object(map.clone())));
        }
        out
    }

    pub fn role_number(&self, name: &QString) -> i32 {
        role_of(&name.to_string()).unwrap_or(-1)
    }

    pub fn role_value(&self, row: i32, name: &QString) -> QVariant {
        let Some(role) = role_of(&name.to_string()) else {
            return QVariant::default();
        };
        let index = self.index(row, 0, &QModelIndex::default());
        self.data(&index, role)
    }

    fn can_fetch_more(&self, parent: &QModelIndex) -> bool {
        self.rows.can_fetch_more(!parent.is_valid())
    }

    // pub: the QML bench triggers loading more the same way the grid does.
    pub fn fetch_more(mut self: Pin<&mut Self>, parent: &QModelIndex) {
        if self.as_mut().rust_mut().rows.fetch_more(!parent.is_valid()) {
            self.as_mut().has_more_changed(false);
            let rows = self.count();
            self.req_fetch_more(rows);
        }
    }

    // ── Rust API for the owner query ──────────────────────────────────────

    /// The "there is a next page" flag (the original's `setHasMore`).
    pub fn set_page_has_more(mut self: Pin<&mut Self>, value: bool) {
        if self.rows.has_more() == value {
            return;
        }
        self.as_mut().rust_mut().rows.set_has_more(value);
        self.has_more_changed(value);
    }

    /// Rows in the model — the query appends to it only the missing tail.
    pub fn row_total(&self) -> usize {
        self.rows.len()
    }

    /// Loading a page: announce the insertion, append, close, then the tail of
    /// the signals.
    pub fn append_rows(mut self: Pin<&mut Self>, rows: Vec<VMap>) {
        if rows.is_empty() {
            return;
        }
        let first = self.rows.len() as i32;
        let last = first + rows.len() as i32 - 1;
        let events = unsafe {
            self.as_mut()
                .begin_insert_rows(&QModelIndex::default(), first, last);
            let events = self.as_mut().rust_mut().rows.append_rows(rows);
            self.as_mut().end_insert_rows();
            events
        };
        self.emit_events(&events);
    }

    /// A full replacement of the library rows (the daemon's answer to
    /// `wallpaper_list`).
    pub fn reset_rows(mut self: Pin<&mut Self>, rows: Vec<VMap>) {
        let events = unsafe {
            self.as_mut().begin_reset_model();
            let events = self.as_mut().rust_mut().rows.reset_rows(rows);
            self.as_mut().end_reset_model();
            events
        };
        self.emit_events(&events);
    }

    /// Keys of the selection — the selection storage uses them.
    pub fn selected_keys_vec(&self) -> Vec<String> {
        self.rows.selected_keys()
    }

    pub fn removable_selected_keys_vec(&self) -> Vec<String> {
        self.rows.removable_selected_keys()
    }

    pub fn selected_count_value(&self) -> i32 {
        self.rows.selected_count() as i32
    }

    pub fn hidden_selected_count_value(&self) -> i32 {
        self.rows.hidden_selected_count() as i32
    }

    pub fn emit_events(mut self: Pin<&mut Self>, events: &[ModelEvent]) {
        for event in events {
            match event {
                // The reset is already announced with begin/endResetModel around the
                // edit.
                ModelEvent::Reset => {}
                ModelEvent::CountChanged => self.as_mut().count_changed(),
                ModelEvent::SelectedRange { first, last } => {
                    let top = self.as_ref().index(*first, 0, &QModelIndex::default());
                    let bottom = self.as_ref().index(*last, 0, &QModelIndex::default());
                    self.as_mut()
                        .data_changed(&top, &bottom, &role_vector(SELECTED_ROLE));
                }
                ModelEvent::SelectedCountChanged => self.as_mut().selected_count_changed(),
                ModelEvent::SelectionChanged => self.as_mut().selection_changed(),
            }
        }
    }
}
