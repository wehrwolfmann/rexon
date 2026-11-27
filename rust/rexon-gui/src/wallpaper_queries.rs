// WallpaperListQuery — the wallpaper library query of the "Wallpapers"
// page.
//
// The heaviest part of porting the "Wallpapers" page: it is here that Qt
// Protobuf GADGETS (`sorts`/`filters`/`filterLogics`) arrive in the query
// properties, which neither Python (PySide6 segfaults on marshalling) nor
// cxx-qt (no `Q_GADGET` in 0.9) can handle.
//
// How it is solved. The values are parsed by the generic C++
// meta-reflection shim `rexon-qt-filter/cpp/rexon_gadget_shim.cpp` — the
// very one the filter editor model stands on; it is one for the project
// (rule #12) and is simply hooked up here too now. The shim takes the
// metatype AT RUN TIME and hands out JSON, while the decisions about the
// values are made by the core (`rexon_core::we_wallpaper`, `we_settings`).
// Not a single protobuf type is linked by name here.
//
// What this file does and does NOT do. It computes not a single rule:
// comparing properties, the order of signals, building the
// `wallpaper_list` frame and parsing the answer are all in the core,
// exactly as with the Python shim. Here there is only the Qt wrapper:
// properties, signals, owning the model and the debounce.
//
// The transport is the shared seam of batches 2-3 (`we_transport`): the
// frame goes out as a signal, the answer comes in through the
// `deliverResponse` invokable.

