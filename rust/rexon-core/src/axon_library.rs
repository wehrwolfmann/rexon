//! The wallpaper library of Razer Axon installed under Wine.
//!
//! Razer Axon is a live wallpaper program for Windows. On Linux it is installed into
//! a Wine prefix (its own `~/.wine`, a Bottles bottle, a Lutris or
//! Proton prefix), and the wallpapers downloaded in it live inside the prefix. The module only
//! READS this library: which wallpapers the person has, what they are called,
//! who the author is, whether it is a video or a picture, whether there is sound, whether they are downloaded and where
//! their files are. This module itself never changes the Axon prefix — no
//! writing, no renaming, no deleting; preparing wallpapers in advance right in
//! the Axon library (with the person's consent, with a backup of the description)
//! is done by a separate module, [`crate::axon_prepare`].
//!
//! The layout inside the prefix (`<prefix>/drive_c/users/<user>/AppData/Local/
//! Razer/RazerAxon/`):
//!
//! * `<account>/WallPaperData` — the library JSON array (UTF-8 with BOM):
//!   `Id`, `Title`, `Author.author_name`, `Type` or `WallPaperType`
//!   (`VIDEO`/`IMAGE`/`WEB`…), `EffectType`, `Audible` (`"1"`/`"0"`),
//!   `Resolution` (`3840x2160`), `WallPaperStatus`, `PlayResoucePath`
//!   (that is how Axon itself spells it, with a typo), `ThumbnailSource`, `PreviewSource`;
//! * `<account>/PlayListSetting` — playlists per screen: which wallpapers and
//!   which of them are on screen now;
//! * `<account>/UserSetting` — `SourceSavedPath`, where the downloads are;
//! * `WallpaperSource/{Id}{Height}{Width}/` — downloaded wallpapers:
//!   `ResourceConfig.txt` (the kind and the file path) and `Resource/…` — a container
//!   that must be prepared before showing: this is the job of [`AxonExtractor`]
//!   (the built-in one — [`BuiltinExtractor`], `crate::axon_extract`).
//!
//! There can be several accounts in one directory (`RZR_<uid>` after
//! logging in, `guest-<…>` before): all that have a library are taken, and
//! identical wallpapers come from the freshest one.
//!
//! The module will not search for prefixes across the whole disk: it looks in known places
//! (`$WINEPREFIX`, `~/.wine`, Bottles, Lutris, Heroic, umu, PlayOnLinux,
//! Proton prefixes in Steam libraries) one level deep.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::i18n::{tr, trf};

/// The path from the Windows user directory to the Axon root.
pub const AXON_SUBDIR: &[&str] = &["AppData", "Local", "Razer", "RazerAxon"];
/// The library file in the account directory.
pub const LIBRARY_FILE: &str = "WallPaperData";
/// The account's playlists.
pub const PLAYLIST_FILE: &str = "PlayListSetting";
/// The account's settings (`SourceSavedPath`).
pub const USER_SETTING_FILE: &str = "UserSetting";
/// The description of downloaded wallpapers in their directory.
pub const RESOURCE_CONFIG: &str = "ResourceConfig.txt";
/// The default downloads directory (when `UserSetting` is silent).
pub const DEFAULT_SOURCE_DIR: &str = "WallpaperSource";


/// The mark of a ready file in the cache directory.
pub const READY_STAMP: &str = "ready.json";

// ============================================================================
// Environment
// ============================================================================

/// Everything the module takes from the process environment. Checks substitute their own.
#[derive(Debug, Clone, Default)]
pub struct AxonEnv {
    pub home: PathBuf,
    /// `$WINEPREFIX`, if set.
    pub wineprefix: Option<PathBuf>,
    /// `$XDG_CACHE_HOME`, if set.
    pub cache_home: Option<PathBuf>,
    /// `$XDG_DATA_HOME`, if set.
    pub data_home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, if set.
    pub config_home: Option<PathBuf>,
    /// Wallpaper folders (`WallpaperSource`) specified by the person by hand
    /// (the [`SETTINGS_KEY`] setting).
    pub source_dirs: Vec<PathBuf>,
}

impl AxonEnv {
    /// The live process environment.
    pub fn from_process() -> AxonEnv {
        let dir = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        AxonEnv {
            home: dir("HOME").unwrap_or_default(),
            wineprefix: dir("WINEPREFIX"),
            cache_home: dir("XDG_CACHE_HOME"),
            data_home: dir("XDG_DATA_HOME"),
            config_home: dir("XDG_CONFIG_HOME"),
            source_dirs: Vec::new(),
        }
    }

    /// A "home only" environment — for checks.
    pub fn with_home(home: impl Into<PathBuf>) -> AxonEnv {
        AxonEnv { home: home.into(), ..AxonEnv::default() }
    }

    fn data_home(&self) -> PathBuf {
        self.data_home.clone().unwrap_or_else(|| self.home.join(".local/share"))
    }

    fn config_home(&self) -> PathBuf {
        self.config_home.clone().unwrap_or_else(|| self.home.join(".config"))
    }

    /// Backups of the original descriptions (`ResourceConfig.txt`) of wallpapers
    /// prepared in advance: `~/.local/share/rexon/axon-originals`.
    pub fn originals_root(&self) -> PathBuf {
        self.data_home().join("rexon").join("axon-originals")
    }

    /// Rexon's cache directory for Axon wallpapers: `~/.cache/rexon/axon`.
    pub fn cache_root(&self) -> PathBuf {
        self.cache_home
            .clone()
            .unwrap_or_else(|| self.home.join(".cache"))
            .join("rexon")
            .join("axon")
    }
}

// ============================================================================
// Finding prefixes
// ============================================================================

/// Whether a directory looks like a Wine prefix.
fn is_prefix(dir: &Path) -> bool {
    dir.join("drive_c").is_dir()
}

/// Subdirectories of `dir` (one level, by name), without errors leaking out.
fn children(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

/// Steam library directories: the main one and those listed in
/// `steamapps/libraryfolders.vdf` (lines `"path" "…"`).
fn steam_libraries(env: &AxonEnv) -> Vec<PathBuf> {
    let roots = [
        env.data_home().join("Steam"),
        env.home.join(".steam/steam"),
        env.home.join(".steam/root"),
        env.home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        env.home.join("snap/steam/common/.local/share/Steam"),
    ];
    let mut libs: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        libs.push(root.clone());
        let vdf = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            // `\t\t"path"\t\t"/mnt/games/SteamLibrary"` → 5 pieces.
            if parts.len() >= 4 && parts[1] == "path" {
                libs.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
            }
        }
    }
    libs
}

