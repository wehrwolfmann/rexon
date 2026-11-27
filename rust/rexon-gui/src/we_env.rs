// Process environment for the WE tab queries: the home directory, XDG, the
// numeric user id and the directory of the `rexon/` package.
//
// Not a bridge: plain functions used by the wrappers. Moved here so that
// the package directory is taken from ONE place (rule #12) — otherwise
// every query would invent its own guess about where `rexon/` lies.

use core::pin::Pin;
use std::ffi::{c_char, CString};
use std::sync::{Mutex, OnceLock};

/// Home directory. An empty string is acceptable too: the core then builds
/// the path from the root, and the file simply will not be written — Python
/// behaved the same way.
pub fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `$XDG_CONFIG_HOME`, if it is set and non-empty.
pub fn xdg_config_home() -> Option<String> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Numeric user id.
///
/// Without libc: the owner of `/proc/self` is the current uid. The core asks
/// for it only for the `/run/user/<uid>/systemd` path.
pub fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0)
}

/// Live file system checks for `we_client::find_daemon_binary`.
///
/// One per crate: the daemon binary is looked for both by the autostart page
/// and by the transport, and two copies of these two methods would silently
/// drift apart (rule #12). The order and the set of candidates are decided
/// by the core; here there is only `stat` and walking `$PATH`.
pub struct FsProbe;

impl rexon_core::we_client::DaemonFs for FsProbe {
    fn is_exec_file(&mut self, path: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    fn which(&mut self, name: &str) -> Option<String> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if self.is_exec_file(&candidate.to_string_lossy()) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
        None
    }

    /// `<engine> --version`, memoized by (path, size, modification time).
    ///
    /// The core asks for the version only when BOTH engines are on disk — the
    /// packaged one and the managed one. Starting an AppImage costs half a
    /// second, while the binary search is called on every tab opening and on
    /// every autostart tick, so the answer is remembered until the file is
    /// replaced.
    fn binary_version(&mut self, path: &str) -> Option<String> {
        use std::collections::HashMap;
        use std::time::{Duration, Instant};
        type Key = (String, u64, Option<std::time::SystemTime>);
        static CACHE: OnceLock<Mutex<HashMap<Key, Option<String>>>> = OnceLock::new();

        let meta = std::fs::metadata(path).ok()?;
        let key: Key = (path.to_string(), meta.len(), meta.modified().ok());
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(hit) = cache.lock().ok()?.get(&key) {
            return hit.clone();
        }

        let argv = rexon_core::engine_update::binary_version_argv(path);
        let mut child = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        let limit = Duration::from_secs(rexon_core::engine_update::BINARY_VERSION_TIMEOUT_SECS);
        let started = Instant::now();
        let version = loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Different builds print the version to stdout or to stderr.
                    use std::io::Read;
                    let mut out = String::new();
                    if let Some(mut stdout) = child.stdout.take() {
                        let _ = stdout.read_to_string(&mut out);
                    }
                    out.push('\n');
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = stderr.read_to_string(&mut out);
                    }
                    let v = rexon_core::engine_update::parse_binary_version(&out);
                    break (!v.is_empty()).then_some(v);
                }
                Ok(None) if started.elapsed() < limit => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                // Hung or not waited for — this is OUR child process by its id, not a
                // lookup by name.
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        if let Ok(mut map) = cache.lock() {
            map.insert(key, version.clone());
        }
        version
    }
}

/// Path to the wallpaper daemon binary (or `None`). One door for the whole
/// crate.
pub fn daemon_binary() -> Option<String> {
    rexon_core::we_client::find_daemon_binary(&mut FsProbe, &pkg_dir(), &home())
}

/// Engine layouts from packages (`we_client::engine_layouts`) that are
/// really installed on this machine: `plugins/` or the daemon itself is
/// there.
pub fn present_engine_layouts() -> Vec<rexon_core::we_client::EngineLayout> {
    use rexon_core::we_client::DaemonFs;
    rexon_core::we_client::engine_layouts()
        .into_iter()
        .filter(|l| std::path::Path::new(&l.plugins_dir()).is_dir() || FsProbe.is_exec_file(&l.daemon))
        .collect()
}

