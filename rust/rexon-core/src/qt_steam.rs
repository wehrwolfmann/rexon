//! Port of `rexon/qt_steam/window.py`: the «Steam» tab, its content and decisions.
//!
//! The tab glues together two showcases related only by their data source,
//! Steam:
//!
//!   * 🎬 **Startup movie at login**: a showcase of movies (Valve built-ins +
//!     ones bought in the Points Shop), choice, favorites, rotation, autostart.
//!     The splash mechanics have long lived in [`crate::session_splash`]; here
//!     is what the showcase shows: the origin badge, the source caption,
//!     filter chips, the counter line, the large card and the summary chip.
//!   * 🖼 **Steam profile backgrounds**: a grid of inventory items applied as
//!     wallpapers. Parsing of Steam responses is already ported
//!     ([`crate::steam_profile`], [`crate::steam_profile_items`]); here is the
//!     paged grid, search, the card, the «✓ Applied» mark and the state
//!     texts.
//!
//! What is NOT here: network, file system and Qt. Cache paths are built but
//! not read; request URLs are assembled but not executed: that is the job of
//! the executor (`rexon-gui/src/steam_workers.rs`).
//!
//! ⚠️ Fixes to the original (rule #16: a defect found while porting is fixed,
//! not preserved "for the sake of parity"), each marked `Fix` in place:
//!   1. the tab's cache and config paths were hard-coded as literals
//!      `~/.cache/rexon…` and `~/.config/rexon…`, bypassing `$XDG_CACHE_HOME`
//!      and `$XDG_CONFIG_HOME`: the same class already fixed in
//!      [`crate::session_splash::preview_cache_dir`] and
//!      [`crate::utils::config_home`]. With a non-standard XDG the inventory
//!      list was cached into one directory and read from another: the
//!      showcase opened empty EVERY time and waited for the network;
//!   2. the dead pair `_THUMB_W, _THUMB_H = 96, 54` (thumbnail size of the
//!      list mode, which the tab no longer has) is not ported;
//!   3. `_download_community` left a partially downloaded `<file>.part` on
//!      disk on failure; here the temporary file name is given separately
//!      ([`part_path`]) together with the instruction to remove it on any outcome;
//!   4. both `_InventoryWorker` and `_SplashMetaWorker` fetched the WHOLE
//!      public inventory each on its own (up to 20 pages of 2000 items,
//!      twice on every tab opening). Parsing here is split into "pages" and
//!      "what to take from them", so the executor downloads once and feeds
//!      both showcases ([`splash_item_meta`] and
//!      [`crate::steam_profile_items::parse_inventory_backgrounds`]).
//!
//! TRANSLATION (since 28.09.2026). Captions are English `msgid`s through
//! [`crate::i18n`]: constants became functions (`sv_pick_text()` instead of
//! `SV_PICK_TEXT`: unlike a constant, a function cannot be forgotten when
//! translating), the previous Russian text verbatim is in `po/ru.po` (snapshot
//! `tests/golden/qt_steam_ru_before.json`). The language of Steam responses
//! (tags and names of inventory items) follows the interface language,
//! [`steam_language`].
//!
//! Parity check: `rust/rexon-core/tests/qt_steam_parity.rs`.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::i18n::{self, n_, tr, trf};
use crate::session_splash;
use crate::steam_profile_items::ProfileBackgroundItem;
use crate::ui_base;
use crate::utils;
use crate::wallpaper_exec::{WallpaperExec, WpSys};

// ============================================================================
// Window and header
// ============================================================================

pub const WINDOW_TITLE: &str = "Rexon — Steam";
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 720);

pub const HEADER_TITLE: &str = "🎮 Steam";
pub const HEADER_TITLE_POINT_SIZE: i32 = 20;
/// Font size of the group title («🎬 Startup Movie at Login»).
pub const GROUP_TITLE_POINT_SIZE: i32 = 13;

/// Caption on the right of the header: where Steam was found, or what to do if it was not.
///
/// The original returns RichText with `<tt>` around the path; the markup stays
/// with the view, the core returns the pair "text, monospace path". An empty
/// second element means there is no monospace part.
pub fn header_found_text(steam_root: Option<&str>) -> (String, String) {
    match steam_root {
        Some(root) if !root.is_empty() => (
            // TRANSLATORS: followed by the path of the Steam folder; keep the
            // trailing space.
            tr("Found: "),
            root.to_string(),
        ),
        _ => (
            tr("Steam not found — install Steam to use startup movies and backgrounds"),
            String::new(),
        ),
    }
}

// ============================================================================
// Segmented subtab switcher
// ============================================================================

/// Identifier of the «Startup Movie» subtab.
pub const SEG_SPLASH: &str = "splash";
/// Identifier of the «Steam Backgrounds» subtab.
pub const SEG_BG: &str = "bg";
/// Identifier of the «Profile Showcases» subtab (cropping a picture for a showcase,
/// `steam_showcase`).
pub const SEG_SHOWCASE: &str = "showcase";
/// Identifier of the «Screensaver» subtab (the Rexon screensaver for the idle
/// screen of Steam Big Picture, `steam_screensaver`).
pub const SEG_SCREENSAVER: &str = "screensaver";

/// Set and ORDER of the switcher chips. The splash goes first: it is the
/// flagship feature of the tab, profile backgrounds are secondary.
///
/// The caption is a `msgid`; show it through [`segments`].
pub const SEGMENTS: &[(&str, &str)] = &[
    (SEG_SPLASH, n_("🎬 Startup Movie")),
    (SEG_BG, n_("🖼️ Steam Backgrounds")),
    (SEG_SHOWCASE, n_("🖼️ Profile Showcases")),
    (SEG_SCREENSAVER, n_("🌙 Screensaver")),
];

/// Switcher chips with captions in the interface language.
pub fn segments() -> Vec<(&'static str, String)> {
    SEGMENTS.iter().map(|(key, label)| (*key, tr(label))).collect()
}

/// Stack page index by subtab key (0 is the startup movie, 1 backgrounds,
/// 2 showcases, 3 the Big Picture screensaver). An unknown key means
/// backgrounds, as it was before showcases.
pub fn segment_index(key: &str) -> u32 {
    match key {
        SEG_SPLASH => 0,
        SEG_SHOWCASE => 2,
        SEG_SCREENSAVER => 3,
        _ => 1,
    }
}

/// Whether the subtab is known (switching to a foreign key is silently ignored).
pub fn is_segment(key: &str) -> bool {
    SEGMENTS.iter().any(|(k, _)| *k == key)
}

// ============================================================================
// Section groups
// ============================================================================

pub const SPLASH_GROUP_OBJECT: &str = "steamSplashGroup";
pub fn splash_group_title() -> String {
    tr("🎬 Startup Movie at Login")
}
/// The main point of the section in one line, without jargon (rule #13).
pub fn splash_pitch() -> String {
    tr("A video at login is a Steam Deck signature. Rexon brings it to your regular computer: \
        Valve's built-in startup movies and the ones you've already bought in the Points Shop — \
        with full sound and without the desktop flashing through.")
}

pub const BG_GROUP_OBJECT: &str = "steamBgGroup";
pub fn bg_group_title() -> String {
    tr("🖼️ Steam Profile Backgrounds")
}
pub fn bg_group_desc() -> String {
    tr("Steam profile backgrounds from your inventory as desktop wallpapers: static ones as a \
        picture, animated ones as live wallpapers. Your inventory has to be public in Steam's \
        privacy settings.")
}

// ============================================================================
// Startup movie showcase: sizes and captions
// ============================================================================

/// Pause before switching the movie on hover: when the mouse is moved across
/// the whole grid, the player must not twitch on every card.
pub const SV_HOVER_DELAY_MS: u32 = 220;
/// Internal key of the «★ Favorites» chip (the other chips are group names).
pub const SV_GROUP_FAV: &str = "\u{0}favs";
/// For this long the preview freezes while the movie is shown full screen
/// (lifted earlier when focus returns to the window).
pub const SV_TEST_PAUSE_S: f64 = 25.0;
/// Minimum interval between unpacking preview frames (~15 fps): a frame
/// arrives at the movie's full resolution and costs ~8 MB per unpack.
pub const SV_FRAME_MIN_INTERVAL: f64 = 1.0 / 15.0;

/// Preview on a showcase card, 16:10, the native aspect ratio of the movies.
pub const SV_THUMB_W: i32 = 176;
pub const SV_THUMB_H: i32 = 110;
/// Width of a showcase card. CONSTANT: the grid engine counts columns by it.
pub const SV_CARD_W: i32 = SV_THUMB_W + 24;
pub const SV_MIN_COLS: u32 = 2;
pub const SV_MAX_COLS: u32 = 8;
/// Large preview of the chosen movie.
pub const SV_HERO_W: i32 = 400;
pub const SV_HERO_H: i32 = 250;
/// Width at which the name on the card is truncated (minus the room for the star).
pub const SV_TITLE_ELIDE_W: i32 = SV_THUMB_W - 22;

pub fn sv_search_placeholder() -> String {
    tr("🔍 Search by title, game or event…")
}
pub fn sv_pick_text() -> String {
    tr("📁 Choose File…")
}
pub fn sv_pick_tooltip() -> String {
    tr("Choose your own video (webm/mp4) from disk")
}
pub fn sv_pick_dialog_title() -> String {
    tr("Choose a Startup Movie")
}
pub fn sv_pick_filter() -> String {
    // TRANSLATORS: file dialog filter: translate only the group names ("Video", "All files"); keep the patterns in parentheses and the ";;" separator as they are.
    tr("Video (*.webm *.mp4 *.mkv);;All files (*)")
}

pub fn sv_autostart_text() -> String {
    tr("Play at login")
}
pub fn sv_autostart_tooltip() -> String {
    tr("Master switch: the movie plays right after you log in")
}
pub fn sv_rotate_text() -> String {
    tr("New movie at every login")
}
pub fn sv_rotate_tooltip() -> String {
    tr("The startup movie changes by itself. If at least two movies are starred, only those take \
        turns; otherwise all found ones do.")
}
pub fn sv_sound_text() -> String {
    tr("🔊 Sound")
}

pub fn sv_test_text() -> String {
    tr("▶️ Watch Full Screen")
}
pub fn sv_test_tooltip() -> String {
    tr("Play the movie right now — this is how it looks at login")
}
pub fn sv_test_playing() -> String {
    tr("▶️ Playing…")
}
/// Refusal to show the splash while no movie is chosen.
///
/// ⚠️ There were THREE copies of this string (counted 07.09.2026): here, in
/// [`crate::session_splash::MSG_CHOOSE_FIRST`] and in the Python host of the
/// QML tab. All three mean the same, "choose a splash first", and they must
/// not diverge: the showcase and the autostart refuse for one and the same
/// reason. One remains, the others refer here.
pub const SV_TEST_NO_ITEM: &str = crate::session_splash::MSG_CHOOSE_FIRST;