/// Lutris prefixes from game descriptions (`prefix:` in `games/*.yml`).
fn lutris_prefixes(env: &AxonEnv) -> Vec<PathBuf> {
    let dirs = [
        env.config_home().join("lutris/games"),
        env.data_home().join("lutris/games"),
        env.home.join(".var/app/net.lutris.Lutris/config/lutris/games"),
        env.home.join(".var/app/net.lutris.Lutris/data/lutris/games"),
    ];
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("yml") {
                continue;
            }
            for line in fs::read_to_string(&p).unwrap_or_default().lines() {
                if let Some(rest) = line.trim_start().strip_prefix("prefix:") {
                    let v = rest.trim().trim_matches(|c| c == '\'' || c == '"');
                    if !v.is_empty() {
                        let v = match v.strip_prefix("~/") {
                            Some(r) => env.home.join(r),
                            None => PathBuf::from(v),
                        };
                        out.push(v);
                    }
                }
            }
        }
    }
    out
}

/// Wine prefixes where Axon MAY be installed — without checking for Axon.
///
/// The order is from explicit to general: `$WINEPREFIX`, `~/.wine`, then game
/// managers. Repeats (the same prefix by two routes) are removed.
pub fn candidate_prefixes(env: &AxonEnv) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if is_prefix(&p) && !found.contains(&p) {
            found.push(p);
        }
    };
    if let Some(p) = &env.wineprefix {
        push(p.clone());
    }
    push(env.home.join(".wine"));
    let data = env.data_home();
    let one_level = [
        data.join("wineprefixes"),
        data.join("bottles/bottles"),
        env.home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
        env.home.join("Games"),
        env.home.join("Games/umu"),
        env.home.join("Games/Heroic/Prefixes"),
        env.home.join("Games/Heroic/Prefixes/default"),
        env.home.join(".PlayOnLinux/wineprefix"),
    ];
    for dir in one_level {
        for child in children(&dir) {
            push(child);
        }
    }
    for p in lutris_prefixes(env) {
        push(p);
    }
    for lib in steam_libraries(env) {
        for app in children(&lib.join("steamapps/compatdata")) {
            push(app.join("pfx"));
        }
    }
    found
}

/// The Axon directory of one Windows user in a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxonRoot {
    /// The Wine prefix.
    pub prefix: PathBuf,
    /// The Axon root: `<prefix>/drive_c/users/<user>/AppData/Local/Razer/RazerAxon`.
    pub root: PathBuf,
}

/// Axon directories in a prefix — one per Windows user.
pub fn roots_in_prefix(prefix: &Path) -> Vec<AxonRoot> {
    children(&prefix.join("drive_c/users"))
        .into_iter()
        .map(|user| AXON_SUBDIR.iter().fold(user, |p, part| p.join(part)))
        .filter(|root| root.is_dir())
        .map(|root| AxonRoot { prefix: prefix.to_path_buf(), root })
        .collect()
}

/// Axon account directories with a library (`WallPaperData`).
pub fn account_dirs(root: &Path) -> Vec<PathBuf> {
    children(root).into_iter().filter(|d| d.join(LIBRARY_FILE).is_file()).collect()
}

/// All Axon directories with a library in known places.
pub fn find_roots(env: &AxonEnv) -> Vec<AxonRoot> {
    candidate_prefixes(env)
        .iter()
        .flat_map(|p| roots_in_prefix(p))
        .filter(|r| !account_dirs(&r.root).is_empty())
        .collect()
}

// ============================================================================
// Windows → Linux paths
// ============================================================================

/// A Windows path (`C:\users\…`) inside a prefix → a Linux path.
///
/// `C:` is `drive_c`; other letters go through `dosdevices/<x>:` (that is how Wine
/// stores them; `Z:` usually leads to `/`). Wine preserves the case of letters in directory names
/// on disk, so segments are taken as is, and a case mismatch
/// is corrected by a case-insensitive search.
pub fn windows_to_linux(prefix: &Path, win: &str) -> Option<PathBuf> {
    let win = win.trim();
    let bytes = win.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let letter = (bytes[0] as char).to_ascii_lowercase();
    let mut base = if letter == 'c' {
        prefix.join("drive_c")
    } else {
        let dev = prefix.join("dosdevices").join(format!("{letter}:"));
        match fs::read_link(&dev) {
            Ok(target) if target.is_absolute() => target,
            Ok(target) => prefix.join("dosdevices").join(target),
            Err(_) if dev.exists() => dev,
            Err(_) => return None,
        }
    };
    for seg in win[2..].split(['\\', '/']).filter(|s| !s.is_empty()) {
        if seg == "." {
            continue;
        }
        if seg == ".." {
            base.pop();
            continue;
        }
        let exact = base.join(seg);
        base = if exact.exists() {
            exact
        } else {
            find_case_insensitive(&base, seg).unwrap_or(exact)
        };
    }
    Some(base)
}

fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = name.to_lowercase();
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().to_lowercase() == want)
        .map(|e| e.path())
}

/// A relative path from `ResourceConfig.txt` (`Resource\\a.mp4`) inside
/// the wallpaper directory. Leaving the directory (`..`, an absolute path) — `None`.
pub fn resource_relative(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    let p = Path::new(&rel);
    if p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    Some(dir.join(p))
}

// ============================================================================
// Parsing Axon files
// ============================================================================

/// Wallpaper kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxonKind {
    /// A video (`VIDEO`).
    Video,
    /// A picture (`IMAGE`).
    Image,
    /// A web page (`WEB`, `LOCALWEB`).
    Web,
    /// Other — as Axon wrote it.
    Other(String),
}

impl AxonKind {
    pub fn parse(raw: &str) -> AxonKind {
        match raw.trim().to_ascii_uppercase().as_str() {
            "VIDEO" | "LOCALVIDEO" => AxonKind::Video,
            "IMAGE" | "LOCALIMAGE" | "PICTURE" => AxonKind::Image,
            "WEB" | "LOCALWEB" => AxonKind::Web,
            _ => AxonKind::Other(raw.trim().to_string()),
        }
    }

    /// The kind label for the tile.
    pub fn label(&self) -> String {
        match self {
            AxonKind::Video => tr("🎞️ Video"),
            AxonKind::Image => tr("🖼️ Picture"),
            AxonKind::Web => tr("🌐 Web page"),
            AxonKind::Other(s) if s.is_empty() => tr("Unknown kind"),
            AxonKind::Other(s) => s.clone(),
        }
    }
}

/// The wallpaper type from a record: `WallPaperType` (new versions) or `Type` (old ones).
pub fn kind_field(v: &Value) -> Option<&str> {
    ["WallPaperType", "Type"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()))
}

/// The text of an Axon file: without the BOM at the start.
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str(strip_bom(&text)).map_err(|e| format!("{}: {e}", path.display()))
}

