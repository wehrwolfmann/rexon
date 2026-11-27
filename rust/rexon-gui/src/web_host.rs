// The web wallpaper host in Rust: a replacement for the removed
// `python -m rexon.web_wallpaper`.
//
// What this piece is and why it appeared again. On 30.07.2026 the hosts of
// web, app and parallax wallpapers (2 179 lines) were removed from the
// tree on the pretext that the waywallen engine draws all of that. For app
// and parallax that is true, for the web it is not: the wallpaper engine
// kept building the `python -m rexon.web_wallpaper` command line
// (`wallpaper_engine::web_plan`), the module behind it no longer existed,
// and from that day the "web" format was not shown on A SINGLE desktop.
// Meanwhile `qt6-webengine` kept being installed for every customer
// (`installer::dep_packages`) and was not used once in the code.
//
// The division of duties is exactly the same as for the video host:
//
//   * WHAT to show, which properties to put into the QML root and which JS
//     to inject into the page — `rexon-core::wallpaper`
//     (`web_context_properties`, `web_defaults_json`, `WE_POLYFILL_JS`,
//     `web_apply_defaults_js`, `WE_TICKER_JS`, `web_host_reject_reason`).
//     There are no strings or numbers of our own here;
//   * HOW to put the window into the background — three ways, and all three
//     are shared with the video host: `WallpaperLayer` (src/layer_shell.rs)
//     and `WallpaperPlacement` (src/wallpaper_host.rs). There is no second
//     decision about the layer, xprop or the frame stream in the project
//     (rule #12).
//
// Since 15.09.2026 this is a MODE of the single wallpaper host
// `rexon-wallpaper-host` (`--mode web`), not a separate `rexon-web-host`
// binary: it was decided not to multiply hosts. Bringing up Qt is shared —
// src/bin/wallpaper_host/main.rs; what to show —
// src/bin/wallpaper_host/web.rs.
//
// Warning: the only thing video does not have: `QtWebEngineQuick::initialize()`
// must run BEFORE `QGuiApplication` is created. cxx-qt-lib has no binding
// for it, and there is no C++ shim either.
//
// Why the function is taken with `dlopen`/`dlsym` rather than with a cxx
// bridge, as the separate web host did. The bridge gave a direct reference
// to the symbol, and the binary was linked with libQt6WebEngineQuick and
// through it with Chromium: the web host had 240 libraries among its
// dependencies versus 96 for video. In one binary with video this would
// mean loading Chromium on EVERY start of a video wallpaper. Measurement
// before the merge (offscreen platform, `--selftest=500`, 5 runs, median):
// the video host 956 ms and a 335.5 MiB peak, the same with QtWebEngineQuick
// force-loaded (`LD_PRELOAD`) — 997 ms and 347.2 MiB. So the library is
// opened here and only in web mode; the `QtWebEngine` QML import then finds
// it already loaded.
//
// There is still zero C++ of our own, and rule #16 is not broken:
// `void QtWebEngineQuick::initialize()` is a free function without
// arguments and without a return value; under the Itanium ABI calling it
// is no different from an `extern "C" fn()`. The mangled name is stable
// across all of Qt 6 (symbol version `Qt_6`), and if it turns out to be
// missing — a refusal with an explanation, not a loader crash with code
// 127.

use std::ffi::{c_char, c_int, c_void, CStr};

/// The QtWebEngine library for Qt Quick (the `qt6-webengine` package).
const WEB_ENGINE_QUICK_LIB: &CStr = c"libQt6WebEngineQuick.so.6";

/// Mangled name of `QtWebEngineQuick::initialize()`.
const INITIALIZE_SYMBOL: &CStr = c"_ZN16QtWebEngineQuick10initializeEv";

const RTLD_NOW: c_int = 0x2;
/// Global: the `QtWebEngine` QML plugin, loaded later, must see THE SAME
/// copy of the library that has already brought up Chromium.
const RTLD_GLOBAL: c_int = 0x100;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

fn dl_error() -> String {
    // SAFETY: dlerror returns either NULL or a string that lives until the
    // next dl* call in this thread; we copy it at once.
    let raw = unsafe { dlerror() };
    if raw.is_null() {
        // TRANSLATORS: stands in for the system loader's error text when it gives none.
        tr("no reason given")
    } else {
        unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned()
    }
}

