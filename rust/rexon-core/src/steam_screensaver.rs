//! Rexon screensaver for the Steam Big Picture idle screen (stage 1 of the idle
//! screensaver).
//!
//! Steam looks for its screensavers in the folders
//! `<Steam>/config/uioverrides/screensavers/<name>/index.html` and shows them in
//! "Settings → Customization → Screensaver" as "<name> (User Created)". Steam
//! opens the page in its own browser, on a separate host; it has no disk access,
//! and the addresses are relative. So Rexon puts DOWNSCALED COPIES of the
//! pictures into the folder as a flat list next to `index.html` (the way the
//! built-in Steam screensaver is laid out: files next to the page) and
//! `manifest.js` with their list. A script, not JSON: `<script src>` loads both
//! in Steam and in a regular browser with `file://`, where `fetch` of a local
//! JSON is forbidden.
//!
//! What we take: art of recent games (`LastPlayed` in `localconfig.vdf`, the
//! pictures in `appcache/librarycache/<appid>/`, own art from
//! `userdata/<id>/config/grid` takes precedence) and the latest screenshots
//! (`userdata/<id>/760/screenshots.vdf`).
//!
//! ⛔ Rexon does NOT touch the screensaver choice in Steam
//! (`screensaver_current_id`): the person picks the screensaver in the Steam
//! settings. Other screensaver folders are not touched: our folder is recognized
//! by the marker file [`MARKER_FILE`]; without it Rexon will neither overwrite
//! nor delete a folder with our name. Rexon does not draw the Steam logo (a Valve
//! trademark) — only its own logo and game logos.
//!
//! Parsing and the plan are pure functions; execution ([`install`], [`remove`],
//! [`status`]) is at the end of the module.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::i18n::{ntr, tr, trf};

/// Name of the screensaver folder — Steam shows it in the list: "rexon (User
/// Created)".
///
/// ⛔ Lowercase letters only. Steam opens the screensaver at the address
/// `uioverride-<name>.steamscreensavers.host`, and the built-in browser
/// lowercases the host name: Steam looks for the folder `Rexon` as `rexon`,
/// doesn't find it on Linux and shows "Error code: -105" (verified live
/// 09.11.2026, Steam Big Picture).
pub const DIR_NAME: &str = "rexon";
/// Former names of our folder (with a capital letter — they did not open in
/// Steam). Install and removal clean up a folder with the Rexon marker under
/// such a name.
pub const LEGACY_DIR_NAMES: &[&str] = &["Rexon"];
/// The ID under which Steam remembers choosing this screensaver
/// (`uioverride-<name>`). For hints only: we never write it anywhere ourselves.
pub const STEAM_ID: &str = "uioverride-rexon";
/// The "Rexon made this folder" marker + the settings it was built with.
pub const MARKER_FILE: &str = "rexon-screensaver.json";
/// Version of the folder layout.
pub const LAYOUT_VERSION: u32 = 1;

/// The page, styles and script of the screensaver are in the binary, so the
/// folder is built the same way from the tree and from the package.
pub const PAGE_HTML: &str = include_str!("../../../data/steam-screensaver/index.html");
pub const PAGE_CSS: &str = include_str!("../../../data/steam-screensaver/screensaver.css");
pub const PAGE_JS: &str = include_str!("../../../data/steam-screensaver/screensaver.js");
/// Rexon logo for the "bouncing logo".
pub const REXON_LOGO_PNG: &[u8] = include_bytes!("../../../rexon_icon_512.png");
pub const REXON_LOGO_NAME: &str = "rexon-logo.png";

/// A slideshow frame is fitted into this (Full HD is enough for both the Deck and a TV).
pub const PHOTO_MAX: (u32, u32) = (1920, 1080);
/// Game logo over the frame.
pub const LOGO_MAX: (u32, u32) = (960, 540);
/// Blurred background under a vertical cover: small, it is blurred anyway.
pub const BACKDROP_MAX: (u32, u32) = (640, 360);
pub const JPEG_QUALITY: u8 = 85;

// ============================================================================
// Settings
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Art of recent games and screenshots.
    #[default]
    Slideshow,
    /// Bouncing logo.
    Bounce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogoChoice {
    /// Rexon logo (default).
    #[default]
    Rexon,
    /// Logos of recent games in turn.
    Games,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub mode: Mode,
    pub logo: LogoChoice,
    pub game_art: bool,
    pub screenshots: bool,
    pub clock: bool,
    /// Length of a slideshow frame, seconds.
    pub interval_s: u32,
    pub max_games: usize,
    pub max_screenshots: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Slideshow,
            logo: LogoChoice::Rexon,
            game_art: true,
            screenshots: true,
            clock: true,
            interval_s: 9,
            max_games: 16,
            max_screenshots: 16,
        }
    }
}

// ============================================================================
// Text VDF (KeyValues)
// ============================================================================

/// A text VDF node: a string or a nested block (key order preserved).
#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Str(String),
    Obj(Vec<(String, Kv)>),
}

impl Kv {
    /// Child node by key, case-insensitive (Steam writes keys in different ways).
    pub fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Obj(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::Str(_) => None,
        }
    }
    pub fn path(&self, keys: &[&str]) -> Option<&Kv> {
        keys.iter().try_fold(self, |node, k| node.get(k))
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Kv::Str(s) => Some(s),
            Kv::Obj(_) => None,
        }
    }
    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Kv::str)
    }
    pub fn entries(&self) -> &[(String, Kv)] {
        match self {
            Kv::Obj(items) => items,
            Kv::Str(_) => &[],
        }
    }
}

/// Parse a text VDF into the root block. A broken tail does not break parsing:
/// whatever was read is returned (Steam writes the files itself, but a file
/// truncated by a crash must not break the whole screensaver).
pub fn parse_kv(text: &str) -> Kv {
    let tokens = kv_tokens(text);
    let mut pos = 0;
    Kv::Obj(kv_block(&tokens, &mut pos))
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

fn kv_tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '"' => break,
                        '\\' => match it.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        o => s.push(o),
                    }
                }
                out.push(Tok::Str(s));
            }
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '/' if it.peek() == Some(&'/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                // A key without quotes (occurs in old files).
                let mut s = String::from(c);
                while let Some(&n) = it.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    it.next();
                }
                out.push(Tok::Str(s));
            }
        }
    }
    out
}

fn kv_block(tokens: &[Tok], pos: &mut usize) -> Vec<(String, Kv)> {
    let mut items = Vec::new();
    while *pos < tokens.len() {
        match &tokens[*pos] {
            Tok::Close => {
                *pos += 1;
                return items;
            }
            Tok::Open => {
                // A block without a key — skip its contents.
                *pos += 1;
                kv_block(tokens, pos);
            }
            Tok::Str(key) => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Tok::Str(v)) => {
                        items.push((key.clone(), Kv::Str(v.clone())));
                        *pos += 1;
                    }
                    Some(Tok::Open) => {
                        *pos += 1;
                        let child = kv_block(tokens, pos);
                        items.push((key.clone(), Kv::Obj(child)));
                    }
                    _ => {}
                }
            }
        }
    }
    items
}

