//! The "Razer Axon" source of the "Appearance → HD Wallpapers" section: what
//! the library shows and what things in it are called.
//!
//! Axon has no tab of its own (Wehrwolfmann's decision, 14.11.2026): it is a
//! "Razer Axon" chip next to KDE Store and Wallhaven ([`SOURCE_KEY`],
//! [`SECTIONS`]), and our settings are the "Razer Axon" block in the
//! "⚙️ Settings" view of the wallpaper section.
//!
//! The source model (the Qt side, `AxonActions`) decides nothing by itself:
//! which wallpapers get into the grid, in which order, how many filters are
//! on, which screen to show, when there is nothing to show, and all the
//! labels — all of that is here. The library is read by
//! [`crate::axon_library`]; this module does not touch Axon's files at all.
//!
//! The yardstick is the "Library" section of Razer Axon itself (2.11.1.0): a
//! tab for someone else's program reproduces the original and nothing more.
//! How it works (from the code of its interface):
//!
//! * wallpapers come in two sections: downloaded ones (heading "Recently
//!   added") and "Available to sync" — those in the account's cloud library
//!   that are not downloaded (`WallPaperStatus` = `Uninstalled`). Under the
//!   heading of the second section there is our note on why they are not on
//!   disk and who downloads them ([`available_section_note`]), plus an "Open
//!   Razer Axon" button when there is something to launch it with
//!   ([`crate::axon_library::launch_plan`]) — Wehrwolfmann's decision of
//!   15.11.2026: without the note the section read as "the tile dropped to
//!   the bottom for some reason";
//! * sorting is "by name" (the title, case-insensitive) or "by date added",
//!   which is also the default. A record has no separate date field
//!   (`FinishedTime` of downloaded ones is 0): "by date" Axon leaves the list
//!   in the order the library returns it (`WallPaperData`), and it puts newly
//!   downloaded wallpapers at its start. So "by date" = record order, newest
//!   first; the same here — the order of [`AxonLibrary`] is already like that
//!   (`read_roots` keeps the order of the most recent account);
//! * library filters: "Favorites only", "Premium", "Rewards", "With sound",
//!   "AI wallpapers" and the "Type" group (dynamic / static / interactive), a
//!   "Clear" button and the number of enabled filters on the filter button.
//!   Axon's library has no "Resolution" group — only the store catalog has
//!   one, and resolution is not part of the library selection. Our addition
//!   (Wehrwolfmann's decision of 15.11.2026) is the "Orientation" group
//!   (horizontal / vertical) by the record's resolution: some Axon
//!   wallpapers are vertical `2160x3840` videos with horizontal covers, and
//!   without a label they cannot be told apart;
//! * empty: with a search or filter active — "No wallpapers found." and a
//!   "Clear filter" link, otherwise "No items".
//!
//! Warning — fixes to the original (rule: bugs are fixed when porting, not
//! preserved):
//!   1. search: Axon splits the string at spaces, and a double space yields an
//!      empty word that matches ANY wallpaper. Empty words are skipped;
//!   2. the "Wallpaper names | Tags" search toggle in Axon's library had no
//!      effect: either way it searched the title, author and tags. Here the
//!      mode works: "Wallpaper names" means title and author, "Tags" means tags;
//!   3. the "Type" filter: for wallpapers without `EffectType` (an empty
//!      string) Axon found the empty string in the list of selected types and
//!      let them through for any selection. Wallpapers without a type do not
//!      match a type filter.

use crate::axon_library::{self as ax, display_title, find_roots, AxonEnv, AxonLibrary, AxonWallpaper};
use crate::i18n::{tr, trc, trf};
use crate::wallpaper_screens::{orientation_group_label, orientation_warning, WallpaperOrientation};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// ============================================================================
// Whether the source exists
// ============================================================================

/// The source key in "Appearance" (`appearance_page::SOURCE_AXON`).
pub const SOURCE_KEY: &str = "axon";

/// The "Appearance" sections that have the "Razer Axon" source: wallpapers only.
pub const SECTIONS: &[&str] = &["wallpapers"];

/// Whether section `section` has the source (if Axon is found — [`show_source`]).
pub fn has_section(section: &str) -> bool {
    SECTIONS.contains(&section)
}

/// Whether to show the "Razer Axon" source (the wallpaper section chip and
/// the block in its "⚙️ Settings"): yes if Razer Axon with a library is found
/// in the known places, or the person has specified a folder with its
/// wallpapers (`env.source_dirs`, from Rexon's settings). Otherwise the source
/// does not exist at all: someone without Axon has no use for it. A specified
/// folder keeps the source even if it has vanished — then the source says the
/// folder is unavailable rather than disappearing silently.
pub fn show_source(env: &AxonEnv) -> bool {
    !env.source_dirs.is_empty() || !find_roots(env).is_empty()
}

/// [`show_source`] for the live process: the environment and the folders from Rexon's settings.
pub fn show_source_live() -> bool {
    show_source(&crate::axon_prepare::live_env().0)
}

/// The label of the "Store:" chip: the program name, not translated.
pub fn chip_label() -> String {
    "Razer Axon".to_string()
}

/// The chip tooltip.
pub fn chip_tooltip() -> String {
    tr("Wallpapers you downloaded in Razer Axon on this computer. Rexon shows them \
        itself: Razer Axon doesn't have to be running.")
}

// Labels are translated in the "Razer Axon" context: the strings are
// verbatim as in Axon, and translations shared with other tabs ("Type",
// "Static"…) do not override them.

