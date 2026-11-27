//! Labels of wallpaper catalogs in the interface language (Steam Workshop,
//! Wallhaven) — port of `rexon/qt_wallpaper_engine/qml_ui/remote_l10n.py`.
//!
//! The daemon's source plugins return everything in English: the source name,
//! sort orders, filter names, login buttons, status lines. The buyer must not
//! see "Top rated" and "Not signed in" (rule #13), but the vendored QML must not
//! be edited for this either — vendor updates go as a `git diff`. So the
//! translation lives BETWEEN the daemon and QML: the source description is
//! passed through [`Localizer::translate_source`].
//!
//! Filter values (tags) go back to Steam/Wallhaven as request parameters, so a
//! translated value is turned back into the original before sending —
//! [`Localizer::untranslate_tags`]. Both sides take ONE map, [`TAGS`], so a new
//! tag needs to be written only once.
//!
//! An unknown string stays as is: an unfamiliar sort order of a new plugin will
//! show in English but will break nothing.
//!
//! # Labels of the vendored QML itself — only through a `.qm` catalog
//!
//! The pages' strings are embedded in the vendored `.qml` via `qsTr()`. The right
//! (and the only safe) way is to build a real Qt translation catalog and give it
//! to `QTranslator::load()`.
//!
//! Never: the translator must be a REAL `.qm` catalog, not a `QTranslator` with
//! an overridden `translate()` method on the Python side. Qt asks for
//! translations from the QQmlThread too (page compilation), and PySide6 takes
//! the GIL on a call to a Python method; the main thread at that moment sits
//! inside a synchronous QML load and does not release the GIL — both threads
//! hang forever, and the tab is "not responding" (checked live, commit
//! 3ed7dd2). A `.qm` catalog looks strings up in C++ and blocks no threads.
//!
//! # What is here and what is not
//!
//! Here is PURE logic (deterministic input→output), as in `system_plan.rs` /
//! `importers.rs` / `compliance.rs`:
//!   * all translation tables and lookup in them;
//!   * translating a source description and translating tags back (with the
//!     per-source map state — in [`Localizer`]);
//!   * checking the vendored translation catalog against the vendored `.qml` and
//!     the `lrelease` argv — [`plan_build_qm`] (a PLAN of the step, it executes
//!     nothing);
//!   * choosing the language of the vendored pages — [`ui_language_codes`],
//!     [`normalize_ui_language`], [`plan_ui_language`].
//!
//! NOT ported (execution is behind the bridge): walking the `.qml` tree (`glob`)
//! and reading files, reading the vendored `.ts`, running `lrelease`
//! (`subprocess.run(check=True)` → the executor produces
//! `FileNotFoundError`/`CalledProcessError`), `QCoreApplication.instance()`,
//! `QTranslator::load/installTranslator`, `os.path.isfile` — their RESULT is
//! passed as a parameter.
//!
//! # Fixes to the original (rule #16)
//!
//!   1. `untranslate_tags` took the source's map via `or` — an empty string in
//!      the source's map silently fell through to the shared map. Here the
//!      PRESENCE of the key is checked. The fix is PROTECTIVE: today no
//!      difference is visible, because an empty value gets into the map only
//!      under an empty label, and the shared map answers nothing for it either;
//!      but `or` instead of a key check is the wrong idiom, and the very first
//!      label with an empty value would silently break a filter.
//!   2. `translate_source` rebuilt the `sorts` element from two fields and lost
//!      all other keys (unlike `filters`/`actions`, which are copied entirely).
//!      Here extra keys are preserved.
//!   3. The source's back-translation map in Python accumulated forever
//!      (`setdefault`): after a plugin update it kept labels of tags that had
//!      disappeared. Here the source's map is rebuilt on every
//!      `translate_source`.
//!   4. The catalog builder saw only `qsTr("…")` and did not notice `qsTr('…')`
//!      — QML allows both quotes equally, and the string silently stayed
//!      untranslated. Here both are recognized (the vendor writes single ones,
//!      for example `qsTr('Status')` in `Window.qml` v0.3.5).
//!   5. Divergence between the catalog and `ui/qml` was invisible in both
//!      directions. Now the plan names both vendored strings without a
//!      translation ([`QmBuildPlan::untranslated`]) and catalog entries that are
//!      no longer in `ui/qml` ([`QmBuildPlan::stale`]).
//!   6. The path to `lrelease` was hard-coded for Arch
//!      (`/usr/lib/qt6/bin/lrelease`). The candidates are moved into
//!      [`LRELEASE_CANDIDATES`] — the executor takes the first one found
//!      (`which`), the hard-coded path remains the last fallback.
//!   7. Global mutable state (`_TAGS_BACK_BY_SOURCE`) is replaced by a field of
//!      [`Localizer`]: two tabs/two threads no longer share one map.
//!   8. Garbage in a source description (a `filters`/`actions` element that is
//!      not a dictionary) raised `AttributeError` in the middle of translation
//!      and killed the daemon's whole output; now such an element is skipped,
//!      and the rest are translated.
//!   9. The builder knew only single strings: a call with a counter
//!      (`qsTr("%n wallpaper(s)", "", n)`) was not recognized AT ALL — neither
//!      as a match nor as a miss. Now it counts like the others, and the plural
//!      forms come ready from the vendored catalog (`numerus` messages).
//!  10. Our own dictionary of vendored QML labels held 135 strings out of the
//!      vendor's 486 — the buyer saw four new pages in English, and on 55 shared
//!      strings our text diverged from the original. There is now ONE source of
//!      translation — the vendored `ui/i18n/waywallen_ru.ts` ([`VENDOR_TS_REL`]),
//!      vendored with the same `git diff` as `ui/qml`.
//!
//! A preserved QUIRK of the original (must not be changed): in `fields` and
//! `status` the keys `label`/`description`/`placeholder`/`group`/`value` are
//! ALWAYS set, even if they were not in the input (an empty string appears). The
//! vendored QML distinguishes `undefined` from `""` (`text !== ""`), so a "fix"
//! here would break the page — the behavior is preserved byte for byte.