// ============================================================================
// What Steam has
// ============================================================================

/// A screenshot from the `760/screenshots.vdf` listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub appid: String,
    /// Path relative to `userdata/<id>/760/remote/`.
    pub filename: String,
    /// Time of the shot (unix).
    pub creation: i64,
}

/// The screenshot listing, newest first. Shortcuts of non-Steam games (huge ids)
/// and records without a file are skipped not here but when building the plan —
/// by whether the file exists.
pub fn parse_screenshots(text: &str) -> Vec<Shot> {
    let root = parse_kv(text);
    let Some(shots) = root.get("screenshots") else { return Vec::new() };
    let mut out = Vec::new();
    for (appid, block) in shots.entries() {
        if !appid.chars().all(|c| c.is_ascii_digit()) {
            continue; // shortcutnames and other service blocks
        }
        for (_, item) in block.entries() {
            let Some(filename) = item.str_at("filename") else { continue };
            if filename.is_empty() || filename.contains("..") {
                continue;
            }
            let creation = item.str_at("creation").and_then(|s| s.parse().ok()).unwrap_or(0);
            out.push(Shot { appid: appid.clone(), filename: filename.to_string(), creation });
        }
    }
    out.sort_by(|a, b| b.creation.cmp(&a.creation).then_with(|| a.filename.cmp(&b.filename)));
    out
}

/// Recently launched games from `localconfig.vdf`: `(appid, LastPlayed)`,
/// newest first; without `LastPlayed` or with zero — not "recent".
pub fn parse_last_played(text: &str) -> Vec<(String, i64)> {
    let root = parse_kv(text);
    let Some(apps) = root.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else {
        return Vec::new();
    };
    let mut out: Vec<(String, i64)> = apps
        .entries()
        .iter()
        .filter(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(id, node)| {
            let t: i64 = node.str_at("LastPlayed")?.parse().ok()?;
            (t > 0).then(|| (id.clone(), t))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Library paths from `steamapps/libraryfolders.vdf`.
pub fn parse_library_paths(text: &str) -> Vec<String> {
    let root = parse_kv(text);
    let Some(folders) = root.get("libraryfolders") else { return Vec::new() };
    folders
        .entries()
        .iter()
        .filter_map(|(_, node)| node.str_at("path").map(str::to_string))
        .filter(|p| !p.is_empty())
        .collect()
}

/// Game title from `appmanifest_<appid>.acf`.
pub fn parse_manifest_name(text: &str) -> Option<String> {
    let root = parse_kv(text);
    let name = root.get("AppState")?.str_at("name")?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Pictures of one game: file names in its cache folder or full paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Art {
    pub hero: Option<String>,
    pub logo: Option<String>,
    pub portrait: Option<String>,
    pub header: Option<String>,
}

impl Art {
    pub fn is_empty(&self) -> bool {
        self.hero.is_none() && self.portrait.is_none() && self.header.is_none()
    }
}

/// Picking pictures from the `appcache/librarycache/<appid>/` listing: first the
/// variant in the Steam language (`*_russian.*`), then the base one. The blurred
/// hero copy (`library_hero_blur`) does not count as a hero.
pub fn pick_cache_art(names: &[String], steam_lang: &str) -> Art {
    let set: HashSet<&str> = names.iter().map(String::as_str).collect();
    let find = |stems: &[&str], exts: &[&str]| -> Option<String> {
        for stem in stems {
            let mut variants = Vec::new();
            if !steam_lang.is_empty() && steam_lang != "english" {
                variants.push(format!("{stem}_{steam_lang}"));
            }
            variants.push((*stem).to_string());
            for v in &variants {
                for ext in exts {
                    let name = format!("{v}.{ext}");
                    if set.contains(name.as_str()) {
                        return Some(name);
                    }
                }
            }
        }
        None
    };
    Art {
        hero: find(&["library_hero"], &["jpg", "png"]),
        logo: find(&["logo"], &["png"]),
        portrait: find(&["library_600x900", "library_capsule"], &["jpg", "png"]),
        header: find(&["header", "library_header"], &["jpg", "png"]),
    }
}

/// Own art from `userdata/<id>/config/grid` (the "Set custom artwork" button in
/// the library): `<appid>_hero.*`, `<appid>_logo.*`, `<appid>p.*`, `<appid>.*`.
pub fn pick_grid_art(names: &[String], appid: &str) -> Art {
    let find = |stem: String| -> Option<String> {
        ["png", "jpg", "jpeg", "webp"]
            .iter()
            .map(|ext| format!("{stem}.{ext}"))
            .find(|n| names.iter().any(|x| x == n))
    };
    Art {
        hero: find(format!("{appid}_hero")),
        logo: find(format!("{appid}_logo")),
        portrait: find(format!("{appid}p")),
        header: find(appid.to_string()),
    }
}

/// Own art over the cache: whatever the person set is what we take.
pub fn merge_art(grid: Art, cache: Art) -> Art {
    Art {
        hero: grid.hero.or(cache.hero),
        logo: grid.logo.or(cache.logo),
        portrait: grid.portrait.or(cache.portrait),
        header: grid.header.or(cache.header),
    }
}

// ============================================================================
// Plan: what to show and where to copy it
// ============================================================================

/// How to downscale a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Frame: fit into [`PHOTO_MAX`], JPEG.
    Photo,
    /// Logo: fit into [`LOGO_MAX`], PNG with transparency.
    Logo,
    /// Blurred background under a cover: [`BACKDROP_MAX`], blur, JPEG.
    Backdrop,
}

/// One copy: from where, under which name in the screensaver folder and how to shrink it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub src: PathBuf,
    pub name: String,
    pub fit: Fit,
}

/// A candidate game: the found art with full paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameArt {
    pub appid: String,
    pub title: String,
    pub art: Art,
}

/// A screenshot with its full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotFile {
    pub appid: String,
    pub title: String,
    pub path: PathBuf,
    pub creation: i64,
}

/// Result of the plan: the list of copies and the manifest for the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub copies: Vec<Copy>,
    pub manifest: Value,
}