/// A string from a field where Axon writes sometimes a number, sometimes a string.
fn loose_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// One `WallPaperData` record — as Axon wrote it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryEntry {
    pub id: String,
    pub title: String,
    pub author: String,
    /// The type from the record (`VIDEO`…); empty — Axon did not write it.
    pub kind: String,
    pub effect: String,
    pub audible: bool,
    /// `3840x2160` → (3840, 2160).
    pub resolution: Option<(u32, u32)>,
    pub status: String,
    /// The Windows path to the display file (`PlayResoucePath`).
    pub play_path: String,
    pub thumbnail_url: String,
    pub preview_url: String,
    pub description: String,
    pub tags: String,
    /// Information for filters, details and display properties.
    pub details: AxonDetails,
}

/// Information of a `WallPaperData` record needed by library filters,
/// the details window and display properties — all as Axon wrote it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AxonDetails {
    /// In favorites (`IsFavorite` = 1). This is only a copy of the flag from the Razer
    /// server: it cannot be toggled without an account, it is only good for a filter.
    pub is_favorite: bool,
    /// Price in silver (`Silver`): above zero — "premium" wallpapers.
    pub silver: u64,
    /// Bought for silver (`IsRedeemed` = 1).
    pub is_redeemed: bool,
    /// Download reward (`DownloadReward`); the 2.11.1.0 library has no such field,
    /// and then the "Rewards" filter is empty — as in Axon itself.
    pub download_reward: u64,
    /// `PaperType`: `Generate` — wallpapers created by the Axon AI generator.
    pub paper_type: String,
    /// `Method` — the creation method (for AI wallpapers), as written.
    pub method: String,
    /// `DetailId` — the AI creation number; non-zero for AI wallpapers.
    pub detail_id: u64,
    /// Catalog category numbers (`Category`: `"18,17"`).
    pub categories: Vec<String>,
    /// Collections (`CollectionInfos`): number and title.
    pub collections: Vec<(String, String)>,
    /// Likes (`Upvoted`).
    pub upvoted: u64,
    /// Downloads (`Downloads`).
    pub downloads: u64,
    /// The sound of these wallpapers is muted (`IsMute`).
    pub is_mute: bool,
    /// Display properties (`PlayEffects`): brightness, contrast, hue, saturation,
    /// speed, fill style… Values as strings, as in Axon.
    pub play_effects: BTreeMap<String, String>,
}

impl AxonDetails {
    /// AI wallpapers: this is how Axon itself selects them (`DetailId` is present); `PaperType`
    /// `Generate` — the same sign for records that came from the cloud.
    pub fn is_ai(&self) -> bool {
        self.detail_id != 0 || self.paper_type.eq_ignore_ascii_case("generate")
    }
}

/// A number from a field where Axon writes sometimes a number, sometimes a string; anything else — 0.
fn loose_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f > 0.0).map(|f| f as u64))
            .unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        Some(Value::Bool(b)) => u64::from(*b),
        _ => 0,
    }
}

/// A flag: `1`, `"1"`, `true`, `"true"`.
fn loose_bool(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true"),
        _ => false,
    }
}

/// `PlayEffects`: a dictionary, or the same as a JSON string (that is how the Axon
/// player writes it). Values as strings.
fn parse_play_effects(v: Option<&Value>) -> BTreeMap<String, String> {
    let parsed;
    let obj = match v {
        Some(Value::Object(o)) => Some(o),
        Some(Value::String(s)) if !s.trim().is_empty() => {
            parsed = serde_json::from_str::<Value>(s).ok();
            parsed.as_ref().and_then(Value::as_object)
        }
        _ => None,
    };
    obj.into_iter()
        .flatten()
        .filter(|(_, x)| !x.is_null())
        .map(|(k, x)| (k.clone(), loose_str(Some(x))))
        .collect()
}

/// Information from a raw library record.
fn parse_details(raw: &Value) -> AxonDetails {
    let categories = loose_str(raw.get("Category"))
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect();
    let collections = raw
        .get("CollectionInfos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| (loose_str(c.get("collection_id")), loose_str(c.get("title"))))
        .filter(|(id, title)| !id.is_empty() || !title.is_empty())
        .collect();
    AxonDetails {
        is_favorite: loose_bool(raw.get("IsFavorite")),
        silver: loose_u64(raw.get("Silver")),
        is_redeemed: loose_bool(raw.get("IsRedeemed")),
        download_reward: loose_u64(raw.get("DownloadReward")),
        paper_type: loose_str(raw.get("PaperType")),
        method: loose_str(raw.get("Method")),
        detail_id: loose_u64(raw.get("DetailId")),
        categories,
        collections,
        upvoted: loose_u64(raw.get("Upvoted")),
        downloads: loose_u64(raw.get("Downloads")),
        is_mute: loose_bool(raw.get("IsMute")),
        play_effects: parse_play_effects(raw.get("PlayEffects")),
    }
}

/// The record's tags (`AllTags`: `" Cyberpunk,Some Artist, 3D"`) as a list.
pub fn tag_list(tags: &str) -> Vec<String> {
    tags.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// `3840x2160` → (width, height).
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.trim().split_once(['x', 'X', '×'])?;
    let w = w.trim().parse().ok()?;
    let h = h.trim().parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Parsing `WallPaperData`. Records without an `Id` are skipped; a file that is not
/// a JSON array is an error (its text is for the log).
pub fn parse_library(text: &str) -> Result<Vec<LibraryEntry>, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    let list = v.as_array().ok_or_else(|| "WallPaperData: ожидался массив".to_string())?;
    let mut out = Vec::with_capacity(list.len());
    for raw in list {
        let id = loose_str(raw.get("Id"));
        if id.is_empty() {
            continue;
        }
        let author = match raw.get("Author") {
            Some(Value::Object(a)) => loose_str(a.get("author_name")),
            other => loose_str(other),
        };
        out.push(LibraryEntry {
            title: loose_str(raw.get("Title")),
            author,
            kind: kind_field(raw).unwrap_or("").trim().to_string(),
            effect: loose_str(raw.get("EffectType")),
            audible: matches!(loose_str(raw.get("Audible")).as_str(), "1" | "true" | "True"),
            resolution: parse_resolution(&loose_str(raw.get("Resolution"))),
            status: loose_str(raw.get("WallPaperStatus")),
            play_path: loose_str(raw.get("PlayResoucePath")),
            thumbnail_url: loose_str(raw.get("ThumbnailSource")),
            preview_url: loose_str(raw.get("PreviewSource")),
            description: loose_str(raw.get("Description")),
            tags: loose_str(raw.get("AllTags")),
            details: parse_details(raw),
            id,
        });
    }
    Ok(out)
}

/// The playlist (display queue) of one screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayList {
    pub monitor: String,
    pub wallpaper_ids: Vec<String>,
    pub current_index: usize,
    pub mute: bool,
    /// Screen volume 0–100 (`MaxVolume`).
    pub volume: u32,
    /// Display stopped with the "Stop" button (`IsStopped`).
    pub stopped: bool,
    /// This screen is selected in the queue panel (`IsSelect`).
    pub selected: bool,
}

