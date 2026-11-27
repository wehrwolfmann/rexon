// RemoteListModel — the result model of the Discover page of the Wallpaper
// Engine tab, ported from Python to Rust/cxx-qt.
//
// The reference is rexon/qt_wallpaper_engine/qml_ui/bridge.py:566 (itself a
// mirror of waywallen's model::RemoteListModel, roles 1:1 with
// remote_model.cpp). We do NOT touch the vendored QML
// (page/DiscoverPage.qml, page/RemoteCard.qml), so the names of the type,
// roles, properties, signals and invokables must match character for
// character.
//
// Warning: what leaving PySide6 was started for in the first place.
// Historically it was exactly this model that crashed: the user closed the
// tab while the `remote_search` answer was coming from the daemon, the
// callback arrived at an already deleted object and gave "RuntimeError:
// Internal C++ object (RemoteListModel) already deleted", and the exception
// handler finished it off with a second one — SIGSEGV territory.
// In Python this is closed by hand: the `_pending` map keeps the owner
// QObject, and on delivery `shiboken6.isValid(owner)` is checked. Here
// there is no such branch at all: the only delivery path is
// `CxxQtThread::queue`, and on a dead object it returns the typed error
// `ObjectDestroyed`, and on an object that died AFTER queueing it simply
// does nothing. The rule is `queue(...).ok()`, without `unwrap()` and
// without a preceding `is_destroyed()` (between the check and the queueing
// the object may die — the race is described right in the cxx-qt doc
// comment). The proof is `selftest::run()`.
//
// The layer split is the same as in the rest of the crate: all state and
// all decisions about "which signals and in what order" live in the plain
// `RemoteRows` struct without Qt (that is what the golden test checks
// against the LIVE Python model), while the QObject only executes the
// resulting list of events.

/// cxx-qt bridge: a QAbstractListModel subclass.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// The base Qt type: cxx-qt-lib has no QAbstractListModel wrapper.
        ///
        /// Warning: declared WITHOUT `#[qobject]` — as in the crate's second bridge
        /// (user_property_model.rs) and for the same reason: for every
        /// `#[qobject]` type in `extern "C++Qt"` cxx-qt emits a pair of cast shims
        /// `cxx_qt_ffi_<Type>_upcastPtr/downcastPtr`, whose name is built from the
        /// TYPE name, not the bridge's. A second such bridge in the crate gives a
        /// `duplicate symbol` at link time. Without `#[qobject]` the shims are not
        /// emitted, while `#[base]`, `#[inherit]` and `#[cxx_override]` work: they
        /// hang on the subclass.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        /// QVariantMap — the vendored QML gets a whole row with it
        /// (`detailStore.item = searchQuery.model.item(index)`).
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// Set of roles for dataChanged (in Qt6 QVector<int> == QList<int>).
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    // Object factory and destruction: `new` and `delete` live only in C++.
    // Needed by the late callback bench — it creates the model without a
    // parent and deliberately kills it while the answer is in flight.
    unsafe extern "C++" {
        // QObject has to be declared by hand. cxx-qt itself emits this
        // declaration only if the bridge has a QObject without a base or an
        // `extern "C++Qt"` block with a `#[qobject]` type
        // (cxx-qt-gen/generator/rust: add_qobject_import). We have neither: the
        // base is declared with an ordinary `extern "C++"` to avoid the duplicate
        // symbol — so the declaration is ours.
        #[namespace = ""]
        type QObject = cxx_qt::QObject;

        include!("rexon_gui_factory.h");
        /// # Safety
        /// parent is either nullptr (ownership stays with the caller) or a live
        /// QObject.
        //
        // An explicit allow: cxx moves the declaration into its own extern block,
        // losing the doc comment, and clippy sees an unsafe function "without a
        // Safety section".
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_remote_list_model(parent: *mut QObject) -> *mut QObject;
        /// # Safety
        /// The object must have been created without a parent and have no owning
        /// children.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_delete_object(object: *mut QObject);
        /// Spin the event loop for no longer than ms milliseconds.
        fn rexon_process_events(ms: i32);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        // hasMore is read-only: in the Python model the property also has no
        // WRITE, and it is moved internally by set_has_more/the result page.
        #[qproperty(bool, has_more, cxx_name = "hasMore",
                    READ = has_more, NOTIFY = has_more_changed)]
        type RemoteListModel = super::RemoteListModelRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut RemoteListModel>);

        /// hasMoreChanged carries the value — that is how the original declares it
        /// too (`Q_SIGNAL void hasMoreChanged(bool)` in qextra), and the Python
        /// bridge.
        #[qsignal]
        #[cxx_name = "hasMoreChanged"]
        fn has_more_changed(self: Pin<&mut RemoteListModel>, value: bool);

        /// The model asks for the next page. The argument is the row number to
        /// continue from (in the original this is `reqFetchMore(qint32)`, caught by
        /// RemoteSearchQuery::fetchMore).
        #[qsignal]
        #[cxx_name = "reqFetchMore"]
        fn req_fetch_more(self: Pin<&mut RemoteListModel>, row: i32);

        /// dataChanged of the base class — a targeted update of one role.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut RemoteListModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        // Warning: do NOT add modelReset and rowsInserted here. These are private
        // Qt signals: they have a hidden parameter
        // QAbstractItemModel::QPrivateSignal, and `#[inherit] #[qsignal]` on them
        // does not compile — cxx-qt takes a member pointer of the form
        // `void (RemoteListModel::*)()`, while the real type is
        // `void (QAbstractItemModel::*)(QPrivateSignal)`, and the compiler refuses
        // to convert. They can be subscribed to from QML (moc exposes such signals
        // normally).
    }

    // ── Inherited methods of the base class ───────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Requires a paired end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut RemoteListModel>);
        /// # Safety
        /// Paired with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut RemoteListModel>);

        /// # Safety
        /// The range must be correct; a paired end_insert_rows is required.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut RemoteListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Paired with begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut RemoteListModel>);
    }

    // index() is safe to call, but cxx-qt requires safe #[inherit] functions to
    // live in an unsafe block (unsafe here is about matching the declaration to
    // the C++ signature, not about the call being dangerous).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &RemoteListModel,
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
        fn row_count(self: &RemoteListModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &RemoteListModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &RemoteListModel, index: &QModelIndex, role: i32) -> QVariant;

        /// The infinite scrolling protocol: the view asks by itself; there has been
        /// no manual loadMore in QML since v0.2.5.
        #[cxx_override]
        #[cxx_name = "canFetchMore"]
        fn can_fetch_more(self: &RemoteListModel, parent: &QModelIndex) -> bool;

        #[cxx_override]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut RemoteListModel>, parent: &QModelIndex);
    }

    // ── Property getters ──────────────────────────────────────────────────
    extern "RustQt" {
        fn count(self: &RemoteListModel) -> i32;
        #[cxx_name = "hasMore"]
        fn has_more(self: &RemoteListModel) -> bool;
    }

    // ── API for QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// A whole row as a QVariantMap. The vendored QML calls `item(index)`
        /// (DiscoverPage.qml:175), `get` is the bridge's former name.
        #[qinvokable]
        fn get(self: &RemoteListModel, row: i32) -> QMap_QString_QVariant;
        #[qinvokable]
        fn item(self: &RemoteListModel, row: i32) -> QMap_QString_QVariant;

        /// Compatibility: a boolean input → acquisition state 3/0.
        #[qinvokable]
        #[cxx_name = "setInstalled"]
        fn set_installed(
            self: Pin<&mut RemoteListModel>,
            source_id: &QString,
            item_id: &QString,
            installed: bool,
        );

        /// Acquisition state of a row (0 — none, 1 — queued, 2 — in progress,
        /// 3 — present/subscribed). A targeted dataChanged for exactly one role.
        #[qinvokable]
        #[cxx_name = "setAcquisitionState"]
        fn set_acquisition_state(
            self: Pin<&mut RemoteListModel>,
            source_id: &QString,
            item_id: &QString,
            state: i32,
        );

        /// Add author names to already shown cards: the key is the itemId, the
        /// value is the name. In Python this is an ordinary method (the query calls
        /// it), not a slot; here it is an invokable — so both the bench and the
        /// future Rust query of batch 3 can use it. The vendored QML does not know
        /// it, so extending the surface is safe.
        #[qinvokable]
        #[cxx_name = "setAuthors"]
        fn set_authors(self: Pin<&mut RemoteListModel>, names: &QMap_QString_QVariant);

        /// The same for author avatars: the key is the itemId, the value is the
        /// picture address. A separate invokable rather than a field in setAuthors,
        /// because it touches ITS OWN role: a card whose avatar did not download
        /// must stay exactly as it was.
        #[qinvokable]
        #[cxx_name = "setAuthorAvatars"]
        fn set_author_avatars(
            self: Pin<&mut RemoteListModel>,
            avatars: &QMap_QString_QVariant,
        );

        /// Put a result page. A seam for batch 3 (there the page will be supplied
        /// by a Rust query) and an entry for the bench: the JSON is an array of
        /// objects with the seven role fields. append=false is a full replacement
        /// (reset_rows), true is loading more (append_rows). Returns the number of
        /// parsed rows.
        #[qinvokable]
        #[cxx_name = "applyPageJson"]
        fn apply_page_json(
            self: Pin<&mut RemoteListModel>,
            json: &QString,
            has_more: bool,
            append: bool,
        ) -> i32;

        /// Role number by name (-1 if there is no such role) — so that the bench
        /// can check which role exactly arrived in dataChanged.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &RemoteListModel, name: &QString) -> i32;

        /// A role's value by row number — checks without a delegate.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &RemoteListModel, row: i32, name: &QString) -> QVariant;

        /// Broadcast the acquisition state through the SHARED store — that is, to
        /// all live models at once, not only this one. A mirror of
        /// `_acq_store().set()`. Returns the number of live receivers.
        #[qinvokable]
        #[cxx_name = "publishAcquisitionState"]
        fn publish_acquisition_state(
            self: &RemoteListModel,
            source_id: &QString,
            item_id: &QString,
            state: i32,
        ) -> i32;
    }

    impl cxx_qt::Threading for RemoteListModel {}
    impl cxx_qt::Initialize for RemoteListModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant, QVector,
};