/// The packaged engine the live daemon is compared with: the layout of the
/// daemon being started, and if a foreign one is started (the user's
/// managed AppImage) — the first one present on disk. `None` means there is
/// no packaged engine (a developer tree, a manual installation without
/// packages).
pub fn package_engine() -> Option<rexon_core::we_client::EngineLayout> {
    let present = present_engine_layouts();
    let binary = daemon_binary();
    binary
        .as_deref()
        .and_then(|b| rexon_core::we_client::layout_of(&present, b).cloned())
        .or_else(|| present.into_iter().next())
}

/// The directory the daemon gets with the `--plugin` flag
/// (`we_client::engine_launch_args`) for starting binary `binary`. Decided
/// by the core (`we_client::plugin_root_for`): the flag is needed only if
/// the daemon does not see the package's plugin directory by itself —
/// `waywallen` from the distribution package sees it, `waywallen-bin` and a
/// managed AppImage do not.
pub fn engine_plugin_root_for(binary: Option<&str>) -> Option<String> {
    let with_plugins: Vec<_> = rexon_core::we_client::engine_layouts()
        .into_iter()
        .filter(|l| std::path::Path::new(&l.plugins_dir()).is_dir())
        .collect();
    let resolved = binary
        .and_then(|b| std::fs::canonicalize(b).ok())
        .map(|p| p.to_string_lossy().into_owned());
    rexon_core::we_client::plugin_root_for(&with_plugins, resolved.as_deref().or(binary))
}

/// [`engine_plugin_root_for`] for the daemon Rexon will start.
pub fn engine_plugin_root() -> Option<String> {
    engine_plugin_root_for(daemon_binary().as_deref())
}

/// Plugins of the packaged engine: `id`/`name`/`version` of each
/// `<engine dir>/plugins/*/plugin.toml`. Empty means there is no package.
pub fn bundled_plugins() -> Vec<rexon_core::we_client::PluginCopy> {
    let Some(layout) = package_engine() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(layout.plugins_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("plugin.toml")).ok())
        .filter_map(|text| rexon_core::we_client::plugin_manifest_copy(&text))
        .collect()
}

/// Whether Rexon is installed as a distribution package: the package
/// launcher set `REXON_PACKAGED`, or the binary itself lies where only the
/// package manager puts things (a launcher may call the window directly,
/// bypassing the package launcher). The decisions are the core's
/// (`installer::PACKAGED_ENV`, `installer::packaged_location`).
pub fn rexon_packaged() -> bool {
    if std::env::var_os(rexon_core::installer::PACKAGED_ENV).is_some() {
        return true;
    }
    std::fs::read_link("/proc/self/exe")
        .map(|p| rexon_core::installer::packaged_location(&p.to_string_lossy()))
        .unwrap_or(false)
}

fn pkg_dir_slot() -> &'static Mutex<String> {
    static SLOT: OnceLock<Mutex<String>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(String::new()))
}

/// Directory of the `rexon/` package — from it the core builds the path to
/// the bundled engine binary (`rexon/vendor/`).
///
/// Empty until the tab has been prepared: the value is put in by
/// [`discover_pkg_dir`] on the first process preparation
/// (`prepare_process`), deriving it from the path of its own binary.
pub fn pkg_dir() -> String {
    pkg_dir_slot()
        .lock()
        .expect("каталог пакета не отравлен")
        .clone()
}

// ============================================================================
// Built QML plugins of the tab
// ============================================================================

/// Live file system checks for [`rexon_core::qml_modules`].
///
/// Not a copy of the decision: the order and the set of directories are held
/// by the core; here there is only `is_file`.
struct ModuleProbe;