/// Build the plan from what was found. Frames are interleaved: game art,
/// screenshot, game art… — until one runs out, then the rest of the other.
pub fn build_plan(games: &[GameArt], shots: &[ShotFile], opts: &Options, locale: &str) -> Plan {
    let mut copies: Vec<Copy> = Vec::new();
    let mut by_src: HashMap<(PathBuf, &'static str), String> = HashMap::new();
    let mut add = |copies: &mut Vec<Copy>, src: &Path, fit: Fit| -> String {
        let tag = match fit {
            Fit::Photo => "img",
            Fit::Logo => "logo",
            Fit::Backdrop => "blur",
        };
        if let Some(name) = by_src.get(&(src.to_path_buf(), tag)) {
            return name.clone();
        }
        let ext = if fit == Fit::Logo { "png" } else { "jpg" };
        let name = format!("{tag}-{:03}.{ext}", copies.iter().filter(|c| c.fit == fit).count() + 1);
        copies.push(Copy { src: src.to_path_buf(), name: name.clone(), fit });
        by_src.insert((src.to_path_buf(), tag), name.clone());
        name
    };

    let mut art_slides = Vec::new();
    let mut logos = Vec::new();
    for g in games.iter().take(opts.max_games) {
        let logo = g.art.logo.as_ref().map(|p| add(&mut copies, Path::new(p), Fit::Logo));
        if let Some(l) = &logo {
            logos.push(l.clone());
        }
        if !opts.game_art {
            continue;
        }
        let slide = if let Some(hero) = &g.art.hero {
            json!({"kind": "art", "appid": g.appid, "title": g.title,
                   "image": add(&mut copies, Path::new(hero), Fit::Photo),
                   "logo": logo, "framed": false})
        } else if let Some(cover) = g.art.portrait.as_ref().or(g.art.header.as_ref()) {
            json!({"kind": "art", "appid": g.appid, "title": g.title,
                   "image": add(&mut copies, Path::new(cover), Fit::Photo),
                   "backdrop": add(&mut copies, Path::new(cover), Fit::Backdrop),
                   "logo": Value::Null, "framed": true})
        } else {
            continue;
        };
        art_slides.push(slide);
    }

    let mut shot_slides = Vec::new();
    if opts.screenshots {
        for s in shots.iter().take(opts.max_screenshots) {
            shot_slides.push(json!({"kind": "shot", "appid": s.appid, "title": s.title,
                "image": add(&mut copies, &s.path, Fit::Photo), "when": s.creation}));
        }
    }

    let mut slides = Vec::new();
    let (mut a, mut b) = (art_slides.into_iter(), shot_slides.into_iter());
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() {
            break;
        }
        slides.extend(x);
        slides.extend(y);
    }

    let manifest = json!({
        "version": LAYOUT_VERSION,
        "mode": opts.mode,
        "logo": opts.logo,
        "clock": opts.clock,
        "interval": opts.interval_s.clamp(INTERVAL_MIN, INTERVAL_MAX),
        "locale": locale,
        "rexonLogo": REXON_LOGO_NAME,
        "logos": logos,
        "slides": slides,
    });
    Plan { copies, manifest }
}

/// Text of `manifest.js`. `</` is escaped so that a game title cannot close the
/// script tag.
pub fn manifest_js(manifest: &Value) -> String {
    let body = serde_json::to_string_pretty(manifest).unwrap_or_else(|_| "{}".into());
    format!(
        "// Создано Rexon. Набор картинок заставки — обновляется из Rexon.\n\
         window.REXON_SCREENSAVER = {};\n",
        body.replace("</", "<\\/")
    )
}

/// UI language → tag for the page's `Intl` (`ru_RU.UTF-8` → `ru-RU`).
pub fn page_locale(ui_language: &str) -> String {
    let base = ui_language.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    if base.is_empty() || base == "C" || base == "POSIX" {
        "en".into()
    } else {
        base
    }
}

/// Steam screensavers folder in the root `root`.
pub fn screensavers_dir(root: &Path) -> PathBuf {
    root.join("config").join("uioverrides").join("screensavers")
}

/// The Rexon screensaver folder.
pub fn target_dir(root: &Path) -> PathBuf {
    screensavers_dir(root).join(DIR_NAME)
}

/// Folders of former names with the Rexon marker. On a case-insensitive file
/// system the former name is the same folder as [`target_dir`]; it is not
/// counted.
fn legacy_dirs(root: &Path) -> Vec<(PathBuf, Marker)> {
    let target = std::fs::canonicalize(target_dir(root)).ok();
    LEGACY_DIR_NAMES
        .iter()
        .map(|n| screensavers_dir(root).join(n))
        .filter(|d| target.is_none() || std::fs::canonicalize(d).ok() != target)
        .filter_map(|d| read_marker(&d).map(|m| (d, m)))
        .collect()
}

/// Where the installed Rexon screensaver is: our own folder or the folder of the
/// former name (until the first «Update Screensaver»).
pub fn installed_dir(root: &Path) -> Option<PathBuf> {
    let target = target_dir(root);
    if read_marker(&target).is_some() {
        return Some(target);
    }
    legacy_dirs(root).into_iter().next().map(|(d, _)| d)
}

/// Steam roots where the screensaver can be installed: the usual one and Flatpak
/// (the shared list [`crate::steam_token::steam_roots`], already without
/// duplicates by realpath) — only real client installations that have `config/`.
pub fn steam_roots(home: &str) -> Vec<PathBuf> {
    crate::steam_token::steam_roots(home).into_iter().filter(|r| r.join("config").is_dir()).collect()
}

// ============================================================================
// Execution
// ============================================================================

/// Folder marker: who built it, with which settings, what was created along the way.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Marker {
    pub owner: String,
    pub layout: u32,
    pub options: Options,
    pub slides: usize,
    pub updated: i64,
    /// Folders that did not exist before Rexon (`uioverrides`, `screensavers`):
    /// on removal we remove them if they are empty.
    pub created_dirs: Vec<String>,
}

/// State in one Steam root.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub root: String,
    pub installed: bool,
    /// A folder with our name exists, but Rexon did not make it — don't touch it.
    pub foreign: bool,
    pub slides: usize,
    pub updated: i64,
    pub options: Options,
}

fn read_marker(dir: &Path) -> Option<Marker> {
    let text = std::fs::read_to_string(dir.join(MARKER_FILE)).ok()?;
    let m: Marker = serde_json::from_str(&text).ok()?;
    (m.owner == "rexon").then_some(m)
}

pub fn status(root: &Path) -> Status {
    let dir = target_dir(root);
    let own = read_marker(&dir);
    let foreign = own.is_none() && dir.exists();
    // Folder of the former name: shown as installed; «Update Screensaver» moves
    // it under the new name with the same settings.
    let marker = own.or_else(|| legacy_dirs(root).into_iter().next().map(|(_, m)| m));
    Status {
        root: root.display().to_string(),
        installed: marker.is_some() && !foreign,
        foreign,
        slides: marker.as_ref().map_or(0, |m| m.slides),
        updated: marker.as_ref().map_or(0, |m| m.updated),
        options: marker.map(|m| m.options).unwrap_or_default(),
    }
}

