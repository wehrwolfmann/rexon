// Wallpaper playlists: the list, details of one, and mutations (create,
// delete, rename, set the contents/mode/interval, update as a whole,
// enable, disable, skip).
//
// Reference: waywallen `ui/src/query/playlist_query.cpp` @e5626c1 (vendor
// commit v0.2.6+9).
//
// What changed compared with our Python bridge:
//
//   * the `data` and `count` properties were removed. `data` returned an
//     empty stub model (`_EmptyListModel`), `count` the list length; the
//     original has neither, and NOT A SINGLE QML line reads them (checked
//     with grep over the vendored tree and our pages). There is no reason to
//     keep a dead model for the sake of a property name.
//   * `sourceKind` in the playlist map is KEPT, although the original does
//     not have it: our pages use it to tell a "smart" playlist from an
//     ordinary one, and an extra key does not bother the vendored QML.
//
// The `done()` signal ALWAYS fires — on a daemon refusal too: QML reads
// `.status` after it (the same in the original,
// `PlaylistMutationQuery::send`).

/// cxx-qt bridge: the playlist list and mutations.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Create the playlist contents model as a child of the query.
        ///
        /// Our own names, not shared with the "Wallpapers" page bridge: the same
        /// `extern "C++"` declaration in two bridges of one crate gives a
        /// `duplicate symbol` at link time.
        ///
        /// # Safety
        /// `parent` is a live QObject (ownership goes to Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_wallpaper_list_model(parent: *mut QObject) -> *mut QObject;

        /// Single-shot deferred reload timer (`delayReload` of the `Query` base,
        /// 100 ms).
        ///
        /// # Safety
        /// `owner` is a live QObject with a `reload()` invokable.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;

        /// # Safety
        /// `timer` is a timer created by `rexon_new_playlist_debounce`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_restart_playlist_debounce(timer: *mut QObject);

        // ── Test bench factories (src/wallpaper_page_selftest.rs) ────────
        //
        // In the live Rexon these objects are created by QML; the bench needs the
        // same classes without a window and without QML — as for the "Wallpapers"
        // page queries.
        //
        // # Safety
        // `parent` is either nullptr or a live QObject (ownership goes to Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_list_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_detail_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_mutation_query(parent: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, playlists, READ = playlists, NOTIFY = playlists_changed)]
        type PlaylistListQuery = super::PlaylistListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// Warning: `f64`, not `i64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`, the live check in
        /// `src/qml_int64_probe.rs`).
        #[qproperty(f64, created_id, cxx_name = "createdId", READ = created_id,
                    NOTIFY = created_id_changed)]
        type PlaylistMutationQuery = super::PlaylistMutationQueryRust;

        /// Details of one playlist — the vendor's 0.3.7 `PlaylistEditPage` edit page
        /// (it replaced the removed `PlaylistDetailPanel`).
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// Warning: `double`, not `qint64`, although the original has `qint64`
        /// here. The reason is technical and verified live: cxx-qt emits
        /// `Q_PROPERTY(::std::int64_t …)`, and on 64-bit Linux that is `long` — a
        /// separate Qt metatype for which the QML engine CANNOT do a declarative
        /// binding. `playlistId: root.playlistId` in `PlaylistEditPage.qml` would
        /// fail when loading the page ("Cannot assign value … to property
        /// playlistId"), while an imperative assignment works — which is why the
        /// defect is visible neither to the build nor to the type gate. The vendor
        /// itself declares this value on the QML side as `required property double
        /// playlistId`, so there is no divergence in observable behaviour: playlist
        /// numbers are far below 2^53.
        #[qproperty(f64, playlist_id, cxx_name = "playlistId", READ = playlist_id,
                    WRITE = set_playlist_id, NOTIFY = playlist_id_changed)]
        #[qproperty(QVariant, playlist, READ = playlist, NOTIFY = playlist_changed)]
        #[qproperty(f64, revision, READ = revision, NOTIFY = playlist_changed)]
        #[qproperty(QStringList, entry_ids, cxx_name = "entryIds", READ = entry_ids,
                    NOTIFY = playlist_changed)]
        /// Contents model. The property name is `data`, as in the original: the
        /// vendored QML writes `model: detailQuery.data`. CONSTANT — the model is
        /// created in post-construction and lives as long as the query does.
        #[qproperty(*mut QObject, model, cxx_name = "data", READ = data, CONSTANT)]
        type PlaylistDetailQuery = super::PlaylistDetailQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PlaylistListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PlaylistListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PlaylistListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PlaylistListQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PlaylistMutationQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PlaylistMutationQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PlaylistMutationQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PlaylistMutationQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PlaylistDetailQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PlaylistDetailQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PlaylistDetailQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PlaylistDetailQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "playlistIdChanged"]
        fn playlist_id_changed(self: Pin<&mut PlaylistDetailQuery>);
        /// One signal for the map, the revision number and the contents — as in the
        /// original.
        #[qsignal]
        #[cxx_name = "playlistChanged"]
        fn playlist_changed(self: Pin<&mut PlaylistDetailQuery>);

        #[qsignal]
        #[cxx_name = "playlistsChanged"]
        fn playlists_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "createdIdChanged"]
        fn created_id_changed(self: Pin<&mut PlaylistMutationQuery>);
        /// "The mutation has finished" — whether it is a success or a refusal is
        /// read from `status`.
        #[qsignal]
        fn done(self: Pin<&mut PlaylistMutationQuery>);
    }

    extern "RustQt" {
        fn status(self: &PlaylistListQuery) -> i32;
        fn error(self: &PlaylistListQuery) -> QString;
        fn querying(self: &PlaylistListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PlaylistListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PlaylistListQuery>, value: bool);

        fn status(self: &PlaylistMutationQuery) -> i32;
        fn error(self: &PlaylistMutationQuery) -> QString;
        fn querying(self: &PlaylistMutationQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PlaylistMutationQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PlaylistMutationQuery>, value: bool);

        fn playlists(self: &PlaylistListQuery) -> QVariant;
        #[cxx_name = "createdId"]
        fn created_id(self: &PlaylistMutationQuery) -> f64;

        fn status(self: &PlaylistDetailQuery) -> i32;
        fn error(self: &PlaylistDetailQuery) -> QString;
        fn querying(self: &PlaylistDetailQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PlaylistDetailQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PlaylistDetailQuery>, value: bool);
        #[cxx_name = "playlistId"]
        fn playlist_id(self: &PlaylistDetailQuery) -> f64;
        #[cxx_name = "setPlaylistId"]
        fn set_playlist_id(self: Pin<&mut PlaylistDetailQuery>, value: f64);
        fn playlist(self: &PlaylistDetailQuery) -> QVariant;
        fn revision(self: &PlaylistDetailQuery) -> f64;
        #[cxx_name = "entryIds"]
        fn entry_ids(self: &PlaylistDetailQuery) -> QStringList;
    }

    // unsafe block: the getter of the `data` property returns a raw pointer.
    unsafe extern "RustQt" {
        fn data(self: &PlaylistDetailQuery) -> *mut QObject;
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut PlaylistListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PlaylistListQuery>, token: i64, frame: &QByteArray);

        /// In the original a mutation's `reload()` is empty: it is started only by
        /// an explicit call of one of the actions below.
        #[qinvokable]
        fn reload(self: Pin<&mut PlaylistMutationQuery>);
        /// Warning: the 0.3.7 signature: `synchronizedSelection` became FOURTH,
        /// before the contents (the original's `PlaylistMutationQuery::create`).
        #[qinvokable]
        fn create(self: Pin<&mut PlaylistMutationQuery>, name: QString, mode: i32,
                  interval_secs: i32, synchronized_selection: bool, item_ids: QVariant);

        /// Update the playlist as a whole — the 0.3.7 edit form.
        #[qinvokable]
        fn update(self: Pin<&mut PlaylistMutationQuery>, id: f64, name: QString, mode: i32,
                  interval_secs: i32, synchronized_selection: bool, item_ids: QVariant,
                  expected_revision: f64);
        /// Save a smart playlist (waywallen 0.5.0): `id > 0` — update, otherwise
        /// create. `selection` is the opaque selection of
        /// `WallpaperListQuery.selection`.
        #[qinvokable]
        #[cxx_name = "saveSmart"]
        fn save_smart(self: Pin<&mut PlaylistMutationQuery>, id: f64, name: QString, mode: i32,
                      interval_secs: i32, synchronized_selection: bool, selection: &QVariant,
                      expected_revision: f64);
        #[qinvokable]
        fn remove(self: Pin<&mut PlaylistMutationQuery>, id: f64);
        #[qinvokable]
        fn rename(self: Pin<&mut PlaylistMutationQuery>, id: f64, name: QString);
        /// Warning: the 0.3.7 signature: the third parameter is the revision number
        /// the contents were read at. The daemon compares it with its own and
        /// answers `ERROR_CODE_PLAYLIST_REVISION_CONFLICT` if the playlist changed
        /// between reading and writing.
        #[qinvokable]
        #[cxx_name = "setItems"]
        fn set_items(self: Pin<&mut PlaylistMutationQuery>, id: f64, item_ids: QVariant,
                     expected_revision: f64);
        #[qinvokable]
        #[cxx_name = "setMode"]
        fn set_mode(self: Pin<&mut PlaylistMutationQuery>, id: f64, mode: i32);
        #[qinvokable]
        #[cxx_name = "setInterval"]
        fn set_interval(self: Pin<&mut PlaylistMutationQuery>, id: f64, interval_secs: i32);
        #[qinvokable]
        fn activate(self: Pin<&mut PlaylistMutationQuery>, id: f64, targets: QVariant,
                    auto_attach: bool);
        #[qinvokable]
        fn deactivate(self: Pin<&mut PlaylistMutationQuery>, targets: QVariant,
                      clear_auto_attach: f64);
        #[qinvokable]
        #[cxx_name = "jumpTo"]
        fn jump_to(self: Pin<&mut PlaylistMutationQuery>, id: f64, entry_id: QString);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PlaylistMutationQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut PlaylistDetailQuery>);
        /// `delayReload` of the `Query` base (qextra): a reload after 100 ms,
        /// consecutive triggers are glued into ONE request. In the vendored QML it
        /// is called only by the playlist edit page — it also re-reads the contents
        /// after every mutation and after a daemon event.
        #[qinvokable]
        #[cxx_name = "delayReload"]
        fn delay_reload(self: Pin<&mut PlaylistDetailQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PlaylistDetailQuery>, token: i64,
                            frame: &QByteArray);
    }

    // Post-construction: the contents model and the deferred reload timer.
    impl cxx_qt::Constructor<()> for PlaylistDetailQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use prost::Message;
