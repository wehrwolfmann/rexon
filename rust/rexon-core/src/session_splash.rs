//! Pure logic of the login splash — port of `rexon/session_splash.py`.
//!
//! The original (Qt-free, ~1600 lines) plays the chosen webm fullscreen
//! RIGHT AFTER LOGIN into the session (like `deck_startup.webm` on the Steam Deck):
//! the Steam splash scan (built-in + Points Shop inventory), the config,
//! the «KSplash hybrid» (poster theme + early systemd unit + the
//! ksplash→mpv relay), the KWin «above everything» rule, three run paths (the Wayland
//! Rust helper rexon-splash-overlay / the X11 override-redirect layer / windowed
//! mpv) and autostart.
//!
//! Here is ONLY the deterministic part: input → output, without the file
//! system, sockets and processes. Observations of the environment (presence of binaries,
//! file contents, environment variables, xrandr output, bytes from
//! the mpv-IPC socket) are passed as PARAMETERS; execution (Popen of mpv/ffmpeg/
//! systemctl/kwriteconfig6, unix sockets, flock, writing files, waits
//! with timeouts) stays in the Python shim, which runs the argv/texts/decisions
//! built here — for a sample of the approach see `system_plan.rs`.
//!
//! Fundamentally non-portable into the core (lives in Python / a separate crate):
//!   * `_X11SplashOverlay` as a whole — a live X11 client on ctypes+libX11
//!     (window/pixmap/XPutImage/event-driven raise loop, lifetime = the live
//!     connection to the X server) + Pillow scaling of the poster;
//!   * the `_hybrid_wait_and_unpause` relay itself and all poll waits
//!     (`time.monotonic` loops) — here only their timeout constants.
//!
//! Byte-for-byte parity with live Python — `tests/session_splash_parity.rs`
//! (the reference was recorded read-only: HOME/XDG → temp, subprocess/socket mocked,
//! mpv/ffmpeg/systemctl/KWin were NOT started; paths normalized to /HOME and
//! /usr/local/lib/rexon).
//!
//! ⚠️ Bugs of the original fixed during the port (rule #16 — we do not preserve them):
//!   1. `_ensure_kwin_splash_rule`: with `[General]` lacking the `rules=`/
//!      `count=` lines Python ran `re.sub` to no effect — the new rule block
//!      was appended but NOT registered in `rules=` (KWin ignores it).
//!      Here the missing keys are written into the `[General]` section.
//!   2. `_mpv_ipc_get`: a socket line with valid JSON that is NOT an object (for example
//!      `[1,2]`) made `msg.get` raise an AttributeError that was not caught
//!      (only OSError is caught) and could crash the splash process in the middle of
//!      waiting for EOF. Here such lines are skipped.
//!   3. `scan_startup_movies`: for an inventory file without «_» the id was taken from
//!      the FULL name with the extension («weird.webm» → id/name with «.webm»).
//!      Here the id is taken from the stem («weird»); for normal names
//!      `<id>_<hash>.webm` the behavior does not change.
//!   4. `_startup_movie_names`: `communityitemid: null` in JSON gave
//!      Python the string key `"None"` (str(None)); here null/absence —
//!      the record is skipped, as is an empty string.
//!   5. User config paths ignored `$XDG_CONFIG_HOME`:
//!      `~/.config` was baked into `CONFIG_PATH`, `AUTOSTART_PATH`,
//!      `SYSTEMD_UNIT_PATH`, the manual symlink `plasma-workspace.target.wants`
//!      and `kwinrulesrc`, although the neighbouring `_ksplashrc_get` honored the variable.
//!      With a non-standard `XDG_CONFIG_HOME` the unit and the `.desktop` were written
//!      where systemd/XDG do not look for them (the splash silently did not turn on), and
//!      the KWin rule missed the live `kwinrulesrc`. The root is now one for
//!      the whole project — [`crate::we_client::config_home`] (the same class
//!      of defect was already fixed there, rule #12).
//!   6. `preview_frame`: the cache file name of the showcase frame was computed by Python's
//!      `hash((abspath, mtime))`, and string hashing in CPython is salted by
//!      PYTHONHASHSEED and DIFFERS in every process — there was never a cache hit,
//!      ffmpeg ran on every opening of the showcase, and the cache directory
//!      piled up garbage. Here the fingerprint is deterministic (FNV-1a).
//!   7. `_appinfo_names`: recursive tree parsing on a broken appinfo.vdf
//!      hit RecursionError, while only ValueError/
//!      IndexError/struct.error were caught — the process crashed. Here the depth is bounded.
//!
//! ⚠️ Self-healing of the ksplashrc setup (09.09.2026, proven live). The hybrid
//! hangs on ONE line of someone else's file — `Theme=` in the `[KSplash]` group
//! of the user's `ksplashrc`. We are not the only ones writing this key: it is
//! overwritten by applying a Plasma global theme (the «Global
//! Theme» KCM / `plasma-apply-lookandfeel`, and also our own
//! «Appearance» tab — `ainst_lnf_component_plan`), and sometimes it is simply lost,
//! leaving the file empty. Then KSplash silently takes the theme from the global one
//! (`LookAndFeelPackage` in kdeglobals) and at login shows for a second the
//! STOCK Plasma splash — a black background with the KDE logo — after which
//! our clip pops up over it: the seamless KSplash → mpv handover
//! dies, while the hybrid config, the unit and the theme stay intact, so not a single
//! sign of «broken» comes out. Fixing it by hand is useless —
//! the next theme application breaks it again. So a real splash
//! run (`play`, the `blocking && hybrid` branch — i.e. a launch from the
//! systemd unit at login) FIRST restores the setup and only then
//! plays: no theme directory — regenerate it, a foreign theme is set —
//! put ours back (the foreign value goes into
//! `prev_ksplash_theme`/`prev_ksplash_engine`, so that the hybrid rollback returns
//! the user their REAL theme, not ours). A self-healing failure is only
//! a line in the log: showing the clip matters more.
//!
//! ⚠️ Self-healing has a flip side, and it is closed by an EXPLICIT QUESTION, not
//! by a default: in the «Appearance» tab the user is free to choose THEIR OWN
//! KDE splash, and silently bringing our theme back would cancel their choice.
//! Settling the dispute over the slot — [`splash_slot_conflict`] below.

use crate::i18n::{n_, tr, trf};
use crate::we_client::config_home;
use serde_json::{Map, Value};

// ============================================================================
// Constants and paths (port of module constants; $HOME is a parameter)
// ============================================================================

/// ID of the hybrid's KSplash LnF theme — port of `KSPLASH_THEME_ID`.
pub const KSPLASH_THEME_ID: &str = "org.rexon.sessionsplash";
/// Name of the systemd user unit — port of `SYSTEMD_UNIT_NAME`.
pub const SYSTEMD_UNIT_NAME: &str = "rexon-session-splash.service";
/// app_id of the mpv window on Wayland (the KWin rule matches by it) — port of
/// `_KWIN_APP_ID`.
pub const KWIN_APP_ID: &str = "rexon-session-splash";
/// Name of the Rust helper of the Wayland overlay.
pub const SPLASH_OVERLAY_BINARY: &str = "rexon-splash-overlay";

/// Human names of Steam's built-in startup videos — port of
/// `_BUILTIN_NAMES` (order = traversal order of the scan). The name is a `msgid`,
/// translated during the scan (`tr`); Latin names do not go into the catalog.
pub const BUILTIN_NAMES: &[(&str, &str)] = &[
    // TRANSLATORS: name of Steam's standard built-in startup movie.
    ("deck_startup", n_("Steam Deck (Default)")),
    ("oled_startup", "Steam Deck OLED"),
    ("bigpicture_startup", "Big Picture"),
    // Clips of new Valve devices (the Steam client, `steamui/sp.js`: chosen by
    // device type — SteamOS in general, Steam Machine, Steam Frame; the Steam
    // «family» clip plays on the Deck's factory image). Steam has no labels of its own
    // for these clips — it writes the device names in Latin script.
    ("steam_os_startup", "SteamOS"),
    ("startup_machine", "Steam Machine"),
    ("steamframe_startup_450x450", "Steam Frame"),
    // TRANSLATORS: name of a built-in Steam startup movie: the one Steam plays on a freshly installed (factory) Steam Deck system image.
    ("steam_os_family_startup", n_("Steam Deck (Factory Image)")),
];

/// Binaries required for the hybrid — port of the list in `hybrid_supported`.
pub const HYBRID_REQUIRED_BINARIES: &[&str] =
    &["ffmpeg", "kwriteconfig6", "kreadconfig6", "ksplashqml", "systemctl"];

/// request_id of the `get_property` request in mpv-IPC — port of the literal 700.
pub const MPV_IPC_REQUEST_ID: i64 = 700;

// ── Wait timeouts (the loops themselves are in the executor) ───────────────

/// Waiting for mpv's IPC socket in the hybrid relay (`< TimeoutStartSec=15`).
pub const HYBRID_SOCK_WAIT_SEC: f64 = 6.0;
/// Waiting for ksplashqml to appear after our start.
pub const KSPLASH_APPEAR_WAIT_SEC: f64 = 3.0;
/// Cap on waiting for ksplashqml to die (its own safety nets are 30 s + 40 s).
pub const KSPLASH_DEATH_WAIT_SEC: f64 = 45.0;
/// Cap on the video run (mpv/helper), after which it is forcibly stopped.
pub const PLAY_TIMEOUT_SEC: f64 = 120.0;
/// Early helper diagnostics: the socket did not come up within this time → fallback.
/// A crash is caught separately and instantly, here we wait for «alive but not ready»:
/// on a cold first login (empty libmpv/EGL/glslang cache, cold GPU,
/// first shader compilation) initialization balloons, and killing
/// a live helper drops the splash into the leaky windowed fallback, where the panel
/// breaks through on top (incident 2026-07-25). The delay is not visible — all this
/// time ksplashqml holds the poster. Invariant: `<= HYBRID_SOCK_WAIT_SEC`,
/// and the worst case until READY (6 + 6 = 12 s) < `TimeoutStartSec=15`.
pub const HELPER_SOCK_WAIT_SEC: f64 = 6.0;
/// Waiting for the compositor on a cold X11 login (`wait_compositor`).
pub const COMPOSITOR_WAIT_SEC: f64 = 2.5;
/// Safety interval of the X11 layer's raise loop.
pub const RAISE_INTERVAL_SEC: f64 = 0.5;

/// `$XDG_CONFIG_HOME/rexon/session_splash.json` — port of `CONFIG_PATH`
/// (fix #5: the config root honors the variable).
pub fn config_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/rexon/session_splash.json")
}

/// `$XDG_CONFIG_HOME/autostart/rexon-session-splash.desktop` — port of
/// `AUTOSTART_PATH` (fix #5).
pub fn autostart_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/autostart/rexon-session-splash.desktop")
}

/// Directory of the hybrid's LnF theme — port of `LNF_THEME_DIR`.
pub fn lnf_theme_dir(home: &str) -> String {
    format!("{home}/.local/share/plasma/look-and-feel/{KSPLASH_THEME_ID}")
}

/// Path of the systemd user unit — port of `SYSTEMD_UNIT_PATH` (fix #5:
/// with a non-standard `$XDG_CONFIG_HOME` the unit was written where systemd
/// does not look for it, and `enable` could not find the unit).
pub fn systemd_unit_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/systemd/user/{SYSTEMD_UNIT_NAME}")
}

/// Directory of the manual enable fallback `plasma-workspace.target.wants`.
pub fn systemd_wants_dir(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/systemd/user/plasma-workspace.target.wants")
}

/// The manual enable-fallback symlink in `plasma-workspace.target.wants`.
pub fn systemd_wants_link(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/{SYSTEMD_UNIT_NAME}",
        systemd_wants_dir(xdg_config_home, home)
    )
}

/// Candidates for the Steam directory — port of the tuple in `steam_root` (the executor
/// runs them through realpath and checks for `config/`).
pub fn steam_root_candidates(home: &str) -> Vec<String> {
    vec![
        format!("{home}/.local/share/Steam"),
        format!("{home}/.steam/steam"),
        format!("{home}/.steam/root"),
    ]
}

/// Path of the user's ksplashrc — port of the computation in `_ksplashrc_get`
/// (`$XDG_CONFIG_HOME` or `~/.config`; an empty string = not set).
pub fn ksplashrc_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/ksplashrc")
}

/// Path of kwinrulesrc — port of `os.path.expanduser("~/.config/kwinrulesrc")`
/// (fix #5: the KWin rule was written past the live file with a non-standard
/// `$XDG_CONFIG_HOME`).
pub fn kwinrulesrc_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/kwinrulesrc")
}

/// The runtime directory — port of `_runtime_dir` (`$XDG_RUNTIME_DIR` or `/tmp`;
/// an empty string is falsy, as in Python).
pub fn runtime_dir(xdg_runtime_dir: Option<&str>) -> String {
    match xdg_runtime_dir {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => "/tmp".to_string(),
    }
}

/// The «one splash per session» flock file — port of `_acquire_play_lock`.
pub fn play_lock_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon-session-splash.lock")
}

/// IPC socket of mpv/the helper — port of `ipc_sock` in `play`.
pub fn ipc_sock_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon-splash-mpv.sock")
}

/// A freshly extracted poster in runtime — port of `out` in `_poster_for_video`.
pub fn runtime_poster_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon-splash-poster.png")
}

/// A ready KSplash theme poster — port of `theme_poster` in `_poster_for_video`.
pub fn theme_poster_path(home: &str) -> String {
    format!("{}/contents/splash/images/poster.png", lnf_theme_dir(home))
}

// ============================================================================
// Steam: names from localconfig.vdf + splash scan
// ============================================================================

/// Python truthiness of a JSON value (`or` chains, `if not x`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `dict.get(key, default)` — the value if the key exists (even null).
fn get_or(obj: &Value, key: &str, default: Value) -> Value {
    obj.get(key).cloned().unwrap_or(default)
}

/// Find the `"GetStartupMovies"` value in VDF text — the semantics of the regex
/// `"GetStartupMovies"\s*"(.*?)"\s*[\r\n]` (re.S, lazy): the closing quote is
/// the first one followed only by whitespace up to `\r`/`\n`.
fn find_startup_movies_raw(txt: &str) -> Option<&str> {
    const NEEDLE: &str = "\"GetStartupMovies\"";
    let bytes = txt.as_bytes();
    let mut from = 0;
    while let Some(rel) = txt[from..].find(NEEDLE) {
        let mut i = from + rel + NEEDLE.len();
        // \s* up to the opening quote
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while let Some(rel_q) = txt[j..].find('"') {
                let q = j + rel_q;
                // after the quote: \s*[\r\n]
                let mut k = q + 1;
                loop {
                    if k >= bytes.len() {
                        break; // EOF without \r\n → this quote does not fit
                    }
                    let c = bytes[k] as char;
                    if c == '\r' || c == '\n' {
                        return Some(&txt[start..q]);
                    }
                    if !c.is_whitespace() {
                        break;
                    }
                    k += 1;
                }
                j = q + 1;
            }
        }
        from += rel + NEEDLE.len();
    }
    None
}

/// communityitemid → {name, item_title, appid} from localconfig.vdf text —
/// port of parsing ONE file in `_startup_movie_names` (walking userdata and
/// merging across users is on the executor). No key/broken JSON → empty.
pub fn parse_startup_movie_names(vdf_text: &str) -> Value {
    let mut out = Map::new();
    let Some(raw) = find_startup_movies_raw(vdf_text) else {
        return Value::Object(out);
    };
    // A VDF string: inner quotes are escaped as \" → unpack into JSON
    // (sequential replaces — as in the original).
    let unescaped = raw.replace("\\\"", "\"").replace("\\\\", "\\");
    let Ok(parsed) = serde_json::from_str::<Value>(&unescaped) else {
        return Value::Object(out);
    };
    let Some(entries) = parsed.as_array() else {
        return Value::Object(out);
    };
    for e in entries {
        // Port of `str(e.get("communityitemid", ""))`; fix #4: null →
        // skip (Python produced the key "None").
        let cid = match e.get("communityitemid") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        if cid.is_empty() {
            continue;
        }
        let name_v = get_or(e, "name", Value::Null);
        let title_v = get_or(e, "item_title", Value::Null);
        let name = if truthy(&name_v) {
            name_v
        } else if truthy(&title_v) {
            title_v.clone()
        } else {
            Value::from("")
        };
        let item_title = if truthy(&title_v) { title_v } else { Value::from("") };
        let mut m = Map::new();
        m.insert("name".into(), name);
        m.insert("item_title".into(), item_title);
        m.insert("appid".into(), get_or(e, "appid", Value::Null));
        out.insert(cid, Value::Object(m));
    }
    Value::Object(out)
}

/// Port of `os.path.splitext(...)[1].lower()`: the extension with the dot; the leading
/// dot of a hidden file does not count as an extension.
fn splitext_ext_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(pos) if pos > 0 && !name[..pos].chars().all(|c| c == '.') => {
            name[pos..].to_lowercase()
        }
        _ => String::new(),
    }
}

/// Available startup videos — port of `scan_startup_movies`, observations
/// as parameters: `builtin_present` — file names in `<root>/steamui/movies`,
/// `inventory_files` — listing of `<root>/config/communityitemscache/
/// startupmovies` (unsorted, sorting here — like `sorted()`),
/// `names` — the result of [`parse_startup_movie_names`] (merged across users),
/// `custom_files` — listing of `<root>/config/uioverrides/movies` (custom clips
/// for Steam, see [`custom_movie_records`]).
/// Each element: `{id, name, path, builtin}`; custom clips also have
/// `origin`, `origin_label`, `title`, `thumb`.
pub fn scan_movies(
    root: &str,
    builtin_present: &[String],
    inventory_files: &[String],
    names: &Value,
    custom_files: &[String],
) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    // Built-in startup videos.
    let movies = format!("{root}/steamui/movies");
    for (key, nice) in BUILTIN_NAMES {
        let fname = format!("{key}.webm");
        let path = format!("{movies}/{fname}");
        if builtin_present.iter().any(|f| f == &fname) && !seen.contains(&path) {
            seen.push(path.clone());
            let mut m = Map::new();
            m.insert("id".into(), Value::from(*key));
            m.insert("name".into(), Value::from(tr(nice)));
            m.insert("path".into(), Value::from(path));
            m.insert("builtin".into(), Value::from(true));
            result.push(Value::Object(m));
        }
    }

    // Inventory (Points Shop) — with human names from the cache.
    let sm_dir = format!("{root}/config/communityitemscache/startupmovies");
    let mut sorted_files: Vec<&String> = inventory_files.iter().collect();
    sorted_files.sort();
    for fname in sorted_files {
        let ext = splitext_ext_lower(fname);
        if ext != ".webm" && ext != ".mp4" {
            continue;
        }
        let path = format!("{sm_dir}/{fname}");
        if seen.contains(&path) {
            continue;
        }
        seen.push(path.clone());
        // Fix #3: id from the STEM (without the extension) — in Python
        // `fn.split("_", 1)[0]` of a name without «_» captured «.webm».
        let stem = &fname[..fname.len() - ext.len()];
        let cid = stem.split('_').next().unwrap_or(stem).to_string();
        let nice = names
            .get(&cid)
            .map(|meta| get_or(meta, "name", Value::from("")))
            .filter(truthy)
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| {
                // TRANSLATORS: startup movie from the Steam inventory whose name is unknown; %1 is its item id.
                trf("Inventory · %1", &[&cid])
            });
        let mut m = Map::new();
        m.insert("id".into(), Value::from(cid));
        m.insert("name".into(), Value::from(nice));
        m.insert("path".into(), Value::from(path));
        m.insert("builtin".into(), Value::from(false));
        result.push(Value::Object(m));
    }

    // Custom clips — the third source, as in Steam itself.
    for rec in custom_movie_records(root, custom_files) {
        let path = rec["path"].as_str().unwrap_or_default().to_string();
        if !seen.contains(&path) {
            seen.push(path);
            result.push(rec);
        }
    }
    result
}

/// Steam custom clips: `<root>/config/uioverrides/movies/*.webm`.
///
/// That is how the client itself sees them (`steamui.so`: mask `*.webm`, address
/// `/uioverrides/movies/%s`; `sp.js` `GetLocalStartupMovies`, in the
/// «Customization → Startup Movie» list the name = the file name). The mask is as in Steam,
/// case-sensitive; hidden files (partially downloaded copies) are skipped.
/// Steam makes the `.jpg` thumbnail itself (`GenerateLocalStartupMoviesThumbnails`,
/// the string `%s.jpg`); the binary does not show whether it appends `.jpg` to the stem or to the
/// whole name, so both are accepted (`<stem>.jpg` first). None — `thumb` is
/// empty, and then ffmpeg makes the showcase frame, as for the others.
///
/// `id` is the full path (as for a manually chosen file, [`describe_custom`]):
/// the name may coincide with the built-in `deck_startup`.
pub fn custom_movie_records(root: &str, files: &[String]) -> Vec<Value> {
    let dir = steam_custom_movies_dir(root);
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    let mut out = Vec::new();
    for fname in sorted {
        if fname.starts_with('.') || fname.contains('/') {
            continue;
        }
        let Some(stem) = fname.strip_suffix(".webm") else {
            continue;
        };
        if stem.is_empty() {
            continue;
        }
        let path = format!("{dir}/{fname}");
        let thumb = [format!("{stem}.jpg"), format!("{fname}.jpg")]
            .into_iter()
            .find(|j| files.iter().any(|f| f == j))
            .map(|j| format!("{dir}/{j}"))
            .unwrap_or_default();
        let mut m = Map::new();
        m.insert("id".into(), Value::from(path.clone()));
        m.insert("name".into(), Value::from(fname.as_str()));
        m.insert("path".into(), Value::from(path));
        m.insert("builtin".into(), Value::from(false));
        m.insert("origin".into(), Value::from(ORIGIN_CUSTOM));
        m.insert("origin_label".into(), Value::from(custom_movie_label()));
        m.insert("title".into(), Value::from(stem));
        m.insert("thumb".into(), Value::from(thumb));
        out.push(Value::Object(m));
    }
    out
}

/// Label of a custom clip from the Steam folder. Deliberately differs from «Own file»
/// ([`origin_label`] for [`ORIGIN_CUSTOM`]): a file somewhere on disk
/// is seen only by Rexon, while a clip from `uioverrides/movies` is seen by Steam itself too.
pub fn custom_movie_label() -> String {
    // TRANSLATORS: where a startup movie comes from: the user's own movie in Steam's folder for custom startup movies (Steam shows it too).
    tr("Your own movie")
}

// ============================================================================
// Splash showcase: Steam item metadata, poster, favorites, rotation
// ============================================================================

/// A clip built into the Steam client.
pub const ORIGIN_BUILTIN: &str = "builtin";
/// A clip bought in the Points Shop (inventory).
pub const ORIGIN_SHOP: &str = "shop";
/// A file chosen by the user by hand.
pub const ORIGIN_CUSTOM: &str = "custom";

/// A human label of the origin — without jargon (rule #13).
/// An unknown value gives an empty string: an empty label is not drawn on the card,
/// and «origin=xyz» cannot be explained to the user.
pub fn origin_label(origin: &str) -> String {
    match origin {
        // TRANSLATORS: where a startup movie comes from: built into the Steam client.
        ORIGIN_BUILTIN => tr("Built into Steam"),
        ORIGIN_SHOP => "Points Shop".to_string(),
        // TRANSLATORS: where a startup movie comes from: a video file the user picked.
        ORIGIN_CUSTOM => tr("Your own file"),
        _ => String::new(),
    }
}

/// communityitemid → the FULL item record from the GetStartupMovies cache.
///
/// The difference from [`parse_startup_movie_names`]: it returns the whole record (cover,
/// item_class, localized title) — the showcase needs all fields, and the scan format
/// must not change. Walking `userdata/<id>/` and merging are on the executor.
///
/// Records with an empty `communityitemid`, `null` or the string «none» (in any
/// case) are skipped: they have no key by which the showcase would find
/// the metadata.
pub fn parse_startup_movie_items(vdf_text: &str) -> Value {
    let mut out = Map::new();
    let Some(raw) = find_startup_movies_raw(vdf_text) else {
        return Value::Object(out);
    };
    let unescaped = raw.replace("\\\"", "\"").replace("\\\\", "\\");
    let Ok(parsed) = serde_json::from_str::<Value>(&unescaped) else {
        return Value::Object(out);
    };
    let Some(entries) = parsed.as_array() else {
        return Value::Object(out);
    };
    for e in entries {
        if !e.is_object() {
            continue;
        }
        let cid = match e.get("communityitemid") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        if cid.is_empty() || cid.eq_ignore_ascii_case("none") {
            continue;
        }
        out.insert(cid, e.clone());
    }
    Value::Object(out)
}

// ── appcache/appinfo.vdf: appid → game title ───────────────────────────

/// The appinfo.vdf version with a string table (keys are indexes into it).
const APPINFO_MAGIC_STRING_TABLE: u32 = 0x0756_4429;
/// Versions where keys lie as strings right in the tree.
const APPINFO_MAGIC_INLINE: [u32; 2] = [0x0756_4427, 0x0756_4428];
/// The service header of an application record before the key-value tree.
const APPINFO_APP_HEADER: usize = 60;
/// Tree nesting limit. The real depth of appinfo.vdf is a few levels;
/// ⚠️ fix: in Python recursion over a broken file hit
/// RecursionError, which was NOT caught (only ValueError/
/// IndexError/struct.error were) and crashed the process — here parsing simply
/// stops.
const APPINFO_MAX_DEPTH: u32 = 32;

fn le_u32(data: &[u8], pos: usize) -> Option<u32> {
    data.get(pos..pos + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn le_i64(data: &[u8], pos: usize) -> Option<i64> {
    data.get(pos..pos + 8).map(|s| {
        i64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])
    })
}

