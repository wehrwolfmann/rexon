//! Wallpaper entry-point decisions: what to launch and with which settings.
//!
//! Ported from `rexon/wallpaper.py`, the former SINGLE wallpaper entry point.
//! It is no longer in the tree: wallpapers of every type except video are
//! drawn by the waywallen engine, and the video host became an entry point of
//! its own. The Python host is gone too (removed 2026-08-23); the
//! `rexon-wallpaper-host` binary of the `rexon-gui` crate calls in here
//! instead. So what is left here is exactly what the video host calls:
//!
//!   * [`use_parent_dir`] — the path points to a file INSIDE the wallpaper directory;
//!   * [`mmap_size`] and the defaults [`DEFAULT_MMAP_WIDTH`]/[`DEFAULT_MMAP_HEIGHT`]
//!     — the mmap frame size from the environment, with a fallback on garbage;
//!   * [`parse_user_props_override`] — property overrides from the environment;
//!   * the video host's own decisions: which QML file to load
//!     ([`video_qml_file`]), what to put into the QML context and with which
//!     defaults ([`video_context_properties`]), and whether this host takes
//!     the wallpaper at all ([`video_host_reject_reason`]).
//!
//! Runner selection (`runner_id`/`runner_source`, the `REXON_WP_RUNNER`
//! variable) is gone: it served application wallpapers, and their engine and
//! runner package were removed — nobody reads the variable any more except
//! historical notes in `docs/`.
//!
//! Launching the renderer itself, reading `project.json` and working with the
//! file system stay with the runner.

use serde_json::{Map, Value};

/// Extensions for which a path is taken to point to a file INSIDE the
/// wallpaper directory rather than to the directory itself: old entry points
/// accepted `index.html`, `scene.json` and the like, and such calls must keep working.
pub const LEGACY_FILE_SUFFIXES: &[&str] =
    &[".html", ".htm", ".json", ".mp4", ".webm", ".exe"];

/// Default mmap frame width.
pub const DEFAULT_MMAP_WIDTH: i64 = 1920;
/// Default mmap frame height.
pub const DEFAULT_MMAP_HEIGHT: i64 = 1080;

/// Whether to go up to the parent directory: the path points to a file with an
/// "old" extension. The extension's case does not matter.
pub fn use_parent_dir(path: &str, is_file: bool) -> bool {
    if !is_file {
        return false;
    }
    let lower = path.to_lowercase();
    LEGACY_FILE_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

/// Result of parsing the user property override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserPropsOverride {
    /// No override — values are taken from `project.json`.
    None,
    /// Parsed properties in the canonical form `{key: {"value": …}}`.
    Props(Map<String, Value>),
    /// The value did not parse — the runner warns and falls back to the
    /// default values instead of crashing.
    Broken,
}

/// Parse a property override from the environment.
///
/// The configurator puts the saved JSON into an environment variable. The
/// format is normalized to the canonical `{key: {"value": …}}`: a bare value
/// gets wrapped, an already wrapped one stays as is. An empty string and `{}`
/// mean "no override".
///
/// Valid JSON that is not an object (a list, a string, `null`) is treated as
/// no override — the same as in the original.
pub fn parse_user_props_override(raw: &str) -> UserPropsOverride {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return UserPropsOverride::None;
    }
    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return UserPropsOverride::Broken,
    };
    let Value::Object(map) = parsed else {
        return UserPropsOverride::None;
    };
    let mut out = Map::new();
    for (key, value) in map {
        let wrapped = match value {
            Value::Object(inner) => Value::Object(inner),
            other => {
                let mut w = Map::new();
                w.insert("value".to_string(), other);
                Value::Object(w)
            }
        };
        out.insert(key, wrapped);
    }
    UserPropsOverride::Props(out)
}

/// The mmap frame size from the environment.
///
/// ⚠️ A deliberate divergence from the original (fixed during the port): there
/// a non-numeric variable value crashed the wallpaper launch with an unhandled
/// `ValueError` right at startup. Here garbage falls back to the default — a
/// broken environment variable must not leave the user without a wallpaper.
/// Surrounding whitespace is trimmed, as Python's `int()` did.
pub fn mmap_size(width: Option<&str>, height: Option<&str>) -> (i64, i64) {
    (
        parse_dimension(width, DEFAULT_MMAP_WIDTH),
        parse_dimension(height, DEFAULT_MMAP_HEIGHT),
    )
}