/// Refusal reason when the subsystem did not name its own.
///
/// Side actions of the showcase (autostart, full-screen show) answer with a
/// pair "it worked / why not". An empty "why" is no reason to show the
/// customer an empty status line: they pressed a button and must see an
/// answer. The widget reference did not know this case at all and printed nothing.
pub fn sv_no_reason() -> String {
    // TRANSLATORS: reason shown after a failed action when the system gave no reason of its own.
    tr("it didn't work")
}

/// Heading of the refusal to turn on the splash at login.
pub fn sv_autostart_on_failed() -> String {
    tr("❌ Couldn't turn on the startup movie at login")
}
/// Heading of the refusal to turn it off.
pub fn sv_autostart_off_failed() -> String {
    tr("❌ Couldn't fully turn off the startup movie at login")
}
/// The tail when the reason cannot be named in plain words.
pub fn sv_autostart_try_again() -> String {
    tr("Please try again.")
}
/// Reasons the customer can understand and fix themselves.
pub fn sv_cause_no_access() -> String {
    // TRANSLATORS: reason after "Couldn't turn on…: ", starts in lower case.
    tr("no permission to write to the settings folder")
}
pub fn sv_cause_no_space() -> String {
    // TRANSLATORS: reason after "Couldn't turn on…: ", starts in lower case.
    tr("the disk is full")
}
pub fn sv_cause_read_only() -> String {
    // TRANSLATORS: reason after "Couldn't turn on…: ", starts in lower case.
    tr("the settings folder is read-only")
}

/// A system file error → the reason in plain words, if it can be named.
///
/// Raw text arrives: from the Rust executor `… (os error 13)` (with the first
/// part in the system language, too), from the Python host `[Errno 13] …`.
/// The error number is the only language-independent part, so it is used to recognize it.
fn plain_file_cause(reason: &str) -> Option<String> {
    let low = reason.to_lowercase();
    let errno = |n: u32| {
        low.contains(&format!("os error {n})")) || low.contains(&format!("errno {n}]"))
    };
    if errno(13) || errno(1) || low.contains("permission denied") {
        Some(sv_cause_no_access())
    } else if errno(28) || low.contains("no space left") {
        Some(sv_cause_no_space())
    } else if errno(30) || low.contains("read-only file system") {
        Some(sv_cause_read_only())
    } else {
        None
    }
}

/// What to show the customer after the «Play at login» switch.
///
/// `on` is what they asked for (turn on or off), `reason` is the answer of
/// the side action: empty means it worked (the page rereads the checkbox
/// anyway), otherwise the refusal reason.
///
/// WHY SEPARATELY. The reason is put together by the splash route for the
/// journal, and it speaks the language of the internals: "Could not turn on:
/// Permission denied (os error 13)", "unit: …; ksplashrc: kwriteconfig6
/// failed; autostart: …". The widget tab printed this to the customer as is,
/// and the QML version did not print it at all: the checkbox silently
/// bounced back. Here it is one phrase: what failed and, if it can be named
/// in plain words, why. The details stay in the journal.
pub fn autostart_refusal_text(on: bool, reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return None;
    }
    // This reason is already said in human language and is clear without a
    // heading. Recognized by `msgid` and by the translation: the splash route
    // returns it already in the interface language (`session_splash::msg_choose_first`).
    if reason == SV_TEST_NO_ITEM || reason == tr(SV_TEST_NO_ITEM) {
        return Some(format!("❌ {}", tr(SV_TEST_NO_ITEM)));
    }
    let head = if on {
        sv_autostart_on_failed()
    } else {
        sv_autostart_off_failed()
    };
    Some(match plain_file_cause(reason) {
        Some(cause) => format!("{head}: {cause}."),
        None => format!("{head}. {}", sv_autostart_try_again()),
    })
}

/// Whether to append the person's OWN movie from the config to the showcase.
///
/// The showcase is built from a scan of Steam startup videos, while the
/// customer could choose a file from anywhere on disk. Then their choice is
/// not in the scan, and without this addition it would disappear from view,
/// even though it is exactly the one applied.
///
/// `exists` is whether the file is on disk; it is an observation of the
/// environment, and the executor brings it. A card for a path to a missing
/// file never exists: the config may have outlived the movie's removal.
pub fn needs_custom_movie(current: &str, scanned: &[String], exists: bool) -> bool {
    !current.is_empty() && exists && !scanned.iter().any(|p| p == current)
}

pub fn sv_star_on() -> String {
    // TRANSLATORS: button state: the startup movie is in favorites (click removes it).
    tr("⭐ In Favorites")
}
pub fn sv_star_off() -> String {
    tr("☆ Add to Favorites")
}
pub fn sv_star_tooltip() -> String {
    tr("Favorite movies take turns when “New movie at every login” is on")
}
pub fn sv_card_star_tooltip() -> String {
    tr("Add to favorites — “New movie at every login” takes turns among favorites")
}
pub const SV_STAR_GLYPH_ON: &str = "⭐";
pub const SV_STAR_GLYPH_OFF: &str = "☆";

pub fn sv_all_chip() -> String {
    // TRANSLATORS: filter chip: show startup movies from all sources.
    tr("All")
}
pub fn sv_fav_chip() -> String {
    tr("⭐ Favorites")
}
pub fn sv_empty_text() -> String {
    tr("No startup movies found. Steam isn't installed or the movies haven't been downloaded yet — \
        you can pick your own video with the “Choose File…” button.")
}
pub fn sv_hero_empty_title() -> String {
    tr("No startup movie selected")
}
pub fn sv_hero_preview_hint() -> String {
    tr("Preview — click the card to use this movie at login")
}
pub fn sv_rotate_need_two() -> String {
    tr("Taking turns needs at least two videos")
}

/// What to tell the customer when they toggled «New movie at every login».
///
/// Turned it on but there is nothing to rotate: say so honestly instead of
/// pretending (rule #13; that is how the widget tab answered). Turning it off
/// and a sufficient pool need no answer: the result is visible in the state
/// chip anyway. The threshold is shared with the draw at login
/// ([`session_splash::rotation_possible`]).
pub fn rotation_refusal(on: bool, favs_len: usize, items_len: usize) -> Option<String> {
    (on && !session_splash::rotation_possible(favs_len, items_len)).then(sv_rotate_need_two)
}
/// Card placeholder while the movie frame is not ready.
pub const SV_THUMB_PLACEHOLDER: &str = "🎬";

// ============================================================================
// Startup movie showcase: decisions
// ============================================================================

/// Steam item tags for one movie: what the inventory shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SplashMeta {
    /// Item name in the inventory.
    pub name: String,
    /// «Startup movie from Days Gone»: the item's `type` string.
    pub source: String,
    /// Event/sale (the `event` tag), if any.
    pub event: String,
    /// Rarity, game, item type, trade and market restrictions.
    pub labels: Vec<String>,
}

/// A string field of an object: "" if the key is missing/`null`/not a string.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `appid` of a movie record (0 if missing).
pub fn movie_appid(item: &Value) -> i64 {
    item.get("appid").and_then(Value::as_i64).unwrap_or(0)
}

/// Tags of a movie by its `appid` (empty if the inventory has not arrived yet).
fn meta_of<'a>(meta: &'a MetaMap, item: &Value) -> Option<&'a SplashMeta> {
    meta.get(&movie_appid(item))
}

/// appid → tags. `BTreeMap`, not `HashMap`: the iteration order must be
/// stable, otherwise the filter chips would reorder from run to run.
pub type MetaMap = std::collections::BTreeMap<i64, SplashMeta>;

/// Short origin badge of a movie: the event matters more than the shop.
pub fn badge_text(item: &Value, meta: &MetaMap) -> String {
    if let Some(m) = meta_of(meta, item) {
        if !m.event.is_empty() {
            return m.event.clone();
        }
    }
    let game = s(item, "game");
    if !game.is_empty() {
        return game.to_string();
    }
    s(item, "origin_label").to_string()
}

/// Badge tone: ones bought in the Points Shop are highlighted with the accent.
pub fn badge_tone(item: &Value) -> &'static str {
    if s(item, "origin") == session_splash::ORIGIN_SHOP {
        BADGE_ACCENT
    } else {
        BADGE_NEUTRAL
    }
}

/// Source line in the style of the Steam inventory: «Startup movie from X».
pub fn source_text(item: &Value, meta: &MetaMap) -> String {
    if let Some(m) = meta_of(meta, item) {
        if !m.source.is_empty() {
            return m.source.clone();
        }
    }
    let game = s(item, "game");
    if !game.is_empty() {
        // TRANSLATORS: %1 is the name of a game or a Steam sale event.
        return trf("Startup movie from %1", &[&game]);
    }
    match s(item, "origin") {
        session_splash::ORIGIN_BUILTIN => tr("Built-in Steam startup movie"),
        session_splash::ORIGIN_CUSTOM => tr("Your own video from disk"),
        _ => tr("Startup movie from the Points Shop"),
    }
}

/// Groups the movies actually have (in order of appearance).
pub fn group_keys(items: &[Value], meta: &MetaMap) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for it in items {
        let k = badge_text(it, meta);
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// Whether at least one of the movies is a favorite.
pub fn has_favorites(items: &[Value], favs: &[String]) -> bool {
    items
        .iter()
        .any(|it| favs.iter().any(|f| f == s(it, "path")))
}

/// The chosen group may have disappeared (star removed, movie gone): silently
/// go back to «All», so the showcase does not end up empty.
pub fn normalize_group_key(selected: &str, items: &[Value], favs: &[String], meta: &MetaMap) -> String {
    if selected.is_empty() {
        return String::new();
    }
    if selected == SV_GROUP_FAV {
        return if has_favorites(items, favs) {
            SV_GROUP_FAV.to_string()
        } else {
            String::new()
        };
    }
    if group_keys(items, meta).iter().any(|k| k == selected) {
        selected.to_string()
    } else {
        String::new()
    }
}

/// One chip of the "where the splash comes from" filter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupChip {
    pub key: String,
    pub label: String,
    pub checked: bool,
}

/// Filter chips for the current set of movies.
///
/// An empty answer means the filter line must NOT be shown: there are no
/// empty buttons, and if there is only one group and no favorites, there
/// simply is no choice (rule #13).
pub fn group_chips(items: &[Value], favs: &[String], meta: &MetaMap, selected: &str) -> Vec<GroupChip> {
    let keys = group_keys(items, meta);
    let favs_present = has_favorites(items, favs);
    if keys.len() < 2 && !favs_present {
        return Vec::new();
    }
    let cur = normalize_group_key(selected, items, favs, meta);
    let mut out = vec![GroupChip {
        key: String::new(),
        label: sv_all_chip().to_string(),
        checked: cur.is_empty(),
    }];
    for k in keys {
        out.push(GroupChip {
            checked: cur == k,
            label: k.clone(),
            key: k,
        });
    }
    if favs_present {
        out.push(GroupChip {
            key: SV_GROUP_FAV.to_string(),
            label: sv_fav_chip().to_string(),
            checked: cur == SV_GROUP_FAV,
        });
    }
    out
}