impl rexon_core::qml_modules::ModuleFs for ModuleProbe {
    fn is_file(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    /// The `.note.qt.metadata` note lies at the very start of the plugin file
    /// (the first few hundred bytes), so the head of the file is enough, not the
    /// whole of it.
    fn plugin_qt_version(&mut self, path: &str) -> Option<(u32, u32)> {
        use std::io::Read;
        let mut head = vec![0u8; 64 * 1024];
        let mut file = std::fs::File::open(path).ok()?;
        let n = file.read(&mut head).ok()?;
        rexon_core::qml_modules::plugin_qt_version(&head[..n])
    }

    fn running_qt_version(&mut self) -> Option<(u32, u32)> {
        running_qt_version()
    }

    /// The module's build stamp is one line; no more than a kilobyte is read.
    fn read_text(&mut self, path: &str) -> Option<String> {
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(path)
            .ok()?
            .take(1024)
            .read_to_string(&mut text)
            .ok()?;
        Some(text)
    }
}

unsafe extern "C" {
    /// `qVersion()` from QtCore — the Qt version the process RUNS with (not the
    /// one it was built with). Declared in Qt with C linkage.
    fn qVersion() -> *const c_char;
}

/// Version of the running Qt (major, minor).
pub fn running_qt_version() -> Option<(u32, u32)> {
    let raw = unsafe { qVersion() };
    if raw.is_null() {
        return None;
    }
    let text = unsafe { std::ffi::CStr::from_ptr(raw) }.to_str().ok()?;
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// `Qcm.Material` was found but built for a different minor Qt version:
/// `(build version, running Qt version)`. Such a build does not load (see
/// `rexon_core::qml_modules`), and the tab placeholder explains why.
pub fn qml_modules_wrong_qt() -> Option<((u32, u32), (u32, u32))> {
    let found = rexon_core::qml_modules::incompatible_qcm(&mut ModuleProbe, &qml_module_dirs())?;
    let (path, built, running) = found;
    rexon_core::log_warn!(
        "wallpaper",
        "Qcm.Material собран под Qt {}.{}, а запущен Qt {}.{} — сборка пропущена: {path}",
        built.0,
        built.1,
        running.0,
        running.1
    );
    Some((built, running))
}

/// `$XDG_DATA_HOME`, if it is set and non-empty.
pub fn xdg_data_home() -> Option<String> {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// `$XDG_DATA_DIRS` as the raw value (colon-separated).
pub fn xdg_data_dirs() -> String {
    std::env::var("XDG_DATA_DIRS").unwrap_or_default()
}

/// Expand `qml_modules::MULTIARCH_GLOB` by walking `/usr/lib`.
///
/// The same answer as `sorted(glob.glob(...))` in Python and as the
/// neighbouring executor in `rexon-py`; the pattern is taken from the core
/// here too.
fn multiarch_dirs() -> Vec<String> {
    let Ok(rd) = std::fs::read_dir("/usr/lib") else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|name| format!("/usr/lib/{name}/rexon/qml_modules"))
        .filter(|p| std::path::Path::new(p).is_dir())
        .collect();
    out.sort();
    out
}

/// All directories where built QML plugins may lie, in decreasing order of
/// preference. The set and the order are the core's: the user build
/// (`tools/build_qml_*.sh`) wins only when run from the source tree, while
/// for a package the packaged copy wins (`qml_modules::order_module_dirs`).
pub fn qml_module_dirs() -> Vec<String> {
    use rexon_core::qml_modules as qm;
    let xdg = xdg_data_home();
    let home = home();
    let all = qm::module_dirs(xdg.as_deref(), &home, &xdg_data_dirs(), &multiarch_dirs());
    let user = qm::user_module_dirs(xdg.as_deref(), &home);
    let user_first =
        qm::user_modules_first(rexon_packaged(), !rexon_core::cli::repo_root().is_empty());
    let order = qm::order_module_dirs(&mut ModuleProbe, &all, &user, user_first);
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        for note in &order.notes {
            match qm::user_copy_note_line(note) {
                Some((true, line)) => rexon_core::log_warn!("wallpaper", "{line}"),
                Some((false, line)) => rexon_core::log_info!("wallpaper", "{line}"),
                None => {}
            }
        }
    });
    order.dirs
}

// ============================================================================
// Tab host: preparing the process and the QML engine
// ============================================================================
//
// Preparing the wallpaper tab for the Rust window (`bin/main_window.rs`).
// There are no decisions here: the set and order of directories, file
// names, build candidates and the cache are the core's (`qml_modules`,
// `remote_l10n`).