use serde_json::Value;

use rexon_core::proto::control as pb;
use rexon_core::we_manage;
use rexon_core::we_query::{QuerySignal, QueryState};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::{json_to_variant, variant_to_json};

/// A core playlist → a QML map. The smart playlist's selection (waywallen
/// 0.5.0) goes under `smartSelection` as bytes — the same opaque value as
/// `WallpaperListQuery.selection`: `PlaylistEditPage` passes it straight to
/// `restoreSelection`/`matchesSelection`. There is no place for bytes in the
/// JSON of the core map, so the key is added here.
fn playlist_variant(view: &we_manage::PlaylistView) -> QVariant {
    let (Value::Object(map), Some(bytes)) = (&view.map, &view.smart_selection) else {
        return json_to_variant(&view.map);
    };
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, item) in map {
        out.insert(QString::from(key), json_to_variant(item));
    }
    out.insert(
        QString::from(we_manage::SMART_SELECTION_KEY),
        QVariant::from(&QByteArray::from(&bytes[..])),
    );
    QVariant::from(&out)
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistListQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PlaylistListQueryRust {
    base: QueryState,
    data: we_manage::PlaylistListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PlaylistListQuery);

impl qobject::PlaylistListQuery {
    pub fn playlists(&self) -> QVariant {
        let mut list = QList::<QVariant>::default();
        for view in &self.data.playlists {
            list.append(playlist_variant(view));
        }
        QVariant::from(&list)
    }