use crate::i18n::{n_, tr};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

// ============================================================================
// Translation tables (order = as in the Python dictionaries, it matters for TAGS_BACK)
// ============================================================================
//
// ⭐ The value of each pair is the English msgid of the Rexon catalog (`n_`),
// not a ready label: the label in the interface language is given by [`tr`] at
// the place of display (the Russian is word for word the former one, in
// `po/ru.po`). The key stays what the source sends and is not subject to
// translation.

/// `SOURCE_NAMES` — source names.
pub const SOURCE_NAMES: &[(&str, &str)] = &[
    ("wallpaper_engine", n_("Steam Workshop")),
    ("wallhaven", n_("Wallhaven")),
];

/// `WP_TYPES` — the wallpaper type.
///
/// The key is a technical string of the engine (it also goes into the thumbnail
/// request and into the type filter), the value is the label for the buyer.
/// ONLY the display is translated: [`wp_type_label`] is called by the places of
/// output, the string itself is substituted nowhere.
pub const WP_TYPES: &[(&str, &str)] = &[
    ("scene", n_("Scene")),
    ("video", n_("Video")),
    ("web", n_("Web")),
    ("image", n_("Image")),
    ("application", n_("Application")),
];

/// `SORTS` — catalog sort orders.
pub const SORTS: &[(&str, &str)] = &[
    // Steam Workshop
    ("trend_day", n_("Trending (Today)")),
    ("trend_week", n_("Trending (Week)")),
    ("trend_month", n_("Trending (Month)")),
    ("trend_3months", n_("Trending (3 Months)")),
    ("trend_6months", n_("Trending (6 Months)")),
    ("trend_year", n_("Trending (Year)")),
    ("recent", n_("Recently Added")),
    ("most_subscribed", n_("Most Subscribed")),
    ("top_rated", n_("Top Rated")),
    // our own display modes on top of the Workshop catalog
    ("rexon_my_items", n_("My Subscriptions")),
    ("rexon_collections", n_("Collections")),
    // Wallhaven
    ("trend", n_("Trending")),
    ("popular", n_("Popular")),
];

/// `FILTER_TITLES` — filter names.
pub const FILTER_TITLES: &[(&str, &str)] = &[
    ("type", n_("Type")),
    ("questionable", n_("12+ (Questionable)")),
    ("mature", n_("18+ (Mature)")),
    ("miscellaneous", n_("Features")),
    ("genre", n_("Genre")),
    ("resolution", n_("Resolution")),
    ("topics", n_("Topics")),
];

/// `FILTER_DESCRIPTIONS` — explanations of the filters.
pub const FILTER_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "questionable",
        n_("Show wallpapers marked “12+ (Questionable)”."),
    ),
    ("mature", n_("Show wallpapers marked 18+.")),
];

/// `FILTER_CONFIRMATIONS` — the question before enabling a filter.
pub const FILTER_CONFIRMATIONS: &[(&str, &str)] = &[(
    "mature",
    n_("This shows wallpapers marked 18+. Turn it on only if you are an adult. Continue?"),
)];

/// `TAGS` — filter values (they are also the request tags).
///
/// The key is what goes to the source; the value is what the buyer sees.
/// Numeric resolutions (1920 x 1080) are not translated and are not needed here.
///
/// Warning: the order of entries matters for the reverse map: Wallhaven calls the
/// genre `CG`, the Workshop `CGI`, and both have the same label. Python built the
/// reverse map with a dict comprehension, where the LAST pair wins — the same
/// here, see [`tag_back`].
pub const TAGS: &[(&str, &str)] = &[
    // wallpaper type
    ("Scene", n_("Scene")),
    ("Video", n_("Video")),
    ("Web", n_("Web")),
    ("Application", n_("Application")),
    // features
    ("Approved", n_("Approved")),
    ("Audio responsive", n_("Audio responsive")),
    ("3D", n_("3D")),
    ("Customizable", n_("Customizable")),
    ("Puppet Warp", n_("Puppet Warp")),
    ("HDR", n_("HDR")),
    ("Media Integration", n_("Media Integration")),
    ("User Shortcut", n_("User Shortcut")),
    ("Video Texture", n_("Video Texture")),
    ("No Animation", n_("No Animation")),
    // what is published in the Workshop besides the wallpapers themselves
    ("Wallpaper", n_("Wallpaper")),
    ("Preset", n_("Preset")),
    ("Asset", n_("Asset")),
    ("Asset Pack", n_("Asset Pack")),
    // age ratings (they are also the values of the rating filters)
    ("Everyone", n_("0+ (Everyone)")),
    ("Questionable", n_("12+ (Questionable)")),
    ("Mature", n_("18+ (Mature)")),
    // genres
    ("Abstract", n_("Abstract")),
    ("Animal", n_("Animal")),
    ("Anime", n_("Anime")),
    ("Cartoon", n_("Cartoon")),
    ("CG", n_("CGI")),
    ("CGI", n_("CGI")),
    ("Cyberpunk", n_("Cyberpunk")),
    ("Fantasy", n_("Fantasy")),
    ("Game", n_("Game")),
    ("Girls", n_("Girls")),
    ("Guys", n_("Guys")),
    ("Landscape", n_("Landscape")),
    ("Medieval", n_("Medieval")),
    ("Memes", n_("Memes")),
    ("MMD", n_("MikuMikuDance (animation)")),
    ("Music", n_("Music")),
    ("Nature", n_("Nature")),
    ("Pixel art", n_("Pixel art")),
    ("Relaxing", n_("Relaxing")),
    ("Retro", n_("Retro")),
    ("Sci-Fi", n_("Sci-Fi")),
    ("Sports", n_("Sports")),
    ("Technology", n_("Technology")),
    ("Television", n_("Television")),
    ("Vehicle", n_("Vehicle")),
    ("Unspecified", n_("Unspecified genre")),
    // resolutions
    ("Standard Definition", n_("Standard Definition")),
    ("Ultrawide Standard Definition", n_("Ultrawide Standard Definition")),
    ("Ultrawide 2560 x 1080", n_("Ultrawide 2560 x 1080")),
    ("Ultrawide 3440 x 1440", n_("Ultrawide 3440 x 1440")),
    ("Dual Standard Definition", n_("Dual Standard Definition")),
    ("Dual 3840 x 1080", n_("Dual 3840 x 1080")),
    ("Dual 5120 x 1440", n_("Dual 5120 x 1440")),
    ("Dual 7680 x 2160", n_("Dual 7680 x 2160")),
    ("Triple Standard Definition", n_("Triple Standard Definition")),
    ("Triple 4096 x 768", n_("Triple 4096 x 768")),
    ("Triple 5760 x 1080", n_("Triple 5760 x 1080")),
    ("Triple 7680 x 1440", n_("Triple 7680 x 1440")),
    ("Triple 11520 x 2160", n_("Triple 11520 x 2160")),
    ("Portrait Standard Definition", n_("Portrait Standard Definition")),
    ("Portrait 720 x 1280", n_("Portrait 720 x 1280")),
    ("Portrait 1080 x 1920", n_("Portrait 1080 x 1920")),
    ("Portrait 1440 x 2560", n_("Portrait 1440 x 2560")),
    ("Portrait 2160 x 3840", n_("Portrait 2160 x 3840")),
    ("Other resolution", n_("Other resolution")),
    ("Dynamic resolution", n_("Dynamic resolution")),
    ("Oversized", n_("Larger than the screen")),
];