// ============================================================================
// The query
// ============================================================================

/// The grid order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortOrder {
    /// "Sort by date added", Axon's default.
    #[default]
    DateAdded,
    /// "Sort by name".
    Name,
}

impl SortOrder {
    /// All sort menu items, in the original's order.
    pub const ALL: [SortOrder; 2] = [SortOrder::Name, SortOrder::DateAdded];

    pub fn label(self) -> String {
        match self {
            SortOrder::Name => trc("Razer Axon", "Sort by name"),
            SortOrder::DateAdded => trc("Razer Axon", "Sort by date added"),
        }
    }

    /// The key for the model and the settings.
    pub fn key(self) -> &'static str {
        match self {
            SortOrder::Name => "name",
            SortOrder::DateAdded => "date",
        }
    }

    pub fn from_key(key: &str) -> SortOrder {
        if key == "name" { SortOrder::Name } else { SortOrder::DateAdded }
    }
}

/// Where to look for the search string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    /// "Wallpaper names": title and author.
    #[default]
    Titles,
    /// "Tags".
    Tags,
}

impl SearchMode {
    pub const ALL: [SearchMode; 2] = [SearchMode::Titles, SearchMode::Tags];

    pub fn label(self) -> String {
        match self {
            SearchMode::Titles => trc("Razer Axon", "Wallpaper titles"),
            SearchMode::Tags => trc("Razer Axon", "Tags"),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SearchMode::Titles => "titles",
            SearchMode::Tags => "tags",
        }
    }

    pub fn from_key(key: &str) -> SearchMode {
        if key == "tags" { SearchMode::Tags } else { SearchMode::Titles }
    }
}

/// An item of the "Type" group: Axon's `EffectType` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    Dynamic,
    Static,
    Interactive,
}

impl EffectKind {
    pub const ALL: [EffectKind; 3] = [EffectKind::Dynamic, EffectKind::Static, EffectKind::Interactive];

    /// How Axon writes the type in a record (`EffectType`).
    pub fn axon_value(self) -> &'static str {
        match self {
            EffectKind::Dynamic => "Dynamic",
            EffectKind::Static => "Static",
            EffectKind::Interactive => "Interactive",
        }
    }

    /// The key for the model: Axon's value.
    pub fn key(self) -> &'static str {
        self.axon_value()
    }

    pub fn from_axon(value: &str) -> Option<EffectKind> {
        EffectKind::ALL.into_iter().find(|k| k.axon_value().eq_ignore_ascii_case(value.trim()))
    }

    pub fn label(self) -> String {
        match self {
            EffectKind::Dynamic => trc("Razer Axon", "Dynamic"),
            EffectKind::Static => trc("Razer Axon", "Static"),
            EffectKind::Interactive => trc("Razer Axon", "Interactive"),
        }
    }
}

/// A checkbox of the library filter panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFlag {
    /// "Favorites only".
    Favorites,
    /// "Premium": wallpapers for silver.
    Premiums,
    /// "Rewards": wallpapers with a download reward.
    Rewards,
    /// "With sound".
    AudioEnabled,
    /// "AI wallpapers".
    AiWallpaper,
}

impl FilterFlag {
    /// The checkboxes in the order of the original's panel (followed by the "Type" group).
    pub const ALL: [FilterFlag; 5] = [
        FilterFlag::Favorites,
        FilterFlag::Premiums,
        FilterFlag::Rewards,
        FilterFlag::AudioEnabled,
        FilterFlag::AiWallpaper,
    ];

    /// The key for the model.
    pub fn key(self) -> &'static str {
        match self {
            FilterFlag::Favorites => "favorites",
            FilterFlag::Premiums => "premiums",
            FilterFlag::Rewards => "rewards",
            FilterFlag::AudioEnabled => "audio",
            FilterFlag::AiWallpaper => "ai",
        }
    }

    pub fn from_key(key: &str) -> Option<FilterFlag> {
        FilterFlag::ALL.into_iter().find(|f| f.key() == key)
    }

    pub fn label(self) -> String {
        match self {
            FilterFlag::Favorites => trc("Razer Axon", "Favorites"),
            FilterFlag::Premiums => trc("Razer Axon", "Premiums"),
            FilterFlag::Rewards => trc("Razer Axon", "Rewards"),
            FilterFlag::AudioEnabled => trc("Razer Axon", "Audio-enabled"),
            FilterFlag::AiWallpaper => trc("Razer Axon", "AI wallpaper"),
        }
    }

    /// Whether a wallpaper matches a checkbox, with the same attributes as Axon:
    /// favorite (`IsFavorite`), silver above zero (`Silver`), a download reward
    /// above zero, sound (`Audible` = 1), an AI creation number (`DetailId`).
    pub fn accepts(self, w: &AxonWallpaper) -> bool {
        let d = &w.details;
        match self {
            FilterFlag::Favorites => d.is_favorite,
            FilterFlag::Premiums => d.silver > 0,
            FilterFlag::Rewards => d.download_reward > 0,
            FilterFlag::AudioEnabled => w.audible,
            FilterFlag::AiWallpaper => d.is_ai(),
        }
    }
}

/// The state of the filter panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// Enabled checkboxes (without duplicates).
    pub flags: Vec<FilterFlag>,
    /// Checked "Type" items: wallpapers of any of them match.
    pub kinds: Vec<EffectKind>,
    /// Checked "Orientation" items (our group): wallpapers of any of them
    /// match.
    pub orientations: Vec<WallpaperOrientation>,
}