fn parse_dimension(value: Option<&str>, default: i64) -> i64 {
    match value.map(str::trim) {
        Some(v) if !v.is_empty() => v.parse().unwrap_or(default),
        _ => default,
    }
}

// ── video host: what to show and with which settings ───────────────────────
//
// Decisions taken over from the former Python host (`rexon/video_wallpaper.py`,
// removed from the tree 2026-08-23). Qt itself (QQmlApplicationEngine,
// LayerShellQt, QtMultimedia) and the compositor protocol live in the
// `rexon-gui` crate; here is what gets decided before the first Qt call:
// which display mode to choose, which properties to put into the QML context
// and with which values, and whether this host takes this wallpaper at all.

/// ⚠️ The three constants below are display-mode TOKENS, not file names on
/// disk: no files with these names have existed in the tree since the Python
/// host was removed. The Rust host draws with a single `WallpaperVideo.qml`
/// and translates the token into a `rexon-gui::wallpaper_host::placement` mode.
///
/// Offscreen mode: there is no window on screen, frames go to the shell
/// extension through mmap (GNOME/Cinnamon on Wayland — no layer-shell there).
pub const QML_OFFSCREEN: &str = "VideoWallpaperOffscreen.qml";
/// QML file of the Wayland mode: a window on the BACKGROUND layer via LayerShellQt.
pub const QML_WAYLAND: &str = "VideoWallpaper.qml";
/// QML file of the X11 mode: a regular window + `_NET_WM_WINDOW_TYPE_DESKTOP`.
pub const QML_X11: &str = "VideoWallpaperX11.qml";

/// The only wallpaper type this host draws. The rest are drawn by the
/// waywallen engine.
pub const VIDEO_HOST_TYPE: &str = "video";

/// Sound is on by default for WE wallpapers (`audioenabled`).
pub const DEFAULT_AUDIO_ENABLED: bool = true;
/// Default volume as a fraction of one (`volume`; WE stores 0..100).
pub const DEFAULT_VOLUME: f64 = 0.5;
/// Looping by default (`loopvideo`).
pub const DEFAULT_LOOP: bool = true;

/// Which QML file the video host loads — port of `VideoWallpaper.get_qml_file`.
///
/// The order of checks matters: offscreen mode overrides everything, because
/// with an mmap output set no window is shown on screen at all and there is
/// nothing there for LayerShellQt to serve.
pub fn video_qml_file(has_mmap_output: bool, wayland: bool) -> &'static str {
    if has_mmap_output {
        QML_OFFSCREEN
    } else if wayland {
        QML_WAYLAND
    } else {
        QML_X11
    }
}

/// A QML context property value in the form it has to be set.
#[derive(Debug, Clone, PartialEq)]
pub enum QmlValue {
    /// A local path the shim wraps in `QUrl.fromLocalFile`.
    LocalFileUrl(String),
    /// A ready URL (`http://`, `https://`, `file://`) — it must NOT be wrapped
    /// in `fromLocalFile`: that would produce «file:///https:/example.com».
    Url(String),
    Bool(bool),
    /// A floating-point value (QML `real`).
    Real(f64),
    Int(i64),
}

/// One QML context property: name and value.
#[derive(Debug, Clone, PartialEq)]
pub struct QmlContextProperty {
    pub name: String,
    pub value: QmlValue,
}