fn list_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// The Steam account (folder `userdata/<accountid>`) — the one whose
/// `localconfig.vdf` is newer; the same choice as the tab's
/// ([`crate::qt_steam::pick_account`]).
pub fn active_account(root: &Path) -> Option<String> {
    let userdata = root.join("userdata");
    let entries: Vec<(String, f64)> = list_names(&userdata)
        .into_iter()
        .filter_map(|id| {
            let m = std::fs::metadata(userdata.join(&id).join("config/localconfig.vdf")).ok()?;
            let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
            Some((id, t))
        })
        .collect();
    let id64: i64 = crate::qt_steam::pick_account(&entries)?.parse().ok()?;
    Some((id64 - crate::qt_steam::STEAMID64_BASE).to_string())
}

/// Game titles from the manifests of all libraries.
fn game_titles(root: &Path) -> HashMap<String, String> {
    let mut libs = vec![root.display().to_string()];
    if let Ok(t) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        libs.extend(parse_library_paths(&t));
    }
    let mut out = HashMap::new();
    let mut seen = HashSet::new();
    for lib in libs {
        let dir = Path::new(&lib).join("steamapps");
        if !seen.insert(dir.clone()) {
            continue;
        }
        for name in list_names(&dir) {
            let Some(id) = name.strip_prefix("appmanifest_").and_then(|n| n.strip_suffix(".acf")) else {
                continue;
            };
            if let Some(title) = std::fs::read_to_string(dir.join(&name)).ok().and_then(|t| parse_manifest_name(&t)) {
                out.insert(id.to_string(), title);
            }
        }
    }
    out
}

/// Collect candidates from the Steam disk (read-only).
pub fn collect(root: &Path, steam_lang: &str) -> (Vec<GameArt>, Vec<ShotFile>) {
    let Some(account) = active_account(root) else { return (Vec::new(), Vec::new()) };
    let user = root.join("userdata").join(&account);
    let titles = game_titles(root);
    let title_of = |id: &str| titles.get(id).cloned().unwrap_or_default();

    let grid_dir = user.join("config/grid");
    let grid_names = list_names(&grid_dir);
    let cache = root.join("appcache/librarycache");
    let recent = std::fs::read_to_string(user.join("config/localconfig.vdf"))
        .map(|t| parse_last_played(&t))
        .unwrap_or_default();
    let mut games = Vec::new();
    for (appid, _) in recent {
        let cdir = cache.join(&appid);
        let full = |base: &Path, n: Option<String>| n.map(|n| base.join(n).display().to_string());
        let c = pick_cache_art(&list_names(&cdir), steam_lang);
        let g = pick_grid_art(&grid_names, &appid);
        let cache_art = Art {
            hero: full(&cdir, c.hero),
            logo: full(&cdir, c.logo),
            portrait: full(&cdir, c.portrait),
            header: full(&cdir, c.header),
        };
        let grid_art = Art {
            hero: full(&grid_dir, g.hero),
            logo: full(&grid_dir, g.logo),
            portrait: full(&grid_dir, g.portrait),
            header: full(&grid_dir, g.header),
        };
        let art = merge_art(grid_art, cache_art);
        if art.is_empty() {
            continue; // tools and runtimes without art
        }
        games.push(GameArt { title: title_of(&appid), appid, art });
    }

    let remote = user.join("760/remote");
    let shots = std::fs::read_to_string(user.join("760/screenshots.vdf"))
        .map(|t| parse_screenshots(&t))
        .unwrap_or_default()
        .into_iter()
        .map(|s| ShotFile { title: title_of(&s.appid), path: remote.join(&s.filename), appid: s.appid, creation: s.creation })
        .filter(|s| s.path.is_file())
        .collect();
    (games, shots)
}

/// Downscale a picture for the screensaver.
pub fn write_fitted(src: &Path, out: &Path, fit: Fit) -> Result<(), String> {
    let img = image::ImageReader::open(src)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let (mw, mh) = match fit {
        Fit::Photo => PHOTO_MAX,
        Fit::Logo => LOGO_MAX,
        Fit::Backdrop => BACKDROP_MAX,
    };
    let img = if img.width() > mw || img.height() > mh {
        img.resize(mw, mh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    match fit {
        Fit::Logo => img.to_rgba8().write_to(&mut w, image::ImageFormat::Png).map_err(|e| e.to_string()),
        Fit::Photo | Fit::Backdrop => {
            let rgb = if fit == Fit::Backdrop { img.blur(18.0).to_rgb8() } else { img.to_rgb8() };
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, JPEG_QUALITY);
            rgb.write_with_encoder(enc).map_err(|e| e.to_string())
        }
    }
}

/// Result of the install in one root.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstallReport {
    pub root: String,
    pub dir: String,
    pub slides: usize,
    pub skipped: usize,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Write the finished screensaver folder (page, logo, copies, manifest, marker)
/// into `dir`, which does not exist yet. Frames whose pictures could not be
/// opened are dropped from the manifest. Returns (frames, skipped).
pub fn write_folder(dir: &Path, plan: &Plan, marker: &Marker) -> Result<(usize, usize), String> {
    std::fs::create_dir_all(dir).map_err(|e| trf("couldn't create %1: %2", &[&dir.display(), &e]))?;
    let put = |name: &str, data: &[u8]| {
        std::fs::write(dir.join(name), data).map_err(|e| trf("couldn't write %1: %2", &[&name, &e]))
    };
    put("index.html", PAGE_HTML.as_bytes())?;
    put("screensaver.css", PAGE_CSS.as_bytes())?;
    put("screensaver.js", PAGE_JS.as_bytes())?;
    put(REXON_LOGO_NAME, REXON_LOGO_PNG)?;

    let mut failed: HashSet<String> = HashSet::new();
    for c in &plan.copies {
        if write_fitted(&c.src, &dir.join(&c.name), c.fit).is_err() {
            let _ = std::fs::remove_file(dir.join(&c.name));
            failed.insert(c.name.clone());
        }
    }
    let mut manifest = plan.manifest.clone();
    let mut skipped = 0;
    if let Some(slides) = manifest.get_mut("slides").and_then(Value::as_array_mut) {
        let before = slides.len();
        slides.retain(|s| {
            ["image", "backdrop"].iter().all(|k| s.get(*k).and_then(Value::as_str).is_none_or(|n| !failed.contains(n)))
        });
        for s in slides.iter_mut() {
            if s.get("logo").and_then(Value::as_str).is_some_and(|n| failed.contains(n)) {
                s["logo"] = Value::Null;
            }
        }
        skipped = before - slides.len();
    }
    if let Some(logos) = manifest.get_mut("logos").and_then(Value::as_array_mut) {
        logos.retain(|l| l.as_str().is_some_and(|n| !failed.contains(n)));
    }
    let slides = manifest.get("slides").and_then(Value::as_array).map_or(0, Vec::len);
    put("manifest.js", manifest_js(&manifest).as_bytes())?;
    let marker = Marker { slides, ..marker.clone() };
    let text = serde_json::to_string_pretty(&marker).unwrap_or_default();
    put(MARKER_FILE, text.as_bytes())?;
    Ok((slides, skipped))
}

/// Install or update the screensaver in the Steam root `root`.
///
/// The folder is built next to it in a temporary one and swapped in whole: a
/// Steam that opened the screensaver at that minute won't see a half-empty
/// folder. Someone else's folder with our name (without the marker) is not
/// touched.
pub fn install(root: &Path, opts: &Options, steam_lang: &str, locale: &str) -> Result<InstallReport, String> {
    let parent = screensavers_dir(root);
    let target = target_dir(root);
    let old_marker = read_marker(&target);
    if target.exists() && old_marker.is_none() {
        return Err(trf(
            "The folder %1 was not made by Rexon — it was left untouched.",
            &[&target.display()],
        ));
    }
    // Which folders we create for the first time — to restore things as they were on removal.
    let legacy = legacy_dirs(root);
    let old_marker = old_marker.or_else(|| legacy.first().map(|(_, m)| m.clone()));
    let mut created_dirs = old_marker.as_ref().map(|m| m.created_dirs.clone()).unwrap_or_default();
    if old_marker.is_none() {
        for d in [root.join("config/uioverrides"), parent.clone()] {
            if !d.exists() {
                created_dirs.push(d.display().to_string());
            }
        }
    }
    std::fs::create_dir_all(&parent).map_err(|e| trf("couldn't create %1: %2", &[&parent.display(), &e]))?;

    let (games, shots) = collect(root, steam_lang);
    let plan = build_plan(&games, &shots, opts, locale);
    let pid = std::process::id();
    let staging = parent.join(format!(".{DIR_NAME}.new-{pid}"));
    let _ = std::fs::remove_dir_all(&staging);
    let marker = Marker {
        owner: "rexon".into(),
        layout: LAYOUT_VERSION,
        options: opts.clone(),
        slides: 0,
        updated: now_unix(),
        created_dirs,
    };
    let (slides, skipped) = match write_folder(&staging, &plan, &marker) {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let old = parent.join(format!(".{DIR_NAME}.old-{pid}"));
    if target.exists() {
        std::fs::rename(&target, &old).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            trf("couldn't replace %1: %2", &[&target.display(), &e])
        })?;
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        // Put the previous folder back.
        let _ = std::fs::rename(&old, &target);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(trf("couldn't replace %1: %2", &[&target.display(), &e]));
    }
    let _ = std::fs::remove_dir_all(&old);
    for (d, _) in &legacy {
        let _ = std::fs::remove_dir_all(d);
    }
    Ok(InstallReport { root: root.display().to_string(), dir: target.display().to_string(), slides, skipped })
}

