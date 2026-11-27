//! The «Wallpapers» page of the Wallpaper Engine tab — port of waywallen's
//! `ui/src/query/wallpaper_query.cpp` (MIT, (c) hypengw;
//! attribution in NOTICE).
//!
//! This is home to: the library entry converter (`WallpaperEntry` → the map
//! QML reads), the `WallpaperListQuery` state with all its
//! properties (`sorts`/`filters`/`filterLogics`/`skipTypes`/`filterTags`/
//! `skipContentRatings`/`searchText`/`wpType`), building the
//! `wallpaper_list` request and parsing the response. The same entry converter serves
//! `WallpaperGetQuery` — the details panel reads exactly the same keys.
//!
//! A separate module (rather than the tail of `we_query.rs`) along the same boundary
//! as the original: `wallpaper_query.cpp` is its own translation unit, and this is the
//! busiest page of the tab.
//!
//! Sort and filter rules are converted by `we_settings`
//! (`sort_rule_to_map`/`map_to_sort_rule` and friends): the very same rules
//! travel in the daemon settings, and there must be one engine (rule #12).
//!
//! ⚠️ What this port fixes compared to the former Python bridge
//! (rule #16 — bugs are fixed during the port, not preserved):
//!
//!   1. The entry map lost THREE fields: `wallpaperLayoutOverride`,
//!      `wallpaperLayoutOverrideSet` and `supportsItemUnsubscribe`.
//!      `WallpaperDetailPanel.qml` reads all three: without the first two the
//!      «Layout override» block always showed the default layout, and the
//!      reset button was forever grey even for a wallpaper with a saved
//!      personal override; without the third the «Unsubscribe» action
//!      was never enabled and never got into the panel's action set.
//!   2. The `skipTypes`/`filterTags`/`skipContentRatings` setters did not emit
//!      `hasActiveFiltersChanged`, although these lists are part of
//!      `hasActiveFilters`. The highlight of the «Filters» button was updated only
//!      as a side effect of a successful daemon response (the bridge emitted the
//!      signal unconditionally in the response handler) — i.e. with a delay, and on
//!      a request error not at all.
//!   3. `total` was taken as the length of the received list, not from the `count`
//!      field of the response. With `page_size = 0` the values match, but the
//!      daemon's contract is different: `count` is the number of entries matching the filter.
//!
//! Pages (waywallen 0.5.0). QExtra's `QueryList` base — `offset`/`limit`/
//! `noMore` and `fetchMore`: `reload` asks for page 0 of size
//! `(offset + 1) * limit`, `fetchMore` — for page `offset + 1` of size
//! `limit` and appends the rows to the tail. The `WallpaperListQuery` constructor
//! sets `limit = 0`, which means "no pages, return everything": that is how
//! the main «Wallpapers» page lives, and nothing changed for it. Paged
//! loading is used by the smart playlist preview (`PlaylistEditPage`, `limit: 50`
//! and `fetchMore(50)` at the end of scrolling) and «Hidden wallpapers» (`limit: 100`,
//! the grid itself asks for more via the model's `canFetchMore`). The base's `sort`/`asc`
//! are not ported: `WallpaperListRequest` has no such fields, and
//! `initReqForReload` does not set them.
//!
//! Selection (waywallen 0.5.0): `selection` is a snapshot of all fields the
//! daemon selects wallpapers by (`pb::WallpaperSelection`). QML does not parse it,
//! only stores and hands it back (`restoreSelection`/`matchesSelection`,
//! later — the smart playlist's `saveSmart`), so it goes out as
//! opaque bytes, see [`encode_selection`].

use crate::proto::control as pb;
use crate::we_query::{self as q, QuerySignal, QueryState};
use crate::we_settings::{self as s, VMap};
use prost::Message;
use serde_json::Value;

// ── library entry: `WallpaperEntry` → map for QML ───────────────────────
//
// The original has no converter at all: the list carries Qt Protobuf gadgets
// `control::v1::WallpaperEntry`, and QML reads their properties directly. So
// the keys here are the GADGET property names, not the proto fields: camelCase
// (`json_name`) plus one exception — the Qt Protobuf generator names the `id` field
// `id_proto` (`id` is taken in Qt). The key set was checked by grep over the
// vendored `ui/qml`: the details panel and the card read all of them.