/// QML context properties for video wallpapers — a full port of
/// `VideoWallpaper.setup_renderer` (names, order, defaults and type
/// conversion).
///
/// `target_file` is the absolute path to the video file; `user_props` are the
/// properties from `project.json` in the `{key: {"value": …}}` form or plain
/// values (both are accepted: in the original `defaults_for_js` came here,
/// and bare values came from the configurator).
///
/// ⚠️ The name is `audioVolume`, NOT `volume`: a context property with the
/// same name as `AudioOutput.volume` sends QML into binding recursion.
pub fn video_context_properties(
    target_file: &str,
    windowed: bool,
    user_props: &Map<String, Value>,
    mmap_width: i64,
    mmap_height: i64,
) -> Vec<QmlContextProperty> {
    let prop = |key: &str| -> Option<&Value> {
        match user_props.get(key) {
            Some(Value::Object(obj)) => obj.get("value"),
            other => other,
        }
    };
    let named = |name: &str, value: QmlValue| QmlContextProperty {
        name: name.to_string(),
        value,
    };
    vec![
        named("videoUrl", QmlValue::LocalFileUrl(target_file.to_string())),
        named("isWindowed", QmlValue::Bool(windowed)),
        named(
            "audioEnabled",
            QmlValue::Bool(as_bool(prop("audioenabled"), DEFAULT_AUDIO_ENABLED)),
        ),
        named(
            "audioVolume",
            QmlValue::Real(as_volume01(prop("volume"), DEFAULT_VOLUME)),
        ),
        named(
            "loopVideo",
            QmlValue::Bool(as_bool(prop("loopvideo"), DEFAULT_LOOP)),
        ),
        // Frame size for the offscreen variant: there is no window on screen, so
        // there is nowhere to take Screen.width/height from in QML.
        named("renderWidth", QmlValue::Int(mmap_width)),
        named("renderHeight", QmlValue::Int(mmap_height)),
    ]
}

/// A WE property as a boolean — port of `_as_bool`.
///
/// WE stores flags as a real `true`, as a number, and as a string «true»/«1»/«on».
/// Anything unrecognized (including a missing property and `null`) gives the
/// default.
pub fn as_bool(value: Option<&Value>, default: bool) -> bool {
    match value {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(default),
        Some(Value::String(s)) => {
            matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on")
        }
        _ => default,
    }
}

/// WE volume as a fraction of one — port of `_as_float01`.
///
/// WE stores volume in 0..100, while `AudioOutput.volume` in QML expects 0..1:
/// a value greater than one is divided by a hundred. Anything unparsed gives
/// the default; the result is clamped to 0..1.
pub fn as_volume01(value: Option<&Value>, default: f64) -> f64 {
    let raw = match value {
        Some(Value::Number(n)) => n.as_f64(),
        // The original parsed a string with the same `float()` as a number.
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    };
    let Some(mut f) = raw.filter(|f| !f.is_nan()) else {
        return default;
    };
    if f > 1.0 {
        f /= 100.0;
    }
    f.clamp(0.0, 1.0)
}

/// Whether the video host takes this wallpaper type. `None` — it does; `Some(text)`
/// — a refusal with a ready explanation (port of the `_build_ctx_from_dir` message).
///
/// The refusal is honest and clear: an empty window instead of a wallpaper
/// explains nothing to a person (rule #13).
pub fn video_host_reject_reason(wp_type: &str, project_json: &str) -> Option<String> {
    if wp_type == VIDEO_HOST_TYPE {
        return None;
    }
    // Single quotes — the way the original's `repr()` printed them.
    let shown = if wp_type.is_empty() { "?" } else { wp_type };
    Some(format!(
        "Обои типа '{shown}' рисует движок waywallen, а не этот хост: {project_json}"
    ))
}

// ── web wallpapers ─────────────────────────────────────────────────────────
//
// Decisions of the wallpaper host's web mode (`rexon-wallpaper-host --mode web`,
// `rexon-gui` crate; until 15.09.2026 a separate `rexon-web-host` binary). The
// same split of duties as for video above: here is everything computed BEFORE
// the first Qt call — what to show, which properties to put into the QML
// root, which JS to inject into the page and whether this host takes the
// wallpaper.
//
// ⚠️ Why this is being written anew at all. The former host
// (`rexon/web_wallpaper.py`, 791 lines) was deleted on 30.07.2026 as
// "unneeded, waywallen draws it"; the parsing went away with it. Yet the
// wallpaper engine kept building the `python -m rexon.web_wallpaper` command
// line — meaning the "web" format has not been shown ANYWHERE, on any
// desktop, since that day, while the `qt6-webengine` dependency kept being
// installed for every customer.

/// The wallpaper type the web wallpaper host draws.
pub const WEB_HOST_TYPE: &str = "web";

/// The web wallpaper entry point inside the directory — as Wallpaper Engine names it.
pub const WEB_ENTRY_FILE: &str = "index.html";