impl PlayList {
    /// The wallpapers Axon is showing on this screen now.
    pub fn current(&self) -> Option<&str> {
        self.wallpaper_ids.get(self.current_index).map(String::as_str)
    }
}

/// The order of changing wallpapers in the queue (`PlayOrder`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayOrder {
    /// "In order" (`Ordered`).
    #[default]
    Ordered,
    /// "Random" (`Random`).
    Random,
}

/// How wallpapers are laid out across several screens (`PlayMode`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayMode {
    /// Different wallpapers on each screen (`None`).
    #[default]
    Separate,
    /// The selected screen is repeated on the others (`Duplicate`).
    Duplicate,
    /// The selected screen's wallpaper is stretched across all (`Span`).
    Span,
}

/// The wallpaper change timer "Never" (`SwitchIntervalSeconds` not above zero).
pub const SWITCH_NEVER: i64 = -1;

/// The whole `PlayListSetting`: screen queues and shared display settings.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayListSetting {
    /// After how many seconds to change wallpapers; [`SWITCH_NEVER`] — never.
    pub switch_interval_seconds: i64,
    /// The timer is set with its own number ("Custom"), not from the list.
    pub is_custom_interval: bool,
    pub play_order: PlayOrder,
    pub play_mode: PlayMode,
    /// "General mode": pause on the screen where a window is maximized to full screen
    /// (`IsPauseWhenFullScreen`).
    pub pause_when_fullscreen: bool,
    /// "Economy mode": pause on all screens during a full-screen game
    /// (`IsPauseAllWhenD3dFullScreen`).
    pub pause_all_when_game_fullscreen: bool,
    /// Default volume 0–100 (`DefaultVolume`).
    pub default_volume: u32,
    pub playlists: Vec<PlayList>,
}

impl Default for PlayListSetting {
    fn default() -> Self {
        PlayListSetting {
            switch_interval_seconds: SWITCH_NEVER,
            is_custom_interval: false,
            play_order: PlayOrder::Ordered,
            play_mode: PlayMode::Separate,
            pause_when_fullscreen: false,
            pause_all_when_game_fullscreen: false,
            default_volume: 50,
            playlists: Vec::new(),
        }
    }
}

/// Volume 0–100; no field — `fallback`.
fn volume_of(v: Option<&Value>, fallback: u32) -> u32 {
    match v {
        None | Some(Value::Null) => fallback,
        x => loose_u64(x).min(100) as u32,
    }
}

/// Parsing `PlayListSetting` together with the display settings. No `PlayLists`
/// array — an error (the wrong file); missing settings — defaults.
pub fn parse_playlist_setting(text: &str) -> Result<PlayListSetting, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    let lists = v
        .get("PlayLists")
        .and_then(Value::as_array)
        .ok_or_else(|| "PlayListSetting: нет PlayLists".to_string())?;
    let d = PlayListSetting::default();
    let interval = match v.get("SwitchIntervalSeconds") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(SWITCH_NEVER),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(SWITCH_NEVER),
        _ => SWITCH_NEVER,
    };
    let play_order = match loose_str(v.get("PlayOrder")).to_ascii_lowercase().as_str() {
        "random" => PlayOrder::Random,
        _ => PlayOrder::Ordered,
    };
    let play_mode = match loose_str(v.get("PlayMode")).to_ascii_lowercase().as_str() {
        "duplicate" => PlayMode::Duplicate,
        "span" => PlayMode::Span,
        _ => PlayMode::Separate,
    };
    Ok(PlayListSetting {
        switch_interval_seconds: if interval <= 0 { SWITCH_NEVER } else { interval },
        is_custom_interval: loose_bool(v.get("IsCustomInterval")),
        play_order,
        play_mode,
        pause_when_fullscreen: loose_bool(v.get("IsPauseWhenFullScreen")),
        pause_all_when_game_fullscreen: loose_bool(v.get("IsPauseAllWhenD3dFullScreen")),
        default_volume: volume_of(v.get("DefaultVolume"), d.default_volume),
        playlists: lists
            .iter()
            .map(|p| PlayList {
                monitor: loose_str(p.get("MonitorId")),
                wallpaper_ids: p
                    .get("WallpaperIds")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|x| loose_str(Some(x))).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default(),
                current_index: p.get("CurrentPlayIndex").and_then(Value::as_u64).unwrap_or(0) as usize,
                mute: loose_bool(p.get("IsMute")),
                volume: volume_of(p.get("MaxVolume"), 100),
                stopped: loose_bool(p.get("IsStopped")),
                selected: loose_bool(p.get("IsSelect")),
            })
            .collect(),
    })
}

/// Parsing `PlayListSetting`: only the screen queues.
pub fn parse_playlists(text: &str) -> Result<Vec<PlayList>, String> {
    parse_playlist_setting(text).map(|s| s.playlists)
}

/// The account's display settings (`PlayListSetting`), if the file exists and
/// is readable. Read-only: Rexon does not edit Axon files.
pub fn read_playlist_setting(account: &Path) -> Option<PlayListSetting> {
    fs::read(account.join(PLAYLIST_FILE))
        .ok()
        .and_then(|b| parse_playlist_setting(&String::from_utf8_lossy(&b)).ok())
}

/// The description of downloaded wallpapers (`ResourceConfig.txt`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceConfig {
    pub kind: AxonKind,
    /// The path to the display file relative to the wallpaper directory, as written.
    pub source: String,
}

/// Parsing `ResourceConfig.txt`: both type field names and the BOM.
pub fn parse_resource_config(text: &str) -> Result<ResourceConfig, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    Ok(ResourceConfig {
        kind: AxonKind::parse(kind_field(&v).unwrap_or("")),
        source: loose_str(v.get("Source")),
    })
}

// ============================================================================
// Wallpaper model
// ============================================================================

/// One wallpaper of the Axon library — everything the tile and "Apply" need.
#[derive(Debug, Clone, PartialEq)]
pub struct AxonWallpaper {
    pub id: String,
    pub title: String,
    pub author: String,
    pub kind: AxonKind,
    /// `Dynamic` — Axon's "dynamic" wallpapers.
    pub effect: String,
    /// The video has sound and it is intended.
    pub audible: bool,
    pub resolution: Option<(u32, u32)>,
    /// The cover on the network (Axon's CDN).
    pub cover_url: String,
    /// The cover on disk, if Axon saved it.
    pub cover_local: Option<PathBuf>,
    /// The downloaded wallpaper directory (`WallpaperSource/{Id}{H}{W}`), if any.
    pub resource_dir: Option<PathBuf>,
    /// The container file inside it, if any.
    pub container: Option<PathBuf>,
    /// Whether the wallpaper is fully downloaded (the directory, description and container in place).
    pub downloaded: bool,
    /// Whether Axon is showing it now (per the playlist).
    pub playing: bool,
    /// The state from the library (`WallPaperStatus`: `Installed`, `Playing`…).
    pub status: String,
    pub description: String,
    /// Tags (`AllTags`), as written: comma-separated.
    pub tags: String,
    /// Information for filters, details and display properties.
    pub details: AxonDetails,
    /// The Wine prefix the wallpaper is from.
    pub prefix: PathBuf,
}