/// `WallpaperEntry` → map in QML terms.
pub fn entry_to_map(e: &pb::WallpaperEntry) -> VMap {
    let mut m = VMap::new();
    m.insert("id_proto".into(), Value::String(e.id.clone()));
    m.insert("name".into(), Value::String(e.name.clone()));
    m.insert("wpType".into(), Value::String(e.wp_type.clone()));
    m.insert("resource".into(), Value::String(e.resource.clone()));
    m.insert("preview".into(), Value::String(e.preview.clone()));
    // Proto map order is nondeterministic — sort, otherwise the same
    // entry would keep looking "changed" and trigger a model reset.
    let mut meta = VMap::new();
    let mut keys: Vec<&String> = e.metadata.keys().collect();
    keys.sort();
    for k in keys {
        meta.insert(k.clone(), Value::String(e.metadata[k].clone()));
    }
    m.insert("metadata".into(), Value::Object(meta));
    m.insert("size".into(), Value::from(e.size));
    m.insert("width".into(), Value::from(e.width));
    m.insert("height".into(), Value::from(e.height));
    m.insert(
        "contentRating".into(),
        Value::String(e.content_rating.clone()),
    );
    m.insert(
        "tags".into(),
        Value::Array(e.tags.iter().map(|t| Value::String(t.clone())).collect()),
    );
    m.insert(
        "userPropertiesSchema".into(),
        Value::String(e.user_properties_schema.clone()),
    );
    m.insert(
        "userPropertyOverrides".into(),
        Value::String(e.user_property_overrides.clone()),
    );
    m.insert("description".into(), Value::String(e.description.clone()));
    m.insert("externalId".into(), Value::String(e.external_id.clone()));
    // v0.3.5: the wallpaper's page on the source site («open in browser»).
    // Empty for local wallpapers.
    m.insert("webUrl".into(), Value::String(e.web_url.clone()));
    // The gadget always returns the nested message (an unset one — with default
    // fields), and QML relies on that: it takes `wallpaperLayout`
    // unconditionally and decides by `wallpaperLayoutOverrideSet`. So the key
    // is always put, the map shape must not jump.
    // waywallen 0.4.4: the layout gadget gained `flip` (mirroring), the
    // details panel reads it as `wallpaperLayout.flip`. The shared
    // `layout_to_map` deliberately does not put it: the original's
    // `settings_query.cpp` also carries the settings layout without mirroring.
    let mut layout = s::layout_to_map(
        e.wallpaper_layout_override.as_ref().unwrap_or(&DEFAULT_LAYOUT),
    );
    layout.insert(
        "flip".into(),
        Value::from(
            e.wallpaper_layout_override
                .as_ref()
                .map_or(0, |l| l.flip),
        ),
    );
    m.insert("wallpaperLayoutOverride".into(), Value::Object(layout));
    m.insert(
        "wallpaperLayoutOverrideSet".into(),
        Value::Bool(e.wallpaper_layout_override_set),
    );
    m.insert(
        "supportsItemRemove".into(),
        Value::Bool(e.supports_item_remove),
    );
    m.insert(
        "supportsItemUnsubscribe".into(),
        Value::Bool(e.supports_item_unsubscribe),
    );
    // waywallen 0.4.4: the wallpaper is hidden from the library (`wallpaper_hide`).
    // Read by the card (the «hidden» badge), the details panel and the selection
    // store's `hiddenSelectedCount` counter.
    m.insert("hidden".into(), Value::Bool(e.hidden));
    // waywallen 0.6.0: the wallpaper has an update at the source (a badge on the
    // card and in the details) and an entry subtype — 1 = «timeline» (wallpapers
    // that change with the time of day, `WALLPAPER_SUB_TYPE_TIMELINE`). The subtype is
    // a number, as in the original's gadget: the page compares `subType === 1`.
    m.insert("updateAvailable".into(), Value::Bool(e.update_available));
    m.insert("subType".into(), Value::from(e.sub_type));
    m
}

/// Empty layout for an entry without a personal override.
const DEFAULT_LAYOUT: pb::LayoutPrefs = pb::LayoutPrefs {
    fillmode: 0,
    align: 0,
    rotation: 0,
    location_x: 0,
    location_y: 0,
    location_set: false,
    // waywallen 0.4.4: FLIP_UNSPECIFIED — no mirroring and none set.
    flip: 0,
};

// ── WallpaperListQuery ──────────────────────────────────────────────────

/// `WallpaperListQuery` state: filter properties + the response result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WallpaperListData {
    pub wp_type: String,
    pub search_text: String,
    pub sorts: Vec<pb::WallpaperSortRule>,
    pub filters: Vec<pb::WallpaperFilterRule>,
    pub filter_logics: Vec<pb::FilterLogic>,
    pub skip_types: Vec<String>,
    pub filter_tags: Vec<String>,
    pub skip_content_ratings: Vec<String>,
    /// waywallen 0.4.4: `WallpaperHiddenFilter` — 0 hide hidden ones
    /// (daemon default), 1 only hidden ones, 2 all.
    pub hidden_filter: i32,
    /// waywallen 0.5.0: ids of HIDDEN libraries (`hiddenLibraryIds`) —
    /// always ascending and without duplicates, as the original keeps them. An empty
    /// list — «show all».
    pub hidden_library_ids: Vec<i64>,
    /// `count` of the last daemon response (the `total` property).
    pub total: i32,
    /// `limit` of the `QueryList` base: page size. 0 — no pages, the daemon
    /// returns the whole list (the original constructor's default).
    pub limit: i32,
    /// The base's `offset`: number of the last loaded page (0 — only the
    /// first). Reset by any selection change and by every `reload`.
    pub offset: i32,
    /// The base's `noMore`: there are no further pages. Set by response parsing.
    pub no_more: bool,
    /// The original's `m_reload_pending`: the selection changed, the deferred `reload`
    /// has not gone out yet. While it is pending, `fetchMore` does not load more — a page
    /// of the old selection would land in the tail of the new one.
    pub reload_pending: bool,
    /// Rows of the library model — [`entry_to_map`] maps.
    pub items: Vec<VMap>,
}