impl Filters {
    pub fn has(&self, flag: FilterFlag) -> bool {
        self.flags.contains(&flag)
    }

    pub fn has_kind(&self, kind: EffectKind) -> bool {
        self.kinds.contains(&kind)
    }

    pub fn set(&mut self, flag: FilterFlag, on: bool) {
        self.flags.retain(|f| *f != flag);
        if on {
            self.flags.push(flag);
        }
    }

    pub fn set_kind(&mut self, kind: EffectKind, on: bool) {
        self.kinds.retain(|k| *k != kind);
        if on {
            self.kinds.push(kind);
        }
    }

    pub fn has_orientation(&self, o: WallpaperOrientation) -> bool {
        self.orientations.contains(&o)
    }

    pub fn set_orientation(&mut self, o: WallpaperOrientation, on: bool) {
        self.orientations.retain(|x| *x != o);
        if on {
            self.orientations.push(o);
        }
    }

    /// The number on the filter button: each checkbox and each "Type" item counts
    /// as one, as Axon counts; "Orientation" items count the same way.
    pub fn active_count(&self) -> usize {
        self.flags.len() + self.kinds.len() + self.orientations.len()
    }

    /// Whether anything is enabled (then the "Clear" button is available).
    pub fn any(&self) -> bool {
        self.active_count() > 0
    }

    /// "Clear".
    pub fn clear(&mut self) {
        *self = Filters::default();
    }

    /// Whether a wallpaper matches all enabled filters.
    pub fn accepts(&self, w: &AxonWallpaper) -> bool {
        if !self.flags.iter().all(|f| f.accepts(w)) {
            return false;
        }
        // Fix 3: without `EffectType` a wallpaper does not match a type filter.
        let kind_ok =
            self.kinds.is_empty() || EffectKind::from_axon(&w.effect).is_some_and(|k| self.kinds.contains(&k));
        // Without a resolution the orientation is unknown — such wallpapers
        // do not match an orientation filter (like untyped ones a type filter).
        kind_ok && (self.orientations.is_empty() || ax::orientation(w).is_some_and(|o| self.orientations.contains(&o)))
    }
}

/// Everything that defines the grid: the search string, its mode, the sorting, the filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub mode: SearchMode,
    pub sort: SortOrder,
    pub filters: Filters,
}

impl Query {
    /// Whether the query narrows the list (search or filter): then an empty result
    /// is "No wallpapers found." rather than "No items".
    pub fn narrows(&self) -> bool {
        !self.text.trim().is_empty() || self.filters.any()
    }

    /// The "Clear filter" link: removes the filters and the search, keeps the
    /// sorting and the search mode.
    pub fn clear_filter(&mut self) {
        self.text.clear();
        self.filters.clear();
    }
}

/// Whether the search text matches: any word of the string is part of the
/// title or author ("Wallpaper names") or of the tags ("Tags"),
/// case-insensitively.
pub fn matches_text(w: &AxonWallpaper, text: &str, mode: SearchMode) -> bool {
    // Fix 1: empty words (a double space) do not match everything.
    let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return true;
    }
    // Fix 2: the search mode takes effect.
    let hay = match mode {
        SearchMode::Titles => format!("{}\n{}", w.title, w.author),
        SearchMode::Tags => w.tags.clone(),
    }
    .to_lowercase();
    words.iter().any(|word| hay.contains(word.as_str()))
}

/// Whether a wallpaper goes into the "Available to sync" section: Axon puts
/// cloud library records that are not on disk (`Uninstalled`) there.
pub fn is_available_for_sync(w: &AxonWallpaper) -> bool {
    w.status.trim().eq_ignore_ascii_case("uninstalled")
}

/// The grid for a query: wallpaper indices in `list`, by section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryView {
    /// Downloaded ones (the "Recently added" section).
    pub downloaded: Vec<usize>,
    /// "Available to sync".
    pub available: Vec<usize>,
}

impl LibraryView {
    pub fn is_empty(&self) -> bool {
        self.downloaded.is_empty() && self.available.is_empty()
    }
}

/// The "by name" sort key: the title as the person sees it, case-insensitive.
fn name_key(w: &AxonWallpaper) -> String {
    display_title(w).to_lowercase()
}

/// Selects and orders the library wallpapers according to the query.
pub fn query(list: &[AxonWallpaper], q: &Query) -> LibraryView {
    let mut order: Vec<usize> = (0..list.len())
        .filter(|&i| matches_text(&list[i], &q.text, q.mode) && q.filters.accepts(&list[i]))
        .collect();
    if q.sort == SortOrder::Name {
        // Stable: identical titles stay in the order they were added.
        order.sort_by_cached_key(|&i| name_key(&list[i]));
    }
    let (available, downloaded) = order.into_iter().partition(|&i| is_available_for_sync(&list[i]));
    LibraryView { downloaded, available }
}

/// What to show in place of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The grid: there is something to show.
    Grid,
    /// The search or filter found nothing: "No wallpapers found." + "Clear filter".
    NotFound,
    /// Nothing to show and nothing filtered: "No items".
    NoItems,
}

pub fn screen(view: &LibraryView, q: &Query) -> Screen {
    if !view.is_empty() {
        Screen::Grid
    } else if q.narrows() {
        Screen::NotFound
    } else {
        Screen::NoItems
    }
}

// ============================================================================
// Labels: verbatim as in Axon
// ============================================================================