    /// A new generation (the original's `++m_generation`): an answer to the
    /// previous request, if still in flight, is no longer ours.
    pub fn reload(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().pending.clear();
        let payload = pb::PlaylistListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("playlist_list", payload) {
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
            we_manage::apply_playlist_list(&mut rust.base, &mut rust.data, frame.as_slice())
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
            "playlists" => self.as_mut().playlists_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistMutationQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PlaylistMutationQueryRust {
    base: QueryState,
    created_id: i64,
    /// token → whether to take the number of the created playlist from the
    /// answer.
    pending: std::collections::BTreeMap<i64, bool>,
}

impl_query_base!(PlaylistMutationQuery);

impl qobject::PlaylistMutationQuery {
    pub fn created_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.created_id)
    }

    /// Send a mutation. `capture_create` — whether to expect the new playlist's
    /// number in the answer.
    fn send(
        mut self: Pin<&mut Self>,
        field: &'static str,
        payload: Vec<u8>,
        capture_create: bool,
    ) {
        if let Some(token) = self.as_mut().send_frame(field, payload) {
            self.as_mut().rust_mut().pending.insert(token, capture_create);
            return;
        }
        // No connection — the frame did not go out, but `done()` must arrive: the
        // page waits for it to clear "saving…".
        self.done();
    }

    pub fn reload(self: Pin<&mut Self>) {
        let _ = self;
    }

    pub fn create(
        self: Pin<&mut Self>,
        name: QString,
        mode: i32,
        interval_secs: i32,
        synchronized_selection: bool,
        item_ids: QVariant,
    ) {
        let payload = pb::PlaylistCreateRequest {
            name: name.to_string(),
            mode,
            interval_secs: interval_secs.max(0) as u32,
            entry_ids: we_manage::list_to_strings(&variant_to_json(&item_ids)),
            synchronized_selection,
            // waywallen 0.5.0: without `source` the daemon creates a curated playlist
            // from `entry_ids` — which is exactly what this form does.
            source: None,
        }
        .encode_to_vec();
        self.send("playlist_create", payload, true);
    }

    /// `PlaylistUpdate` (field 87 of the 0.3.7 protocol): name, mode, interval,
    /// synchronized selection and contents in one frame — that is how the
    /// `PlaylistEditPage` form edits them. `expected_revision` is the edit race
    /// guard.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        self: Pin<&mut Self>,
        id: f64,
        name: QString,
        mode: i32,
        interval_secs: i32,
        synchronized_selection: bool,
        item_ids: QVariant,
        expected_revision: f64,
    ) {
        // For a smart playlist (waywallen 0.5.0) the core substitutes its
        // selection for the contents — see `we_manage::playlist_update_request`.
        let payload = we_manage::playlist_update_request(
            crate::qvalue::qml_to_i64(id),
            &name.to_string(),
            mode,
            interval_secs.max(0) as u32,
            synchronized_selection,
            we_manage::list_to_strings(&variant_to_json(&item_ids)),
            crate::qvalue::qml_to_i64(expected_revision),
        )
        .encode_to_vec();
        self.send("playlist_update", payload, false);
    }