impl WallpaperListData {
    /// The original's `hasActiveFilters`: hidden libraries OR other filters
    /// (waywallen 0.5.0). The group logic is NOT part of it — by itself it
    /// filters nothing out.
    pub fn has_active_filters(&self) -> bool {
        !self.hidden_library_ids.is_empty() || self.has_other_filters()
    }

    /// The original's `hasOtherFilters` (waywallen 0.5.0): everything except hidden
    /// libraries. The page uses it to highlight the «Filters» button, while hidden
    /// libraries are shown separately by the «Library manager» button.
    pub fn has_other_filters(&self) -> bool {
        !self.filters.is_empty()
            || !self.skip_types.is_empty()
            || !self.filter_tags.is_empty()
            || !self.skip_content_ratings.is_empty()
            || self.hidden_filter != 0
    }
}

/// Library ids from a QML value: numbers (from QML they arrive as
/// doubles) or strings holding a number; anything else is dropped. The result is
/// ascending and without duplicates, like the original's `setHiddenLibraryIds`.
pub fn library_ids(value: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = match value {
        Value::Array(items) => items
            .iter()
            .filter(|v| v.is_number() || v.as_str().is_some_and(|t| t.trim().parse::<i64>().is_ok()))
            .map(s::value_i64)
            .collect(),
        _ => Vec::new(),
    };
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Property names [`set_property`] accepts.
pub const LIST_PROPERTIES: &[&str] = &[
    "wpType",
    "searchText",
    "sorts",
    "filters",
    "filterLogics",
    "skipTypes",
    "filterTags",
    "skipContentRatings",
    "hiddenFilter",
    // waywallen 0.5.0
    "hiddenLibraryIds",
];

fn rules<T>(value: &Value, from_map: fn(&VMap) -> T) -> Vec<T> {
    match value {
        Value::Array(items) => items.iter().map(|v| from_map(&s::as_map(v))).collect(),
        _ => Vec::new(),
    }
}

fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => s::string_list(items),
        _ => Vec::new(),
    }
}

/// Write a `WallpaperListQuery` property — the original's setters in full.
///
/// `None` — the name is not among [`LIST_PROPERTIES`]. Signal order exactly
/// as in the original: first the property's own signal, then
/// `filterStateChanged` (it alone triggers the list reload for both
/// changes — rules and group logic), and only then
/// `hasActiveFiltersChanged`, if the «filters on» flag flipped.
///
/// The original's `setOffset(0)` and `selectionChanged` are not emitted here: both
/// are a consequence of `invalidateList`, which the original subscribes to the
/// selection property signals. In the port the bridge runs it on the same signals
/// ([`invalidate_list`]), so the signal order of the setters themselves stays
/// unchanged.
pub fn set_property(
    d: &mut WallpaperListData,
    name: &str,
    value: &Value,
) -> Option<Vec<QuerySignal>> {
    let mut out = Vec::new();
    let had_other = d.has_other_filters();
    match name {
        "wpType" => q::assign(&mut d.wp_type, s::value_str(value), "wpType", &mut out),
        "searchText" => q::assign(
            &mut d.search_text,
            s::value_str(value),
            "searchText",
            &mut out,
        ),
        "sorts" => q::assign(
            &mut d.sorts,
            rules(value, s::map_to_sort_rule),
            "sorts",
            &mut out,
        ),
        "filters" => {
            let had = d.has_active_filters();
            q::assign(
                &mut d.filters,
                rules(value, s::map_to_filter_rule),
                "filters",
                &mut out,
            );
            if !out.is_empty() {
                out.push(QuerySignal::Property("filterState"));
                push_other_flip(d, had_other, &mut out);
                if had != d.has_active_filters() {
                    out.push(QuerySignal::Property("hasActiveFilters"));
                }
            }
        }
        "filterLogics" => {
            q::assign(
                &mut d.filter_logics,
                rules(value, s::map_to_filter_logic),
                "filterLogics",
                &mut out,
            );
            if !out.is_empty() {
                out.push(QuerySignal::Property("filterState"));
            }
        }
        "skipTypes" | "filterTags" | "skipContentRatings" => {
            let had = d.has_active_filters();
            let (slot, signal) = match name {
                "skipTypes" => (&mut d.skip_types, "skipTypes"),
                "filterTags" => (&mut d.filter_tags, "filterTags"),
                _ => (&mut d.skip_content_ratings, "skipContentRatings"),
            };
            q::assign(slot, strings(value), signal, &mut out);
            if !out.is_empty() {
                push_other_flip(d, had_other, &mut out);
            }
            if !out.is_empty() && had != d.has_active_filters() {
                out.push(QuerySignal::Property("hasActiveFilters"));
            }
        }
        // waywallen 0.4.4: `setHiddenFilter` — same order as the
        // lists above: the property signal, then `hasActiveFilters`.
        "hiddenFilter" => {
            let had = d.has_active_filters();
            q::assign(&mut d.hidden_filter, s::value_i32(value), "hiddenFilter", &mut out);
            if !out.is_empty() {
                push_other_flip(d, had_other, &mut out);
            }
            if !out.is_empty() && had != d.has_active_filters() {
                out.push(QuerySignal::Property("hasActiveFilters"));
            }
        }
        // waywallen 0.5.0: `setHiddenLibraryIds` — the list is normalized
        // (ascending, no duplicates) BEFORE comparing, so [3,1,3]
        // over [1,3] is silence. Hidden libraries do not touch
        // `hasOtherFilters` by definition.
        "hiddenLibraryIds" => {
            let had = d.has_active_filters();
            q::assign(
                &mut d.hidden_library_ids,
                library_ids(value),
                "hiddenLibraryIds",
                &mut out,
            );
            if !out.is_empty() && had != d.has_active_filters() {
                out.push(QuerySignal::Property("hasActiveFilters"));
            }
        }
        _ => return None,
    }
    Some(out)
}