/// Whether the movie passes the chosen "where the splash comes from" filter.
pub fn in_group(item: &Value, key: &str, favs: &[String], meta: &MetaMap) -> bool {
    if key.is_empty() {
        return true;
    }
    if key == SV_GROUP_FAV {
        return favs.iter().any(|f| f == s(item, "path"));
    }
    badge_text(item, meta) == key
}

/// Showcase search: name, game, event. `needle` is already lowercased and
/// trimmed (like `.strip().lower()` of the original).
pub fn sv_matches(item: &Value, needle: &str, key: &str, favs: &[String], meta: &MetaMap) -> bool {
    if !in_group(item, key, favs, meta) {
        return false;
    }
    if needle.is_empty() {
        return true;
    }
    let m = meta_of(meta, item);
    let hay = [
        s(item, "title"),
        s(item, "name"),
        s(item, "game"),
        s(item, "origin_label"),
        m.map(|x| x.source.as_str()).unwrap_or(""),
        m.map(|x| x.event.as_str()).unwrap_or(""),
    ]
    .join(" ")
    .to_lowercase();
    hay.contains(needle)
}

/// Movies selected by search and filter (in the order of the source list).
pub fn sv_shown<'a>(items: &'a [Value], needle: &str, key: &str, favs: &[String], meta: &MetaMap) -> Vec<&'a Value> {
    items
        .iter()
        .filter(|it| sv_matches(it, needle, key, favs, meta))
        .collect()
}

/// The counter line under the filter chips.
pub fn sv_count_text(shown: usize, total: usize, filtered: bool) -> String {
    if total == 0 {
        sv_empty_text().to_string()
    } else if filtered {
        showing_text(shown, total)
    } else {
        trf("Startup movies available: %1", &[&total])
    }
}

/// «Showing 3 of 10»: one phrase for both showcases.
fn showing_text(shown: usize, total: usize) -> String {
    // TRANSLATORS: search result counter: %1 shown out of %2 in total.
    trf("Showing %1 of %2", &[&shown, &total])
}

/// Large card of the chosen (or hovered) movie.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HeroView {
    /// Whether there is anything to show at all.
    pub present: bool,
    pub title: String,
    /// «🎮 Startup movie from X».
    pub source: String,
    /// «Tags: …» or empty (an empty line must not be shown).
    pub labels: String,
    /// «Event: …» or empty.
    pub event: String,
    /// Caption of the star button.
    pub star_text: String,
    pub star_on: bool,
    /// The explanation "I am looking at one, but at login there will be another".
    pub status: String,
}

/// Build the large card. `shown` is what the card shows (hovered or chosen),
/// `current` is what will actually play at login.
pub fn hero(shown: Option<&Value>, current: Option<&Value>, favs: &[String], meta: &MetaMap) -> HeroView {
    let Some(item) = shown else {
        return HeroView {
            present: false,
            title: sv_hero_empty_title().to_string(),
            ..HeroView::default()
        };
    };
    let title = {
        let t = s(item, "title");
        let n = s(item, "name");
        if !t.is_empty() {
            t.to_string()
        } else if !n.is_empty() {
            n.to_string()
        } else {
            "?".to_string()
        }
    };
    let m = meta_of(meta, item);
    let labels = m.map(|x| x.labels.clone()).unwrap_or_default();
    let event = m.map(|x| x.event.clone()).unwrap_or_default();
    let star_on = favs.iter().any(|f| f == s(item, "path"));
    // «Preview» when we show NOT the movie that will play at login.
    let is_current = current.map(|c| s(c, "path") == s(item, "path")).unwrap_or(false);
    HeroView {
        present: true,
        title,
        source: format!("🎮 {}", source_text(item, meta)),
        labels: if labels.is_empty() {
            String::new()
        } else {
            // TRANSLATORS: %1 is a comma-separated list of Steam item tags
            // (rarity, game, item type…).
            trf("Tags: %1", &[&labels.join(", ")])
        },
        event: if event.is_empty() {
            String::new()
        } else {
            // TRANSLATORS: %1 is the name of a Steam sale or event.
            trf("Event: %1", &[&event])
        },
        star_text: if star_on { sv_star_on() } else { sv_star_off() }.to_string(),
        star_on,
        status: if is_current {
            String::new()
        } else {
            sv_hero_preview_hint().to_string()
        },
    }
}

/// Tone of the large summary state chip.
pub const CHIP_OK: &str = "ok";
pub const CHIP_WARN: &str = "warn";
pub const CHIP_OFF: &str = "off";

/// The summary state in ONE line (instead of a scatter of checkmarks).
///
/// `favs_len` is how many movies are in favorites, `items_len` is how many
/// were found in total: with two or more favorites the rotation goes through exactly those.
pub fn state_chip(
    current: Option<&Value>,
    enabled: bool,
    rotate: bool,
    favs_len: usize,
    items_len: usize,
) -> (String, &'static str) {
    let Some(cur) = current else {
        return (tr("⚪ No startup movie selected"), CHIP_OFF);
    };
    let title = {
        let t = s(cur, "title");
        if t.is_empty() {
            s(cur, "name").to_string()
        } else {
            t.to_string()
        }
    };
    if !enabled {
        return (
            tr("⚪ Off — nothing plays at login"),
            CHIP_OFF,
        );
    }
    // ⚠️ Fix while porting (rule #16): with one movie on disk the chip promised
    // "a random movie out of all (1)", although the draw at login skips such a
    // pool (`session_splash::rotation_choices`) and the same movie is shown.
    // Now a rotation that will not happen is not promised in the chip.
    if rotate && session_splash::rotation_possible(favs_len, items_len) {
        let by_favs = session_splash::rotation_uses_favorites(favs_len);
        let pool = session_splash::rotation_pool_size(favs_len, items_len);
        let text = if by_favs {
            // TRANSLATORS: %1 is how many favorite movies take turns, %2 is
            // the title of the movie that is up next.
            trf(
                "✅ Ready: the next login plays a random favorite (%1 in the pool), “%2” is up next",
                &[&pool, &title],
            )
        } else {
            // TRANSLATORS: %1 is how many movies take turns, %2 is the title
            // of the movie that is up next.
            trf(
                "✅ Ready: the next login plays a random movie out of all (%1 in the pool), “%2” is up next",
                &[&pool, &title],
            )
        };
        return (text, CHIP_OK);
    }
    (
        // TRANSLATORS: %1 is the title of the startup movie.
        trf("✅ Ready: the next login plays “%1”", &[&title]),
        CHIP_OK,
    )
}

// ============================================================================
// Profile background showcase: sizes and captions
// ============================================================================

/// Preview on a background card, 16:9, like a desktop wallpaper.
pub const BG_THUMB_W: i32 = 140;
pub const BG_THUMB_H: i32 = 79;
/// Width of a background card. CONSTANT for the same reason as for the showcase:
/// the column count is arithmetic over the card width.
pub const BG_CARD_W: i32 = 180;
/// Pagination is set in ROWS, not in the number of items: with five columns a
/// page has 25 items, with three 15, and the last row is always full.
pub const BG_ROWS: u32 = 5;
pub const BG_MIN_COLS: u32 = 2;
/// The column ceiling. It was 8, while the QML grid laid out cards by itself
/// and on a 1920 window put nine: a page of 8 × 5 = 40 ended with a row of
/// four cards. Now the core sets the grid columns ([`bg_columns`]), and the
/// ceiling is raised so a wide window does not stretch cards twofold.
pub const BG_MAX_COLS: u32 = 16;

/// Columns in the background grid for the grid's own width `width` (without
/// group margins): how many `BG_CARD_W` cards with the `CARD_GAP` gap fit.
/// The same number is in `columns` of the QML grid, so a page of
/// `cols × BG_ROWS` is always whole rows.
pub fn bg_columns(width: i32) -> u32 {
    ui_base::grid_columns(width, BG_CARD_W, CARD_GAP, 0, BG_MIN_COLS, BG_MAX_COLS)
}

pub fn bg_search_placeholder() -> String {
    tr("🔍 Search by name or game…")
}
pub fn bg_audio_text() -> String {
    tr("🔊 Live wallpaper sound")
}
pub fn bg_loading_text() -> String {
    tr("⏳ Loading your Steam inventory…")
}
pub fn bg_apply_text() -> String {
    tr("Apply")
}
pub fn bg_applied_label() -> String {
    tr("✅ Applied")
}
pub fn bg_prev_text() -> String {
    // TRANSLATORS: pager button: previous page.
    tr("‹ Prev")
}
pub fn bg_next_text() -> String {
    // TRANSLATORS: pager button: next page.
    tr("Next ›")
}
pub fn bg_equipped_sub() -> String {
    // TRANSLATORS: short note under a background card: this background is
    // equipped on the Steam profile. Keep it short, the card is narrow.
    tr("🎮 equipped")
}
pub fn bg_animated_tooltip() -> String {
    tr("Animated background (webm) — applies as a live wallpaper")
}
/// 📼 for animated ones, 🖼️ for static ones (VS16 means a color emoji, they are distinguishable).
pub const BG_ICON_ANIMATED: &str = "📼";
pub const BG_ICON_STATIC: &str = "🖼️";

pub fn bg_no_steam() -> String {
    tr("Steam not found.")
}
pub fn bg_no_account() -> String {
    tr("Couldn't find your Steam account (no userdata folder).")
}
pub fn bg_nothing_found() -> String {
    tr("No profile backgrounds in your inventory — get or buy one (for example in the Points Shop) \
        and it will show up here.")
}
pub fn bg_busy() -> String {
    tr("⏳ Already applying a wallpaper — please wait")
}
pub fn bg_no_engine() -> String {
    tr("⚠️ Wallpapers can't be applied right now")
}
pub fn bg_apply_failed() -> String {
    tr("Couldn't apply")
}

/// How many characters of the inventory error are shown (`error[:160]` of the original).
pub const BG_ERROR_CLIP: usize = 160;

/// Text of an inventory loading failure.
pub fn bg_error_text(error: &str) -> String {
    let clipped: String = error.chars().take(BG_ERROR_CLIP).collect();
    // TRANSLATORS: %1 is the error text (may be technical, in English).
    trf(
        "⚠️ Couldn't load the inventory (it may be hidden by Steam's privacy settings): %1",
        &[&clipped],
    )
}

/// Caption of the profile backgrounds section: the reason matters more than the count.
///
/// The reason ("Steam not found.", "no userdata", "the inventory did not load",
/// "no backgrounds") is written in the original into the same label as the
/// background count (`rexon/qt_steam/window.py`, `_bg_status`), and that is
/// the only honest place for it: «Backgrounds found: 0» with Steam not found
/// explains nothing to a person, and a popup message on top of the
/// neighbouring section even less (it also goes away after three seconds).
pub fn bg_section_text(notice: &str, total: usize, shown: usize, needle: &str) -> String {
    bg_section_text_filtered(notice, total, shown, !needle.is_empty())
}

