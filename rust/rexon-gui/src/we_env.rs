// Окружение процесса для запросов вкладки WE: домашний каталог, XDG,
// числовой идентификатор пользователя и каталог пакета `rexon/`.
//
// Не мост: чистые функции, которыми пользуются оболочки. Вынесено сюда,
// чтобы каталог пакета брался из ОДНОГО места (правило #12) — иначе каждый
// запрос завёл бы свою догадку о том, где лежит `rexon/`.

use core::pin::Pin;
use std::ffi::{c_char, CString};
use std::sync::{Mutex, OnceLock};

/// Домашний каталог. Пустая строка тоже допустима: ядро тогда соберёт путь
/// от корня, и файл просто не запишется — так же вёл себя Python.
pub fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `$XDG_CONFIG_HOME`, если он задан и непустой.
pub fn xdg_config_home() -> Option<String> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Числовой идентификатор пользователя.
///
/// Без libc: владелец `/proc/self` — это и есть текущий uid. Ядро просит
/// его только ради пути `/run/user/<uid>/systemd`.
pub fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0)
}

/// Живые проверки файловой системы для `we_client::find_daemon_binary`.
///
/// Одна на крейт: бинарь демона ищут и страница автозапуска, и транспорт, а
/// две копии этих двух методов разъехались бы молча (правило #12). Порядок и
/// состав кандидатов решает ядро, здесь только `stat` и обход `$PATH`.
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

    /// `<движок> --version`, с запоминанием по (путь, размер, время правки).
    ///
    /// Ядро спрашивает версию, только когда на диске лежат ОБА движка —
    /// из пакета и управляемый. Запуск AppImage стоит полсекунды, а поиск
    /// бинаря зовут на каждом открытии вкладки и на каждом тике автозапуска,
    /// поэтому ответ помнится, пока файл не подменили.
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
                    // Разные сборки печатают версию то в stdout, то в stderr.
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
                // Завис или не ждётся — это НАШ дочерний процесс по его
                // идентификатору, не поиск по имени.
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

/// Путь к бинарю демона обоев (или `None`). Одна дверь на весь крейт.
pub fn daemon_binary() -> Option<String> {
    rexon_core::we_client::find_daemon_binary(&mut FsProbe, &pkg_dir(), &home())
}

/// Раскладки движка из пакетов (`we_client::engine_layouts`), которые
/// реально стоят на этой машине: лежит `plugins/` или сам демон.
pub fn present_engine_layouts() -> Vec<rexon_core::we_client::EngineLayout> {
    use rexon_core::we_client::DaemonFs;
    rexon_core::we_client::engine_layouts()
        .into_iter()
        .filter(|l| std::path::Path::new(&l.plugins_dir()).is_dir() || FsProbe.is_exec_file(&l.daemon))
        .collect()
}

/// Движок из пакета, с которым сверяется живой демон: раскладка запускаемого
/// демона, а если запускается чужой (управляемый AppImage человека) — первая
/// стоящая на диске. `None` — движка из пакетов нет (дерево разработчика,
/// ручная установка без пакетов).
pub fn package_engine() -> Option<rexon_core::we_client::EngineLayout> {
    let present = present_engine_layouts();
    let binary = daemon_binary();
    binary
        .as_deref()
        .and_then(|b| rexon_core::we_client::layout_of(&present, b).cloned())
        .or_else(|| present.into_iter().next())
}

/// Каталог, который демон получает флагом `--plugin`
/// (`we_client::engine_launch_args`), для запуска бинаря `binary`. Решает
/// ядро (`we_client::plugin_root_for`): флаг нужен, только если демон не
/// видит каталог плагинов пакета сам — `waywallen` из пакета дистрибутива
/// его видит, `waywallen-bin` и управляемый AppImage нет.
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

/// [`engine_plugin_root_for`] для демона, которого запустит Rexon.
pub fn engine_plugin_root() -> Option<String> {
    engine_plugin_root_for(daemon_binary().as_deref())
}

/// Плагины движка из пакета: `id`/`name`/`version` каждого
/// `<каталог движка>/plugins/*/plugin.toml`. Пусто — пакета нет.
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

/// Поставлен ли Rexon пакетом дистрибутива: лаунчер пакета выставил
/// `REXON_PACKAGED`, либо сам бинарь лежит там, куда кладёт только пакетный
/// менеджер (ярлык может звать окно напрямую, мимо лаунчера). Решения —
/// ядра (`installer::PACKAGED_ENV`, `installer::packaged_location`).
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

/// Каталог пакета `rexon/` — от него ядро строит путь к вшитому бинарю
/// движка (`rexon/vendor/`).
///
/// Пусто, пока вкладку не готовили: значение кладёт [`discover_pkg_dir`] при
/// первой подготовке процесса (`prepare_process`), выводя его из пути своего бинаря.
pub fn pkg_dir() -> String {
    pkg_dir_slot()
        .lock()
        .expect("каталог пакета не отравлен")
        .clone()
}