/// `hasOtherFiltersChanged` — right after the property's own signal (and after
/// `filterStateChanged`), before `hasActiveFiltersChanged`: the original
/// subscribes it to the property signals, and the setter emits `hasActiveFiltersChanged`
/// last. The original emits it on EVERY change of these properties; the port —
/// only when the flag flipped (like `hasActiveFilters` and the other
/// properties of the batch): QML reads it through a binding, and an idle signal is
/// of no use to it.
fn push_other_flip(d: &WallpaperListData, had_other: bool, out: &mut Vec<QuerySignal>) {
    if had_other != d.has_other_filters() {
        out.push(QuerySignal::Property("hasOtherFilters"));
    }
}

/// The original's `toggleLibraryHidden` (waywallen 0.5.0): the id was hidden —
/// remove it, was not — add it; then the regular `hiddenLibraryIds` setter.
pub fn toggle_library_hidden(d: &mut WallpaperListData, id: i64) -> Vec<QuerySignal> {
    let mut ids = d.hidden_library_ids.clone();
    match ids.iter().position(|v| *v == id) {
        Some(at) => {
            ids.remove(at);
        }
        None => ids.push(id),
    }
    let value = Value::Array(ids.into_iter().map(Value::from).collect());
    set_property(d, "hiddenLibraryIds", &value).unwrap_or_default()
}

/// The original's `replaceFilterState`: rules and group logic change at once,
/// i.e. the page gets ONE `filterStateChanged` and one
/// list reload instead of two. Returns `(whether it changed, signals)`.
pub fn replace_filter_state(
    d: &mut WallpaperListData,
    filters: &Value,
    logics: &Value,
) -> (bool, Vec<QuerySignal>) {
    let new_filters = rules(filters, s::map_to_filter_rule);
    let new_logics = rules(logics, s::map_to_filter_logic);
    let filters_changed = d.filters != new_filters;
    let logics_changed = d.filter_logics != new_logics;
    if !filters_changed && !logics_changed {
        return (false, Vec::new());
    }
    let had = d.has_active_filters();
    let had_other = d.has_other_filters();
    d.filters = new_filters;
    d.filter_logics = new_logics;
    let mut out = Vec::new();
    if filters_changed {
        out.push(QuerySignal::Property("filters"));
    }
    if logics_changed {
        out.push(QuerySignal::Property("filterLogics"));
    }
    out.push(QuerySignal::Property("filterState"));
    push_other_flip(d, had_other, &mut out);
    if had != d.has_active_filters() {
        out.push(QuerySignal::Property("hasActiveFilters"));
    }
    (true, out)
}

// ── Selection (waywallen 0.5.0) ─────────────────────────────────────────

/// The original's `selection()`: all fields the daemon selects wallpapers by —
/// exactly as they lie in the properties (without dropping unusable
/// rules: an unfilled editor row is part of the draft too, and
/// `restoreSelection` must put it back).
pub fn selection_of(d: &WallpaperListData) -> pb::WallpaperSelection {
    pb::WallpaperSelection {
        filters: d.filters.clone(),
        filter_logics: d.filter_logics.clone(),
        sorts: d.sorts.clone(),
        search_text: d.search_text.clone(),
        wp_type: d.wp_type.clone(),
        skip_types: d.skip_types.clone(),
        filter_tags: d.filter_tags.clone(),
        skip_content_ratings: d.skip_content_ratings.clone(),
        hidden_filter: d.hidden_filter,
        hidden_library_ids: d.hidden_library_ids.clone(),
    }
}