/// Whether this is a URL (and not a file path).
///
/// One decision per product: the wallpaper engine asks the same question
/// (`wallpaper_engine::is_url`), and there must be no second copy of the rule.
pub fn is_web_url(path: &str) -> bool {
    crate::wallpaper_engine::is_url(path) || path.starts_with("file://")
}

/// QML root properties for web wallpapers — the same technique as for video.
///
/// `target` is the absolute path to the HTML or a ready URL (`http://`,
/// `https://`, `file://`): a URL has no local file, and it must not be wrapped
/// in `QUrl::fromLocalFile` — that would produce «file:///https:/example.com».
pub fn web_context_properties(
    target: &str,
    windowed: bool,
    mmap_width: i64,
    mmap_height: i64,
) -> Vec<QmlContextProperty> {
    let named = |name: &str, value: QmlValue| QmlContextProperty {
        name: name.to_string(),
        value,
    };
    vec![
        named(
            "targetUrl",
            if is_web_url(target) {
                QmlValue::Url(target.to_string())
            } else {
                QmlValue::LocalFileUrl(target.to_string())
            },
        ),
        named("isWindowed", QmlValue::Bool(windowed)),
        named("renderWidth", QmlValue::Int(mmap_width)),
        named("renderHeight", QmlValue::Int(mmap_height)),
    ]
}

/// Wallpaper Engine properties in the form the page's JS expects:
/// `{"name": {"value": …}}`. Port of the former host's `_load_web_defaults`
/// together with its rule.
///
/// ⚠️ The rule is non-obvious and important: a property WITHOUT a value (or
/// with an empty string) goes out as an empty object `{}` rather than being
/// skipped. For wallpapers with a file property (a picture the person did not
/// choose) a whole branch of their own code depends on this: on seeing `{}`,
/// the wallpaper loads its fallback file instead of staying empty.
pub fn web_defaults_for_js(user_props: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (name, raw) in user_props {
        // A plain value (from the configurator) is wrapped the same way as one
        // coming from project.json: past this point they cannot be told apart.
        let value = match raw {
            Value::Object(obj) => obj.get("value").cloned(),
            other => Some(other.clone()),
        };
        let keep = match &value {
            None | Some(Value::Null) => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(_) => true,
        };
        let mut entry = Map::new();
        if keep {
            entry.insert("value".to_string(), value.unwrap());
        }
        out.insert(name.clone(), Value::Object(entry));
    }
    out
}

/// The same properties as one JSON string — the form the JS accepts.
pub fn web_defaults_json(user_props: &Map<String, Value>) -> String {
    serde_json::to_string(&Value::Object(web_defaults_for_js(user_props)))
        .unwrap_or_else(|_| "{}".to_string())
}

/// Stubs of Wallpaper Engine global functions injected into the page.
///
/// Without them Workshop wallpapers break while the HTML is still being parsed:
/// they check `typeof window.wallpaperRegisterAudioListener === 'function'`
/// synchronously, and a missing function means for them not "there will be no
/// sound" but an abort of the whole script. That is why the script is injected
/// at the `DocumentCreation` point, before the page's own first script.
///
/// The stub does NOT throw the audio listener away, it keeps it: the
/// wallpaper redraws its canvas exactly from it, and a silent `noop` would
/// leave the first frame with fallback colors on screen instead of the
/// configured ones.
pub const WE_POLYFILL_JS: &str = r#"(function() {
    if (window.__rexonWePolyfillInstalled) return;
    window.__rexonWePolyfillInstalled = true;
    var noop = function() {};
    window._rexonAudioListeners = [];
    window.wallpaperRegisterAudioListener = function(cb) {
        if (typeof cb === 'function') window._rexonAudioListeners.push(cb);
    };
    window.wallpaperRegisterMediaStatusListener = noop;
    window.wallpaperRegisterMediaPlaybackListener = noop;
    window.wallpaperRegisterMediaPropertiesListener = noop;
    window.wallpaperRegisterMediaThumbnailListener = noop;
    window.wallpaperRegisterMediaTimelineListener = noop;
    window.wallpaperRequestRandomFileForProperty = function(_, cb) {
        if (cb) cb('', '');
    };
    if (!window.wallpaperPropertyListener) {
        window.wallpaperPropertyListener = {
            applyUserProperties: noop, applyGeneralProperties: noop };
    }
    if (!window.wallpaperPluginListener) {
        window.wallpaperPluginListener = { onPluginLoaded: noop };
    }
})();"#;