/// The search field placeholder.
pub fn search_placeholder() -> String {
    trc("Razer Axon", "Search wallpapers")
}

/// The label of the "where to search" choice (names or tags) in the
/// "Filter": the search field itself is the shared field in the page header,
/// as with the neighboring stores.
pub fn search_in_label() -> String {
    trc("Razer Axon", "Search in")
}

/// The sort button tooltip.
pub fn sort_tooltip() -> String {
    trc("Razer Axon", "Sort")
}

/// The tooltip and heading of the "Type" group.
pub fn type_label() -> String {
    trc("Razer Axon", "Type")
}

/// The filter panel's "Clear" button, in capitals as in Axon.
pub fn clear_text() -> String {
    trc("Razer Axon", "Clear").to_uppercase()
}

/// The heading of the downloaded wallpapers section, in capitals as in Axon.
pub fn downloaded_section_title() -> String {
    trc("Razer Axon", "Recently added").to_uppercase()
}

/// The heading of the section of cloud wallpapers not downloaded yet.
pub fn available_section_title() -> String {
    trc("Razer Axon", "AVAILABLE FOR SYNC")
}

/// The note under the "Available to sync" heading (ours, Axon has none):
/// what these wallpapers are and why Rexon does not download them itself.
pub fn available_section_note() -> String {
    tr("Available in your Razer account, but not downloaded to this computer. \
        Only Razer Axon can download them.")
}

/// The button under the note: launch Razer Axon.
pub fn launch_text() -> String {
    tr("Open Razer Axon")
}

/// Result of the "Open Razer Axon" button: started.
pub fn launch_started_text() -> String {
    tr("Razer Axon is starting…")
}

/// Result of the "Open Razer Axon" button: failed to start.
pub fn launch_failed_text(why: &str) -> String {
    // TRANSLATORS: %1 is the reason from the system, e.g. "No such file or directory".
    trf("Couldn't start Razer Axon: %1", &[&why])
}

/// Empty, nothing filtered.
pub fn no_items_text() -> String {
    trc("Razer Axon", "No items")
}

/// The search or filter found nothing.
pub fn not_found_text() -> String {
    trc("Razer Axon", "No wallpapers found.")
}

/// The link next to "No wallpapers found.".
pub fn clear_filter_text() -> String {
    trc("Razer Axon", "Clear filter")
}

/// The filter panel button: "FILTER", and with the number of enabled filters
/// "FILTER (2)" (in capitals in Axon).
pub fn filter_button_text(active: usize) -> String {
    let label = trc("Razer Axon", "FILTER");
    if active == 0 {
        label
    } else {
        format!("{label} ({active})")
    }
}

/// The tile menu item "Apply" (Apply in Axon).
pub fn apply_text() -> String {
    trc("Razer Axon", "Apply")
}

/// The tile menu item "Go to file location" (Open file location in Axon).
pub fn open_location_text() -> String {
    trc("Razer Axon", "Open file location")
}

/// The tile menu item and the title of the details window. In Axon the
/// details open with a click on the tile; the menu item is ours, for the
/// keyboard.
pub fn details_text() -> String {
    tr("Details")
}

/// The tile tooltip: what the clicks do.
pub fn tile_tooltip() -> String {
    tr("Double-click — details; right click — menu")
}

/// The tooltip of the "Apply" item while no monitor has been chosen.
pub fn apply_tooltip() -> String {
    tr("On all screens")
}

/// The heading of the "Razer Axon" block in the "⚙️ Settings" of the wallpaper section.
pub fn settings_title() -> String {
    chip_label()
}

/// The block's explanation: these are Rexon's settings, not Axon's own.
pub fn settings_description() -> String {
    tr("How Rexon shows wallpapers from Razer Axon. The settings of Razer Axon itself \
        are not changed here.")
}

// ============================================================================
// The model's answer: screen, grid, details
// ============================================================================

/// Where a tile gets its cover from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverPlan {
    /// The picture is already on disk.
    Ready(PathBuf),
    /// Download from the address into the shared "Appearance" preview cache.
    Download(String),
    /// Nowhere to get it from: a placeholder.
    Missing,
}

/// A network address (`http(s)://`).
pub fn is_web_url(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s.starts_with("https://") || s.starts_with("http://")
}

/// The file is an ordinary picture that `Image` reads (JPEG, PNG, GIF, WebP,
/// BMP). An Axon container (ZIP, `PK…`) and a video (`ftyp`) are not: a frame
/// cannot be taken from them without preparation.
pub fn is_plain_image(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let h = &head[..n];
    h.starts_with(&[0xFF, 0xD8, 0xFF])
        || h.starts_with(b"\x89PNG\r\n\x1a\n")
        || h.starts_with(b"GIF87a")
        || h.starts_with(b"GIF89a")
        || h.starts_with(b"BM")
        || (h.starts_with(b"RIFF") && h.get(8..12) == Some(b"WEBP"))
}