/// Tag of an opaque selection value.
///
/// The value goes to QML as bytes (an `ArrayBuffer` in JS): the tag + the prost encoding
/// of `pb::WallpaperSelection`. The tag is needed because for an empty selection the
/// prost encoding is zero bytes, and without it an «empty selection» would be
/// indistinguishable from an empty string or `null`, which QML happily coerces to a byte array.
/// In the original the gadget type plays this role: `canConvert<WallpaperSelection>`
/// is false for anything that is not a selection — `matchesSelection(undefined)` gives
/// false, `restoreSelection(undefined)` does nothing.
pub const SELECTION_TAG: &[u8] = b"wwsel1:";

/// Selection → opaque value for QML.
pub fn encode_selection(s: &pb::WallpaperSelection) -> Vec<u8> {
    let mut out = SELECTION_TAG.to_vec();
    s.encode(&mut out).expect("Vec<u8> растёт сам");
    out
}

/// Opaque value from QML → selection. `None` — it is not a selection (no
/// tag or the encoding does not parse).
pub fn decode_selection(bytes: &[u8]) -> Option<pb::WallpaperSelection> {
    let body = bytes.strip_prefix(SELECTION_TAG)?;
    pb::WallpaperSelection::decode(body).ok()
}

/// The original's `matchesSelection`: compares MESSAGES, not bytes — one
/// message may have more than one encoding.
pub fn matches_selection(d: &WallpaperListData, bytes: &[u8]) -> bool {
    decode_selection(bytes).is_some_and(|s| s == selection_of(d))
}

/// The original's `restoreSelection`: spread the selection over the properties through
/// the REGULAR setters in its order (`replaceFilterState`, `sorts`,
/// `searchText`, `wpType`, `skipTypes`, `filterTags`, `skipContentRatings`,
/// `hiddenFilter`, `hiddenLibraryIds`). A matching field stays silent, so
/// restoring the current selection emits no signal at all and does not
/// reload the list. Not a selection — do nothing.
///
/// Values go through maps — the same way QML writes them: the port has no
/// other "value → property" path and must not have one.
pub fn restore_selection(d: &mut WallpaperListData, bytes: &[u8]) -> Vec<QuerySignal> {
    let Some(sel) = decode_selection(bytes) else {
        return Vec::new();
    };
    let maps = |items: Vec<VMap>| Value::Array(items.into_iter().map(Value::Object).collect());
    let texts = |items: &[String]| Value::Array(items.iter().cloned().map(Value::String).collect());
    let filters = maps(sel.filters.iter().map(s::filter_rule_to_map).collect());
    let logics = maps(sel.filter_logics.iter().map(s::filter_logic_to_map).collect());
    let (_, mut out) = replace_filter_state(d, &filters, &logics);
    let steps: [(&str, Value); 8] = [
        ("sorts", maps(sel.sorts.iter().map(s::sort_rule_to_map).collect())),
        ("searchText", Value::String(sel.search_text.clone())),
        ("wpType", Value::String(sel.wp_type.clone())),
        ("skipTypes", texts(&sel.skip_types)),
        ("filterTags", texts(&sel.filter_tags)),
        ("skipContentRatings", texts(&sel.skip_content_ratings)),
        ("hiddenFilter", Value::from(sel.hidden_filter)),
        (
            "hiddenLibraryIds",
            Value::Array(sel.hidden_library_ids.iter().copied().map(Value::from).collect()),
        ),
    ];
    for (name, value) in steps {
        out.extend(set_property(d, name, &value).unwrap_or_default());
    }
    out
}

/// The original's `invalidateList` — what ANY selection change does:
/// `selectionChanged`, resetting pages (the setters' `setOffset(0)`) and the
/// «reload in flight» mark. The generation and cancelling the in-flight request are
/// done by the bridge (it holds the frame tokens), the deferred reload by its debounce.
pub fn invalidate_list(d: &mut WallpaperListData) -> Vec<QuerySignal> {
    let mut out = vec![QuerySignal::Property("selection")];
    q::assign(&mut d.offset, 0, "offset", &mut out);
    d.reload_pending = true;
    out
}

/// Write `limit`. Returns `(signals, whether to reload the list)`.
///
/// In the original `limitChanged → invalidateList`: changing the page size
/// emits `selectionChanged` and reloads the list deferred. The port does
/// not always do so — a deliberate difference. The vendor QML writes `limit`
/// declaratively (`limit: 50` of the smart playlist preview, `limit: 100`
/// of «Hidden wallpapers»), i.e. IN THE CONSTRUCTOR, when the list has never been
/// loaded. The original sends a request at that point that the page did not
/// ask for: «Hidden wallpapers» calls `delayReload` from
/// `Component.onCompleted` anyway, and the playlist editor decides itself when to
/// load (`reload` after `restoreSelection`) — and the extra request
/// arrives with an EMPTY selection, i.e. it briefly shows the whole library.
/// So before the first run (`status == Uninitialized`) a `limit` change
/// is only remembered and emits `selectionChanged`, and afterwards it behaves
/// like the original: the list is already shown, and the new page must arrive with
/// the new size.
pub fn set_limit(
    d: &mut WallpaperListData,
    value: i32,
    status: i32,
) -> (Vec<QuerySignal>, bool) {
    let mut out = Vec::new();
    q::assign(&mut d.limit, value, "limit", &mut out);
    if out.is_empty() {
        return (out, false);
    }
    if status == q::STATUS_UNINITIALIZED {
        out.push(QuerySignal::Property("selection"));
        return (out, false);
    }
    out.extend(invalidate_list(d));
    (out, true)
}

