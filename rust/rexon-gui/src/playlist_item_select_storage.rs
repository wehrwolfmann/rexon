// PlaylistItemSelectStorage — choosing the contents of a playlist on the
// "Wallpapers" page.
//
// It appeared at the vendor in v0.3.7
// (`ui/src/model/wallpaper_select_storage.cppm`) and is declared DIRECTLY
// in `WallpaperPage.qml:24`: without this type the "Wallpapers" page is
// not created at all, rather than "working with caveats".
//
// How it differs from `WallpaperSelectStorage`, on top of which it stands:
//
//   * it remembers WHICH playlist's contents are being edited
//     (`playlistId`) and at which revision (`revision`) they were read —
//     this number goes to the daemon as `expected_revision`, and by it the
//     daemon catches a race of two edits
//     (`ERROR_CODE_PLAYLIST_REVISION_CONFLICT` of the 0.3.7 protocol);
//   * it keeps the ORIGINAL order of the contents (`initialEntryIds`) and
//     the order of what was added (the original's `m_new_entry_ids`), so
//     `orderedWallpaperIds()` returns not "what is selected" but "what is
//     selected, in the order the playlist came together": first the
//     surviving old ones, then the added ones in the order of adding;
//   * while a playlist is set, selection mode does not go off on an empty
//     selection — the playlist can be emptied entirely and "Apply"
//     pressed.
//
// cxx-qt has no inheritance, so both bases come from the shared engine
// `impl_select_storage_base!` (src/select_storage_base.rs) — the same
// technique as the `Query` base of the queries.