/// Remove the Rexon screensaver from the root `root`. `Ok(false)` — it was not
/// there. The `uioverrides`/`screensavers` folders created by Rexon are removed
/// only if nothing else is left in them.
pub fn remove(root: &Path) -> Result<bool, String> {
    let target = target_dir(root);
    let own = read_marker(&target);
    if own.is_none() && target.exists() {
        return Err(trf(
            "The folder %1 was not made by Rexon — it was left untouched.",
            &[&target.display()],
        ));
    }
    let mut found: Vec<(PathBuf, Marker)> = own.map(|m| (target.clone(), m)).into_iter().collect();
    found.extend(legacy_dirs(root));
    let Some(marker) = found.first().map(|(_, m)| m.clone()) else {
        return Ok(false);
    };
    for (d, _) in &found {
        std::fs::remove_dir_all(d).map_err(|e| trf("couldn't remove %1: %2", &[&d.display(), &e]))?;
    }
    // First the nested one (screensavers), then uioverrides; remove_dir won't
    // touch a non-empty folder.
    let mut dirs = marker.created_dirs.clone();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for d in dirs {
        let p = PathBuf::from(&d);
        if p.starts_with(root.join("config")) {
            let _ = std::fs::remove_dir(p);
        }
    }
    Ok(true)
}

/// Hint for the person: where to choose the screensaver in Steam.
pub fn how_to_select() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("In Steam Big Picture open Settings → Customization → Screensaver and choose «rexon (User Created)». \
        Steam keeps your choice; Rexon never changes it.")
}

// ============================================================================
// The «🌙 Screensaver» subtab of the «Steam» tab: choices, captions, settings parsing
// ============================================================================
//
// The markup (`SteamScreensaverSection.qml`) and its executor
// (`rexon-gui::steam_screensaver_actions`) decide nothing: the options, limits
// and all phrases for the person are here.

/// Limits of the slideshow frame length, seconds (the page holds the same ones).
pub const INTERVAL_MIN: u32 = 4;
pub const INTERVAL_MAX: u32 = 60;

/// A choice option for the markup: key, caption, explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub key: &'static str,
    pub label: String,
    pub hint: String,
}

/// What the screensaver shows.
pub fn mode_choices() -> Vec<Choice> {
    vec![
        Choice {
            key: "slideshow",
            label: tr("Slideshow"),
            hint: tr("Artwork of your recently played games and your screenshots take turns."),
        },
        Choice {
            key: "bounce",
            label: tr("Bouncing logo"),
            hint: tr("A logo floats across the screen and bounces off the edges."),
        },
    ]
}

/// Which logo bounces. There is no Steam logo here and there won't be: it is a
/// Valve trademark.
pub fn logo_choices() -> Vec<Choice> {
    vec![
        Choice { key: "rexon", label: tr("Rexon logo"), hint: tr("Changes colour every time it hits an edge.") },
        Choice {
            key: "games",
            label: tr("Logos of your games"),
            hint: tr("After each bounce off an edge the next recently played game is shown."),
        },
    ]
}

impl Options {
    /// Settings sent by the markup (JSON) over `base`. Unknown and empty values are
    /// taken from `base`, the frame length is clamped to the limits.
    pub fn from_ui(v: &Value, base: &Options) -> Options {
        let flag = |key: &str, dflt: bool| v.get(key).and_then(Value::as_bool).unwrap_or(dflt);
        let mode = match v.get("mode").and_then(Value::as_str) {
            Some("slideshow") => Mode::Slideshow,
            Some("bounce") => Mode::Bounce,
            _ => base.mode,
        };
        let logo = match v.get("logo").and_then(Value::as_str) {
            Some("rexon") => LogoChoice::Rexon,
            Some("games") => LogoChoice::Games,
            _ => base.logo,
        };
        let interval = v
            .get("interval")
            .and_then(Value::as_f64)
            .filter(|x| x.is_finite())
            .map(|x| x.round().clamp(INTERVAL_MIN as f64, INTERVAL_MAX as f64) as u32)
            .unwrap_or(base.interval_s);
        Options {
            mode,
            logo,
            game_art: flag("gameArt", base.game_art),
            screenshots: flag("screenshots", base.screenshots),
            clock: flag("clock", base.clock),
            interval_s: interval.clamp(INTERVAL_MIN, INTERVAL_MAX),
            ..base.clone()
        }
    }

