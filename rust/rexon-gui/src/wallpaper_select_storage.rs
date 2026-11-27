// WallpaperSelectStorage — multiple selection of wallpapers on the
// "Wallpapers" page.
//
// Warning: the reference is the ORIGINAL, not our Python. The Python class
// (bridge.py:4459) calls itself "simplified, stage 4a" and is simplified so
// much that porting it would mean preserving four divergences (rule #16
// and the rule "in a third-party program's tab the yardstick is parity
// with the original"):
//
//   1. the selection was kept by row INDICES, while in the original it is
//      by keys and lives in the model (`qextra/src/select_storage.cpp` +
//      `kstore` QMetaListModel);
//   2. `removableSelectedCount`/`removableSelectedWallpaperIds` returned the
//      WHOLE selection without looking at `supportsItemRemove`
//      (`ui/src/model/wallpaper_select_storage.cpp:65`) — that is, the
//      "Delete" button promised to delete what cannot be deleted;
//   3. `isEditingPlaylist` always returned false, so playlist editing mode
//      was not indicated in the interface at all;
//   4. `editPlaylistSelection` only remembered the playlist, while it must
//      also fill the selection with its contents, turn on selection mode
//      and reset the anchor.
//
// We have no inheritance (cxx-qt cannot make a Rust QObject inherit from a
// Rust QObject), so the `SelectStorage` base is carried over by
// COMPOSITION: its fields and rules lie in the plain `SelectState` struct,
// and the wallpaper specifics next to it. Exactly the same technique as
// with the `Query` base in batch 3.
//
// The selection itself is stored by the MODEL (crate::wallpaper_list_model),
// as in the original. Here there is only the selection mode, the anchor,
// the active flag and the playlist being edited.