// ── Pages: reload and fetchMore ─────────────────────────────────────────

/// Build a `WallpaperListRequest` for page `page` of size `page_size`.
///
/// Selection fields — [`selection_of`] as is, exactly like the original's
/// `listRequest`: the fields are listed ONCE in the project (rule #12), and the smart
/// playlist (`saveSmart`) sends the same selection.
///
/// ⛔ Rules are NOT filtered here. From 24.10.2026 until this change unusable
/// rules were silently dropped — and when the gadget shim corrupted the payload
/// (see `rexon_gadget_shim.cpp`, `isPointerProperty`), ALL of them were dropped:
/// the preview showed all wallpapers, and the playlist was saved without filters.
/// Daemon 0.5.0 rejects an unfilled rule (`InvalidArgument`,
/// «invalid wallpaper selection»), and the page shows its error — just
/// like the original.
fn list_request(d: &WallpaperListData, page: u32, page_size: u32) -> pb::WallpaperListRequest {
    let s = selection_of(d);
    pb::WallpaperListRequest {
        wp_type: s.wp_type,
        page,
        page_size,
        filters: s.filters,
        filter_logics: s.filter_logics,
        sorts: s.sorts,
        search_text: s.search_text,
        // WE tab boundaries: hidden types (gltf) are filtered out by the daemon itself, so
        // `count` and the pages are honest. They are added only to the frame, not to the
        // selection: the `skipTypes` property and the smart playlist are the person's choice.
        skip_types: q::we_tab_skip_types(&s.skip_types),
        filter_tags: s.filter_tags,
        skip_content_ratings: s.skip_content_ratings,
        // waywallen 0.4.4: which wallpapers to show by the «hidden» flag.
        hidden_filter: s.hidden_filter,
        // waywallen 0.5.0: hidden libraries. The message is put ALWAYS,
        // as in the original (`listRequest`) — an empty list means «show
        // all». A daemon before 0.5.0 does not know field 14 and skips it.
        library_visibility: Some(pb::WallpaperLibraryVisibility {
            hidden_library_ids: s.hidden_library_ids,
        }),
    }
}

/// Start of `reload()`: the generation changed, the deferred reload is gone,
/// pages are reset (the original's `setOffset(0)`, `setNoMore(false)`).
pub fn begin_reload(d: &mut WallpaperListData) -> Vec<QuerySignal> {
    let mut out = Vec::new();
    d.reload_pending = false;
    q::assign(&mut d.offset, 0, "offset", &mut out);
    q::assign(&mut d.no_more, false, "noMore", &mut out);
    out
}

/// Build a `WallpaperListRequest` for `reload()` and serialize it.
///
/// `page = 0`, `page_size = (offset + 1) * limit` from the base's
/// `initReqForReload`. After [`begin_reload`] `offset` is always 0, so the size is exactly
/// `limit`; the main page has `limit = 0`, and that is «return everything» per
/// the protocol.
pub fn encode_list_request(d: &WallpaperListData) -> Vec<u8> {
    let size = (d.offset.max(0) + 1).saturating_mul(d.limit.max(0));
    list_request(d, 0, size as u32).encode_to_vec()
}

/// Build the `fetchMore()` frame: page `offset + 1` of size `limit`
/// (the base's `initReqForFetchMore`). `None` — no more can be loaded, the request does not
/// start.
///
/// The original's conditions: there are no further pages (`noMore`), a selection change
/// waits for its reload, or a request is already in flight. The fourth condition is
/// ours: `limit <= 0`. In the original `fetchMore` before the first response
/// (`noMore` still false) with `limit = 0` asks for page 1 of size 0, i.e.
/// the WHOLE list once more, and appends it to the tail — the library
/// doubles. No pages — nothing to load.
pub fn encode_fetch_more_request(d: &WallpaperListData, status: i32) -> Option<Vec<u8>> {
    if d.no_more || d.reload_pending || status == q::STATUS_QUERYING || d.limit <= 0 {
        return None;
    }
    let page = (d.offset.max(0) as u32).saturating_add(1);
    Some(list_request(d, page, d.limit as u32).encode_to_vec())
}