use serde::Deserialize;
use serde_json::{Map, Value};

// ============================================================================
// Pure layer: result rows and signal decisions (no Qt here)
// ============================================================================

/// Roles start at Qt::UserRole + 1 (Qt::UserRole == 256) — the same numbers
/// as in the Python model and in the original (remote_model.cpp).
pub const ROLE_BASE: i32 = 256 + 1;

/// Role names in number order — the single source of truth for roleNames,
/// data() and roleNumber(). The order must not change: the vendored QML
/// addresses the roles by name, and the numbers go into dataChanged.
/// The last two roles are appended AT THE END on purpose: the numbers of
/// already shown roles must not shift, they go into dataChanged.
/// `authorAvatarUrl` is the row property name of waywallen 0.5.0
/// (`remote_model.cppm`; before 0.5.0 it was our `authorAvatar` role for the
/// Workshop, the same number), `authorUrl` is OUR CHANGE (parity with
/// Wallpaper Engine: going to the author's other works).
pub const ROLE_NAMES: [&str; 19] = [
    "itemId",
    "sourceId",
    "title",
    "previewUrl",
    "author",
    "wpType",
    "acquisitionState",
    "authorAvatarUrl",
    "authorUrl",
    // waywallen 0.6.0 (`remote_model.cppm`): the size of the resource itself
    // (not of the cropped preview; 0 — the source does not know) and the
    // download progress.
    "width",
    "height",
    "downloadTransferActive",
    "downloadReceivedBytes",
    "downloadTotalBytes",
    // waywallen 0.6.0 (`GridListModel`): the row key and the whole row — read
    // by the grid's `MD.ItemModelSource` (`keyRole: "itemKey"`, the `item`
    // role). The key is "source␀item", like the original's `remoteKey`.
    "itemKey",
    "item",
    // waywallen 0.6.1 (`remote_model.cppm`): HTTP headers the source needs
    // for its images (e.g. a Referer) and the badges of the card.
    "previewHeaders",
    "authorAvatarHeaders",
    "badges",
];

/// How many roles the Discover golden file knows (captured before 0.6.0).
/// A row in the reference's JSON form ([`RemoteRow::to_json`]) is exactly
/// these roles; new roles go to QML but are not part of the comparison
/// with the previous reference.
pub const GOLDEN_ROLES: usize = 9;

/// Result row key — the original's `remoteKey`: one item may lie in the
/// results of two sources, so both identifiers are in the key.
pub fn remote_key(source_id: &str, item_id: &str) -> String {
    let mut key = String::with_capacity(source_id.len() + item_id.len() + 1);
    key.push_str(source_id);
    key.push('\0');
    key.push_str(item_id);
    key
}

/// A Discover result row. Exactly nine fields — as many as `_row_from_item`
/// (bridge.py:708) and the Workshop branch put.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRow {
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub preview_url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub wp_type: String,
    #[serde(default)]
    pub acquisition_state: i32,
    #[serde(default)]
    pub author_avatar_url: String,
    #[serde(default)]
    pub author_url: String,
    /// waywallen 0.6.0: resource size; 0 means unknown.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// waywallen 0.6.0: a file transfer is in progress
    /// (`RemoteDownloadProgress.transfer`).
    #[serde(default)]
    pub download_transfer_active: bool,
    #[serde(default)]
    pub download_received_bytes: u64,
    /// 0 means the source did not report the full size.
    #[serde(default)]
    pub download_total_bytes: u64,
    /// waywallen 0.6.1: headers for loading the preview (object; Null = none).
    #[serde(default)]
    pub preview_headers: Value,
    /// waywallen 0.6.1: headers for loading the author avatar.
    #[serde(default)]
    pub author_avatar_headers: Value,
    /// waywallen 0.6.1: card badges, `[{id, text}]` (Null = none).
    #[serde(default)]
    pub badges: Value,
}

impl RemoteRow {
    /// A role's value by its ordinal number in ROLE_NAMES.
    fn role_slot(&self, slot: usize) -> Option<RoleValue<'_>> {
        Some(match *ROLE_NAMES.get(slot)? {
            "itemId" => RoleValue::Text(&self.item_id),
            "sourceId" => RoleValue::Text(&self.source_id),
            "title" => RoleValue::Text(&self.title),
            "previewUrl" => RoleValue::Text(&self.preview_url),
            "author" => RoleValue::Text(&self.author),
            "wpType" => RoleValue::Text(&self.wp_type),
            "acquisitionState" => RoleValue::Number(self.acquisition_state),
            "authorAvatarUrl" => RoleValue::Text(&self.author_avatar_url),
            "authorUrl" => RoleValue::Text(&self.author_url),
            "width" => RoleValue::Unsigned(u64::from(self.width)),
            "height" => RoleValue::Unsigned(u64::from(self.height)),
            "downloadTransferActive" => RoleValue::Flag(self.download_transfer_active),
            "downloadReceivedBytes" => RoleValue::Unsigned(self.download_received_bytes),
            "downloadTotalBytes" => RoleValue::Unsigned(self.download_total_bytes),
            "itemKey" => RoleValue::Key,
            "item" => RoleValue::Whole,
            "previewHeaders" => RoleValue::Json(&self.preview_headers),
            "authorAvatarHeaders" => RoleValue::Json(&self.author_avatar_headers),
            "badges" => RoleValue::Json(&self.badges),
            _ => return None,
        })
    }

    /// Row key (the `itemKey` role).
    pub fn key(&self) -> String {
        remote_key(&self.source_id, &self.item_id)
    }

    /// The row as a JSON object in the golden form: only the first
    /// [`GOLDEN_ROLES`] roles (the reference was captured before waywallen
    /// 0.6.0).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        for (slot, name) in ROLE_NAMES.iter().enumerate().take(GOLDEN_ROLES) {
            let value = match self.role_slot(slot) {
                Some(RoleValue::Text(text)) => Value::String(text.to_string()),
                Some(RoleValue::Number(number)) => Value::from(number),
                _ => Value::Null,
            };
            map.insert((*name).to_string(), value);
        }
        Value::Object(map)
    }

    /// Role value for QML. `item` is the whole row as a map (without `item` and
    /// `itemKey` inside), like the original's `RemoteRow` gadget.
    fn role_variant(&self, slot: usize) -> QVariant {
        match self.role_slot(slot) {
            Some(RoleValue::Text(text)) => QVariant::from(&QString::from(text)),
            Some(RoleValue::Number(number)) => QVariant::from(&number),
            // We do not expose a 64-bit integer to QML (`qvalue::u64_to_qml`): bytes
            // and sizes are far from 2^53.
            Some(RoleValue::Unsigned(number)) => {
                QVariant::from(&crate::qvalue::u64_to_qml(number))
            }
            Some(RoleValue::Flag(flag)) => QVariant::from(&flag),
            Some(RoleValue::Json(value)) => crate::qvalue::json_to_variant(value),
            Some(RoleValue::Key) => QVariant::from(&QString::from(&self.key())),
            Some(RoleValue::Whole) => {
                let mut map = QMap::<QMapPair_QString_QVariant>::default();
                for (slot, name) in ROLE_NAMES.iter().enumerate() {
                    if matches!(*name, "item" | "itemKey") {
                        continue;
                    }
                    map.insert(QString::from(*name), self.role_variant(slot));
                }
                QVariant::from(&map)
            }
            None => QVariant::default(),
        }
    }

    /// Download progress from the store (see [`acquisition::Transfer`]).
    fn set_transfer(&mut self, transfer: acquisition::Transfer) {
        self.download_transfer_active = transfer.active;
        self.download_received_bytes = transfer.received;
        self.download_total_bytes = transfer.total;
    }

    fn transfer(&self) -> acquisition::Transfer {
        acquisition::Transfer {
            active: self.download_transfer_active,
            received: self.download_received_bytes,
            total: self.download_total_bytes,
        }
    }
}