/// cxx-qt bridge: queries of the "Wallpapers" page.
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

    // The gadget meta-reflection shim (`rexon-qt-filter/cpp`). The live Rexon
    // does not need these entries from here — the filter editor model calls
    // them from its own build. They are here for the bench
    // (src/wallpaper_page_selftest.rs): it builds rules on real gadgets with
    // THE SAME calls as the model and checks that they reach the core intact.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("rexon_gadget_shim.h");
        fn rexon_default_gadget(type_name: &str) -> QVariant;
        fn rexon_coerce_gadget(type_name: &str, value: &QVariant) -> QVariant;
        fn rexon_gadget_set_by_role(gadget: &QVariant, role_index: i32, value: &QVariant)
            -> QVariant;
        fn rexon_gadget_set_int(gadget: &QVariant, prop: &str, value: i32) -> QVariant;
        fn rexon_role_names(type_name: &str) -> QHash_i32_QByteArray;
    }

    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Create the library model as a child of the query.
        ///
        /// # Safety
        /// `parent` is a live QObject (ownership goes to Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_list_model(parent: *mut QObject) -> *mut QObject;

        /// Single-shot deferred reload timer (the original's 100 ms).
        ///
        /// # Safety
        /// `owner` is a live QObject with a `reload()` invokable.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;

        /// # Safety
        /// `timer` is a timer created by `rexon_new_reload_debounce`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_restart_wallpaper_debounce(timer: *mut QObject);

        // ── Golden bench factories (src/wallpaper_page_selftest.rs) ──────
        //
        // In the live Rexon all eight objects of the page are created by QML, and
        // it does not need the factory. The bench needs the same classes without a
        // window and without QML — exactly as done for the Discover queries.
        //
        // # Safety
        // `parent` is either nullptr or a live QObject (ownership goes to Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_list_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_get_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_apply_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_remove_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_scan_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_property_set_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_layout_set_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_apply_via_portal_query(parent: *mut QObject)
            -> *mut QObject;
        /// waywallen 0.4.4: "hide wallpaper" (the v0.4.4 check of the same bench).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_hide_query(parent: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Query base ────────────────────────────────────────────────────
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        // ── parameter properties ──────────────────────────────────────────
        #[qproperty(QString, wp_type, cxx_name = "wpType", READ = wp_type,
                    WRITE = set_wp_type, NOTIFY = wp_type_changed)]
        #[qproperty(QString, search_text, cxx_name = "searchText",
                    READ = search_text, WRITE = set_search_text,
                    NOTIFY = search_text_changed)]
        #[qproperty(QVariant, sorts, READ = sorts, WRITE = set_sorts,
                    NOTIFY = sorts_changed)]
        #[qproperty(QVariant, filters, READ = filters, WRITE = set_filters,
                    NOTIFY = filters_changed)]
        #[qproperty(QVariant, filter_logics, cxx_name = "filterLogics",
                    READ = filter_logics, WRITE = set_filter_logics,
                    NOTIFY = filter_logics_changed)]
        #[qproperty(QStringList, skip_types, cxx_name = "skipTypes",
                    READ = skip_types, WRITE = set_skip_types,
                    NOTIFY = skip_types_changed)]
        #[qproperty(QStringList, filter_tags, cxx_name = "filterTags",
                    READ = filter_tags, WRITE = set_filter_tags,
                    NOTIFY = filter_tags_changed)]
        #[qproperty(QStringList, skip_content_ratings, cxx_name = "skipContentRatings",
                    READ = skip_content_ratings, WRITE = set_skip_content_ratings,
                    NOTIFY = skip_content_ratings_changed)]
        /// waywallen 0.4.4: which wallpapers to show by the "hidden" flag
        /// (`WallpaperHiddenFilter`: 0 hide, 1 only hidden, 2 all).
        #[qproperty(i32, hidden_filter, cxx_name = "hiddenFilter",
                    READ = hidden_filter, WRITE = set_hidden_filter,
                    NOTIFY = hidden_filter_changed)]
        /// `limit` of the `QueryList` base — the page size; 0 (the default) means
        /// no pages, the daemon returns the whole list. Written by "Hidden
        /// wallpapers" (`limit: 100`) and the smart playlist preview (`limit: 50`,
        /// waywallen 0.5.0). When a change reloads the list — see
        /// `rexon_core::we_wallpaper::set_limit`.
        #[qproperty(i32, limit, READ = limit, WRITE = set_limit, NOTIFY = limit_changed)]
        /// `offset` of the base: the number of the last loaded page.
        #[qproperty(i32, offset, READ = offset, WRITE = set_offset, NOTIFY = offset_changed)]
        /// `noMore` of the base: there are no further pages.
        #[qproperty(bool, no_more, cxx_name = "noMore", READ = no_more,
                    WRITE = set_no_more, NOTIFY = no_more_changed)]
        /// waywallen 0.5.0: the selection is an opaque value (an `ArrayBuffer` in
        /// JS) that QML only stores and hands back to
        /// `restoreSelection`/`matchesSelection` (and `saveSmart`). Its form — see
        /// `rexon_core::we_wallpaper::encode_selection`.
        #[qproperty(QVariant, selection, READ = selection, NOTIFY = selection_changed)]
        /// waywallen 0.5.0: numbers of hidden libraries (the original's
        /// `QList<qlonglong>`). Exposed as a list of double numbers: a 64-bit
        /// integer is not exposed to QML (see `qvalue.rs`). The setter normalizes
        /// the list — ascending, without duplicates.
        #[qproperty(QVariant, hidden_library_ids, cxx_name = "hiddenLibraryIds",
                    READ = hidden_library_ids, WRITE = set_hidden_library_ids,
                    NOTIFY = hidden_library_ids_changed)]
        // ── result properties ─────────────────────────────────────────────
        #[qproperty(bool, has_active_filters, cxx_name = "hasActiveFilters",
                    READ = has_active_filters, NOTIFY = has_active_filters_changed)]
        /// waywallen 0.5.0: filters OTHER THAN hidden libraries (highlighting the
        /// "Filters" button on the "Wallpapers" page).
        #[qproperty(bool, has_other_filters, cxx_name = "hasOtherFilters",
                    READ = has_other_filters, NOTIFY = has_other_filters_changed)]
        #[qproperty(i32, total, READ = total, NOTIFY = total_changed)]
        /// The library model. The property name is `data`, as in the original: the
        /// vendored QML writes `model: wallpaperQuery.data`. CONSTANT — the model is
        /// created in post-construction and lives as long as the query does.
        #[qproperty(*mut QObject, model, cxx_name = "data", READ = data, CONSTANT)]
        type WallpaperListQuery = super::WallpaperListQueryRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperListQuery>, error: QString);

        #[qsignal]
        #[cxx_name = "wpTypeChanged"]
        fn wp_type_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "searchTextChanged"]
        fn search_text_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "sortsChanged"]
        fn sorts_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "filtersChanged"]
        fn filters_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "filterLogicsChanged"]
        fn filter_logics_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "skipTypesChanged"]
        fn skip_types_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "filterTagsChanged"]
        fn filter_tags_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "skipContentRatingsChanged"]
        fn skip_content_ratings_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "hiddenFilterChanged"]
        fn hidden_filter_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "limitChanged"]
        fn limit_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "offsetChanged"]
        fn offset_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "noMoreChanged"]
        fn no_more_changed(self: Pin<&mut WallpaperListQuery>);
        /// waywallen 0.5.0: the selection changed — the original's `invalidateList`
        /// sends it on every change of a selection property and of `limit`.
        #[qsignal]
        #[cxx_name = "selectionChanged"]
        fn selection_changed(self: Pin<&mut WallpaperListQuery>);
        /// waywallen 0.5.0.
        #[qsignal]
        #[cxx_name = "hiddenLibraryIdsChanged"]
        fn hidden_library_ids_changed(self: Pin<&mut WallpaperListQuery>);
        /// waywallen 0.5.0.
        #[qsignal]
        #[cxx_name = "hasOtherFiltersChanged"]
        fn has_other_filters_changed(self: Pin<&mut WallpaperListQuery>);
        /// One signal for both filter states — it also triggers reloading the list
        /// (the original's `connect_requet_reload`).
        #[qsignal]
        #[cxx_name = "filterStateChanged"]
        fn filter_state_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "hasActiveFiltersChanged"]
        fn has_active_filters_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "totalChanged"]
        fn total_changed(self: Pin<&mut WallpaperListQuery>);
        /// An internal signal of the shim (the original has no `items` property):
        /// the set of wallpapers changed, the model is rebuilt.
        ///
        /// Warning: order: the model is rebuilt BEFORE this signal — in the Python
        /// bridge the rebuild was attached by the query constructor, that is, it was
        /// first in the chain of subscribers. QML on `onItemsChanged` must already
        /// read the NEW rows.
        #[qsignal]
        #[cxx_name = "itemsChanged"]
        fn items_changed(self: Pin<&mut WallpaperListQuery>);

        /// Frame out — the shared seam of batches 2-3.
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(
            self: Pin<&mut WallpaperListQuery>,
            token: i64,
            field: QString,
            request: QByteArray,
        );
    }

    extern "RustQt" {
        fn status(self: &WallpaperListQuery) -> i32;
        fn error(self: &WallpaperListQuery) -> QString;
        fn querying(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperListQuery>, value: bool);

        #[cxx_name = "wpType"]
        fn wp_type(self: &WallpaperListQuery) -> QString;
        #[cxx_name = "setWpType"]
        fn set_wp_type(self: Pin<&mut WallpaperListQuery>, value: QString);
        #[cxx_name = "searchText"]
        fn search_text(self: &WallpaperListQuery) -> QString;
        #[cxx_name = "setSearchText"]
        fn set_search_text(self: Pin<&mut WallpaperListQuery>, value: QString);

        fn sorts(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setSorts"]
        fn set_sorts(self: Pin<&mut WallpaperListQuery>, value: QVariant);
        fn filters(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setFilters"]
        fn set_filters(self: Pin<&mut WallpaperListQuery>, value: QVariant);
        #[cxx_name = "filterLogics"]
        fn filter_logics(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setFilterLogics"]
        fn set_filter_logics(self: Pin<&mut WallpaperListQuery>, value: QVariant);

        #[cxx_name = "skipTypes"]
        fn skip_types(self: &WallpaperListQuery) -> QStringList;
        #[cxx_name = "setSkipTypes"]
        fn set_skip_types(self: Pin<&mut WallpaperListQuery>, value: QStringList);
        #[cxx_name = "filterTags"]
        fn filter_tags(self: &WallpaperListQuery) -> QStringList;
        #[cxx_name = "setFilterTags"]
        fn set_filter_tags(self: Pin<&mut WallpaperListQuery>, value: QStringList);
        #[cxx_name = "skipContentRatings"]
        fn skip_content_ratings(self: &WallpaperListQuery) -> QStringList;
        #[cxx_name = "setSkipContentRatings"]
        fn set_skip_content_ratings(self: Pin<&mut WallpaperListQuery>, value: QStringList);
        #[cxx_name = "hiddenFilter"]
        fn hidden_filter(self: &WallpaperListQuery) -> i32;
        #[cxx_name = "setHiddenFilter"]
        fn set_hidden_filter(self: Pin<&mut WallpaperListQuery>, value: i32);
        fn limit(self: &WallpaperListQuery) -> i32;
        #[cxx_name = "setLimit"]
        fn set_limit(self: Pin<&mut WallpaperListQuery>, value: i32);
        fn offset(self: &WallpaperListQuery) -> i32;
        #[cxx_name = "setOffset"]
        fn set_offset(self: Pin<&mut WallpaperListQuery>, value: i32);
        #[cxx_name = "noMore"]
        fn no_more(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "setNoMore"]
        fn set_no_more(self: Pin<&mut WallpaperListQuery>, value: bool);
        fn selection(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "hiddenLibraryIds"]
        fn hidden_library_ids(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setHiddenLibraryIds"]
        fn set_hidden_library_ids(self: Pin<&mut WallpaperListQuery>, value: QVariant);

        #[cxx_name = "hasActiveFilters"]
        fn has_active_filters(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "hasOtherFilters"]
        fn has_other_filters(self: &WallpaperListQuery) -> bool;
        fn total(self: &WallpaperListQuery) -> i32;
    }

    // ── Invokables ────────────────────────────────────────────────────────
    // unsafe block: the getter of the `data` property returns a raw pointer.
    unsafe extern "RustQt" {
        fn data(self: &WallpaperListQuery) -> *mut QObject;
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperListQuery>);

        /// waywallen 0.6.0: "Retry" after an error — the failed step again (loading
        /// more or a reload).
        #[qinvokable]
        fn retry(self: Pin<&mut WallpaperListQuery>);

        /// `delayReload` of the `Query` base (qextra): a reload through the same
        /// 100 ms debounce — consecutive requests are glued into one request.
        /// Called by the "Hidden wallpapers" page (waywallen 0.4.4).
        #[qinvokable]
        #[cxx_name = "delayReload"]
        fn delay_reload(self: Pin<&mut WallpaperListQuery>);

        /// The original's `replaceFilterState`: the rules and the group logic
        /// change at once — one `filterStateChanged` and one reload instead of two.
        /// Returns whether anything changed.
        #[qinvokable]
        #[cxx_name = "replaceFilterState"]
        fn replace_filter_state(
            self: Pin<&mut WallpaperListQuery>,
            filters: QVariant,
            logics: QVariant,
        ) -> bool;

        /// waywallen 0.5.0: the library eye icon — hide/show its wallpapers. The
        /// number is a double (a 64-bit integer is not exposed to QML).
        #[qinvokable]
        #[cxx_name = "toggleLibraryHidden"]
        fn toggle_library_hidden(self: Pin<&mut WallpaperListQuery>, id: f64);

        /// waywallen 0.5.0: spread the selection over the properties. Not a
        /// selection — do nothing.
        #[qinvokable]
        #[cxx_name = "restoreSelection"]
        fn restore_selection(self: Pin<&mut WallpaperListQuery>, value: &QVariant);

        /// waywallen 0.5.0: whether the current selection matches the saved one.
        #[qinvokable]
        #[cxx_name = "matchesSelection"]
        fn matches_selection(self: &WallpaperListQuery, value: &QVariant) -> bool;

        /// `fetchMore(qint32)` of the `QueryList` base: load the next page. The
        /// original does not read the argument (the size is `limit`).
        #[qinvokable]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut WallpaperListQuery>, count: i32);

        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperListQuery>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Constructor<()> for WallpaperListQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QStringList, QVariant};
use serde_json::Value;

use rexon_core::we_query::{self, QuerySignal, QueryState, STATUS_ERROR};
use rexon_core::we_settings::VMap;
use rexon_core::we_wallpaper;

use crate::qvalue::{json_to_variant, variant_to_json};
use crate::we_transport;

/// Reload debounce — 100 ms, as in the original (qextra query.cpp:16).
const DEBOUNCE_MS: i32 = 100;

/// Which properties trigger a deferred reload of the list — the list from
/// the original's constructor (`connect_requet_reload`). It is exposed not
/// for looks: the golden replay (wallpaper_page_golden) uses the same list,
/// and there must be no second copy of it in the project.
pub const LIST_RELOAD_ON: &[&str] = &[
    "wpType",
    "filterState",
    "sorts",
    "searchText",
    "skipTypes",
    "filterTags",
    "skipContentRatings",
    // waywallen 0.4.4: `connect_requet_reload(&hiddenFilterChanged)`.
    "hiddenFilter",
    // waywallen 0.5.0: `hiddenLibraryIdsChanged → invalidateList`.
    "hiddenLibraryIds",
];

/// Whose answer the frame token is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// `reload()` — the whole list (the first page).
    Reload,
    /// `fetchMore()` — the page with this number, to the tail.
    Page(i32),
}

/// The `data` property hands out the model; set in post-construction.
pub struct WallpaperListQueryRust {
    base: QueryState,
    data: we_wallpaper::WallpaperListData,
    /// Frame tokens of the CURRENT generation (the original's `m_generation`):
    /// every `reload` and every selection change wipes them, and an answer to a
    /// stale request is silently dropped — the old selection will not override
    /// the new one, and a page of the old selection will not land in the tail
    /// of the new one.
    pending: std::collections::BTreeMap<i64, Pending>,
    model: *mut qobject::QObject,
    debounce: *mut qobject::QObject,
    /// Subscription to the model's `reqFetchMore` (lives as long as the query).
    fetch_guard: Option<cxx_qt::QMetaObjectConnectionGuard>,
    /// waywallen 0.6.0: the last request was loading more — "Retry" after an
    /// error loads more rather than reloading the list.
    failed_append: bool,
}

impl Default for WallpaperListQueryRust {
    fn default() -> Self {
        Self {
            base: QueryState::default(),
            data: we_wallpaper::WallpaperListData::default(),
            pending: std::collections::BTreeMap::new(),
            failed_append: false,
            model: core::ptr::null_mut(),
            debounce: core::ptr::null_mut(),
            fetch_guard: None,
        }
    }
}

/// Pointer to the query for the subscription closure (see
/// discover_queries.rs).
struct OwnerPtr(*mut qobject::WallpaperListQuery);
// SAFETY: the model's signal arrives in the GUI thread, where the query
// lives too; Send is needed only by the `on_req_fetch_more` signature.
unsafe impl Send for OwnerPtr {}

/// Pointer to the object itself as a QObject.
fn self_ptr<T>(object: Pin<&mut T>) -> *mut qobject::QObject {
    let raw: *mut T = unsafe { object.get_unchecked_mut() };
    raw.cast()
}

/// The model as a mutable reference (see wallpaper_select_storage::model_pin).
///
/// # Safety
/// `ptr` is either nullptr or a live `WallpaperListModel`.
unsafe fn model_pin<'a>(
    ptr: *mut qobject::QObject,
) -> Option<Pin<&'a mut crate::wallpaper_list_model::qobject::WallpaperListModel>> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe {
        Pin::new_unchecked(
            &mut *ptr.cast::<crate::wallpaper_list_model::qobject::WallpaperListModel>(),
        )
    })
}