/// Find the directory of the `rexon/` package if nobody has named it yet.
///
/// The root is derived from the path of our own binary — with the same core
/// function the window uses to look for the splash (`cli::repo_root`).
/// The package is accepted only if it really contains the vendored
/// interface: a foreign `rexon` directory nearby must not pass for it.
fn discover_pkg_dir() -> String {
    let known = pkg_dir();
    if !known.is_empty() {
        return known;
    }
    // There are three layouts — a developer tree, a distribution package and
    // an `installer.py` installation; their set and order are held by the
    // core, here there is only a live file probe (rule #12). Without this the
    // window from a package opened the wallpaper tab without the vendored
    // pages: there is no `rust/Cargo.toml` tree next to it.
    let root = rexon_core::cli::repo_root();
    let Some(candidate) = rexon_core::installer::pick_package_data_dir(&root, &|path: &str| {
        std::path::Path::new(path).is_file()
    }) else {
        return String::new();
    };
    *pkg_dir_slot().lock().expect("каталог пакета не отравлен") = candidate.clone();
    candidate
}

/// Directory of the vendored waywallen interface (`qml_ui/` of the
/// package); empty means there is no package, and the page opens without
/// the vendored pages.
pub fn vendor_qml_dir() -> String {
    let pkg = pkg_dir();
    if pkg.is_empty() {
        return String::new();
    }
    let dir = rexon_core::remote_l10n::ui_module_dir(&pkg);
    if std::path::Path::new(&dir).is_dir() {
        dir
    } else {
        String::new()
    }
}

/// Tree root for build candidates: the parent of the `rexon/` package, by
/// the real path (the symlink of the installed copy leads into the working
/// tree).
fn repo_root_of_pkg() -> String {
    let pkg = pkg_dir();
    std::fs::canonicalize(&pkg)
        .ok()
        .and_then(|p| p.parent().map(|r| r.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Say in the log (once per process) which module directory is skipped
/// because of a `waywallen.control` built from another protocol. The
/// decision itself is the core's (`qml_modules::stale_control_dirs`).
fn log_stale_control_once(dirs: &[String]) {
    use rexon_core::qml_modules::{self as qm, ControlBuild};
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let stale = qm::stale_control_dirs(&mut ModuleProbe, dirs);
        if stale.is_empty() {
            return;
        }
        let fresh = qm::fresh_control_dir(&mut ModuleProbe, dirs).unwrap_or_default();
        for (dir, state) in stale {
            let why = match state {
                ControlBuild::Unmarked => "отметки протокола нет (сборка старого образца)",
                _ => "собран из другого протокола",
            };
            rexon_core::log_warn!(
                "wallpaper",
                "каталог модулей пропущен: {dir} — waywallen.control в нём устарел: {why}; \
                 берётся {fresh}"
            );
        }
    });
}

/// Directories for `addImportPath` in order of preference: where something
/// really lies, and if nowhere — the directory where `Qcm.Material` was
/// looked for (as in Python).
fn engine_import_dirs() -> Vec<String> {
    let dirs = qml_module_dirs();
    log_stale_control_once(&dirs);
    let found = rexon_core::qml_modules::import_dirs(&mut ModuleProbe, &dirs);
    if !found.is_empty() {
        return found;
    }
    let first = rexon_core::qml_modules::resolve_import_dir(&mut ModuleProbe, &dirs);
    if first.is_empty() {
        Vec::new()
    } else {
        vec![first]
    }
}

/// Which `QExtra` module the engine gets: the author's real plugin or our
/// QML replacement from the vendored markup directory. The decision is the
/// core's (`qml_modules::choose_qextra`); here there are live probes and one
/// log line per process.
fn qextra_choice() -> &'static rexon_core::qml_modules::QExtraChoice {
    use rexon_core::qml_modules as qm;
    static CHOICE: OnceLock<qm::QExtraChoice> = OnceLock::new();
    CHOICE.get_or_init(|| {
        let args: Vec<String> = std::env::args_os()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let platform = qm::requested_qpa_platform(
            &args,
            std::env::var("QT_QPA_PLATFORM").ok().as_deref(),
        );
        let rhi = qm::scene_graph_uses_rhi(
            platform.as_deref(),
            std::env::var("QT_QUICK_BACKEND").ok().as_deref(),
        );
        let choice = qm::choose_qextra(&mut ModuleProbe, &qml_module_dirs(), rhi);
        rexon_core::log_info!("wallpaper", "{}", qm::qextra_choice_line(&choice));
        choice
    })
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: i32) -> *mut std::ffi::c_void;
    fn dlsym(handle: *mut std::ffi::c_void, symbol: *const c_char) -> *mut std::ffi::c_void;
}
const RTLD_NOW: i32 = 2;
const RTLD_GLOBAL: i32 = 0x100;