impl AxonWallpaper {
    /// The cache directory key: the Axon number without characters that do not belong in
    /// a directory name.
    pub fn cache_key(&self) -> String {
        sanitize_id(&self.id)
    }
}

fn sanitize_id(id: &str) -> String {
    let s: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() { "_".to_string() } else { s }
}

/// The downloaded wallpaper directory: the display path from the library, and without it —
/// `{Id}{H}{W}` in the storage, then any `{Id}…` with a description inside.
fn locate_resource_dir(prefix: &Path, store: &Path, e: &LibraryEntry) -> Option<PathBuf> {
    let has_config = |d: &Path| d.join(RESOURCE_CONFIG).is_file();
    if !e.play_path.is_empty() {
        if let Some(file) = windows_to_linux(prefix, &e.play_path) {
            // …\{Id}{H}{W}\Resource\x.mp4 → …\{Id}{H}{W}
            let dir = file.parent().and_then(Path::parent).map(Path::to_path_buf);
            if let Some(d) = dir.filter(|d| has_config(d)) {
                return Some(d);
            }
        }
    }
    if let Some((w, h)) = e.resolution {
        let d = store.join(format!("{}{h}{w}", e.id));
        if has_config(&d) {
            return Some(d);
        }
    }
    // No resolution in the record: the longest `{Id}<digits>` — the native
    // Axon layout; `{Id}` itself — the layout of third-party downloaders.
    let mut best: Option<PathBuf> = None;
    for d in children(store) {
        let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(rest) = name.strip_prefix(&e.id) else { continue };
        if !rest.chars().all(|c| c.is_ascii_digit()) || !has_config(&d) {
            continue;
        }
        let longer = best
            .as_ref()
            .map(|b| b.file_name().map_or(0, |n| n.len()) < name.len())
            .unwrap_or(true);
        if longer {
            best = Some(d);
        }
    }
    best
}

/// Where the account's download storage is.
fn store_dir(prefix: &Path, root: &Path, account: &Path) -> PathBuf {
    read_json(&account.join(USER_SETTING_FILE))
        .ok()
        .and_then(|v| v.get("SourceSavedPath").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| windows_to_linux(prefix, &s))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| root.join(DEFAULT_SOURCE_DIR))
}

/// The cover saved by Axon (`WallpaperCovers/<Id>.*`), if any.
fn local_cover(root: &Path, id: &str) -> Option<PathBuf> {
    let dir = root.join("WallpaperCovers");
    let rd = fs::read_dir(&dir).ok()?;
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| p.file_stem().map(|s| s.to_string_lossy() == id).unwrap_or(false))
}

/// The description and container of downloaded wallpapers in the directory `dir`.
fn read_resource(dir: &Path) -> (Option<ResourceConfig>, Option<PathBuf>) {
    let config = fs::read(dir.join(RESOURCE_CONFIG))
        .ok()
        .and_then(|b| parse_resource_config(&String::from_utf8_lossy(&b)).ok());
    let container = config
        .as_ref()
        .filter(|c| !c.source.is_empty())
        .and_then(|c| resource_relative(dir, &c.source));
    (config, container)
}

/// A wallpaper from a library record (if any) and the download directory (if any).
fn build(
    prefix: &Path,
    root: Option<&Path>,
    id: &str,
    entry: Option<&LibraryEntry>,
    resource_dir: Option<PathBuf>,
    playing: bool,
) -> AxonWallpaper {
    let (config, container) = match &resource_dir {
        Some(d) => read_resource(d),
        None => (None, None),
    };
    let downloaded = container.as_ref().is_some_and(|c| c.is_file());
    // The kind — from the download description (it is more precise), otherwise from the library.
    let kind = match &config {
        Some(c) if !matches!(&c.kind, AxonKind::Other(s) if s.is_empty()) => c.kind.clone(),
        _ => AxonKind::parse(entry.map_or("", |e| e.kind.as_str())),
    };
    // Resolution: from the library, otherwise from the file name (`3840x2160.mp4`).
    let resolution = entry.and_then(|e| e.resolution).or_else(|| {
        container
            .as_ref()
            .and_then(|c| c.file_stem())
            .and_then(|s| parse_resolution(&s.to_string_lossy()))
    });
    let e = entry.cloned().unwrap_or_default();
    AxonWallpaper {
        cover_local: root.and_then(|r| local_cover(r, id)),
        playing,
        id: id.to_string(),
        title: e.title,
        author: e.author,
        kind,
        effect: e.effect,
        audible: e.audible,
        resolution,
        cover_url: if e.thumbnail_url.is_empty() { e.preview_url } else { e.thumbnail_url },
        resource_dir,
        container,
        downloaded,
        status: e.status,
        description: e.description,
        tags: e.tags,
        details: e.details,
        prefix: prefix.to_path_buf(),
    }
}