/// A list of rules from QML → a JSON value for the core.
///
/// The elements arrive as Qt Protobuf gadgets (written by the Rust filter
/// editor model) or as ordinary JS objects (maps from the daemon settings).
/// Both kinds are parsed by the SHARED converter `qvalue::variant_to_json` —
/// this page no longer has a list shortcut of its own (rule #12): every
/// "QML → core" path understands gadgets, not just this one.
///
/// Not a list — an empty array: the core treats it as "no rules", which is
/// exactly what the Python `_rules_plain` did.
fn rules_value(value: &QVariant) -> Value {
    match variant_to_json(value) {
        list @ Value::Array(_) => list,
        _ => Value::Array(Vec::new()),
    }
}

fn strings_value(list: &QStringList) -> Value {
    Value::Array(
        list.iter()
            .map(|s| Value::String(s.to_string()))
            .collect::<Vec<_>>(),
    )
}

/// Selection bytes from a QML value. From JS it comes back as an
/// `ArrayBuffer`, that is, a `QByteArray`; `undefined`/`null` do not convert
/// to `QByteArray`, and that is "not a selection". Whether the bytes are
/// really a selection is decided by the core by a tag
/// (`we_wallpaper::decode_selection`). `PlaylistMutationQuery.saveSmart`
/// accepts a selection the same way.
pub(crate) fn selection_bytes(value: &QVariant) -> Option<Vec<u8>> {
    value.value::<QByteArray>().map(|bytes| bytes.as_slice().to_vec())
}