/// `ACTION_LABELS` — the plugins' login buttons.
pub const ACTION_LABELS: &[(&str, &str)] = &[
    ("Log in to Steam", n_("Log in to Steam")),
    ("Sign out", n_("Sign out")),
    ("Log in to Wallhaven", n_("Log in to Wallhaven")),
    ("Remove API key", n_("Remove API key")),
    ("API key", n_("API key")),
];

/// `TEXTS` — groups, status lines and explanations of the plugins.
pub const TEXTS: &[(&str, &str)] = &[
    // groups and status lines
    ("Steam account", n_("Steam account")),
    ("Wallhaven account", n_("Wallhaven account")),
    ("Status", n_("Status")),
    ("Not signed in", n_("Not signed in")),
    ("Guest mode", n_("Guest mode")),
    ("Signed in", n_("Signed in")),
    // explanations of the plugins
    (
        "Waywallen only manages Workshop subscriptions and does not download \
         wallpapers. Keep the Steam desktop client running to download \
         subscribed items.",
        n_("The Steam client downloads the wallpaper files itself — keep it running, and subscribed wallpapers will appear in the list automatically."),
    ),
    (
        "Optional. Use an API key from your Wallhaven account settings. Do not \
         enter your Wallhaven password.",
        n_("Optional. Paste the API key from your Wallhaven account settings. No password is needed."),
    ),
    (
        "Copy the API key from Wallhaven Account Settings.",
        n_("The API key is in your Wallhaven account settings."),
    ),
    ("Wallhaven API key", n_("Wallhaven API key")),
];

/// Labels of the vendored QML itself — the VENDORED catalog, not our dictionary.
///
/// The pages' strings are embedded in `.qml` via `qsTr()`, and waywallen itself
/// already has a Russian translation for them: `ui/i18n/waywallen_ru.ts`. It is
/// vendored together with `ui/qml` from the same tag ([`VENDOR_TS_REL`]), and we
/// only compile it into `.qm`.
///
/// Warning: there is NO LONGER a dictionary of our own here. It held 135 strings
/// out of the vendor's 486 — the buyer saw the new pages ("Monitors", "Status",
/// "Settings", "About") in English, and on 55 shared strings our text diverged
/// from the vendor's («Открыть страницу в Steam» where the original has "Open
/// web page"), which is exactly what the parity rule forbids. A translation of a
/// foreign program's labels has ONE proper place — its own catalog: then it
/// reaches all its users, not just us.
pub const VENDOR_TS_REL: &str = "waywallen/ui/i18n/waywallen_ru.ts";

// ============================================================================
// Lookup in the tables
// ============================================================================

/// `dict.get(key)` for a slice table (keys in our tables are unique, except for
/// the stipulated case of the reverse map — see [`tag_back`]).
fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Put our KEYED translation into the `PluginMessage` map next to the `msgid`.
///
/// Since waywallen 0.3.7 the vendored QML shows not a string field but
/// `*Text: {pluginId, msgid}` passed through `PluginTranslations`. Our tables of
/// sort orders, filter headings and source names are KEYED (`trend_day`,
/// `mature`, `wallpaper_engine`), and `PluginMessage` has no key. So we put the
/// label computed here into the same map as the `rexonFallback` field:
/// `plugin_l10n::resolve` will take it as the second item — after the plugin's
/// own catalog and before the bare `msgid`.
///
/// The field is OURS and is not read by the vendored QML: an extra key in the map
/// is harmless, while substituting the `msgid` itself would break the lookup in
/// the plugin's catalog.
///
/// Does nothing when there is no translation (`ru == original`) or there is no
/// message map in the description at all (a daemon older than 0.3.7).
fn put_fallback(item: &mut Map<String, Value>, key: &str, ru: &str, original: &str) {
    if ru == original || ru.is_empty() {
        return;
    }
    let Some(Value::Object(message)) = item.get_mut(key) else {
        return;
    };
    message.insert("rexonFallback".into(), Value::String(ru.to_string()));
}

/// A label from the table in the interface language (`None` — the key is not in
/// the table).
fn lookup_tr(table: &[(&'static str, &'static str)], key: &str) -> Option<String> {
    lookup(table, key).map(tr)
}

/// Wallpaper type → label for the buyer ("scene" → "Scene" / «Сцена»).
///
/// Case and surrounding spaces do not matter; an unknown type is returned AS IT
/// WAS (neither trimmed nor lower-cased) — as in Python.
pub fn wp_type_label(value: &str) -> String {
    let key = value.trim().to_lowercase();
    lookup_tr(WP_TYPES, &key).unwrap_or_else(|| value.to_string())
}