/// Load the Qt Protobuf value types of `waywallen.control` into the process
/// without a QML engine: the type library is taken from a fresh module
/// directory (the same one handed to the engine). Needed by the
/// "Wallpapers" page bench — it runs the gadget shim on REAL types, not on a
/// fake. `Ok` is the library path.
pub fn load_control_types() -> Result<String, String> {
    use rexon_core::qml_modules as qm;
    let dirs = qml_module_dirs();
    let dir = qm::fresh_control_dir(&mut ModuleProbe, &dirs)
        .ok_or_else(|| format!("свежего waywallen.control нет ни в одном каталоге: {dirs:?}"))?;
    let so = qm::plugin_path(&dir, qm::WAYWALLEN_CONTROL_TYPES_REL);
    let path = CString::new(so.as_str()).map_err(|_| format!("путь с нулём: {so}"))?;
    if unsafe { dlopen(path.as_ptr(), RTLD_NOW | RTLD_GLOBAL) }.is_null() {
        return Err(format!("библиотека типов не загрузилась: {so}"));
    }
    Ok(so)
}

/// Register the filter editor model in `waywallen.ui`.
///
/// The model is a separate crate (`rexon-qt-filter`, `cdylib` only: Qt
/// Protobuf gadgets are marshalled only in C++/Rust), so its build is
/// loaded from disk, the first existing core candidate. `false` means none
/// was found: the filter editor will be empty, but the page will not crash.
pub fn register_filter_model() -> bool {
    static DONE: OnceLock<bool> = OnceLock::new();
    *DONE.get_or_init(|| {
        let root = repo_root_of_pkg();
        // Directories — already without stale ones: the filter model writes Qt
        // Protobuf gadgets and must come from the same build as
        // `waywallen.control`.
        let dirs = rexon_core::qml_modules::usable_dirs(&mut ModuleProbe, &qml_module_dirs());
        for so in rexon_core::qml_modules::filter_lib_candidates(&root, &dirs) {
            if !std::path::Path::new(&so).is_file() {
                continue;
            }
            let Ok(path) = CString::new(so.as_str()) else { continue };
            let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
            if handle.is_null() {
                rexon_core::log_warn!("wallpaper", "модель фильтров не загрузилась: {so}");
                continue;
            }
            let sym = unsafe { dlsym(handle, c"rexon_register_filter_model".as_ptr()) };
            if sym.is_null() {
                rexon_core::log_warn!("wallpaper", "в сборке нет модели фильтров: {so}");
                continue;
            }
            let register: extern "C" fn() = unsafe { std::mem::transmute(sym) };
            register();
            return true;
        }
        rexon_core::log_warn!("wallpaper", "сборка rexon-qt-filter не найдена — редактор фильтров обоев будет пуст");
        false
    })
}

thread_local! {
    /// The warm-up engine lives until the end of the process — as in Python.
    static PREWARM: std::cell::RefCell<Option<cxx::UniquePtr<cxx_qt_lib::QQmlApplicationEngine>>> =
        const { std::cell::RefCell::new(None) };
}