/// cxx-qt bridge: the selection storage of the "Wallpapers" page.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    // Warning: `type QObject` is NOT declared here. This bridge has a QObject
    // without a base, and cxx-qt emits the declaration by itself (cxx-qt-gen:
    // add_qobject_import); a second one gives "the type QObject is defined
    // multiple times". In the model bridges it is the other way round — the
    // base is declared by hand there, and the QObject declaration has to be
    // written ourselves.
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Checked cast to the wallpaper library model.
        ///
        /// Exactly `qobject_cast`, not a bare pointer cast: the `model` property is
        /// written by QML, and ANY QObject can be slipped in there. The original
        /// also uses `qobject_cast` here — a foreign model is simply not accepted
        /// (`select_storage.cpp:16`).
        ///
        /// # Safety
        /// `object` is either nullptr or a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_as_wallpaper_list_model(object: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(*mut QObject, model, READ = model, WRITE = set_model,
                    NOTIFY = model_changed)]
        #[qproperty(bool, selection_mode, cxx_name = "selectionMode",
                    READ = selection_mode, WRITE = set_selection_mode,
                    NOTIFY = selection_mode_changed)]
        #[qproperty(i32, anchor_index, cxx_name = "anchorIndex",
                    READ = anchor_index, WRITE = set_anchor_index,
                    NOTIFY = anchor_index_changed)]
        #[qproperty(i32, selected_count, cxx_name = "selectedCount",
                    READ = selected_count, NOTIFY = selected_count_changed)]
        #[qproperty(bool, active, READ = active, NOTIFY = active_changed)]
        #[qproperty(i32, removable_selected_count, cxx_name = "removableSelectedCount",
                    READ = removable_selected_count,
                    NOTIFY = selected_removable_count_changed)]
        /// waywallen 0.4.4: how many of the selected wallpapers are hidden — by it
        /// the page decides whether to "Hide" or "Restore" the selection.
        #[qproperty(i32, hidden_selected_count, cxx_name = "hiddenSelectedCount",
                    READ = hidden_selected_count,
                    NOTIFY = selected_hidden_count_changed)]
        #[qproperty(QVariant, playlist_edit_target, cxx_name = "playlistEditTarget",
                    READ = playlist_edit_target, WRITE = set_playlist_edit_target,
                    NOTIFY = playlist_edit_target_changed)]
        /// Warning: `f64`, not `i64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`). The field inside stays an exact `i64`.
        #[qproperty(f64, playlist_edit_target_id, cxx_name = "playlistEditTargetId",
                    READ = playlist_edit_target_id,
                    NOTIFY = playlist_edit_target_changed)]
        type WallpaperSelectStorage = super::WallpaperSelectStorageRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "modelChanged"]
        fn model_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectionModeChanged"]
        fn selection_mode_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "anchorIndexChanged"]
        fn anchor_index_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectedCountChanged"]
        fn selected_count_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "activeChanged"]
        fn active_changed(self: Pin<&mut WallpaperSelectStorage>);
        /// A separate signal of the original: the number of items to delete may
        /// decrease without the selection size changing (the list changed).
        #[qsignal]
        #[cxx_name = "selectedRemovableCountChanged"]
        fn selected_removable_count_changed(self: Pin<&mut WallpaperSelectStorage>);
        /// waywallen 0.4.4: the number of hidden items in the selection changed —
        /// the selection, the model or a row's "hidden" flag changed.
        #[qsignal]
        #[cxx_name = "selectedHiddenCountChanged"]
        fn selected_hidden_count_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "playlistEditTargetChanged"]
        fn playlist_edit_target_changed(self: Pin<&mut WallpaperSelectStorage>);
    }

    // unsafe block: the accessors of the `model` property take a raw pointer,
    // and cxx requires such functions to be marked unsafe (this is about
    // matching the C++ signature, not about the call being dangerous).
    // Warning: `set_model` is moreover an `unsafe fn` by itself: it PASSES the
    // pointer to the `qobject_cast` shim, which dereferences it. The pointer
    // comes from QML; the caller is responsible for it being alive.
    unsafe extern "RustQt" {
        fn model(self: &WallpaperSelectStorage) -> *mut QObject;
        #[cxx_name = "setModel"]
        unsafe fn set_model(self: Pin<&mut WallpaperSelectStorage>, model: *mut QObject);
        #[cxx_name = "selectionMode"]
        fn selection_mode(self: &WallpaperSelectStorage) -> bool;
        #[cxx_name = "setSelectionMode"]
        fn set_selection_mode(self: Pin<&mut WallpaperSelectStorage>, value: bool);
        #[cxx_name = "anchorIndex"]
        fn anchor_index(self: &WallpaperSelectStorage) -> i32;
        #[cxx_name = "setAnchorIndex"]
        fn set_anchor_index(self: Pin<&mut WallpaperSelectStorage>, value: i32);
        #[cxx_name = "selectedCount"]
        fn selected_count(self: &WallpaperSelectStorage) -> i32;
        fn active(self: &WallpaperSelectStorage) -> bool;
        #[cxx_name = "removableSelectedCount"]
        fn removable_selected_count(self: &WallpaperSelectStorage) -> i32;
        #[cxx_name = "hiddenSelectedCount"]
        fn hidden_selected_count(self: &WallpaperSelectStorage) -> i32;
        #[cxx_name = "playlistEditTarget"]
        fn playlist_edit_target(self: &WallpaperSelectStorage) -> QVariant;
        #[cxx_name = "setPlaylistEditTarget"]
        fn set_playlist_edit_target(self: Pin<&mut WallpaperSelectStorage>, value: QVariant);
        #[cxx_name = "playlistEditTargetId"]
        fn playlist_edit_target_id(self: &WallpaperSelectStorage) -> f64;
    }

    // ── Invokables (names from the original) ─────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn begin(self: Pin<&mut WallpaperSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "setSelected"]
        fn set_selected(self: Pin<&mut WallpaperSelectStorage>, row: i32, selected: bool) -> bool;

        #[qinvokable]
        #[cxx_name = "toggleSelected"]
        fn toggle_selected(self: Pin<&mut WallpaperSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(
            self: Pin<&mut WallpaperSelectStorage>,
            from: i32,
            to: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        fn clear(self: Pin<&mut WallpaperSelectStorage>);

        #[qinvokable]
        #[cxx_name = "setSelectedKeys"]
        fn set_selected_keys(self: Pin<&mut WallpaperSelectStorage>, keys: &QStringList);

        #[qinvokable]
        #[cxx_name = "selectedKeys"]
        fn selected_keys(self: &WallpaperSelectStorage) -> QStringList;

        #[qinvokable]
        #[cxx_name = "selectedItems"]
        fn selected_items(self: &WallpaperSelectStorage) -> QList_QVariant;

        /// The selected identifiers as a list of values — that is how the playlist
        /// action of the vendored page accepts them.
        #[qinvokable]
        #[cxx_name = "selectedWallpaperIds"]
        fn selected_wallpaper_ids(self: &WallpaperSelectStorage) -> QList_QVariant;

        #[qinvokable]
        #[cxx_name = "removableSelectedWallpaperIds"]
        fn removable_selected_wallpaper_ids(self: &WallpaperSelectStorage) -> QStringList;

        #[qinvokable]
        #[cxx_name = "hasPlaylistEditTarget"]
        fn has_playlist_edit_target(self: &WallpaperSelectStorage) -> bool;

        #[qinvokable]
        #[cxx_name = "isEditingPlaylist"]
        fn is_editing_playlist(self: &WallpaperSelectStorage, playlist: &QVariant) -> bool;

        #[qinvokable]
        #[cxx_name = "editPlaylistSelection"]
        fn edit_playlist_selection(self: Pin<&mut WallpaperSelectStorage>, playlist: &QVariant);

        /// Recompute everything that depends on the selection in the model. Called
        /// when the model reported a selection change (in the original this is a
        /// lambda in `SelectStorage::connectModel`).
        #[qinvokable]
        #[cxx_name = "onModelSelectionChanged"]
        fn on_model_selection_changed(self: Pin<&mut WallpaperSelectStorage>);
    }

    impl cxx_qt::Constructor<()> for WallpaperSelectStorage {}
}
use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList, QVariant};
use serde_json::Value;