/// Source tag → label for the buyer.
pub fn tag_label(value: &str) -> String {
    lookup_tr(TAGS, value).unwrap_or_else(|| value.to_string())
}

/// [`tag_label`] over a list.
pub fn tag_labels<S: AsRef<str>>(values: &[S]) -> Vec<String> {
    values
        .iter()
        .map(|v| tag_label(v.as_ref()))
        .collect()
}

/// Label → original tag by the SHARED map (`_TAGS_BACK`).
///
/// Python built it with the dict comprehension `{ru: en for en, ru in TAGS}`,
/// where with a repeated label the LAST pair wins — so we search from the end
/// («Компьютерная графика», "Computer graphics" → `CGI`, not `CG`).
pub fn tag_back(label: &str) -> Option<&'static str> {
    // The label is compared TRANSLATED — the one the buyer saw.
    TAGS.iter()
        .rev()
        .find(|(_, msgid)| tr(msgid) == label)
        .map(|(tag, _)| *tag)
}

/// Name of a sort order; `fallback` — the vendored label (`s["label"]`).
pub fn sort_label(key: &str, fallback: &str) -> String {
    lookup_tr(SORTS, key).unwrap_or_else(|| fallback.to_string())
}

/// Source name by its `id` (None — the source is unknown to us).
pub fn source_name(source_id: &str) -> Option<String> {
    lookup_tr(SOURCE_NAMES, source_id)
}

/// `_text` — a plugin's status line/button: first [`TEXTS`], then
/// [`ACTION_LABELS`], otherwise as is.
pub fn text(value: &str) -> String {
    lookup_tr(TEXTS, value)
        .or_else(|| lookup_tr(ACTION_LABELS, value))
        .unwrap_or_else(|| value.to_string())
}

// ============================================================================
// Translating a source description
// ============================================================================

/// An error translating a source description.
///
/// In Python a non-dictionary raised `TypeError` (`'NoneType' object is not
/// iterable` / `object is not iterable`) — here the same case is expressed by a
/// type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L10nError {
    /// The source description is not a JSON object.
    NotAnObject,
}

/// A translator of source descriptions with tag back-translation maps.
///
/// The back-translation map is kept SEPARATELY for each source: it is filled by
/// [`Self::translate_source`] with the values the source actually returned. The
/// reason is a live defect: Wallhaven calls the genre `CG`, the Workshop `CGI`,
/// and both have the same label («Компьютерная графика», "Computer graphics").
/// One shared reverse map returned the foreign `CGI` for Wallhaven, and the
/// genre filter silently did not work. The shared map remains a fallback, but
/// first we ask the source the request goes to (rule #12 — fix the mechanism,
/// not a particular pair of tags).
#[derive(Debug, Default, Clone)]
pub struct Localizer {
    back_by_source: BTreeMap<String, BTreeMap<String, String>>,
}

impl Localizer {
    /// An empty translator (`translate_source` will fill the source maps).
    pub fn new() -> Self {
        Self::default()
    }

    /// The "label → original tag" map of a particular source (for checks).
    pub fn source_back_map(&self, source_id: &str) -> Option<&BTreeMap<String, String>> {
        self.back_by_source.get(source_id)
    }

    /// Filter labels → values that THIS source understands.
    ///
    /// First we look at the source's own map (built by [`Self::translate_source`]
    /// from its own values), then at the shared one. That way two sources can have
    /// different original tags under one Russian label — see `CG`/`CGI` above.
    ///
    /// Warning — rule #16 (protective): Python took the value from the source's map
    /// via `or`, and an empty string fell through to the shared map. Here the
    /// PRESENCE of the key is checked — see the note in the module header.
    pub fn untranslate_tags<S: AsRef<str>>(&self, values: &[S], source_id: &str) -> Vec<String> {
        let back = self.back_by_source.get(source_id);
        values
            .iter()
            .map(|v| {
                let v = v.as_ref();
                back.and_then(|m| m.get(v))
                    .cloned()
                    .or_else(|| tag_back(v).map(str::to_string))
                    .unwrap_or_else(|| v.to_string())
            })
            .collect()
    }