/// The same, but the caller says "whether filtered": not only search filters,
/// but also the kind filter (static / animated).
pub fn bg_section_text_filtered(notice: &str, total: usize, shown: usize, filtered: bool) -> String {
    if !notice.is_empty() {
        return notice.to_string();
    }
    if total == 0 {
        trf("Backgrounds found: %1", &[&0])
    } else if filtered {
        showing_text(shown, total)
    } else {
        trf("Backgrounds found: %1", &[&total])
    }
}

/// Status line under the search.
pub fn bg_status_text(total: usize, shown: usize, needle: &str) -> String {
    bg_section_text_filtered("", total, shown, !needle.is_empty())
}

/// The line "showing the cache, refreshing over the network".
pub fn bg_refreshing_text(cached: usize) -> String {
    // TRANSLATORS: %1 is how many backgrounds are shown from the cache while
    // the list is being refreshed from Steam.
    trf("🔄 Updating… (%1 from cache)", &[&cached])
}

/// Toast "downloading and applying".
pub fn bg_applying_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a profile background.
    trf("⏳ Downloading and applying: %1…", &[&name])
}

/// Success toast.
pub fn bg_applied_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a profile background.
    trf("✅ Applied as wallpaper: %1", &[&name])
}

/// Failure toast.
pub fn bg_apply_error_text(msg: &str) -> String {
    if msg.is_empty() {
        format!("❌ {}", bg_apply_failed())
    } else {
        format!("❌ {msg}")
    }
}

/// Apply a downloaded profile background as the wallpaper through the core's shared executor.
///
/// Port of the body of `_ApplyBgWorker.run` after the download: the format by
/// extension ([`apply_format`]) and the name from the showcase; the executor
/// cannot derive it itself, and the bridge needs it to tell the listener WHAT
/// was applied. Returns "whether it worked" and the executor's text for the toast.
///
/// ⚠️ There are no own application rules here and there must not be: a
/// picture, a video, refusing formats the executor does not carry yet are
/// all decided by [`crate::wallpaper_exec::WallpaperExec::apply`], the same
/// as for the «Settings» tab. Before this function the Rust «Steam» tab
/// applied nothing: in place of the engine stood a stub "wallpaper engine unavailable".
///
/// `audio` is the «🔊 Live wallpaper sound» checkbox: it goes to the executor
/// with the same argument «Settings» uses to pass sound
/// ([`crate::wallpaper_exec::WallpaperExec::apply_with_audio`]); a picture
/// does not need it. Volume and "sound on/off" live in `settings.json`
/// (`wallpaper_audio`) and act on top of it.
pub fn apply_background_with<S: WpSys>(
    exec: &mut WallpaperExec<S>,
    path: &str,
    audio: bool,
    name: &str,
) -> (bool, String) {
    let res = exec.apply_with_audio(path, apply_format(path), name, audio);
    // The executor's warning follows the message, as it was before it was moved
    // into its own field.
    let message = match res.warning {
        Some(warning) if res.ok => format!("{} {warning}", res.message).trim().to_string(),
        _ => res.message,
    };
    (res.ok, message)
}

// ============================================================================
// Profile background showcase: decisions
// ============================================================================

/// Search over the background grid: name and game.
pub fn bg_matches(item: &ProfileBackgroundItem, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    format!("{} {}", item.name, item.game)
        .to_lowercase()
        .contains(needle)
}

/// Whether a background is animated: ONE decision for the whole tab.
///
/// The same sign by which «Apply» chooses between a picture and a live
/// wallpaper: Steam sent a movie (`animated`), or the background URL itself
/// points to a movie or GIF ([`apply_format`] by the URL extension). The
/// card, the mark on the thumbnail and the large view ask it from here
/// instead of each guessing by extension on its own.
pub fn bg_is_animated(item: &ProfileBackgroundItem) -> bool {
    item.animated || apply_format(&format!("bg{}", url_ext_or_jpg(&item.url))) != "static"
}

/// Mark of an animated background in the caption of the large view.
pub fn bg_animated_badge() -> String {
    // TRANSLATORS: short mark shown next to a Steam profile background that
    // is animated (a video), as opposed to a still picture.
    tr("🎞️ Animated background")
}

/// Mark of an animated background in the corner of the thumbnail.
pub const BG_ANIMATED_MARK: &str = "🎞️";

// ── Kind filter: all / static / animated ────────────────────────────────

/// Keys of the background kind filter. Empty means "all".
pub const BG_KIND_ALL: &str = "";
pub const BG_KIND_STATIC: &str = "static";
pub const BG_KIND_ANIMATED: &str = "animated";

/// Normalize the filter key to a known one: an unknown one means "all".
pub fn normalize_bg_kind(key: &str) -> &'static str {
    match key {
        BG_KIND_STATIC => BG_KIND_STATIC,
        BG_KIND_ANIMATED => BG_KIND_ANIMATED,
        _ => BG_KIND_ALL,
    }
}

/// Whether the background passes the kind filter.
pub fn bg_kind_matches(item: &ProfileBackgroundItem, kind: &str) -> bool {
    match normalize_bg_kind(kind) {
        BG_KIND_STATIC => !bg_is_animated(item),
        BG_KIND_ANIMATED => bg_is_animated(item),
        _ => true,
    }
}

/// Background kind switcher with counters in the captions.
///
/// What passed the SEARCH is counted: with "portal" typed, the captions show
/// how many static and animated ones are among the found ones, not in the
/// whole inventory; otherwise the number on the button would not match what
/// the person sees after pressing it. An empty answer means nothing to show (no backgrounds at all).
pub fn bg_kind_chips(items: &[ProfileBackgroundItem], needle: &str, selected: &str) -> Vec<GroupChip> {
    if items.is_empty() {
        return Vec::new();
    }
    let found: Vec<&ProfileBackgroundItem> =
        items.iter().filter(|it| bg_matches(it, needle)).collect();
    let animated = found.iter().filter(|it| bg_is_animated(it)).count();
    let still = found.len() - animated;
    let cur = normalize_bg_kind(selected);
    vec![
        GroupChip {
            key: BG_KIND_ALL.to_string(),
            // TRANSLATORS: filter button above the Steam profile backgrounds
            // grid; %1 is how many backgrounds there are.
            label: trf("All (%1)", &[&found.len()]),
            checked: cur == BG_KIND_ALL,
        },
        GroupChip {
            key: BG_KIND_STATIC.to_string(),
            // TRANSLATORS: filter button: backgrounds that are still
            // pictures (not animated); %1 is how many.
            label: trf("Still (%1)", &[&still]),
            checked: cur == BG_KIND_STATIC,
        },
        GroupChip {
            key: BG_KIND_ANIMATED.to_string(),
            // TRANSLATORS: filter button: animated (video) backgrounds;
            // %1 is how many.
            label: trf("Animated (%1)", &[&animated]),
            checked: cur == BG_KIND_ANIMATED,
        },
    ]
}

// ── Why there are no animated backgrounds in the list ───────────────────

/// How the request for owned profile items ended: the only source of links
/// to animations. The public inventory does not give animations at all, and
/// when this request failed, all backgrounds in the list are static.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AnimatedGap {
    /// The owned list arrived: no gap.
    #[default]
    None,
    /// No access key; inside is the reason in the core's words
    /// ([`crate::steam_token::no_login_reason`] and its neighbours).
    NoKey(String),
    /// Steam refused even with a fresh key.
    Refused,
    /// Steam did not answer: network, timeout, unparseable response.
    NoAnswer,
}

/// Refusal of one request for owned items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedError {
    /// 401/403: the key was not accepted.
    Refused,
    /// Everything else: network, timeout, the response did not parse.
    NoAnswer,
}

/// Refusal by response code: "key not accepted" is 401 and 403, the rest is
/// "did not answer".
pub fn owned_error_of_status(code: Option<u16>) -> OwnedError {
    match code {
        Some(401) | Some(403) => OwnedError::Refused,
        _ => OwnedError::NoAnswer,
    }
}

/// Get the owned backgrounds: key → request, on refusal a fresh key and ONE
/// retry (the key is short-lived and may have expired before the cache term).
///
/// `key(force)` returns `(key, reason)`: an empty key means no login, the
/// reason goes to the person. `fetch(key)` is the request itself. The network
/// and key reading come from outside; here only the order of steps and the verdict.
pub fn owned_backgrounds_with<T>(
    mut key: impl FnMut(bool) -> (String, String),
    mut fetch: impl FnMut(&str) -> Result<Vec<T>, OwnedError>,
) -> (Vec<T>, AnimatedGap) {
    let (token, reason) = key(false);
    if token.is_empty() {
        return (Vec::new(), AnimatedGap::NoKey(reason));
    }
    match fetch(&token) {
        Ok(items) => (items, AnimatedGap::None),
        Err(OwnedError::NoAnswer) => (Vec::new(), AnimatedGap::NoAnswer),
        Err(OwnedError::Refused) => {
            let (token, reason) = key(true);
            if token.is_empty() {
                return (Vec::new(), AnimatedGap::NoKey(reason));
            }
            match fetch(&token) {
                Ok(items) => (items, AnimatedGap::None),
                Err(OwnedError::Refused) => (Vec::new(), AnimatedGap::Refused),
                Err(OwnedError::NoAnswer) => (Vec::new(), AnimatedGap::NoAnswer),
            }
        }
    }
}

/// The line above the background grid: why there are no animated ones and what
/// to do about it. Empty means no gap, the line is not shown.
pub fn bg_animated_gap_text(gap: &AnimatedGap) -> String {
    match gap {
        AnimatedGap::None => String::new(),
        // TRANSLATORS: %1 is the reason in lower case, e.g. "sign in to
        // Steam on this computer". "Retry" is the button next to it.
        AnimatedGap::NoKey(reason) => {
            // A reason without words is the same "no key given": the phrase must not be
            // cut off with a colon.
            let fallback = crate::steam_token::no_token_reason();
            let reason = if reason.trim().is_empty() { &fallback } else { reason };
            trf(
                "⚠️ Animated backgrounds are not shown: %1. Start Steam, sign in and press \"Retry\".",
                &[reason],
            )
        }
        AnimatedGap::Refused => tr(
            "⚠️ Animated backgrounds are not shown: Steam refused to list your profile items. Start Steam, sign in and press \"Retry\".",
        ),
        AnimatedGap::NoAnswer => tr(
            "⚠️ Animated backgrounds are not shown: Steam didn't answer. Check the internet connection and press \"Retry\".",
        ),
    }
}

/// The button next to the [`bg_animated_gap_text`] line.
pub fn bg_retry_text() -> String {
    tr("Retry")
}