/// Where to get the wallpaper cover from.
///
/// Axon does not put its own cover on disk (`WallpaperCovers` is empty): in a
/// library record it is a CDN address (`ThumbnailSource`, JPEG 500×280,
/// otherwise `PreviewSource`, WebP 960×540). The order:
/// 1. a cover on disk ([`AxonWallpaper::cover_local`]);
/// 2. a network address: an already downloaded copy from the cache (`cached`),
///    otherwise download it. The same cache and the same download as for the
///    KDE Store and Wallhaven tiles;
/// 3. without an address (wallpapers only from a folder): the wallpaper file
///    itself, if it is an ordinary picture rather than an Axon container.
pub fn cover_plan(w: &AxonWallpaper, cached: &dyn Fn(&str) -> Option<PathBuf>) -> CoverPlan {
    if let Some(p) = w.cover_local.as_ref().filter(|p| p.is_file()) {
        return CoverPlan::Ready(p.clone());
    }
    let url = w.cover_url.trim();
    if is_web_url(url) {
        return match cached(url) {
            Some(p) => CoverPlan::Ready(p),
            None => CoverPlan::Download(url.to_string()),
        };
    }
    match w.container.as_ref().filter(|c| c.is_file() && is_plain_image(c)) {
        Some(c) => CoverPlan::Ready(c.clone()),
        None => CoverPlan::Missing,
    }
}

/// `file://` for a path: bytes outside the "safe" set are percent-encoded,
/// otherwise a space, `#` or `%` in the path break the address for `Image`.
pub fn file_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The cover address for QML: `file://` of a picture on disk, otherwise empty
/// (a placeholder). What to put into [`AxonWallpaper::cover_local`] is
/// decided by [`cover_plan`] on the model side.
fn cover_url(w: &AxonWallpaper) -> String {
    w.cover_local.as_ref().filter(|p| p.is_file()).map(|p| file_url(p)).unwrap_or_default()
}

fn folder_of(w: &AxonWallpaper) -> String {
    w.resource_dir.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A grid tile.
pub fn tile(w: &AxonWallpaper) -> Value {
    json!({
        "id": w.id,
        "title": display_title(w),
        "author": w.author.trim(),
        "line": ax::tile_line(w),
        "cover": cover_url(w),
        "downloaded": w.downloaded,
        "playing": w.playing,
        "folder": folder_of(w),
    })
}

/// What to say instead of the grid when the library cannot be read: if the
/// specified folder has vanished, that it is gone; otherwise that Axon was not
/// found and a folder can be specified. `None` means the library was read.
pub fn library_note(lib: &AxonLibrary) -> Option<String> {
    if lib.found {
        return None;
    }
    if !lib.source_dirs.is_empty() && lib.source_dirs.iter().all(|d| !d.is_dir()) {
        return Some(ax::folder_missing_text());
    }
    Some(ax::not_found_text())
}

/// The screen key for the model.
pub fn screen_key(screen: Screen) -> &'static str {
    match screen {
        Screen::Grid => "grid",
        Screen::NotFound => "notfound",
        Screen::NoItems => "noitems",
    }
}

/// The model's answer to a query: the screen (`note` — no library, `grid`,
/// `notfound`, `noitems`), its label, the grid sections with tiles (the
/// section of not downloaded ones carries a `note`) and the filter button
/// with the number of enabled filters.
pub fn view_json(lib: &AxonLibrary, q: &Query) -> Value {
    let filter = json!({
        "text": filter_button_text(q.filters.active_count()),
        "count": q.filters.active_count(),
        "flags": q.filters.flags.iter().map(|f| f.key()).collect::<Vec<_>>(),
        "kinds": q.filters.kinds.iter().map(|k| k.key()).collect::<Vec<_>>(),
        "orientations": q.filters.orientations.iter().map(|o| o.key()).collect::<Vec<_>>(),
    });
    let base = json!({"sort": q.sort.key(), "mode": q.mode.key(), "text": q.text});
    if let Some(note) = library_note(lib) {
        return json!({"screen": "note", "note": note, "sections": [], "filter": filter, "query": base});
    }
    let v = query(&lib.wallpapers, q);
    let scr = screen(&v, q);
    let tiles = |idx: &[usize]| idx.iter().map(|&i| tile(&lib.wallpapers[i])).collect::<Vec<_>>();
    let mut sections = Vec::new();
    if !v.downloaded.is_empty() {
        sections.push(json!({"title": downloaded_section_title(), "note": "", "items": tiles(&v.downloaded)}));
    }
    if !v.available.is_empty() {
        sections.push(json!({
            "title": available_section_title(),
            "note": available_section_note(),
            "items": tiles(&v.available),
        }));
    }
    let note = match scr {
        Screen::Grid => String::new(),
        Screen::NotFound => not_found_text(),
        Screen::NoItems => no_items_text(),
    };
    json!({"screen": screen_key(scr), "note": note, "sections": sections, "filter": filter, "query": base})
}