    /// A copy of the source description with Russian labels.
    ///
    /// Only what the buyer sees is translated. Identifiers (the source `id`, the
    /// `id`s of filters and actions, sort keys) stay untouched — both the vendored
    /// QML and the daemon go by them.
    ///
    /// `filters`/`actions` elements that are not objects are silently skipped (in
    /// Python they raised `AttributeError`): the daemon does not send such things,
    /// and there is no reason to break the whole tab because of them.
    pub fn translate_source(&mut self, info: &Value) -> Result<Value, L10nError> {
        let info = info.as_object().ok_or(L10nError::NotAnObject)?;
        let mut out = info.clone();

        let source_id = str_of(info.get("id"));
        if let Some(name) = source_name(&source_id) {
            let original = str_of(info.get("displayName"));
            put_fallback(&mut out, "displayNameText", &name, &original);
            out.insert("displayName".into(), Value::String(name));
        }

        // ── sort orders ─────────────────────────────────────────────────
        //
        // Warning — rule #16: Python rebuilt the element from two fields and lost
        // all other keys (unlike filters/actions, which are copied entirely).
        // Here key/label go first — the order as in the original — and the
        // other input keys are preserved after them.
        let sorts: Vec<Value> = array_of(info.get("sorts"))
            .iter()
            .map(|s| {
                let src = s.as_object();
                let key = src.map(|m| str_of(m.get("key"))).unwrap_or_default();
                let label = src.map(|m| str_of(m.get("label"))).unwrap_or_default();
                let mut item = Map::new();
                item.insert("key".into(), Value::String(key.clone()));
                let ru = sort_label(&key, &label);
                item.insert("label".into(), Value::String(ru.clone()));
                if let Some(src) = src {
                    for (k, v) in src {
                        if k != "key" && k != "label" {
                            item.insert(k.clone(), v.clone());
                        }
                    }
                }
                put_fallback(&mut item, "labelText", &ru, &label);
                Value::Object(item)
            })
            .collect();
        out.insert("sorts".into(), Value::Array(sorts));

        out.insert(
            "tags".into(),
            Value::Array(
                str_list(info.get("tags"))
                    .iter()
                    .map(|t| Value::String(tag_label(t)))
                    .collect(),
            ),
        );

        // Warning — rule #16: the source's map is rebuilt anew. In Python it
        // accumulated forever (`setdefault`), and after a plugin update it kept
        // labels of tags that had disappeared.
        let mut back: BTreeMap<String, String> = BTreeMap::new();

        // ── filters ─────────────────────────────────────────────────────
        let mut filters: Vec<Value> = Vec::new();
        for f in array_of(info.get("filters")) {
            let src = match f.as_object() {
                Some(m) => m,
                None => continue,
            };
            let fid = str_of(src.get("id"));
            let mut item = src.clone();
            let title = str_of(src.get("title"));
            let title_ru = lookup_tr(FILTER_TITLES, &fid).unwrap_or_else(|| title.clone());
            item.insert("title".into(), Value::String(title_ru.clone()));
            put_fallback(&mut item, "titleText", &title_ru, &title);
            let description = str_of(src.get("description"));
            if !description.is_empty() {
                let description_ru =
                    lookup_tr(FILTER_DESCRIPTIONS, &fid).unwrap_or_else(|| text(&description));
                item.insert("description".into(), Value::String(description_ru.clone()));
                put_fallback(&mut item, "descriptionText", &description_ru, &description);
            }
            let confirmation = str_of(src.get("confirmation"));
            if !confirmation.is_empty() {
                let confirmation_ru =
                    lookup_tr(FILTER_CONFIRMATIONS, &fid).unwrap_or_else(|| text(&confirmation));
                item.insert("confirmation".into(), Value::String(confirmation_ru.clone()));
                put_fallback(&mut item, "confirmationText", &confirmation_ru, &confirmation);
            }
            // ── filter options ──────────────────────────────────────────
            //
            // ⭐ waywallen 0.3.8 separated the technical value from the label:
            // an option comes as a triple `{value, label, labelText}`, `value`
            // goes into the request, the label is shown to the person. Before
            // that there was a single field (`repeated string values`), and the
            // label had to be substituted right in it and translated back before
            // sending (the `back_by_source` map). Now ONLY the label is
            // substituted: the value stays what the source understands.
            let mut options: Vec<Value> = Vec::new();
            for opt in array_of(src.get("options")) {
                let Some(o) = opt.as_object() else { continue };
                let mut out_opt = o.clone();
                let value = str_of(o.get("value"));
                let original = {
                    let label = str_of(o.get("label"));
                    if label.is_empty() { value.clone() } else { label }
                };
                // The tag dictionary is keyed by the VALUE (`Anime`), not by the
                // label: the plugin's label differs between sources, the value is
                // shared.
                let shown = lookup_tr(TAGS, &value).unwrap_or_else(|| original.clone());
                // The back-translation map stays: it picks up a choice saved before
                // the move to 0.3.8 (there the page state holds labels, not values).
                back.insert(shown.clone(), value.clone());
                out_opt.insert("label".into(), Value::String(shown.clone()));
                match out_opt.get("labelText") {
                    // The plugin's label map: our translation goes as a fallback.
                    Some(Value::Object(_)) => {
                        put_fallback(&mut out_opt, "labelText", &shown, &original)
                    }
                    // A fallback string (the plugin has no label) — we replace it.
                    Some(_) if shown != original => {
                        out_opt.insert("labelText".into(), Value::String(shown.clone()));
                    }
                    _ => {}
                }
                options.push(Value::Object(out_opt));
            }
            item.insert("options".into(), Value::Array(options));
            filters.push(Value::Object(item));
        }
        out.insert("filters".into(), Value::Array(filters));
        self.back_by_source.insert(source_id, back);

        // ── actions (login buttons and source settings) ─────────────────
        let mut actions: Vec<Value> = Vec::new();
        for a in array_of(info.get("actions")) {
            let src = match a.as_object() {
                Some(m) => m,
                None => continue,
            };
            let mut item = src.clone();
            for key in [
                "label",
                "group",
                "description",
                "browseDescription",
                "browseButtonLabel",
            ] {
                let value = str_of(src.get(key));
                if !value.is_empty() {
                    item.insert(key.into(), Value::String(text(&value)));
                }
            }
            // The original's quirk is PRESERVED: three keys are always set, even
            // if they were absent — the vendored QML distinguishes undefined from "".
            item.insert(
                "fields".into(),
                Value::Array(
                    array_of(src.get("fields"))
                        .iter()
                        .map(|fld| {
                            let mut field = fld.as_object().cloned().unwrap_or_default();
                            for key in ["label", "description", "placeholder"] {
                                let value = str_of(field.get(key));
                                field.insert(key.into(), Value::String(text(&value)));
                            }
                            Value::Object(field)
                        })
                        .collect(),
                ),
            );
            actions.push(Value::Object(item));
        }
        out.insert("actions".into(), Value::Array(actions));

        // ── status lines ────────────────────────────────────────────────
        out.insert(
            "status".into(),
            Value::Array(
                array_of(info.get("status"))
                    .iter()
                    .map(|row| {
                        let mut item = row.as_object().cloned().unwrap_or_default();
                        for key in ["label", "group", "value"] {
                            let value = str_of(item.get(key));
                            item.insert(key.into(), Value::String(text(&value)));
                        }
                        Value::Object(item)
                    })
                    .collect(),
            ),
        );

        let hint = str_of(info.get("remoteHint"));
        if !hint.is_empty() {
            out.insert("remoteHint".into(), Value::String(text(&hint)));
        }
        Ok(Value::Object(out))
    }
}

/// `str(value or "")` for a field of a JSON object: a string as is, absence and
/// other types — an empty string (all translatable fields in a source
/// description are strings, `_source_info` assembles them from protobuf).
fn str_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