/// Parse the `wallpaper_list` response to `reload()`.
///
/// The `items` signal is ours: in the original the list goes to the model via
/// `t->sync(items)` (smart store sync), while our Qt model
/// lives in the shim, and it needs an explicit reason to rebuild. We emit it
/// only on a REAL list change: a model reset loses the scroll position
/// and the selection, and the daemon sends the same list after every
/// library rescan.
pub fn apply_wallpaper_list(
    st: &mut QueryState,
    data: &mut WallpaperListData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::WallpaperList(wl)) = rsp.payload {
        let items: Vec<VMap> = wl.wallpapers.iter().map(entry_to_map).collect();
        q::assign(&mut data.items, items, "items", &mut out);
        q::assign(&mut data.total, wl.count as i32, "total", &mut out);
        let more = more_pages(data, wl.wallpapers.is_empty());
        q::assign(&mut data.no_more, !more, "noMore", &mut out);
    }
    Ok(q::finish(st, out))
}

/// Parse the `wallpaper_list` response to `fetchMore()`: the page rows
/// are appended to the tail (the `itemsAppended` signal — the bridge appends them to the
/// model by insertion, not by reset: scrolling and selection stay in place),
/// `offset` becomes the number of this page.
pub fn apply_wallpaper_page(
    st: &mut QueryState,
    data: &mut WallpaperListData,
    frame_bytes: &[u8],
    page: i32,
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::WallpaperList(wl)) = rsp.payload {
        if !wl.wallpapers.is_empty() {
            data.items.extend(wl.wallpapers.iter().map(entry_to_map));
            out.push(QuerySignal::Property("itemsAppended"));
        }
        q::assign(&mut data.offset, page, "offset", &mut out);
        q::assign(&mut data.total, wl.count as i32, "total", &mut out);
        let more = more_pages(data, wl.wallpapers.is_empty());
        q::assign(&mut data.no_more, !more, "noMore", &mut out);
    }
    Ok(q::finish(st, out))
}

/// The original's `more`: there are fewer rows than match the selection, and the last
/// page is not empty (an empty one means the daemon will give nothing further,
/// even if `count` says otherwise).
fn more_pages(d: &WallpaperListData, page_empty: bool) -> bool {
    (d.items.len() as i64) < i64::from(d.total) && !page_empty
}

// ── WallpaperGetQuery (details panel) ───────────────────────────────────

/// The `wallpaper` property of the `WallpaperGetQuery` request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WallpaperGetData {
    pub wallpaper: VMap,
}

/// Parse the `wallpaper_get` response.
///
/// A response without an entry is treated like the original — as an entry with default
/// fields: in Qt Protobuf reading an unset message returns exactly that. For
/// the panel this means «no fresh data» — it falls back to the
/// list row by itself (`id_proto` empty).
pub fn apply_wallpaper_get(
    st: &mut QueryState,
    data: &mut WallpaperGetData,
    frame_bytes: &[u8],
) -> Result<Vec<QuerySignal>, prost::DecodeError> {
    let (answer, mut out) = q::take_response(st, frame_bytes)?;
    let Some(rsp) = answer else {
        return Ok(out);
    };
    if let Some(pb::response::Payload::WallpaperGet(wg)) = rsp.payload {
        let m = entry_to_map(&wg.entry.unwrap_or_default());
        q::assign(&mut data.wallpaper, m, "wallpaper", &mut out);
    }
    Ok(q::finish(st, out))
}

// ── WallpaperApplyQuery: playlists stopped by applying ──────────────────

/// The list of playlists the daemon stopped while applying a wallpaper — in the
/// form the details panel reads it (`WallpaperDetailPanel.qml`,
/// `toastStoppedPlaylists`): one map per playlist with the keys `playlistId`,
/// `playlistName`, `allDisplays` and `displayIds`.
///
/// Applying a single wallpaper to a display where a playlist was running removes
/// the playlist; doing it silently is not allowed — the person would see that the
/// scheduled wallpaper change simply stopped working. An empty list — «nothing
/// stopped».
pub fn stopped_playlists(rsp: &pb::WallpaperApplyResponse) -> Value {
    Value::Array(
        rsp.stopped_playlists
            .iter()
            .map(|p| {
                serde_json::json!({
                    "playlistId": p.playlist_id,
                    "playlistName": p.playlist_name,
                    "allDisplays": p.all_displays,
                    "displayIds": p.display_ids,
                })
            })
            .collect(),
    )
}

// ── WallpaperLayoutSetQuery (layout of a single wallpaper) ──────────────

/// Properties of `WallpaperLayoutSetQuery`.
///
/// The layout is set for ONE wallpaper and overrides the shared one (the settings'
/// `layout_defaults`). The details panel writes position, fit and rotation here,
/// and sends a reset as a separate `clear` flag — then the layout body is not
/// sent at all and the daemon removes the personal override.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutSetData {
    pub wallpaper_id: String,
    pub clear: bool,
    pub fillmode: i32,
    pub location_x: i32,
    pub location_y: i32,
    pub rotation: i32,
    /// waywallen 0.4.4: mirroring (`Flip`). The original's default is
    /// `FLIP_NONE` (1); the Qt wrapper's constructor sets it.
    pub flip: i32,
}