/// The slice filtered by search (indices of the source list).
pub fn bg_filtered<'a>(items: &'a [ProfileBackgroundItem], needle: &str) -> Vec<&'a ProfileBackgroundItem> {
    bg_filtered_by(items, needle, BG_KIND_ALL)
}

/// The slice selected by search AND the kind filter at once.
pub fn bg_filtered_by<'a>(
    items: &'a [ProfileBackgroundItem],
    needle: &str,
    kind: &str,
) -> Vec<&'a ProfileBackgroundItem> {
    items
        .iter()
        .filter(|it| bg_matches(it, needle) && bg_kind_matches(it, kind))
        .collect()
}

/// A card of the background grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BgCardView {
    pub id: String,
    /// 📼 or 🖼️.
    pub icon: &'static str,
    /// The full name (truncation by width is done by the view, it has the font metrics).
    pub name: String,
    /// Second line: "equipped in profile", the game, or empty. An empty line is
    /// drawn anyway, otherwise the cards of a row diverge in height.
    pub sub: String,
    /// Tooltip of the second line (empty if there is no tooltip).
    pub sub_tooltip: String,
    pub animated: bool,
    /// Preview tooltip (animated ones only).
    pub thumb_tooltip: String,
    /// Mark in the thumbnail corner: an icon for an animated one, empty for a
    /// static one. Its tooltip is shared with the thumbnail (`thumb_tooltip`).
    pub mark: &'static str,
    /// URL to fetch the thumbnail from (preview, otherwise the background itself).
    pub preview_url: String,
}

/// Build a background card.
pub fn bg_card(item: &ProfileBackgroundItem) -> BgCardView {
    let animated = bg_is_animated(item);
    let icon = if animated {
        BG_ICON_ANIMATED
    } else {
        BG_ICON_STATIC
    };
    let (sub, sub_tooltip) = if item.equipped {
        (bg_equipped_sub().to_string(), String::new())
    } else if !item.game.is_empty() {
        (item.game.clone(), item.game.clone())
    } else {
        (String::new(), String::new())
    };
    BgCardView {
        id: item.id.clone(),
        icon,
        name: if item.name.is_empty() {
            "?".to_string()
        } else {
            item.name.clone()
        },
        sub,
        sub_tooltip,
        animated,
        thumb_tooltip: if animated { bg_animated_tooltip() } else { String::new() },
        mark: if animated { BG_ANIMATED_MARK } else { "" },
        preview_url: if item.preview.is_empty() {
            item.url.clone()
        } else {
            item.preview.clone()
        },
    }
}

/// The pagination bar.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PagerView {
    pub visible: bool,
    pub prev_enabled: bool,
    pub next_enabled: bool,
    pub text: String,
    /// Page number after normalization (0-based).
    pub page: u32,
    pub pages: u32,
    /// Slice of the current page: [start, end).
    pub start: usize,
    pub end: usize,
}

/// Cut the page and build the pagination bar.
///
/// `cols` is computed by [`ui_base::grid_columns`] from the live width; here
/// only page arithmetic (the same engine as the product's other grids).
pub fn bg_pager(shown: usize, page: u32, cols: u32) -> PagerView {
    let per_page = ui_base::grid_page_size(cols, BG_ROWS);
    let pages = ui_base::grid_page_count(shown as u32, per_page);
    let page = page.min(pages.saturating_sub(1));
    let start = (page as usize) * (per_page as usize);
    let end = (start + per_page as usize).min(shown);
    PagerView {
        visible: shown > per_page as usize,
        prev_enabled: page > 0,
        next_enabled: page + 1 < pages,
        // TRANSLATORS: pager: page %1 of %2.
        text: trf("Page %1 / %2", &[&(page + 1), &pages]),
        page,
        pages,
        start: start.min(shown),
        end,
    }
}

/// Page number when the column count changes: what was first on screen stays
/// first (`grid_page_for_index` of the original).
pub fn bg_page_after_resize(page: u32, old_cols: u32, new_cols: u32) -> u32 {
    let first = page * ui_base::grid_page_size(old_cols, BG_ROWS);
    ui_base::grid_page_for_index(first, ui_base::grid_page_size(new_cols, BG_ROWS))
}

/// Format for applying a background as the wallpaper, by file extension.
///
/// webm/mp4 → live wallpaper, gif → animated, the rest → a static picture.
pub fn apply_format(path: &str) -> &'static str {
    match splitext_lower(path).as_str() {
        ".webm" | ".mp4" => "video",
        ".gif" => "animated",
        _ => "static",
    }
}

// ============================================================================
// Steam account, inventory and caches
// ============================================================================

/// SteamID64 offset: accountid (the `userdata/<id>` folder name) + this number.
pub const STEAMID64_BASE: i64 = 76561197960265728;

/// User-Agent of the tab's requests.
pub const USER_AGENT: &str = "Rexon/5.0";

/// Timeout of an inventory page, seconds (`_fetch_inventory_pages`).
pub const INVENTORY_TIMEOUT_S: u64 = 25;
/// Ceiling on the number of inventory pages: protection against endless `more_items`.
pub const INVENTORY_MAX_PAGES: u32 = 20;
/// Timeout of downloading a full-size background, seconds.
pub const BG_DOWNLOAD_TIMEOUT_S: u64 = 60;
/// Timeout of downloading a thumbnail, seconds.
pub const THUMB_DOWNLOAD_TIMEOUT_S: u64 = 30;

/// SteamID64 by the folder name `userdata/<accountid>`.
///
/// `None` means the name is not a number, or it is the internal "0".
pub fn steamid64_from_accountid(accountid: &str) -> Option<String> {
    if accountid.is_empty() || accountid == "0" || !accountid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let id: i64 = accountid.parse().ok()?;
    Some((id + STEAMID64_BASE).to_string())
}

/// Account choice: the one whose `localconfig.vdf` is newer (the last one logged in).
///
/// Input: pairs "folder name → localconfig modification time". The executor
/// simply does not put missing files into the list.
pub fn pick_account(entries: &[(String, f64)]) -> Option<String> {
    let mut best: Option<(&str, f64)> = None;
    for (sid, mtime) in entries {
        if sid == "0" || !sid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        match best {
            Some((_, bm)) if *mtime <= bm => {}
            _ => best = Some((sid.as_str(), *mtime)),
        }
    }
    best.and_then(|(sid, _)| steamid64_from_accountid(sid))
}

/// Language of Steam responses (`l=` of the inventory, `language=` of WebAPI)
/// by the Rexon interface language: Steam returns item names, types and tags
/// already translated, and they must be requested in the language the tab speaks.
///
/// Previously the language was hard-coded, `l=russian`, and the English tab
/// would show Russian inventory tags. The codes are from the Steam Web API
/// language list; a language Steam does not have means English.
pub fn steam_api_language(ui_language: &str) -> &'static str {
    let lang = ui_language.to_ascii_lowercase().replace('-', "_");
    let base = lang.split(['_', '.', '@']).next().unwrap_or("");
    match (base, lang.as_str()) {
        ("pt", l) if l.starts_with("pt_br") => "brazilian",
        ("es", l) if l.starts_with("es_") && !l.starts_with("es_es") => "latam",
        ("zh", l) if l.starts_with("zh_tw") || l.starts_with("zh_hk") => "tchinese",
        ("zh", _) => "schinese",
        ("ru", _) => "russian",
        ("uk", _) => "ukrainian",
        ("de", _) => "german",
        ("fr", _) => "french",
        ("es", _) => "spanish",
        ("it", _) => "italian",
        ("pt", _) => "portuguese",
        ("pl", _) => "polish",
        ("cs", _) => "czech",
        ("nl", _) => "dutch",
        ("da", _) => "danish",
        ("fi", _) => "finnish",
        ("nb" | "nn" | "no", _) => "norwegian",
        ("sv", _) => "swedish",
        ("hu", _) => "hungarian",
        ("ro", _) => "romanian",
        ("bg", _) => "bulgarian",
        ("el", _) => "greek",
        ("tr", _) => "turkish",
        ("th", _) => "thai",
        ("vi", _) => "vietnamese",
        ("id", _) => "indonesian",
        ("ja", _) => "japanese",
        ("ko", _) => "koreana",
        _ => "english",
    }
}

/// Language of Steam responses for the current interface language.
pub fn steam_language() -> &'static str {
    steam_api_language(&i18n::current_language())
}

/// URL of one page of the public inventory (753/6).
///
/// An empty `start` means the first page; otherwise `start_assetid` from the
/// previous response. The response language is the interface language ([`steam_language`]).
pub fn inventory_page_url(sid: &str, start: &str) -> String {
    let base = format!(
        "https://steamcommunity.com/inventory/{sid}/753/6?l={}&count=2000",
        steam_language()
    );
    if start.is_empty() {
        base
    } else {
        format!("{base}&start_assetid={}", url_quote(start))
    }
}