    /// The original's `saveSmart` (waywallen 0.5.0). The frame is built by the
    /// core (`we_manage::playlist_save_smart_request`): the selection is
    /// cleaned, the contents empty. Not a selection — no frame, an error and
    /// `done()`, as in the original.
    #[allow(clippy::too_many_arguments)]
    pub fn save_smart(
        mut self: Pin<&mut Self>,
        id: f64,
        name: QString,
        mode: i32,
        interval_secs: i32,
        synchronized_selection: bool,
        selection: &QVariant,
        expected_revision: f64,
    ) {
        let save = crate::wallpaper_queries::selection_bytes(selection).and_then(|bytes| {
            we_manage::playlist_save_smart_request(
                crate::qvalue::qml_to_i64(id),
                &name.to_string(),
                mode,
                interval_secs.max(0) as u32,
                synchronized_selection,
                &bytes,
                crate::qvalue::qml_to_i64(expected_revision),
            )
        });
        let Some(save) = save else {
            let signals = we_manage::playlist_smart_invalid(&mut self.as_mut().rust_mut().base);
            self.dispatch(signals);
            return;
        };
        self.send(save.field(), save.encode_to_vec(), save.captures_create());
    }

    pub fn remove(self: Pin<&mut Self>, id: f64) {
        let id = crate::qvalue::qml_to_i64(id);
        let payload = pb::PlaylistDeleteRequest { id }.encode_to_vec();
        self.send("playlist_delete", payload, false);
    }