/// Type of a role value.
enum RoleValue<'a> {
    Text(&'a str),
    Number(i32),
    Unsigned(u64),
    Flag(bool),
    /// waywallen 0.6.1: headers and badges, handed to QML as-is.
    Json(&'a Value),
    /// `itemKey`.
    Key,
    /// `item` is the whole row.
    Whole,
}

/// An observable model event. The order of these events is as much a part
/// of the contract as the role values: the golden file was captured from the
/// LIVE Python model precisely as a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelEvent {
    /// Emitted by QAbstractItemModel itself in endResetModel.
    ModelReset,
    /// Emitted by QAbstractItemModel itself in endInsertRows.
    RowsInserted { first: usize, last: usize },
    CountChanged,
    HasMoreChanged(bool),
    ReqFetchMore(usize),
    /// Always EXACTLY one role: a "wide" dataChanged (an empty role set) would
    /// make the view recompute the whole card — that is exactly what the bench
    /// counts separately and treats as a failure.
    DataChanged {
        first: usize,
        last: usize,
        role: &'static str,
    },
}

impl ModelEvent {
    /// Structural events are emitted by the base class in the end* methods,
    /// Rust does not send them. This is the only place where this knowledge is
    /// written down.
    pub fn is_structural(&self) -> bool {
        matches!(self, Self::ModelReset | Self::RowsInserted { .. })
    }
}

/// Model state: the result rows and the "there are more pages" flag.
#[derive(Debug, Default, Clone)]
pub struct RemoteRows {
    rows: Vec<RemoteRow>,
    has_more: bool,
}

impl RemoteRows {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn rows(&self) -> &[RemoteRow] {
        &self.rows
    }

    /// Row by number; None means there is no such number (negative ones
    /// included).
    pub fn row(&self, row: i32) -> Option<&RemoteRow> {
        usize::try_from(row).ok().and_then(|r| self.rows.get(r))
    }

    /// canFetchMore: in a flat list only the root has children, so for a valid
    /// parent it is always false.
    pub fn can_fetch_more(&self, root: bool) -> bool {
        root && self.has_more
    }

    /// A single change of the load-more flag.
    pub fn set_has_more(&mut self, value: bool) -> Vec<ModelEvent> {
        if self.has_more == value {
            return Vec::new();
        }
        self.has_more = value;
        vec![ModelEvent::HasMoreChanged(value)]
    }

    /// A full replacement of the results (a new search, a new sort, a new
    /// source).
    pub fn reset_rows(&mut self, rows: Vec<RemoteRow>, has_more: bool) -> Vec<ModelEvent> {
        // The flag is compared BEFORE the rows are replaced — just as in the
        // original.
        let changed = self.has_more != has_more;
        self.rows = rows;
        self.has_more = has_more;
        let mut events = vec![ModelEvent::ModelReset, ModelEvent::CountChanged];
        if changed {
            events.push(ModelEvent::HasMoreChanged(has_more));
        }
        events
    }

    /// The row range that append_rows will add. None means there is nothing to
    /// add, and beginInsertRows must not be called. A separate method because
    /// Qt requires announcing the insertion BEFORE editing the state.
    pub fn insert_range(&self, added: usize) -> Option<(usize, usize)> {
        if added == 0 {
            return None;
        }
        let first = self.rows.len();
        Some((first, first + added - 1))
    }

    /// Loading the next page.
    pub fn append_rows(&mut self, rows: Vec<RemoteRow>, has_more: bool) -> Vec<ModelEvent> {
        // An empty page adds no rows — there is nothing to announce an insertion
        // with, only the load-more flag remains.
        if rows.is_empty() {
            return self.set_has_more(has_more);
        }
        let changed = self.has_more != has_more;
        let (first, last) = self
            .insert_range(rows.len())
            .expect("непустая страница всегда даёт диапазон");
        self.rows.extend(rows);
        self.has_more = has_more;
        let mut events = vec![
            ModelEvent::RowsInserted { first, last },
            ModelEvent::CountChanged,
        ];
        if changed {
            events.push(ModelEvent::HasMoreChanged(has_more));
        }
        events
    }

    /// The infinite scrolling protocol. The flag is cleared BEFORE the request
    /// — otherwise the view would poke fetchMore again while the page is in
    /// flight.
    pub fn fetch_more(&mut self, root: bool) -> Vec<ModelEvent> {
        if !root || !self.has_more {
            return Vec::new();
        }
        let mut events = self.set_has_more(false);
        events.push(ModelEvent::ReqFetchMore(self.rows.len()));
        events
    }

    /// Acquisition state. BOTH identifiers must match: the same item may lie in
    /// the results of two sources.
    pub fn set_acquisition_state(
        &mut self,
        source_id: &str,
        item_id: &str,
        state: i32,
    ) -> Vec<ModelEvent> {
        self.set_acquisition(source_id, item_id, state, acquisition::Transfer::default())
    }

    /// Acquisition state together with the transfer progress (waywallen 0.6.0,
    /// `setRemoteAcquisitionState` with bytes). For every changed role — its
    /// own `dataChanged` with one role.
    pub fn set_acquisition(
        &mut self,
        source_id: &str,
        item_id: &str,
        state: i32,
        transfer: acquisition::Transfer,
    ) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        for (row, entry) in self.rows.iter_mut().enumerate() {
            if entry.source_id != source_id || entry.item_id != item_id {
                continue;
            }
            // The same value — no signal: otherwise every subscription check would
            // repaint the card for nothing.
            let before = entry.transfer();
            if entry.acquisition_state != state {
                entry.acquisition_state = state;
                events.push(ModelEvent::DataChanged {
                    first: row,
                    last: row,
                    role: ROLE_NAMES[ROLE_ACQUISITION_STATE],
                });
            }
            if before == transfer {
                continue;
            }
            entry.set_transfer(transfer);
            for (slot, changed) in [
                (ROLE_TRANSFER_ACTIVE, before.active != transfer.active),
                (ROLE_TRANSFER_RECEIVED, before.received != transfer.received),
                (ROLE_TRANSFER_TOTAL, before.total != transfer.total),
            ] {
                if changed {
                    events.push(ModelEvent::DataChanged {
                        first: row,
                        last: row,
                        role: ROLE_NAMES[slot],
                    });
                }
            }
        }
        events
    }

    /// Boolean input of the old bridge: "downloaded" → 3, otherwise 0.
    pub fn set_installed(
        &mut self,
        source_id: &str,
        item_id: &str,
        installed: bool,
    ) -> Vec<ModelEvent> {
        self.set_acquisition_state(source_id, item_id, if installed { 3 } else { 0 })
    }

    /// Add author names to already shown cards. An empty name does not
    /// overwrite the shown one (in the original this is `if name and ...`).
    pub fn set_authors(&mut self, names: &[(String, String)]) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        for (row, entry) in self.rows.iter_mut().enumerate() {
            let Some((_, name)) = names.iter().find(|(id, _)| *id == entry.item_id) else {
                continue;
            };
            if name.is_empty() || entry.author == *name {
                continue;
            }
            entry.author.clone_from(name);
            events.push(ModelEvent::DataChanged {
                first: row,
                last: row,
                role: ROLE_NAMES[ROLE_AUTHOR],
            });
        }
        events
    }

    /// Add author avatars — by the same targeted path as the names. An empty
    /// address does not overwrite the shown one: an avatar that did not
    /// download has no right to erase one already shown.
    pub fn set_author_avatars(&mut self, avatars: &[(String, String)]) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        for (row, entry) in self.rows.iter_mut().enumerate() {
            let Some((_, url)) = avatars.iter().find(|(id, _)| *id == entry.item_id) else {
                continue;
            };
            if url.is_empty() || entry.author_avatar_url == *url {
                continue;
            }
            entry.author_avatar_url.clone_from(url);
            events.push(ModelEvent::DataChanged {
                first: row,
                last: row,
                role: ROLE_NAMES[ROLE_AUTHOR_AVATAR],
            });
        }
        events
    }
}

/// Indices of the roles that are updated in a targeted way.
const ROLE_AUTHOR: usize = 4;
const ROLE_ACQUISITION_STATE: usize = 6;
const ROLE_AUTHOR_AVATAR: usize = 7;
const ROLE_TRANSFER_ACTIVE: usize = 11;
const ROLE_TRANSFER_RECEIVED: usize = 12;
const ROLE_TRANSFER_TOTAL: usize = 13;

/// Parse a result page from a JSON array. A broken JSON gives an empty page
/// — silently, like parsing the overrides in the property editor: a parse
/// error must not bring the tab down.
pub fn parse_rows(json: &str) -> Vec<RemoteRow> {
    serde_json::from_str::<Vec<RemoteRow>>(json).unwrap_or_default()
}

// ============================================================================
// Shared store of acquisition states (a port of _AcquisitionStore / AppStore)
// ============================================================================