/// What Axon is showing now according to the account's playlists.
fn playing_ids(account: &Path) -> Vec<String> {
    read_playlist_setting(account)
        .map(|s| s.playlists.iter().filter_map(|p| p.current().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn read_entries(account: &Path) -> Result<Vec<LibraryEntry>, String> {
    let bytes = fs::read(account.join(LIBRARY_FILE)).map_err(|e| e.to_string())?;
    parse_library(&String::from_utf8_lossy(&bytes))
}

/// The wallpapers of one account.
pub fn read_account(prefix: &Path, root: &Path, account: &Path) -> Result<Vec<AxonWallpaper>, String> {
    let entries = read_entries(account)?;
    let playing = playing_ids(account);
    let store = store_dir(prefix, root, account);
    Ok(entries
        .iter()
        .map(|e| {
            let dir = locate_resource_dir(prefix, &store, e);
            build(prefix, Some(root), &e.id, Some(e), dir, playing.contains(&e.id))
        })
        .collect())
}

fn mtime(p: &Path) -> SystemTime {
    fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

// ── A wallpaper folder specified by hand ───────────────────────────────

/// The Rexon setting key: Axon wallpaper folders specified by hand (an array
/// of strings in `settings.json`).
pub const SETTINGS_KEY: &str = "axon_wallpaper_dirs";

/// Folders from Rexon's settings (strings only, empty ones and repeats dropped).
pub fn saved_source_dirs(settings: &serde_json::Map<String, Value>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for v in settings.get(SETTINGS_KEY).and_then(Value::as_array).into_iter().flatten() {
        if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            let p = PathBuf::from(s);
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// The list for the settings after adding `dir` (without repeats, at the end).
pub fn with_source_dir(saved: &[PathBuf], dir: &Path) -> Value {
    let mut list: Vec<PathBuf> = saved.to_vec();
    if !list.iter().any(|p| p == dir) {
        list.push(dir.to_path_buf());
    }
    Value::Array(list.iter().map(|p| Value::from(p.to_string_lossy().into_owned())).collect())
}

/// Wallpaper directories (`{Id}{H}{W}` with `ResourceConfig.txt`) in a folder.
fn resource_dirs_in(dir: &Path) -> Vec<PathBuf> {
    children(dir).into_iter().filter(|d| d.join(RESOURCE_CONFIG).is_file()).collect()
}

/// Check the folder the person chose: it must be an Axon wallpaper folder
/// (`WallpaperSource`). If the directory of a single wallpaper is chosen, the folder above
/// it is taken. An error is text for the person.
pub fn check_source_dir(dir: &Path) -> Result<PathBuf, String> {
    if !dir.is_dir() {
        return Err(folder_missing_text());
    }
    let dir = if dir.join(RESOURCE_CONFIG).is_file() {
        dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf())
    } else {
        dir.to_path_buf()
    };
    if resource_dirs_in(&dir).is_empty() {
        return Err(folder_not_axon_text());
    }
    Ok(dir)
}

/// Screen heights and widths Axon names directories with
/// (`{Id}{Height}{Width}`) — to separate the number when there is no library nearby.
const HEIGHTS: &[u32] = &[720, 768, 800, 900, 1080, 1200, 1440, 1600, 1800, 2160, 2400, 2880, 4320];
const WIDTHS: &[u32] = &[1280, 1366, 1440, 1600, 1920, 2048, 2560, 2880, 3440, 3840, 5120, 7680];

/// The wallpaper number from a `{Id}{H}{W}` directory name when there is no library: by
/// the resolution from the file name, otherwise by known screen resolutions, otherwise
/// the whole name.
pub fn id_from_dir_name(name: &str, resolution: Option<(u32, u32)>) -> String {
    let strip = |w: u32, h: u32| {
        name.strip_suffix(&format!("{h}{w}"))
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
    };
    if let Some(id) = resolution.and_then(|(w, h)| strip(w, h)) {
        return id;
    }
    for &w in WIDTHS {
        for &h in HEIGHTS {
            if let Some(id) = strip(w, h) {
                return id;
            }
        }
    }
    name.to_string()
}

/// Wallpapers from a `WallpaperSource` folder specified by hand. The title, author and
/// cover are taken from the library nearby (the folder above it → `*/WallPaperData`)
/// if there is one; otherwise — what `ResourceConfig.txt` knows, and the number.
pub fn read_source_dir(dir: &Path) -> Result<Vec<AxonWallpaper>, String> {
    let dir = check_source_dir(dir)?;
    let root = dir.parent().map(Path::to_path_buf);
    let prefix = dir
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "drive_c"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    // A library nearby: the freshest record wins.
    let mut entries: Vec<LibraryEntry> = Vec::new();
    let mut playing: Vec<String> = Vec::new();
    if let Some(root) = &root {
        let mut accounts = account_dirs(root);
        accounts.sort_by_key(|a| std::cmp::Reverse(mtime(&a.join(LIBRARY_FILE))));
        for acc in accounts {
            playing.extend(playing_ids(&acc));
            for e in read_entries(&acc).unwrap_or_default() {
                if !entries.iter().any(|x| x.id == e.id) {
                    entries.push(e);
                }
            }
        }
    }
    let mut out: Vec<AxonWallpaper> = Vec::new();
    for res in resource_dirs_in(&dir) {
        let name = res.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // A library record whose number is the start of the name, and the rest is digits;
        // of several — the one with the longest number.
        let entry = entries
            .iter()
            .filter(|e| name.strip_prefix(&e.id).is_some_and(|r| r.chars().all(|c| c.is_ascii_digit())))
            .max_by_key(|e| e.id.len());
        let id = match entry {
            Some(e) => e.id.clone(),
            None => {
                let (_, container) = read_resource(&res);
                let resolution = container
                    .as_ref()
                    .and_then(|c| c.file_stem())
                    .and_then(|s| parse_resolution(&s.to_string_lossy()));
                id_from_dir_name(&name, resolution)
            }
        };
        let w = build(&prefix, root.as_deref(), &id, entry, Some(res), playing.contains(&id));
        merge(&mut out, w);
    }
    Ok(out)
}

/// Add a wallpaper to the list: if the same one (the same number) is already there — keep
/// the downloaded one, and of two downloaded ones — the native Axon layout (a long
/// `{Id}{H}{W}` directory name), not a third-party `{Id}`.
fn merge(list: &mut Vec<AxonWallpaper>, w: AxonWallpaper) {
    let name_len = |x: &AxonWallpaper| {
        x.resource_dir.as_ref().and_then(|d| d.file_name()).map_or(0, |n| n.len())
    };
    match list.iter().position(|x| x.id == w.id) {
        None => list.push(w),
        Some(i) => {
            let old = &list[i];
            let better = (!old.downloaded && w.downloaded)
                || (old.downloaded == w.downloaded && name_len(&w) > name_len(old));
            if better {
                // We do not lose the library information if the new record does not have it.
                let mut w = w;
                if w.title.is_empty() {
                    w.title = old.title.clone();
                    w.author = old.author.clone();
                    w.cover_url = old.cover_url.clone();
                    w.audible = old.audible;
                    w.tags = old.tags.clone();
                    w.details = old.details.clone();
                }
                list[i] = w;
            }
        }
    }
}

/// The Axon library on the machine: all found directories and accounts.
#[derive(Debug, Clone, Default)]
pub struct AxonLibrary {
    pub wallpapers: Vec<AxonWallpaper>,
    /// Wallpaper folders (`WallpaperSource`) they were taken from: found ones and
    /// those specified by hand, without repeats. The first one is for "Open folder".
    pub source_dirs: Vec<PathBuf>,
    /// Whether at least one library or wallpaper folder was found.
    pub found: bool,
    /// The display settings (`PlayListSetting`) of the freshest account
    /// that has them. Read-only: Rexon keeps the queue, timer and pauses
    /// in its own settings, and takes the initial values from here.
    pub play_setting: Option<PlayListSetting>,
    /// What could not be read (for the log): the file and the reason.
    pub problems: Vec<String>,
}

impl AxonLibrary {
    pub fn get(&self, id: &str) -> Option<&AxonWallpaper> {
        self.wallpapers.iter().find(|w| w.id == id)
    }
}

/// Read the library from the found directories. Identical wallpapers (the same
/// `Id` in two accounts or prefixes) come from the freshest
/// library, but if they are not downloaded there and are downloaded in another — the downloaded ones.
/// The order is as in the Axon library (the freshest account first).
pub fn read_roots(roots: &[AxonRoot]) -> AxonLibrary {
    let mut accounts: Vec<(SystemTime, &AxonRoot, PathBuf)> = Vec::new();
    for r in roots {
        for acc in account_dirs(&r.root) {
            accounts.push((mtime(&acc.join(LIBRARY_FILE)), r, acc));
        }
    }
    accounts.sort_by(|a, b| b.0.cmp(&a.0));
    let mut lib = AxonLibrary { found: !accounts.is_empty(), ..AxonLibrary::default() };
    for (_, r, acc) in accounts {
        if lib.play_setting.is_none() {
            lib.play_setting = read_playlist_setting(&acc);
        }
        let store = store_dir(&r.prefix, &r.root, &acc);
        if store.is_dir() && !lib.source_dirs.contains(&store) {
            lib.source_dirs.push(store);
        }
        match read_account(&r.prefix, &r.root, &acc) {
            Ok(list) => {
                for w in list {
                    merge(&mut lib.wallpapers, w);
                }
            }
            Err(why) => lib.problems.push(format!("{}: {why}", acc.join(LIBRARY_FILE).display())),
        }
    }
    lib
}

/// The machine's Axon library: found prefixes plus folders specified by hand.
pub fn load_library(env: &AxonEnv) -> AxonLibrary {
    let mut lib = read_roots(&find_roots(env));
    for dir in &env.source_dirs {
        match read_source_dir(dir) {
            Ok(list) => {
                lib.found = true;
                let dir = check_source_dir(dir).unwrap_or_else(|_| dir.clone());
                if !lib.source_dirs.contains(&dir) {
                    lib.source_dirs.push(dir);
                }
                for w in list {
                    merge(&mut lib.wallpapers, w);
                }
            }
            Err(why) => lib.problems.push(format!("{}: {why}", dir.display())),
        }
    }
    lib
}

// ============================================================================
// Preparing for display
// ============================================================================

/// Why the wallpaper could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxonExtractError {
    /// Nothing to prepare with: this build does not have that capability.
    Unavailable,
    /// The preparation does not know this wallpaper kind.
    Unsupported(String),
    /// The wallpaper files are incomplete or corrupted.
    Damaged(String),
    /// Any other failure — text for the log.
    Failed(String),
}

impl std::fmt::Display for AxonExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AxonExtractError::Unavailable => write!(f, "unavailable"),
            AxonExtractError::Unsupported(s) => write!(f, "unsupported: {s}"),
            AxonExtractError::Damaged(s) => write!(f, "damaged: {s}"),
            AxonExtractError::Failed(s) => write!(f, "failed: {s}"),
        }
    }
}

/// Preparing downloaded wallpapers for display.
///
/// It receives an Axon wallpaper directory (`WallpaperSource/{Id}{H}{W}`, read-only)
/// and an empty or previous cache directory; it puts a file ready for
/// display (a video or a picture) there and returns its path. The Axon directory
/// must not be touched.
pub trait AxonExtractor: Send + Sync {
    fn extract(&self, resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, AxonExtractError>;
}

/// The built-in preparation (`crate::axon_extract`).
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltinExtractor;

impl AxonExtractor for BuiltinExtractor {
    fn extract(&self, resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, AxonExtractError> {
        use crate::axon_extract::{extract, ExtractError};
        extract(resource_dir, out_dir).map_err(|e| match e {
            ExtractError::Config(m) => AxonExtractError::Damaged(m),
            // An unknown scheme or the wrong password — we do not know this wallpaper kind
            // yet; anything else (not a ZIP, truncated) — the files are corrupted.
            ExtractError::Format(m) if m.starts_with("unsupported") || m.contains("wrong password") => {
                AxonExtractError::Unsupported(m)
            }
            ExtractError::Format(m) => AxonExtractError::Damaged(m),
            ExtractError::Write(m) => AxonExtractError::Failed(m),
        })
    }
}

/// What to prepare wallpapers with in this build — the built-in preparation.
pub fn default_extractor() -> Box<dyn AxonExtractor> {
    Box::new(BuiltinExtractor)
}

/// How to apply a ready file: the format of the shared wallpaper executor
/// (`WallpaperExec::apply_with_audio`). `None` — such a file cannot be set
/// as a wallpaper.
pub fn apply_format(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if ext == "gif" {
        return Some("animated");
    }
    if crate::wallpaper_engine::VALID_VIDEO_EXTS.iter().any(|e| e.trim_start_matches('.') == ext) {
        return Some("video");
    }
    match ext.as_str() {
        "gif" => Some("animated"),
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "avif" | "jxl" | "tif" | "tiff" => Some("static"),
        _ => None,
    }
}

/// A wallpaper ready to apply.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub path: PathBuf,
    /// The shared executor's format: `video`, `static`, `animated`.
    pub format: &'static str,
    /// The video's sound — as intended in Axon (`Audible`).
    pub audio: bool,
    pub name: String,
}

/// The cache mark: which container the ready file was made from.
fn stamp_of(container: &Path) -> Option<Value> {
    let m = fs::metadata(container).ok()?;
    let secs = m.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    Some(serde_json::json!({
        "container": container.to_string_lossy(),
        "size": m.len(),
        "mtime": secs,
    }))
}

/// The ready file from last time, if the container has not changed since.
fn cached(out_dir: &Path, container: &Path) -> Option<PathBuf> {
    let saved = read_json(&out_dir.join(READY_STAMP)).ok()?;
    let now = stamp_of(container)?;
    for k in ["container", "size", "mtime"] {
        if saved.get(k) != now.get(k) {
            return None;
        }
    }
    let file = out_dir.join(saved.get("file")?.as_str()?);
    (file.is_file() && fs::metadata(&file).map(|m| m.len() > 0).unwrap_or(false)).then_some(file)
}

/// Prepare a wallpaper for applying: refusals without preparation, the cache, preparation,
/// checking the answer. An error is text for the person.
pub fn prepare(
    w: &AxonWallpaper,
    extractor: Option<&dyn AxonExtractor>,
    cache_root: &Path,
) -> Result<Prepared, String> {
    let name = display_title(w);
    if w.kind == AxonKind::Web {
        return Err(web_unsupported_text(&name));
    }
    let (Some(dir), Some(container)) = (&w.resource_dir, &w.container) else {
        return Err(not_downloaded_text(&name));
    };
    if !w.downloaded {
        return Err(not_downloaded_text(&name));
    }
    let out_dir = cache_root.join(w.cache_key());
    let audio = w.audible && w.kind == AxonKind::Video;
    if let Some(path) = cached(&out_dir, container) {
        if let Some(format) = apply_format(&path) {
            return Ok(Prepared { path, format, audio, name });
        }
    }
    let Some(extractor) = extractor else {
        return Err(unavailable_text());
    };
    fs::create_dir_all(&out_dir).map_err(|e| {
        crate::log_warn!("axon", "каталог кэша {} не создан: {e}", out_dir.display());
        prepare_failed_text(&name)
    })?;
    let path = match extractor.extract(dir, &out_dir) {
        Ok(p) => p,
        Err(e) => {
            crate::log_warn!("axon", "обои {} ({}) не подготовлены: {e}", w.id, dir.display());
            return Err(match e {
                AxonExtractError::Unavailable => unavailable_text(),
                AxonExtractError::Unsupported(_) => kind_unsupported_text(&name),
                AxonExtractError::Damaged(_) => damaged_text(&name),
                AxonExtractError::Failed(_) => prepare_failed_text(&name),
            });
        }
    };
    // We trust the answer only if the file is real and lies in the given directory.
    let inside = match (path.canonicalize(), out_dir.canonicalize()) {
        (Ok(p), Ok(o)) => p.starts_with(&o) && p.is_file(),
        _ => false,
    };
    if !inside {
        crate::log_warn!("axon", "подготовка вернула негодный путь: {}", path.display());
        return Err(prepare_failed_text(&name));
    }
    let Some(format) = apply_format(&path) else {
        crate::log_warn!("axon", "подготовленный файл неизвестного вида: {}", path.display());
        return Err(kind_unsupported_text(&name));
    };
    if let (Some(mut stamp), Some(file)) = (stamp_of(container), path.file_name()) {
        stamp["file"] = Value::from(file.to_string_lossy().into_owned());
        let _ = fs::write(out_dir.join(READY_STAMP), stamp.to_string());
    }
    Ok(Prepared { path, format, audio, name })
}

// ============================================================================
// Section tiles
// ============================================================================

/// The title to show (no title — the number).
pub fn display_title(w: &AxonWallpaper) -> String {
    if w.title.trim().is_empty() {
        format!("Razer Axon {}", w.id)
    } else {
        w.title.trim().to_string()
    }
}

/// The line under the tile title: kind, sound, resolution; for those not downloaded —
/// that they are not on disk.
pub fn tile_line(w: &AxonWallpaper) -> String {
    let mut parts = vec![w.kind.label()];
    if w.audible && w.kind == AxonKind::Video {
        parts.push(tr("🔊 Sound"));
    }
    if let Some((x, y)) = w.resolution {
        parts.push(format!("{x}×{y}"));
    }
    if !w.downloaded {
        parts.push(tr("not downloaded"));
    }
    parts.join(" · ")
}

/// Filtering by the search string: title, author.
pub fn matches(w: &AxonWallpaper, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || format!("{} {}", w.title, w.author).to_lowercase().contains(&q)
}

// ── Texts ──────────────────────────────────────────────────────────────

/// The wallpaper is not downloaded in Axon yet.
pub fn not_downloaded_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("“%1” is not downloaded yet. Download it in Razer Axon first.", &[&name])
}

/// Rexon does not show web wallpapers.
pub fn web_unsupported_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("“%1” is a web page wallpaper — Rexon can't show this kind from Razer Axon yet.", &[&name])
}