/// `info.get(key) or []` — an array or empty.
fn array_of(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// An array of strings (non-string elements are dropped — in a source
/// description `tags`/`values` are declared `repeated string` by the protocol).
fn str_list(v: Option<&Value>) -> Vec<String> {
    array_of(v)
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

// ============================================================================
// Building the translation catalog (.ts → lrelease → .qm)
// ============================================================================

/// Name of the built catalog next to the UI module.
pub const QM_FILE_NAME: &str = "waywallen_ru.qm";

/// Directory of the vendored translation catalogs (`.ts`) relative to the UI
/// module. Since 0.5.0 the upstream has two of them — `waywallen_ru.ts` and
/// `waywallen_de.ts`, and each is built into its own `waywallen_<lang>.qm` next
/// to the module ([`qm_file_name`]): then a German Rexon sees the vendored pages
/// in German rather than in the source English strings.
pub const VENDOR_I18N_REL: &str = "waywallen/ui/i18n";

/// Languages of the vendored catalogs by the file names `waywallen_<lang>.ts`,
/// by code, without duplicates.
pub fn vendor_catalog_languages<S: AsRef<str>>(file_names: &[S]) -> Vec<String> {
    let mut codes: BTreeSet<String> = BTreeSet::new();
    for name in file_names {
        let code = name
            .as_ref()
            .strip_prefix(QM_PREFIX)
            .and_then(|rest| rest.strip_suffix(".ts"))
            .filter(|code| !code.is_empty());
        if let Some(code) = code {
            codes.insert(code.to_string());
        }
    }
    codes.into_iter().collect()
}

/// Path of the vendored translation catalog of language `code` relative to the
/// UI module.
pub fn vendor_ts_path_for(module_dir: &str, code: &str) -> String {
    join(module_dir, &format!("{VENDOR_I18N_REL}/{QM_PREFIX}{code}.ts"))
}

/// Path of the built catalog of language `code` next to the UI module.
pub fn qm_path_for(module_dir: &str, code: &str) -> String {
    join(module_dir, &qm_file_name(code))
}

/// Directory of the vendored QML relative to the UI module.
pub const QML_DIR_REL: &str = "waywallen/ui/qml";

/// The hard-coded `lrelease` path from the original (Arch/CachyOS).
pub const DEFAULT_LRELEASE: &str = "/usr/lib/qt6/bin/lrelease";

/// Warning — rule #16: `lrelease` candidates in decreasing order of preference —
/// the executor takes the first one found via `which`. In Python the path was
/// hard-coded for Arch, and on any other distribution the catalog build failed
/// with `FileNotFoundError`. The hard-coded path remains the last fallback.
pub const LRELEASE_CANDIDATES: &[&str] = &[
    "lrelease-qt6",
    "lrelease6",
    "lrelease",
    DEFAULT_LRELEASE,
];

/// Directory of the UI module inside the `rexon/` package.
///
/// The `.qm` catalogs lie next to `remote_l10n.py`, while the crate knows only
/// the package directory (Python reports it when loading the `.so`,
/// `we_env::pkg_dir`). The core holds the path between them — there must be no
/// second copy of it in the bridge.
pub const UI_MODULE_REL: &str = "qt_wallpaper_engine/qml_ui";

/// Directory of the UI module from the `rexon/` package directory.
pub fn ui_module_dir(pkg_dir: &str) -> String {
    join(pkg_dir, UI_MODULE_REL)
}

/// Path of the built catalog next to the UI module (`QM_PATH`).
pub fn qm_path(module_dir: &str) -> String {
    join(module_dir, QM_FILE_NAME)
}

/// The default vendored QML directory.
pub fn default_qml_dir(module_dir: &str) -> String {
    join(module_dir, QML_DIR_REL)
}

/// Path of the vendored translation catalog relative to the UI module.
pub fn vendor_ts_path(module_dir: &str) -> String {
    join(module_dir, VENDOR_TS_REL)
}

/// The catalog build plan: what to compile and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QmBuildPlan {
    /// The catalog source is the vendored `.ts`. We do not write it, only read it:
    /// it comes with a vendor update together with `ui/qml`.
    pub ts_path: String,
    /// Where `lrelease` will put the compiled catalog.
    pub qm_path: String,
    /// The compilation command (`check=True`: a non-zero code is an error).
    pub lrelease_argv: Vec<String>,
    /// Warning: strings the vendored QML hands out for translation but for which
    /// there is no vendored translation — the buyer will see them in English (rule
    /// #13). Non-zero means the catalog has fallen behind `ui/qml`: this is fixed
    /// upstream, at waywallen, not with a crutch on our side.
    pub untranslated: Vec<String>,
    /// Warning: the reverse skew: the catalog has a string that is no longer in
    /// `ui/qml`. Harmless by itself (an extra entry in the `.qm`), but it means the
    /// vendored `qml` and `i18n` were vendored from different commits.
    pub stale: Vec<String>,
}

/// All strings the vendored QML hands out for translation — the first arguments
/// of all `qsTr` calls in the tree.
///
/// Warning — rule #16: both forms are recognized, `qsTr("…")` and `qsTr('…')` —
/// QML allows either quote, while Python saw only double ones; a call with a
/// counter `qsTr("…", "", n)`, which Python did not see at all; and escapes
/// inside the string. The last one is not a trifle: the vendor writes
/// `qsTr("Deleted \"%1\"")` and `qsTr("\nRelated display: #%1")`, and in the
/// catalog these strings are stored already UNESCAPED — without unescaping they
/// would not match any entry, and nine live labels would count as untranslated.
///
/// ⭐ And the second translation function — `qsTranslate("Context", "…")`: its
/// string is the SECOND argument. The 0.5.0 markup calls it for the first time
/// (`WallpaperFilterRules.qml`, moved out of the filter dialog, keeps the
/// previous context `WallpaperFilterDialog`: "Group %1", "AND", "OR"), and
/// without parsing it three live labels counted as extra catalog entries.
pub fn vendor_sources<P: AsRef<str>, T: AsRef<str>>(files: &[(P, T)]) -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for (_, text) in files {
        let text = text.as_ref();
        for (at, _) in text.match_indices("qsTr(") {
            if !starts_word(text, at) {
                continue;
            }
            if let Some(literal) = call_literal(&text[at + "qsTr(".len()..]) {
                found.insert(literal);
            }
        }
        for (at, _) in text.match_indices("qsTranslate(") {
            if !starts_word(text, at) {
                continue;
            }
            let rest = &text[at + "qsTranslate(".len()..];
            let Some((_context, tail)) = leading_literal(rest) else {
                continue;
            };
            let Some(tail) = tail.trim_start().strip_prefix(',') else {
                continue;
            };
            if let Some(literal) = call_literal(tail.trim_start()) {
                found.insert(literal);
            }
        }
    }
    found.into_iter().collect()
}