/// Store of acquisition states: upstream the result rows and the details
/// panel look into ONE storage, so after subscribing the button immediately
/// becomes "Unsubscribe" without reloading the page. This is a bug already
/// fixed during the port to Python, and the regression must not be
/// preserved — the store moves together with the model.
///
/// The store's design is the main difference from Python and the point of
/// the whole batch. In Python the store keeps a BOUND METHOD of the model
/// (`changed.connect(self.setAcquisitionState)`), that is, it keeps the
/// model alive forever while knowing nothing about the death of its C++
/// half — hence the historical "Internal C++ object already deleted". Here
/// the store keeps a `CxxQtThread<RemoteListModel>`: it is `Send + Sync`,
/// delivery goes through the event loop queue, and a dead object gives
/// `ObjectDestroyed` instead of accessing freed memory. The entry is removed
/// from the registry either in Drop of the Rust half or on the very first
/// failed broadcast.
pub mod acquisition {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    /// File transfer progress (waywallen 0.6.0, `RemoteDownloadTransfer`):
    /// whether it is in progress, how many bytes arrived and how many in total
    /// (0 — unknown).
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Transfer {
        pub active: bool,
        pub received: u64,
        pub total: u64,
    }

    /// Delivering a state to ONE receiver. `Err(())` means the object died and
    /// the entry should be removed.
    ///
    /// A closure rather than `CxxQtThread<RemoteListModel>`: the store is read
    /// not only by the result model but also by the single item holder
    /// (`RemoteStoreItem`, the Discover details panel) — upstream this is ONE
    /// storage for both. `CxxQtThread` cannot be made generic over the type:
    /// cxx-qt emits its own alias for every QObject.
    pub type Deliver = Box<dyn Fn(&str, &str, i32, Transfer) -> Result<(), ()> + Send>;

    #[derive(Default)]
    struct Registry {
        next_id: u64,
        entries: Vec<(u64, Deliver)>,
        /// The last broadcast state of an item while it is "in transit" (1 —
        /// waiting, 2 — downloading). By it new results do not overwrite a running
        /// download (the original's `mergeRemoteRow`).
        in_flight: HashMap<(String, String), (i32, Transfer)>,
    }

    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    /// Register a receiver. Returns the entry number for removal in Drop.
    pub fn register(deliver: Deliver) -> u64 {
        let mut reg = registry().lock().expect("реестр склада не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((id, deliver));
        id
    }

    /// Remove an entry. Idempotent: the entry may already be gone if a
    /// broadcast got ObjectDestroyed before the destructor.
    pub fn unregister(id: u64) {
        let mut reg = registry().lock().expect("реестр склада не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// How many models are in the registry right now (for the bench and
    /// debugging).
    pub fn registered() -> usize {
        registry()
            .lock()
            .expect("реестр склада не отравлен")
            .entries
            .len()
    }

    /// Broadcast a state to all live models. Returns the number of receivers
    /// for which the task was queued successfully.
    ///
    /// The store discards empty identifiers — just like the original.
    pub fn set(source_id: &str, item_id: &str, state: i32) -> usize {
        set_progress(source_id, item_id, state, Transfer::default())
    }

    /// The same as [`set`], together with the transfer progress.
    pub fn set_progress(source_id: &str, item_id: &str, state: i32, transfer: Transfer) -> usize {
        if source_id.is_empty() || item_id.is_empty() {
            return 0;
        }
        let mut delivered = 0usize;
        let mut reg = registry().lock().expect("реестр склада не отравлен");
        let key = (source_id.to_string(), item_id.to_string());
        if state == 1 || state == 2 {
            reg.in_flight.insert(key, (state, transfer));
        } else {
            reg.in_flight.remove(&key);
        }
        // retain rather than iteration: the entry of a dead receiver is removed
        // here.
        reg.entries.retain(|(_, deliver)| match deliver(source_id, item_id, state, transfer) {
            Ok(()) => {
                delivered += 1;
                true
            }
            Err(()) => false,
        });
        delivered
    }

    /// Forget what is downloading (for the bench: every scenario starts with a
    /// clean store — otherwise an unfinished download of the previous scenario
    /// would mix into the results of the next one).
    pub fn forget_in_flight() {
        registry().lock().expect("реестр склада не отравлен").in_flight.clear();
    }

    /// The original's `mergeRemoteRow`: while an item is downloading, fresh
    /// results (where it is still "not downloaded") do not reset the shown
    /// download progress.
    pub fn merge(row: &mut super::RemoteRow) {
        if row.acquisition_state == 3 {
            return;
        }
        let reg = registry().lock().expect("реестр склада не отравлен");
        if let Some((state, transfer)) =
            reg.in_flight.get(&(row.source_id.clone(), row.item_id.clone()))
        {
            row.acquisition_state = *state;
            row.set_transfer(*transfer);
        }
    }
}

// ============================================================================
// Qt layer
// ============================================================================

/// Rust state of the model.
#[derive(Default)]
pub struct RemoteListModelRust {
    rows: RemoteRows,
    /// Entry number in the shared store; 0 means not registered.
    store_id: u64,
}

impl Drop for RemoteListModelRust {
    fn drop(&mut self) {
        if self.store_id != 0 {
            acquisition::unregister(self.store_id);
        }
    }
}

/// A role set of one number — what goes as the third argument of
/// dataChanged. An EMPTY vector would mean "all roles changed", and the view
/// would recompute the whole card; here that never happens.
fn role_vector(name: &str) -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    if let Some(slot) = ROLE_NAMES.iter().position(|n| *n == name) {
        roles.append(ROLE_BASE + slot as i32);
    }
    roles
}

impl cxx_qt::Initialize for qobject::RemoteListModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Registration in the shared store — in post-construction: qt_thread()
        // requires an already constructed QObject.
        let handle = self.as_ref().get_ref().qt_thread();
        // Neither unwrap() nor a preceding is_destroyed(): between the check and
        // the queueing the object may die.
        let id = acquisition::register(Box::new(move |source_id, item_id, state, transfer| {
            let source = source_id.to_string();
            let item = item_id.to_string();
            match handle.queue(move |mut model| {
                let events =
                    model.as_mut().rust_mut().rows.set_acquisition(&source, &item, state, transfer);
                model.emit_events(&events);
            }) {
                Ok(()) => Ok(()),
                // The object died — the entry is no longer needed. Other errors
                // (invokeMethod failed) do not bury the object.
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed) => Err(()),
                Err(_) => Ok(()),
            }
        }));
        self.as_mut().rust_mut().store_id = id;
    }
}

impl qobject::RemoteListModel {
    // ── Properties ────────────────────────────────────────────────────────
    // pub: the batch 3 query bench reads the row counter.
    pub fn count(&self) -> i32 {
        self.rows.len() as i32
    }