/// Nothing to prepare with.
pub fn unavailable_text() -> String {
    tr("Razer Axon wallpapers can't be shown in this version of Rexon yet.")
}

/// The preparation does not know the wallpaper kind.
pub fn kind_unsupported_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("“%1”: this kind of Razer Axon wallpaper is not supported yet.", &[&name])
}

/// The wallpaper files are incomplete.
pub fn damaged_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("The files of “%1” look incomplete. Download the wallpaper again in Razer Axon.", &[&name])
}

/// Any other preparation failure.
pub fn prepare_failed_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("Could not prepare “%1” for playback.", &[&name])
}

/// Preparation started.
pub fn preparing_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("Preparing “%1”…", &[&name])
}

/// An empty library.
pub fn empty_text() -> String {
    tr("The Razer Axon library is empty: add wallpapers in Razer Axon.")
}

/// There is no library either in known places or in the specified folders.
pub fn not_found_text() -> String {
    tr("Razer Axon library not found. If Razer Axon is installed under Wine, \
        choose the folder with its wallpapers (WallpaperSource).")
}

/// The folder selection button.
pub fn pick_folder_text() -> String {
    tr("📁 Choose Axon wallpaper folder…")
}

/// The folder selection button tooltip and the chooser window title.
pub fn pick_folder_tooltip() -> String {
    tr("The WallpaperSource folder of Razer Axon: …/AppData/Local/Razer/RazerAxon/WallpaperSource \
        inside the Wine prefix where Razer Axon is installed.")
}