/// Warm up Qt Protobuf registration on the MAIN thread.
///
/// The vendored pages load asynchronously, that is, they are compiled in
/// `QQmlThread`; the first encounter with `waywallen.control` there
/// deadlocks on the global registration of protobuf types. So the module is
/// imported once here, synchronously, with a separate engine. `false` means
/// there is nothing to warm up (no package or plugins).
pub fn prewarm() -> bool {
    if PREWARM.with(|p| p.borrow().is_some()) {
        return true;
    }
    let vendor = vendor_qml_dir();
    if vendor.is_empty() || !qml_plugins_ready() {
        return false;
    }
    let mut engine = cxx_qt_lib::QQmlApplicationEngine::new();
    let Some(mut eng) = engine.as_mut() else { return false };
    for dir in engine_import_dirs().iter().rev() {
        eng.as_mut().add_import_path(&cxx_qt_lib::QString::from(dir.as_str()));
    }
    let file = rexon_core::qml_modules::qml_path(&vendor, rexon_core::qml_modules::PREWARM_QML);
    eng.as_mut().load(&crate::we_page::local_file_url(&file));
    PREWARM.with(|p| *p.borrow_mut() = Some(engine));
    true
}

/// Process preparation for the Rust window: everything the Python host does
/// before the first QML load. Call from the main thread with a live
/// application.
pub fn prepare_process() {
    let pkg = discover_pkg_dir();
    if pkg.is_empty() {
        rexon_core::log_warn!("wallpaper", "пакет rexon/ рядом не найден — вкладка обоев откроется без своих страниц");
    }
    // The `waywallen.ui` types and the Notify/App singletons — before
    // connecting to the daemon: it sends the start snapshot only once. At
    // birth `App` installs the translator of the vendored pages by the package
    // catalogue.
    crate::register_we_types();
    // The skin's link colour goes into the application palette before the
    // first QML page: HTML is parsed on loading, and without this links come
    // out `#0000ff`.
    crate::skin::apply_app_palette();
    register_filter_model();
    crate::we_singletons::set_launch_hook(Some(crate::we_socket::launch_daemon_hook));
    prewarm();
}

/// Prepare the QML engine for the vendored pages: import paths of the
/// plugins and of the vendored `waywallen.ui`, the thumbnail disk cache (one
/// per process, managed by `App` from the settings page) and recording the
/// engine for changing the language on the fly.
///
/// # Safety
/// `engine` is a live `QQmlEngine` (or one with it as the first ancestor)
/// from the main thread, or nullptr.
pub unsafe fn prepare_engine(engine: *mut std::ffi::c_void) -> bool {
    if engine.is_null() {
        return false;
    }
    let mut eng = unsafe { Pin::new_unchecked(&mut *engine.cast::<cxx_qt_lib::QQmlEngine>()) };
    // `addImportPath` puts the path AT THE START of the list — we walk from
    // the end, so that the most wanted directory ends up first.
    for dir in engine_import_dirs().iter().rev() {
        eng.as_mut().add_import_path(&cxx_qt_lib::QString::from(dir.as_str()));
    }
    let vendor = vendor_qml_dir();
    if !vendor.is_empty() {
        eng.as_mut().add_import_path(&cxx_qt_lib::QString::from(vendor.as_str()));
        // The vendor directory contains a QML replacement of the `QExtra` module,
        // and it is first in the list. The real plugin is taken because its module
        // directory goes even earlier; if it is not chosen, the list is left
        // alone, and the replacement works.
        if let rexon_core::qml_modules::QExtraChoice::Real { dir, .. } = qextra_choice() {
            eng.as_mut().add_import_path(&cxx_qt_lib::QString::from(dir.as_str()));
        }
    }
    let xdg_cache = match std::env::var("XDG_CACHE_HOME") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    };
    let cache = rexon_core::qml_modules::net_cache_dir(xdg_cache.as_deref(), &home());
    if let Ok(cache) = CString::new(cache) {
        unsafe {
            crate::we_singletons::set_network_cache(
                cache.as_ptr(),
                rexon_core::qml_modules::NET_CACHE_MAX_BYTES,
            );
            crate::we_singletons::install_qml_network_cache(engine);
        }
    }
    unsafe { crate::we_singletons::register_qml_engine(engine) };
    true
}

/// Whether BOTH plugins of the vendored interface are built — `Qcm.Material`
/// and `waywallen.control`.
///
/// Without them the vendored pages do not import at all, and the tab must
/// show a placeholder with build instructions rather than an empty space.
pub fn qml_plugins_ready() -> bool {
    rexon_core::qml_modules::qml_plugins_ready(&mut ModuleProbe, &qml_module_dirs())
}