    // pub: the load-more flag is also read by the owning query (batch 3) — its
    // `hasMore` property is not its own but a retelling of the model, as in the
    // bridge (`return self._model.hasMore`).
    pub fn has_more(&self) -> bool {
        self.rows.has_more()
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // In a flat list only the root has children.
        if parent.is_valid() {
            0
        } else {
            self.rows.len() as i32
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        for (slot, name) in ROLE_NAMES.iter().enumerate() {
            names.insert(ROLE_BASE + slot as i32, QByteArray::from(*name));
        }
        names
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if !index.is_valid() {
            return QVariant::default();
        }
        let Some(entry) = self.rows.row(index.row()) else {
            return QVariant::default();
        };
        let Ok(slot) = usize::try_from(role - ROLE_BASE) else {
            return QVariant::default();
        };
        entry.role_variant(slot)
    }

    fn can_fetch_more(&self, parent: &QModelIndex) -> bool {
        self.rows.can_fetch_more(!parent.is_valid())
    }

    // pub: loading more is triggered by the batch 3 query bench — exactly as
    // GridView does it, through the model's own protocol.
    pub fn fetch_more(mut self: Pin<&mut Self>, parent: &QModelIndex) {
        let root = !parent.is_valid();
        let events = self.as_mut().rust_mut().rows.fetch_more(root);
        self.emit_events(&events);
    }

    // ── API for QML ───────────────────────────────────────────────────────
    // pub: the batch 3 query bench reads a whole row.
    pub fn get(&self, row: i32) -> qobject::QMap_QString_QVariant {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        let Some(entry) = self.rows.row(row) else {
            // No row — an empty map, as in the original (`return {}`).
            return map;
        };
        for (slot, name) in ROLE_NAMES.iter().enumerate() {
            if matches!(*name, "item" | "itemKey") {
                continue;
            }
            map.insert(QString::from(*name), entry.role_variant(slot));
        }
        map
    }

    fn item(&self, row: i32) -> qobject::QMap_QString_QVariant {
        self.get(row)
    }

    fn set_installed(
        mut self: Pin<&mut Self>,
        source_id: &QString,
        item_id: &QString,
        installed: bool,
    ) {
        let events = self.as_mut().rust_mut().rows.set_installed(
            &source_id.to_string(),
            &item_id.to_string(),
            installed,
        );
        self.emit_events(&events);
    }

    fn set_acquisition_state(
        mut self: Pin<&mut Self>,
        source_id: &QString,
        item_id: &QString,
        state: i32,
    ) {
        let events = self.as_mut().rust_mut().rows.set_acquisition_state(
            &source_id.to_string(),
            &item_id.to_string(),
            state,
        );
        self.emit_events(&events);
    }

    fn set_authors(mut self: Pin<&mut Self>, names: &qobject::QMap_QString_QVariant) {
        let names: Vec<(String, String)> = names
            .iter()
            .map(|(key, value)| {
                let text = value
                    .value::<QString>()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                (key.to_string(), text)
            })
            .collect();
        let events = self.as_mut().rust_mut().rows.set_authors(&names);
        self.emit_events(&events);
    }

    fn set_author_avatars(mut self: Pin<&mut Self>, avatars: &qobject::QMap_QString_QVariant) {
        let avatars: Vec<(String, String)> = avatars
            .iter()
            .map(|(key, value)| {
                let text = value
                    .value::<QString>()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                (key.to_string(), text)
            })
            .collect();
        let events = self.as_mut().rust_mut().rows.set_author_avatars(&avatars);
        self.emit_events(&events);
    }

    fn apply_page_json(
        mut self: Pin<&mut Self>,
        json: &QString,
        has_more: bool,
        append: bool,
    ) -> i32 {
        let rows = parse_rows(&json.to_string());
        let parsed = rows.len() as i32;
        if append {
            self.as_mut().append_page(rows, has_more);
        } else {
            self.as_mut().reset_page(rows, has_more);
        }
        parsed
    }

    fn role_number(&self, name: &QString) -> i32 {
        let name = name.to_string();
        ROLE_NAMES
            .iter()
            .position(|n| *n == name)
            .map(|slot| ROLE_BASE + slot as i32)
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

    fn publish_acquisition_state(
        &self,
        source_id: &QString,
        item_id: &QString,
        state: i32,
    ) -> i32 {
        acquisition::set(&source_id.to_string(), &item_id.to_string(), state) as i32
    }

    // ── Rust API (the batch 3 query will use it) ───────────────────────────
    /// A full replacement of the results: a model reset, then the trailing
    /// signals.
    pub fn reset_page(mut self: Pin<&mut Self>, rows: Vec<RemoteRow>, has_more: bool) {
        let events = unsafe {
            self.as_mut().begin_reset_model();
            let events = self.as_mut().rust_mut().rows.reset_rows(rows, has_more);
            self.as_mut().end_reset_model();
            events
        };
        self.emit_events(&events);
    }

    /// A single change of the load-more flag. The batch 3 owning query uses it:
    /// the original clears `hasMore` BEFORE sending the page, otherwise GridView
    /// pokes `fetchMore` again while the page is in flight.
    pub fn set_page_has_more(mut self: Pin<&mut Self>, value: bool) {
        let events = self.as_mut().rust_mut().rows.set_has_more(value);
        self.emit_events(&events);
    }

    /// Loading a page: announce the insertion, append, close, then the tail.
    pub fn append_page(mut self: Pin<&mut Self>, rows: Vec<RemoteRow>, has_more: bool) {
        // The range is computed BEFORE editing the state: beginInsertRows must
        // know it in advance.
        let range = self.rows.insert_range(rows.len());
        let events = unsafe {
            if let Some((first, last)) = range {
                self.as_mut().begin_insert_rows(
                    &QModelIndex::default(),
                    first as i32,
                    last as i32,
                );
            }
            let events = self.as_mut().rust_mut().rows.append_rows(rows, has_more);
            if range.is_some() {
                self.as_mut().end_insert_rows();
            }
            events
        };
        self.emit_events(&events);
    }

    // ── Internals ─────────────────────────────────────────────────────────
    /// The only place where events of the pure layer turn into Qt signals.
    /// Structural ones (modelReset/rowsInserted) are skipped: QAbstractItemModel
    /// itself emits them in end_reset_model / end_insert_rows.
    ///
    /// Warning: the values are taken LIVE, not as planned, and this is not
    /// nitpicking. GridView asks `canFetchMore`/`fetchMore` right from the
    /// `modelReset` handler, that is, REENTRANTLY — still inside
    /// `end_reset_model()`, before the reset has finished its tail of signals.
    /// By the time the queue reaches the trailing `hasMoreChanged`, the flag has
    /// already been knocked to false by loading more, and sending the planned
    /// `true` would lie to the view. The original sends exactly the current
    /// state (`self.hasMoreChanged.emit(self._has_more)`, bridge.py:691) — the
    /// same here.
    fn emit_events(mut self: Pin<&mut Self>, events: &[ModelEvent]) {
        for event in events {
            match event {
                ModelEvent::ModelReset | ModelEvent::RowsInserted { .. } => {}
                ModelEvent::CountChanged => self.as_mut().count_changed(),
                ModelEvent::HasMoreChanged(_) => {
                    let live = self.has_more();
                    self.as_mut().has_more_changed(live);
                }
                ModelEvent::ReqFetchMore(_) => {
                    let live = self.count();
                    self.as_mut().req_fetch_more(live);
                }
                ModelEvent::DataChanged { first, last, role } => {
                    let roles = role_vector(role);
                    let root = QModelIndex::default();
                    let top_left = self.index(*first as i32, 0, &root);
                    let bottom_right = self.index(*last as i32, 0, &root);
                    self.as_mut().data_changed(&top_left, &bottom_right, &roles);
                }
            }
        }
    }
}

// ============================================================================
// Golden: parsing the reference captured from the LIVE Python model
// ============================================================================

/// Parsing `tests/golden/remote_list_cases.json`. Lives outside
/// `#[cfg(test)]` because the `selftest::run()` bench uses the same
/// reference: the pure layer is checked by `cargo test`, and the Qt layer by
/// a real QObject.
pub mod golden {
    use super::{ModelEvent, RemoteRow, RemoteRows};
    use serde::Deserialize;
    use serde_json::Value;

    /// The whole reference.
    #[derive(Debug, Deserialize)]
    pub struct Golden {
        pub cases: Vec<Case>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Case {
        pub name: String,
        pub models: usize,
        pub steps: Vec<Step>,
        pub events: Vec<Event>,
        pub final_: Vec<Final>,
    }

    // "final" is a Rust keyword, which does not get in the way only as a field
    // identifier, but the serde name in the golden file is exactly final, hence
    // the rename.
    #[derive(Debug, Deserialize)]
    pub struct Step {
        pub op: String,
        #[serde(default)]
        pub model: usize,
        #[serde(default)]
        pub rows: Vec<RemoteRow>,
        #[serde(default, rename = "hasMore")]
        pub has_more: bool,
        #[serde(default, rename = "sourceId")]
        pub source_id: String,
        #[serde(default, rename = "itemId")]
        pub item_id: String,
        #[serde(default)]
        pub state: i32,
        #[serde(default)]
        pub installed: bool,
        #[serde(default)]
        pub names: serde_json::Map<String, Value>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Event {
        pub model: usize,
        pub sig: String,
        #[serde(default)]
        pub first: usize,
        #[serde(default)]
        pub last: usize,
        #[serde(default)]
        pub roles: Vec<String>,
        #[serde(default)]
        pub value: bool,
        #[serde(default)]
        pub row: usize,
    }

    #[derive(Debug, Deserialize)]
    pub struct Final {
        pub count: usize,
        #[serde(rename = "hasMore")]
        pub has_more: bool,
        #[serde(rename = "canFetchMore")]
        pub can_fetch_more: bool,
        #[serde(rename = "canFetchMoreChild")]
        pub can_fetch_more_child: bool,
        pub rows: Vec<RemoteRow>,
        #[serde(rename = "getFirst")]
        pub get_first: Value,
        #[serde(rename = "getOutOfRange")]
        pub get_out_of_range: Value,
        #[serde(rename = "getNegative")]
        pub get_negative: Value,
        #[serde(rename = "itemFirst")]
        pub item_first: Value,
    }

    /// The golden file in the crate sources — the very file printed by
    /// tests/golden/remote_list_cases.py from the live Python bridge.
    pub const GOLDEN_JSON: &str = include_str!("../tests/golden/remote_list_cases.json");

    pub fn load() -> Golden {
        // serde cannot handle the key "final" without a rename, and the golden
        // file writes exactly that: we fix it on the fly so as not to introduce a
        // second format.
        let raw: Value = serde_json::from_str(GOLDEN_JSON).expect("голден читается");
        let mut fixed = raw;
        if let Some(cases) = fixed.get_mut("cases").and_then(Value::as_array_mut) {
            for case in cases {
                if let Some(object) = case.as_object_mut() {
                    if let Some(value) = object.remove("final") {
                        object.insert("final_".to_string(), value);
                    }
                }
            }
        }
        serde_json::from_value(fixed).expect("голден разбирается в структуры")
    }

    /// An expected golden event → a model event. A "wide" dataChanged is also
    /// caught here: in the reference it ALWAYS has exactly one role, and a set
    /// of zero or two roles will fail the parsing.
    pub fn expected_event(event: &Event) -> (usize, ModelEvent) {
        let model = event.model;
        let value = match event.sig.as_str() {
            "modelReset" => ModelEvent::ModelReset,
            "rowsInserted" => ModelEvent::RowsInserted {
                first: event.first,
                last: event.last,
            },
            "countChanged" => ModelEvent::CountChanged,
            "hasMoreChanged" => ModelEvent::HasMoreChanged(event.value),
            "reqFetchMore" => ModelEvent::ReqFetchMore(event.row),
            "dataChanged" => {
                assert_eq!(
                    event.roles.len(),
                    1,
                    "голден: dataChanged обязан нести РОВНО одну роль, а несёт {:?}",
                    event.roles
                );
                let name = super::ROLE_NAMES
                    .iter()
                    .find(|n| **n == event.roles[0])
                    .unwrap_or_else(|| panic!("голден: неизвестная роль {}", event.roles[0]));
                ModelEvent::DataChanged {
                    first: event.first,
                    last: event.last,
                    role: name,
                }
            }
            other => panic!("голден: неизвестный сигнал {other}"),
        };
        (model, value)
    }

    /// Author names from the golden file in record order.
    pub fn names_of(step: &Step) -> Vec<(String, String)> {
        step.names
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    value.as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    /// Run a case through the PURE layer and return the model state together
    /// with the event journal in order of occurrence.
    pub fn replay_pure(case: &Case) -> (Vec<RemoteRows>, Vec<(usize, ModelEvent)>) {
        let mut models: Vec<RemoteRows> = (0..case.models).map(|_| RemoteRows::default()).collect();
        let mut journal: Vec<(usize, ModelEvent)> = Vec::new();

        for step in &case.steps {
            let index = step.model;
            let events = match step.op.as_str() {
                "resetRows" => models[index].reset_rows(step.rows.clone(), step.has_more),
                "appendRows" => models[index].append_rows(step.rows.clone(), step.has_more),
                "fetchMore" => models[index].fetch_more(true),
                "setAcquisitionState" => {
                    models[index].set_acquisition_state(&step.source_id, &step.item_id, step.state)
                }
                "setInstalled" => {
                    models[index].set_installed(&step.source_id, &step.item_id, step.installed)
                }
                "setAuthors" => models[index].set_authors(&names_of(step)),
                "setAuthorAvatars" => models[index].set_author_avatars(&names_of(step)),
                "storeSet" => {
                    // The store broadcasts to ALL live models; it discards empty identifiers.
                    if step.source_id.is_empty() || step.item_id.is_empty() {
                        Vec::new()
                    } else {
                        for (i, model) in models.iter_mut().enumerate() {
                            let events = model.set_acquisition_state(
                                &step.source_id,
                                &step.item_id,
                                step.state,
                            );
                            journal.extend(events.into_iter().map(|e| (i, e)));
                        }
                        Vec::new()
                    }
                }
                other => panic!("голден: неизвестное действие {other}"),
            };
            journal.extend(events.into_iter().map(|e| (index, e)));
        }
        (models, journal)
    }
}

// ============================================================================
// Bench: late callbacks and the Qt layer on a live QObject
// ============================================================================

/// Self-test of the Qt layer without QML and without a window.
///
/// Here we prove what the batch exists for: the model is deleted while a
/// callback is still in flight — and that is not a crash but
/// `ObjectDestroyed`. Along the way all golden cases are run through a REAL
/// QObject, not only through the pure layer: the full signal order
/// (including the modelReset/rowsInserted of QAbstractItemModel itself), the
/// role numbers in dataChanged, the role values through data() and the
/// contents of get()/item() are compared.
///
/// Return: 0 — everything matched, 1 — there are mismatches. There is no
/// silent success by timeout: the bench does not wait for anything it
/// cannot wait for.
pub mod selftest {
    use super::golden;
    use super::qobject::{
        rexon_delete_object, rexon_new_remote_list_model, rexon_process_events, RemoteListModel,
    };
    use super::{acquisition, ModelEvent, RemoteRow, GOLDEN_ROLES, ROLE_BASE, ROLE_NAMES};
    use core::pin::Pin;
    use cxx_qt::{Threading, ThreadingQueueError};
    use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant};
    use std::sync::{Arc, Mutex};

    pub const ARG: &str = "--remote-selftest";

    /// A live model object created by the factory without a parent. Removed
    /// only by an explicit `destroy()` — the bench needs a controlled death.
    struct OwnedModel {
        raw: *mut super::qobject::QObject,
    }

    impl OwnedModel {
        fn new() -> Self {
            let raw = unsafe { rexon_new_remote_list_model(core::ptr::null_mut()) };
            assert!(!raw.is_null(), "фабрика вернула nullptr");
            Self { raw }
        }

        /// A pointer to the subclass: single inheritance, the address is the same.
        fn model(&self) -> *mut RemoteListModel {
            self.raw.cast()
        }

        // A mutable reference from an immutable one — this is intended: Qt owns the
        // object, not Rust, and "mutability" here is about the C++ object, not
        // about the raw field.
        #[allow(clippy::mut_from_ref)]
        fn pinned(&self) -> Pin<&mut RemoteListModel> {
            unsafe { Pin::new_unchecked(&mut *self.model()) }
        }

        fn destroy(self) {
            unsafe { rexon_delete_object(self.raw) };
        }
    }

    /// The collected signal journal of a live QObject.
    type Journal = Arc<Mutex<Vec<ModelEvent>>>;

    /// Subscribe to the observable signals of the model. The returned connection
    /// "guards" must live while the case runs: their Drop breaks the
    /// connection.
    ///
    /// There are no structural signals (modelReset, rowsInserted) here and
    /// cannot be: they are private (QPrivateSignal), and Rust cannot subscribe
    /// to them. Their order is checked by the pure layer in `cargo test` and by
    /// the live view in the QML bench — see the module header.
    fn connect_journal(
        model: Pin<&mut RemoteListModel>,
        journal: &Journal,
    ) -> Vec<cxx_qt::QMetaObjectConnectionGuard> {
        let mut guards = Vec::new();
        let raw = unsafe { model.get_unchecked_mut() as *mut RemoteListModel };
        let pin = || unsafe { Pin::new_unchecked(&mut *raw) };

        let sink = Arc::clone(journal);
        guards.push(pin().on_count_changed(move |_| {
            sink.lock().expect("журнал").push(ModelEvent::CountChanged);
        }));

        let sink = Arc::clone(journal);
        guards.push(pin().on_has_more_changed(move |_, value| {
            sink.lock()
                .expect("журнал")
                .push(ModelEvent::HasMoreChanged(value));
        }));

        let sink = Arc::clone(journal);
        guards.push(pin().on_req_fetch_more(move |_, row| {
            sink.lock()
                .expect("журнал")
                .push(ModelEvent::ReqFetchMore(row as usize));
        }));

        let sink = Arc::clone(journal);
        guards.push(pin().on_data_changed(move |_, top_left, bottom_right, roles| {
            // Roles arrive as numbers — we translate them into names IN THE SAME way
            // the golden file prints them. An empty or multi-role set will not turn
            // into one role here: it will get into the journal as "wide" and will not
            // match the reference.
            let names: Vec<&'static str> = (0..roles.len())
                .filter_map(|i| roles.get(i).copied())
                .filter_map(|role| {
                    usize::try_from(role - ROLE_BASE)
                        .ok()
                        .and_then(|slot| ROLE_NAMES.get(slot).copied())
                })
                .collect();
            let role = if names.len() == 1 { names[0] } else { "<широкий>" };
            sink.lock().expect("журнал").push(ModelEvent::DataChanged {
                first: top_left.row() as usize,
                last: bottom_right.row() as usize,
                role,
            });
        }));

        guards
    }

    /// A role value of the live model by name — through the real data().
    fn role_text(model: &RemoteListModel, row: i32, name: &str) -> String {
        let value = model.role_value(row, &QString::from(name));
        variant_text(&value)
    }

    fn variant_text(value: &QVariant) -> String {
        if let Some(text) = value.value::<QString>() {
            return text.to_string();
        }
        match value.value::<i32>() {
            Some(number) => number.to_string(),
            None => String::new(),
        }
    }

    /// A row of the live model in the same JSON form the golden file prints.
    fn map_to_json(map: &QMap<QMapPair_QString_QVariant>) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        for (key, value) in map.iter() {
            let name = key.to_string();
            if !ROLE_NAMES[..GOLDEN_ROLES].contains(&name.as_str()) {
                continue;
            }
            let json = if name == "acquisitionState" {
                serde_json::Value::from(value.value::<i32>().unwrap_or_default())
            } else {
                serde_json::Value::String(variant_text(value))
            };
            object.insert(name, json);
        }
        serde_json::Value::Object(object)
    }

    struct Report {
        failures: Vec<String>,
        wide_updates: usize,
        cases: usize,
        events: usize,
    }

    impl Report {
        fn check(&mut self, condition: bool, message: impl FnOnce() -> String) {
            if !condition {
                self.failures.push(message());
            }
        }
    }

    /// Running one golden case through a live QObject.
    fn run_case(case: &golden::Case, report: &mut Report) {
        let models: Vec<OwnedModel> = (0..case.models).map(|_| OwnedModel::new()).collect();
        let journals: Vec<Journal> = (0..case.models)
            .map(|_| Arc::new(Mutex::new(Vec::new())))
            .collect();
        let _guards: Vec<_> = models
            .iter()
            .zip(&journals)
            .map(|(model, journal)| connect_journal(model.pinned(), journal))
            .collect();

        for step in &case.steps {
            let model = models[step.model].pinned();
            match step.op.as_str() {
                "resetRows" => model.reset_page(step.rows.clone(), step.has_more),
                "appendRows" => model.append_page(step.rows.clone(), step.has_more),
                "fetchMore" => model.fetch_more(&QModelIndex::default()),
                "setAcquisitionState" => model.set_acquisition_state(
                    &QString::from(&step.source_id),
                    &QString::from(&step.item_id),
                    step.state,
                ),
                "setInstalled" => model.set_installed(
                    &QString::from(&step.source_id),
                    &QString::from(&step.item_id),
                    step.installed,
                ),
                "setAuthors" => {
                    let mut names = QMap::<QMapPair_QString_QVariant>::default();
                    for (key, value) in golden::names_of(step) {
                        names.insert(QString::from(&key), QVariant::from(&QString::from(&value)));
                    }
                    model.set_authors(&names);
                }
                // Avatars arrive in a second pass, with a SEPARATE invokable: the name is
                // already drawn, the picture catches up with its own role.
                "setAuthorAvatars" => {
                    let mut avatars = QMap::<QMapPair_QString_QVariant>::default();
                    for (key, value) in golden::names_of(step) {
                        avatars
                            .insert(QString::from(&key), QVariant::from(&QString::from(&value)));
                    }
                    model.set_author_avatars(&avatars);
                }
                "storeSet" => {
                    model.publish_acquisition_state(
                        &QString::from(&step.source_id),
                        &QString::from(&step.item_id),
                        step.state,
                    );
                    // The store delivers through the event loop queue: in Python this is a
                    // direct connection, here it is a queue, and it has to be spun.
                    rexon_process_events(50);
                }
                other => panic!("стенд: неизвестное действие {other}"),
            }
        }
        // The tail of the store queue in case the last step was storeSet.
        rexon_process_events(50);

        // ── journal comparison ─────────────────────────────────────────────
        let mut expected: Vec<Vec<ModelEvent>> = vec![Vec::new(); case.models];
        for event in &case.events {
            let (index, value) = golden::expected_event(event);
            // Structural signals are thrown out of the expected sequence: Rust cannot
            // subscribe to them (see connect_journal). Their order is checked by the
            // pure layer in cargo test and by the live view in QML.
            if value.is_structural() {
                continue;
            }
            expected[index].push(value);
        }
        for (index, journal) in journals.iter().enumerate() {
            let actual = journal.lock().expect("журнал").clone();
            report.events += actual.len();
            report.wide_updates += actual
                .iter()
                .filter(|e| matches!(e, ModelEvent::DataChanged { role, .. } if *role == "<широкий>"))
                .count();
            let want = &expected[index];
            report.check(actual == *want, || {
                format!(
                    "{} · модель {index}: порядок сигналов разошёлся\n  получено: {actual:?}\n  ожидалось: {want:?}",
                    case.name
                )
            });
        }

        // ── state comparison ──────────────────────────────────────────────
        for (index, want) in case.final_.iter().enumerate() {
            let model = models[index].pinned();
            let at = format!("{} · модель {index}", case.name);
            report.check(model.count() == want.count as i32, || {
                format!("{at}: count {} вместо {}", model.count(), want.count)
            });
            report.check(model.has_more() == want.has_more, || {
                format!("{at}: hasMore {} вместо {}", model.has_more(), want.has_more)
            });
            let root = QModelIndex::default();
            report.check(model.can_fetch_more(&root) == want.can_fetch_more, || {
                format!("{at}: canFetchMore разошёлся")
            });
            if want.count > 0 {
                let child = model.index(0, 0, &root);
                report.check(
                    model.can_fetch_more(&child) == want.can_fetch_more_child,
                    || format!("{at}: canFetchMore по строке-родителю разошёлся"),
                );
            }
            for (row, row_want) in want.rows.iter().enumerate() {
                let at = format!("{at} · строка {row} ({})", row_want.item_id);
                // The golden file was captured before waywallen 0.6.0: the new roles are
                // checked by `new_roles_reach_qml` below.
                for name in &ROLE_NAMES[..GOLDEN_ROLES] {
                    let actual = role_text(&model, row as i32, name);
                    let expected = expected_role_text(row_want, name);
                    report.check(actual == expected, || {
                        format!("{at}: роль {name} = «{actual}», ожидалась «{expected}»")
                    });
                }
            }
            report.check(map_to_json(&model.get(0)) == want.get_first, || {
                format!("{at}: get(0) разошёлся")
            });
            report.check(map_to_json(&model.item(0)) == want.item_first, || {
                format!("{at}: item(0) разошёлся")
            });
            report.check(
                map_to_json(&model.get(want.count as i32)) == want.get_out_of_range,
                || format!("{at}: get(за границей) разошёлся"),
            );
            report.check(map_to_json(&model.get(-1)) == want.get_negative, || {
                format!("{at}: get(-1) разошёлся")
            });
            // A role that does not exist must give -1 and an empty QVariant.
            report.check(model.role_number(&QString::from("такойРолиНет")) == -1, || {
                format!("{at}: несуществующая роль дала не -1")
            });
        }

        for model in models {
            model.destroy();
        }
    }

    fn expected_role_text(row: &RemoteRow, name: &str) -> String {
        match name {
            "itemId" => row.item_id.clone(),
            "sourceId" => row.source_id.clone(),
            "title" => row.title.clone(),
            "previewUrl" => row.preview_url.clone(),
            "author" => row.author.clone(),
            "wpType" => row.wp_type.clone(),
            "acquisitionState" => row.acquisition_state.to_string(),
            "authorAvatarUrl" => row.author_avatar_url.clone(),
            "authorUrl" => row.author_url.clone(),
            other => panic!("стенд: неизвестная роль {other}"),
        }
    }

    /// A sample page for the late callback scenarios.
    fn probe_page() -> Vec<RemoteRow> {
        vec![RemoteRow {
            item_id: "late".to_string(),
            source_id: "wallhaven".to_string(),
            title: "Обоина под поздний колбэк".to_string(),
            ..RemoteRow::default()
        }]
    }

    /// ⭐ The core of the batch: late callbacks on a deleted model.
    fn run_late_callbacks(report: &mut Report) {
        // ── 1. The model is ALIVE: the answer gets through ────────────────
        let alive = OwnedModel::new();
        alive.pinned().reset_page(probe_page(), false);
        let handle = unsafe { (*alive.model()).qt_thread() };
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            let result = worker.queue(|model| {
                model.set_acquisition_state(
                    &QString::from("wallhaven"),
                    &QString::from("late"),
                    3,
                );
            });
            tx.send(result.is_ok()).ok();
        });
        rexon_process_events(400);
        report.check(rx.recv().unwrap_or(false), || {
            "живая модель: постановка в очередь не удалась".to_string()
        });
        report.check(role_text(&alive.pinned(), 0, "acquisitionState") == "3", || {
            "живая модель: ответ из фонового потока не доехал".to_string()
        });
        alive.destroy();

        // ── 2. The model DIED, the answer is still in flight ──────────────
        // Exactly the scenario that gave "Internal C++ object (RemoteListModel)
        // already deleted": the tab was closed while the remote_search answer was
        // coming from the daemon.
        let doomed = OwnedModel::new();
        doomed.pinned().reset_page(probe_page(), false);
        let handle = unsafe { (*doomed.model()).qt_thread() };
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            // queue(...).ok() — without unwrap() and without is_destroyed(): between
            // the check and the queueing the object may die.
            let outcome = match worker.queue(|model| {
                model.set_acquisition_state(
                    &QString::from("wallhaven"),
                    &QString::from("late"),
                    3,
                );
            }) {
                Ok(()) => "доставлено".to_string(),
                Err(ThreadingQueueError::ObjectDestroyed) => "объект уничтожен".to_string(),
                Err(other) => format!("иная ошибка: {other}"),
            };
            tx.send(outcome).ok();
        });
        // The model dies BEFORE the background thread wakes up.
        doomed.destroy();
        rexon_process_events(500);
        let verdict = rx.recv().unwrap_or_else(|_| "поток не ответил".to_string());
        report.check(verdict == "объект уничтожен", || {
            format!("поздний колбэк на удалённую модель дал «{verdict}», а обязан был дать «объект уничтожен»")
        });
        // Getting here alive means there was no access to freed memory: in Python
        // this place crashed with a RuntimeError and was finished off by a second
        // exception in the handler.
        report.check(handle.is_destroyed(), || {
            "CxxQtThread не считает удалённую модель уничтоженной".to_string()
        });