/// The function name stands as a separate word: `myqsTr(` is someone else's function.
fn starts_word(text: &str, at: usize) -> bool {
    !text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// A string literal at the start of `rest` and the remainder after it.
fn leading_literal(rest: &str) -> Option<(String, &str)> {
    let quote = match rest.chars().next() {
        Some(q @ ('"' | '\'')) => q,
        _ => return None,
    };
    js_string(&rest[quote.len_utf8()..], quote)
}

/// The literal is a call argument: it is followed by a closing parenthesis or
/// the next argument (a counter, a comment). Otherwise it is an expression
/// (`qsTr(a + b)`), and a string for the catalog cannot be extracted from it.
fn call_literal(rest: &str) -> Option<String> {
    let (literal, tail) = leading_literal(rest)?;
    matches!(tail.chars().next(), Some(')' | ',')).then_some(literal)
}

/// Parse a JS string literal up to the closing quote `quote`: return its value
/// (with escapes expanded) and the rest of the text after the quote.
///
/// An unknown escape (`\d`) gives the character itself — as in JS for anything
/// not in the table; the vendor does not use hexadecimal forms (`\xNN`,
/// `\uNNNN`) in `qsTr`, and parsing them would be guesswork.
fn js_string(body: &str, quote: char) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == quote {
            return Some((out, &body[i + c.len_utf8()..]));
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let (_, escaped) = chars.next()?;
        out.push(match escaped {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            other => other,
        });
    }
    None
}

/// Strings for which the catalog HAS a ready translation.
///
/// Parsing is deliberately crude — by the `<source>` and `<translation>` tags,
/// without an XML parser: the catalog is written by the vendor's `lupdate`, its
/// format is always the same, and an extra dependency for four tags does not pay
/// off. An unfinished translation (`type="unfinished"`) does NOT count as a
/// translation: Qt will return the source string, and the buyer will see
/// English.
pub fn catalog_translated(ts: &str) -> BTreeSet<String> {
    let mut done = BTreeSet::new();
    let mut pending: Option<String> = None;
    for (at, _) in ts.match_indices('<') {
        let rest = &ts[at..];
        if let Some(body) = tag_body(rest, "source") {
            pending = Some(xml_unescape(body));
        } else if rest.starts_with("<translation") {
            let head_end = match rest.find('>') {
                Some(i) => i,
                None => continue,
            };
            let unfinished = rest[..head_end].contains("unfinished");
            if let (Some(source), false) = (pending.take(), unfinished) {
                done.insert(source);
            }
        }
    }
    done
}

/// The body of a simple tag `<name>…</name>` at the start of the slice.
fn tag_body<'a>(rest: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let body = rest.strip_prefix(&open)?;
    let end = body.find(&close)?;
    Some(&body[..end])
}

/// The inverse of `xml.sax.saxutils.escape`; `&amp;` is handled last, so that
/// `&amp;lt;` stays the text `&lt;` instead of turning into `<`.
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Vendored strings without a vendored translation — the buyer sees them in
/// English (rule #13). They appear when `ui/qml` was updated but `ui/i18n` was
/// not.
pub fn untranslated_vendor_sources<P: AsRef<str>, T: AsRef<str>>(
    files: &[(P, T)],
    ts: &str,
) -> Vec<String> {
    let translated = catalog_translated(ts);
    vendor_sources(files)
        .into_iter()
        .filter(|source| !translated.contains(source))
        .collect()
}

/// Build `waywallen_ru.qm` from the VENDORED catalog.
///
/// `files` — "path → text" pairs of all the vendor's `.qml` (in the executor
/// this is `glob("**/*.qml", recursive=True)` + reading), `ts` — the contents of
/// the vendored `.ts`. The files are needed not for the build but for the check:
/// the plan also reports where the catalog and `ui/qml` diverged. `lrelease` is
/// Qt's catalog compiler: it is on the developer's machine, the buyer does not
/// need it — a ready `.qm` goes into the build.
pub fn plan_build_qm<P: AsRef<str>, T: AsRef<str>>(
    files: &[(P, T)],
    ts: &str,
    ts_path: &str,
    qm_path: &str,
    lrelease: &str,
) -> QmBuildPlan {
    let translated = catalog_translated(ts);
    let sources: BTreeSet<String> = vendor_sources(files).into_iter().collect();
    QmBuildPlan {
        lrelease_argv: vec![
            lrelease.to_string(),
            ts_path.to_string(),
            "-qm".to_string(),
            qm_path.to_string(),
        ],
        untranslated: sources.difference(&translated).cloned().collect(),
        stale: translated.difference(&sources).cloned().collect(),
        ts_path: ts_path.to_string(),
        qm_path: qm_path.to_string(),
    }
}

// ============================================================================
// Interface language of the vendored pages
// ============================================================================
//
// Port of waywallen's `ui/src/ui_language.cpp`. The vendor's settings page has a
// "Language" combo box, and `App` gives it three members —
// `availableUiLanguages`, `uiLanguage`, `setUiLanguage()`. Here is the pure
// part of how they work: which languages to consider available, how to bring a
// choice to a supported code and which catalog to load. The `QTranslator`
// itself, the locale labels and rebuilding the QML bindings are with the
// executor (the `cpp/rexon_we_singletons.cpp` shim).
//
// Warning: NO second language mechanism is created here: the available
// languages are exactly those `.qm` catalogs that [`plan_build_qm`] already
// builds from the vendored `.ts`. The only new thing here is that the catalog
// is no longer hard-wired to Russian, and the former "install the Russian
// translator" (`plan_install_translator`, `remote_l10n.install_translator()`) is
// removed entirely: the translator now has one owner — the `App` singleton.