/// cxx-qt bridge: the playlist contents selection storage.
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

    // Warning: `type QObject` is NOT declared here — cxx-qt emits the
    // declaration by itself (see the explanation in
    // src/wallpaper_select_storage.rs).
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Checked cast to the wallpaper library model.
        ///
        /// Its own name, not shared with the neighbouring storage's bridge: the same
        /// `extern "C++"` declaration in two bridges of one crate gives a
        /// `duplicate symbol` at link time.
        ///
        /// # Safety
        /// `object` is either nullptr or a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_as_playlist_wallpaper_list_model(object: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── SelectStorage base + the wallpaper layer ──────────────────────
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
        // ── our own level: editing the playlist contents ──────────────────
        /// Warning: both numbers are exposed as `f64`, not `i64`: we do not expose a
        /// 64-bit integer to QML — a literal binding to such a property fails
        /// together with the whole document. The analysis and precision limits are
        /// in `src/qvalue.rs`, the live check in `src/qml_int64_probe.rs`.
        /// Inside, the fields stay exact `i64`.
        #[qproperty(f64, playlist_id, cxx_name = "playlistId", READ = playlist_id,
                    NOTIFY = playlist_changed)]
        #[qproperty(f64, revision, READ = revision, NOTIFY = playlist_changed)]
        #[qproperty(QStringList, initial_entry_ids, cxx_name = "initialEntryIds",
                    READ = initial_entry_ids, NOTIFY = playlist_changed)]
        type PlaylistItemSelectStorage = super::PlaylistItemSelectStorageRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "modelChanged"]
        fn model_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectionModeChanged"]
        fn selection_mode_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "anchorIndexChanged"]
        fn anchor_index_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectedCountChanged"]
        fn selected_count_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "activeChanged"]
        fn active_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        /// A separate signal of the original: the number of items to delete may
        /// decrease without the selection size changing (the list changed).
        #[qsignal]
        #[cxx_name = "selectedRemovableCountChanged"]
        fn selected_removable_count_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        /// waywallen 0.4.4: the number of hidden items in the selection changed —
        /// the selection, the model or a row's "hidden" flag changed.
        #[qsignal]
        #[cxx_name = "selectedHiddenCountChanged"]
        fn selected_hidden_count_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        /// The edited playlist changed: number, revision and original contents.
        #[qsignal]
        #[cxx_name = "playlistChanged"]
        fn playlist_changed(self: Pin<&mut PlaylistItemSelectStorage>);
    }

    // unsafe block: the accessors of the `model` property take a raw pointer.
    // Warning: `set_model` is moreover an `unsafe fn` by itself: it PASSES the
    // pointer to the `qobject_cast` shim, which dereferences it. The pointer
    // comes from QML; the caller is responsible for it being alive.
    unsafe extern "RustQt" {
        fn model(self: &PlaylistItemSelectStorage) -> *mut QObject;
        #[cxx_name = "setModel"]
        unsafe fn set_model(self: Pin<&mut PlaylistItemSelectStorage>, model: *mut QObject);
        #[cxx_name = "selectionMode"]
        fn selection_mode(self: &PlaylistItemSelectStorage) -> bool;
        #[cxx_name = "setSelectionMode"]
        fn set_selection_mode(self: Pin<&mut PlaylistItemSelectStorage>, value: bool);
        #[cxx_name = "anchorIndex"]
        fn anchor_index(self: &PlaylistItemSelectStorage) -> i32;
        #[cxx_name = "setAnchorIndex"]
        fn set_anchor_index(self: Pin<&mut PlaylistItemSelectStorage>, value: i32);
        #[cxx_name = "selectedCount"]
        fn selected_count(self: &PlaylistItemSelectStorage) -> i32;
        fn active(self: &PlaylistItemSelectStorage) -> bool;
        #[cxx_name = "removableSelectedCount"]
        fn removable_selected_count(self: &PlaylistItemSelectStorage) -> i32;
        #[cxx_name = "hiddenSelectedCount"]
        fn hidden_selected_count(self: &PlaylistItemSelectStorage) -> i32;
        #[cxx_name = "playlistId"]
        fn playlist_id(self: &PlaylistItemSelectStorage) -> f64;
        fn revision(self: &PlaylistItemSelectStorage) -> f64;
        #[cxx_name = "initialEntryIds"]
        fn initial_entry_ids(self: &PlaylistItemSelectStorage) -> QStringList;
    }

    // ── Invokables (names from the original) ─────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn begin(self: Pin<&mut PlaylistItemSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "setSelected"]
        fn set_selected(
            self: Pin<&mut PlaylistItemSelectStorage>,
            row: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        #[cxx_name = "toggleSelected"]
        fn toggle_selected(self: Pin<&mut PlaylistItemSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(
            self: Pin<&mut PlaylistItemSelectStorage>,
            from: i32,
            to: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        fn clear(self: Pin<&mut PlaylistItemSelectStorage>);

        #[qinvokable]
        #[cxx_name = "setSelectedKeys"]
        fn set_selected_keys(self: Pin<&mut PlaylistItemSelectStorage>, keys: &QStringList);

        #[qinvokable]
        #[cxx_name = "selectedKeys"]
        fn selected_keys(self: &PlaylistItemSelectStorage) -> QStringList;

        #[qinvokable]
        #[cxx_name = "selectedItems"]
        fn selected_items(self: &PlaylistItemSelectStorage) -> QList_QVariant;

        #[qinvokable]
        #[cxx_name = "selectedWallpaperIds"]
        fn selected_wallpaper_ids(self: &PlaylistItemSelectStorage) -> QList_QVariant;

        #[qinvokable]
        #[cxx_name = "removableSelectedWallpaperIds"]
        fn removable_selected_wallpaper_ids(self: &PlaylistItemSelectStorage) -> QStringList;

        /// Enter editing of a playlist's contents: remember the number and the
        /// revision, fill the selection with its contents, turn on selection mode.
        #[qinvokable]
        #[cxx_name = "beginPlaylistItems"]
        fn begin_playlist_items(
            self: Pin<&mut PlaylistItemSelectStorage>,
            playlist_id: f64,
            revision: f64,
            entry_ids: &QStringList,
        );

        /// The selection IN playlist ORDER — this list goes into
        /// `playlist_set_items`.
        #[qinvokable]
        #[cxx_name = "orderedWallpaperIds"]
        fn ordered_wallpaper_ids(self: &PlaylistItemSelectStorage) -> QList_QVariant;

        /// Recompute everything that depends on the selection in the model.
        #[qinvokable]
        #[cxx_name = "onModelSelectionChanged"]
        fn on_model_selection_changed(self: Pin<&mut PlaylistItemSelectStorage>);
    }

    impl cxx_qt::Constructor<()> for PlaylistItemSelectStorage {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList, QVariant};

use crate::impl_select_storage_base;
use crate::wallpaper_select_storage::SelectState;

/// Rust state of the storage.
pub struct PlaylistItemSelectStorageRust {
    state: SelectState,
    /// The library model. A raw pointer, like the original's QPointer.
    model: *mut qobject::QObject,
    /// Model subscriptions (waywallen 0.4.4, see `watch_model`): the guards
    /// break the connection when the model changes and when the storage is
    /// destroyed.
    model_watch: Vec<cxx_qt::QMetaObjectConnectionGuard>,
    /// The edited playlist and its revision at the time of reading.
    playlist_id: i64,
    revision: i64,
    /// The original playlist contents in order, without duplicates or empties.
    initial_entry_ids: Vec<String>,
    /// Items added beyond the original contents — in the order of adding.
    new_entry_ids: Vec<String>,
}

impl Default for PlaylistItemSelectStorageRust {
    fn default() -> Self {
        Self {
            state: SelectState::new(),
            model: core::ptr::null_mut(),
            model_watch: Vec::new(),
            playlist_id: 0,
            revision: 0,
            initial_entry_ids: Vec::new(),
            new_entry_ids: Vec::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::PlaylistItemSelectStorage {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

// Both bases of the original — through the shared engine
// (src/select_storage_base.rs).
impl_select_storage_base!(PlaylistItemSelectStorage, rexon_as_playlist_wallpaper_list_model);

impl qobject::PlaylistItemSelectStorage {
    // ── hooks of the shared engine ────────────────────────────────────────

    /// `keepActiveWithoutSelection` of this level: while a playlist is set,
    /// selection mode holds even with an empty selection — otherwise the
    /// playlist could not be emptied entirely.
    fn keep_active(&self) -> bool {
        self.playlist_id > 0
    }

    /// The `selectedCountChanged` handler that the original attaches in the
    /// subclass constructor: recompute the order of the added items.
    fn after_selected_count_changed(self: Pin<&mut Self>) {
        self.sync_new_entry_order();
    }

    // ── level properties ──────────────────────────────────────────────────

    fn playlist_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.playlist_id)
    }

    fn revision(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.revision)
    }

    fn initial_entry_ids(&self) -> QStringList {
        QStringList::from_iter(self.initial_entry_ids.iter().map(QString::from))
    }

    // ── level invokables ──────────────────────────────────────────────────

    fn begin_playlist_items(
        mut self: Pin<&mut Self>,
        playlist_id: f64,
        revision: f64,
        entry_ids: &QStringList,
    ) {
        // Order as in the original: first RESET the base (the old selection must
        // not leak into the new playlist), then the new state.
        self.as_mut().clear_base();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.playlist_id = crate::qvalue::qml_to_i64(playlist_id);
            rust.revision = crate::qvalue::qml_to_i64(revision);
            rust.initial_entry_ids.clear();
            rust.new_entry_ids.clear();
            for entry_id in entry_ids.iter() {
                let entry_id = entry_id.to_string();
                if entry_id.is_empty() || rust.initial_entry_ids.contains(&entry_id) {
                    continue;
                }
                rust.initial_entry_ids.push(entry_id);
            }
        }
        self.as_mut().playlist_changed();
        let keys = self.initial_entry_ids();
        self.as_mut().set_selected_keys(&keys);
        self.as_mut().set_selection_mode(true);
        self.as_mut().set_anchor_index(-1);
        self.notify_active_changed();
    }

    fn ordered_wallpaper_ids(&self) -> qobject::QList_QVariant {
        let selected: Vec<String> = self
            .selected_keys()
            .iter()
            .map(cxx_qt_lib::QString::to_string)
            .collect();
        let mut emitted: Vec<String> = Vec::new();
        // First the survivors of the original contents — in playlist order.
        for entry_id in &self.initial_entry_ids {
            if !selected.contains(entry_id) || emitted.contains(entry_id) {
                continue;
            }
            emitted.push(entry_id.clone());
        }
        // Then the added ones — in the order of adding.
        for entry_id in &self.new_entry_ids {
            if entry_id.is_empty() || emitted.contains(entry_id) {
                continue;
            }
            if !selected.contains(entry_id) {
                continue;
            }
            emitted.push(entry_id.clone());
        }
        // Tail: selected items we do not know about yet (the model changed between
        // signals) — the original's branch, not a safety net.
        for entry_id in &selected {
            if entry_id.is_empty() || emitted.contains(entry_id) {
                continue;
            }
            emitted.push(entry_id.clone());
        }
        let mut out = qobject::QList_QVariant::default();
        for entry_id in &emitted {
            out.append(QVariant::from(&QString::from(entry_id)));
        }
        out
    }

    /// Order as in the original: first the playlist is cleared (otherwise
    /// `keepActiveWithoutSelection` would not let us leave the mode), then the
    /// selection.
    fn clear(mut self: Pin<&mut Self>) {
        let had_playlist = self.playlist_id > 0
            || self.revision > 0
            || !self.initial_entry_ids.is_empty();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.playlist_id = 0;
            rust.revision = 0;
            rust.initial_entry_ids.clear();
            rust.new_entry_ids.clear();
        }
        if had_playlist {
            self.as_mut().playlist_changed();
        }
        self.clear_base();
    }

    // ── internals ─────────────────────────────────────────────────────────

    /// The original's `syncNewEntryOrder`: what was deselected is forgotten,
    /// and unknown selected items are appended at the end — that way the added
    /// items keep the order of adding rather than the order in the library
    /// list.
    fn sync_new_entry_order(mut self: Pin<&mut Self>) {
        let selected: Vec<String> = self
            .selected_keys()
            .iter()
            .map(cxx_qt_lib::QString::to_string)
            .collect();
        let mut rust = self.as_mut().rust_mut();
        rust.new_entry_ids.retain(|entry_id| selected.contains(entry_id));
        for entry_id in &selected {
            if entry_id.is_empty()
                || rust.initial_entry_ids.contains(entry_id)
                || rust.new_entry_ids.contains(entry_id)
            {
                continue;
            }
            rust.new_entry_ids.push(entry_id.clone());
        }
    }
}