        // ── 3. The answer is queued, the model died BEFORE it ran ─────────
        let racing = OwnedModel::new();
        racing.pinned().reset_page(probe_page(), false);
        let handle = unsafe { (*racing.model()).qt_thread() };
        let queued = handle.queue(|model| {
            model.set_acquisition_state(&QString::from("wallhaven"), &QString::from("late"), 3);
        });
        report.check(queued.is_ok(), || {
            "живая модель: постановка в очередь до удаления не удалась".to_string()
        });
        racing.destroy();
        // The task stayed in the event queue of an already dead object: cxx-qt
        // prints a qWarning and does nothing. We check that the event loop survives
        // this.
        rexon_process_events(300);

        // ── 4. Shared store: a dead receiver does not hinder a live one ───
        let live = OwnedModel::new();
        live.pinned().reset_page(probe_page(), false);
        let dead = OwnedModel::new();
        dead.pinned().reset_page(probe_page(), false);
        let registered_before = acquisition::registered();
        report.check(registered_before >= 2, || {
            format!("склад: зарегистрировано {registered_before} моделей, ожидалось не меньше двух")
        });
        dead.destroy();
        let delivered = acquisition::set("wallhaven", "late", 2);
        rexon_process_events(300);
        report.check(delivered == 1, || {
            format!("склад: рассылка дошла до {delivered} моделей, ожидалась ровно одна живая")
        });
        report.check(role_text(&live.pinned(), 0, "acquisitionState") == "2", || {
            "склад: живая модель не получила состояние".to_string()
        });
        live.destroy();
        let registered_after = acquisition::registered();
        report.check(registered_after == 0, || {
            format!("склад: после сноса моделей осталось {registered_after} записей")
        });
    }

    /// Entry point of the bench. The exit code is the verdict.
    pub fn run() -> i32 {
        // The application core is enough for the bench: there are no windows and
        // no QML here.
        let mut app = cxx_qt_lib::QCoreApplication::new();
        if let Some(app) = app.as_mut() {
            app.set_application_name(&QString::from("Rexon (стенд Discover)"));
        }

        let golden = golden::load();
        let mut report = Report {
            failures: Vec::new(),
            wide_updates: 0,
            cases: golden.cases.len(),
            events: 0,
        };

        // ⭐ An empty selection = a failure, not a success: a golden file without
        // cases would run the loop NOT ONCE and declare the bench green without
        // checking anything. The same class the QML bench fell for: the Workshop
        // scenario ran on an empty model and compared zero with zero.
        report.check(!golden.cases.is_empty(), || {
            "в эталоне нет ни одного случая — стенду нечего проверять".to_string()
        });
        for case in &golden.cases {
            run_case(case, &mut report);
        }
        run_late_callbacks(&mut report);

        println!(
            "REXON-REMOTE-SELFTEST cases={} events={} wideUpdates={} failures={}",
            report.cases,
            report.events,
            report.wide_updates,
            report.failures.len()
        );
        for failure in &report.failures {
            println!("REXON-REMOTE-SELFTEST-FAIL {failure}");
        }
        // There must not be a single "wide" dataChanged: a targeted role update has
        // no right to degenerate into "everything changed".
        let ok = report.failures.is_empty() && report.wide_updates == 0;
        println!(
            "REXON-REMOTE-SELFTEST-VERDICT {} failures={}",
            if ok { "OK" } else { "FAIL" },
            report.failures.len()
        );
        i32::from(!ok)
    }
}