fn maps_to_variant(maps: &[VMap]) -> QVariant {
    json_to_variant(&Value::Array(
        maps.iter().map(|m| Value::Object(m.clone())).collect(),
    ))
}

impl cxx_qt::Initialize for qobject::WallpaperListQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner = self_ptr(self.as_mut());
        // The model is created in post-construction, not lazily in the getter: QML
        // reads `data` on the very first binding.
        let model = unsafe { qobject::rexon_new_wallpaper_list_model(owner) };
        let timer = unsafe { qobject::rexon_new_wallpaper_debounce(owner, DEBOUNCE_MS) };
        {
            let mut rust = self.as_mut().rust_mut();
            rust.model = model;
            rust.debounce = timer;
        }
        // QExtra's `try_connect_fetch_more`: the model asks for the next page. The
        // pointer to the owner is safe precisely because the model is its child
        // and will not outlive the query.
        let query = OwnerPtr(unsafe { self.as_mut().get_unchecked_mut() });
        if let Some(model) = unsafe { model_pin(model) } {
            let guard = model.on_req_fetch_more(move |_model, rows| {
                // Capture the WHOLE wrapper, not a field (the 2021 edition).
                let owner = &query;
                let query = unsafe { Pin::new_unchecked(&mut *owner.0) };
                query.fetch_more(rows);
            });
            self.as_mut().rust_mut().fetch_guard = Some(guard);
        }
    }
}