/// The language choice key in `QSettings`. The same as in the original
/// (`ui_language.cpp`, `kSettingsKey`): there is one setting for the program,
/// and there is nothing for it to diverge from.
pub const UI_LANGUAGE_KEY: &str = "ui/language";

/// "As in the system" — the default value (the original's `kSystemLanguage`).
pub const UI_LANGUAGE_SYSTEM: &str = "system";

/// English. There is no catalog for it and none is needed: the source `qsTr()`
/// strings in the vendored QML are English (the original's `kEnglishLanguage`).
pub const UI_LANGUAGE_ENGLISH: &str = "en";

/// Label of the English language — its own endonym. In the original it is also
/// written as a string (`QStringLiteral("English")`) rather than taken from
/// QLocale: there is no catalog for English, and there is nothing to ask the
/// locale about.
pub const UI_LANGUAGE_ENGLISH_LABEL: &str = "English";

/// Beginning and end of the vendor's translation catalog name.
const QM_PREFIX: &str = "waywallen_";
const QM_SUFFIX: &str = ".qm";

/// Translation catalog name for a language code: `ru` → `waywallen_ru.qm`.
pub fn qm_file_name(code: &str) -> String {
    format!("{QM_PREFIX}{code}{QM_SUFFIX}")
}

/// Language code from a catalog name; `None` — the file is not ours.
///
/// The original's `discoverLanguages` branch, only there the names come from the
/// `:/i18n` resource, while ours come from the directory next to the UI module.
pub fn qm_file_language(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix(QM_PREFIX)?;
    let code = rest.strip_suffix(QM_SUFFIX)?;
    if code.is_empty() {
        return None;
    }
    Some(code.to_string())
}

/// Languages that have a built catalog next to the module.
///
/// English is dropped, as in the original: its strings are the source anyway,
/// and a catalog for it would mean translating English into English. The order
/// is by code (the original's `QDir::Name`), duplicates are removed.
pub fn ui_language_codes<S: AsRef<str>>(file_names: &[S]) -> Vec<String> {
    let mut codes: BTreeSet<String> = BTreeSet::new();
    for name in file_names {
        let Some(code) = qm_file_language(name.as_ref()) else {
            continue;
        };
        if base_language(&code) == UI_LANGUAGE_ENGLISH {
            continue;
        }
        codes.insert(code);
    }
    codes.into_iter().collect()
}

/// Base language of a locale code: `ru_RU` → `ru`, `zh-CN` → `zh`.
fn base_language(code: &str) -> String {
    let cut = code
        .find(['_', '-', '.', '@'])
        .unwrap_or(code.len());
    code[..cut].to_ascii_lowercase()
}

/// Bring a choice to a supported code; `None` — we do not have such a language.
///
/// The order is exactly as in the original's `normalizePreference`: "as in the
/// system" and English are always accepted, then an exact match with a built
/// catalog, then a match by base language (`ru_RU` takes the `ru` catalog, and
/// `zh` the `zh_CN` catalog).
pub fn normalize_ui_language(preference: &str, codes: &[String]) -> Option<String> {
    let pref = preference.trim();
    if pref.is_empty() {
        return None;
    }
    if pref == UI_LANGUAGE_SYSTEM {
        return Some(UI_LANGUAGE_SYSTEM.to_string());
    }
    let base = base_language(pref);
    if base == UI_LANGUAGE_ENGLISH {
        return Some(UI_LANGUAGE_ENGLISH.to_string());
    }
    if let Some(exact) = codes.iter().find(|code| code.as_str() == pref) {
        return Some(exact.clone());
    }
    codes
        .iter()
        .find(|code| base_language(code) == base)
        .cloned()
}

/// The decision of [`plan_ui_language`]: what to show and what to load for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiLanguagePlan {
    /// The code that will become the buyer's choice (`system`, `en`, `ru`).
    pub preference: String,
    /// The Qt locale name for `QTranslator::load` (`ru_RU`, `en`); empty — the
    /// system locale.
    pub locale_name: String,
    /// Name of the catalog next to the module; empty — there is nothing to translate
    /// with and no need to.
    pub qm_file: String,
    /// The catalog is mandatory: without it the language change counts as failed,
    /// and the vendored QML shows the toast «Не удалось изменить язык» ("Failed to
    /// change the language").
    pub required: bool,
}

/// What to do so that the vendored pages speak the chosen language.
///
/// `system_locale` — the name of the system Qt locale (`ru_RU`), only the
/// executor knows it. `codes` — the answer of [`ui_language_codes`].
///
/// `None` — the language is not supported; in the original this also means
/// "return `false` from `setUiLanguage`".
pub fn plan_ui_language(
    preference: &str,
    system_locale: &str,
    codes: &[String],
) -> Option<UiLanguagePlan> {
    let preference = normalize_ui_language(preference, codes)?;
    if preference == UI_LANGUAGE_SYSTEM {
        // "As in the system" requires no catalog: if there is no Russian
        // catalog, the pages stay English, and that is not an error.
        let qm_file = normalize_ui_language(system_locale, codes)
            .filter(|code| code != UI_LANGUAGE_SYSTEM && code != UI_LANGUAGE_ENGLISH)
            .map(|code| qm_file_name(&code))
            .unwrap_or_default();
        return Some(UiLanguagePlan {
            preference,
            locale_name: String::new(),
            qm_file,
            required: false,
        });
    }
    if preference == UI_LANGUAGE_ENGLISH {
        return Some(UiLanguagePlan {
            locale_name: preference.clone(),
            preference,
            qm_file: String::new(),
            required: false,
        });
    }
    Some(UiLanguagePlan {
        qm_file: qm_file_name(&preference),
        locale_name: preference.clone(),
        preference,
        required: true,
    })
}

// ============================================================================
// Path helpers (os.path semantics)
// ============================================================================

fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}