/// Cursor of the next inventory page: `None` means there are no more pages.
///
/// An empty `last_assetid` with `more_items` also means the end; otherwise
/// the loop would re-request the first page forever.
pub fn inventory_next_start(page: &Value) -> Option<String> {
    let more = page.get("more_items").map(truthy).unwrap_or(false);
    if !more {
        return None;
    }
    let last = page
        .get("last_assetid")
        .map(|v| match v {
            Value::String(x) => x.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    if last.is_empty() {
        None
    } else {
        Some(last)
    }
}

/// Merge inventory pages into one response (`assets` + `descriptions`).
pub fn merge_inventory_pages(pages: &[Value]) -> Value {
    let mut assets: Vec<Value> = Vec::new();
    let mut descs: Vec<Value> = Vec::new();
    for p in pages {
        if let Some(a) = p.get("assets").and_then(Value::as_array) {
            assets.extend(a.iter().cloned());
        }
        if let Some(d) = p.get("descriptions").and_then(Value::as_array) {
            descs.extend(d.iter().cloned());
        }
    }
    let mut m = Map::new();
    m.insert("assets".into(), Value::Array(assets));
    m.insert("descriptions".into(), Value::Array(descs));
    Value::Object(m)
}

/// Localized value of an item tag (or an empty string).
pub fn item_tag(desc: &Value, category: &str) -> String {
    let Some(tags) = desc.get("tags").and_then(Value::as_array) else {
        return String::new();
    };
    for t in tags {
        if s(t, "category").to_lowercase() == category {
            return s(t, "localized_tag_name").to_string();
        }
    }
    String::new()
}

/// Item tags in the same composition the Steam inventory shows:
/// rarity, game/event, item type, trade and market restrictions.
pub fn item_labels(desc: &Value) -> Vec<String> {
    let mut out = vec![
        item_tag(desc, "droprate"),
        item_tag(desc, "game"),
        item_tag(desc, "item_class"),
    ];
    if !desc.get("tradable").map(truthy).unwrap_or(false) {
        // TRANSLATORS: Steam inventory label, as Steam writes it.
        out.push(tr("Not Tradable"));
    }
    if !desc.get("marketable").map(truthy).unwrap_or(false) {
        // TRANSLATORS: Steam inventory label, as Steam writes it.
        out.push(tr("Not Marketable"));
    }
    let mut uniq: Vec<String> = Vec::new();
    for x in out {
        if !x.is_empty() && !uniq.contains(&x) {
            uniq.push(x);
        }
    }
    uniq
}

/// `internal_name` of the "startup movie" item.
pub const SPLASH_ITEM_CLASS: &str = "item_class_17";

/// appid of the source game → startup movie tags from the inventory.
///
/// `market_fee_app` points to the game (or to an event page; then the "Game"
/// among the tags is the sale's name, exactly as in the Steam inventory).
pub fn splash_item_meta(descs: &[Value]) -> MetaMap {
    let mut out = MetaMap::new();
    for d in descs {
        let is_splash = d
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| tags.iter().any(|t| s(t, "internal_name") == SPLASH_ITEM_CLASS))
            .unwrap_or(false);
        if !is_splash {
            continue;
        }
        let appid = match d.get("market_fee_app") {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            Some(Value::String(x)) => x.parse::<i64>().unwrap_or(0),
            _ => 0,
        };
        if appid == 0 {
            continue;
        }
        out.insert(
            appid,
            SplashMeta {
                name: s(d, "name").to_string(),
                source: s(d, "type").to_string(),
                event: item_tag(d, "event"),
                labels: item_labels(d),
            },
        );
    }
    out
}

/// Parse the disk cache of splash tags (`appid` as a string → tags).
pub fn parse_meta_cache(data: &Value) -> MetaMap {
    let mut out = MetaMap::new();
    let Some(obj) = data.as_object() else {
        return out;
    };
    for (k, v) in obj {
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(appid) = k.parse::<i64>() else { continue };
        out.insert(
            appid,
            SplashMeta {
                name: s(v, "name").to_string(),
                source: s(v, "source").to_string(),
                event: s(v, "event").to_string(),
                labels: v
                    .get("labels")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        );
    }
    out
}

/// Contents of the disk cache of splash tags (what `json.dump` wrote).
pub fn meta_cache_json(meta: &MetaMap) -> String {
    let mut m = Map::new();
    for (appid, v) in meta {
        m.insert(
            appid.to_string(),
            serde_json::to_value(v).unwrap_or(Value::Null),
        );
    }
    serde_json::to_string(&Value::Object(m)).unwrap_or_else(|_| "{}".into())
}

// ── Equipped background from localconfig.vdf ────────────────────────────

/// The key under which Steam caches the `GetEquippedProfileItemsForUser` response.
const EQUIPPED_KEY_PREFIX: &str = "GetEquippedProfileItemsForUser";

/// Extract the JSON of equipped profile items from the text of `localconfig.vdf`.
///
/// Port of the regex `"GetEquippedProfileItemsForUser\d+"\s*"(.*?)"\s*[\r\n]`
/// with the subsequent VDF un-escape. The core has no regex engine, and adding
/// one for a single place is pointless: the parsing is line-based and does
/// exactly the same: finds the key, takes the value up to the quote followed
/// (after whitespace) by a line break, and removes the escaping.
pub fn extract_equipped_json(vdf_text: &str) -> Option<String> {
    let bytes = vdf_text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = vdf_text[from..].find(EQUIPPED_KEY_PREFIX) {
        let key_start = from + rel;
        // The key must be quoted: `"GetEquippedProfileItemsForUser<digits>"`.
        if key_start == 0 || bytes[key_start - 1] != b'"' {
            from = key_start + EQUIPPED_KEY_PREFIX.len();
            continue;
        }
        let mut i = key_start + EQUIPPED_KEY_PREFIX.len();
        let digits_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits_start || i >= bytes.len() || bytes[i] != b'"' {
            from = key_start + EQUIPPED_KEY_PREFIX.len();
            continue;
        }
        i += 1; // closing quote of the key
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            from = key_start + EQUIPPED_KEY_PREFIX.len();
            continue;
        }
        i += 1; // opening quote of the value
        let val_start = i;
        // Non-greedy `(.*?)"\s*[\r\n]`: the first quote followed, after whitespace,
        // by a line break.
        let mut j = val_start;
        while j < bytes.len() {
            if bytes[j] == b'"' {
                let mut k = j + 1;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if k < bytes.len() && (bytes[k] == b'\r' || bytes[k] == b'\n') {
                    return Some(unescape_vdf(&vdf_text[val_start..j]));
                }
            }
            j += 1;
        }
        from = key_start + EQUIPPED_KEY_PREFIX.len();
    }
    None
}

/// `raw.replace('\\"', '"').replace('\\\\', '\\')` of the original.
fn unescape_vdf(raw: &str) -> String {
    raw.replace("\\\"", "\"").replace("\\\\", "\\")
}

// ── Cache and config paths ──────────────────────────────────────────────

/// Root of the user cache: `$XDG_CACHE_HOME` or `~/.cache`.
///
/// Fix #1: the original hard-coded `~/.cache` as a literal. The same approach
/// as in [`crate::session_splash::preview_cache_dir`].
pub fn cache_home(xdg_cache_home: Option<&str>, home: &str) -> String {
    match xdg_cache_home {
        Some(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => format!("{}/.cache", home.trim_end_matches('/')),
    }
}

/// The tab's cache directory: `<cache>/rexon`.
pub fn rexon_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/rexon", cache_home(xdg_cache_home, home))
}

/// Cache of downloaded full-size profile backgrounds.
pub fn bg_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/steam_profile_bg", rexon_cache_dir(xdg_cache_home, home))
}

/// Cache of preview thumbnails of the background grid. Separate from
/// [`bg_cache_dir`]: thumbnails are small and shared by all.
pub fn thumb_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/steam_profile_thumb", rexon_cache_dir(xdg_cache_home, home))
}

/// Disk cache of startup movie tags for the account.
pub fn splash_meta_cache_path(xdg_cache_home: Option<&str>, home: &str, sid: &str) -> String {
    format!(
        "{}/steam_splash_items_{}.json",
        rexon_cache_dir(xdg_cache_home, home),
        safe_name(sid)
    )
}

/// Disk cache of the background inventory LIST for the account.
pub fn inventory_cache_path(xdg_cache_home: Option<&str>, home: &str, sid: &str) -> String {
    format!(
        "{}/steam_profile_inventory_{}.json",
        rexon_cache_dir(xdg_cache_home, home),
        safe_name(sid)
    )
}

/// Config "which profile background is currently applied as the wallpaper".
///
/// Fix #1: the original expanded `~/.config/rexon/…` through `expand_user`,
/// which respects `$XDG_CONFIG_HOME`; here the same with an explicit call of
/// [`crate::utils::config_home`].
pub fn applied_config_path(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/steam_profile_bg.json",
        utils::config_home(xdg_config_home, home)
    )
}

/// Cache file name of a full-size background (`<id><URL extension>`).
pub fn bg_cache_file_name(id: &str, url: &str) -> String {
    format!("{}{}", id, url_ext_or_jpg(url))
}

/// Cache file name of a thumbnail. `id` may contain "/" (a fallback key from
/// the name), so the file name is made safe.
pub fn thumb_cache_file_name(id: &str, url: &str) -> String {
    format!("{}{}", safe_name(id), url_ext_or_jpg(url))
}

/// Temporary download file. Fix #3: it must be removed on ANY outcome,
/// otherwise the cache fills up with partial `.part` files.
pub fn part_path(path: &str) -> String {
    format!("{path}.part")
}

// ── Disk cache of the inventory list ────────────────────────────────────

/// Contents of the inventory list cache file.
pub fn inventory_cache_json(sid: &str, items: &[ProfileBackgroundItem], ts: i64) -> String {
    let mut m = Map::new();
    m.insert("ts".into(), Value::from(ts));
    m.insert("sid".into(), Value::from(sid));
    m.insert(
        "items".into(),
        serde_json::to_value(items).unwrap_or(Value::Array(Vec::new())),
    );
    serde_json::to_string(&Value::Object(m)).unwrap_or_else(|_| "{}".into())
}