/// The first audio tick with an empty spectrum — after it the canvas is
/// redrawn with the person's settings.
///
/// ONE tick, not a stream: the host currently has nowhere to take a real
/// spectrum from (the former path pulled it from the Python side via stdin,
/// and it went away together with the Python host). One tick is the
/// difference between "the wallpaper drew with the configured colors" and
/// "the wallpaper froze on the fallback ones"; a stream without data would be
/// a deception.
pub const WE_TICKER_JS: &str = r#"(function() {
    if (window.__rexonInitialTickFired) return;
    window.__rexonInitialTickFired = true;
    if (!window._rexonAudioListeners) window._rexonAudioListeners = [];
    if (typeof window.wallpaperAudioListener === 'function' &&
        window._rexonAudioListeners.indexOf(window.wallpaperAudioListener) < 0) {
        window._rexonAudioListeners.push(window.wallpaperAudioListener);
    }
    setTimeout(function () {
        var z = new Float32Array(128);
        var L = window._rexonAudioListeners || [];
        for (var i = 0; i < L.length; i++) {
            try { L[i](z); } catch (e) { /* беда обоины — не беда хоста */ }
        }
    }, 50);
})();"#;

/// Script that passes the wallpaper properties into the page: `applyUserProperties`.
///
/// Wrapped in `try`, because the wallpaper may have no listener at all, and an
/// exception in the injected script would abort our audio tick right after it.
pub fn web_apply_defaults_js(defaults_json: &str) -> String {
    format!(
        "(function(d){{try{{if(window.wallpaperPropertyListener && \
         window.wallpaperPropertyListener.applyUserProperties)\
         window.wallpaperPropertyListener.applyUserProperties(d);\
         }}catch(e){{}}}})({defaults_json});"
    )
}