impl qobject::WallpaperListQuery {
    // ── Query base ────────────────────────────────────────────────────────

    pub fn status(&self) -> i32 {
        self.base.status
    }

    pub fn error(&self) -> QString {
        QString::from(&self.base.error)
    }

    pub fn querying(&self) -> bool {
        self.base.status == we_query::STATUS_QUERYING
    }

    pub fn forward_error(&self) -> bool {
        self.base.forward_error
    }

    pub fn set_forward_error(mut self: Pin<&mut Self>, value: bool) {
        let signals = we_query::set_forward_error(&mut self.as_mut().rust_mut().base, value);
        self.dispatch(signals);
    }

    // ── parameter properties ──────────────────────────────────────────────

    pub fn wp_type(&self) -> QString {
        QString::from(&self.data.wp_type)
    }

    pub fn set_wp_type(self: Pin<&mut Self>, value: QString) {
        self.set_core_property("wpType", Value::String(value.to_string()));
    }

    pub fn search_text(&self) -> QString {
        QString::from(&self.data.search_text)
    }

    pub fn set_search_text(self: Pin<&mut Self>, value: QString) {
        self.set_core_property("searchText", Value::String(value.to_string()));
    }

    pub fn sorts(&self) -> QVariant {
        maps_to_variant(
            &self
                .data
                .sorts
                .iter()
                .map(rexon_core::we_settings::sort_rule_to_map)
                .collect::<Vec<_>>(),
        )
    }