/// Parse the inventory list cache file.
///
/// `None` means there is no cache, or it is broken, empty or belongs to another account.
pub fn parse_inventory_cache(data: &Value, sid: &str) -> Option<Vec<ProfileBackgroundItem>> {
    let obj = data.as_object()?;
    let cached_sid = match obj.get("sid") {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    if cached_sid != sid {
        return None;
    }
    let items = obj.get("items")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    Some(items.iter().map(parse_cached_item).collect())
}

/// One item from the cache. Missing fields are empty: the cache may have been
/// written by an earlier version of the product, and the showcase must not crash because of it.
fn parse_cached_item(v: &Value) -> ProfileBackgroundItem {
    ProfileBackgroundItem {
        id: s(v, "id").to_string(),
        name: s(v, "name").to_string(),
        game: s(v, "game").to_string(),
        url: s(v, "url").to_string(),
        preview: s(v, "preview").to_string(),
        animated: v.get("animated").map(truthy).unwrap_or(false),
        equipped: v.get("equipped").map(truthy).unwrap_or(false),
    }
}

/// Contents of the "what is applied" config (indented, like `json.dump(indent=2)`).
pub fn applied_config_json(id: &str, name: &str) -> String {
    let mut m = Map::new();
    m.insert("id".into(), Value::from(id));
    m.insert("name".into(), Value::from(name));
    serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_else(|_| "{}".into())
}

/// id of the applied background from the config (empty string means none).
pub fn parse_applied_config(data: &Value) -> String {
    match data.get("id") {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

// ============================================================================
// Styles: QSS for the widget tab and bare colors for QML
// ============================================================================

pub const BADGE_NEUTRAL: &str = "neutral";
pub const BADGE_ACCENT: &str = "accent";
pub const BADGE_GOLD: &str = "gold";

/// The tab's accent: the same blue as the product's main buttons.
pub const ACCENT_RGB: (u8, u8, u8) = (59, 130, 246);
/// Green "applied".
pub const APPLIED_RGB: (u8, u8, u8) = (34, 197, 94);
/// Gold "in favorites".
pub const GOLD_RGB: (u8, u8, u8) = (234, 179, 8);

/// `rgba(r,g,b,a)` from a triple and an alpha: exactly the notation used in QSS.
///
/// The alpha is passed as a STRING, not a number: in the original it is typed
/// as literals (`0.85`, `1.0`, `0.90`), and any reformatting through `%g`
/// would make the stylesheet a different string for the same color. The
/// parity check compares strings.
fn rgba(c: (u8, u8, u8), a: &str) -> String {
    format!("rgba({},{},{},{})", c.0, c.1, c.2, a)
}

/// `#rrggbb` from a triple: the same paint for QML, which does not understand QSS.
pub fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// Margins of a section group: left/right, top/bottom and the gap between rows.
pub const GROUP_MARGIN_H: i32 = 16;
pub const GROUP_MARGIN_V: i32 = 14;
pub const GROUP_SPACING: i32 = 8;
/// Corner radius of a section group.
pub const GROUP_RADIUS: i32 = 6;

/// The "background, border" pair of a section group. One place for both sides:
/// the QSS for widgets is built right here ([`group_qss`]), bare colors go to QML.
pub fn group_colors(dark: bool) -> (String, String) {
    if dark {
        (
            "rgba(255,255,255,0.04)".to_string(),
            "rgba(255,255,255,0.12)".to_string(),
        )
    } else {
        (
            "rgba(0,0,0,0.03)".to_string(),
            "rgba(0,0,0,0.15)".to_string(),
        )
    }
}

/// QSS of a section group (theme-aware, border reset ONLY for QLabel, rule #9).
pub fn group_qss(name: &str, dark: bool) -> String {
    let sel = format!("QFrame#{name}");
    let (bg, border) = group_colors(dark);
    format!(
        "{sel} {{ background-color: {bg}; border: 1px solid {border}; \
border-radius: 6px; }} {sel} QLabel {{ border: none; background: transparent; }}"
    )
}

/// Text color on the "main" and green buttons.
pub const BTN_TEXT_ON_FILL: &str = "white";
/// Corner radius of the "main" button.
pub const BTN_RADIUS: i32 = 4;

/// The quadruple "background, hover background, disabled background, text" of the main button.
pub fn primary_btn_colors() -> (String, String, String, String) {
    (
        rgba(ACCENT_RGB, "0.85"),
        rgba(ACCENT_RGB, "1.0"),
        "rgba(120,120,120,0.35)".to_string(),
        BTN_TEXT_ON_FILL.to_string(),
    )
}

/// The triple "background, hover background, text" of the green «✓ Applied» button.
pub fn applied_btn_colors() -> (String, String, String) {
    (
        rgba(APPLIED_RGB, "0.90"),
        rgba(APPLIED_RGB, "1.0"),
        BTN_TEXT_ON_FILL.to_string(),
    )
}

/// QSS of the main button.
pub fn primary_btn_qss() -> String {
    let (bg, hover, disabled, fg) = primary_btn_colors();
    format!(
        "QPushButton {{ background-color: {bg}; color: {fg}; border: none; \
border-radius: 4px; padding: 6px 14px; font-weight: 600; }} \
QPushButton:hover {{ background-color: {hover}; }} \
QPushButton:disabled {{ background-color: {disabled}; }}"
    )
}

/// QSS of the green «✓ Applied» button.
pub fn applied_btn_qss() -> String {
    let (bg, hover, fg) = applied_btn_colors();
    format!(
        "QPushButton {{ background-color: {bg}; color: {fg}; border: none; \
border-radius: 4px; padding: 6px 14px; font-weight: 600; }} \
QPushButton:hover {{ background-color: {hover}; }}"
    )
}

/// Corner radius of a showcase card and its inner padding.
pub const CARD_RADIUS: i32 = 6;
pub const CARD_MARGIN: i32 = 8;
pub const CARD_SPACING: i32 = 6;
/// Gap between cards in the grid.
pub const CARD_GAP: i32 = 10;

/// The triple "background, border, hover border" of a showcase card.
pub fn card_colors(selected: bool, dark: bool) -> (String, String, String) {
    // The fill is the product's shared surface (`ui_base::CARD_FILL_*`): QML paints
    // the card with the same notation, only there the core returns it already
    // blended with the page background (`surface_card`). Two copies of the
    // literal would diverge silently.
    let (mut bg, mut border, mut hover) = if dark {
        (
            ui_base::CARD_FILL_DARK.to_string(),
            "rgba(255,255,255,0.12)".to_string(),
            "rgba(255,255,255,0.30)".to_string(),
        )
    } else {
        (
            ui_base::CARD_FILL_LIGHT.to_string(),
            "rgba(0,0,0,0.15)".to_string(),
            "rgba(0,0,0,0.35)".to_string(),
        )
    };
    if selected {
        bg = rgba(ACCENT_RGB, "0.18");
        border = rgba(ACCENT_RGB, "0.85");
        hover = rgba(ACCENT_RGB, "1.0");
    }
    (bg, border, hover)
}

/// QSS of a showcase card: ONE for both subtabs (rule #12).
pub fn card_qss(name: &str, selected: bool, dark: bool) -> String {
    let sel = format!("QFrame#{name}");
    let (bg, border, hover) = card_colors(selected, dark);
    format!(
        "{sel} {{ background-color: {bg}; border: 1px solid {border}; \
border-radius: 6px; }} {sel}:hover {{ border-color: {hover}; }} \
{sel} QLabel {{ border: none; background: transparent; }}"
    )
}

/// The "background, text" pair of a pill badge.
pub fn badge_colors(tone: &str, dark: bool) -> (String, String) {
    match tone {
        BADGE_ACCENT => (rgba(ACCENT_RGB, "0.85"), "white".to_string()),
        BADGE_GOLD => (rgba(GOLD_RGB, "0.85"), "#1a1a1a".to_string()),
        _ => (
            ui_base::neutral_rgba(0.14),
            ui_base::primary_text_color(dark).to_string(),
        ),
    }
}

/// QSS of the "pill" badge on a card.
pub fn badge_qss(tone: &str, dark: bool) -> String {
    let (bg, fg) = badge_colors(tone, dark);
    format!(
        "QLabel {{ background-color: {bg}; color: {fg}; border-radius: 7px; \
padding: 1px 7px; font-size: 11px; }}"
    )
}

/// The triple "background, border, text" of the large state chip.
pub fn chip_colors(tone: &str, dark: bool) -> (String, String, String) {
    match tone {
        CHIP_OK => (
            rgba(APPLIED_RGB, "0.16"),
            rgba(APPLIED_RGB, "0.65"),
            rgba(APPLIED_RGB, "1.0"),
        ),
        CHIP_WARN => (
            rgba(GOLD_RGB, "0.16"),
            rgba(GOLD_RGB, "0.65"),
            rgba(GOLD_RGB, "1.0"),
        ),
        _ => (
            ui_base::neutral_rgba(0.08),
            ui_base::neutral_rgba(0.22),
            ui_base::secondary_text_color(dark).to_string(),
        ),
    }
}

/// QSS of the large summary state chip.
pub fn chip_qss(tone: &str, dark: bool) -> String {
    let (bg, border, fg) = chip_colors(tone, dark);
    format!(
        "QLabel {{ background-color: {bg}; border: 1px solid {border}; \
color: {fg}; border-radius: 12px; padding: 5px 12px; font-weight: 600; }}"
    )
}

/// The quadruple "border, selected background, selected border, hover background"
/// of a switcher chip.
pub fn switch_chip_colors() -> (String, String, String, String) {
    (
        ui_base::neutral_rgba(0.20),
        rgba(ACCENT_RGB, "0.4"),
        rgba(ACCENT_RGB, "0.6"),
        ui_base::neutral_rgba(0.14),
    )
}

/// QSS of a switcher chip (subtabs and the showcase filter look the same).
pub fn switch_chip_qss() -> String {
    let (border, checked_bg, checked_border, hover_bg) = switch_chip_colors();
    format!(
        "QToolButton {{ padding: 4px 10px; border-radius: 4px; \
border: 1px solid {border}; }}QToolButton:checked {{ background-color: \
{checked_bg}; border-color: {checked_border}; }}QToolButton:hover:!checked \
{{ background-color: {hover_bg}; }}"
    )
}

/// Color of the "to favorites" star.
pub fn star_color(on: bool) -> String {
    if on {
        rgba(GOLD_RGB, "1.0")
    } else {
        ui_base::neutral_rgba(0.45)
    }
}

/// QSS of the star: a transparent button without a border.
pub fn star_btn_qss(on: bool) -> String {
    format!(
        "QToolButton {{ background: transparent; border: none; color: {}; \
font-size: 15px; padding: 0px; }} QToolButton:hover {{ color: {}; }}",
        star_color(on),
        rgba(GOLD_RGB, "1.0")
    )
}

/// Width of the colored stripe on the left of the callout and its padding.
pub const PITCH_BAR_W: i32 = 3;
pub const PITCH_RADIUS: i32 = 4;
pub const PITCH_MARGIN_H: i32 = 12;
pub const PITCH_MARGIN_V: i32 = 8;

/// The "background, left stripe" pair of the callout above the startup movie showcase.
pub fn pitch_colors() -> (String, String) {
    (rgba(ACCENT_RGB, "0.12"), rgba(ACCENT_RGB, "0.8"))
}

/// Thumbnail backing of a startup movie showcase card (behind the preview frame).
pub const SV_THUMB_BG: &str = "rgba(0,0,0,0.45)";
/// Thumbnail backing of a profile background card (behind the picture).
pub const BG_THUMB_BG: &str = "rgba(128,128,128,0.18)";
/// Corner radius of the thumbnail on a card.
pub const THUMB_RADIUS: i32 = 3;
/// Corner radius of the large preview in the chosen movie card.
pub const HERO_RADIUS: i32 = 5;
/// Backing of the large preview.
pub const HERO_THUMB_BG: &str = "rgba(0,0,0,0.45)";

/// QSS of the callout above the startup movie showcase.
pub fn pitch_qss() -> String {
    let (bg, bar) = pitch_colors();
    format!(
        "QLabel {{ background-color: {bg}; border-left: 3px solid {bar}; \
border-radius: 4px; padding: 8px 12px; }}"
    )
}

// ============================================================================
// Parsing odds and ends
// ============================================================================

/// Python truthiness of a JSON value (empty string/0/empty list are false).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `os.path.splitext(path)[1].lower()`.
fn splitext_lower(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rfind('.') {
        // A leading dot is not an extension (`.bashrc`), as in Python.
        Some(0) | None => String::new(),
        Some(i) => base[i..].to_lowercase(),
    }
}

/// Extension from the path of the URL (without query and fragment), or `.jpg`.
pub fn url_ext_or_jpg(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or(url);
    let path = no_frag.split('?').next().unwrap_or(no_frag);
    // The scheme and host do not get in the way: the extension is looked for in the last segment.
    let ext = splitext_lower(path);
    if ext.is_empty() {
        ".jpg".to_string()
    } else {
        ext
    }
}

/// `re.sub(r"[^\w.-]", "_", value)`: a safe file name.
///
/// `\w` of a Python string is Unicode letters, digits and underscore; here the
/// same through `char::is_alphanumeric`.
pub fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `urllib.parse.quote` for the inventory cursor value (only digits there, but
/// escaped honestly: the cursor comes from Steam, not from us).
fn url_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(path: &str, title: &str, game: &str, origin: &str, appid: i64) -> Value {
        serde_json::json!({
            "path": path,
            "title": title,
            "name": title,
            "game": game,
            "origin": origin,
            "origin_label": session_splash::origin_label(origin),
            "appid": appid,
        })
    }

    #[test]
    fn equipped_json_is_extracted_and_unescaped() {
        let vdf = "\t\t\t\t\"GetEquippedProfileItemsForUser76561198000000000\"\t\t\"{\\\"response\\\":{\\\"profile_background\\\":{\\\"image_large\\\":\\\"a.jpg\\\"}}}\"\n";
        let raw = extract_equipped_json(vdf).expect("значение должно найтись");
        let parsed: Value = serde_json::from_str(&raw).expect("валидный JSON");
        assert_eq!(
            parsed["response"]["profile_background"]["image_large"],
            Value::from("a.jpg")
        );
    }

    #[test]
    fn account_pick_takes_freshest_localconfig() {
        let entries = vec![
            ("0".to_string(), 999.0),
            ("111".to_string(), 10.0),
            ("222".to_string(), 20.0),
            ("не_число".to_string(), 100.0),
        ];
        assert_eq!(
            pick_account(&entries).as_deref(),
            Some((222 + STEAMID64_BASE).to_string().as_str())
        );
    }

    #[test]
    fn group_chips_hidden_when_no_choice() {
        let items = vec![movie("/a.webm", "A", "Игра", session_splash::ORIGIN_SHOP, 1)];
        let meta = MetaMap::new();
        assert!(group_chips(&items, &[], &meta, "").is_empty());
        // One group, but there are favorites: a choice appeared.
        let favs = vec!["/a.webm".to_string()];
        assert_eq!(group_chips(&items, &favs, &meta, "").len(), 3);
    }

    #[test]
    fn pager_slices_and_labels() {
        let p = bg_pager(202, 0, 5);
        assert_eq!((p.start, p.end), (0, 25));
        assert_eq!(p.pages, 9);
        assert!(!p.prev_enabled && p.next_enabled && p.visible);
        // A page beyond the list is pulled back to the last one.
        let p = bg_pager(202, 99, 5);
        assert_eq!(p.page, 8);
        assert_eq!((p.start, p.end), (200, 202));
        assert!(!p.next_enabled);
    }

    #[test]
    fn apply_format_by_extension() {
        assert_eq!(apply_format("/x/a.WEBM"), "video");
        assert_eq!(apply_format("/x/a.mp4"), "video");
        assert_eq!(apply_format("/x/a.gif"), "animated");
        assert_eq!(apply_format("/x/a.jpg"), "static");
        assert_eq!(apply_format("/x/noext"), "static");
    }

    #[test]
    fn url_ext_ignores_query() {
        assert_eq!(url_ext_or_jpg("https://h/a/b.PNG?x=1"), ".png");
        assert_eq!(url_ext_or_jpg("https://h/a/b"), ".jpg");
    }

    // ── "≥2 videos": one threshold for the showcase and for the draw at login ──

    #[test]
    fn rotation_threshold_is_one_truth() {
        use session_splash as ss;
        // The showcase refuses exactly where the draw at login has nothing to rotate.
        for favs in 0..4usize {
            for all in 0..4usize {
                let fav_paths: Vec<String> = (0..favs).map(|i| format!("/f{i}")).collect();
                let all_paths: Vec<String> = (0..all).map(|i| format!("/a{i}")).collect();
                let pool = ss::rotation_pool(&fav_paths, &all_paths);
                assert_eq!(pool.len(), ss::rotation_pool_size(favs, all), "{favs}/{all}");
                let rotates = !ss::rotation_choices(&pool, None).is_empty();
                assert_eq!(rotates, ss::rotation_possible(favs, all), "{favs}/{all}");
                assert_eq!(rotation_refusal(true, favs, all).is_some(), !rotates, "{favs}/{all}");
                assert_eq!(rotation_refusal(false, favs, all), None);
            }
        }
        assert_eq!(rotation_refusal(true, 0, 1), Some(sv_rotate_need_two()));
        assert_eq!(rotation_refusal(true, 1, 5), None);
    }

    /// The language of the module's checks is the source one (English): unit
    /// checks have no catalog, and the `msgid` is shown as is. The Russian "as it
    /// was" is checked by `tests/qt_steam_parity.rs` on the real `po/ru.po`.
    fn en() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    #[test]
    fn chip_does_not_promise_rotation_that_will_not_happen() {
        en();
        let cur = movie("/a.webm", "Days Gone", "", session_splash::ORIGIN_SHOP, 1);
        assert_eq!(
            state_chip(Some(&cur), true, true, 0, 1),
            ("✅ Ready: the next login plays “Days Gone”".to_string(), CHIP_OK)
        );
        assert!(state_chip(Some(&cur), true, true, 0, 2).0.contains("out of all (2 in the pool)"));
    }

    // ── Autostart refusal: in plain words, polite, without internals ────

    #[test]
    fn autostart_success_says_nothing() {
        assert_eq!(autostart_refusal_text(true, ""), None);
        assert_eq!(autostart_refusal_text(false, "  "), None);
    }

    #[test]
    fn autostart_refusal_names_plain_causes() {
        use session_splash as ss;
        en();
        // The "choose first" reason comes from another module (session_splash),
        // its text arrives from there as is.
        assert_eq!(
            autostart_refusal_text(true, ss::MSG_CHOOSE_FIRST),
            Some(format!("❌ {}", crate::i18n::tr(ss::MSG_CHOOSE_FIRST)))
        );
        assert_eq!(
            autostart_refusal_text(true, &ss::msg_choose_first()).as_deref(),
            Some("❌ Choose a startup movie first")
        );
        assert_eq!(
            autostart_refusal_text(true, &ss::msg_autostart_write_failed("Permission denied (os error 13)"))
                .as_deref(),
            Some("❌ Couldn't turn on the startup movie at login: no permission to write to the settings folder.")
        );
        // The first part of a system error is in the system language; recognized by the number.
        assert_eq!(
            autostart_refusal_text(true, "Не удалось включить: Нет места на устройстве (os error 28)")
                .as_deref(),
            Some("❌ Couldn't turn on the startup movie at login: the disk is full.")
        );
        // The Python host returns the exception text.
        assert_eq!(
            autostart_refusal_text(false, "[Errno 30] Read-only file system: '/x'").as_deref(),
            Some("❌ Couldn't fully turn off the startup movie at login: the settings folder is read-only.")
        );
        assert_eq!(
            autostart_refusal_text(true, &sv_no_reason()).as_deref(),
            Some("❌ Couldn't turn on the startup movie at login. Please try again.")
        );
    }

    #[test]
    fn autostart_refusal_has_no_device_words() {
        use session_splash as ss;
        en();
        let raw = [
            ss::msg_autostart_write_failed("Permission denied (os error 13)"),
            ss::msg_config_write_failed("No such file or directory (os error 2)"),
            ss::hybrid_fallback_message("юнит не встал", false, "Не удалось включить: x"),
            [
                ss::msg_problem_unit("Device or resource busy (os error 16)"),
                ss::msg_problem_ksplashrc(),
                ss::msg_problem_theme("x"),
                ss::msg_problem_autostart("x"),
            ]
            .join("; "),
            ss::msg_rollback_problems(&ss::msg_problem_unit("x")),
        ];
        let jargon = [
            "autostart", "юнит", "ksplash", "kwriteconfig", "os error", "errno", "гибрид",
            "systemd", ".desktop", "конфиг",
        ];
        for on in [true, false] {
            for r in &raw {
                let text = autostart_refusal_text(on, r).expect("отказ обязан быть сказан");
                let low = text.to_lowercase();
                for word in jargon {
                    assert!(!low.contains(word), "«{word}» в тексте покупателю: {text}");
                }
                assert!(text.starts_with("❌ "), "{text}");
            }
        }
    }

    // ── Applying a background: the shared executor behind a fake seam ───

    /// A fake machine: does nothing, only records the commands.
    #[derive(Default)]
    struct FakeSys {
        files: Vec<String>,
        commands: Vec<Vec<String>>,
        clock: f64,
    }

    impl WpSys for FakeSys {
        fn run(&mut self, argv: &[String], _timeout: f64) -> Option<session_splash::RunOut> {
            self.commands.push(argv.to_vec());
            Some(session_splash::RunOut { rc: 0, stdout: String::new(), stderr: String::new() })
        }
        fn spawn(&mut self, argv: &[String], _new_session: bool) -> Result<i64, String> {
            self.commands.push(argv.to_vec());
            Ok(self.commands.len() as i64)
        }
        fn poll(&mut self, _proc: i64) -> Option<i64> {
            None
        }
        fn pid(&mut self, proc: i64) -> Option<i64> {
            Some(1000 + proc)
        }
        fn kill_process(&mut self, _proc: i64) {}
        fn has_tool(&mut self, _tool: &str) -> bool {
            true
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
        fn is_dir(&mut self, _path: &str) -> bool {
            false
        }
        fn listdir(&mut self, _path: &str) -> Vec<String> {
            Vec::new()
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
        fn write_text(&mut self, _path: &str, _content: &str) {}
        fn remove_file(&mut self, _path: &str) {}
        fn env(&mut self, name: &str) -> String {
            if name == "HOME" { "/fake-home".into() } else { String::new() }
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 1.0;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, _live: bool, _name: &str, _announce: bool) {}
    }

    fn fake_exec(files: &[&str]) -> WallpaperExec<FakeSys> {
        let sys = FakeSys { files: files.iter().map(|f| f.to_string()).collect(), ..FakeSys::default() };
        WallpaperExec::with_env(sys, "sway", "wayland")
    }

    #[test]
    fn background_picture_goes_through_the_executor() {
        let path = "/cache/steam-bg/42.jpg";
        let mut exec = fake_exec(&[path]);
        let _ = apply_background_with(&mut exec, path, true, "Ночной город");
        let cmds = &exec.sys_mut().commands;
        assert!(
            cmds.iter().any(|argv| argv.iter().any(|a| a.contains(path))),
            "картинка должна уйти в план исполнителя: {cmds:?}"
        );
    }

    #[test]
    fn background_video_takes_the_video_route() {
        let path = "/cache/steam-bg/7.webm";
        let mut exec = fake_exec(&[path]);
        // The fake machine has no video host: the refusal comes exactly from the
        // executor's video branch, not "wallpaper engine unavailable".
        assert_eq!(
            apply_background_with(&mut exec, path, false, "Анимированный"),
            (false, crate::wallpaper_engine::video_host_missing_message())
        );
        assert_ne!(crate::wallpaper_engine::video_host_missing_message(), bg_no_engine());
    }

    #[test]
    fn background_missing_file_is_told() {
        let path = "/cache/steam-bg/нет.png";
        let mut exec = fake_exec(&[]);
        assert_eq!(
            apply_background_with(&mut exec, path, true, "x"),
            (false, crate::wallpaper_engine::file_missing_message(path))
        );
        assert!(exec.sys_mut().commands.is_empty(), "без файла ничего не запускается");
    }
}