/// The folder chooser window title.
pub fn pick_folder_title() -> String {
    tr("Folder with Razer Axon wallpapers")
}

/// The "Open folder" button of the section and of the tile.
pub fn open_folder_text() -> String {
    tr("📂 Open folder")
}

/// The "Open folder" tooltip.
pub fn open_folder_tooltip() -> String {
    tr("Open the folder with these wallpapers in the file manager")
}

/// The chosen folder does not exist.
pub fn folder_missing_text() -> String {
    tr("This folder doesn't exist anymore.")
}

/// The chosen folder has no Axon wallpapers.
pub fn folder_not_axon_text() -> String {
    tr("No Razer Axon wallpapers in this folder. Choose the WallpaperSource folder \
        (…/AppData/Local/Razer/RazerAxon/WallpaperSource).")
}

/// The folder was added.
pub fn folder_added_text(count: usize) -> String {
    // TRANSLATORS: %1 is a number of wallpapers found in the chosen folder.
    trf("Razer Axon folder added: wallpapers found — %1.", &[&count])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_parses() {
        assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
        assert_eq!(parse_resolution(" 1920 X 1080 "), Some((1920, 1080)));
        assert_eq!(parse_resolution("None"), None);
        assert_eq!(parse_resolution("0x10"), None);
    }

    #[test]
    fn kind_parses_both_spellings() {
        assert_eq!(AxonKind::parse("video"), AxonKind::Video);
        assert_eq!(AxonKind::parse("LOCALWEB"), AxonKind::Web);
        assert_eq!(AxonKind::parse("X"), AxonKind::Other("X".into()));
    }

    #[test]
    fn relative_never_escapes() {
        let d = Path::new("/a/b");
        assert_eq!(resource_relative(d, "Resource\\x.mp4"), Some(PathBuf::from("/a/b/Resource/x.mp4")));
        assert_eq!(resource_relative(d, "..\\x.mp4"), None);
        assert_eq!(resource_relative(d, "/etc/passwd"), None);
    }

    #[test]
    fn sanitize_keeps_digits() {
        assert_eq!(sanitize_id("12034"), "12034");
        assert_eq!(sanitize_id("../x"), "___x");
        assert_eq!(sanitize_id(""), "_");
    }

    #[test]
    fn formats_by_extension() {
        assert_eq!(apply_format(Path::new("/c/a.MP4")), Some("video"));
        assert_eq!(apply_format(Path::new("/c/a.jpg")), Some("static"));
        assert_eq!(apply_format(Path::new("/c/a.gif")), Some("animated"));
        assert_eq!(apply_format(Path::new("/c/a.zip")), None);
    }
}