/// A string up to the zero byte starting at `pos` → (string, position past the zero).
/// No zero until the end — None (in Python this is a ValueError from `bytes.index`).
fn cstr_at(data: &[u8], pos: usize) -> Option<(String, usize)> {
    let rest = data.get(pos..)?;
    let end = rest.iter().position(|b| *b == 0)?;
    Some((
        String::from_utf8_lossy(&rest[..end]).into_owned(),
        pos + end + 1,
    ))
}

/// String table of version 0x07564429: a count + consecutive C strings.
fn appinfo_string_table(data: &[u8]) -> Option<Vec<String>> {
    let tbl = le_i64(data, 8)?;
    let mut pos = usize::try_from(tbl).ok()?;
    let count = le_u32(data, pos)?;
    pos += 4;
    let mut out = Vec::with_capacity(count.min(1 << 20) as usize);
    for _ in 0..count {
        let (s, next) = cstr_at(data, pos)?;
        out.push(s);
        pos = next;
    }
    Some(out)
}

/// Field key: an index into the string table or a C string in place.
fn appinfo_key(
    blob: &[u8],
    pos: usize,
    strings: Option<&[String]>,
) -> Option<(String, usize)> {
    match strings {
        Some(tbl) => {
            let idx = le_u32(blob, pos)? as usize;
            // An index beyond the table — an empty key (as in Python).
            Some((tbl.get(idx).cloned().unwrap_or_default(), pos + 4))
        }
        None => cstr_at(blob, pos),
    }
}

/// The application name from the key-value tree: the first `name` field no
/// deeper than the second level (`common/name`). It also moves the cursor — the tree
/// has to be walked in full to know where the section ends.
fn appinfo_find_name(
    blob: &[u8],
    mut pos: usize,
    depth: u32,
    strings: Option<&[String]>,
) -> Option<(String, usize)> {
    let mut found = String::new();
    if depth > APPINFO_MAX_DEPTH {
        return Some((found, blob.len()));
    }
    while pos < blob.len() {
        let kind = blob[pos];
        pos += 1;
        if kind == 0x08 {
            // End of section.
            return Some((found, pos));
        }
        let (key, next) = appinfo_key(blob, pos, strings)?;
        pos = next;
        match kind {
            0x00 => {
                let (sub, next) = appinfo_find_name(blob, pos, depth + 1, strings)?;
                pos = next;
                if !sub.is_empty() && found.is_empty() {
                    found = sub;
                }
            }
            0x01 => {
                let (val, next) = cstr_at(blob, pos)?;
                pos = next;
                if key == "name" && depth <= 2 && found.is_empty() {
                    found = val;
                }
            }
            0x02 | 0x03 => pos += 4,
            0x07 => pos += 8,
            // An unknown type — we cannot go further, the offsets would drift.
            _ => return Some((found, blob.len())),
        }
    }
    Some((found, pos))
}

/// appid → game title from the bytes of `appcache/appinfo.vdf`.
///
/// The Steam client's binary VDF: a header (magic + universe), for version
/// 0x07564429 also the string table offset, then consecutive records
/// «appid, size, 60 bytes of service header, tree». We parse ONLY
/// the tree of the requested appids — there is no reason to parse an 8 MB file in full.
///
/// Needed for the «Video splash from <game>» label WITHOUT a network or the Steam Web API:
/// the user may be offline. An unknown version/truncated record →
/// whatever was parsed so far (in Python — the same best effort).
pub fn parse_appinfo_names(data: &[u8], appids: &[i64]) -> Vec<(i64, String)> {
    let mut want: Vec<i64> = appids.iter().copied().filter(|a| *a != 0).collect();
    want.sort_unstable();
    want.dedup();
    if want.is_empty() || data.len() < 16 {
        return Vec::new();
    }
    let Some(magic) = le_u32(data, 0) else {
        return Vec::new();
    };
    let mut strings: Option<Vec<String>> = None;
    let mut off = 8usize;
    if magic == APPINFO_MAGIC_STRING_TABLE {
        match appinfo_string_table(data) {
            Some(t) => strings = Some(t),
            // The table was not parsed — keys cannot be read without it.
            None => return Vec::new(),
        }
        off = 16;
    } else if !APPINFO_MAGIC_INLINE.contains(&magic) {
        return Vec::new();
    }

    let mut out: Vec<(i64, String)> = Vec::new();
    while off + 8 <= data.len() && !want.is_empty() {
        let (Some(appid), Some(size)) = (le_u32(data, off), le_u32(data, off + 4))
        else {
            break;
        };
        if appid == 0 {
            break;
        }
        let body = off + 8;
        let appid = i64::from(appid);
        if let Ok(idx) = want.binary_search(&appid) {
            // Slicing as in Python: the bounds are clipped to the buffer length,
            // «end before start» gives an empty slice, not a panic.
            let end = body.saturating_add(size as usize).min(data.len());
            let start = body.saturating_add(APPINFO_APP_HEADER).min(end);
            let blob = &data[start..end];
            let name = appinfo_find_name(blob, 0, 0, strings.as_deref())
                .map(|(n, _)| n)
                .unwrap_or_default();
            if !name.is_empty() {
                out.push((appid, name));
            }
            want.remove(idx);
        }
        off = body.saturating_add(size as usize);
    }
    out
}

// ── Enriching scan records with item metadata ───────────────────────────

/// `int(x)` for a JSON value where Python accepts both a number and a string.
fn as_int(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or_else(|| {
            n.as_f64().map(|f| f.trunc() as i64).unwrap_or(0)
        }),
        Some(Value::String(s)) => s.trim().parse::<i64>().unwrap_or(0),
        Some(Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// The first «truthy» string value among the listed keys.
fn first_truthy_str(meta: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(v) = meta.get(*k) {
            if truthy(v) {
                return match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
            }
        }
    }
    String::new()
}

/// Supplement the scan records with Steam item metadata (all local).
///
/// The single enrichment point for BOTH scan routes (bridge and Python) —
/// otherwise they would drift apart. Adds: title (the large label), game
/// (the source game), appid, origin/origin_label, image (the item cover on
/// the community CDN), item_class. Missing data stays as empty
/// strings — the card simply does not draw it (rule #13).
///
/// `items` — the result of [`parse_startup_movie_items`], `names` — the result of
/// [`parse_appinfo_names`].
pub fn enrich_movies(
    movies: &[Value],
    items: &Value,
    names: &[(i64, String)],
) -> Vec<Value> {
    let mut out = Vec::with_capacity(movies.len());
    for m in movies {
        let mut rec = m.as_object().cloned().unwrap_or_default();
        let builtin = rec.get("builtin").map(truthy).unwrap_or(false);
        // A custom clip comes from the scan already with its origin and label.
        let preset = rec.get("origin").and_then(Value::as_str).unwrap_or_default().to_string();
        if preset.is_empty() {
            let origin = if builtin { ORIGIN_BUILTIN } else { ORIGIN_SHOP };
            rec.insert("origin".into(), Value::from(origin));
            rec.insert("origin_label".into(), Value::from(origin_label(origin)));
        } else if !rec.get("origin_label").is_some_and(truthy) {
            rec.insert("origin_label".into(), Value::from(origin_label(&preset)));
        }

        // The metadata key is the scan record's id: for the inventory it is the
        // communityitemid, built-in clips have no such item.
        let id_key = match rec.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(v) if truthy(v) => v.to_string(),
            _ => String::new(),
        };
        let empty = Value::Object(Map::new());
        let meta = items
            .get(&id_key)
            .filter(|v| v.is_object())
            .unwrap_or(&empty);

        let appid = as_int(meta.get("appid"));
        rec.insert("appid".into(), Value::from(appid));
        let game = if appid != 0 {
            names
                .iter()
                .find(|(a, _)| *a == appid)
                .map(|(_, n)| n.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        rec.insert("game".into(), Value::from(game));

        // The localized item_title is better than the English name: the user's
        // interface is Russian (rule #13).
        let mut title = first_truthy_str(meta, &["item_title", "name"]);
        if title.is_empty() {
            title = first_truthy_str(&Value::Object(rec.clone()), &["title"]);
        }
        if title.is_empty() {
            title = match rec.get("name") {
                Some(Value::String(s)) => s.clone(),
                Some(v) if truthy(v) => v.to_string(),
                _ => String::new(),
            };
        }
        rec.insert("title".into(), Value::from(title));

        let image = first_truthy_str(meta, &["image_large", "image_small"]);
        rec.insert(
            "image".into(),
            Value::from(image.trim_start_matches('/').to_string()),
        );
        let item_class = meta.get("item_class").filter(|v| truthy(v)).cloned();
        rec.insert("item_class".into(), item_class.unwrap_or(Value::from(0)));
        out.push(Value::Object(rec));
    }
    out
}

/// A showcase record for a file chosen by the user by hand. `basename` and
/// `stem` (the name without the extension) are computed by the executor — it owns the path.
pub fn describe_custom(path: &str, basename: &str, stem: &str) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::from(path));
    m.insert("name".into(), Value::from(basename));
    m.insert("path".into(), Value::from(path));
    m.insert("builtin".into(), Value::from(false));
    m.insert("origin".into(), Value::from(ORIGIN_CUSTOM));
    m.insert("origin_label".into(), Value::from(origin_label(ORIGIN_CUSTOM)));
    m.insert("title".into(), Value::from(stem));
    m.insert("game".into(), Value::from(""));
    m.insert("appid".into(), Value::from(0));
    m.insert("image".into(), Value::from(""));
    m.insert("item_class".into(), Value::from(0));
    Value::Object(m)
}

// ── Showcase frame (ffprobe/ffmpeg) ─────────────────────────────────────

/// The share of the duration at which we take a «representative» frame: by this
/// moment the splash has drawn its logo but has not started fading yet.
pub const PREVIEW_AT: f64 = 0.75;

/// Cache directory of showcase frames. ⚠️ Do NOT confuse with the KSplash hybrid poster
/// ([`poster_ffmpeg_argv`]) — that one needs strictly the FIRST frame, otherwise
/// the ksplash→mpv relay stops being seamless; for the showcase the first frame
/// is useless, it is black in almost all clips.
pub fn preview_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    let base = match xdg_cache_home {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => format!("{home}/.cache"),
    };
    format!("{base}/rexon/steam_splash_preview")
}

/// ffprobe argv for the clip duration.
pub fn ffprobe_duration_argv(exe: &str, path: &str) -> Vec<String> {
    [
        exe,
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "csv=p=0",
        path,
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Duration from ffprobe output: negative and non-numeric → 0.0.
pub fn parse_duration(stdout: &str) -> f64 {
    stdout.trim().parse::<f64>().map(|v| v.max(0.0)).unwrap_or(0.0)
}

// ── Accepting a custom clip: checking the file with ffprobe ─────────────

/// Shorter — the clip flashes by unnoticed.
pub const MOVIE_MIN_SEC: f64 = 1.0;
/// Longer — this is no longer a splash; refusal.
pub const MOVIE_MAX_SEC: f64 = 60.0;
/// Longer — accept, but warn: it will be waited for at every login.
pub const MOVIE_WARN_SEC: f64 = 30.0;
/// The smallest picture: long side / short side (a vertical
/// 360×640 phone clip is fine too).
pub const MOVIE_MIN_LONG_SIDE: i64 = 640;
/// See [`MOVIE_MIN_LONG_SIDE`].
pub const MOVIE_MIN_SHORT_SIDE: i64 = 360;

/// Video codecs Steam (its built-in Chromium) plays from webm.
const WEBM_VIDEO_CODECS: &[&str] = &["vp8", "vp9", "av1"];
/// webm audio codecs.
const WEBM_AUDIO_CODECS: &[&str] = &["opus", "vorbis"];

/// ffprobe argv for checking a custom clip: all streams and the container as JSON.
/// The same call as [`ffprobe_duration_argv`], only with full parsing.
pub fn ffprobe_streams_argv(exe: &str, path: &str) -> Vec<String> {
    [exe, "-v", "error", "-show_streams", "-show_format", "-of", "json", path]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// What to do with the chosen file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieVerdict {
    /// Already a webm Steam understands — copy as is.
    AsIs,
    /// Fit, but Steam will not play it — transcode to webm (VP9 + Opus).
    Transcode,
    /// Not fit — show `reason`.
    Reject,
}

/// Result of the clip check (for the executor and the interface).
#[derive(Debug, Clone, PartialEq)]
pub struct MovieCheck {
    pub verdict: MovieVerdict,
    /// The refusal reason in human language; empty if not a refusal.
    pub reason: String,
    /// Warnings that do not prevent acceptance (a long clip).
    pub warnings: Vec<String>,
    /// Duration, s; 0.0 — ffprobe does not know it.
    pub duration: f64,
    pub width: i64,
    pub height: i64,
    /// Video/audio codec as ffprobe names it; empty audio — no sound.
    pub video_codec: String,
    pub audio_codec: String,
}

impl MovieCheck {
    fn reject(reason: String) -> Self {
        MovieCheck {
            verdict: MovieVerdict::Reject,
            reason,
            warnings: Vec::new(),
            duration: 0.0,
            width: 0,
            height: 0,
            video_codec: String::new(),
            audio_codec: String::new(),
        }
    }
}

/// A number from an ffprobe field: in JSON it can be both a string ("3.117000") and a number.
fn probe_num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite() && *x > 0.0)
}

fn probe_str<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Album cover art (mp3/m4a with a picture) — a «video stream», but not video.
fn probe_is_cover(s: &Value) -> bool {
    s.get("disposition")
        .and_then(|d| d.get("attached_pic"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        != 0
}

/// Decision from the output of `ffprobe -show_streams -show_format -of json`
/// ([`ffprobe_streams_argv`]): copy / transcode / refuse.
///
/// * Refuse: the output cannot be parsed; there is no video stream (album cover art —
///   `disposition.attached_pic` — does not count as video); the duration is less than
///   [`MOVIE_MIN_SEC`] or more than [`MOVIE_MAX_SEC`]; the picture is smaller than
///   640×360 (by the long and the short side).
/// * As is: a webm/Matroska container, one VP8/VP9/AV1 video, audio
///   Opus/Vorbis (one track) or no audio, nothing extra (subtitles,
///   attachments). ffprobe names Matroska and webm the same
///   (`matroska,webm`), so an mkv with the same codecs also goes as is.
/// * Otherwise — transcode.
/// * ffprobe does not know the duration (a webm recorded in a browser can lack
///   it) — transcode: ffmpeg will write it, and the executor will check the limits
///   with a repeated run on the finished file.
/// * Longer than [`MOVIE_WARN_SEC`] — a warning, not a refusal.
pub fn check_movie_probe(json_text: &str) -> MovieCheck {
    let Ok(probe) = serde_json::from_str::<Value>(json_text) else {
        return MovieCheck::reject(msg_movie_unreadable());
    };
    let streams: Vec<&Value> = probe
        .get("streams")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let format = probe.get("format").cloned().unwrap_or(Value::Null);
    if streams.is_empty() && format.is_null() {
        return MovieCheck::reject(msg_movie_unreadable());
    }

    let videos: Vec<&Value> = streams
        .iter()
        .copied()
        .filter(|s| probe_str(s, "codec_type") == "video" && !probe_is_cover(s))
        .collect();
    let audios: Vec<&Value> = streams
        .iter()
        .copied()
        .filter(|s| probe_str(s, "codec_type") == "audio")
        .collect();
    let Some(video) = videos.first() else {
        return MovieCheck::reject(msg_movie_no_picture());
    };

    let width = video.get("width").and_then(Value::as_i64).unwrap_or(0);
    let height = video.get("height").and_then(Value::as_i64).unwrap_or(0);
    let duration = probe_num(format.get("duration"))
        .or_else(|| probe_num(video.get("duration")))
        .unwrap_or(0.0);
    let mut check = MovieCheck {
        verdict: MovieVerdict::Transcode,
        reason: String::new(),
        warnings: Vec::new(),
        duration,
        width,
        height,
        video_codec: probe_str(video, "codec_name").to_string(),
        audio_codec: audios
            .first()
            .map(|a| probe_str(a, "codec_name").to_string())
            .unwrap_or_default(),
    };

    let refusal = if duration > 0.0 && duration < MOVIE_MIN_SEC {
        Some(msg_movie_too_short())
    } else if duration > MOVIE_MAX_SEC {
        Some(msg_movie_too_long())
    } else if width.max(height) < MOVIE_MIN_LONG_SIDE || width.min(height) < MOVIE_MIN_SHORT_SIDE {
        Some(msg_movie_too_small(width, height))
    } else {
        None
    };
    if let Some(reason) = refusal {
        check.verdict = MovieVerdict::Reject;
        check.reason = reason;
        return check;
    }
    if duration > MOVIE_WARN_SEC {
        check.warnings.push(msg_movie_long_warning());
    }

    let webm = probe_str(&format, "format_name")
        .split(',')
        .any(|f| f == "webm" || f == "matroska");
    let only_av = streams.iter().all(|s| {
        let kind = probe_str(s, "codec_type");
        kind == "audio" || (kind == "video" && !probe_is_cover(s))
    });
    let video_ok = WEBM_VIDEO_CODECS.contains(&check.video_codec.as_str());
    let audio_ok = audios
        .iter()
        .all(|a| WEBM_AUDIO_CODECS.contains(&probe_str(a, "codec_name")));
    if webm && only_av && videos.len() == 1 && audios.len() <= 1 && video_ok && audio_ok && duration > 0.0 {
        check.verdict = MovieVerdict::AsIs;
    }
    check
}

/// The file cannot be read.
pub fn msg_movie_unreadable() -> String {
    // TRANSLATORS: a startup movie the user picked can't be used: ffprobe couldn't read it.
    tr("Couldn't read this file — it may be damaged or not a video.")
}

/// The file has no picture (for example, it is music).
pub fn msg_movie_no_picture() -> String {
    // TRANSLATORS: a startup movie the user picked can't be used: the file has no video picture (e.g. it is music).
    tr("This file has no picture — choose a video file.")
}

/// The clip is shorter than a second.
pub fn msg_movie_too_short() -> String {
    // TRANSLATORS: a startup movie the user picked can't be used: it is shorter than one second.
    tr("The video is shorter than a second — it would flash by unnoticed. Choose a longer one.")
}

/// The clip is longer than a minute.
pub fn msg_movie_too_long() -> String {
    // TRANSLATORS: a startup movie the user picked can't be used: it is longer than one minute.
    tr("The video is longer than a minute — a startup movie should be short. Trim it or choose another one.")
}

/// The picture is smaller than 640×360.
pub fn msg_movie_too_small(width: i64, height: i64) -> String {
    // The literal is on one line with `trf(`: otherwise xgettext loses the comment.
    let (w, h) = (width, height);
    // TRANSLATORS: a startup movie the user picked can't be used: its picture is too small; %1 and %2 are its width and height in pixels.
    trf("The picture is too small (%1×%2) — it would look blurry on screen. Choose a video of at least 640×360.", &[&w, &h])
}

/// The clip is longer than 30 s — accept, but warn.
pub fn msg_movie_long_warning() -> String {
    // TRANSLATORS: warning when adding a startup movie longer than 30 seconds; it is still accepted.
    tr("The video is longer than 30 seconds — it's a startup movie, not a film: you'll wait for it at every login.")
}

/// The clip will be transcoded (a hint for the interface).
pub fn msg_movie_will_convert() -> String {
    // TRANSLATORS: shown when adding a startup movie that Steam can't play as is; the original file stays untouched.
    tr("The video will be converted to WebM so Steam can play it. The original file stays as it is.")
}

// ── Accepting a custom clip: where it goes and under what name ──────────
//
// WHY A COPY. Until 24.11.2026 «Choose File…» stored the absolute path of the
// original: the person moved or deleted the file, unplugged the flash drive —
// and the startup movie silently disappeared. Now the clip is COPIED into a
// folder we own: Steam's folder for custom startup movies
// (`config/uioverrides/movies`, Steam shows the clip in «Customization →
// Startup Movie» too), and without Steam — Rexon's own folder
// ([`rexon_movies_dir`]). The original is never touched.

/// Rexon's own folder of startup movies, relative to `$XDG_DATA_HOME`.
pub const REXON_MOVIES_SUBDIR: &str = "rexon/startup-movies";

/// Rexon's own folder of startup movies: where a clip goes when there is no Steam,
/// and where an old selection that Steam cannot play is moved.
pub fn rexon_movies_dir(xdg_data_home: Option<&str>, home: &str) -> String {
    let base = match xdg_data_home {
        Some(x) if !x.is_empty() => x.trim_end_matches('/').to_string(),
        _ => format!("{home}/.local/share"),
    };
    format!("{base}/{REXON_MOVIES_SUBDIR}")
}

/// Extensions of the clips kept in [`rexon_movies_dir`] (mpv plays all of them).
pub const REXON_MOVIE_EXTS: &[&str] = &[".webm", ".mp4", ".mkv", ".mov"];

/// Records of the clips in Rexon's own folder — the same shape as
/// [`custom_movie_records`]; the label is «Your own file»: Steam does not see
/// them.
pub fn rexon_movie_records(dir: &str, files: &[String]) -> Vec<Value> {
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    let mut out = Vec::new();
    for fname in sorted {
        if fname.starts_with('.') || fname.contains('/') {
            continue;
        }
        let ext = splitext_ext_lower(fname);
        if !REXON_MOVIE_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let stem = &fname[..fname.len() - ext.len()];
        if stem.is_empty() {
            continue;
        }
        let path = format!("{dir}/{fname}");
        let mut m = Map::new();
        m.insert("id".into(), Value::from(path.clone()));
        m.insert("name".into(), Value::from(fname.as_str()));
        m.insert("path".into(), Value::from(path));
        m.insert("builtin".into(), Value::from(false));
        m.insert("origin".into(), Value::from(ORIGIN_CUSTOM));
        m.insert("origin_label".into(), Value::from(origin_label(ORIGIN_CUSTOM)));
        m.insert("title".into(), Value::from(stem));
        m.insert("thumb".into(), Value::from(""));
        out.push(Value::Object(m));
    }
    out
}

/// Whether `path` is a clip lying DIRECTLY in one of our folders (`dirs`):
/// only such clips may be deleted. Built-in and Points Shop clips live
/// elsewhere and never pass; neither do `..`, nested paths and hidden files
/// (a partially written copy).
pub fn is_own_movie_path(path: &str, dirs: &[String]) -> bool {
    let Some((dir, name)) = path.rsplit_once('/') else {
        return false;
    };
    if name.is_empty() || name.starts_with('.') || path.split('/').any(|p| p == "..") {
        return false;
    }
    dirs.iter()
        .any(|d| !d.is_empty() && d.trim_end_matches('/') == dir)
}

/// A safe Latin file stem for a copied clip.
///
/// Steam shows a custom clip under its file name and builds the address
/// `/uioverrides/movies/<name>` for its built-in browser. How it copes with
/// Cyrillic and spaces in that address was not checked, so the name is made
/// Latin: Cyrillic is transliterated (the shared rule of imported themes),
/// everything else becomes `-`. A name with no letters left (CJK, emoji) gets
/// a stable `movie-<hash>`; an empty one — `startup-movie`.
pub fn safe_movie_stem(original_stem: &str) -> String {
    let lowered = original_stem.to_lowercase();
    let has_letters = lowered.chars().any(|c| {
        c.is_ascii_alphanumeric() || ('а'..='я').contains(&c) || "ёґєіїў".contains(c) // i18n-exempt: Cyrillic letters the file name transliteration accepts
    });
    if !has_letters {
        if original_stem.trim().is_empty() {
            return "startup-movie".to_string();
        }
        let digest = fnv1a64(original_stem.trim().as_bytes());
        return format!("movie-{:08x}", digest as u32);
    }
    let slug = crate::importers::slug60(original_stem);
    if slug.is_empty() {
        "startup-movie".to_string()
    } else {
        slug
    }
}

/// A file name in the target folder that does not overwrite anything: `stem.ext`,
/// and if taken — `stem-2.ext`, `stem-3.ext`… (`existing` — the folder listing;
/// compared case-insensitively, like the person reads names).
pub fn unique_movie_file_name(stem: &str, ext: &str, existing: &[String]) -> String {
    let taken = |n: &str| existing.iter().any(|e| e.eq_ignore_ascii_case(n));
    let first = format!("{stem}{ext}");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|i| format!("{stem}-{i}{ext}"))
        .find(|n| !taken(n))
        .unwrap_or(first)
}

/// `(file name, name without the extension)` of a path.
pub fn split_name(path: &str) -> (String, String) {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let stem = match name.rfind('.') {
        Some(pos) if pos > 0 && !name[..pos].chars().all(|c| c == '.') => name[..pos].to_string(),
        _ => name.clone(),
    };
    (name, stem)
}

/// Name of the temporary file the copy is written into: hidden, so neither
/// Steam (mask `*.webm`) nor our scan pick up a half-written clip.
pub fn movie_part_name(final_name: &str) -> String {
    format!(".{final_name}.part")
}

/// ffmpeg argv for converting into webm Steam plays: VP9 (constant quality,
/// row multithreading) + Opus. Only the first video and audio track; no
/// subtitles or attachments. `-progress pipe:1` gives machine-readable
/// progress on stdout ([`parse_ffmpeg_progress`]); `-f webm` because the
/// temporary name has no `.webm` extension.
pub fn transcode_argv(exe: &str, src: &str, out: &str) -> Vec<String> {
    [
        exe, "-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", src,
        "-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn",
        "-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-row-mt", "1",
        "-deadline", "good", "-cpu-used", "4",
        "-c:a", "libopus", "-b:a", "128k",
        "-progress", "pipe:1", "-nostats", "-f", "webm", out,
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Seconds already converted from one `-progress` line (`out_time_us=` /
/// `out_time_ms=` — both are in MICROseconds in ffmpeg, a known quirk;
/// `out_time=HH:MM:SS.micro`). Other lines and `N/A` — `None`.
pub fn parse_ffmpeg_progress(line: &str) -> Option<f64> {
    let (key, value) = line.trim().split_once('=')?;
    let value = value.trim();
    match key {
        "out_time_us" | "out_time_ms" => value
            .parse::<i64>()
            .ok()
            .filter(|v| *v >= 0)
            .map(|v| v as f64 / 1_000_000.0),
        "out_time" => {
            let mut parts = value.split(':');
            let h = parts.next()?.parse::<f64>().ok()?;
            let m = parts.next()?.parse::<f64>().ok()?;
            let s = parts.next()?.parse::<f64>().ok()?;
            let t = h * 3600.0 + m * 60.0 + s;
            (t.is_finite() && t >= 0.0).then_some(t)
        }
        _ => None,
    }
}

/// Converted share 0..=1; unknown duration — `None` (the interface then shows
/// the stage without a percentage).
pub fn progress_fraction(done_sec: f64, duration: f64) -> Option<f64> {
    (duration > 0.0 && done_sec.is_finite()).then(|| (done_sec / duration).clamp(0.0, 1.0))
}

/// What the import does with a checked file.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportAction {
    /// Copy byte for byte.
    Copy,
    /// Convert into webm VP9 + Opus.
    Transcode,
    /// Refuse with this reason.
    Refuse(String),
}

/// The decision for one file: with Steam a clip must be a webm Steam plays
/// (otherwise — conversion); without Steam only our splash plays it, and mpv
/// plays any accepted video — a plain copy.
///
/// `src_is_webm` — the file is named `.webm`. ffprobe names Matroska and webm
/// the same, but an `.mkv` carries the «matroska» document type inside, and
/// Steam's built-in browser may refuse it even with VP9 inside — so for Steam
/// such a file is converted, not renamed.
pub fn import_action(check: &MovieCheck, steam_present: bool, src_is_webm: bool) -> ImportAction {
    match check.verdict {
        MovieVerdict::Reject => ImportAction::Refuse(check.reason.clone()),
        MovieVerdict::AsIs if steam_present && !src_is_webm => ImportAction::Transcode,
        MovieVerdict::AsIs => ImportAction::Copy,
        MovieVerdict::Transcode if steam_present => ImportAction::Transcode,
        MovieVerdict::Transcode => ImportAction::Copy,
    }
}

/// The import stage — for the progress line in the interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImportStage {
    /// ffmpeg is being installed (the password window is up).
    Installing,
    /// ffprobe reads the file.
    Checking,
    /// The file is being copied.
    Copying,
    /// Conversion; the share done if the duration is known.
    Converting(Option<f64>),
}

/// The progress line for a stage — in plain words (rule #13).
pub fn import_stage_text(stage: ImportStage) -> String {
    match stage {
        // TRANSLATORS: progress while adding a startup movie: the video tools (ffmpeg) are being installed.
        ImportStage::Installing => tr("⏳ Installing the video tools (ffmpeg)…"),
        // TRANSLATORS: progress while adding a startup movie: the chosen file is being checked.
        ImportStage::Checking => tr("⏳ Checking the video…"),
        // TRANSLATORS: progress while adding a startup movie: the file is being copied.
        ImportStage::Copying => tr("⏳ Copying the video…"),
        ImportStage::Converting(None) => tr("⏳ Converting the video for Steam…"),
        ImportStage::Converting(Some(f)) => {
            let pct = (f * 100.0).floor() as i64;
            // TRANSLATORS: progress while converting a startup movie; %1 is the percentage done.
            trf("⏳ Converting the video for Steam… %1%", &[&pct])
        }
    }
}

/// How the import ended.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOutcome {
    pub ok: bool,
    /// Where the copy lies (empty on failure).
    pub path: String,
    /// The line for the person: success with warnings, or the reason.
    pub message: String,
}

impl ImportOutcome {
    pub fn failed(message: String) -> Self {
        ImportOutcome { ok: false, path: String::new(), message }
    }
}

/// The clip was added and chosen.
pub fn msg_import_done(title: &str, in_steam: bool) -> String {
    if in_steam {
        // TRANSLATORS: a startup movie was added; %1 is its name. Steam lists it too.
        trf("✅ Added “%1” — it's chosen for login and also appears in Steam (the Customization section).", &[&title])
    } else {
        // TRANSLATORS: a startup movie was added (Steam isn't installed); %1 is its name.
        trf("✅ Added “%1” — it's chosen for login.", &[&title])
    }
}

/// The person cancelled the conversion.
pub fn msg_import_cancelled() -> String {
    // TRANSLATORS: adding a startup movie was cancelled by the user.
    tr("Adding the video was cancelled.")
}

/// Copying or converting failed; %1 — the system's reason.
pub fn msg_import_failed(why: &str) -> String {
    // TRANSLATORS: a startup movie couldn't be added; %1 is the system's reason.
    trf("Couldn't add the video: %1", &[&why])
}

/// ffmpeg could not be installed — nothing is done (all or nothing).
pub fn msg_tools_missing(why: &str) -> String {
    // TRANSLATORS: adding a startup movie needs ffmpeg, and installing it failed; %1 is the reason.
    trf("Adding a video needs the video tools (ffmpeg), and they couldn't be installed: %1", &[&why])
}

/// An old selection was copied into our folder.
pub fn msg_migrated(title: &str) -> String {
    // TRANSLATORS: the startup movie chosen earlier was copied into Rexon's folder so it keeps working if the original is moved; %1 is its name.
    trf("The startup movie “%1” is now kept as a copy — it keeps working even if the original file is moved or deleted.", &[&title])
}

/// The deletion question.
pub fn msg_delete_question(title: &str, chosen_in_steam: bool) -> String {
    // TRANSLATORS: confirmation before deleting the user's own startup movie; %1 is its name.
    let base = trf("Delete the startup movie “%1”? The copy is removed from disk; the original video you added it from is not touched.", &[&title]);
    if chosen_in_steam {
        // TRANSLATORS: extra warning in the delete confirmation: Steam currently uses this movie.
        format!("{base}\n\n{}", tr("⚠️ Steam currently plays this movie at startup — after deleting, Steam goes back to its standard one."))
    } else {
        base
    }
}

/// Deleted.
pub fn msg_deleted(title: &str) -> String {
    // TRANSLATORS: the user's own startup movie was deleted; %1 is its name.
    trf("🗑 Deleted “%1”.", &[&title])
}

/// Refusal: the clip is not ours (built-in or bought).
pub fn msg_delete_not_own() -> String {
    // TRANSLATORS: refusal to delete a startup movie that is built into Steam or bought in the Points Shop.
    tr("Only your own movies can be deleted — the ones built into Steam and bought in the Points Shop stay.")
}

/// Whether the current selection must be copied into our folder: it is set,
/// the file exists, and it lies neither in our folders nor inside Steam (a
/// built-in or bought clip is Steam's business).
pub fn needs_migration(current: &str, exists: bool, own_dirs: &[String], steam_root: Option<&str>) -> bool {
    if current.is_empty() || !exists || is_own_movie_path(current, own_dirs) {
        return false;
    }
    match steam_root {
        Some(root) if !root.is_empty() => !current.starts_with(&format!("{}/", root.trim_end_matches('/'))),
        _ => true,
    }
}

/// The config after a clip moved: `old` → `new` in the selection and the stars.
pub fn config_with_replaced_movie(cfg: &Value, old: &str, new: &str) -> Value {
    let mut m = cfg.as_object().cloned().unwrap_or_default();
    if m.get("path").and_then(Value::as_str) == Some(old) {
        m.insert("path".into(), Value::from(new));
    }
    let mut fav = favorites_from_config(cfg);
    for f in fav.iter_mut() {
        if f == old {
            *f = new.to_string();
        }
    }
    fav.dedup();
    m.insert("favorites".into(), Value::Array(fav.into_iter().map(Value::from).collect()));
    Value::Object(m)
}

/// The config after deleting a clip: its star goes away; if it was the
/// selection, `fallback` takes its place (another found clip — the splash
/// must not be left enabled with nothing to play; rule #13), or nothing.
pub fn config_without_movie(cfg: &Value, path: &str, fallback: Option<&str>) -> Value {
    let (without_star, _) = config_with_favorite(cfg, path, false);
    let mut m = without_star.as_object().cloned().unwrap_or_default();
    if m.get("path").and_then(Value::as_str) == Some(path) {
        m.insert("path".into(), Value::from(fallback.unwrap_or("")));
    }
    Value::Object(m)
}

/// `config/config.vdf` — where Steam keeps its own startup movie choice.
pub fn steam_config_vdf_path(root: &str) -> String {
    format!("{root}/config/config.vdf")
}

/// Steam's own choice (`Customization` → `StartupMovie` → `LocalPath`, a path
/// relative to `config/`). `None` — nothing chosen, or «Shuffle» is on
/// (then there is no single chosen clip). The block is looked for at any
/// depth: the nesting differs between client versions.
pub fn parse_steam_startup_movie(vdf_text: &str) -> Option<String> {
    fn find<'a>(node: &'a crate::steam_screensaver::Kv) -> Option<&'a crate::steam_screensaver::Kv> {
        if let Some(sm) = node.path(&["Customization", "StartupMovie"]) {
            return Some(sm);
        }
        node.entries().iter().find_map(|(_, child)| find(child))
    }
    let root = crate::steam_screensaver::parse_kv(vdf_text);
    let sm = find(&root)?;
    let shuffle = sm.str_at("Shuffle").map(str::trim).unwrap_or("");
    if shuffle == "1" || shuffle.eq_ignore_ascii_case("true") {
        return None;
    }
    let local = sm.str_at("LocalPath")?.trim();
    (!local.is_empty()).then(|| local.to_string())
}