// ============================================================================
// Собранные QML-плагины вкладки
// ============================================================================

/// Живые проверки ФС для [`rexon_core::qml_modules`].
///
/// Не копия решения: порядок и состав каталогов держит ядро, здесь только
/// `is_file`.
struct ModuleProbe;

impl rexon_core::qml_modules::ModuleFs for ModuleProbe {
    fn is_file(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    /// Заметка `.note.qt.metadata` лежит в самом начале файла плагина
    /// (первые сотни байт), поэтому хватает головы файла, а не всего его.
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

    /// Отметка сборки модуля — одна строка; больше килобайта не читается.
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
    /// `qVersion()` из QtCore — версия Qt, с которой процесс РАБОТАЕТ (не
    /// та, с которой собран). Объявлена в Qt с C-связыванием.
    fn qVersion() -> *const c_char;
}

/// Версия запущенного Qt (старшая, младшая).
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

/// `Qcm.Material` нашёлся, но собран под другую младшую версию Qt:
/// `(версия сборки, версия запущенного Qt)`. Такая сборка не грузится
/// (см. `rexon_core::qml_modules`), и заглушка вкладки объясняет почему.
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

/// `$XDG_DATA_HOME`, если он задан и непустой.
pub fn xdg_data_home() -> Option<String> {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// `$XDG_DATA_DIRS` сырым значением (через двоеточие).
pub fn xdg_data_dirs() -> String {
    std::env::var("XDG_DATA_DIRS").unwrap_or_default()
}

/// Развернуть `qml_modules::MULTIARCH_GLOB` обходом `/usr/lib`.
///
/// Тот же ответ, что у `sorted(glob.glob(...))` в Python и у соседнего
/// исполнителя в `rexon-py`; шаблон и здесь берётся у ядра.
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

/// Все каталоги, где могут лежать собранные QML-плагины, по убыванию
/// предпочтения. Состав и порядок — ядро.
pub fn qml_module_dirs() -> Vec<String> {
    rexon_core::qml_modules::module_dirs(
        xdg_data_home().as_deref(),
        &home(),
        &xdg_data_dirs(),
        &multiarch_dirs(),
    )
}

// ============================================================================
// Хозяин вкладки: подготовка процесса и движка QML
// ============================================================================
//
// Подготовка вкладки обоев для Rust-окна (`bin/main_window.rs`).
// Решений здесь нет: состав и порядок каталогов, имена файлов, кандидаты
// сборок и кэш — ядро (`qml_modules`, `remote_l10n`).

/// Найти каталог пакета `rexon/`, если его ещё никто не назвал.
///
/// Корень выводится из пути своего бинаря — той же функцией ядра, какой
/// окно ищет заставку (`cli::repo_root`).
/// Пакет признаётся, только если в нём действительно лежит вендорный
/// интерфейс: чужой каталог `rexon` рядом не должен сойти за него.
fn discover_pkg_dir() -> String {
    let known = pkg_dir();
    if !known.is_empty() {
        return known;
    }
    // Раскладок три — дерево разработчика, пакет дистрибутива и установка
    // `installer.py`; их состав и порядок держит ядро, здесь только живая
    // проба файла (правило #12). Без этого окно из пакета открывало вкладку
    // обоев без вендорных страниц: дерева `rust/Cargo.toml` рядом с ним нет.
    let root = rexon_core::cli::repo_root();
    let Some(candidate) = rexon_core::installer::pick_package_data_dir(&root, &|path: &str| {
        std::path::Path::new(path).is_file()
    }) else {
        return String::new();
    };
    *pkg_dir_slot().lock().expect("каталог пакета не отравлен") = candidate.clone();
    candidate
}

/// Каталог вендорного интерфейса waywallen (`qml_ui/` пакета); пусто —
/// пакета нет, и страница открывается без вендорных страниц.
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

/// Корень дерева для кандидатов сборок: родитель пакета `rexon/`, по
/// настоящему пути (симлинк установленной копии ведёт в рабочее дерево).
fn repo_root_of_pkg() -> String {
    let pkg = pkg_dir();
    std::fs::canonicalize(&pkg)
        .ok()
        .and_then(|p| p.parent().map(|r| r.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Сказать в журнале (один раз за процесс), какой каталог модулей пропущен
/// из-за `waywallen.control`, собранного из другого протокола. Само решение —
/// ядро (`qml_modules::stale_control_dirs`).
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

/// Каталоги для `addImportPath` в порядке предпочтения: где реально что-то
/// лежит, а если нигде — каталог, где искали `Qcm.Material` (как у Python).
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

/// Какой модуль `QExtra` получит движок: настоящий плагин автора или наша
/// QML-подмена из каталога вендорной разметки. Решение — ядро
/// (`qml_modules::choose_qextra`); здесь живые пробы и одна строка в журнал
/// на процесс.
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

/// Загрузить в процесс value-типы Qt-Protobuf `waywallen.control` без
/// движка QML: библиотека типов берётся из свежего каталога модулей (того
/// же, что отдаётся движку). Нужна стенду страницы «Обои» — он гоняет шим
/// гаджетов на НАСТОЯЩИХ типах, а не на подделке. `Ok` — путь библиотеки.
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

/// Зарегистрировать модель редактора фильтров в `waywallen.ui`.
///
/// Модель — отдельный крейт (`rexon-qt-filter`, только `cdylib`: гаджеты
/// Qt-Protobuf маршалятся лишь в C++/Rust), поэтому её сборка грузится с
/// диска, первым существующим кандидатом ядра. `false` — не нашлась ни
/// одна: редактор фильтров будет пуст, но страница не упадёт.
pub fn register_filter_model() -> bool {
    static DONE: OnceLock<bool> = OnceLock::new();
    *DONE.get_or_init(|| {
        let root = repo_root_of_pkg();
        // Каталоги — уже без устаревших: модель фильтров пишет гаджеты
        // Qt-Protobuf и обязана быть из той же сборки, что `waywallen.control`.
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
    /// Движок прогрева живёт до конца процесса — как у Python.
    static PREWARM: std::cell::RefCell<Option<cxx::UniquePtr<cxx_qt_lib::QQmlApplicationEngine>>> =
        const { std::cell::RefCell::new(None) };
}

/// Прогреть регистрацию Qt-Protobuf на ГЛАВНОМ потоке.
///
/// Вендорные страницы грузятся асинхронно, то есть компилируются в
/// `QQmlThread`; первая встреча с `waywallen.control` там встаёт на
/// дедлоке глобальной регистрации protobuf-типов. Поэтому модуль один раз
/// импортируется здесь, синхронно, отдельным движком. `false` — греть
/// нечего (пакета или плагинов нет).
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

/// Подготовка процесса для Rust-окна: всё, что питоновский хозяин делает
/// до первой загрузки QML. Звать из главного потока с живым приложением.
pub fn prepare_process() {
    let pkg = discover_pkg_dir();
    if pkg.is_empty() {
        rexon_core::log_warn!("wallpaper", "пакет rexon/ рядом не найден — вкладка обоев откроется без своих страниц");
    }
    // Типы `waywallen.ui` и синглтоны Notify/App — раньше подключения к
    // демону: стартовый снимок он шлёт один раз. `App` при рождении ставит
    // переводчик вендорных страниц по каталогу пакета.
    crate::register_we_types();
    register_filter_model();
    crate::we_singletons::set_launch_hook(Some(crate::we_socket::launch_daemon_hook));
    prewarm();
}

/// Подготовить движок QML к вендорным страницам: пути импорта плагинов и
/// вендорного `waywallen.ui`, дисковый кэш миниатюр (один на процесс,
/// им управляет `App` со страницы настроек) и запись движка для смены
/// языка на лету.
///
/// # Safety
/// `engine` — живой `QQmlEngine` (или наследник первым предком) из главного
/// потока либо nullptr.
pub unsafe fn prepare_engine(engine: *mut std::ffi::c_void) -> bool {
    if engine.is_null() {
        return false;
    }
    let mut eng = unsafe { Pin::new_unchecked(&mut *engine.cast::<cxx_qt_lib::QQmlEngine>()) };
    // `addImportPath` кладёт путь В НАЧАЛО списка — обходим с конца, чтобы
    // первым остался самый желанный каталог.
    for dir in engine_import_dirs().iter().rev() {
        eng.as_mut().add_import_path(&cxx_qt_lib::QString::from(dir.as_str()));
    }
    let vendor = vendor_qml_dir();
    if !vendor.is_empty() {
        eng.as_mut().add_import_path(&cxx_qt_lib::QString::from(vendor.as_str()));
        // В каталоге вендора лежит QML-подмена модуля `QExtra`, и стоит он в
        // списке первым. Настоящий плагин берётся тем, что его каталог
        // модулей встаёт ещё раньше; не выбран — список не трогаем, и
        // работает подмена.
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

/// Собраны ли ОБА плагина вендорного интерфейса — `Qcm.Material` и
/// `waywallen.control`.
///
/// Без них вендорные страницы не импортируются вовсе, и вкладка обязана
/// показать заглушку с инструкцией по сборке, а не пустое место.
pub fn qml_plugins_ready() -> bool {
    rexon_core::qml_modules::qml_plugins_ready(&mut ModuleProbe, &qml_module_dirs())
}