    pub fn set_sorts(self: Pin<&mut Self>, value: QVariant) {
        let rules = rules_value(&value);
        self.set_core_property("sorts", rules);
    }

    pub fn filters(&self) -> QVariant {
        maps_to_variant(
            &self
                .data
                .filters
                .iter()
                .map(rexon_core::we_settings::filter_rule_to_map)
                .collect::<Vec<_>>(),
        )
    }

    pub fn set_filters(self: Pin<&mut Self>, value: QVariant) {
        let rules = rules_value(&value);
        self.set_core_property("filters", rules);
    }

    pub fn filter_logics(&self) -> QVariant {
        maps_to_variant(
            &self
                .data
                .filter_logics
                .iter()
                .map(rexon_core::we_settings::filter_logic_to_map)
                .collect::<Vec<_>>(),
        )
    }

    pub fn set_filter_logics(self: Pin<&mut Self>, value: QVariant) {
        let rules = rules_value(&value);
        self.set_core_property("filterLogics", rules);
    }

    pub fn skip_types(&self) -> QStringList {
        QStringList::from_iter(self.data.skip_types.iter().map(QString::from))
    }

    pub fn set_skip_types(self: Pin<&mut Self>, value: QStringList) {
        let list = strings_value(&value);
        self.set_core_property("skipTypes", list);
    }

    pub fn filter_tags(&self) -> QStringList {
        QStringList::from_iter(self.data.filter_tags.iter().map(QString::from))
    }

    pub fn set_filter_tags(self: Pin<&mut Self>, value: QStringList) {
        let list = strings_value(&value);
        self.set_core_property("filterTags", list);
    }

    pub fn skip_content_ratings(&self) -> QStringList {
        QStringList::from_iter(self.data.skip_content_ratings.iter().map(QString::from))
    }

    pub fn set_skip_content_ratings(self: Pin<&mut Self>, value: QStringList) {
        let list = strings_value(&value);
        self.set_core_property("skipContentRatings", list);
    }

    pub fn hidden_filter(&self) -> i32 {
        self.data.hidden_filter
    }

    pub fn set_hidden_filter(self: Pin<&mut Self>, value: i32) {
        self.set_core_property("hiddenFilter", Value::from(value));
    }

    pub fn limit(&self) -> i32 {
        self.data.limit
    }