/// The absolute path of Steam's choice.
pub fn steam_choice_path(root: &str, local_path: &str) -> String {
    let root = root.trim_end_matches('/');
    if local_path.starts_with('/') {
        format!("{root}/config{local_path}")
    } else {
        format!("{root}/config/{local_path}")
    }
}

/// Showcase marks: `steam_selected` — this clip is Steam's own choice
/// (Steam writes the extension in any case, `.webM`, so case is ignored);
/// `deletable` — the clip lies in one of our folders ([`is_own_movie_path`]).
pub fn mark_movies(movies: &mut [Value], steam_choice: Option<&str>, own_dirs: &[String]) {
    for m in movies.iter_mut() {
        let path = m.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
        let chosen = steam_choice.is_some_and(|c| !c.is_empty() && c.eq_ignore_ascii_case(&path));
        let own = is_own_movie_path(&path, own_dirs);
        if let Some(obj) = m.as_object_mut() {
            obj.insert("steam_selected".into(), Value::from(chosen));
            obj.insert("deletable".into(), Value::from(own));
        }
    }
}

/// FNV-1a 64 — a deterministic fingerprint for the cache file name.
///
/// ⚠️ Fix of a bug of the original: the cache key was computed by Python's
/// `hash((abspath, mtime))`, and string hashing in CPython is salted by PYTHONHASHSEED and
/// DIFFERS in every process. So the cache file name did not match the
/// previous run: there was NEVER a cache hit, ffmpeg was started on
/// every opening of the showcase, and the directory piled up garbage. Here the fingerprint is
/// stable, and the cache starts working.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// A file-system name sanitizer: everything except letters, digits, `_`, `.`
/// and `-` is replaced with `_`; the last 64 characters are taken. The semantics of
/// `re.sub(r"[^\w.-]", "_", name)[-64:]` (Python's `\w` is Unicode-aware, here
/// it is `char::is_alphanumeric` + `_`).
fn preview_key(basename: &str) -> String {
    let cleaned: Vec<char> = basename
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let start = cleaned.len().saturating_sub(64);
    cleaned[start..].iter().collect()
}

/// Path of the frame cache file. The key is path + mtime, so a replaced clip
/// is regenerated by itself.
pub fn preview_cache_path(cache_dir: &str, video_abs: &str, mtime: i64) -> String {
    let basename = video_abs.rsplit('/').next().unwrap_or(video_abs);
    let key = preview_key(basename);
    let digest = fnv1a64(format!("{video_abs}\0{mtime}").as_bytes());
    format!("{cache_dir}/{key}.{digest:x}.png")
}

/// ffmpeg argv for the showcase frame. `seek` ≤ 0.05 s — without `-ss` (the clip is
/// shorter than expected): the «first frame» retry behaves the same way.
pub fn preview_ffmpeg_argv(exe: &str, video: &str, out_tmp: &str, seek: f64) -> Vec<String> {
    let mut cmd: Vec<String> = vec![exe.into(), "-y".into(), "-v".into(), "error".into()];
    if seek > 0.05 {
        cmd.push("-ss".into());
        cmd.push(format!("{seek:.3}"));
    }
    cmd.extend([
        "-i".to_string(),
        video.to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        "-update".to_string(),
        "1".to_string(),
        out_tmp.to_string(),
    ]);
    cmd
}

// ── Favorites and clip rotation ─────────────────────────────────────────

/// Paths of starred clips from the config (non-strings are dropped).
pub fn favorites_from_config(cfg: &Value) -> Vec<String> {
    cfg.get("favorites")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The config with an updated star → (new config, the current list).
pub fn config_with_favorite(cfg: &Value, path: &str, on: bool) -> (Value, Vec<String>) {
    let mut fav = favorites_from_config(cfg);
    if on {
        if !fav.iter().any(|p| p == path) {
            fav.push(path.to_string());
        }
    } else {
        fav.retain(|p| p != path);
    }
    let mut obj = cfg.as_object().cloned().unwrap_or_default();
    obj.insert(
        "favorites".into(),
        Value::Array(fav.iter().map(|p| Value::from(p.clone())).collect()),
    );
    (Value::Object(obj), fav)
}

/// Whether «a random splash at every login» is enabled.
pub fn rotation_enabled(cfg: &Value) -> bool {
    cfg.get("rotate").map(truthy).unwrap_or(false)
}

/// The config with rotation toggled.
pub fn config_with_rotation(cfg: &Value, enabled: bool) -> Value {
    let mut obj = cfg.as_object().cloned().unwrap_or_default();
    obj.insert("rotate".into(), Value::from(enabled));
    Value::Object(obj)
}

/// How many clips are needed for «a new clip at every login» to change
/// anything at all.
///
/// ⭐ One truth for everyone. The number «two» was a literal in five places: the pool
/// choice, the draw, the scan short circuit, the state chip of the «Steam» tab and
/// the refusal of its switch (in Python also `len(self._sv_items) < 2`).
/// If they diverged, the chip would promise a rotation that does not happen at login.
/// Ask through the functions below rather than comparing with a number in place.
pub const ROTATION_MIN_POOL: usize = 2;

/// Whether the favorites rotate (rather than all found clips).
pub fn rotation_uses_favorites(favorites: usize) -> bool {
    favorites >= ROTATION_MIN_POOL
}

/// Rotation pool size by the number of favorites and of all found clips — the same
/// decision as [`rotation_pool`], only on numbers (the showcase does not need the files).
pub fn rotation_pool_size(favorites: usize, all: usize) -> usize {
    if rotation_uses_favorites(favorites) {
        favorites
    } else {
        all
    }
}

/// Whether rotation will really change the clip at login.
pub fn rotation_possible(favorites: usize, all: usize) -> bool {
    rotation_pool_size(favorites, all) >= ROTATION_MIN_POOL
}

/// What we rotate from: the favorites, and if there are fewer than two — everything found.
/// The executor checks that the files exist (the I/O boundary).
///
/// Foolproofing (rule #13): enabled rotation with ONE star must not
/// silently turn into «the same clip at every login».
pub fn rotation_pool(existing_favorites: &[String], existing_all: &[String]) -> Vec<String> {
    if rotation_uses_favorites(existing_favorites.len()) {
        return existing_favorites.to_vec();
    }
    existing_all.to_vec()
}

/// What to choose the clip for the NEXT login from: the pool without the current one, and if
/// nothing is left after exclusion — the whole pool. An empty list = nothing to
/// rotate (the pool has fewer than two elements); the draw itself is made by the executor —
/// the random number generator is an I/O boundary.
pub fn rotation_choices(pool: &[String], current: Option<&str>) -> Vec<String> {
    if pool.len() < ROTATION_MIN_POOL {
        return Vec::new();
    }
    let filtered: Vec<String> = pool
        .iter()
        .filter(|p| Some(p.as_str()) != current)
        .cloned()
        .collect();
    if filtered.is_empty() {
        pool.to_vec()
    } else {
        filtered
    }
}

// ============================================================================
// Config (which webm + sound + the hybrid's service keys)
// ============================================================================

/// Port of `load_config`: file text → dict; no file/broken JSON/not a dict → {}.
pub fn parse_config(text: Option<&str>) -> Value {
    if let Some(t) = text {
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if v.is_object() {
                return v;
            }
        }
    }
    Value::Object(Map::new())
}

/// Port of `save_config`: update `path`/`sound` WITHOUT overwriting the service
/// keys (prev_ksplash_theme etc.). Returns the new config.
pub fn config_with_selection(cfg: &Value, path: &str, sound: bool) -> Value {
    let mut m = cfg.as_object().cloned().unwrap_or_default();
    m.insert("path".into(), Value::from(path));
    m.insert("sound".into(), Value::from(sound));
    Value::Object(m)
}

/// Port of `save_hybrid_pref`: remember the «hybrid on/off» checkbox.
pub fn config_with_hybrid_pref(cfg: &Value, enabled: bool) -> Value {
    let mut m = cfg.as_object().cloned().unwrap_or_default();
    m.insert("hybrid".into(), Value::from(enabled));
    Value::Object(m)
}

/// Config serialization — `json.dump(cfg, ensure_ascii=False, indent=2)`
/// (port of `_write_config`; the atomic .tmp+rename write is on the executor).
pub fn config_json(cfg: &Value) -> String {
    serde_json::to_string_pretty(cfg).expect("Value сериализуется")
}

/// Port of `hybrid_installed`: the flag in the config AND the unit file in place.
pub fn hybrid_installed(cfg: &Value, unit_file_exists: bool) -> bool {
    truthy(&get_or(cfg, "hybrid_installed", Value::Null)) && unit_file_exists
}

/// Whether to update the theme poster when saving the choice — port of the condition in
/// `save_config` (the hybrid is installed and the theme directory exists).
pub fn should_refresh_hybrid_poster(cfg: &Value, theme_dir_exists: bool) -> bool {
    truthy(&get_or(cfg, "hybrid_installed", Value::Null)) && theme_dir_exists
}

// ============================================================================
// KSplash hybrid: poster theme, ksplashrc, systemd unit
// ============================================================================

/// Splash.qml of the poster theme — port of `_SPLASH_QML` (without Kirigami, instant
/// display of the clip's first frame with PreserveAspectFit on black).
pub const SPLASH_QML: &str = "\
import QtQuick

Rectangle {
    id: root
    color: \"black\"

    property int stage

    Image {
        anchors.fill: parent
        fillMode: Image.PreserveAspectFit
        asynchronous: false
        cache: false
        source: \"images/poster.png\"
    }
}
";

// TRANSLATORS: Description of Rexon's splash theme in KDE System Settings (Splash Screen). KSplash and mpv are program names.
const LNF_DESCRIPTION: &str = n_("Rexon startup movie poster (Steam style) — seamless handover from the KDE splash screen to the movie");
// TRANSLATORS: Description of the startup movie autostart entry (shown in the desktop's autostart settings).
const AUTOSTART_COMMENT: &str = n_("Startup movie at login (Steam style)");

/// metadata.json of the LnF package — port of `meta` in `_write_lnf_theme`
/// (`json.dump(..., ensure_ascii=False, indent=4)`).
pub fn lnf_metadata_json() -> String {
    // The description in all catalog languages (`Description[ru]`): KDE's «System
    // Settings» shows it and picks the language itself by the session locale.
    let mut plugin = serde_json::Map::new();
    plugin.insert("Id".into(), KSPLASH_THEME_ID.into());
    plugin.insert("Name".into(), "Rexon Session Splash".into());
    plugin.insert("Description".into(), LNF_DESCRIPTION.into());
    for (code, text) in crate::system_l10n::translations(LNF_DESCRIPTION) {
        plugin.insert(format!("Description[{code}]"), text.into());
    }
    plugin.insert("Authors".into(), serde_json::json!([{"Name": "Wehrwolfmann"}]));
    plugin.insert("License".into(), "LGPL-2.1+".into());
    let meta = serde_json::json!({
        "KPackageStructure": "Plasma/LookAndFeel",
        "KPlugin": plugin,
        "X-Plasma-APIVersion": "2",
    });
    // indent=4 — as in the original (json.dump there uses 4 spaces, not 2).
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(&meta, &mut ser).expect("Value сериализуется");
    String::from_utf8(buf).expect("JSON — валидный UTF-8")
}

/// ffmpeg argv «first frame → PNG» — port of cmd in `_extract_poster`.
/// `ffmpeg` — the path from `shutil.which` (the executor's observation).
pub fn poster_ffmpeg_argv(ffmpeg: &str, video: &str, out_png: &str) -> Vec<String> {
    vec![
        ffmpeg.into(),
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-i".into(),
        video.into(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        out_png.into(),
    ]
}

/// Port of `_ksplashrc_get`: the key value from the `[KSplash]` group of
/// the USER's ksplashrc (the file text as a parameter). Deliberately not
/// the kconfig cascade: for backup/rollback the fact «there was no key» ('') matters.
pub fn ksplashrc_get(text: &str, key: &str) -> String {
    let mut in_group = false;
    for ln in text.lines() {
        let s = ln.trim();
        if s.starts_with('[') {
            in_group = s == "[KSplash]";
        } else if in_group {
            if let Some((k, v)) = s.split_once('=') {
                if k.trim() == key {
                    return v.trim().to_string();
                }
            }
        }
    }
    String::new()
}

/// kwriteconfig6 argv — port of `_ksplashrc_set` (an empty value = delete the key).
/// `exe` — the path from `shutil.which("kwriteconfig6")`.
pub fn kwriteconfig_argv(exe: &str, key: &str, value: &str) -> Vec<String> {
    let mut cmd: Vec<String> = vec![
        exe.into(),
        "--file".into(),
        "ksplashrc".into(),
        "--group".into(),
        "KSplash".into(),
        "--key".into(),
        key.into(),
    ];
    if value.is_empty() {
        cmd.push("--delete".into());
    } else {
        cmd.push(value.into());
    }
    cmd
}

/// argv of `systemctl --user …` — port of `_systemctl_user`.
pub fn systemctl_user_argv(exe: &str, args: &[&str]) -> Vec<String> {
    let mut cmd: Vec<String> = vec![exe.into(), "--user".into()];
    cmd.extend(args.iter().map(|a| (*a).to_string()));
    cmd
}

/// Text of the systemd user unit — port of `_systemd_unit_content`.
///
/// After=kwin (both branches, a nonexistent one is ignored), Type=notify +
/// Before=plasma-plasmashell.service — plasmashell does not start until mpv has
/// brought up its IPC socket (READY=1), so ksplash does not go away before the first
/// frame. ⚠️ Do NOT add After=plasma-ksplash.service — a mutual
/// deadlock (the oneshot job finishes on ksplashqml's EXIT; incident
/// 2026-07-16, the Wayland login hung on a black splash).
///
/// `Description` — in English and not from the catalog: systemd has no
/// localized keys (it does not know `Description[ru]`), and the line is visible
/// only in `systemctl`/the journal. Labels for people are in the autostart entry.
pub fn systemd_unit_content(launcher_exec: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Rexon Session Splash (startup movie at login, Steam style)\n\
         PartOf=graphical-session.target\n\
         After=plasma-kwin_wayland.service plasma-kwin_x11.service\n\
         Before=plasma-plasmashell.service\n\
         \n\
         [Service]\n\
         Type=notify\n\
         ExecStart={launcher_exec}\n\
         Slice=session.slice\n\
         # safety nets: a broken mpv delays login by at most ~15 s\n\
         # (overlay threshold 6 s + fallback mpv 6 s = 12 s until READY, + slack).\n\
         TimeoutStartSec=15\n\
         TimeoutStopSec=5\n\
         \n\
         [Install]\n\
         WantedBy=plasma-workspace.target\n"
    )
}

/// The splash launch command for the Python entry — port of `_launcher_exec`.
/// `rexon_path` — the result of `shutil.which("rexon")`; empty — nothing
/// to start with, and then the line is EMPTY: it must not be written.
///
/// ⛔`python_executable` no longer takes part in any answer. Previously with an
/// empty `rexon_path` this produced `python3 -m rexon.app
/// --session-splash` — a line that the `.desktop` autostart and the
/// systemd unit run without a working directory, and the `rexon` package is not in
/// the system interpreter's `sys.path`: the login splash silently did not
/// show. The choice moved entirely into [`crate::app::launch_program`].
/// ⚠️The parameter is kept for the signature's sake — the `rexon-py` bridge holds it
/// (`ss_launcher_exec`); it will go away together with the bridge.
pub fn launcher_exec(rexon_path: Option<&str>, _python_executable: &str) -> String {
    crate::app::launch_command(
        &crate::installer::LaunchEntry::Python,
        rexon_path,
        &[SESSION_SPLASH_FLAG],
    )
    .unwrap_or_default()
}

/// The login splash route flag (parsed by `app::route`).
pub const SESSION_SPLASH_FLAG: &str = "--session-splash";

/// The splash launch command for the autostart `Exec=` (Desktop Entry format).
///
/// ⚠️ The entry is chosen by THE SAME function as for the shortcut and the tray
/// ([`crate::installer::launch_entry_live`]) — it arrives here ready.
/// The Rust entry is the window binary with [`SESSION_SPLASH_FLAG`]: the route is handled by
/// the core without Qt (`cli::run_headless`), no interpreter is needed at login.
/// The Python entry (fallback) is the `rexon` launcher by full path.
///
/// An empty line — nothing to start with ([`crate::app::launch_program`]); it must not
/// be written into `Exec=`, the caller must refuse out loud.
/// ⚠️`python_executable` is not used — see [`launcher_exec`].
pub fn desktop_launcher_exec(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    _python_executable: &str,
) -> String {
    crate::app::launch_command(entry, rexon_path, &[SESSION_SPLASH_FLAG]).unwrap_or_default()
}