    pub fn rename(self: Pin<&mut Self>, id: f64, name: QString) {
        let payload = pb::PlaylistRenameRequest {
            id: crate::qvalue::qml_to_i64(id),
            name: name.to_string(),
        }
        .encode_to_vec();
        self.send("playlist_rename", payload, false);
    }

    /// `expected_revision` is the revision number the contents were read at.
    /// The daemon treats zero as "do not compare".
    pub fn set_items(
        self: Pin<&mut Self>,
        id: f64,
        item_ids: QVariant,
        expected_revision: f64,
    ) {
        let payload = pb::PlaylistSetItemsRequest {
            id: crate::qvalue::qml_to_i64(id),
            entry_ids: we_manage::list_to_strings(&variant_to_json(&item_ids)),
            expected_revision: crate::qvalue::qml_to_i64(expected_revision),
        }
        .encode_to_vec();
        self.send("playlist_set_items", payload, false);
    }

    pub fn set_mode(self: Pin<&mut Self>, id: f64, mode: i32) {
        let id = crate::qvalue::qml_to_i64(id);
        let payload = pb::PlaylistSetModeRequest { id, mode }.encode_to_vec();
        self.send("playlist_set_mode", payload, false);
    }

    pub fn set_interval(self: Pin<&mut Self>, id: f64, interval_secs: i32) {
        let payload = pb::PlaylistSetIntervalRequest {
            id: crate::qvalue::qml_to_i64(id),
            interval_secs: interval_secs.max(0) as u32,
        }
        .encode_to_vec();
        self.send("playlist_set_interval", payload, false);
    }

    /// `targets` are the v0.3.5 targets: a list of maps with `displayId` or
    /// `canvasId` (`WallpaperPage.qml:854`). The daemon treats an empty list as
    /// "every canvas once plus every standalone display".
    pub fn activate(self: Pin<&mut Self>, id: f64, targets: QVariant, auto_attach: bool) {
        let payload = pb::PlaylistActivateRequest {
            id: crate::qvalue::qml_to_i64(id),
            targets: we_manage::presentation_targets(&variant_to_json(&targets)),
            auto_attach,
        }
        .encode_to_vec();
        self.send("playlist_activate", payload, false);
    }

    pub fn deactivate(self: Pin<&mut Self>, targets: QVariant, clear_auto_attach: f64) {
        let payload = pb::PlaylistDeactivateRequest {
            targets: we_manage::presentation_targets(&variant_to_json(&targets)),
            clear_auto_attach: crate::qvalue::qml_to_i64(clear_auto_attach),
        }
        .encode_to_vec();
        self.send("playlist_deactivate", payload, false);
    }

    /// Warning: daemon v0.2.4 performs the skip but does NOT broadcast
    /// `playlist_changed` — the displays' push statuses stay old. Verified live
    /// on 2026-07-12, a candidate for a daemon patch; there is nothing to fix
    /// on the UI side.
    pub fn jump_to(self: Pin<&mut Self>, id: f64, entry_id: QString) {
        let payload = pb::PlaylistJumpToRequest {
            id: crate::qvalue::qml_to_i64(id),
            entry_id: entry_id.to_string(),
        }
        .encode_to_vec();
        self.send("playlist_jump_to", payload, false);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(capture_create) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_playlist_mutation(
                &mut rust.base,
                &mut rust.created_id,
                capture_create,
                frame.as_slice(),
            )
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let mut signals = decode_failed(&mut self.as_mut().rust_mut().base);
                signals.push(QuerySignal::Property("done"));
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "createdId" => self.as_mut().created_id_changed(),
            "done" => self.as_mut().done(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistDetailQuery
// ════════════════════════════════════════════════════════════════════════

/// Reload debounce — 100 ms, as in the `Query` base (qextra query.cpp:16).
const DETAIL_DEBOUNCE_MS: i32 = 100;

pub struct PlaylistDetailQueryRust {
    base: QueryState,
    data: we_manage::PlaylistDetailData,
    playlist_id: i64,
    /// Tokens of sent frames: an answer to a stale request must not override a
    /// fresh one.
    pending: std::collections::BTreeSet<i64>,
    /// Contents model — the `data` property, set in post-construction.
    model: *mut qobject::QObject,
    debounce: *mut qobject::QObject,
}

impl Default for PlaylistDetailQueryRust {
    fn default() -> Self {
        Self {
            base: QueryState::default(),
            data: we_manage::PlaylistDetailData::default(),
            playlist_id: 0,
            pending: std::collections::BTreeSet::new(),
            model: core::ptr::null_mut(),
            debounce: core::ptr::null_mut(),
        }
    }
}

impl cxx_qt::Initialize for qobject::PlaylistDetailQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner: *mut qobject::PlaylistDetailQuery =
            unsafe { self.as_mut().get_unchecked_mut() };
        let owner = owner.cast::<qobject::QObject>();
        // The model is created in post-construction, not lazily in the getter: QML
        // reads `data` on the very first binding.
        let model = unsafe { qobject::rexon_new_playlist_wallpaper_list_model(owner) };
        let timer = unsafe { qobject::rexon_new_playlist_debounce(owner, DETAIL_DEBOUNCE_MS) };
        let mut rust = self.as_mut().rust_mut();
        rust.model = model;
        rust.debounce = timer;
    }
}

impl_query_base!(PlaylistDetailQuery);

impl qobject::PlaylistDetailQuery {
    pub fn data(&self) -> *mut qobject::QObject {
        self.model
    }