/// Property names [`set_layout_property`] accepts.
pub const LAYOUT_SET_PROPERTIES: &[&str] = &[
    "wallpaperId",
    "clear",
    "fillmode",
    "locationX",
    "locationY",
    "rotation",
    "flip",
];

/// Write a `WallpaperLayoutSetQuery` property.
///
/// In the original all six setters are one `WW_SET` macro: assign and, if the
/// value REALLY changed, emit the shared `paramsChanged`.
/// There are no per-property signals, so there is one signal name here —
/// `params`. `None` — the name is not among [`LAYOUT_SET_PROPERTIES`].
pub fn set_layout_property(
    d: &mut LayoutSetData,
    name: &str,
    value: &Value,
) -> Option<Vec<QuerySignal>> {
    let mut out = Vec::new();
    match name {
        "wallpaperId" => q::assign(
            &mut d.wallpaper_id,
            s::value_str(value),
            "params",
            &mut out,
        ),
        "clear" => q::assign(&mut d.clear, s::value_bool(value), "params", &mut out),
        "fillmode" => q::assign(&mut d.fillmode, s::value_i32(value), "params", &mut out),
        "locationX" => q::assign(&mut d.location_x, s::value_i32(value), "params", &mut out),
        "locationY" => q::assign(&mut d.location_y, s::value_i32(value), "params", &mut out),
        "rotation" => q::assign(&mut d.rotation, s::value_i32(value), "params", &mut out),
        "flip" => q::assign(&mut d.flip, s::value_i32(value), "params", &mut out),
        _ => return None,
    }
    Some(out)
}

/// Build a `WallpaperLayoutSetRequest`.
///
/// `None` — no wallpaper selected: in that case the original leaves `reload()`
/// BEFORE switching the status, i.e. the request does not start at all.
///
/// The layout body is sent only with `clear == false`. `location_set` is
/// always `true`: the position is set together with the rest, there is no separate
/// «own position» toggle in the details panel. Percentages are clamped to
/// 0…100 — like the original, duplicating the QML limit (the page may
/// send anything).
///
/// `align` is not filled: the original does not have it here, and the panel has no
/// alignment control. The field stays at its default value.
pub fn encode_layout_set(d: &LayoutSetData) -> Option<Vec<u8>> {
    if d.wallpaper_id.is_empty() {
        return None;
    }
    let layout = if d.clear {
        None
    } else {
        Some(pb::LayoutPrefs {
            fillmode: d.fillmode,
            rotation: d.rotation,
            location_x: d.location_x.clamp(0, 100) as u32,
            location_y: d.location_y.clamp(0, 100) as u32,
            location_set: true,
            flip: d.flip,
            ..Default::default()
        })
    };
    Some(
        pb::WallpaperLayoutSetRequest {
            wallpaper_id: d.wallpaper_id.clone(),
            clear: d.clear,
            layout,
        }
        .encode_to_vec(),
    )
}


// ── WallpaperHideQuery (hide wallpapers, waywallen 0.4.4) ───────────────

/// Non-empty ids in the original order — the original's `setHiddenForIds`
/// filters out empty ones before anything else.
pub fn non_empty_ids(ids: &[String]) -> Vec<String> {
    ids.iter().filter(|id| !id.is_empty()).cloned().collect()
}

/// Build a `WallpaperHideRequest`.
///
/// `None` — after filtering out empty ones not a single id is left:
/// in that case the original leaves BEFORE switching the status, there is no request at all.
/// A single id goes to `wallpaper_id`, a set — to
/// `wallpaper_ids`, exactly as in the original.
pub fn encode_hide_request(ids: &[String], hidden: bool) -> Option<Vec<u8>> {
    let ids = non_empty_ids(ids);
    let request = match ids.as_slice() {
        [] => return None,
        [one] => pb::WallpaperHideRequest {
            wallpaper_id: one.clone(),
            hidden,
            ..Default::default()
        },
        _ => pb::WallpaperHideRequest {
            wallpaper_ids: ids,
            hidden,
            ..Default::default()
        },
    };
    Some(request.encode_to_vec())
}

#[cfg(test)]
mod we_tab_scope_tests {
    use super::*;

    /// WE tab boundaries: the daemon cuts off glTF by the frame's `skip_types`, while
    /// the person's choice (`skipTypes`, the smart playlist selection) does not change.
    #[test]
    fn list_request_skips_hidden_types_on_the_wire_only() {
        let mut d = WallpaperListData::default();
        d.skip_types = vec!["web".into()];
        let req = pb::WallpaperListRequest::decode(encode_list_request(&d).as_slice())
            .expect("кадр разобран");
        assert_eq!(req.skip_types, ["web", "gltf"]);
        d.limit = 10;
        let page = encode_fetch_more_request(&d, q::STATUS_FINISHED).expect("страница");
        let req = pb::WallpaperListRequest::decode(page.as_slice()).expect("кадр разобран");
        assert_eq!(req.skip_types, ["web", "gltf"]);
        assert_eq!(d.skip_types, ["web"]);
        assert_eq!(selection_of(&d).skip_types, ["web"]);
    }
}