/// The same for the hybrid systemd unit's `ExecStart=`.
///
/// Separate from [`desktop_launcher_exec`] because systemd's quoting rules
/// differ: in a Desktop Entry `\`, `$`, `` ` `` and `"` inside quotes
/// double the backslash once more, while systemd would read the extra backslash literally.
pub fn unit_launcher_exec(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    _python_executable: &str,
) -> String {
    crate::app::unit_launch_command(entry, rexon_path, &[SESSION_SPLASH_FLAG]).unwrap_or_default()
}

/// An `ExecStart=` command-line argument per `systemd.service(5)`:
/// `%` is a specifier (`%%`), `$` is variable substitution (`$$`); a word with
/// a space, a quote, `\` or `;` is put in double quotes, inside
/// which `"` and `\` are escaped with a backslash.
pub fn systemd_exec_arg(arg: &str) -> String {
    let arg = arg.replace('%', "%%").replace('$', "$$");
    if !arg.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | ';')) {
        return arg;
    }
    let mut q = String::with_capacity(arg.len() + 2);
    q.push('"');
    for c in arg.chars() {
        if matches!(c, '"' | '\\') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q
}

/// «Is the desktop KDE» — port of `"KDE" in XDG_CURRENT_DESKTOP.upper()`.
pub fn is_kde_desktop(xdg_current_desktop: Option<&str>) -> bool {
    xdg_current_desktop
        .unwrap_or("")
        .to_uppercase()
        .contains("KDE")
}

/// The `hybrid_supported` decision: KDE + all binaries + an active
/// plasma-workspace.target (the presence of binaries and is-active are observations).
pub fn hybrid_supported_decision(
    xdg_current_desktop: Option<&str>,
    all_binaries_present: bool,
    plasma_target_active: bool,
) -> bool {
    is_kde_desktop(xdg_current_desktop) && all_binaries_present && plasma_target_active
}

/// Whether to back up the current ksplashrc theme before installing the hybrid — port of
/// the condition in `_install_hybrid` (re-enabling does not overwrite the backup).
pub fn should_backup_prev_theme(cur_theme: &str) -> bool {
    cur_theme != KSPLASH_THEME_ID
}

/// Whether to roll back ksplashrc when removing the hybrid — port of the condition in
/// `_uninstall_hybrid` (a foreign theme chosen later is left alone).
pub fn should_rollback_ksplashrc(cur_theme: &str) -> bool {
    cur_theme == KSPLASH_THEME_ID
}

/// Whether to repair the ksplashrc setup before a real splash display: the hybrid
/// counts as installed, while `[KSplash] Theme` holds a theme that is NOT ours (including
/// empty — the key was lost, and KSplash will take the theme from the global one).
///
/// The reason for self-healing is in the module header: any application of a Plasma
/// global theme overwrites the key, and the login silently loses its seamlessness.
pub fn should_heal_ksplashrc(cfg: &Value, cur_theme: &str) -> bool {
    truthy(&get_or(cfg, "hybrid_installed", Value::Null)) && cur_theme != KSPLASH_THEME_ID
}

// ============================================================================
// The dispute over the KSplash slot: the login video splash versus the KDE splash
// ============================================================================
//
// ⚠️ There is ONE slot at login. The self-healing above brings our theme back into
// `ksplashrc` at every login — and exactly because of that it must know about
// the «Appearance» tab: there the user may choose THEIR OWN KDE splash
// (`ainst::lnf_component_plan`, the `splash` kind) or a whole look
// (`global`, applied by `plasma-apply-lookandfeel` — the same key). If we
// asked nothing, the person would choose a splash, and at the next login
// the product would SILENTLY bring its own back: the user's deliberate choice is cancelled without
// a single word, and their own choice looks to blame.
//
// So here there is no «smart» rule but an honest question with two outcomes:
// the slot stays with the clip or goes to the chosen splash, and then the hybrid
// is removed for real (`uninstall_hybrid`), so that there is nothing to bring
// back. The decision and all texts are here; the interface only shows the question
// and executes the answer.

/// Whether applying this look kind takes the KSplash splash slot.
///
/// `splash` writes `ksplashrc[KSplash]Theme` directly, `global` — the same key
/// by the hands of `plasma-apply-lookandfeel`. Other kinds (cursors, icons, GTK,
/// wallpapers, lock screen) do not touch the slot, and there is nothing to ask about them.
pub fn apply_kind_takes_ksplash_slot(apply_kind: &str) -> bool {
    matches!(apply_kind, "splash" | "global")
}

/// The question to the user and both of its outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashSlotConflict {
    /// Title of the question window.
    pub title: &'static str,
    /// The explanation and the question itself. «Yes» — give the slot to the chosen splash,
    /// «No» — keep Rexon's video splash.
    pub question: String,
    /// What to tell the user when they kept the video splash.
    pub kept: String,
    /// Whether the REST can be applied while keeping the video splash.
    ///
    /// For a whole look the splash is just one of the parts, and there is no reason to refuse
    /// the whole theme because of it: the theme is applied, and we take the splash slot
    /// back right away (`heal_hybrid`). For the `splash` kind there is nothing to give
    /// back — the splash is the whole point of the action there, so «No»
    /// means «do not apply».
    pub keep_applies_rest: bool,
}

/// Title of the question about the slot.
///
/// `&'static str`, not `String`: the field [`SplashSlotConflict::title`] is declared
/// that way, and the window hosts take it as is. The translation for each language of the
/// process is leaked once (there are only a few languages, the leak is bounded).
pub fn slot_question_title() -> &'static str {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    // TRANSLATORS: dialog title: the KDE splash screen and the Rexon startup movie compete for one slot.
    let text = tr("One Splash Screen at a Time");
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(s) = map.get(&text) {
        return s;
    }
    let leaked: &'static str = Box::leak(text.clone().into_boxed_str());
    map.insert(text, leaked);
    leaked
}

/// Whether applying a look conflicts with the login video splash.
///
/// `None` — nothing to dispute: the look kind does not touch the slot, the hybrid is not
/// installed, or our own poster theme is being applied.
pub fn splash_slot_conflict(
    apply_kind: &str,
    theme_name: &str,
    hybrid_installed: bool,
) -> Option<SplashSlotConflict> {
    if !hybrid_installed || !apply_kind_takes_ksplash_slot(apply_kind) {
        return None;
    }
    // Our own poster theme: it already sits in the slot, there is nobody to take the place from.
    if theme_name == KSPLASH_THEME_ID {
        return None;
    }
    if apply_kind == "global" {
        return Some(SplashSlotConflict {
            title: slot_question_title(),
            question: trf(
                "The “%1” global theme comes with its own KDE splash screen — the \
                 screen shown while you log in. There is only one slot for it, and \
                 right now it is taken by the Rexon startup movie: the video that \
                 plays right after you log in. The two can't work together.\n\n\
                 “Yes” — apply the whole global theme, splash screen included. The \
                 Rexon startup movie will be turned off and the video will no \
                 longer play at login.\n\n\
                 “No” — apply the global theme but leave the splash screen alone: \
                 the video stays.",
                &[&theme_name],
            ),
            kept: msg_slot_kept_video(),
            keep_applies_rest: true,
        });
    }
    // The «None» tile — turn the KDE splash off entirely. The clip relies on
    // it too (the poster theme), so we must ask, but in other words: the person
    // has never seen the name «None» anywhere.
    if theme_name == crate::appearance_installer::SPLASH_NONE {
        return Some(SplashSlotConflict {
            title: slot_question_title(),
            question: tr(
                "Right now the Rexon startup movie plays when you log in — the video \
                 that plays right after you log in. It works through the KDE splash \
                 screen slot, so turning the splash screen off turns the movie off too.\n\n\
                 “Yes” — turn off the splash screen and the Rexon startup movie.\n\n\
                 “No” — keep the Rexon startup movie.",
            ),
            kept: tr("The splash screen wasn't turned off — the Rexon startup movie stays at login."),
            keep_applies_rest: false,
        });
    }
    Some(SplashSlotConflict {
        title: slot_question_title(),
        question: trf(
            "The KDE splash screen and the Rexon startup movie share one slot: \
             only one of them is shown when you log in.\n\n\
             Right now the slot is taken by the Rexon startup movie — the video \
             that plays right after you log in.\n\n\
             “Yes” — apply the “%1” splash screen. The Rexon startup movie will \
             be turned off and the video will no longer play at login.\n\n\
             “No” — keep the Rexon startup movie and don't apply the “%1” \
             splash screen.",
            &[&theme_name],
        ),
        kept: msg_slot_kept_instead_of(theme_name),
        keep_applies_rest: false,
    })
}

/// The user kept the video splash, and the rest of the look was applied.
pub fn msg_slot_kept_video() -> String {
    tr("The splash screen was left alone — the Rexon startup movie still plays at login.")
}

/// The user kept the video splash, the chosen splash was NOT applied.
pub fn msg_slot_kept_instead_of(theme_name: &str) -> String {
    trf(
        "The “%1” splash screen wasn't applied — the Rexon startup movie stays at login.",
        &[&theme_name],
    )
}

/// The user gave the slot to the chosen splash — the hybrid was removed.
pub fn msg_slot_released() -> String {
    tr("The Rexon startup movie is off — the chosen splash screen now shows at login.")
}

/// Could not give up the slot: the hybrid was not removed.
///
/// Applying the chosen splash in this case is NOT allowed: the hybrid counts as
/// installed, and at the next login self-healing would bring our theme back —
/// the user would see that the choice «was not saved», for no reason at all.
pub fn msg_slot_release_failed(detail: &str) -> String {
    trf(
        "Couldn't turn off the Rexon startup movie: %1. The chosen splash screen \
         wasn't applied — otherwise Rexon would bring its own back at the next login.",
        &[&detail],
    )
}

/// The user's answer to the slot dispute, turned into an apply plan.
///
/// One for all hosts of the «Appearance» tab (the Rust window and the Python host
/// of the live product): the «Yes/No → what to do» branching lives here, and
/// the hosts only show the question and execute the plan (rule #12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplashSlotPlan {
    /// Apply. `keep_video` — take the splash slot back right after
    /// applying (the clip path), `note` — what to append to the result.
    Apply {
        keep_video: Option<String>,
        note: String,
    },
    /// Do not apply — report with this text.
    Stop(String),
}

impl SplashSlotPlan {
    /// No dispute — a normal apply without notes.
    pub fn plain() -> Self {
        SplashSlotPlan::Apply {
            keep_video: None,
            note: String::new(),
        }
    }
}

/// The apply result with a note about the splash slot.
///
/// The note is appended only to a SUCCESS: an installer refusal speaks for
/// itself, and «the clip stayed in place» next to it would read as success.
pub fn msg_with_slot_note(msg: &str, note: &str, ok: bool) -> String {
    let mut out = msg.to_string();
    if ok && !note.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(note);
    }
    out
}

// ============================================================================
// The KWin «above everything» rule (kwinrulesrc)
// ============================================================================

/// The rule name — the person sees it in KDE's «System Settings → Window
/// Management → Window Rules», so it is in the interface language.
// TRANSLATORS: name of a KDE window rule that keeps Rexon's startup movie
// above the panel; people see it in System Settings → Window Rules.
pub const KWIN_RULE_TITLE: &str = n_("Rexon: startup movie on top of everything");

/// The rule's first line as the original wrote it (`_KWIN_RULE_DESC`) — in
/// Russian for all languages. New rules are no longer written this way
/// ([`kwin_rule_desc_line`]); the constant remains to recognize already
/// installed rules, and for the bridge to the old Python.
pub const KWIN_RULE_DESC: &str = "Description=Rexon: заставка входа поверх всего\n";

/// The name from [`KWIN_RULE_DESC`] without the key and the newline.
const KWIN_RULE_TITLE_LEGACY_RU: &str = "Rexon: заставка входа поверх всего";

/// The rule's first line in the current interface language.
pub fn kwin_rule_desc_line() -> String {
    format!("Description={}\n", tr(KWIN_RULE_TITLE))
}

/// All names under which our rule may have been written: the original
/// English one, the former Russian one (before the translation catalog) and the translation into every
/// catalog language installed on the machine, plus the current one.
///
/// The rule is found primarily by `wmclass` ([`KWIN_APP_ID`]); the names are
/// a second sign (a rule without `wmclass`, edited by hand) and, most importantly,
/// the permission to rewrite the name into the current language: a name not from this
/// list is the person's own, and we do not touch it.
pub fn kwin_rule_known_titles() -> Vec<String> {
    let mut all = vec![KWIN_RULE_TITLE.to_string(), KWIN_RULE_TITLE_LEGACY_RU.to_string()];
    all.extend(crate::i18n::translations_everywhere(KWIN_RULE_TITLE));
    all.push(tr(KWIN_RULE_TITLE));
    let mut out: Vec<String> = Vec::new();
    for t in all {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}
/// Removing FSP (0=None, 2=Force) — port of `_KWIN_RULE_FSP`. The key point:
/// keepAbove does NOT raise the window above the panel (DockLayer is above AboveLayer);
/// only an ACTIVE fullscreen goes above the panel, and mpv gets activation only
/// with focus-stealing-prevention removed (incident 2026-07-17 19:57).
pub const KWIN_RULE_FSP: &str = "fsplevel=0\nfsplevelrule=2\n";

/// Body of the KWin rule (without the `[N]` header) — port of `_KWIN_RULE_BODY`.
pub fn kwin_rule_body() -> String {
    kwin_rule_body_with(&kwin_rule_desc_line())
}

/// [`kwin_rule_body`] with a given first line (`Description=…\n`).
fn kwin_rule_body_with(desc_line: &str) -> String {
    format!(
        "{desc_line}{KWIN_RULE_FSP}\
         above=true\naboverule=2\n\
         fullscreen=true\nfullscreenrule=2\n\
         noborder=true\nnoborderrule=2\n\
         skiptaskbar=true\nskiptaskbarrule=2\n\
         skippager=true\nskippagerrule=2\n\
         skipswitcher=true\nskipswitcherrule=2\n\
         wmclass={KWIN_APP_ID}\nwmclassmatch=1\n\
         types=1\n"
    )
}

/// Candidates for the live KWin reconfigure command — port of `_kwin_reconfigure`
/// (the executor takes the first existing binary; reconfigure is asynchronous —
/// after it a ~0.5 s pause before mpv starts).
pub fn kwin_reconfigure_candidates() -> Vec<Vec<String>> {
    vec![
        vec!["qdbus6".into(), "org.kde.KWin".into(), "/KWin".into(), "reconfigure".into()],
        vec!["qdbus".into(), "org.kde.KWin".into(), "/KWin".into(), "reconfigure".into()],
        vec![
            "gdbus".into(),
            "call".into(),
            "--session".into(),
            "--dest".into(),
            "org.kde.KWin".into(),
            "--object-path".into(),
            "/KWin".into(),
            "--method".into(),
            "org.kde.KWin.reconfigure".into(),
        ],
    ]
}

/// Split the kwinrulesrc text into sections — the semantics of `re.split(r"(?m)^(?=\[)")`
/// (boundaries BEFORE each `[Header]` at the start of a line; headers are kept).
fn split_sections(txt: &str) -> Vec<&str> {
    let mut cuts = vec![0usize];
    for (i, _) in txt.match_indices('[') {
        if i > 0 && txt.as_bytes()[i - 1] == b'\n' {
            cuts.push(i);
        }
    }
    let mut out = Vec::with_capacity(cuts.len());
    for w in cuts.windows(2) {
        out.push(&txt[w[0]..w[1]]);
    }
    out.push(&txt[*cuts.last().unwrap()..]);
    out
}

/// The `[N]` number at the start of a line — the semantics of `^\[(\d+)\]` (re.M).
fn rule_numbers(txt: &str) -> Vec<u64> {
    let mut nums = Vec::new();
    for line in txt.lines() {
        let Some(rest) = line.strip_prefix('[') else { continue };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(']') {
            if let Ok(n) = digits.parse() {
                nums.push(n);
            }
        }
    }
    nums
}

/// Byte offset of the start and end of the FIRST line with the given prefix
/// (the semantics of `re.search(..., re.M)` for `^prefix...$`).
fn find_line_with_prefix(txt: &str, prefix: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    for line in txt.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content.starts_with(prefix) {
            return Some((offset, offset + content.len()));
        }
        offset += line.len();
    }
    None
}

/// The first whole line of the form `count=<digits>` — the semantics of `^count=\d+$`.
fn find_count_line(txt: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    for line in txt.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if let Some(v) = content.strip_prefix("count=") {
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                return Some((offset, offset + content.len()));
            }
        }
        offset += line.len();
    }
    None
}

/// An idempotent KWin rule for the splash window — port of the pure part of
/// `_ensure_kwin_splash_rule` (file text → new file text).
///
/// * the rule already exists and is up to date (`fsplevel=`, the name in the current language)
///   → `None` (the file is left alone);
/// * an old rule without fsplevel → migration in place (inserting the FSP removal);
/// * the name is one of the former variants ([`kwin_rule_known_titles`]), but
///   not in the current language → it is rewritten into the current one;
/// * no rule → a new block `[max+1]` + registration in `[General]`.
///
/// Returning `Some(text)` = the executor rewrites the file and pokes
/// reconfigure of the live KWin ([`kwin_reconfigure_candidates`]).
///
/// Fix #1: if `[General]` exists but has no `rules=`/`count=`,
/// Python lost the rule's registration (re.sub did nothing) — here the missing
/// keys are written right after the `[General]` header.
///
/// Fix #2 (01.10.2026): our own rule is recognized by the WHOLE `wmclass=`
/// line (previously by substring, and a foreign `wmclass=rexon-session-splash-x`
/// would pass for ours) or by one of the known names. The migration
/// inserts the FSP removal after a `Description=` line of ANY language: the original
/// looked for its own Russian line verbatim, and with a different name the migration
/// silently did not happen — the file was rewritten on every launch for nothing.
pub fn ensure_kwin_rule_text(existing: Option<&str>) -> Option<String> {
    ensure_kwin_rule_text_with(existing, &tr(KWIN_RULE_TITLE), &kwin_rule_known_titles())
}

/// Value of the section's first `key=` line (without `\r`).
fn section_value<'a>(sec: &'a str, key: &str) -> Option<&'a str> {
    sec.lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .find_map(|l| l.strip_prefix(key))
}

/// [`ensure_kwin_rule_text`] with explicit names: `title` — the current one,
/// `known` — all names under which the rule may have been written.
pub fn ensure_kwin_rule_text_with(
    existing: Option<&str>,
    title: &str,
    known: &[String],
) -> Option<String> {
    let txt = existing.unwrap_or("");
    let desc_line = format!("Description={title}\n");
    // Does our rule already exist? Look for our block among the sections.
    let sections = split_sections(txt);
    let is_ours = |sec: &str| {
        // A rule section is any except [General]: newer KWin versions
        // name rules not by numbers but by UUID.
        !sec.starts_with("[General]")
            && (sec
                .lines()
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .any(|l| l.strip_prefix("wmclass=") == Some(KWIN_APP_ID))
                || section_value(sec, "Description=").is_some_and(|d| known.iter().any(|k| k == d)))
    };
    for (i, sec) in sections.iter().enumerate() {
        if !is_ours(sec) {
            continue;
        }
        let mut updated = sec.to_string();
        // The name goes into the current language if it is our former one, not one given
        // by the person.
        if let Some((s, e)) = find_line_with_prefix(&updated, "Description=") {
            let current = &updated[s + "Description=".len()..e];
            if current != title && known.iter().any(|k| k == current) {
                updated.replace_range(s + "Description=".len()..e, title);
            }
        }
        // Migration of an old rule: insert the FSP removal after the name (or
        // right after the header if there is no name).
        if !updated.contains("fsplevel=") {
            let at = find_line_with_prefix(&updated, "Description=")
                .or_else(|| find_line_with_prefix(&updated, "["))
                .map(|(_, e)| if updated[e..].starts_with('\n') { e + 1 } else { e })
                .unwrap_or(updated.len());
            let mut fsp = KWIN_RULE_FSP.to_string();
            if at == updated.len() && !updated.ends_with('\n') {
                fsp.insert(0, '\n');
            }
            updated.insert_str(at, &fsp);
        }
        if updated == *sec {
            return None; // up to date, do nothing
        }
        let mut out = String::with_capacity(txt.len() + KWIN_RULE_FSP.len() + title.len());
        for (j, s) in sections.iter().enumerate() {
            if j == i {
                out.push_str(&updated);
            } else {
                out.push_str(s);
            }
        }
        return Some(out);
    }

    // No rule yet — add a new block.
    let new = rule_numbers(txt).into_iter().max().unwrap_or(0) + 1;
    let mut rules: Vec<String> = match find_line_with_prefix(txt, "rules=") {
        Some((s, e)) => txt[s + "rules=".len()..e]
            .split(',')
            .filter(|r| !r.trim().is_empty())
            .map(String::from)
            .collect(),
        None => Vec::new(),
    };
    rules.push(new.to_string());
    let mut body = format!("\n[{new}]\n{}", kwin_rule_body_with(&desc_line));
    let mut txt = txt.to_string();
    if txt.contains("[General]") {
        let count_line = format!("count={}", rules.len());
        let rules_line = format!("rules={}", rules.join(","));
        let mut missing = String::new();
        match find_count_line(&txt) {
            Some((s, e)) => txt.replace_range(s..e, &count_line),
            None => missing.push_str(&format!("{count_line}\n")),
        }
        match find_line_with_prefix(&txt, "rules=") {
            Some((s, e)) => txt.replace_range(s..e, &rules_line),
            None => missing.push_str(&format!("{rules_line}\n")),
        }
        if !missing.is_empty() {
            // Fix #1: write the missing keys right after [General].
            if let Some((_, hdr_end)) = find_line_with_prefix(&txt, "[General]") {
                let insert_at = if txt[hdr_end..].starts_with('\n') {
                    hdr_end + 1
                } else {
                    hdr_end
                };
                txt.insert_str(insert_at, &missing);
            }
        }
    } else {
        body.push_str(&format!("\n[General]\ncount=1\nrules={new}\n"));
    }
    Some(txt + &body)
}

// ============================================================================
// mpv IPC / sd_notify
// ============================================================================

/// A JSON string literal in the style of `json.dumps` (ensure_ascii=True).
fn py_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if ('\u{20}'..='\u{7e}').contains(&c) => out.push(c),
            c => {
                let cp = c as u32;
                if cp <= 0xFFFF {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    let v = cp - 0x1_0000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                }
            }
        }
    }
    out.push('"');
}

/// `json.dumps(v)` with Python's default separators (`", "` / `": "`).
fn py_dumps(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => py_json_string(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                py_dumps(item, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, item)) in o.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                py_json_string(k, out);
                out.push_str(": ");
                py_dumps(item, out);
            }
            out.push('}');
        }
    }
}

/// Payload of `_mpv_ipc_send`: `json.dumps({"command": cmd}) + "\n"`.
pub fn mpv_ipc_command_payload(command: &Value) -> String {
    let mut m = Map::new();
    m.insert("command".into(), command.clone());
    let mut out = String::new();
    py_dumps(&Value::Object(m), &mut out);
    out.push('\n');
    out
}

/// Payload of the `get_property` request in `_mpv_ipc_get`
/// (`request_id` = [`MPV_IPC_REQUEST_ID`]).
pub fn mpv_ipc_get_payload(prop: &str) -> String {
    let mut m = Map::new();
    m.insert(
        "command".into(),
        Value::from(vec![Value::from("get_property"), Value::from(prop)]),
    );
    m.insert("request_id".into(), Value::from(MPV_IPC_REQUEST_ID));
    let mut out = String::new();
    py_dumps(&Value::Object(m), &mut out);
    out.push('\n');
    out
}

/// Parsing the mpv-IPC reply stream — port of the loop in `_mpv_ipc_get`.
///
/// Events pour into the socket mixed with the reply: lines are parsed in
/// order, an unfinished tail (without `\n`) is ignored; the first object with
/// our `request_id` decides: `error == "success"` → `Some(data)`, otherwise
/// `None`. Fix #2: valid JSON that is not an object is skipped (in Python
/// `msg.get` raised an AttributeError past the handler).
pub fn parse_mpv_ipc_response(received: &str, request_id: i64) -> Option<Value> {
    let mut parts: Vec<&str> = received.split('\n').collect();
    parts.pop(); // unfinished tail
    for line in parts {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !msg.is_object() {
            continue; // fix #2
        }
        if msg.get("request_id").and_then(Value::as_i64) == Some(request_id) {
            return if msg.get("error").and_then(Value::as_str) == Some("success") {
                Some(get_or(&msg, "data", Value::Null))
            } else {
                None
            };
        }
    }
    None
}

/// The `sd_notify` socket address — port of the transformation in `_sd_notify_ready`:
/// empty → None (no-op), `@…` (abstract namespace) → `\0…`.
pub fn notify_socket_addr(notify_socket: Option<&str>) -> Option<String> {
    let addr = notify_socket.unwrap_or("");
    if addr.is_empty() {
        return None;
    }
    Some(match addr.strip_prefix('@') {
        Some(rest) => format!("\0{rest}"),
        None => addr.to_string(),
    })
}

/// The READY payload for the systemd manager.
pub const SD_NOTIFY_READY: &str = "READY=1";

// ============================================================================
// X11: primary monitor geometry (the overlay itself is non-portable ctypes code)
// ============================================================================

/// `\b` before position `pos` (Python semantics: a non-word character on the left
/// or the start of the string; `connected` starts with a word character).
fn word_boundary_before(txt: &str, pos: usize) -> bool {
    match txt[..pos].chars().next_back() {
        None => true,
        Some(c) => !(c.is_alphanumeric() || c == '_'),
    }
}

/// Read `\d+` from a position; returns (number, end) or None.
fn read_digits(txt: &str, from: usize) -> Option<(i64, usize)> {
    let bytes = txt.as_bytes();
    let mut i = from;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == from {
        return None;
    }
    txt[from..i].parse().ok().map(|n| (n, i))
}

/// An expected single character at a position; returns the position after it.
fn expect_char(txt: &str, at: usize, ch: char) -> Option<usize> {
    if txt[at..].starts_with(ch) {
        Some(at + ch.len_utf8())
    } else {
        None
    }
}