#[cfg(test)]
mod tests {
    use super::golden;
    use super::*;

    /// The golden file was captured from the LIVE Python model
    /// (tests/golden/remote_list_cases.py). Here it is run through the port's
    /// PURE layer: rows, role values, has_more/canFetchMore and — above all — a
    /// single sequence of signals (order, rows, roles). The Qt layer of the same
    /// model is checked by the `selftest::run()` bench.
    #[test]
    fn golden_parity_with_python_model() {
        let golden = golden::load();
        assert!(!golden.cases.is_empty(), "голден пуст");

        for case in &golden.cases {
            let name = &case.name;
            let (models, journal) = golden::replay_pure(case);
            let expected: Vec<(usize, ModelEvent)> =
                case.events.iter().map(golden::expected_event).collect();
            assert_eq!(journal, expected, "{name}: последовательность сигналов");

            assert_eq!(models.len(), case.final_.len(), "{name}: число моделей");
            for (index, want) in case.final_.iter().enumerate() {
                let model = &models[index];
                let at = format!("{name} · модель {index}");
                assert_eq!(model.len(), want.count, "{at}: число строк");
                assert_eq!(model.has_more(), want.has_more, "{at}: hasMore");
                assert_eq!(
                    model.can_fetch_more(true),
                    want.can_fetch_more,
                    "{at}: canFetchMore"
                );
                assert!(
                    !model.can_fetch_more(false),
                    "{at}: canFetchMore по валидному родителю обязан быть ложью"
                );
                assert_eq!(model.rows(), want.rows.as_slice(), "{at}: строки");

                let first = model
                    .row(0)
                    .map(RemoteRow::to_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                assert_eq!(first, want.get_first, "{at}: get(0)");
                assert_eq!(first, want.item_first, "{at}: item(0)");
                assert!(
                    model.row(model.len() as i32).is_none(),
                    "{at}: строка за границей"
                );
                assert!(model.row(-1).is_none(), "{at}: отрицательная строка");
            }
        }
    }

    #[test]
    fn role_numbers_match_the_original() {
        // Role numbers are part of the contract, not an implementation detail:
        // bridge.py:577-586 (and remote_model.cpp behind it) numbers them
        // Qt::UserRole + 1..9 in EXACTLY this order. The vendored QML goes by names
        // and would not notice a permutation, but anyone who compares a protocol
        // frame or writes a proxy model over ours would.
        // The last two are ours (the avatar and the author's works), appended at
        // the end.
        for (number, name) in [
            (257, "itemId"),
            (258, "sourceId"),
            (259, "title"),
            (260, "previewUrl"),
            (261, "author"),
            (262, "wpType"),
            (263, "acquisitionState"),
            (264, "authorAvatarUrl"),
            (265, "authorUrl"),
        ] {
            let slot = ROLE_NAMES
                .iter()
                .position(|n| *n == name)
                .unwrap_or_else(|| panic!("роли {name} нет"));
            assert_eq!(ROLE_BASE + slot as i32, number, "номер роли {name}");
        }
    }

    /// waywallen 0.6.0: resource size and download progress — targeted, one
    /// role per event; the row key is "source␀item".
    #[test]
    fn transfer_progress_updates_role_by_role() {
        let mut rows = RemoteRows::default();
        rows.reset_rows(
            vec![RemoteRow {
                item_id: "1".into(),
                source_id: "we".into(),
                width: 1920,
                ..RemoteRow::default()
            }],
            false,
        );
        let t = acquisition::Transfer { active: true, received: 10, total: 0 };
        let events = rows.set_acquisition("we", "1", 2, t);
        let roles: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                ModelEvent::DataChanged { role, .. } => Some(*role),
                _ => None,
            })
            .collect();
        assert_eq!(roles, ["acquisitionState", "downloadTransferActive", "downloadReceivedBytes"]);
        assert!(rows.set_acquisition("we", "1", 2, t).is_empty(), "то же — без событий");
        let row = rows.row(0).expect("строка");
        assert_eq!((row.width, row.download_received_bytes), (1920, 10));
        assert_eq!(row.key(), "we\u{0}1");
        assert_eq!(row.to_json().as_object().map(|m| m.len()), Some(GOLDEN_ROLES));
    }

    #[test]
    fn roles_are_unique_and_indexed() {
        let mut seen = std::collections::HashSet::new();
        for name in ROLE_NAMES {
            assert!(seen.insert(name), "роль {name} объявлена дважды");
        }
        assert_eq!(ROLE_NAMES[ROLE_AUTHOR], "author");
        assert_eq!(ROLE_NAMES[ROLE_ACQUISITION_STATE], "acquisitionState");
        assert_eq!(ROLE_NAMES[ROLE_AUTHOR_AVATAR], "authorAvatarUrl");
    }

    #[test]
    fn every_data_changed_carries_exactly_one_role() {
        // A separate check of the same thing the bench counts with the "wide"
        // counter: a targeted update has no right to degenerate into "all roles
        // changed".
        let golden = golden::load();
        for case in &golden.cases {
            let (_, journal) = golden::replay_pure(case);
            for (_, event) in &journal {
                if let ModelEvent::DataChanged { role, .. } = event {
                    assert!(
                        ROLE_NAMES.contains(role),
                        "{}: dataChanged с ролью «{role}» вне набора",
                        case.name
                    );
                }
            }
        }
    }

    #[test]
    fn broken_page_json_is_silent() {
        // A broken page JSON must not bring the tab down: empty results and no
        // panics (the same principle as for the overrides in batch 1).
        assert!(parse_rows("{это не json").is_empty());
        assert!(parse_rows("[]").is_empty());
        assert_eq!(
            parse_rows(r#"[{"itemId":"a","acquisitionState":2}]"#),
            vec![RemoteRow {
                item_id: "a".to_string(),
                acquisition_state: 2,
                ..RemoteRow::default()
            }]
        );
    }

    #[test]
    fn insert_range_is_empty_for_empty_page() {
        let mut rows = RemoteRows::default();
        assert_eq!(rows.insert_range(0), None);
        rows.append_rows(parse_rows(r#"[{"itemId":"a"},{"itemId":"b"}]"#), true);
        assert_eq!(rows.insert_range(2), Some((2, 3)));
    }
}