    pub fn playlist_id(&self) -> f64 {
        self.playlist_id as f64
    }

    /// In the original `delayReload` hangs on `playlistIdChanged`
    /// (`connect_requet_reload` in `componentComplete`). We do not need a
    /// separate "finish on creation": QML assigns the property with a binding,
    /// and the very first write starts the same timer.
    pub fn set_playlist_id(mut self: Pin<&mut Self>, value: f64) {
        // The fractional part is dropped: a playlist number is an integer, while
        // QML assigns its `double`.
        let value = value as i64;
        if self.playlist_id == value {
            return;
        }
        // A new generation and cancellation (the original's `setPlaylistId`): an
        // answer about the previous playlist, still in flight, will not override
        // the new one.
        let mut rust = self.as_mut().rust_mut();
        rust.playlist_id = value;
        rust.pending.clear();
        self.as_mut().playlist_id_changed();
        self.delay_reload();
    }

    pub fn playlist(&self) -> QVariant {
        playlist_variant(&self.data.playlist)
    }

    pub fn revision(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.data.revision)
    }

    pub fn entry_ids(&self) -> QStringList {
        QStringList::from_iter(self.data.entry_ids.iter().map(QString::from))
    }

    pub fn delay_reload(self: Pin<&mut Self>) {
        let timer = self.debounce;
        if timer.is_null() {
            // There is no timer only in a bare Rust bench without Qt — there the
            // reload happens at once, as in the original with `delay: false`.
            self.reload();
            return;
        }
        unsafe { qobject::rexon_restart_playlist_debounce(timer) };
    }

    /// A new generation on every call, as in the original. Without a number —
    /// SILENTLY: no frame, no error, no status change (waywallen 0.5.0,
    /// `playlist_query.cpp`). The smart playlist creation page lives with
    /// `playlistId: 0`, and an error here would show "Playlist unavailable".
    pub fn reload(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().pending.clear();
        let id = self.playlist_id;
        if id <= 0 {
            return;
        }
        // waywallen 0.5.0: contents pages. Zero in `page_size` means the whole
        // answer, as before these fields appeared.
        let payload = pb::PlaylistGetRequest {
            id,
            page: 0,
            page_size: 0,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("playlist_get", payload) {
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
            we_manage::apply_playlist_get(&mut rust.base, &mut rust.data, frame.as_slice())
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
            "items" => {
                // The model is rebuilt FIRST: the page rebuilds its draft on
                // `playlistChanged` and must already see the new contents.
                let rows = self.data.items.clone();
                if !self.model.is_null() {
                    let model: Pin<&mut crate::wallpaper_list_model::qobject::WallpaperListModel> =
                        unsafe {
                            Pin::new_unchecked(
                                &mut *self.model.cast::<
                                    crate::wallpaper_list_model::qobject::WallpaperListModel,
                                >(),
                            )
                        };
                    model.reset_rows(rows);
                }
            }
            "playlist" => self.as_mut().playlist_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