    /// Settings for the markup — the same keys that [`Options::from_ui`] accepts.
    pub fn to_ui(&self) -> Value {
        json!({
            "mode": self.mode,
            "logo": self.logo,
            "gameArt": self.game_art,
            "screenshots": self.screenshots,
            "clock": self.clock,
            "interval": self.interval_s,
        })
    }
}

/// How to name a Steam root to the person: the usual one or Flatpak.
pub fn root_label(root: &Path) -> String {
    if root.to_string_lossy().contains("/.var/app/com.valvesoftware.Steam/") {
        tr("Steam (Flatpak)")
    } else {
        "Steam".into()
    }
}

/// Status line of the subtab across all Steam roots.
pub fn status_text(statuses: &[Status]) -> String {
    if statuses.is_empty() {
        return tr("Steam was not found on this computer — install Steam and start it once.");
    }
    if let Some(st) = statuses.iter().find(|s| s.installed) {
        return if st.slides == 0 {
            tr("✅ Installed. No pictures were found, so the bouncing logo is shown.")
        } else {
            ntr("✅ Installed — %n picture.", "✅ Installed — %n pictures.", st.slides as u64)
        };
    }
    if statuses.iter().any(|s| s.foreign) {
        return tr("Steam already has another screensaver called «Rexon». Rexon did not make it and leaves it alone.");
    }
    tr("⚪ Not installed yet.")
}

/// What of the found will go into the slideshow: (frames with game art, screenshots).
pub fn planned_counts(games: &[GameArt], shots: &[ShotFile], opts: &Options) -> (usize, usize) {
    let plan = build_plan(games, shots, opts, "en");
    let slides = plan.manifest["slides"].as_array().cloned().unwrap_or_default();
    let art = slides.iter().filter(|s| s["kind"] == "art").count();
    (art, slides.len() - art)
}

/// «Found in your Steam: …» — how many pictures there will be.
pub fn found_text(art: usize, shots: usize) -> String {
    if art == 0 && shots == 0 {
        return tr("No game artwork or screenshots were found in Steam yet. Play a game or take a screenshot (F12) — \
                   until then the screensaver shows the bouncing logo.");
    }
    trf(
        "Found in your Steam: %1 and %2.",
        &[
            &ntr("%n game picture", "%n game pictures", art as u64),
            &ntr("%n screenshot", "%n screenshots", shots as u64),
        ],
    )
}

/// Message after installing or updating into all roots.
pub fn installed_message(reports: &[InstallReport], errors: &[String]) -> String {
    if reports.is_empty() {
        return errors.first().cloned().unwrap_or_else(|| tr("Steam was not found on this computer."));
    }
    let mut msg = tr("The screensaver is ready. Now choose it in Steam — see the steps below.");
    if let Some(e) = errors.first() {
        msg.push(' ');
        msg.push_str(e);
    }
    msg
}

/// Message after removal.
pub fn removed_message(removed: usize, errors: &[String]) -> String {
    match (removed, errors.first()) {
        (_, Some(e)) => e.clone(),
        (0, None) => tr("The screensaver was not installed — there was nothing to remove."),
        _ => tr("The screensaver was removed from Steam."),
    }
}