    pub fn set_limit(mut self: Pin<&mut Self>, value: i32) {
        let status = self.base.status;
        let (signals, reload) = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::set_limit(&mut rust.data, value, status)
        };
        if reload {
            self.as_mut().rust_mut().pending.clear();
            self.as_mut().delay_reload();
        }
        self.dispatch(signals);
    }

    pub fn offset(&self) -> i32 {
        self.data.offset
    }

    pub fn set_offset(mut self: Pin<&mut Self>, value: i32) {
        if self.data.offset != value {
            self.as_mut().rust_mut().data.offset = value;
            self.offset_changed();
        }
    }

    pub fn no_more(&self) -> bool {
        self.data.no_more
    }

    pub fn set_no_more(mut self: Pin<&mut Self>, value: bool) {
        if self.data.no_more != value {
            self.as_mut().rust_mut().data.no_more = value;
            self.no_more_changed();
        }
    }

    pub fn selection(&self) -> QVariant {
        let bytes = we_wallpaper::encode_selection(&we_wallpaper::selection_of(&self.data));
        QVariant::from(&QByteArray::from(&bytes[..]))
    }

    pub fn restore_selection(mut self: Pin<&mut Self>, value: &QVariant) {
        let Some(bytes) = selection_bytes(value) else {
            return;
        };
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::restore_selection(&mut rust.data, &bytes)
        };
        self.dispatch(signals);
    }

    pub fn matches_selection(&self, value: &QVariant) -> bool {
        selection_bytes(value).is_some_and(|bytes| we_wallpaper::matches_selection(&self.data, &bytes))
    }

    pub fn hidden_library_ids(&self) -> QVariant {
        json_to_variant(&Value::Array(
            self.data
                .hidden_library_ids
                .iter()
                .map(|id| Value::from(crate::qvalue::i64_to_qml(*id)))
                .collect(),
        ))
    }

    pub fn set_hidden_library_ids(self: Pin<&mut Self>, value: QVariant) {
        // Not a list (undefined from `global.wallpaperHiddenLibraryIds || []` does
        // not get here, but null may) — the core treats it as "nothing hidden".
        let list = variant_to_json(&value);
        self.set_core_property("hiddenLibraryIds", list);
    }

    pub fn toggle_library_hidden(mut self: Pin<&mut Self>, id: f64) {
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::toggle_library_hidden(&mut rust.data, crate::qvalue::qml_to_i64(id))
        };
        self.dispatch(signals);
    }

    pub fn has_other_filters(&self) -> bool {
        self.data.has_other_filters()
    }

    pub fn delay_reload(self: Pin<&mut Self>) {
        let timer = self.debounce;
        if !timer.is_null() {
            unsafe { qobject::rexon_restart_wallpaper_debounce(timer) };
        }
    }

    pub fn has_active_filters(&self) -> bool {
        self.data.has_active_filters()
    }

    pub fn total(&self) -> i32 {
        self.data.total
    }

    pub fn data(&self) -> *mut qobject::QObject {
        self.model
    }

    // ── actions ───────────────────────────────────────────────────────────

    pub fn reload(mut self: Pin<&mut Self>) {
        // A new generation: answers to previous requests are no longer ours, the
        // pages are reset (the original's `reload`: `++m_generation`,
        // `setOffset(0)`, `setNoMore(false)`).
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            rust.pending.clear();
            rust.failed_append = false;
            we_wallpaper::begin_reload(&mut rust.data)
        };
        self.as_mut().dispatch(signals);
        // No connection — the request goes straight to Error, the frame does not
        // go out. The decision and the signals are computed by the core; the
        // branch is the same as Python's `_begin` had.
        let connected = crate::discover_queries::connected();
        let (send, signals) = we_query::begin_call(&mut self.as_mut().rust_mut().base, connected);
        self.as_mut().dispatch(signals);
        if !send {
            return;
        }
        let payload = we_wallpaper::encode_list_request(&self.data);
        self.send_list(payload, Pending::Reload);
    }

    pub fn fetch_more(mut self: Pin<&mut Self>, _count: i32) {
        let Some(payload) = we_wallpaper::encode_fetch_more_request(&self.data, self.base.status)
        else {
            return;
        };
        let connected = crate::discover_queries::connected();
        let (send, signals) = we_query::begin_call(&mut self.as_mut().rust_mut().base, connected);
        self.as_mut().dispatch(signals);
        if !send {
            return;
        }
        let page = self.data.offset + 1;
        self.as_mut().rust_mut().failed_append = true;
        self.send_list(payload, Pending::Page(page));
    }

    pub fn retry(mut self: Pin<&mut Self>) {
        if self.base.status != STATUS_ERROR {
            return;
        }
        if self.failed_append {
            // An error clears "there is more" — loading more is allowed again.
            self.as_mut().rust_mut().data.no_more = false;
            let count = self.data.total;
            self.fetch_more(count);
        } else {
            self.reload();
        }
    }

    /// Send the `wallpaper_list` frame and remember whose answer we expect.
    fn send_list(mut self: Pin<&mut Self>, payload: Vec<u8>, kind: Pending) {
        // The core builds the payload; the transport needs a FULL Request frame
        // with `request_id` already set: the counter is one per process.
        let token = crate::discover_queries::next_token();
        let Ok(bytes) =
            rexon_core::we_client::request_bytes(token as u64, "wallpaper_list", &payload)
        else {
            debug_assert!(false, "поле wallpaper_list неизвестно протоколу");
            return;
        };
        self.as_mut().rust_mut().pending.insert(token, kind);
        let owner = self_ptr(self.as_mut());
        unsafe { we_transport::remember(token, owner.cast()) };
        self.as_mut().send_request(
            token,
            QString::from("wallpaper_list"),
            QByteArray::from(&bytes[..]),
        );
        we_transport::send(token, "wallpaper_list", &bytes);
    }

    pub fn replace_filter_state(mut self: Pin<&mut Self>, filters: QVariant, logics: QVariant) -> bool {
        let filters = rules_value(&filters);
        let logics = rules_value(&logics);
        let (changed, signals) = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::replace_filter_state(&mut rust.data, &filters, &logics)
        };
        self.dispatch(signals);
        changed
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(kind) = self.as_mut().rust_mut().pending.remove(&token) else {
            // The answer is not ours, of a stale generation or already handled — drop
            // it silently.
            return;
        };
        // The original's `t->setHasMore(false)` BEFORE editing the rows: the grid
        // asks `canFetchMore` right from the reset/insert, and the old flag would
        // send a load-more request in the middle of parsing the answer.
        if let Some(model) = unsafe { model_pin(self.model) } {
            model.set_page_has_more(false);
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            // Split the borrow by hand: `&mut rust.base` and `&mut rust.data` are two
            // fields of one struct, but through the Pin wrapper the compiler does not
            // see that.
            let rust = &mut *rust;
            match kind {
                Pending::Reload => we_wallpaper::apply_wallpaper_list(
                    &mut rust.base,
                    &mut rust.data,
                    frame.as_slice(),
                ),
                Pending::Page(page) => we_wallpaper::apply_wallpaper_page(
                    &mut rust.base,
                    &mut rust.data,
                    frame.as_slice(),
                    page,
                ),
            }
        };
        match signals {
            Ok(signals) => {
                if self.base.status != STATUS_ERROR {
                    self.as_mut().rust_mut().failed_append = false;
                }
                self.as_mut().dispatch(signals);
                let more = !self.data.no_more;
                if let Some(model) = unsafe { model_pin(self.model) } {
                    model.set_page_has_more(more);
                }
            }
            Err(_) => {
                // The daemon may be newer than us — crashing in the middle of receiving is
                // not allowed.
                let signals = {
                    let mut rust = self.as_mut().rust_mut();
                    let mut out = we_query::set_error(&mut rust.base, &rexon_core::i18n::tr("could not parse the daemon response"));
                    out.extend(we_query::set_status(&mut rust.base, STATUS_ERROR));
                    out
                };
                self.dispatch(signals);
            }
        }
    }

    // ── internals ─────────────────────────────────────────────────────────

    /// Write a property through the core: it computes the comparison and the
    /// signals.
    pub fn set_core_property(mut self: Pin<&mut Self>, name: &str, value: Value) {
        let Some(signals) = ({
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::set_property(&mut rust.data, name, &value)
        }) else {
            debug_assert!(false, "нет свойства {name} у WallpaperListQuery");
            return;
        };
        self.dispatch(signals);
    }

    /// Emit the signals computed by the core and carry out their side effects:
    /// rebuilding the model and the deferred reload.
    ///
    /// The values are read LIVE (from the state), not as remembered at planning
    /// time: a handler in QML reads the property and must see the new value.
    pub fn dispatch(mut self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        let mut reload = false;
        for signal in signals {
            match signal {
                QuerySignal::StatusChanged(v) => self.as_mut().status_changed(v),
                QuerySignal::QueryingChanged => self.as_mut().querying_changed(),
                QuerySignal::ErrorChanged => self.as_mut().error_changed(),
                QuerySignal::ForwardErrorChanged => self.as_mut().forward_error_changed(),
                QuerySignal::Finished => self.as_mut().finished(),
                QuerySignal::ErrorOccurred(text) => {
                    self.as_mut().error_occurred(QString::from(&text));
                }
                QuerySignal::AppErrorOccurred(text) => {
                    self.as_mut().app_error_occurred(QString::from(&text));
                }
                QuerySignal::Property(name) => {
                    self.as_mut().emit_property(name);
                    if LIST_RELOAD_ON.contains(&name) {
                        reload = true;
                        // The original's `invalidateList` is subscribed to the property's signal
                        // and fires RIGHT after it: `selectionChanged`, resetting the pages,
                        // cancelling the request in flight (a new generation).
                        let follow = {
                            let mut rust = self.as_mut().rust_mut();
                            rust.pending.clear();
                            we_wallpaper::invalidate_list(&mut rust.data)
                        };
                        for next in follow {
                            if let QuerySignal::Property(next) = next {
                                self.as_mut().emit_property(next);
                            }
                        }
                    }
                }
            }
        }
        if reload {
            // The debounce is real: consecutive edits are glued into ONE request, as
            // in the original.
            let timer = self.debounce;
            if !timer.is_null() {
                unsafe { qobject::rexon_restart_wallpaper_debounce(timer) };
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "wpType" => self.as_mut().wp_type_changed(),
            "searchText" => self.as_mut().search_text_changed(),
            "sorts" => self.as_mut().sorts_changed(),
            "filters" => self.as_mut().filters_changed(),
            "filterLogics" => self.as_mut().filter_logics_changed(),
            "skipTypes" => self.as_mut().skip_types_changed(),
            "filterTags" => self.as_mut().filter_tags_changed(),
            "skipContentRatings" => self.as_mut().skip_content_ratings_changed(),
            "hiddenFilter" => self.as_mut().hidden_filter_changed(),
            "hiddenLibraryIds" => self.as_mut().hidden_library_ids_changed(),
            "hasOtherFilters" => self.as_mut().has_other_filters_changed(),
            "filterState" => self.as_mut().filter_state_changed(),
            "hasActiveFilters" => self.as_mut().has_active_filters_changed(),
            "total" => self.as_mut().total_changed(),
            "limit" => self.as_mut().limit_changed(),
            "offset" => self.as_mut().offset_changed(),
            "noMore" => self.as_mut().no_more_changed(),
            "selection" => self.as_mut().selection_changed(),
            "itemsAppended" => {
                // Loading more: the model gets only the missing tail, as an insertion.
                // `itemsChanged` is NOT sent: in the original loading more goes through
                // `t->insert`, not a rebuild, and QML listening for a change of the set
                // must not treat it as a new list.
                if let Some(model) = unsafe { model_pin(self.model) } {
                    let have = model.row_total();
                    let tail = self.data.items.get(have..).unwrap_or_default().to_vec();
                    model.append_rows(tail);
                }
            }
            "items" => {
                // The model is rebuilt FIRST — see the explanation at the signal.
                let rows = self.data.items.clone();
                if let Some(model) = unsafe { model_pin(self.model) } {
                    model.reset_rows(rows);
                }
                self.as_mut().items_changed();
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }
}