/// Open QtWebEngineQuick and find `initialize` in it WITHOUT calling it.
fn resolve_initialize() -> Result<extern "C" fn(), String> {
    let lib = WEB_ENGINE_QUICK_LIB.to_string_lossy();
    // SAFETY: the library name is a NUL-terminated string; the handle is
    // deliberately not closed: the process needs the library until exit.
    let handle = unsafe { dlopen(WEB_ENGINE_QUICK_LIB.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
    if handle.is_null() {
        return Err(fill_plain(
            // TRANSLATORS: printed to the terminal. %1 — the loader's error,
            // %2 — the library file name.
            &tr("QtWebEngine failed to load (%1): web wallpapers need %2 from the qt6-webengine package"),
            &[&dl_error(), &lib],
        ));
    }
    // SAFETY: the handle was obtained above, the symbol name is a
    // NUL-terminated string.
    let symbol = unsafe { dlsym(handle, INITIALIZE_SYMBOL.as_ptr()) };
    if symbol.is_null() {
        return Err(fill_plain(
            // TRANSLATORS: printed to the terminal. %1 — the library file name,
            // %2 — the loader's error.
            &tr("%1 has no QtWebEngineQuick::initialize (%2)"),
            &[&lib, &dl_error()],
        ));
    }
    // SAFETY: the symbol is `void QtWebEngineQuick::initialize()`, a free
    // function without arguments or return value (see the analysis at the
    // start of the file).
    Ok(unsafe { std::mem::transmute::<*mut c_void, extern "C" fn()>(symbol) })
}

/// Bring up QtWebEngine. Call ONLY in web mode and BEFORE `QGuiApplication`:
/// the subsystem sets up a shared OpenGL context for all windows of the
/// process, and after the application is created it is too late.
pub fn init_web_engine() -> Result<(), String> {
    let initialize = resolve_initialize()?;
    initialize();
    Ok(())
}

// ── Argument parsing ───────────────────────────────────────────────────────

/// Command line options of the host.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cli {
    /// A link (`http://`, `https://`, `file://`), a path to HTML or a wallpaper
    /// directory with `project.json`.
    pub target: String,
    pub windowed: bool,
    pub verbose: bool,
    /// Full-screen preview on top of everything; any key or click closes it.
    pub preview: bool,
    /// Self-test: after how many milliseconds the window reports and closes.
    pub selftest_ms: i64,
}

/// Default self-test pause. Longer than for video (3 s): Chromium comes up
/// more slowly than the player, and on a cold start three seconds are not
/// enough even for an empty page — the report would say "did not load"
/// where it did load.
pub const DEFAULT_SELFTEST_MS: i64 = 6000;

/// Parsing of web mode arguments (the `--mode` option has already been
/// removed by [`crate::wallpaper_host::take_mode`]). An error is a ready
/// string for the user; `--help` answers with the host's shared help.
pub fn parse_args(argv: &[String]) -> Result<Cli, String> {
    let mut cli = Cli::default();
    for arg in argv {
        match arg.as_str() {
            "-w" | "--windowed" => cli.windowed = true,
            // A full-screen look at a page (the Steam screensavers in the «Steam»
            // tab): not a wallpaper, so it takes no host lock, like --windowed.
            "--preview" => {
                cli.windowed = true;
                cli.preview = true;
            }
            "-v" | "--verbose" => cli.verbose = true,
            "-h" | "--help" => return Err(USAGE.to_string()),
            "--audio" | "--no-audio" | "--volume" => {
                // The sound of web wallpapers is driven by the page itself; silently
                // swallowing the option would pretend it turned something on.
                // TRANSLATORS: a command line error; %1 — the option, "video" is a mode name.
                return Err(fill_plain(&tr("the %1 option is only available in video mode"), &[arg]));
            }
            "--selftest" => cli.selftest_ms = DEFAULT_SELFTEST_MS,
            other if other.starts_with("--selftest=") => {
                cli.selftest_ms = other["--selftest=".len()..]
                    .parse()
                    .map_err(|_| tr("--selftest= needs a number of milliseconds"))?;
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(fill_plain(&tr("unknown option: %1"), &[&other]));
            }
            other => {
                if !cli.target.is_empty() {
                    // TRANSLATORS: a command line error: a second address after the first one.
                    return Err(fill_plain(&tr("extra address: %1"), &[&other]));
                }
                cli.target = other.to_string();
            }
        }
    }
    if cli.target.is_empty() {
        return Err(tr("no link, HTML file or wallpaper directory given"));
    }
    Ok(cli)
}

use crate::wallpaper_host::USAGE;
use rexon_core::i18n::{fill_plain, tr};

#[cfg(test)]
mod tests {
    use super::*;

    /// The library and the symbol that web mode opens are in place — without
    /// calling: there is no reason to bring up Chromium in the test process.
    /// Goes red if Qt changes the library name or the function's name mangling.
    #[test]
    fn qtwebengine_initialize_находится_без_линковки() {
        assert!(resolve_initialize().is_ok(), "{:?}", resolve_initialize().err());
    }

    #[test]
    fn ключи_звука_только_у_видео() {
        let err = parse_args(&["/a.html".into(), "--no-audio".into()]).unwrap_err();
        assert!(err.contains("video"), "{err}");
    }

    #[test]
    fn адрес_обязателен_и_единственный() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["/a.html".into(), "/b.html".into()]).is_err());
        assert!(parse_args(&["--nosuchoption".into(), "/a.html".into()]).is_err());
    }

    #[test]
    fn ссылка_принимается_как_есть() {
        let cli = parse_args(&["https://example.com".to_string()]).unwrap();
        assert_eq!(cli.target, "https://example.com");
        assert!(!cli.windowed);
        assert_eq!(cli.selftest_ms, 0);
    }

    #[test]
    fn самопроверка_с_умолчанием_и_с_числом() {
        assert_eq!(
            parse_args(&["/a.html".into(), "--selftest".into()])
                .unwrap()
                .selftest_ms,
            DEFAULT_SELFTEST_MS
        );
        assert_eq!(
            parse_args(&["/a.html".into(), "--selftest=900".into()])
                .unwrap()
                .selftest_ms,
            900
        );
        assert!(parse_args(&["/a.html".into(), "--selftest=soon".into()]).is_err());
    }

    #[test]
    fn preview_is_a_window_not_a_wallpaper() {
        let cli = parse_args(&["--preview".into(), "/a.html".into()]).unwrap();
        assert!(cli.preview && cli.windowed);
        assert!(!parse_args(&["/a.html".into()]).unwrap().preview);
    }

    #[test]
    fn справка_не_ошибка_разбора() {
        assert_eq!(parse_args(&["--help".to_string()]), Err(USAGE.to_string()));
    }
}