/// Geometry `(\d+)x(\d+)\+(\d+)\+(\d+)` from a position.
fn read_geometry(txt: &str, from: usize) -> Option<(i64, i64, i64, i64)> {
    let (w, i) = read_digits(txt, from)?;
    let i = expect_char(txt, i, 'x')?;
    let (h, i) = read_digits(txt, i)?;
    let i = expect_char(txt, i, '+')?;
    let (x, i) = read_digits(txt, i)?;
    let i = expect_char(txt, i, '+')?;
    let (y, _) = read_digits(txt, i)?;
    Some((w, h, x, y))
}

/// The first occurrence of `\b<needle><geometry>` — the shared part of both regexes.
fn find_geometry_after(txt: &str, needle: &str) -> Option<(i64, i64, i64, i64)> {
    let mut from = 0;
    while let Some(rel) = txt[from..].find(needle) {
        let pos = from + rel;
        if word_boundary_before(txt, pos) {
            if let Some(g) = read_geometry(txt, pos + needle.len()) {
                return Some(g);
            }
        }
        from = pos + needle.len();
    }
    None
}

/// (x, y, w, h) of the primary monitor from `xrandr --query` output — port of the parsing in
/// `_x11_primary_geometry`. Without the primary flag — the first connected output
/// (covering the whole virtual root with multiple monitors is not allowed); nothing
/// found → None.
pub fn parse_xrandr_geometry(stdout: &str) -> Option<(i64, i64, i64, i64)> {
    let (w, h, x, y) = find_geometry_after(stdout, "connected primary ")
        .or_else(|| find_geometry_after(stdout, "connected "))?;
    Some((x, y, w, h))
}

/// argv of `xrandr --query` (port of the call in `_x11_primary_geometry`).
pub fn xrandr_query_argv(exe: &str) -> Vec<String> {
    vec![exe.into(), "--query".into()]
}

// ============================================================================
// The run: routes, argv of mpv and rexon-splash-overlay, messages
// ============================================================================

/// Port of `_is_x11_session`: DISPLAY is set and WAYLAND_DISPLAY is not (on Plasma
/// Wayland user units have BOTH — the Wayland path stays there).
pub fn is_x11_session(display: Option<&str>, wayland_display: Option<&str>) -> bool {
    display.map(|d| !d.is_empty()).unwrap_or(false)
        && !wayland_display.map(|w| !w.is_empty()).unwrap_or(false)
}

/// The effective hybrid mode — port of `hybrid=None` in `play` (auto: the hybrid is
/// installed AND it is a real run; a test from the UI passes Some(false)).
pub fn effective_hybrid(param: Option<bool>, hybrid_installed: bool, blocking: bool) -> bool {
    param.unwrap_or(hybrid_installed && blocking)
}

/// Whether to try the Rust helper rexon-splash-overlay — port of the condition in `play`
/// (only a real run on Wayland; a test from the UI goes through windowed mpv).
pub fn wants_wayland_helper(
    blocking: bool,
    display: Option<&str>,
    wayland_display: Option<&str>,
) -> bool {
    blocking
        && !is_x11_session(display, wayland_display)
        && wayland_display.map(|w| !w.is_empty()).unwrap_or(false)
}

/// Candidates for the rexon-splash-overlay binary — port of `_find_splash_overlay_binary`
/// (the workspace target rust/target and the crate target, release then debug;
/// the PATH fallback is the executor's observation). `repo_root` — realpath of the root.
pub fn splash_overlay_candidates(repo_root: &str) -> Vec<String> {
    vec![
        format!("{repo_root}/rust/target/release/{SPLASH_OVERLAY_BINARY}"),
        format!("{repo_root}/rust/target/debug/{SPLASH_OVERLAY_BINARY}"),
        format!("{repo_root}/rust/{SPLASH_OVERLAY_BINARY}/target/release/{SPLASH_OVERLAY_BINARY}"),
        format!("{repo_root}/rust/{SPLASH_OVERLAY_BINARY}/target/debug/{SPLASH_OVERLAY_BINARY}"),
    ]
}

/// argv of rexon-splash-overlay — port of cmd in `_play_wayland_overlay`
/// (the helper's protocol = mpv's protocol: --ipc/--paused/unpause/quit).
pub fn splash_overlay_argv(
    helper: &str,
    video: &str,
    ipc_sock: &str,
    sound: bool,
    paused: bool,
) -> Vec<String> {
    let mut cmd: Vec<String> = vec![
        helper.into(),
        "--video".into(),
        video.into(),
        "--ipc".into(),
        ipc_sock.into(),
    ];
    if !sound {
        cmd.push("--mute".into());
    }
    if paused {
        cmd.push("--paused".into());
    }
    cmd
}

/// Where mpv renders — the branches of building cmd in `play`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpvTarget {
    /// X11: draw into our own override-redirect layer via `--wid`
    /// (`--keep-open=yes`: we close the layer FIRST, the poster does not flash).
    X11Overlay { window_id: u64 },
    /// A normal `--fullscreen --ontop` window; on Wayland the app_id is added
    /// (otherwise «above everything» cannot be kept — the KWin rule compensates).
    Window { wayland: bool },
}

/// mpv argv — port of building `cmd` in `play` (one function for both branches;
/// `ipc_sock` gets into argv by the original's rules: overlay — always,
/// window — only in the hybrid).
pub fn mpv_argv(
    path: &str,
    sound: bool,
    hybrid: bool,
    target: &MpvTarget,
    ipc_sock: &str,
) -> Vec<String> {
    let mut cmd: Vec<String> = vec![
        "mpv".into(),
        "--no-config".into(), // ignore the user's mpv.conf
        "--osd-level=0".into(),
        "--no-osc".into(),
        "--no-osd-bar".into(),
        "--no-input-default-bindings".into(),
        "--input-conf=/dev/null".into(),
        "--cursor-autohide=always".into(),
        "--no-input-cursor".into(),
        "--really-quiet".into(),
        "--no-terminal".into(),
        "--loop-file=no".into(),
        "--hwdec=auto".into(),
        format!("--mute={}", if sound { "no" } else { "yes" }),
    ];
    match target {
        MpvTarget::X11Overlay { window_id } => {
            cmd.push(format!("--wid={window_id}"));
            cmd.push("--keep-open=yes".into());
            cmd.push(format!("--input-ipc-server={ipc_sock}"));
        }
        MpvTarget::Window { wayland } => {
            cmd.extend(
                ["--fullscreen", "--no-border", "--ontop", "--keep-open=no"]
                    .map(String::from),
            );
            if *wayland {
                // --ontop is dead on Wayland — staying on top is ensured by the KWin rule by
                // app_id (see ensure_kwin_rule_text).
                cmd.push(format!("--wayland-app-id={KWIN_APP_ID}"));
            }
            if hybrid {
                cmd.push(format!("--input-ipc-server={ipc_sock}"));
            }
        }
    }
    if hybrid {
        // Start PAUSED: the first frame (= the theme poster) is under the ksplashqml
        // overlay; unpause — after it goes away.
        cmd.push("--pause".into());
    }
    cmd.push("--".into());
    cmd.push(path.into());
    cmd
}

// ── play/autostart messages (texts as in the original) ─────────────────────

/// «File not set/not found» at the start of `play`.
pub fn msg_no_file() -> String {
    tr("The startup movie file isn't set or can't be found")
}
/// No mpv (and no usable helper).
pub fn msg_no_mpv() -> String {
    tr("mpv isn't installed (it's needed for playback)")
}
/// A second launch while the flock lock is alive.
pub fn msg_already_playing() -> String {
    tr("The startup movie is already playing (second launch skipped)")
}
/// A successful run.
pub fn msg_played() -> String {
    tr("Played")
}
/// A non-blocking launch (a test from the UI).
pub fn msg_launched() -> String {
    tr("Started")
}
/// mpv did not finish within [`PLAY_TIMEOUT_SEC`].
pub fn msg_mpv_timeout() -> String {
    tr("mpv didn't finish within 120 s — stopped")
}
/// The helper did not finish within [`PLAY_TIMEOUT_SEC`].
pub fn msg_helper_timeout() -> String {
    tr("The startup movie didn't finish within 120 s — stopped")
}
/// No clip is set in `install_autostart`.
///
/// A `msgid`, not a ready text: the showcase recognizes this refusal by it
/// (`qt_steam::autostart_refusal_text`) — see [`msg_choose_first`].
pub const MSG_CHOOSE_FIRST: &str = n_("Choose a startup movie first");
/// [`MSG_CHOOSE_FIRST`] in the interface language.
pub fn msg_choose_first() -> String {
    tr(MSG_CHOOSE_FIRST)
}
/// The autostart .desktop is enabled.
pub fn msg_autostart_enabled() -> String {
    tr("Startup movie at login is on")
}
/// The login splash is completely disabled.
pub fn msg_autostart_disabled() -> String {
    tr("Startup movie at login is off")
}
/// The hybrid is enabled (the hybrid route in `install_autostart`).
pub fn msg_hybrid_route_ok() -> String {
    tr("Turned on (KSplash hybrid — no desktop flash)")
}
/// `_install_hybrid` succeeded.
pub fn msg_hybrid_installed() -> String {
    tr("KSplash hybrid is on")
}
/// `_uninstall_hybrid` succeeded.
pub fn msg_hybrid_uninstalled() -> String {
    tr("KSplash hybrid is off")
}
/// `_write_lnf_theme` succeeded.
pub fn msg_lnf_updated() -> String {
    tr("KSplash theme updated")
}

/// The original's `f"mpv exited with code {rc}"` (it was in Russian).
pub fn mpv_exit_message(rc: i64) -> String {
    trf("mpv exited with code %1", &[&rc])
}

/// The original's `f"rexon-splash-overlay exited with code {rc}"` (it was in Russian).
pub fn helper_exit_message(rc: i64) -> String {
    trf("rexon-splash-overlay exited with code %1", &[&rc])
}

/// The «hybrid failed» fallback message — port of the tail of `install_autostart`.
pub fn hybrid_fallback_message(hybrid_msg: &str, plain_ok: bool, plain_msg: &str) -> String {
    if plain_ok {
        trf("The hybrid didn't work (%1) — regular autostart is on instead", &[&hybrid_msg])
    } else {
        plain_msg.to_string()
    }
}

/// Contents of the autostart .desktop (phase 1) — port of `_install_desktop_autostart`.
///
/// `Comment` — in all catalog languages ([`crate::system_l10n::desktop_key`]):
/// previously it was only Russian, and an English-speaking user saw Russian.
pub fn autostart_desktop_content(launcher_exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rexon Session Splash\n\
         {comment}\
         Exec={launcher_exec}\n\
         X-KDE-autostart-phase=1\n\
         Terminal=false\n\
         NoDisplay=true\n",
        comment = crate::system_l10n::desktop_key("Comment", AUTOSTART_COMMENT),
    )
}

/// The `install_autostart` route (the steps themselves are on the executor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartRoute {
    /// No clip chosen → (false, [`MSG_CHOOSE_FIRST`]).
    NoPath,
    /// Hybrid: `_install_hybrid`; success → remove the .desktop and
    /// [`msg_hybrid_route_ok`]; failure → full hybrid rollback + phase 1
    /// ([`hybrid_fallback_message`]).
    Hybrid,
    /// Pure phase 1 (the autostart .desktop); `rollback_hybrid` — first
    /// roll back the hybrid's leftovers (`hybrid_installed` in the config).
    Plain { rollback_hybrid: bool },
}

/// The route decision — port of the fork in `install_autostart`
/// (the hybrid checkbox defaults to ON: `cfg.get("hybrid", True)`).
pub fn autostart_route(cfg: &Value, hybrid_supported: bool) -> AutostartRoute {
    if !truthy(&get_or(cfg, "path", Value::Null)) {
        return AutostartRoute::NoPath;
    }
    let pref = get_or(cfg, "hybrid", Value::from(true));
    if truthy(&pref) && hybrid_supported {
        return AutostartRoute::Hybrid;
    }
    AutostartRoute::Plain {
        rollback_hybrid: truthy(&get_or(cfg, "hybrid_installed", Value::Null)),
    }
}

/// Port of `is_enabled`: either of the two mechanisms is enabled.
pub fn is_enabled(autostart_file_exists: bool, unit_file_exists: bool) -> bool {
    autostart_file_exists || unit_file_exists
}

/// The poster source for the X11 layer — port of `_poster_for_video`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosterSource {
    /// A ready KSplash theme poster (the login critical path without ~0.3 s of
    /// ffmpeg) — the clip from the config and the file in place.
    Theme(String),
    /// A fresh frame by ffmpeg into the runtime directory.
    Extract(String),
}

/// The decision where the poster comes from: the theme (if `cfg.path == video` and the theme poster
/// exists) or extraction into runtime.
pub fn poster_source(
    cfg: &Value,
    video: &str,
    theme_poster_exists: bool,
    home: &str,
    runtime_dir: &str,
) -> PosterSource {
    let cfg_path = get_or(cfg, "path", Value::Null);
    if cfg_path.as_str() == Some(video) && theme_poster_exists {
        return PosterSource::Theme(theme_poster_path(home));
    }
    PosterSource::Extract(runtime_poster_path(runtime_dir))
}

// ============================================================================
// Layout of the Steam directories and the LnF package (paths — here, listings — the executor)
// ============================================================================

/// Directory of the Steam client's built-in splashes.
pub fn steam_movies_dir(root: &str) -> String {
    format!("{root}/steamui/movies")
}

/// File names of the built-in splashes (order = traversal order of the scan).
pub fn builtin_movie_filenames() -> Vec<String> {
    BUILTIN_NAMES
        .iter()
        .map(|(key, _)| format!("{key}.webm"))
        .collect()
}

/// Directory of Steam's custom clips (`config/uioverrides/movies`).
pub fn steam_custom_movies_dir(root: &str) -> String {
    format!("{root}/config/uioverrides/movies")
}

/// Directory of inventory splashes (purchases from the Points Shop already lie there as files).
pub fn steam_inventory_dir(root: &str) -> String {
    format!("{root}/config/communityitemscache/startupmovies")
}

/// Directory of Steam profiles (`userdata/<steamid>`).
pub fn steam_userdata_dir(root: &str) -> String {
    format!("{root}/userdata")
}

/// `localconfig.vdf` of one profile — the cache of the Points Shop response.
pub fn steam_localconfig_path(root: &str, steam_id: &str) -> String {
    format!("{root}/userdata/{steam_id}/config/localconfig.vdf")
}

/// `appcache/appinfo.vdf` — appid → game title (binary VDF).
pub fn steam_appinfo_path(root: &str) -> String {
    format!("{root}/appcache/appinfo.vdf")
}

/// Paths inside the KSplash theme LnF package — port of the layout of `_write_lnf_theme`.
/// There is no second copy of these names on the Python side: the theme and the poster must
/// lie where `theme_poster_path` looks for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnfLayout {
    /// `contents/splash` — the Splash.qml directory.
    pub splash_dir: String,
    /// `contents/splash/images` — the poster directory.
    pub images_dir: String,
    /// `contents/previews` — previews for the theme list in System Settings.
    pub previews_dir: String,
    /// `contents/splash/images/poster.png` — the clip's first frame.
    pub poster_png: String,
    /// `contents/previews/splash.png` — a copy of the poster for the theme list.
    pub preview_png: String,
    /// The package's `metadata.json`.
    pub metadata_json: String,
    /// `contents/splash/Splash.qml`.
    pub splash_qml: String,
}

/// Layout of the LnF package by theme directory.
pub fn lnf_layout(theme_dir: &str) -> LnfLayout {
    let splash_dir = format!("{theme_dir}/contents/splash");
    let images_dir = format!("{splash_dir}/images");
    let previews_dir = format!("{theme_dir}/contents/previews");
    LnfLayout {
        poster_png: format!("{images_dir}/poster.png"),
        preview_png: format!("{previews_dir}/splash.png"),
        metadata_json: format!("{theme_dir}/metadata.json"),
        splash_qml: format!("{splash_dir}/Splash.qml"),
        splash_dir,
        images_dir,
        previews_dir,
    }
}

// ============================================================================
// Execution constants: names, subprocess timeouts, poll-wait steps
// ============================================================================

/// The systemd target by which the hybrid detects a live Plasma session and in whose
/// `.wants` the manual enable fallback lies.
pub const PLASMA_WORKSPACE_TARGET: &str = "plasma-workspace.target";
/// The `Engine` value in ksplashrc for a QML splash.
pub const KSPLASH_ENGINE: &str = "KSplashQML";
/// The process name of the Plasma splash (it shows whether the overlay went away).
pub const KSPLASH_PROCESS: &str = "ksplashqml";
/// Suffix of the atomic write's temporary file (`.tmp` + `os.replace`).
pub const TMP_SUFFIX: &str = ".tmp";
/// Suffix of an unfinished showcase frame (renamed on ffmpeg success).
pub const PREVIEW_TMP_SUFFIX: &str = ".part.png";
/// X11 visual depths at which the poster is put into the layer as BGRX bytes.
/// A 30-bit (10bpc) visual draws such bytes wrong — then we do not set the poster,
/// the layer stays black (not fatal).
pub const X11_POSTER_DEPTHS: &[i64] = &[24, 32];

/// `ffprobe` timeout (clip duration).
pub const FFPROBE_TIMEOUT_SEC: f64 = 15.0;
/// `ffmpeg` timeout (showcase frame and theme poster).
pub const FFMPEG_TIMEOUT_SEC: f64 = 30.0;
/// Ceiling of one clip conversion (a minute-long clip converts in a few
/// minutes even on a weak processor); a hang beyond it is stopped.
pub const MOVIE_TRANSCODE_TIMEOUT_SEC: f64 = 1800.0;
/// `kwriteconfig6` timeout.
pub const KWRITECONFIG_TIMEOUT_SEC: f64 = 10.0;
/// `systemctl --user` timeout.
pub const SYSTEMCTL_TIMEOUT_SEC: f64 = 15.0;
/// `xrandr --query` timeout.
pub const XRANDR_TIMEOUT_SEC: f64 = 5.0;
/// Timeout of the «is ksplashqml alive» probe.
pub const PGREP_TIMEOUT_SEC: f64 = 5.0;
/// Timeout of the reconfigure call to the live KWin.
pub const DBUS_TIMEOUT_SEC: f64 = 5.0;
/// Pause after reconfigure: it is asynchronous, and the rule is applied when the window is
/// created — let KWin reread kwinrulesrc before mpv starts.
pub const KWIN_RECONFIGURE_SETTLE_SEC: f64 = 0.5;

/// Step of waiting for mpv's IPC socket in the hybrid relay.
pub const SOCK_POLL_SEC: f64 = 0.1;
/// Step of waiting for the Rust helper's IPC socket (it comes up noticeably faster).
pub const HELPER_POLL_SEC: f64 = 0.05;
/// Step of the «is ksplashqml alive» probes.
pub const KSPLASH_POLL_SEC: f64 = 0.1;
/// Step of polling `eof-reached` via mpv-IPC.
pub const EOF_POLL_SEC: f64 = 0.15;
/// Step of waiting for the compositor on a cold X11 login.
pub const COMPOSITOR_POLL_SEC: f64 = 0.1;
/// The «let the compositor draw the first scene» pause after it appears.
pub const COMPOSITOR_SETTLE_SEC: f64 = 0.2;
/// Safety polling of the raise loop when the event subscription failed.
pub const RAISE_POLL_FALLBACK_SEC: f64 = 0.08;
/// Waiting for a clean exit after `quit` via IPC.
pub const QUIT_WAIT_SEC: f64 = 3.0;
/// Waiting after SIGTERM, before SIGKILL.
pub const TERM_WAIT_SEC: f64 = 2.0;
/// Waiting after SIGTERM in the «mpv did not finish within PLAY_TIMEOUT_SEC» branch.
pub const MPV_TERM_WAIT_SEC: f64 = 5.0;
/// Wait timeout on the executor's unix sockets (mpv-IPC and sd_notify).
///
/// The executor no longer has its own copy: it was exactly through parallel copies of
/// timeouts that the wait for the helper's socket once drifted apart from the unit's `TimeoutStartSec`
/// and the panel ended up above the splash (incident 2026-07-25).
pub const IPC_SOCK_TIMEOUT_SEC: f64 = 2.0;
/// How long to wait for the raise-loop thread to finish when stopping the layer.
///
/// A cap, not a pause: the thread is woken immediately and usually leaves within a fraction of
/// a millisecond. The value matters because it EXCEEDS the loop's safety step
/// ([`RAISE_INTERVAL_SEC`]) — otherwise stopping would return before
/// the thread reached the flag check, and the layer would be closed right under it.
pub const RAISE_JOIN_WAIT_SEC: f64 = 1.0;

/// Marker of the REPLY line in the mpv-IPC stream: events are not tagged with this field,
/// so the executor reads the socket up to the first line with it (otherwise `recv`
/// would hang on a live connection).
pub const MPV_IPC_REPLY_MARKER: &str = "\"request_id\"";
/// Read limit of a reply from mpv-IPC — port of `while len(buf) < 65536`.
pub const MPV_IPC_READ_LIMIT: usize = 65536;

/// argv of the «is ksplashqml alive» probe — port of `_ksplash_running`.
/// `exe` — the path from `shutil.which("pgrep")`.
pub fn pgrep_argv(exe: &str, process: &str) -> Vec<String> {
    vec![exe.into(), "-x".into(), process.into()]
}

/// Port of CPython's `round()` (half to even): in the letterbox math
/// exactly .5 occurs at «round» resolutions, and the poster frame would shift
/// by a pixel relative to mpv's frame.
fn py_round(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

/// Fit the picture into the layer preserving the aspect ratio (letterbox — like mpv's
/// `--fullscreen`): (x, y, width, height) inside the `dst_*` window. The bars around
/// stay black. Port of the scaling in `_X11SplashOverlay.set_poster`.
pub fn letterbox_rect(src_w: i64, src_h: i64, dst_w: i64, dst_h: i64) -> (i64, i64, i64, i64) {
    if src_w <= 0 || src_h <= 0 {
        return (0, 0, dst_w.max(1), dst_h.max(1));
    }
    let scale = (dst_w as f64 / src_w as f64).min(dst_h as f64 / src_h as f64);
    let sw = (py_round(src_w as f64 * scale) as i64).max(1);
    let sh = (py_round(src_h as f64 * scale) as i64).max(1);
    ((dst_w - sw) / 2, (dst_h - sh) / 2, sw, sh)
}

// ── Status texts the original did not have in one place ────────────────────

/// Could not create the KSplash theme directory.
pub fn msg_theme_dir_failed(err: &str) -> String {
    trf("Couldn't create the theme folder: %1", &[&err])
}
/// Could not write the KSplash theme files.
pub fn msg_theme_write_failed(err: &str) -> String {
    trf("Couldn't write the theme: %1", &[&err])
}
/// ffmpeg is not installed (nothing to extract the poster with).
pub fn msg_no_ffmpeg() -> String {
    tr("ffmpeg isn't installed")
}
/// ffmpeg did not start.
pub fn msg_ffmpeg_failed(err: &str) -> String {
    format!("ffmpeg: {err}")
}
/// ffmpeg returned an error: the first 200 characters of stderr or «failed».
pub fn msg_ffmpeg_stderr(stderr: &str) -> String {
    let trimmed = stderr.trim();
    let short: String = trimmed.chars().take(200).collect();
    if short.is_empty() {
        tr("ffmpeg: failed")
    } else {
        format!("ffmpeg: {short}")
    }
}
/// The poster was extracted (port of `return True, "OK"`).
pub const MSG_OK: &str = "OK";
/// ksplashrc was not written (no kwriteconfig6 or it returned an error).
pub fn msg_ksplashrc_failed() -> String {
    tr("Couldn't write ksplashrc (kwriteconfig6)")
}
/// Self-healing: the ksplashrc setup was knocked off (the theme in brackets is foreign),
/// we restored ours.
pub fn msg_heal_ksplashrc_restored(prev_theme: &str) -> String {
    let was = if prev_theme.is_empty() {
        "ключ потерян".to_string()
    } else {
        format!("была «{prev_theme}»")
    };
    format!("Связка заставки в ksplashrc восстановлена ({was})")
}
/// Self-healing: could not restore the keys in ksplashrc — the clip is played anyway.
pub const MSG_HEAL_KSPLASHRC_FAILED: &str =
    "Не восстановить связку в ksplashrc — при входе мелькнёт штатная заставка Plasma";
/// Self-healing: the poster theme directory was missing, regenerated.
pub const MSG_HEAL_THEME_REBUILT: &str = "Тема-постер KSplash отсутствовала — пересоздана";
/// Self-healing: could not recreate the poster theme.
pub fn msg_heal_theme_failed(err: &str) -> String {
    format!("Не пересоздать тему-постер KSplash: {err}")
}
/// Self-healing: the config with the backup of the foreign theme was not written.
pub fn msg_heal_backup_failed(err: &str) -> String {
    format!("Не сохранить прежнюю тему KSplash: {err}")
}
/// The systemd unit was not written.
pub fn msg_unit_write_failed(err: &str) -> String {
    trf("Couldn't write the systemd unit: %1", &[&err])
}
/// The systemd unit was not enabled (and the manual symlink failed too).
pub fn msg_unit_enable_failed(err: &str) -> String {
    trf("Couldn't enable the systemd unit: %1", &[&err])
}
/// Summary of hybrid rollback problems (`problems` — already joined with «; »).
pub fn msg_rollback_problems(problems: &str) -> String {
    trf("Hybrid rollback finished with errors: %1", &[&problems])
}
/// A problem with the unit during rollback.
pub fn msg_problem_unit(err: &str) -> String {
    // TRANSLATORS: one problem in a list; %1 is a system error.
    trf("unit: %1", &[&err])
}
/// A problem with ksplashrc during rollback.
pub fn msg_problem_ksplashrc() -> String {
    tr("ksplashrc: kwriteconfig6 failed")
}
/// A problem with the theme directory during rollback.
pub fn msg_problem_theme(err: &str) -> String {
    // TRANSLATORS: one problem in a list; %1 is a system error.
    trf("theme: %1", &[&err])
}
/// A problem with removing the autostart .desktop.
pub fn msg_problem_autostart(err: &str) -> String {
    format!("autostart: {err}")
}
/// Could not enable phase 1 (the autostart .desktop).
pub fn msg_autostart_write_failed(err: &str) -> String {
    trf("Couldn't turn on: %1", &[&err])
}
/// Could not save the splash settings (⚠️ fix: in Python an OSError on
/// writing the config flew as an exception out of the middle of the hybrid install and
/// the user saw a traceback instead of the reason).
pub fn msg_config_write_failed(err: &str) -> String {
    trf("Couldn't save the startup movie settings: %1", &[&err])
}
/// mpv did not start (the exception text comes from the executor).
pub fn msg_mpv_spawn_failed(err: &str) -> String {
    trf("Couldn't start mpv: %1", &[&err])
}

// ============================================================================
// The «core drives the flow» seam: the executor trait
// ============================================================================

/// Result of `subprocess.run(argv, capture_output=True, text=True,
/// timeout=…)`; `None` from [`SplashIo::run`] — OSError/SubprocessError.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    /// `returncode`.
    pub rc: i64,
    /// `stdout` (an empty string if there is nothing).
    pub stdout: String,
    /// `stderr` (an empty string if there is nothing).
    pub stderr: String,
}