/// Hint if the screensaver is not in the Steam list.
pub fn restart_hint() -> String {
    tr("Not in the list? Close Steam completely and start it again.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    const SHOTS_VDF: &str = r#""screenshots"
{
	"4734170"
	{
		"0"
		{
			"type"		"1"
			"filename"		"4734170/screenshots/20260623225805_1.jpg"
			"creation"		"1782248285"
		}
		"1"
		{
			"filename"		"4734170/screenshots/20260623225350_1.jpg"
			"creation"		"1782248030"
		}
	}
	"20"
	{
		"0" { "filename" "20/screenshots/a.jpg" "creation" "1782248300" }
		"1" { "filename" "../etc/passwd" "creation" "1" }
	}
	"shortcutnames"
	{
		"12375318956259409920"		"EEKeeper.exe"
	}
}"#;

    #[test]
    fn screenshots_newest_first_and_safe() {
        let v = parse_screenshots(SHOTS_VDF);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].filename, "20/screenshots/a.jpg");
        assert_eq!(v[1].creation, 1782248285);
        assert!(v.iter().all(|s| !s.filename.contains("..")));
    }

    #[test]
    fn last_played_from_localconfig() {
        let text = r#""UserLocalConfigStore"
{
	"Software" { "Valve" { "steam" { "apps" {
		"20" { "LastPlayed" "1474676422" "Playtime" "1" }
		"70" { "LastPlayed" "1590152417" }
		"80" { "Playtime" "5" }
		"cloud" { "LastPlayed" "9" }
		"90" { "LastPlayed" "0" }
	} } } }
}"#;
        assert_eq!(
            parse_last_played(text),
            vec![("70".to_string(), 1590152417), ("20".to_string(), 1474676422)]
        );
    }

    #[test]
    fn kv_escapes_comments_and_broken_tail() {
        let k = parse_kv("// c\n\"a\" { \"b\" \"x\\\"y\" key value } \"c\" { \"d\"");
        assert_eq!(k.path(&["a", "b"]).and_then(Kv::str), Some("x\"y"));
        assert_eq!(k.path(&["a", "key"]).and_then(Kv::str), Some("value"));
        assert!(k.get("c").is_some());
    }

    #[test]
    fn library_paths_and_manifest_name() {
        let lf = "\"libraryfolders\" { \"0\" { \"path\" \"/home/u/.local/share/Steam\" } \"1\" { \"path\" \"/mnt/g\" } }";
        assert_eq!(parse_library_paths(lf), s(&["/home/u/.local/share/Steam", "/mnt/g"]));
        assert_eq!(
            parse_manifest_name("\"AppState\" { \"appid\" \"70\" \"name\" \"Half-Life\" }").as_deref(),
            Some("Half-Life")
        );
    }

    #[test]
    fn cache_art_prefers_language_and_skips_blur() {
        let names = s(&["header.jpg", "library_hero_blur.jpg", "logo.png", "logo_russian.png", "library_600x900.jpg"]);
        let a = pick_cache_art(&names, "russian");
        assert_eq!(a.hero, None);
        assert_eq!(a.logo.as_deref(), Some("logo_russian.png"));
        assert_eq!(a.portrait.as_deref(), Some("library_600x900.jpg"));
        assert_eq!(pick_cache_art(&names, "english").logo.as_deref(), Some("logo.png"));
    }

    #[test]
    fn grid_art_wins_over_cache() {
        let g = pick_grid_art(&s(&["70_hero.png", "70p.jpg", "80_hero.png"]), "70");
        let c = Art { hero: Some("h.jpg".into()), logo: Some("l.png".into()), ..Art::default() };
        let m = merge_art(g, c);
        assert_eq!(m.hero.as_deref(), Some("70_hero.png"));
        assert_eq!(m.logo.as_deref(), Some("l.png"));
        assert_eq!(m.portrait.as_deref(), Some("70p.jpg"));
    }

    fn game(id: &str, hero: bool) -> GameArt {
        GameArt {
            appid: id.into(),
            title: format!("Game {id}"),
            art: Art {
                hero: hero.then(|| format!("/c/{id}/library_hero.jpg")),
                logo: Some(format!("/c/{id}/logo.png")),
                portrait: Some(format!("/c/{id}/library_600x900.jpg")),
                header: None,
            },
        }
    }

    #[test]
    fn plan_interleaves_and_names_flat_files() {
        let shots = vec![ShotFile { appid: "70".into(), title: "Game 70".into(), path: "/s/1.jpg".into(), creation: 5 }];
        let p = build_plan(&[game("70", true), game("20", false)], &shots, &Options::default(), "ru-RU");
        let slides = p.manifest["slides"].as_array().unwrap();
        let kinds: Vec<&str> = slides.iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["art", "shot", "art"]);
        assert_eq!(slides[0]["logo"], "logo-001.png");
        assert_eq!(slides[2]["framed"], true);
        assert_eq!(slides[2]["backdrop"], "blur-001.jpg");
        // All copies flat next to the page, without subfolders and without escaping outside.
        assert!(p.copies.iter().all(|c| !c.name.contains('/') && !c.name.starts_with('.')));
        assert_eq!(p.manifest["logos"].as_array().unwrap().len(), 2);
        assert_eq!(p.manifest["mode"], "slideshow");
        assert_eq!(p.manifest["logo"], "rexon");
    }

    #[test]
    fn plan_respects_switches_and_limits() {
        let opts = Options { game_art: false, screenshots: false, mode: Mode::Bounce, logo: LogoChoice::Games, ..Options::default() };
        let p = build_plan(&[game("70", true)], &[], &opts, "en");
        assert!(p.manifest["slides"].as_array().unwrap().is_empty());
        // Game logos are needed by the bouncing logo even without art in the slideshow.
        assert_eq!(p.manifest["logos"].as_array().unwrap().len(), 1);
        let many: Vec<GameArt> = (0..40).map(|i| game(&i.to_string(), true)).collect();
        let p = build_plan(&many, &[], &Options::default(), "en");
        assert_eq!(p.manifest["slides"].as_array().unwrap().len(), Options::default().max_games);
    }

    #[test]
    fn manifest_js_cannot_close_script() {
        let js = manifest_js(&json!({"slides": [{"title": "</script><b>"}]}));
        assert!(js.starts_with("// "));
        assert!(js.contains("window.REXON_SCREENSAVER = {"));
        assert!(!js.contains("</script>"));
    }

    #[test]
    fn locale_tag() {
        assert_eq!(page_locale("ru_RU.UTF-8"), "ru-RU");
        assert_eq!(page_locale("C"), "en");
        assert_eq!(page_locale("de"), "de");
    }

    #[test]
    fn paths_follow_steam_layout() {
        let r = Path::new("/h/.local/share/Steam");
        assert_eq!(target_dir(r), Path::new("/h/.local/share/Steam/config/uioverrides/screensavers/rexon"));
        assert_eq!(STEAM_ID, format!("uioverride-{DIR_NAME}"));
    }

    #[test]
    fn page_is_offline_and_relative() {
        for text in [PAGE_HTML, PAGE_CSS, PAGE_JS] {
            assert!(!text.contains("http://") && !text.contains("https://"), "внешний адрес в странице");
        }
        assert!(PAGE_HTML.contains("src=\"manifest.js\""));
        assert!(PAGE_HTML.contains("src=\"screensaver.js\""));
        assert!(REXON_LOGO_PNG.starts_with(b"\x89PNG"));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-ss-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Mini Steam: an account, a recent game with art, a screenshot.
    fn fake_steam(name: &str) -> PathBuf {
        let root = scratch(name);
        let user = root.join("userdata/1234");
        std::fs::create_dir_all(user.join("config")).unwrap();
        std::fs::write(
            user.join("config/localconfig.vdf"),
            "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" { \"70\" { \"LastPlayed\" \"100\" } } } } } }",
        )
        .unwrap();
        let cache = root.join("appcache/librarycache/70");
        std::fs::create_dir_all(&cache).unwrap();
        image::RgbImage::from_pixel(3000, 1000, image::Rgb([200, 20, 20])).save(cache.join("library_hero.jpg")).unwrap();
        image::RgbaImage::from_pixel(64, 32, image::Rgba([255, 255, 255, 128])).save(cache.join("logo.png")).unwrap();
        std::fs::create_dir_all(user.join("760/remote/70/screenshots")).unwrap();
        image::RgbImage::from_pixel(64, 36, image::Rgb([0, 90, 200])).save(user.join("760/remote/70/screenshots/a.jpg")).unwrap();
        std::fs::write(
            user.join("760/screenshots.vdf"),
            "\"screenshots\" { \"70\" { \"0\" { \"filename\" \"70/screenshots/a.jpg\" \"creation\" \"5\" } \"1\" { \"filename\" \"70/screenshots/missing.jpg\" \"creation\" \"6\" } } }",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps/appmanifest_70.acf"), "\"AppState\" { \"name\" \"Half-Life\" }").unwrap();
        root
    }

    #[test]
    fn install_update_remove_roundtrip() {
        let root = fake_steam("roundtrip");
        // Someone else's screensaver next to it is not touched.
        let other = screensavers_dir(&root).join("Other");
        assert!(!root.join("config").exists());
        let r = install(&root, &Options::default(), "english", "en").unwrap();
        assert_eq!(r.slides, 2);
        let dir = target_dir(&root);
        for f in ["index.html", "screensaver.js", "screensaver.css", "manifest.js", REXON_LOGO_NAME, MARKER_FILE] {
            assert!(dir.join(f).is_file(), "{f}");
        }
        let hero = image::open(dir.join("img-001.jpg")).unwrap();
        assert!(hero.width() <= PHOTO_MAX.0 && hero.height() <= PHOTO_MAX.1);
        let js = std::fs::read_to_string(dir.join("manifest.js")).unwrap();
        assert!(js.contains("Half-Life") && !js.contains(&root.display().to_string()), "в манифесте только свои имена");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("index.html"), "x").unwrap();

        // Update: the settings change, the folder is swapped in whole.
        let opts = Options { mode: Mode::Bounce, ..Options::default() };
        install(&root, &opts, "english", "en").unwrap();
        let st = status(&root);
        assert!(st.installed && !st.foreign);
        assert_eq!(st.options.mode, Mode::Bounce);
        assert!(list_names(&screensavers_dir(&root)).iter().all(|n| !n.starts_with('.')), "без хвостов временных папок");

        assert!(remove(&root).unwrap());
        assert!(!dir.exists());
        assert!(other.join("index.html").is_file(), "чужая заставка цела");
        // screensavers is not empty (Other) — it stayed.
        assert!(screensavers_dir(&root).is_dir());
        assert!(!remove(&root).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dir_name_is_lowercase() {
        // Steam looks for the folder by the host name, and the host name is always lowercase.
        assert_eq!(DIR_NAME, DIR_NAME.to_lowercase());
        assert!(!LEGACY_DIR_NAMES.contains(&DIR_NAME));
    }

    #[test]
    fn legacy_folder_moves_on_update_and_goes_on_remove() {
        let root = fake_steam("legacy");
        // The screensaver of the previous version is in the "Rexon" folder.
        install(&root, &Options { mode: Mode::Bounce, ..Options::default() }, "english", "en").unwrap();
        let legacy = screensavers_dir(&root).join(LEGACY_DIR_NAMES[0]);
        std::fs::rename(target_dir(&root), &legacy).unwrap();
        let st = status(&root);
        assert!(st.installed && !st.foreign, "старая папка видна как установленная");
        assert_eq!(st.options.mode, Mode::Bounce);
        assert_eq!(installed_dir(&root).as_deref(), Some(legacy.as_path()));

        // «Update Screensaver» moves it under the new name.
        install(&root, &st.options, "english", "en").unwrap();
        assert!(read_marker(&target_dir(&root)).is_some());
        assert!(!legacy.exists(), "старая папка убрана");
        assert_eq!(installed_dir(&root), Some(target_dir(&root)));

        // Removal also clears the old folder if it happens to remain.
        std::fs::rename(target_dir(&root), &legacy).unwrap();
        assert!(remove(&root).unwrap());
        assert!(!legacy.exists() && !target_dir(&root).exists());
        assert!(!remove(&root).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_restores_missing_parents() {
        let root = fake_steam("parents");
        install(&root, &Options::default(), "english", "en").unwrap();
        assert!(remove(&root).unwrap());
        assert!(!root.join("config/uioverrides").exists(), "созданные Rexon папки убраны");
        assert!(root.join("config").is_dir() || !root.join("config").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn foreign_folder_with_our_name_is_left_alone() {
        let root = fake_steam("foreign");
        let dir = target_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "theirs").unwrap();
        assert!(install(&root, &Options::default(), "english", "en").is_err());
        assert!(remove(&root).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("index.html")).unwrap(), "theirs");
        assert!(status(&root).foreign);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_image_drops_only_its_slide() {
        let root = fake_steam("broken");
        std::fs::write(root.join("appcache/librarycache/70/library_hero.jpg"), b"not an image").unwrap();
        let r = install(&root, &Options::default(), "english", "en").unwrap();
        assert_eq!((r.slides, r.skipped), (1, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn options_from_ui_roundtrip_and_clamp() {
        let base = Options::default();
        let o = Options {
            mode: Mode::Bounce,
            logo: LogoChoice::Games,
            game_art: false,
            screenshots: true,
            clock: false,
            interval_s: 20,
            ..base.clone()
        };
        assert_eq!(Options::from_ui(&o.to_ui(), &base), o);
        // Empty and unknown — from the base.
        assert_eq!(Options::from_ui(&json!({}), &o), o);
        assert_eq!(Options::from_ui(&json!({"mode": "steam", "logo": "steam"}), &base), base);
        // Frame length — within the page limits.
        assert_eq!(Options::from_ui(&json!({"interval": 1}), &base).interval_s, INTERVAL_MIN);
        assert_eq!(Options::from_ui(&json!({"interval": 999.4}), &base).interval_s, INTERVAL_MAX);
        assert_eq!(Options::from_ui(&json!({"interval": 12.6}), &base).interval_s, 13);
        // The markup and page limits are the same.
        assert!(PAGE_JS.contains(&format!("Math.min({INTERVAL_MAX}, Math.max({INTERVAL_MIN},")));
    }

    #[test]
    fn choices_match_options_and_have_no_steam_logo() {
        let modes: Vec<&str> = mode_choices().iter().map(|c| c.key).collect();
        assert_eq!(modes, ["slideshow", "bounce"]);
        let logos: Vec<&str> = logo_choices().iter().map(|c| c.key).collect();
        assert_eq!(logos, ["rexon", "games"]);
        for key in modes.iter().chain(&logos) {
            assert!(!key.contains("steam"), "логотип Steam не предлагаем");
        }
        // Each key parses into a setting and comes back the same.
        for key in modes {
            assert_eq!(Options::from_ui(&json!({"mode": key}), &Options::default()).to_ui()["mode"], key);
        }
        for key in logos {
            assert_eq!(Options::from_ui(&json!({"logo": key}), &Options::default()).to_ui()["logo"], key);
        }
        assert_eq!(Options::default().logo, LogoChoice::Rexon, "по умолчанию — логотип Rexon");
    }

    #[test]
    fn root_label_tells_flatpak() {
        assert_eq!(root_label(Path::new("/home/u/.local/share/Steam")), "Steam");
        assert_ne!(root_label(Path::new("/home/u/.var/app/com.valvesoftware.Steam/data/Steam")), "Steam");
    }

    fn st(installed: bool, foreign: bool, slides: usize) -> Status {
        Status { root: "/s".into(), installed, foreign, slides, updated: 0, options: Options::default() }
    }

    #[test]
    fn status_text_picks_the_right_case() {
        let none = status_text(&[]);
        let fresh = status_text(&[st(false, false, 0)]);
        let foreign = status_text(&[st(false, true, 0)]);
        let empty = status_text(&[st(true, false, 0)]);
        let full = status_text(&[st(false, true, 0), st(true, false, 7)]);
        let all = [&none, &fresh, &foreign, &empty, &full];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(full.contains('7'), "{full}");
    }

    #[test]
    fn planned_counts_follow_options() {
        let root = fake_steam("counts");
        let (games, shots) = collect(&root, "english");
        assert_eq!(planned_counts(&games, &shots, &Options::default()), (1, 1));
        let no_art = Options { game_art: false, ..Options::default() };
        assert_eq!(planned_counts(&games, &shots, &no_art), (0, 1));
        let no_shots = Options { screenshots: false, ..Options::default() };
        assert_eq!(planned_counts(&games, &shots, &no_shots), (1, 0));
        assert_ne!(found_text(0, 0), found_text(1, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn messages_report_errors_first() {
        let rep = InstallReport { root: "/s".into(), dir: "/s/x".into(), slides: 1, skipped: 0 };
        let err = "boom".to_string();
        assert_eq!(installed_message(&[], std::slice::from_ref(&err)), "boom");
        assert!(installed_message(&[rep.clone()], std::slice::from_ref(&err)).ends_with("boom"));
        assert!(!installed_message(&[rep], &[]).contains("boom"));
        assert_eq!(removed_message(1, std::slice::from_ref(&err)), "boom");
        assert_ne!(removed_message(0, &[]), removed_message(1, &[]));
    }
}