use crate::impl_select_storage_base;
use crate::qvalue::variant_to_json;

// ============================================================================
// Plain layer: the rules of the SelectStorage base without Qt
// ============================================================================

/// State of the `SelectStorage` base (qextra) plus playlist editing of
/// wallpapers.
#[derive(Debug, Default)]
pub struct SelectState {
    pub selection_mode: bool,
    pub anchor_index: i32,
    /// Cache of `active`: the signal is sent only on a REAL flip, as in the
    /// original (`notifyActiveChanged`).
    pub active: bool,
    pub playlist_target: Value,
    pub playlist_target_id: i64,
}

impl SelectState {
    pub fn new() -> Self {
        Self {
            selection_mode: false,
            anchor_index: -1,
            active: false,
            playlist_target: Value::Null,
            playlist_target_id: 0,
        }
    }

    /// The original's `keepActiveWithoutSelection`: while a playlist is being
    /// edited, selection mode stays active even with an empty selection.
    pub fn keep_active_without_selection(&self) -> bool {
        self.playlist_target_id > 0
    }
}

/// Playlist identifier from the map that came from QML.
pub fn playlist_id(value: &Value) -> i64 {
    match value.get("id") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

/// Playlist contents — the selection is filled with them on entering
/// editing.
pub fn playlist_entry_ids(value: &Value) -> Vec<String> {
    match value.get("entryIds") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

// ============================================================================
// Qt layer
// ============================================================================

/// Rust state of the storage.
pub struct WallpaperSelectStorageRust {
    state: SelectState,
    /// The library model. A raw pointer, like the original's QPointer: the model
    /// is owned by the list query, the storage only reads it.
    model: *mut qobject::QObject,
    /// Model subscriptions (waywallen 0.4.4, see `watch_model`): the guards
    /// break the connection when the model changes and when the storage is
    /// destroyed.
    model_watch: Vec<cxx_qt::QMetaObjectConnectionGuard>,
}

impl Default for WallpaperSelectStorageRust {
    fn default() -> Self {
        Self {
            state: SelectState::new(),
            model: core::ptr::null_mut(),
            model_watch: Vec::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::WallpaperSelectStorage {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

// The `SelectStorage` base + the wallpaper layer — through the shared
// engine (src/select_storage_base.rs), as two base classes in the original.
impl_select_storage_base!(WallpaperSelectStorage, rexon_as_wallpaper_list_model);

impl qobject::WallpaperSelectStorage {
    // ── hooks of the shared engine ────────────────────────────────────────

    /// `keepActiveWithoutSelection` of this level: playlist editing.
    fn keep_active(&self) -> bool {
        self.state.keep_active_without_selection()
    }

    /// This level has no `selectedCountChanged` handler of its own: both
    /// counters are emitted by the shared engine.
    fn after_selected_count_changed(self: Pin<&mut Self>) {
        let _ = self;
    }

    // ── properties of the "playlist editing" level ────────────────────────

    fn playlist_edit_target(&self) -> QVariant {
        crate::qvalue::json_to_variant(&self.state.playlist_target)
    }

    fn set_playlist_edit_target(mut self: Pin<&mut Self>, value: QVariant) {
        let target = variant_to_json(&value);
        let next_id = playlist_id(&target);
        if self.state.playlist_target == target && self.state.playlist_target_id == next_id {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.state.playlist_target = target;
            rust.state.playlist_target_id = next_id;
        }
        self.as_mut().playlist_edit_target_changed();
        self.notify_active_changed();
    }

    fn playlist_edit_target_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.state.playlist_target_id)
    }

    // ── level invokables ──────────────────────────────────────────────────

    /// Order as in the original: first playlist editing is cleared (otherwise
    /// `keepActiveWithoutSelection` would not let us leave the mode), then the
    /// selection.
    fn clear(mut self: Pin<&mut Self>) {
        if self.state.keep_active_without_selection() {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.state.playlist_target = Value::Null;
                rust.state.playlist_target_id = 0;
            }
            self.as_mut().playlist_edit_target_changed();
        }
        self.clear_base();
    }

    fn has_playlist_edit_target(&self) -> bool {
        self.state.playlist_target_id > 0
    }

    fn is_editing_playlist(&self, playlist: &QVariant) -> bool {
        let id = playlist_id(&variant_to_json(playlist));
        id > 0 && id == self.state.playlist_target_id
    }

    fn edit_playlist_selection(mut self: Pin<&mut Self>, playlist: &QVariant) {
        let target = variant_to_json(playlist);
        let entries = playlist_entry_ids(&target);
        self.as_mut()
            .set_playlist_edit_target(crate::qvalue::json_to_variant(&target));
        let keys = QStringList::from_iter(entries.iter().map(QString::from));
        self.as_mut().set_selected_keys(&keys);
        self.as_mut().set_selection_mode(true);
        self.set_anchor_index(-1);
    }
}