/// The executor interface: each method is exactly one side effect or
/// observation of the Python original. The core drives the whole splash flow (order,
/// decisions, texts, timeouts), while the REAL execution — Popen of mpv/ffmpeg/
/// systemctl, unix sockets, flock, writing files, the ctypes X11 overlay and
/// live threads — stays in the shim. File operation errors are returned
/// as text (`None` = success), so the flow can explain a refusal to the user instead of
/// failing with an exception from the middle of an installation.
pub trait SplashIo {
    /// `os.environ.get(name, "")`.
    fn env(&mut self, name: &str) -> String;
    /// `shutil.which(name) or ""`.
    fn which(&mut self, name: &str) -> String;
    /// Which product entry is selected now — for the live executor it is
    /// [`crate::installer::launch_entry_live`], the same rule as for the shortcut
    /// and the tray. An observation, not a core decision: it looks at PATH and the disks.
    fn launch_entry(&mut self) -> crate::installer::LaunchEntry;
    /// `os.path.isfile`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `os.path.isdir`.
    fn is_dir(&mut self, path: &str) -> bool;
    /// `os.path.exists`.
    fn exists(&mut self, path: &str) -> bool;
    /// `os.path.islink`.
    fn is_link(&mut self, path: &str) -> bool;
    /// `os.path.realpath`.
    fn realpath(&mut self, path: &str) -> String;
    /// `os.path.abspath`.
    fn abspath(&mut self, path: &str) -> String;
    /// `os.listdir(path)`; OSError → empty.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// File text (`errors="replace"`); OSError → None.
    fn read_text(&mut self, path: &str) -> Option<String>;
    /// File bytes; OSError → None.
    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>>;
    /// `int(os.path.getmtime(path))`; OSError → None.
    fn mtime(&mut self, path: &str) -> Option<i64>;
    /// `os.path.getsize(path)`; OSError → 0.
    fn file_size(&mut self, path: &str) -> i64;
    /// `os.makedirs(path, exist_ok=True)`; the OSError text on failure.
    fn makedirs(&mut self, path: &str) -> Option<String>;
    /// Write text; the OSError text on failure.
    fn write_text(&mut self, path: &str, content: &str) -> Option<String>;
    /// Atomic write (`.tmp` + `os.replace`); the OSError text on failure.
    fn write_atomic(&mut self, path: &str, content: &str) -> Option<String>;
    /// `os.replace(src, dst)`; the OSError text on failure.
    fn replace(&mut self, src: &str, dst: &str) -> Option<String>;
    /// `os.remove(path)` (the file may be missing); the OSError text on failure.
    fn remove_file(&mut self, path: &str) -> Option<String>;
    /// `shutil.rmtree(path)`; the OSError text on failure.
    fn remove_tree(&mut self, path: &str) -> Option<String>;
    /// `shutil.copyfile(src, dst)`; the OSError text on failure.
    fn copy_file(&mut self, src: &str, dst: &str) -> Option<String>;
    /// `os.symlink(target, link)`; the OSError text on failure.
    fn symlink(&mut self, target: &str, link: &str) -> Option<String>;
    /// `subprocess.run(argv, capture_output=True, text=True, timeout=…)`.
    fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut>;
    /// `subprocess.Popen(argv[, start_new_session=True])` → a process handle
    /// held by the executor; the exception text on failure.
    fn spawn(&mut self, argv: &[String], detached: bool) -> Result<i64, String>;
    /// `proc.poll()` — the exit code or None (still alive).
    fn poll(&mut self, proc: i64) -> Option<i64>;
    /// `proc.wait(timeout)` — the exit code or None on timeout.
    fn wait(&mut self, proc: i64, timeout: f64) -> Option<i64>;
    /// `proc.terminate()`.
    fn terminate(&mut self, proc: i64);
    /// `proc.kill()`.
    fn kill(&mut self, proc: i64);
    /// `time.monotonic()`.
    fn monotonic(&mut self) -> f64;
    /// `time.sleep(seconds)`.
    fn sleep(&mut self, seconds: f64);
    /// `random.choice(items)` — an empty list gives None.
    fn choice(&mut self, items: &[String]) -> Option<String>;
    /// One mpv-IPC command without a reply; False — the socket is unavailable.
    fn ipc_send(&mut self, sock: &str, payload: &str) -> bool;
    /// An mpv-IPC request: send the payload and read the stream up to the line with
    /// `reply_marker` (or to the end/limit). None — the socket is unavailable.
    fn ipc_request(&mut self, sock: &str, payload: &str, reply_marker: &str) -> Option<String>;
    /// A datagram to the systemd manager (the address is already normalized by the core).
    fn notify(&mut self, addr: &str, message: &str);
    /// `flock(LOCK_EX|LOCK_NB)` on the lock file; False — the lock is taken.
    fn acquire_lock(&mut self, path: &str) -> bool;
    /// reconfigure of the live KWin via the session bus (a shared helper).
    fn kwin_reconfigure_dbus(&mut self, timeout: f64) -> bool;
    /// Create the X11 layer (a fullscreen override-redirect window); None —
    /// no libX11/X server.
    fn x11_create(&mut self) -> Option<i64>;
    /// The layer's `window_id` — goes into mpv's `--wid`.
    fn x11_window_id(&mut self, overlay: i64) -> i64;
    /// Draw the poster as the layer background; False — failed (black stays).
    fn x11_set_poster(&mut self, overlay: i64, png: &str) -> bool;
    /// Show the layer (`XMapRaised`).
    fn x11_map(&mut self, overlay: i64);
    /// `_NET_WM_CM_S<n>` already has an owner (a compositor is running).
    fn x11_compositor_present(&mut self, overlay: i64) -> bool;
    /// Start the layer's background raise loop (event-driven, in its own thread).
    fn x11_raise_start(&mut self, overlay: i64);
    /// Stop the raise loop and wait for the thread.
    fn x11_raise_stop(&mut self, overlay: i64);
    /// Destroy the layer and close the X connection.
    fn x11_close(&mut self, overlay: i64);
    /// `os.access(path, os.X_OK)`.
    fn is_executable(&mut self, path: &str) -> bool;
    /// The compound step «install the hybrid» (the shim's `_install_hybrid`).
    ///
    /// ⚠️ The five methods below are the trait's only COMPOUND steps: their
    /// internals live in the core too ([`Splash::install_hybrid`] and
    /// neighbours), but the `install_autostart` route calls them THROUGH
    /// the executor. So the seam stays exactly where it was in the original:
    /// route verification swaps steps in the shim and compares the
    /// sequence with a recorded reference without starting a live
    /// ffmpeg/systemctl.
    fn step_install_hybrid(&mut self, video: &str) -> (bool, String);
    /// The compound step «roll back the hybrid» (the shim's `_uninstall_hybrid`).
    fn step_uninstall_hybrid(&mut self) -> (bool, String);
    /// The compound step «enable phase 1» (`_install_desktop_autostart`).
    fn step_install_desktop_autostart(&mut self) -> (bool, String);
    /// The compound step «remove the autostart .desktop».
    fn step_remove_desktop_autostart(&mut self);
    /// The compound step «is the hybrid possible» (the shim's `hybrid_supported`).
    fn step_hybrid_supported(&mut self) -> bool;
    /// A line into the log: `level` — `warning`/`info`/`debug`.
    fn log(&mut self, level: &str, message: &str);

    /// Autostart lines in the configs of compositors that do not run `.desktop`
    /// files (`crate::compositor_autostart`). `Some(entry)` — install
    /// and return the final text of the entry (with `NotShowIn=`) and the refusal for
    /// the current session; `None` — remove everywhere. By default — touch
    /// nothing: fake executors of the checks do not touch live configs.
    fn login_lines(&mut self, desktop: Option<&str>) -> (Option<String>, Option<String>) {
        (desktop.map(str::to_string), None)
    }
    /// A long command (the clip conversion): every stdout line goes to
    /// `on_line` as it arrives, `cancelled` is polled while it runs — `true`
    /// stops the command. The exit code; `None` — did not start or was
    /// cancelled. By default — a plain [`SplashIo::run`] with the lines fed
    /// afterwards (fake executors have nothing to stream).
    fn run_streaming(
        &mut self,
        argv: &[String],
        on_line: &mut dyn FnMut(&str),
        cancelled: &dyn Fn() -> bool,
    ) -> Option<i64> {
        if cancelled() {
            return None;
        }
        let out = self.run(argv, MOVIE_TRANSCODE_TIMEOUT_SEC)?;
        for line in out.stdout.lines() {
            on_line(line);
        }
        Some(out.rc)
    }
    /// Install the package with the program `binary` from the official
    /// repositories (the password window is the safeguard). `Err` — the reason
    /// for the person. By default — refuse: fake executors install nothing.
    fn install_program(&mut self, binary: &str) -> Result<(), String> {
        let _ = binary;
        Err(tr("Rexon can't install programs on this system by itself."))
    }
    /// Whether the entry at `desktop_path` will fire in the current session (for
    /// compositors without XDG autostart — only with its own line in their
    /// config).
    fn login_works(&mut self, desktop_path: &str) -> bool {
        let _ = desktop_path;
        true
    }
}

/// The splash flow: the core drives, [`SplashIo`] executes.
///
/// `home` — `os.path.expanduser("~")` (the executor's observation); everything
/// else (XDG roots, paths, texts, timeouts) the core computes itself, so that
/// no parallel copies of the values remain anywhere.
pub struct Splash<'a, IO: SplashIo> {
    io: &'a mut IO,
    home: String,
}

impl<'a, IO: SplashIo> Splash<'a, IO> {
    /// A new flow over an executor.
    pub fn new(io: &'a mut IO, home: &str) -> Self {
        Self {
            io,
            home: home.to_string(),
        }
    }

    // ── Paths (XDG is read every time: it differs between the unit and the UI process) ──

    fn cfg_home(&mut self) -> Option<String> {
        let v = self.io.env("XDG_CONFIG_HOME");
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    }

    fn config_path(&mut self) -> String {
        let x = self.cfg_home();
        config_path(x.as_deref(), &self.home.clone())
    }

    fn autostart_path(&mut self) -> String {
        let x = self.cfg_home();
        autostart_path(x.as_deref(), &self.home.clone())
    }

    fn unit_path(&mut self) -> String {
        let x = self.cfg_home();
        systemd_unit_path(x.as_deref(), &self.home.clone())
    }

    fn wants_dir(&mut self) -> String {
        let x = self.cfg_home();
        systemd_wants_dir(x.as_deref(), &self.home.clone())
    }

    fn wants_link(&mut self) -> String {
        let x = self.cfg_home();
        systemd_wants_link(x.as_deref(), &self.home.clone())
    }

    fn theme_dir(&mut self) -> String {
        lnf_theme_dir(&self.home.clone())
    }

    fn runtime_dir(&mut self) -> String {
        let v = self.io.env("XDG_RUNTIME_DIR");
        runtime_dir(if v.is_empty() { None } else { Some(&v) })
    }

    // ── Steam detection and splash scan ────────────────────────────────

    /// The Steam installation directory — port of `steam_root` (the core's candidates through
    /// realpath, the sign is the presence of `config/`).
    pub fn steam_root(&mut self) -> Option<String> {
        for cand in steam_root_candidates(&self.home.clone()) {
            let p = self.io.realpath(&cand);
            if self.io.is_dir(&format!("{p}/config")) {
                return Some(p);
            }
        }
        None
    }