/// Pause and volume of web wallpapers from inside the page — a script at the
/// `DocumentCreation` point, before the wallpaper's own first script.
///
/// Why from inside. Video mode pauses the player, Flash pauses its own frame
/// loop; a web page has no "player", and Chromium does not allow freezing the
/// page from outside (`lifecycleState = Frozen`) while its window is visible —
/// and the wallpaper window is always visible. Hiding the window is not an
/// option either: during a battery pause the person looks at the desktop and
/// would see a black screen instead of the wallpaper. So the pause is done
/// the way the page itself understands it (the Page Visibility standard):
///
///   * `document.hidden` / `visibilityState` answer "hidden", and the page
///     receives `visibilitychange` — that is how the browser puts it to sleep
///     when the tab goes to the background, and most wallpapers already handle it;
///   * `requestAnimationFrame` callbacks are held back until resume — this is
///     the main engine of canvases and WebGL;
///   * CSS animations and transitions stop (`animation-play-state: paused`);
///   * `<video>`/`<audio>` are paused (and started again — only those that
///     were playing before the pause), `AudioContext` is suspended.
///
/// The frame stays on screen meanwhile — the last one drawn. Volume reaches
/// `<video>`/`<audio>` (Web Audio sound cannot be adjusted by volume — it can
/// only be muted, via the web view's `audioMuted` property).
///
/// Nothing is imposed on the page until the pause is on: without a call to
/// [`web_state_js`] the wrappers let everything through as is.
pub const WEB_PAUSE_JS: &str = r#"(function() {
    if (window.__rexonWallpaperState) return;
    var w = window, d = document;
    var paused = false, volume = -1;
    var raf = w.requestAnimationFrame ? w.requestAnimationFrame.bind(w) : null;
    var caf = w.cancelAnimationFrame ? w.cancelAnimationFrame.bind(w) : null;
    var held = {}, nextId = -1;
    if (raf) {
        w.requestAnimationFrame = function(cb) {
            if (!paused) return raf(cb);
            var id = nextId--;
            held[id] = cb;
            return id;
        };
        w.cancelAnimationFrame = function(id) {
            if (id < 0) { delete held[id]; return; }
            if (caf) caf(id);
        };
    }
    try {
        Object.defineProperty(d, 'hidden', { configurable: true,
            get: function() { return paused; } });
        Object.defineProperty(d, 'visibilityState', { configurable: true,
            get: function() { return paused ? 'hidden' : 'visible'; } });
    } catch (e) { /* не дали — остаются остальные способы */ }
    var contexts = [];
    ['AudioContext', 'webkitAudioContext'].forEach(function(name) {
        var C = w[name];
        if (typeof C !== 'function') return;
        var Wrapped = function() {
            var ctx = new (Function.prototype.bind.apply(C, [null].concat([].slice.call(arguments))))();
            contexts.push(ctx);
            if (paused) { try { ctx.suspend(); } catch (e) {} }
            return ctx;
        };
        Wrapped.prototype = C.prototype;
        w[name] = Wrapped;
    });
    var style = null;
    function media() { return d.querySelectorAll ? d.querySelectorAll('video,audio') : []; }
    function tune(m) {
        if (volume >= 0) { try { m.volume = volume; } catch (e) {} }
    }
    d.addEventListener('play', function(ev) {
        var m = ev.target;
        if (!m || !('paused' in m)) return;
        tune(m);
        if (paused) { m.__rexonWasPlaying = true; try { m.pause(); } catch (e) {} }
    }, true);
    w.__rexonWallpaperState = function(p, v) {
        if (typeof v === 'number' && v >= 0) {
            volume = Math.min(1, v);
            [].forEach.call(media(), tune);
        }
        p = !!p;
        if (p === paused) return;
        paused = p;
        [].forEach.call(media(), function(m) {
            if (p) {
                if (!m.paused) { m.__rexonWasPlaying = true; try { m.pause(); } catch (e) {} }
            } else if (m.__rexonWasPlaying) {
                m.__rexonWasPlaying = false;
                try { var r = m.play(); if (r && r.catch) r.catch(function() {}); } catch (e) {}
            }
        });
        contexts.forEach(function(ctx) {
            try { if (p) ctx.suspend(); else ctx.resume(); } catch (e) {}
        });
        if (p) {
            if (!style && d.documentElement) {
                style = d.createElement('style');
                style.textContent = '*,*::before,*::after{animation-play-state:paused!important;transition:none!important}';
                d.documentElement.appendChild(style);
            }
        } else if (style) {
            if (style.parentNode) style.parentNode.removeChild(style);
            style = null;
        }
        try { d.dispatchEvent(new Event('visibilitychange')); } catch (e) {}
        if (!p && raf) {
            var cbs = held; held = {};
            Object.keys(cbs).forEach(function(id) { raf(cbs[id]); });
        }
    };
})();"#;

/// The call that brings the sound model's decision to the page.
///
/// `paused` — whether to keep the page paused, `volume01` — volume as a
/// fraction of one for `<video>`/`<audio>`. Muting is not here: it is handled
/// by the web view property, which also covers Web Audio. A page without our
/// script (it did not get injected in time) simply skips the call.
pub fn web_state_js(paused: bool, volume01: f64) -> String {
    let v = if volume01.is_finite() { volume01.clamp(0.0, 1.0) } else { 1.0 };
    format!(
        "window.__rexonWallpaperState&&window.__rexonWallpaperState({paused},{v});"
    )
}

/// Chromium flags for offscreen mode.
///
/// `--in-process-gpu` and NO `--disable-gpu`: with the GPU disabled Chromium
/// does not composite the canvas (canvas2D/WebGL) into the Qt scene graph, and
/// a frame grabbed from the window would contain only the DOM — because of
/// this the former host had to drag along a workaround bridge via
/// `canvas.toDataURL` (PNG + base64 on every frame).
pub const WEB_OFFSCREEN_CHROMIUM_FLAGS: &str = "--in-process-gpu";

/// Whether the web host takes this wallpaper type. `None` — it does.
pub fn web_host_reject_reason(wp_type: &str, project_json: &str) -> Option<String> {
    if wp_type == WEB_HOST_TYPE {
        return None;
    }
    let shown = if wp_type.is_empty() { "?" } else { wp_type };
    Some(format!(
        "Обои типа '{shown}' этот хост не показывает — он рисует только веб: \
         {project_json}"
    ))
}