/// Wallpaper details: cover, title, author, the tile line, "label — value"
/// lines (type and tags with Axon's labels, orientation, resolution) and,
/// for vertical ones, what happens on a regular monitor (`warning`).
pub fn details_json(w: &AxonWallpaper) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut row = |label: String, value: String| {
        if !value.trim().is_empty() {
            rows.push(json!({"label": label, "value": value}));
        }
    };
    row(type_label(), EffectKind::from_axon(&w.effect).map(EffectKind::label).unwrap_or_default());
    row(SearchMode::Tags.label(), ax::tag_list(&w.tags).join(", "));
    let orientation = ax::orientation(w);
    row(orientation_group_label(), orientation.map(WallpaperOrientation::label).unwrap_or_default());
    row(tr("Resolution"), w.resolution.map(|(x, y)| format!("{x} × {y}")).unwrap_or_default());
    // Videos are shown by the Rexon video host, which fills the screen and
    // crops; pictures and web are set by the desktop or waywallen — it varies.
    let crops = w.kind == ax::AxonKind::Video;
    let warning = orientation.and_then(|o| orientation_warning(o, crops)).unwrap_or_default();
    let author = w.author.trim();
    json!({
        "id": w.id,
        "title": display_title(w),
        "author": if author.is_empty() { String::new() } else { trf("Author: %1", &[&author]) },
        "line": ax::tile_line(w),
        "cover": cover_url(w),
        "description": w.description.trim(),
        "rows": rows,
        "warning": warning,
        "folder": folder_of(w),
        "downloaded": w.downloaded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axon_library::{AxonDetails, AxonKind};
    use std::path::PathBuf;

    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn w(id: &str, title: &str) -> AxonWallpaper {
        AxonWallpaper {
            id: id.into(),
            title: title.into(),
            author: format!("Автор {id}"),
            kind: AxonKind::Video,
            effect: "Dynamic".into(),
            audible: false,
            resolution: Some((3840, 2160)),
            cover_url: String::new(),
            cover_local: None,
            resource_dir: None,
            container: None,
            downloaded: true,
            playing: false,
            status: "Installed".into(),
            description: String::new(),
            tags: String::new(),
            details: AxonDetails::default(),
            prefix: PathBuf::new(),
        }
    }

    /// The library in Axon's order: newest first.
    fn library() -> Vec<AxonWallpaper> {
        let mut a = w("1", "skyline");
        a.tags = " City,Night".into();
        a.audible = true;
        a.details.is_favorite = true;
        let mut b = w("2", "Aurora");
        b.effect = "Static".into();
        b.details.silver = 120;
        let mut c = w("3", "Neon");
        c.status = "Uninstalled".into();
        c.downloaded = false;
        c.details.detail_id = 7;
        c.effect = String::new();
        let mut d = w("4", "Beach");
        d.effect = "Interactive".into();
        d.details.download_reward = 5;
        d.tags = "Sea".into();
        vec![a, b, c, d]
    }

    fn ids(list: &[AxonWallpaper], idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| list[i].id.clone()).collect()
    }

    #[test]
    fn date_added_keeps_library_order_and_splits_sections() {
        let lib = library();
        let v = query(&lib, &Query::default());
        assert_eq!(ids(&lib, &v.downloaded), ["1", "2", "4"]);
        assert_eq!(ids(&lib, &v.available), ["3"], "не скачанные — в «Можно синхронизировать»");
        assert_eq!(SortOrder::default(), SortOrder::DateAdded, "по умолчанию — по дате");
    }

    #[test]
    fn name_sort_ignores_case() {
        let lib = library();
        let q = Query { sort: SortOrder::Name, ..Query::default() };
        let v = query(&lib, &q);
        assert_eq!(ids(&lib, &v.downloaded), ["2", "4", "1"], "Aurora, Beach, skyline");
        assert_eq!(SortOrder::from_key(SortOrder::Name.key()), SortOrder::Name);
        assert_eq!(SortOrder::from_key("что-то"), SortOrder::DateAdded);
    }

    #[test]
    fn search_by_titles_and_by_tags() {
        let lib = library();
        let find = |text: &str, mode| {
            let v = query(&lib, &Query { text: text.into(), mode, ..Query::default() });
            ids(&lib, &v.downloaded)
        };
        assert_eq!(find("SKY", SearchMode::Titles), ["1"]);
        assert_eq!(find("автор 4", SearchMode::Titles), ["1", "2", "4"], "любое слово: «автор» есть у всех");
        assert_eq!(find("aurora  beach", SearchMode::Titles), ["2", "4"], "двойной пробел не находит всё");
        assert_eq!(find("night", SearchMode::Titles), Vec::<String>::new(), "теги — не в режиме названий");
        assert_eq!(find("night", SearchMode::Tags), ["1"]);
        assert_eq!(find("sea", SearchMode::Tags), ["4"]);
        assert_eq!(find("   ", SearchMode::Tags), ["1", "2", "4"], "пустой поиск не сужает");
    }

    #[test]
    fn filters_flags_kinds_and_counter() {
        let lib = library();
        let run = |f: &Filters| {
            let v = query(&lib, &Query { filters: f.clone(), ..Query::default() });
            (ids(&lib, &v.downloaded), ids(&lib, &v.available))
        };
        let mut f = Filters::default();
        assert_eq!((f.active_count(), f.any()), (0, false));
        f.set(FilterFlag::Favorites, true);
        assert_eq!(run(&f).0, ["1"]);
        f.set(FilterFlag::Favorites, true);
        assert_eq!(f.active_count(), 1, "повторное включение не считается дважды");
        f.set(FilterFlag::AudioEnabled, true);
        assert_eq!((run(&f).0, f.active_count()), (vec!["1".to_string()], 2));
        f.clear();
        f.set(FilterFlag::Premiums, true);
        assert_eq!(run(&f).0, ["2"]);
        f.clear();
        f.set(FilterFlag::Rewards, true);
        assert_eq!(run(&f).0, ["4"]);
        f.clear();
        f.set(FilterFlag::AiWallpaper, true);
        assert_eq!(run(&f), (vec![], vec!["3".to_string()]), "ИИ-обои — по номеру творения");
        f.clear();
        f.set_kind(EffectKind::Static, true);
        f.set_kind(EffectKind::Interactive, true);
        assert_eq!((run(&f).0, f.active_count()), (vec!["2".to_string(), "4".to_string()], 2));
        assert!(run(&f).1.is_empty(), "без типа под фильтр типа не подходят");
        f.set_kind(EffectKind::Static, false);
        assert_eq!(run(&f).0, ["4"]);
        assert_eq!(EffectKind::from_axon(" dynamic "), Some(EffectKind::Dynamic));
        assert_eq!(EffectKind::from_axon(""), None);
    }

    #[test]
    fn screens_and_clear_filter() {
        let lib = library();
        let mut q = Query { text: "нет таких".into(), sort: SortOrder::Name, mode: SearchMode::Tags, ..Query::default() };
        q.filters.set(FilterFlag::Favorites, true);
        assert_eq!(screen(&query(&lib, &q), &q), Screen::NotFound);
        q.clear_filter();
        assert_eq!((q.text.as_str(), q.filters.any()), ("", false));
        assert_eq!((q.sort, q.mode), (SortOrder::Name, SearchMode::Tags), "сортировка и режим остаются");
        assert_eq!(screen(&query(&lib, &q), &q), Screen::Grid);
        assert_eq!(screen(&query(&[], &q), &q), Screen::NoItems);
    }

    #[test]
    fn view_answer_has_sections_screens_and_filter_button() {
        english();
        let lib = AxonLibrary { wallpapers: library(), found: true, ..AxonLibrary::default() };
        let mut q = Query::default();
        let v = view_json(&lib, &q);
        assert_eq!(v["screen"], "grid");
        assert_eq!(v["sections"][0]["title"], "RECENTLY ADDED");
        assert_eq!(v["sections"][0]["note"], "", "nothing to explain for downloaded ones");
        assert_eq!(v["sections"][0]["items"].as_array().unwrap().len(), 3);
        assert_eq!(v["sections"][1]["title"], "AVAILABLE FOR SYNC");
        assert_eq!(v["sections"][1]["note"], available_section_note().as_str());
        assert!(available_section_note().contains("Only Razer Axon"));
        assert_eq!(v["sections"][1]["items"][0]["id"], "3");
        assert_eq!(v["filter"]["text"], "FILTER");
        q.filters.set(FilterFlag::Favorites, true);
        q.filters.set_kind(EffectKind::Static, true);
        let v = view_json(&lib, &q);
        assert_eq!((v["screen"].as_str(), v["note"].as_str()), (Some("notfound"), Some("No wallpapers found.")));
        assert_eq!(v["filter"]["text"], "FILTER (2)");
        assert_eq!(v["filter"]["flags"][0], "favorites");
        assert_eq!(v["filter"]["kinds"][0], "Static");
        let empty = AxonLibrary { found: true, ..AxonLibrary::default() };
        assert_eq!(view_json(&empty, &Query::default())["screen"], "noitems");
        // No library: a screen suggesting to specify a folder; a specified folder
        // has vanished: that it is gone.
        let none = AxonLibrary::default();
        assert_eq!(view_json(&none, &Query::default())["screen"], "note");
        assert_eq!(library_note(&none), Some(ax::not_found_text()));
        let gone = AxonLibrary { source_dirs: vec![PathBuf::from("/нет/такой/папки")], ..AxonLibrary::default() };
        assert_eq!(library_note(&gone), Some(ax::folder_missing_text()));
        // Filter keys there and back.
        for f in FilterFlag::ALL {
            assert_eq!(FilterFlag::from_key(f.key()), Some(f));
        }
        for k in EffectKind::ALL {
            assert_eq!(EffectKind::from_axon(k.key()), Some(k));
        }
        let d = details_json(&lib.wallpapers[0]);
        assert_eq!(d["title"], "skyline");
        assert_eq!(d["rows"][0]["value"], "Dynamic");
        assert_eq!(d["rows"][1]["value"], "City, Night");
        assert_eq!(d["rows"][2]["value"], "Horizontal");
        assert_eq!(d["rows"][3]["value"], "3840 × 2160");
        assert_eq!(d["warning"], "", "nothing to warn about for horizontal ones");
        assert!(has_section("wallpapers") && !has_section("icons"));
    }

    #[test]
    fn labels_are_the_originals() {
        english();
        assert_eq!(SortOrder::ALL.map(SortOrder::label), ["Sort by name", "Sort by date added"]);
        assert_eq!(SearchMode::ALL.map(SearchMode::label), ["Wallpaper titles", "Tags"]);
        assert_eq!(
            FilterFlag::ALL.map(FilterFlag::label),
            ["Favorites", "Premiums", "Rewards", "Audio-enabled", "AI wallpaper"]
        );
        assert_eq!(EffectKind::ALL.map(EffectKind::label), ["Dynamic", "Static", "Interactive"]);
        assert_eq!(clear_text(), "CLEAR");
        assert_eq!(downloaded_section_title(), "RECENTLY ADDED");
        assert_eq!(available_section_title(), "AVAILABLE FOR SYNC");
        assert_eq!(
            [search_placeholder(), search_in_label(), sort_tooltip(), type_label(), no_items_text(), not_found_text(), clear_filter_text()],
            ["Search wallpapers", "Search in", "Sort", "Type", "No items", "No wallpapers found.", "Clear filter"]
        );
    }

    /// Axon records "2160x3840" (Skyline GTR, VHS Mirage, Binary Twilight)
    /// are vertical videos: tile label, filter and warning.
    #[test]
    fn vertical_wallpapers_are_labelled_filtered_and_warned() {
        english();
        let mut tall = w("5", "Skyline GTR");
        tall.resolution = ax::parse_resolution("2160x3840");
        assert_eq!(ax::orientation(&tall), Some(WallpaperOrientation::Portrait));
        assert_eq!(ax::tile_line(&tall), "🎞️ Video · Vertical · 2160×3840");
        let wide = w("6", "Aurora");
        assert_eq!(ax::tile_line(&wide), "🎞️ Video · 3840×2160", "horizontal ones — unchanged");
        let mut unknown = w("7", "No resolution");
        unknown.resolution = None;
        assert_eq!(ax::orientation(&unknown), None);

        let lib = vec![wide.clone(), tall.clone(), unknown];
        let run = |f: &Filters| ids(&lib, &query(&lib, &Query { filters: f.clone(), ..Query::default() }).downloaded);
        let mut f = Filters::default();
        f.set_orientation(WallpaperOrientation::Portrait, true);
        assert_eq!((run(&f), f.active_count()), (vec!["5".to_string()], 1));
        f.set_orientation(WallpaperOrientation::Landscape, true);
        assert_eq!(run(&f), ["6", "5"], "both orientations; no resolution — no match");
        f.set_orientation(WallpaperOrientation::Portrait, false);
        assert_eq!(run(&f), ["6"]);
        f.clear();
        assert!(f.orientations.is_empty() && !f.any());

        let d = details_json(&tall);
        let rows = d["rows"].as_array().unwrap();
        assert!(rows.iter().any(|r| r["label"] == "Orientation" && r["value"] == "Vertical"), "{rows:?}");
        let warn = d["warning"].as_str().unwrap();
        assert!(warn.contains("only the middle part"), "the video host crops videos: {warn}");
        let mut pic = tall.clone();
        pic.kind = AxonKind::Image;
        assert!(details_json(&pic)["warning"].as_str().unwrap().contains("bars"), "the desktop fits pictures");
        assert_eq!(details_json(&wide)["warning"], "");
    }

    /// A temporary check directory, removed on exit.
    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("rexon-axon-cover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }

    /// A layout like Axon's records on a live machine: `ThumbnailSource` is a
    /// JPEG on the CDN, `WallpaperCovers` is empty, `Resource/` is a ZIP container
    /// (`PK…`) or an already prepared video (`ftyp`), a picture from a folder
    /// (`FF D8 FF`).
    #[test]
    fn cover_plan_follows_axon_data() {
        let t = tmp("plan");
        let write = |name: &str, head: &[u8]| {
            let p = t.0.join(name);
            std::fs::write(&p, head).unwrap();
            p
        };
        let zip = write("3840x2160.mp4", b"PK\x03\x04\x2d\x00\x09\x08");
        let mp4 = write("ready.mp4", b"\x00\x00\x00\x18ftypmp42");
        let jpg = write("3840x2400.jpg", b"\xFF\xD8\xFF\xE1\x00\x18Exif");
        let webp = write("x.webp", b"RIFF\x10\x00\x00\x00WEBPVP8 ");
        let cdn = "https://axon-assets-cdn.razerzone.com/thumbnail/x/1.0.0/Sky_Thumb_500x280.jpg";
        let none = |_: &str| None;

        // Only a CDN address, the container is a ZIP: download.
        let mut a = w("1", "a");
        a.cover_url = cdn.into();
        a.container = Some(zip.clone());
        assert_eq!(cover_plan(&a, &none), CoverPlan::Download(cdn.into()));
        // Already in the preview cache: ready, without the network.
        let cached = jpg.clone();
        let hit = move |u: &str| (u == cdn).then(|| cached.clone());
        assert_eq!(cover_plan(&a, &hit), CoverPlan::Ready(jpg.clone()));
        // Our own cover on disk takes precedence over the address.
        a.cover_local = Some(webp.clone());
        assert_eq!(cover_plan(&a, &none), CoverPlan::Ready(webp.clone()));
        // A vanished cover file: the address again.
        a.cover_local = Some(t.0.join("нет.jpg"));
        assert_eq!(cover_plan(&a, &none), CoverPlan::Download(cdn.into()));

        // Without an address: a picture from the folder will do, a container and a video will not.
        let mut b = w("2", "b");
        b.container = Some(jpg.clone());
        assert_eq!(cover_plan(&b, &none), CoverPlan::Ready(jpg.clone()));
        b.container = Some(zip);
        assert_eq!(cover_plan(&b, &none), CoverPlan::Missing);
        b.container = Some(mp4);
        assert_eq!(cover_plan(&b, &none), CoverPlan::Missing);
        // A non-network address (a relative path) does not count as an address.
        b.cover_url = "Resource\\cover.jpg".into();
        assert_eq!(cover_plan(&b, &none), CoverPlan::Missing);
    }

    #[test]
    fn tile_cover_is_encoded_file_url() {
        let t = tmp("url");
        let p = t.0.join("обои #1 50%.jpg");
        std::fs::write(&p, b"\xFF\xD8\xFF").unwrap();
        let mut a = w("1", "a");
        assert_eq!(tile(&a)["cover"], "");
        a.cover_local = Some(p.clone());
        let url = tile(&a)["cover"].as_str().unwrap().to_string();
        assert!(url.starts_with("file:///"), "{url}");
        assert!(url.ends_with("/%D0%BE%D0%B1%D0%BE%D0%B8%20%231%2050%25.jpg"), "{url}");
        assert_eq!(details_json(&a)["cover"], url.as_str());
        assert!(is_web_url("HTTPS://x/y.jpg") && is_web_url("http://x") && !is_web_url("file:///x"));
    }
}