    /// Walk the Steam profiles and merge the parsed `GetStartupMovies` records.
    fn merge_localconfig(&mut self, root: &str, full_records: bool) -> Value {
        let mut out = Map::new();
        let userdata = steam_userdata_dir(root);
        if !self.io.is_dir(&userdata) {
            return Value::Object(out);
        }
        for sid in self.io.listdir(&userdata) {
            let cfg = steam_localconfig_path(root, &sid);
            if !self.io.is_file(&cfg) {
                continue;
            }
            let Some(txt) = self.io.read_text(&cfg) else {
                continue;
            };
            let parsed = if full_records {
                parse_startup_movie_items(&txt)
            } else {
                parse_startup_movie_names(&txt)
            };
            if let Some(m) = parsed.as_object() {
                for (k, v) in m {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        Value::Object(out)
    }

    /// communityitemid → metadata — port of `_startup_movie_names`.
    pub fn startup_movie_names(&mut self, root: &str) -> Value {
        self.merge_localconfig(root, false)
    }

    /// communityitemid → the FULL item record — port of `_startup_movie_items`.
    pub fn startup_movie_items(&mut self, root: &str) -> Value {
        self.merge_localconfig(root, true)
    }

    /// appid → game title from `appcache/appinfo.vdf` — port of `_appinfo_names`.
    pub fn appinfo_names(&mut self, root: &str, appids: &[i64]) -> Vec<(i64, String)> {
        let mut want: Vec<i64> = appids.iter().copied().filter(|a| *a != 0).collect();
        want.sort_unstable();
        want.dedup();
        if want.is_empty() {
            return Vec::new();
        }
        let path = steam_appinfo_path(root);
        match self.io.read_bytes(&path) {
            Some(data) => parse_appinfo_names(&data, &want),
            None => {
                self.io.log("debug", "appinfo.vdf недоступен");
                Vec::new()
            }
        }
    }

    /// Basic scan `{id, name, path, builtin}` — port of `_scan_startup_movies_raw`.
    fn scan_movies_raw(&mut self) -> Vec<Value> {
        let mut out = match self.steam_root() {
            Some(root) => self.scan_steam_movies(&root),
            None => Vec::new(),
        };
        // Rexon's own folder — with and without Steam: a clip copied there
        // must not vanish from the showcase.
        let own = self.rexon_movies_dir();
        if self.io.is_dir(&own) {
            let files = self.io.listdir(&own);
            for rec in rexon_movie_records(&own, &files) {
                let path = rec["path"].as_str().unwrap_or_default().to_string();
                if !out.iter().any(|m| m["path"].as_str() == Some(path.as_str())) {
                    out.push(rec);
                }
            }
        }
        out
    }

    /// The three Steam sources: built-in, Points Shop, custom clips.
    fn scan_steam_movies(&mut self, root: &str) -> Vec<Value> {
        let root = root.to_string();
        let movies = steam_movies_dir(&root);
        let builtin_present: Vec<String> = if self.io.is_dir(&movies) {
            builtin_movie_filenames()
                .into_iter()
                .filter(|name| self.io.is_file(&format!("{movies}/{name}")))
                .collect()
        } else {
            Vec::new()
        };
        let sm_dir = steam_inventory_dir(&root);
        let (inventory, names) = if self.io.is_dir(&sm_dir) {
            let inv = self.io.listdir(&sm_dir);
            let nm = self.startup_movie_names(&root);
            (inv, nm)
        } else {
            (Vec::new(), Value::Object(Map::new()))
        };
        let custom_dir = steam_custom_movies_dir(&root);
        let custom = if self.io.is_dir(&custom_dir) {
            self.io.listdir(&custom_dir)
        } else {
            Vec::new()
        };
        scan_movies(&root, &builtin_present, &inventory, &names, &custom)
    }

    /// Supplement the scan records with item metadata — port of `_enrich_movies`.
    pub fn enrich_movies(&mut self, movies: &[Value]) -> Vec<Value> {
        let Some(root) = self.steam_root() else {
            return enrich_movies(movies, &Value::Object(Map::new()), &[]);
        };
        let items = self.startup_movie_items(&root);
        let appids: Vec<i64> = items
            .as_object()
            .map(|m| m.values().map(|e| as_int(e.get("appid"))).collect())
            .unwrap_or_default();
        let mut names = if appids.iter().any(|a| *a != 0) {
            self.appinfo_names(&root, &appids)
        } else {
            Vec::new()
        };
        names.sort();
        enrich_movies(movies, &items, &names)
    }

    /// Available Steam startup videos — port of `scan_startup_movies`.
    pub fn scan_startup_movies(&mut self) -> Vec<Value> {
        let raw = self.scan_movies_raw();
        let mut movies = self.enrich_movies(&raw);
        let choice = self.steam_chosen_movie();
        let dirs = self.own_movie_dirs();
        mark_movies(&mut movies, choice.as_deref(), &dirs);
        movies
    }

    // ── Own clips: folders, Steam's choice ─────────────────────────────

    /// Rexon's own folder of startup movies ([`rexon_movies_dir`]).
    pub fn rexon_movies_dir(&mut self) -> String {
        let x = self.io.env("XDG_DATA_HOME");
        rexon_movies_dir(if x.is_empty() { None } else { Some(&x) }, &self.home.clone())
    }

    /// Folders whose clips are the person's own (they may be deleted):
    /// Steam's custom clips folder (if Steam is here) and Rexon's own.
    pub fn own_movie_dirs(&mut self) -> Vec<String> {
        let mut dirs = Vec::new();
        if let Some(root) = self.steam_root() {
            dirs.push(steam_custom_movies_dir(&root));
        }
        dirs.push(self.rexon_movies_dir());
        dirs
    }

    /// The clip Steam itself plays at startup (`config.vdf`), as an absolute
    /// path. Read only — Rexon never writes Steam's choice.
    pub fn steam_chosen_movie(&mut self) -> Option<String> {
        let root = self.steam_root()?;
        let text = self.io.read_text(&steam_config_vdf_path(&root))?;
        let local = parse_steam_startup_movie(&text)?;
        Some(steam_choice_path(&root, &local))
    }

    // ── Accepting a custom clip ────────────────────────────────────────

    /// Check the chosen file ([`check_movie_probe`]). No ffprobe or it
    /// did not answer — the refusal «cannot read»: without a check the clip is not accepted.
    pub fn probe_movie(&mut self, path: &str) -> MovieCheck {
        let exe = self.io.which("ffprobe");
        if exe.is_empty() {
            return MovieCheck::reject(msg_movie_unreadable());
        }
        let argv = ffprobe_streams_argv(&exe, path);
        match self.io.run(&argv, FFPROBE_TIMEOUT_SEC) {
            Some(r) if r.rc == 0 => check_movie_probe(&r.stdout),
            _ => MovieCheck::reject(msg_movie_unreadable()),
        }
    }

    /// The program is on `PATH`, and if not — install its package right away
    /// (the owner's rule: missing programs are installed without a yes/no
    /// question, the password window is the safeguard). All or nothing:
    /// `Err` — the reason, and the caller does nothing.
    fn ensure_program(&mut self, binary: &str, progress: &mut dyn FnMut(ImportStage)) -> Result<String, String> {
        let exe = self.io.which(binary);
        if !exe.is_empty() {
            return Ok(exe);
        }
        progress(ImportStage::Installing);
        self.io.log("info", &format!("startup movie: {binary} is missing, installing it"));
        if let Err(why) = self.io.install_program(binary) {
            self.io.log("warning", &format!("startup movie: {binary} was not installed: {why}"));
            return Err(msg_tools_missing(&why));
        }
        let exe = self.io.which(binary);
        if exe.is_empty() {
            // TRANSLATORS: %1 is a program name (ffmpeg/ffprobe) that is still missing after its package was installed.
            let why = trf("%1 is still not found after installing", &[&binary]);
            self.io.log("warning", &format!("startup movie: {binary} still missing after install"));
            return Err(msg_tools_missing(&why));
        }
        Ok(exe)
    }

    /// Add the person's own clip: check it (ffprobe), copy it as is or convert
    /// it (ffmpeg, VP9 + Opus) into Steam's custom clips folder — or, without
    /// Steam, into Rexon's own — under a safe Latin name, and choose it for
    /// login. The original is never changed. Long: call it off the interface
    /// thread. `cancelled` stops a running conversion; `progress` — stages
    /// for the interface. Every step goes to the log.
    pub fn import_movie(
        &mut self,
        src: &str,
        cancelled: &dyn Fn() -> bool,
        progress: &mut dyn FnMut(ImportStage),
    ) -> ImportOutcome {
        self.io.log("info", &format!("startup movie: adding {src}"));
        if src.is_empty() || !self.io.is_file(src) {
            return ImportOutcome::failed(msg_movie_unreadable());
        }
        let sound = self.config_sound();
        // A clip that already lies in one of our folders is simply chosen.
        let own_dirs = self.own_movie_dirs();
        if is_own_movie_path(src, &own_dirs) {
            let (_, stem) = split_name(src);
            return match self.save_config(src, sound) {
                None => ImportOutcome { ok: true, path: src.to_string(), message: msg_import_done(&stem, false) },
                Some(e) => ImportOutcome::failed(msg_import_failed(&e)),
            };
        }

        if let Err(why) = self.ensure_program("ffprobe", progress) {
            return ImportOutcome::failed(why);
        }
        progress(ImportStage::Checking);
        let check = self.probe_movie(src);
        let root = self.steam_root();
        let src_is_webm = splitext_ext_lower(&split_name(src).0) == ".webm";
        let action = import_action(&check, root.is_some(), src_is_webm);
        self.io.log(
            "info",
            &format!(
                "startup movie: probe {}x{} {:.1}s video={} audio={} → {:?}",
                check.width, check.height, check.duration, check.video_codec, check.audio_codec, action
            ),
        );
        if let ImportAction::Refuse(reason) = &action {
            return ImportOutcome::failed(reason.clone());
        }
        let ffmpeg = if action == ImportAction::Transcode {
            match self.ensure_program("ffmpeg", progress) {
                Ok(exe) => exe,
                Err(why) => return ImportOutcome::failed(why),
            }
        } else {
            String::new()
        };

        let dest_dir = match &root {
            Some(r) => steam_custom_movies_dir(r),
            None => self.rexon_movies_dir(),
        };
        if let Some(e) = self.io.makedirs(&dest_dir) {
            self.io.log("warning", &format!("startup movie: cannot create {dest_dir}: {e}"));
            return ImportOutcome::failed(msg_import_failed(&e));
        }
        let (src_name, src_stem) = split_name(src);
        let ext = if action == ImportAction::Transcode {
            ".webm".to_string()
        } else {
            let e = splitext_ext_lower(&src_name);
            if root.is_some() || e.is_empty() { ".webm".to_string() } else { e }
        };
        let existing = self.io.listdir(&dest_dir);
        let name = unique_movie_file_name(&safe_movie_stem(&src_stem), &ext, &existing);
        let dest = format!("{dest_dir}/{name}");
        let part = format!("{dest_dir}/{}", movie_part_name(&name));

        let written = match action {
            ImportAction::Transcode => {
                progress(ImportStage::Converting(progress_fraction(0.0, check.duration)));
                let argv = transcode_argv(&ffmpeg, src, &part);
                let duration = check.duration;
                let mut on_line = |line: &str| {
                    if let Some(done) = parse_ffmpeg_progress(line) {
                        progress(ImportStage::Converting(progress_fraction(done, duration)));
                    }
                };
                let rc = self.io.run_streaming(&argv, &mut on_line, cancelled);
                match rc {
                    Some(0) if self.io.is_file(&part) => Ok(()),
                    _ if cancelled() => Err(None),
                    Some(code) => Err(Some(trf("the converter stopped with code %1", &[&code]))),
                    None => Err(Some(tr("the converter didn't start"))),
                }
            }
            _ => {
                progress(ImportStage::Copying);
                match self.io.copy_file(src, &part) {
                    None => Ok(()),
                    Some(e) => Err(Some(e)),
                }
            }
        };
        if let Err(why) = written {
            let _ = self.io.remove_file(&part);
            return match why {
                None => {
                    self.io.log("info", "startup movie: conversion cancelled");
                    ImportOutcome::failed(msg_import_cancelled())
                }
                Some(why) => {
                    self.io.log("warning", &format!("startup movie: {src} not added: {why}"));
                    ImportOutcome::failed(msg_import_failed(&why))
                }
            };
        }
        // ffprobe did not know the duration of the original (a browser-recorded
        // webm) — the limits are checked on the finished file.
        if action == ImportAction::Transcode && check.duration <= 0.0 {
            let again = self.probe_movie(&part);
            if again.verdict == MovieVerdict::Reject {
                let _ = self.io.remove_file(&part);
                return ImportOutcome::failed(again.reason);
            }
        }
        if let Some(e) = self.io.replace(&part, &dest) {
            let _ = self.io.remove_file(&part);
            return ImportOutcome::failed(msg_import_failed(&e));
        }
        self.io.log("info", &format!("startup movie: {src} → {dest}"));
        if let Some(e) = self.save_config(&dest, sound) {
            return ImportOutcome { ok: false, path: dest, message: msg_import_failed(&e) };
        }
        let title = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name).to_string();
        let mut message = msg_import_done(&title, root.is_some());
        for w in &check.warnings {
            message.push(' ');
            message.push_str(w);
        }
        ImportOutcome { ok: true, path: dest, message }
    }

    /// The sound flag of the current config (on by default — as the tab reads it).
    fn config_sound(&mut self) -> bool {
        self.load_config()
            .get("sound")
            .map(|v| v.as_bool().unwrap_or(true))
            .unwrap_or(true)
    }

    /// The old selection that is a path to someone else's file (before
    /// 24.11.2026 the original was stored, not a copy) — `Some(path)` if it
    /// must be copied ([`needs_migration`]).
    pub fn migration_candidate(&mut self) -> Option<String> {
        let current = self.load_config().get("path").and_then(Value::as_str).unwrap_or("").to_string();
        let exists = !current.is_empty() && self.io.is_file(&current);
        let dirs = self.own_movie_dirs();
        let root = self.steam_root();
        needs_migration(&current, exists, &dirs, root.as_deref()).then_some(current)
    }

    /// Copy the old selection into our folder and point the config (the
    /// selection and the star) at the copy. Without conversion and without
    /// installing anything — it runs by itself when the tab opens: a webm
    /// Steam plays goes into Steam's folder, anything else — into Rexon's own
    /// (our splash plays it there just the same).
    pub fn migrate_movie(&mut self, src: &str) -> ImportOutcome {
        self.io.log("info", &format!("startup movie: keeping a copy of the old selection {src}"));
        let root = self.steam_root();
        let (src_name, src_stem) = split_name(src);
        let ext = {
            let e = splitext_ext_lower(&src_name);
            if e.is_empty() { ".webm".to_string() } else { e }
        };
        let steam_ok = root.is_some()
            && ext == ".webm"
            && !self.io.which("ffprobe").is_empty()
            && self.probe_movie(src).verdict == MovieVerdict::AsIs;
        let dest_dir = match (&root, steam_ok) {
            (Some(r), true) => steam_custom_movies_dir(r),
            _ => self.rexon_movies_dir(),
        };
        if let Some(e) = self.io.makedirs(&dest_dir) {
            return ImportOutcome::failed(msg_import_failed(&e));
        }
        let existing = self.io.listdir(&dest_dir);
        let name = unique_movie_file_name(&safe_movie_stem(&src_stem), &ext, &existing);
        let dest = format!("{dest_dir}/{name}");
        let part = format!("{dest_dir}/{}", movie_part_name(&name));
        if let Some(e) = self.io.copy_file(src, &part) {
            let _ = self.io.remove_file(&part);
            self.io.log("warning", &format!("startup movie: old selection not copied: {e}"));
            return ImportOutcome::failed(msg_import_failed(&e));
        }
        if let Some(e) = self.io.replace(&part, &dest) {
            let _ = self.io.remove_file(&part);
            return ImportOutcome::failed(msg_import_failed(&e));
        }
        let cfg = self.load_config();
        let new_cfg = config_with_replaced_movie(&cfg, src, &dest);
        if let Some(e) = self.write_config(&new_cfg) {
            return ImportOutcome { ok: false, path: dest, message: msg_import_failed(&e) };
        }
        self.io.log("info", &format!("startup movie: old selection {src} → {dest}"));
        ImportOutcome { ok: true, path: dest, message: msg_migrated(&src_stem) }
    }

    /// The deletion question for a clip (with a warning if Steam plays it).
    pub fn delete_question(&mut self, path: &str) -> String {
        let (_, stem) = split_name(path);
        let chosen = self
            .steam_chosen_movie()
            .is_some_and(|c| c.eq_ignore_ascii_case(path));
        msg_delete_question(&stem, chosen)
    }

    /// Delete the person's own clip — only from our folders
    /// ([`is_own_movie_path`]); built-in and bought clips are refused. The
    /// thumbnail Steam made next to it goes too. If it was chosen for login,
    /// another found clip takes its place. `Ok` — the line for the person.
    pub fn delete_own_movie(&mut self, path: &str) -> Result<String, String> {
        let dirs = self.own_movie_dirs();
        // A symlink in our folder is removed as a link: the file it points to
        // stays untouched.
        if !is_own_movie_path(path, &dirs) || !self.io.is_file(path) {
            self.io.log("warning", &format!("startup movie: refused to delete {path}"));
            return Err(msg_delete_not_own());
        }
        if let Some(e) = self.io.remove_file(path) {
            self.io.log("warning", &format!("startup movie: cannot delete {path}: {e}"));
            return Err(msg_import_failed(&e));
        }
        self.io.log("info", &format!("startup movie: deleted {path}"));
        let (name, stem) = split_name(path);
        let dir = path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        for thumb in [format!("{dir}/{stem}.jpg"), format!("{dir}/{name}.jpg")] {
            if self.io.is_file(&thumb) {
                let _ = self.io.remove_file(&thumb);
            }
        }
        let cfg = self.load_config();
        let was_selected = cfg.get("path").and_then(Value::as_str) == Some(path);
        let fallback = if was_selected {
            self.scan_startup_movies()
                .iter()
                .filter_map(|m| m.get("path").and_then(Value::as_str).map(str::to_string))
                .find(|p| p != path)
        } else {
            None
        };
        let new_cfg = config_without_movie(&cfg, path, fallback.as_deref());
        if let Some(e) = self.write_config(&new_cfg) {
            return Err(msg_import_failed(&e));
        }
        if let Some(next) = fallback {
            // Through the ordinary choice: it also refreshes the hybrid poster.
            let sound = self.config_sound();
            let _ = self.save_config(&next, sound);
        }
        Ok(msg_deleted(&stem))
    }

    // ── Showcase frame ─────────────────────────────────────────────────

    /// Clip duration — port of `_video_duration` (0.0 = cannot determine).
    pub fn video_duration(&mut self, path: &str) -> f64 {
        let exe = self.io.which("ffprobe");
        if exe.is_empty() {
            return 0.0;
        }
        let argv = ffprobe_duration_argv(&exe, path);
        match self.io.run(&argv, FFPROBE_TIMEOUT_SEC) {
            Some(r) => parse_duration(&r.stdout),
            None => 0.0,
        }
    }

    /// A clip frame for the showcase (cached) — port of `preview_frame`.
    pub fn preview_frame(&mut self, video: &str) -> Option<String> {
        if video.is_empty() || !self.io.is_file(video) {
            return None;
        }
        let stamp = self.io.mtime(video).unwrap_or(0);
        let abs = self.io.abspath(video);
        let cache_dir = {
            let x = self.io.env("XDG_CACHE_HOME");
            preview_cache_dir(if x.is_empty() { None } else { Some(&x) }, &self.home.clone())
        };
        let out = preview_cache_path(&cache_dir, &abs, stamp);
        if self.io.is_file(&out) && self.io.file_size(&out) > 0 {
            return Some(out);
        }
        let exe = self.io.which("ffmpeg");
        if exe.is_empty() {
            return None;
        }
        if let Some(err) = self.io.makedirs(&cache_dir) {
            self.io.log("debug", &format!("preview_frame: не создать кэш: {err}"));
            return None;
        }
        let seek = self.video_duration(video) * PREVIEW_AT;
        let tmp = format!("{out}{PREVIEW_TMP_SUFFIX}");
        let argv = preview_ffmpeg_argv(&exe, video, &tmp, seek);
        let Some(first) = self.io.run(&argv, FFMPEG_TIMEOUT_SEC) else {
            self.io.log("debug", "preview_frame ffmpeg: не запустился");
            return None;
        };
        if !(first.rc == 0 && self.io.is_file(&tmp)) {
            // The clip is shorter than expected / broken — try the very first frame
            // (the same argv without -ss).
            let argv0 = preview_ffmpeg_argv(&exe, video, &tmp, 0.0);
            let second = self.io.run(&argv0, FFMPEG_TIMEOUT_SEC)?;
            if !(second.rc == 0 && self.io.is_file(&tmp)) {
                return None;
            }
        }
        if self.io.replace(&tmp, &out).is_some() {
            return None;
        }
        Some(out)
    }

    // ── Config, favorites, rotation ────────────────────────────────────

    /// Splash config — port of `load_config`.
    pub fn load_config(&mut self) -> Value {
        let path = self.config_path();
        let text = self.io.read_text(&path);
        parse_config(text.as_deref())
    }

    /// Atomic config write — port of `_write_config`; the error text on
    /// failure (the executor raises it as an exception, like the original).
    pub fn write_config(&mut self, cfg: &Value) -> Option<String> {
        let text = config_json(cfg);
        let path = self.config_path();
        let dir = match path.rfind('/') {
            Some(i) => path[..i].to_string(),
            None => String::new(),
        };
        if !dir.is_empty() {
            if let Some(e) = self.io.makedirs(&dir) {
                return Some(e);
            }
        }
        self.io.write_atomic(&path, &text)
    }

    /// Paths of starred clips — port of `favorites`.
    pub fn favorites(&mut self) -> Vec<String> {
        let cfg = self.load_config();
        favorites_from_config(&cfg)
    }

    /// Star/unstar — port of `set_favorite`.
    pub fn set_favorite(&mut self, path: &str, on: bool) -> (Vec<String>, Option<String>) {
        let cfg = self.load_config();
        let (new_cfg, fav) = config_with_favorite(&cfg, path, on);
        let err = self.write_config(&new_cfg);
        (fav, err)
    }

    /// Whether rotation is enabled — port of `rotation_enabled`.
    pub fn rotation_enabled(&mut self) -> bool {
        let cfg = self.load_config();
        rotation_enabled(&cfg)
    }

    /// Toggle rotation — port of `set_rotation`.
    pub fn set_rotation(&mut self, enabled: bool) -> Option<String> {
        let cfg = self.load_config();
        let new_cfg = config_with_rotation(&cfg, enabled);
        self.write_config(&new_cfg)
    }

    /// What we rotate from — port of `rotation_pool`.
    pub fn rotation_pool(&mut self) -> Vec<String> {
        let fav: Vec<String> = self
            .favorites()
            .into_iter()
            .filter(|p| self.io.is_file(p))
            .collect();
        // A full scan is expensive (ffmpeg/VDF) — only when there are few favorites and
        // the rule would lead to «everything found» anyway.
        let all: Vec<String> = if rotation_needs_full_scan(fav.len()) {
            self.scan_startup_movies()
                .iter()
                .filter_map(|m| m.get("path").and_then(Value::as_str).map(str::to_string))
                .filter(|p| self.io.is_file(p))
                .collect()
        } else {
            Vec::new()
        };
        rotation_pool(&fav, &all)
    }

    /// Pick a clip for the NEXT login and save it — port of `advance_rotation`.
    pub fn advance_rotation(&mut self) -> Option<String> {
        if !self.rotation_enabled() {
            return None;
        }
        let pool = self.rotation_pool();
        let cfg = self.load_config();
        let cur = cfg.get("path").and_then(Value::as_str).map(str::to_string);
        let choices = rotation_choices(&pool, cur.as_deref());
        if choices.is_empty() {
            return None;
        }
        let nxt = self.io.choice(&choices)?;
        let sound = truthy(&get_or(&cfg, "sound", Value::from(true)));
        self.save_config(&nxt, sound);
        Some(nxt)
    }

    /// Save the clip choice (and update the theme poster) — port of `save_config`.
    pub fn save_config(&mut self, path: &str, sound: bool) -> Option<String> {
        let cfg = self.load_config();
        let new_cfg = config_with_selection(&cfg, path, sound);
        let theme_dir = self.theme_dir();
        let dir_exists = self.io.is_dir(&theme_dir);
        let refresh = should_refresh_hybrid_poster(&new_cfg, dir_exists);
        let err = self.write_config(&new_cfg);
        if refresh {
            let (ok, msg) = self.write_lnf_theme(path);
            if !ok {
                self.io
                    .log("warning", &format!("hybrid poster refresh failed: {msg}"));
            }
        }
        err
    }

    /// Remember the «hybrid on/off» checkbox — port of `save_hybrid_pref`.
    pub fn save_hybrid_pref(&mut self, enabled: bool) -> Option<String> {
        let cfg = self.load_config();
        let new_cfg = config_with_hybrid_pref(&cfg, enabled);
        self.write_config(&new_cfg)
    }

    // ── KSplash hybrid ─────────────────────────────────────────────────

    /// The hybrid is possible — port of `hybrid_supported` (short-circuit kept:
    /// not KDE → we do not look for binaries, no binaries → we do not call systemctl).
    pub fn hybrid_supported(&mut self) -> bool {
        let xdg = self.io.env("XDG_CURRENT_DESKTOP");
        let xdg_opt = if xdg.is_empty() { None } else { Some(xdg.clone()) };
        if !is_kde_desktop(xdg_opt.as_deref()) {
            return false;
        }
        let mut all_present = true;
        for bin in HYBRID_REQUIRED_BINARIES {
            if self.io.which(bin).is_empty() {
                all_present = false;
                break;
            }
        }
        let active = all_present && self.systemctl_user(&["is-active", PLASMA_WORKSPACE_TARGET]);
        hybrid_supported_decision(xdg_opt.as_deref(), all_present, active)
    }

    /// The hybrid is installed — port of `hybrid_installed`.
    pub fn hybrid_installed(&mut self) -> bool {
        let cfg = self.load_config();
        let unit = self.unit_path();
        let exists = self.io.is_file(&unit);
        hybrid_installed(&cfg, exists)
    }

    /// The clip's first frame → PNG — port of `_extract_poster`.
    pub fn extract_poster(&mut self, video: &str, out_png: &str) -> (bool, String) {
        let ff = self.io.which("ffmpeg");
        if ff.is_empty() {
            return (false, msg_no_ffmpeg());
        }
        let argv = poster_ffmpeg_argv(&ff, video, out_png);
        let Some(r) = self.io.run(&argv, FFMPEG_TIMEOUT_SEC) else {
            return (false, msg_ffmpeg_failed(&tr("didn't start")));
        };
        if r.rc != 0 || !self.io.is_file(out_png) {
            return (false, msg_ffmpeg_stderr(&r.stderr));
        }
        (true, MSG_OK.to_string())
    }

    /// Generate/update the KSplash theme LnF package — port of `_write_lnf_theme`.
    pub fn write_lnf_theme(&mut self, video: &str) -> (bool, String) {
        let meta_text = lnf_metadata_json();
        let qml_text = SPLASH_QML.to_string();
        let theme_dir = self.theme_dir();
        let lay = lnf_layout(&theme_dir);
        for dir in [&lay.images_dir, &lay.previews_dir] {
            if let Some(e) = self.io.makedirs(dir) {
                return (false, msg_theme_dir_failed(&e));
            }
        }
        let (ok, msg) = self.extract_poster(video, &lay.poster_png);
        if !ok {
            return (false, msg);
        }
        // preview for the theme list in System Settings → Splash Screen
        if let Some(e) = self.io.copy_file(&lay.poster_png, &lay.preview_png) {
            self.io
                .log("debug", &format!("splash preview copy failed: {e}"));
        }
        if let Some(e) = self.io.write_text(&lay.metadata_json, &meta_text) {
            return (false, msg_theme_write_failed(&e));
        }
        if let Some(e) = self.io.write_text(&lay.splash_qml, &qml_text) {
            return (false, msg_theme_write_failed(&e));
        }
        (true, msg_lnf_updated())
    }

    /// A value from the user's ksplashrc — port of `_ksplashrc_get`.
    pub fn ksplashrc_get(&mut self, key: &str) -> String {
        let x = self.cfg_home();
        let path = ksplashrc_path(x.as_deref(), &self.home.clone());
        match self.io.read_text(&path) {
            Some(text) => ksplashrc_get(&text, key),
            None => String::new(),
        }
    }

    /// Write/delete a key in ksplashrc — port of `_ksplashrc_set`.
    pub fn ksplashrc_set(&mut self, key: &str, value: &str) -> bool {
        let exe = self.io.which("kwriteconfig6");
        if exe.is_empty() {
            return false;
        }
        let argv = kwriteconfig_argv(&exe, key, value);
        matches!(self.io.run(&argv, KWRITECONFIG_TIMEOUT_SEC), Some(r) if r.rc == 0)
    }

    /// `systemctl --user …` — port of `_systemctl_user`.
    pub fn systemctl_user(&mut self, args: &[&str]) -> bool {
        let exe = self.io.which("systemctl");
        if exe.is_empty() {
            return false;
        }
        let argv = systemctl_user_argv(&exe, args);
        matches!(self.io.run(&argv, SYSTEMCTL_TIMEOUT_SEC), Some(r) if r.rc == 0)
    }

    /// The splash launch command for the autostart `Exec=` — port of
    /// `_launcher_exec`, the entry follows the shortcut rule ([`desktop_launcher_exec`]).
    pub fn launcher_exec(&mut self, python_executable: &str) -> String {
        let (entry, rexon) = self.entry_and_launcher();
        desktop_launcher_exec(&entry, rexon.as_deref(), python_executable)
    }

    /// The same for the hybrid unit's `ExecStart=` ([`unit_launcher_exec`]).
    pub fn unit_launcher_exec(&mut self, python_executable: &str) -> String {
        let (entry, rexon) = self.entry_and_launcher();
        unit_launcher_exec(&entry, rexon.as_deref(), python_executable)
    }

    /// The product entry and — only for the Python entry — the `rexon` launcher from `PATH`.
    fn entry_and_launcher(&mut self) -> (crate::installer::LaunchEntry, Option<String>) {
        let entry = self.io.launch_entry();
        let rexon = match entry {
            crate::installer::LaunchEntry::MainWindow(_) => None,
            crate::installer::LaunchEntry::Python => {
                Some(self.io.which(crate::installer::PYTHON_ENTRY_CMD)).filter(|p| !p.is_empty())
            }
        };
        (entry, rexon)
    }

    /// Self-healing of the hybrid setup before a REAL splash display —
    /// the cause and the symptom are described in the module header.
    ///
    /// Returns nothing and cancels nothing: every failure is a line in
    /// the log, showing the clip matters more than the setup being intact.
    pub fn heal_hybrid(&mut self, video: &str) {
        // The theme directory may have been taken away by `plasma-apply-lookandfeel --resetLayout`,
        // a cleanup of `~/.local/share` or a profile move — then there is no poster
        // at all, and the login stays black until mpv's first frame.
        let theme_dir = self.theme_dir();
        if !self.io.is_dir(&theme_dir) {
            let (ok, msg) = self.write_lnf_theme(video);
            if ok {
                self.io.log("warning", MSG_HEAL_THEME_REBUILT);
            } else {
                self.io.log("warning", &msg_heal_theme_failed(&msg));
            }
        }
        let cfg = self.load_config();
        let cur_theme = self.ksplashrc_get("Theme");
        if !should_heal_ksplashrc(&cfg, &cur_theme) {
            return;
        }
        let cur_engine = self.ksplashrc_get("Engine");
        // A foreign value goes to the backup under THE SAME condition as during installation
        // (`_install_hybrid`): the hybrid rollback must return the user their
        // real theme, not ours. An empty string is a legitimate value:
        // on rollback it deletes the key, restoring the «no key» state.
        if should_backup_prev_theme(&cur_theme) {
            let mut cfg = cfg;
            if let Some(m) = cfg.as_object_mut() {
                m.insert("prev_ksplash_theme".into(), Value::from(cur_theme.clone()));
                m.insert("prev_ksplash_engine".into(), Value::from(cur_engine));
            }
            if let Some(e) = self.write_config(&cfg) {
                self.io.log("warning", &msg_heal_backup_failed(&e));
            }
        }
        if self.ksplashrc_set("Theme", KSPLASH_THEME_ID)
            && self.ksplashrc_set("Engine", KSPLASH_ENGINE)
        {
            self.io
                .log("warning", &msg_heal_ksplashrc_restored(&cur_theme));
        } else {
            self.io.log("warning", MSG_HEAL_KSPLASHRC_FAILED);
        }
    }

    /// Install the hybrid: poster theme + ksplashrc + systemd unit — port of
    /// `_install_hybrid`.
    pub fn install_hybrid(&mut self, video: &str, python_executable: &str) -> (bool, String) {
        // ⛔First — IS THERE ANYTHING TO START IT WITH. The check comes before a single edit on
        // disk on purpose: refusing halfway (the poster theme is written,
        // ksplashrc already switched to our theme, no unit) would mean
        // leaving the session login in a state worse than the original.
        let exec = self.unit_launcher_exec(python_executable);
        if exec.is_empty() {
            return (false, crate::app::MSG_NOTHING_TO_LAUNCH.to_string());
        }
        let (ok, msg) = self.write_lnf_theme(video);
        if !ok {
            return (false, msg);
        }
        let mut cfg = self.load_config();
        let cur_theme = self.ksplashrc_get("Theme");
        let cur_engine = self.ksplashrc_get("Engine");
        // Back up the original — only if the theme set now is NOT ours (re-enabling
        // must not overwrite the real backup with our own ID).
        // Persist BEFORE editing ksplashrc: if the installation falls over later, the rollback
        // will have something to restore from.
        if should_backup_prev_theme(&cur_theme) {
            if let Some(m) = cfg.as_object_mut() {
                m.insert("prev_ksplash_theme".into(), Value::from(cur_theme.clone()));
                m.insert("prev_ksplash_engine".into(), Value::from(cur_engine));
            }
            if let Some(e) = self.write_config(&cfg) {
                return (false, msg_config_write_failed(&e));
            }
        }
        if !(self.ksplashrc_set("Theme", KSPLASH_THEME_ID)
            && self.ksplashrc_set("Engine", KSPLASH_ENGINE))
        {
            return (false, msg_ksplashrc_failed());
        }
        let unit_path = self.unit_path();
        let unit_dir = match unit_path.rfind('/') {
            Some(i) => unit_path[..i].to_string(),
            None => String::new(),
        };
        let content = systemd_unit_content(&exec);
        if let Some(e) = self.io.makedirs(&unit_dir) {
            return (false, msg_unit_write_failed(&e));
        }
        if let Some(e) = self.io.write_atomic(&unit_path, &content) {
            return (false, msg_unit_write_failed(&e));
        }
        self.systemctl_user(&["daemon-reload"]);
        let unit_name = SYSTEMD_UNIT_NAME;
        if !self.systemctl_user(&["enable", unit_name]) {
            // fallback: a manual symlink in plasma-workspace.target.wants
            let wants_dir = self.wants_dir();
            let link = self.wants_link();
            if let Some(e) = self.io.makedirs(&wants_dir) {
                return (false, msg_unit_enable_failed(&e));
            }
            if !self.io.is_link(&link) {
                if let Some(e) = self.io.symlink(&unit_path, &link) {
                    return (false, msg_unit_enable_failed(&e));
                }
            }
        }
        if let Some(m) = cfg.as_object_mut() {
            m.insert("hybrid_installed".into(), Value::from(true));
        }
        if let Some(e) = self.write_config(&cfg) {
            return (false, msg_config_write_failed(&e));
        }
        (true, msg_hybrid_installed())
    }

    /// Full hybrid rollback: unit, ksplashrc, theme — port of `_uninstall_hybrid`.
    pub fn uninstall_hybrid(&mut self) -> (bool, String) {
        let mut problems: Vec<String> = Vec::new();
        let unit_name = SYSTEMD_UNIT_NAME;
        self.systemctl_user(&["disable", unit_name]);
        let unit_path = self.unit_path();
        if self.io.is_file(&unit_path) {
            if let Some(e) = self.io.remove_file(&unit_path) {
                problems.push(msg_problem_unit(&e));
            }
        }
        // clean up the manual symlink if enable went through the fallback
        let link = self.wants_link();
        if problems.is_empty() && self.io.is_link(&link) {
            if let Some(e) = self.io.remove_file(&link) {
                problems.push(msg_problem_unit(&e));
            }
        }
        self.systemctl_user(&["daemon-reload"]);

        let mut cfg = self.load_config();
        // ksplashrc is rolled back ONLY if our theme is still there
        // (the user may have chosen another in System Settings).
        let cur = self.ksplashrc_get("Theme");
        if should_rollback_ksplashrc(&cur) {
            let prev_theme = get_or(&cfg, "prev_ksplash_theme", Value::from(""))
                .as_str()
                .unwrap_or("")
                .to_string();
            let prev_engine = get_or(&cfg, "prev_ksplash_engine", Value::from(""))
                .as_str()
                .unwrap_or("")
                .to_string();
            if !(self.ksplashrc_set("Theme", &prev_theme)
                && self.ksplashrc_set("Engine", &prev_engine))
            {
                problems.push(msg_problem_ksplashrc());
            }
        }
        let theme_dir = self.theme_dir();
        if self.io.is_dir(&theme_dir) {
            if let Some(e) = self.io.remove_tree(&theme_dir) {
                problems.push(msg_problem_theme(&e));
            }
        }
        if let Some(m) = cfg.as_object_mut() {
            m.remove("hybrid_installed");
            m.remove("prev_ksplash_theme");
            m.remove("prev_ksplash_engine");
        }
        if let Some(e) = self.write_config(&cfg) {
            problems.push(msg_config_write_failed(&e));
        }
        if !problems.is_empty() {
            return (false, msg_rollback_problems(&problems.join("; ")));
        }
        (true, msg_hybrid_uninstalled())
    }

    // ── Dispute over the splash slot when applying a look ──────────────

    /// Settle the dispute over the KSplash slot before applying a look.
    ///
    /// `ask` shows the question to the user and returns «Yes» (`true`) —
    /// each host has its own dialog, the decision is one. Called BEFORE applying:
    /// on «Yes» the hybrid is removed right away so that self-healing does not bring our
    /// theme back at the next login. `ask` is not called at all when there is nothing
    /// to dispute; for kinds that do not touch the slot even the config is not read.
    pub fn resolve_splash_slot(
        &mut self,
        apply_kind: &str,
        theme_name: &str,
        ask: impl FnOnce(&SplashSlotConflict) -> bool,
    ) -> SplashSlotPlan {
        if !apply_kind_takes_ksplash_slot(apply_kind) {
            return SplashSlotPlan::plain();
        }
        let installed = self.hybrid_installed();
        let Some(conflict) = splash_slot_conflict(apply_kind, theme_name, installed) else {
            return SplashSlotPlan::plain();
        };
        if !ask(&conflict) {
            // The slot stays with the clip. For a whole look this is not a refusal:
            // we apply everything, and return the slot with the same self-healing that
            // keeps the hybrid at login (there is no second return mechanism).
            if conflict.keep_applies_rest {
                let video = self
                    .load_config()
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return SplashSlotPlan::Apply {
                    keep_video: Some(video),
                    note: conflict.kept,
                };
            }
            return SplashSlotPlan::Stop(conflict.kept);
        }
        // The slot is given to the chosen splash — we remove the hybrid HONESTLY.
        let (ok, detail) = self.uninstall_hybrid();
        if !ok {
            return SplashSlotPlan::Stop(msg_slot_release_failed(&detail));
        }
        // With «No» there is nothing to show at login — and «the chosen splash» in
        // the note would be untrue.
        let note = if theme_name == crate::appearance_installer::SPLASH_NONE {
            tr("The Rexon startup movie is off.")
        } else {
            msg_slot_released()
        };
        SplashSlotPlan::Apply { keep_video: None, note }
    }

    /// Finish the plan after applying: return the slot to the clip if
    /// the user kept it, and hand back the result with a note.
    ///
    /// We heal ALWAYS, not only on success: `plasma-apply-lookandfeel`
    /// manages to write ksplashrc even in runs it itself considers
    /// failed.
    pub fn finish_splash_slot(
        &mut self,
        keep_video: Option<&str>,
        note: &str,
        ok: bool,
        msg: &str,
    ) -> String {
        if let Some(video) = keep_video {
            self.heal_hybrid(video);
        }
        msg_with_slot_note(msg, note, ok)
    }

    // ── Autostart at login ─────────────────────────────────────────────

    /// Whether splash autostart is enabled — port of `is_enabled`.
    pub fn is_enabled(&mut self) -> bool {
        let a = self.autostart_path();
        let u = self.unit_path();
        let af = self.io.is_file(&a);
        // The entry exists, but this compositor will not run it — not «enabled».
        let af = af && self.io.login_works(&a);
        let uf = self.io.is_file(&u);
        is_enabled(af, uf)
    }

    /// Phase 1: the autostart .desktop — port of `_install_desktop_autostart`.
    pub fn install_desktop_autostart(&mut self, python_executable: &str) -> (bool, String) {
        let exec = self.launcher_exec(python_executable);
        // ⛔An empty line — nothing to start the splash with. Previously a dead
        // `python3 -m rexon.app --session-splash` arrived here, and the autostart
        // entry reported success while showing nothing at login.
        if exec.is_empty() {
            return (false, crate::app::MSG_NOTHING_TO_LAUNCH.to_string());
        }
        let content = autostart_desktop_content(&exec);
        let (content, refusal) = self.io.login_lines(Some(&content));
        let content = content.unwrap_or_default();
        let path = self.autostart_path();
        let dir = match path.rfind('/') {
            Some(i) => path[..i].to_string(),
            None => String::new(),
        };
        if let Some(e) = self.io.makedirs(&dir) {
            return (false, msg_autostart_write_failed(&e));
        }
        if let Some(e) = self.io.write_atomic(&path, &content) {
            return (false, msg_autostart_write_failed(&e));
        }
        // The entry for other environments was written, but it will not fire here.
        if let Some(msg) = refusal {
            return (false, msg);
        }
        (true, msg_autostart_enabled())
    }

    /// Remove the autostart .desktop — port of `_remove_desktop_autostart`.
    pub fn remove_desktop_autostart(&mut self) {
        self.io.login_lines(None);
        let path = self.autostart_path();
        if self.io.is_file(&path) {
            if let Some(e) = self.io.remove_file(&path) {
                self.io
                    .log("warning", &format!("autostart remove failed: {e}"));
            }
        }
    }

    /// Enable the splash at login — port of `install_autostart` (the steps
    /// `install_hybrid`/`uninstall_hybrid`/phase 1 go through the executor,
    /// so the shim keeps exactly the same seam for verification).
    pub fn install_autostart(&mut self) -> (bool, String) {
        let cfg = self.load_config();
        let supported = self.io.step_hybrid_supported();
        match autostart_route(&cfg, supported) {
            AutostartRoute::NoPath => (false, msg_choose_first()),
            AutostartRoute::Hybrid => {
                let video = get_or(&cfg, "path", Value::Null)
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let (ok, msg) = self.io.step_install_hybrid(&video);
                if ok {
                    self.io.step_remove_desktop_autostart();
                    return (true, msg_hybrid_route_ok());
                }
                self.io.log(
                    "warning",
                    &format!("hybrid install failed, fallback to autostart: {msg}"),
                );
                self.io.step_uninstall_hybrid(); // do not leave half a hybrid
                let (ok2, msg2) = self.io.step_install_desktop_autostart();
                (ok2, hybrid_fallback_message(&msg, ok2, &msg2))
            }
            AutostartRoute::Plain { rollback_hybrid } => {
                if rollback_hybrid {
                    self.io.step_uninstall_hybrid();
                }
                self.io.step_install_desktop_autostart()
            }
        }
    }

    /// Disable the splash at login — port of `uninstall_autostart`.
    pub fn uninstall_autostart(&mut self) -> (bool, String) {
        let mut problems: Vec<String> = Vec::new();
        let cfg = self.load_config();
        let unit = self.unit_path();
        let unit_exists = self.io.is_file(&unit);
        if truthy(&get_or(&cfg, "hybrid_installed", Value::Null)) || unit_exists {
            let (ok, msg) = self.io.step_uninstall_hybrid();
            if !ok {
                problems.push(msg);
            }
        }
        self.io.login_lines(None);
        let path = self.autostart_path();
        if self.io.is_file(&path) {
            if let Some(e) = self.io.remove_file(&path) {
                problems.push(msg_problem_autostart(&e));
            }
        }
        if !problems.is_empty() {
            return (false, problems.join("; "));
        }
        (true, msg_autostart_disabled())
    }
}

// ── Splash run: poster, helper, mpv-IPC, relay, three paths ────────────────

/// Whether rotation needs a full scan — port of the short circuit in
/// `rotation_pool` (the scan is expensive: ffmpeg + VDF parsing, and on the critical path
/// of login it is not needed if there are enough favorites anyway).
pub fn rotation_needs_full_scan(existing_favorites: usize) -> bool {
    !rotation_uses_favorites(existing_favorites)
}

/// The mpv-IPC command «unpause».
pub fn unpause_command() -> Value {
    Value::Array(vec![
        Value::from("set_property"),
        Value::from("pause"),
        Value::from(false),
    ])
}

/// The mpv-IPC command «quit».
pub fn quit_command() -> Value {
    Value::Array(vec![Value::from("quit")])
}

/// The mpv property that identifies the end of the clip.
pub const MPV_EOF_PROPERTY: &str = "eof-reached";

/// The `%g` format for seconds in messages (6.0 → «6»).
fn fmt_g(v: f64) -> String {
    let s = format!("{v}");
    s
}

/// The X11 layer was not created — fall back to a managed mpv window.
pub const MSG_X11_OVERLAY_FALLBACK: &str =
    "x11 overlay не создан — откат на --fullscreen/--ontop (панель/попапы могут пробиться)";
/// The Rust helper did not take off — fall back to windowed mpv.
pub const MSG_HELPER_NO_LIFTOFF: &str =
    "rexon-splash-overlay не взлетел — фолбэк на mpv-окно";
/// Neither the bus nor the binaries gave the live KWin a reconfigure.
pub const MSG_KWIN_RECONFIGURE_FAILED: &str =
    "KWin reconfigure не прошёл: ни сессионная шина, ни бинари qdbus/gdbus не \
сработали — правило окна заставки KWin подхватит при следующем входе";

impl<IO: SplashIo> Splash<'_, IO> {
    /// PNG of the clip's first frame — port of `_poster_for_video`.
    pub fn poster_for_video(&mut self, video: &str) -> Option<String> {
        let home = self.home.clone();
        let theme_poster = theme_poster_path(&home);
        let exists = self.io.is_file(&theme_poster);
        let cfg = self.load_config();
        let rt = self.runtime_dir();
        match poster_source(&cfg, video, exists, &home, &rt) {
            PosterSource::Theme(p) => Some(p),
            PosterSource::Extract(out) => {
                let (ok, msg) = self.extract_poster(video, &out);
                if !ok {
                    self.io
                        .log("debug", &format!("_poster_for_video: {msg}"));
                    return None;
                }
                Some(out)
            }
        }
    }

    /// The built rexon-splash-overlay — port of `_find_splash_overlay_binary`
    /// (`repo_root` — realpath of the tree root, the executor's observation).
    pub fn find_splash_overlay_binary(&mut self, repo_root: &str) -> Option<String> {
        for c in splash_overlay_candidates(repo_root) {
            if self.io.is_file(&c) && self.io.is_executable(&c) {
                return Some(c);
            }
        }
        let w = self.io.which(SPLASH_OVERLAY_BINARY);
        if w.is_empty() {
            None
        } else {
            Some(w)
        }
    }

    /// An mpv-IPC command without a reply — port of `_mpv_ipc_send`.
    pub fn mpv_ipc_send(&mut self, sock: &str, command: &Value) -> bool {
        let payload = mpv_ipc_command_payload(command);
        self.io.ipc_send(sock, &payload)
    }

    /// An mpv property via IPC — port of `_mpv_ipc_get`.
    pub fn mpv_ipc_get(&mut self, sock: &str, prop: &str) -> Option<Value> {
        let payload = mpv_ipc_get_payload(prop);
        let received = self.io.ipc_request(sock, &payload, MPV_IPC_REPLY_MARKER)?;
        parse_mpv_ipc_response(&received, MPV_IPC_REQUEST_ID)
    }

    /// READY=1 to the systemd manager — port of `_sd_notify_ready`.
    pub fn sd_notify_ready(&mut self) {
        let ns = self.io.env("NOTIFY_SOCKET");
        let addr = notify_socket_addr(if ns.is_empty() { None } else { Some(&ns) });
        if let Some(a) = addr {
            self.io.notify(&a, SD_NOTIFY_READY);
        }
    }

    /// Whether ksplashqml is alive — port of `_ksplash_running`.
    fn ksplash_running(&mut self) -> bool {
        let pg = self.io.which("pgrep");
        if pg.is_empty() {
            return false;
        }
        let argv = pgrep_argv(&pg, KSPLASH_PROCESS);
        matches!(self.io.run(&argv, PGREP_TIMEOUT_SEC), Some(r) if r.rc == 0)
    }

    /// The KWin «above everything» rule — port of `_ensure_kwin_splash_rule`.
    pub fn ensure_kwin_splash_rule(&mut self) -> bool {
        let x = self.cfg_home();
        let path = kwinrulesrc_path(x.as_deref(), &self.home.clone());
        let txt = if self.io.is_file(&path) {
            match self.io.read_text(&path) {
                Some(t) => Some(t),
                None => {
                    self.io
                        .log("warning", "kwin-правило заставки: файл не читается");
                    return false;
                }
            }
        } else {
            None
        };
        let Some(new_text) = ensure_kwin_rule_text(txt.as_deref()) else {
            return true; // up to date, the file is left alone
        };
        if let Some(e) = self.io.write_text(&path, &new_text) {
            self.io
                .log("warning", &format!("kwin-правило заставки: {e}"));
            return false;
        }
        self.kwin_reconfigure();
        true
    }

    /// Poke reconfigure of the live KWin — port of `_kwin_reconfigure`.
    pub fn kwin_reconfigure(&mut self) {
        if self.io.kwin_reconfigure_dbus(DBUS_TIMEOUT_SEC) {
            // reconfigure is asynchronous, and rules are applied when a window is
            // created — let KWin reread them before mpv starts.
            self.io.sleep(KWIN_RECONFIGURE_SETTLE_SEC);
            return;
        }
        for cand in kwin_reconfigure_candidates() {
            if self.io.which(&cand[0]).is_empty() {
                continue;
            }
            if self.io.run(&cand, DBUS_TIMEOUT_SEC).is_none() {
                continue;
            }
            self.io.sleep(KWIN_RECONFIGURE_SETTLE_SEC);
            return;
        }
        self.io.log("warning", MSG_KWIN_RECONFIGURE_FAILED);
    }

    /// The hybrid relay: wait for ksplashqml to go away → unpause mpv.
    /// Port of `_hybrid_wait_and_unpause`: all waits are strictly bounded,
    /// unpause is done UNCONDITIONALLY at the end (mpv never stays
    /// paused, even if something above went wrong).
    pub fn hybrid_wait_and_unpause(&mut self, proc: i64, sock: &str) {
        // 1. mpv brought up its IPC socket (the first frame is about to be/already drawn under
        //    the ksplash overlay). The cap is strictly < the unit's TimeoutStartSec.
        let mut deadline = self.io.monotonic() + HYBRID_SOCK_WAIT_SEC;
        while self.io.monotonic() < deadline && self.io.poll(proc).is_none() {
            if self.io.exists(sock) {
                break;
            }
            self.io.sleep(SOCK_POLL_SEC);
        }
        // 2. Release plasmashell (Type=notify). UNCONDITIONALLY, even without
        //    a socket: otherwise TimeoutStartSec kills the unit together with mpv.
        self.sd_notify_ready();
        // 3. ksplashqml may have started slightly after us — wait for it to appear
        //    (the splash is turned off in the system → just move on).
        let mut seen = self.ksplash_running();
        if !seen {
            deadline = self.io.monotonic() + KSPLASH_APPEAR_WAIT_SEC;
            while self.io.monotonic() < deadline {
                if self.ksplash_running() {
                    seen = true;
                    break;
                }
                self.io.sleep(KSPLASH_POLL_SEC);
            }
        }
        // 4. Wait for it to die: Plasma's stage counter kills it once
        //    plasmashell is ready (the «desktop» stage).
        if seen {
            deadline = self.io.monotonic() + KSPLASH_DEATH_WAIT_SEC;
            while self.io.monotonic() < deadline {
                if !self.ksplash_running() {
                    break;
                }
                self.io.sleep(KSPLASH_POLL_SEC);
            }
        }
        // 5. The ksplash poster is gone → the same frame is in mpv under it → start the video.
        if self.io.poll(proc).is_none() {
            let cmd = unpause_command();
            self.mpv_ipc_send(sock, &cmd);
        }
    }

    /// Waiting for the end of the clip via mpv-IPC — the shared finale of the overlay paths
    /// (the Wayland helper and the X11 layer behave the same: the last frame
    /// is held by `keep-open=yes`, WE close it on EOF).
    fn wait_eof(&mut self, proc: i64, ipc_sock: &str, helper: bool) -> (bool, String) {
        let deadline = self.io.monotonic() + PLAY_TIMEOUT_SEC;
        let mut eof = false;
        while self.io.monotonic() < deadline && self.io.poll(proc).is_none() {
            if self.mpv_ipc_get(ipc_sock, MPV_EOF_PROPERTY) == Some(Value::Bool(true)) {
                eof = true;
                break;
            }
            self.io.sleep(EOF_POLL_SEC);
        }
        let rc = self.io.poll(proc);
        if eof || rc == Some(0) {
            return (true, msg_played());
        }
        match rc {
            Some(code) if helper => (false, helper_exit_message(code)),
            Some(code) => (false, mpv_exit_message(code)),
            None if helper => (false, msg_helper_timeout()),
            None => (false, msg_mpv_timeout()),
        }
    }

    /// Stopping the live player: `quit` via IPC → SIGTERM → SIGKILL.
    fn quit_player(&mut self, proc: i64, ipc_sock: &str) {
        if self.io.poll(proc).is_some() {
            return;
        }
        let cmd = quit_command();
        self.mpv_ipc_send(ipc_sock, &cmd);
        if self.io.wait(proc, QUIT_WAIT_SEC).is_none() {
            self.io.terminate(proc);
            if self.io.wait(proc, TERM_WAIT_SEC).is_none() {
                self.io.kill(proc);
            }
        }
    }

    /// Wait for a running compositor on a cold X11 login — port of
    /// `_X11SplashOverlay.wait_compositor` (the loop itself was in Python).
    fn wait_compositor(&mut self, overlay: i64) -> bool {
        let deadline = self.io.monotonic() + COMPOSITOR_WAIT_SEC;
        while self.io.monotonic() < deadline {
            if self.io.x11_compositor_present(overlay) {
                // let the compositor draw the first scene
                self.io.sleep(COMPOSITOR_SETTLE_SEC);
                return true;
            }
            self.io.sleep(COMPOSITOR_POLL_SEC);
        }
        false
    }

    /// A splash run with the Rust helper rexon-splash-overlay (Wayland) — port of
    /// `_play_wayland_overlay`. None — the helper did not take off (died before the socket),
    /// the caller falls back to windowed mpv.
    pub fn play_wayland_overlay(
        &mut self,
        helper: &str,
        path: &str,
        sound: bool,
        hybrid: bool,
        ipc_sock: &str,
    ) -> Option<(bool, String)> {
        let argv = splash_overlay_argv(helper, path, ipc_sock, sound, hybrid);
        let proc = match self.io.spawn(&argv, false) {
            Ok(p) => p,
            Err(e) => {
                self.io
                    .log("warning", &format!("rexon-splash-overlay не запустился: {e}"));
                return None;
            }
        };
        // Early diagnostics: the socket appears after mpv_initialize, i.e.
        // fatal environment failures (no Wayland/layer-shell/EGL) are already behind us.
        // A crash is caught separately and instantly, the deadline waits for «alive but not ready».
        let deadline = self.io.monotonic() + HELPER_SOCK_WAIT_SEC;
        let mut up = false;
        while self.io.monotonic() < deadline {
            if let Some(rc) = self.io.poll(proc) {
                self.io.log(
                    "warning",
                    &format!("rexon-splash-overlay умер на старте (код {rc})"),
                );
                return None;
            }
            if self.io.exists(ipc_sock) {
                up = true;
                break;
            }
            self.io.sleep(HELPER_POLL_SEC);
        }
        if !up && !self.io.exists(ipc_sock) {
            self.io.log(
                "warning",
                &format!(
                    "rexon-splash-overlay: IPC-сокет не поднялся за {} c",
                    fmt_g(HELPER_SOCK_WAIT_SEC)
                ),
            );
            self.io.terminate(proc);
            if self.io.wait(proc, TERM_WAIT_SEC).is_none() {
                self.io.kill(proc);
            }
            return None;
        }
        if hybrid {
            self.hybrid_wait_and_unpause(proc, ipc_sock);
        } else {
            self.sd_notify_ready(); // in case a notify unit started us after all
        }
        let res = self.wait_eof(proc, ipc_sock, true);
        // A clean exit: quit via IPC, the safety nets are SIGTERM (the helper catches it and
        // releases mpv/EGL correctly), then SIGKILL.
        self.quit_player(proc, ipc_sock);
        Some(res)
    }

    /// Play the splash as a fullscreen splash — port of `play`.
    ///
    /// `path`/`sound` default to the config; `blocking=true` — wait for the end
    /// (autostart/unit), `false` — return control right away (a test from the UI);
    /// `hybrid=None` — auto. `repo_root` — the tree root for finding the helper.
    pub fn play(
        &mut self,
        path: Option<&str>,
        sound: Option<bool>,
        blocking: bool,
        hybrid: Option<bool>,
        repo_root: &str,
    ) -> (bool, String) {
        let cfg = self.load_config();
        let path = match path {
            Some(p) => p.to_string(),
            None => get_or(&cfg, "path", Value::Null)
                .as_str()
                .unwrap_or("")
                .to_string(),
        };
        let sound = match sound {
            Some(s) => s,
            None => truthy(&get_or(&cfg, "sound", Value::from(true))),
        };
        let mut hybrid = effective_hybrid(
            hybrid,
            truthy(&get_or(&cfg, "hybrid_installed", Value::Null)),
            blocking,
        );
        if path.is_empty() || !self.io.is_file(&path) {
            return (false, msg_no_file());
        }
        // A real run (started from the systemd unit at login) — first
        // repair the setup, then play: a foreign theme in ksplashrc gives a second of the
        // STOCK Plasma splash before our clip (module header).
        // In NON-blocking mode (a test from the UI) there is no reason to touch ksplashrc.
        if blocking && hybrid {
            self.heal_hybrid(&path);
        }
        let display = self.io.env("DISPLAY");
        let wayland = self.io.env("WAYLAND_DISPLAY");
        let disp_opt = if display.is_empty() { None } else { Some(display.as_str()) };
        let wl_opt = if wayland.is_empty() { None } else { Some(wayland.as_str()) };
        // Wayland + a real run: the Rust helper is preferred (layer-shell
        // OVERLAY — above the KDE panels, its own libmpv inside). A test from the UI goes
        // through windowed mpv: the helper holds the last frame until we close it, and in
        // non-blocking mode there is nobody to close it.
        let helper = if wants_wayland_helper(blocking, disp_opt, wl_opt) {
            self.find_splash_overlay_binary(repo_root)
        } else {
            None
        };
        if helper.is_none() && self.io.which("mpv").is_empty() {
            return (false, msg_no_mpv());
        }
        if blocking {
            let rt = self.runtime_dir();
            let lock = play_lock_path(&rt);
            if !self.io.acquire_lock(&lock) {
                return (true, msg_already_playing());
            }
        }
        // X11 (real run only): our own override-redirect layer + --wid —
        // WM-independent, the panel/popups will not break through.
        let mut overlay: Option<i64> = None;
        if blocking && is_x11_session(disp_opt, wl_opt) {
            match self.io.x11_create() {
                None => self.io.log("warning", MSG_X11_OVERLAY_FALLBACK),
                Some(h) => {
                    overlay = Some(h);
                    // On X11 the ksplash→mpv relay is not used:
                    // ksplashqml goes away ~0.4 s after our READY, and
                    // the layer covers its window anyway. The poster as the layer background
                    // holds the screen during ~1.5 s of mpv initialization, and the video
                    // starts from the same frame without a black pause.
                    hybrid = false;
                    self.sd_notify_ready();
                    let poster = self.poster_for_video(&path);
                    // The poster BEFORE mapping: the background pixmap is drawn by the
                    // X server itself, it needs no compositor — a cold login
                    // goes without a black gap.
                    if let Some(p) = poster {
                        self.io.x11_set_poster(h, &p);
                    }
                    self.io.x11_map(h);
                    if !self.io.x11_compositor_present(h) {
                        // cold login: start the video after compositing
                        // is taken over
                        self.wait_compositor(h);
                    }
                }
            }
        }
        let rt = self.runtime_dir();
        let ipc_sock = ipc_sock_path(&rt);
        if let Some(h) = helper {
            if let Some(res) = self.play_wayland_overlay(&h, &path, sound, hybrid, &ipc_sock) {
                return res;
            }
            // The helper did not take off (died before the IPC socket) → windowed mpv below.
            self.io.log("warning", MSG_HELPER_NO_LIFTOFF);
            if self.io.which("mpv").is_empty() {
                return (false, msg_no_mpv());
            }
        }
        let cmd = match overlay {
            Some(h) => {
                let wid = self.io.x11_window_id(h);
                mpv_argv(
                    &path,
                    sound,
                    hybrid,
                    &MpvTarget::X11Overlay {
                        window_id: wid as u64,
                    },
                    &ipc_sock,
                )
            }
            None => {
                let c = mpv_argv(
                    &path,
                    sound,
                    hybrid,
                    &MpvTarget::Window {
                        wayland: !wayland.is_empty(),
                    },
                    &ipc_sock,
                );
                if !wayland.is_empty() {
                    // --ontop is dead on Wayland — staying on top (including above the Plasma
                    // panel) is ensured by the KWin rule by app_id.
                    self.ensure_kwin_splash_rule();
                }
                c
            }
        };
        if !blocking {
            return match self.io.spawn(&cmd, true) {
                Ok(_) => (true, msg_launched()),
                Err(e) => (false, msg_mpv_spawn_failed(&e)),
            };
        }
        let proc = match self.io.spawn(&cmd, false) {
            Ok(p) => p,
            Err(e) => {
                let msg = msg_mpv_spawn_failed(&e);
                if let Some(h) = overlay {
                    self.io.x11_raise_stop(h);
                    self.io.x11_close(h);
                }
                return (false, msg);
            }
        };
        if let Some(h) = overlay {
            self.io.x11_raise_start(h);
        }
        if hybrid {
            self.hybrid_wait_and_unpause(proc, &ipc_sock);
        } else {
            self.sd_notify_ready(); // in case a notify unit started us after all
        }
        let res = if overlay.is_some() {
            // The finale in the right order: mpv stays on the last frame
            // (--keep-open=yes), on EOF we close the layer FIRST — the mpv window
            // collapses together with it, and the poster does not flash.
            self.wait_eof(proc, &ipc_sock, false)
        } else {
            match self.io.wait(proc, PLAY_TIMEOUT_SEC) {
                None => {
                    self.io.terminate(proc); // safety net: do not block the session
                    if self.io.wait(proc, MPV_TERM_WAIT_SEC).is_none() {
                        self.io.kill(proc);
                    }
                    (false, msg_mpv_timeout())
                }
                Some(rc) => (
                    rc == 0,
                    if rc == 0 {
                        msg_played()
                    } else {
                        mpv_exit_message(rc)
                    },
                ),
            }
        };
        if let Some(h) = overlay {
            self.io.x11_raise_stop(h);
            self.io.x11_close(h);
            // the layer is already closed — stop mpv that stood on the last frame
            self.quit_player(proc, &ipc_sock);
        }
        res
    }

    /// The CLI point (`rexon --session-splash`) — port of `main`.
    ///
    /// The clip for the next login is chosen AFTER the display: the login has already been handed
    /// to the desktop, and ffmpeg for the poster delays nothing.
    pub fn main(&mut self, repo_root: &str) -> i64 {
        let (ok, msg) = self.play(None, None, true, None, repo_root);
        if !ok {
            // quietly: autostart must not get in the way of the session login
            self.io.log("warning", &format!("session-splash: {msg}"));
        }
        if let Some(nxt) = self.advance_rotation() {
            self.io.log("info", &format!("следующая заставка: {nxt}"));
        }
        0
    }
}
