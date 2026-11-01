// Хост веб-обоев на Rust: замена удалённого `python -m rexon.web_wallpaper`.
//
// Что это за кусок и почему он появился заново. 30.07.2026 из дерева убрали
// хосты web-, app- и parallax-обоев (2 179 строк) под тем предлогом, что всё
// это рисует движок waywallen. Для app и parallax это так, для веба — нет:
// строку запуска `python -m rexon.web_wallpaper` движок обоев продолжал
// собирать (`wallpaper_engine::web_plan`), модуля под ней уже не было, и
// формат «веб» с того дня не показывался НИ НА ОДНОМ рабочем столе. При этом
// `qt6-webengine` продолжал ставиться каждому покупателю
// (`installer::dep_packages`), а в коде не использовался ни разу.
//
// Разделение обязанностей ровно то же, что у видео-хоста:
//
//   * ЧТО показывать, какие свойства класть в корень QML и какой JS
//     подмешивать в страницу — `rexon-core::wallpaper`
//     (`web_context_properties`, `web_defaults_json`, `WE_POLYFILL_JS`,
//     `web_apply_defaults_js`, `WE_TICKER_JS`, `web_host_reject_reason`).
//     Своих строк и чисел здесь нет;
//   * КАК положить окно на задний план — три способа, и все три общие с
//     видео-хостом: `WallpaperLayer` (src/layer_shell.rs) и
//     `WallpaperPlacement` (src/wallpaper_host.rs). Второго решения про слой,
//     xprop и поток кадров в проекте нет (правило #12).
//
// ⭐ С 15.09.2026 это РЕЖИМ единого хоста обоев `rexon-wallpaper-host`
// (`--mode web`), а не отдельный бинарь `rexon-web-host`: хосты решено не
// плодить. Подъём Qt общий — src/bin/wallpaper_host/main.rs, что показывать
// — src/bin/wallpaper_host/web.rs.
//
// ⚠️ Единственное, чего у видео нет: `QtWebEngineQuick::initialize()` обязан
// отработать ДО создания `QGuiApplication`. Биндинга в cxx-qt-lib нет, шима
// на C++ тоже нет.
//
// ⭐ Почему функция берётся `dlopen`/`dlsym`, а не мостом cxx, как было у
// отдельного веб-хоста. Мост давал прямую ссылку на символ, и бинарь
// линковался с libQt6WebEngineQuick, а через неё с Chromium: у веб-хоста
// было 240 библиотек в зависимостях против 96 у видео. В одном бинаре с
// видео это значило бы грузить Chromium при КАЖДОМ старте видео-обоев.
// Замер до объединения (платформа offscreen, `--selftest=500`, 5 прогонов,
// медиана): видео-хост 956 мс и 335,5 МиБ пика, он же с принудительно
// подгруженной QtWebEngineQuick (`LD_PRELOAD`) — 997 мс и 347,2 МиБ. Поэтому
// библиотека открывается здесь и только в веб-режиме; QML-импорт
// `QtWebEngine` потом находит её уже загруженной.
//
// Своего C++ по-прежнему ноль, и правило #16 не нарушено:
// `void QtWebEngineQuick::initialize()` — свободная функция без аргументов и
// без возврата; по ABI Itanium её вызов не отличается от `extern "C" fn()`.
// Искажённое имя стабильно у всего Qt 6 (версия символа `Qt_6`), а если его
// не окажется — отказ с объяснением, а не падение загрузчика с кодом 127.

use std::ffi::{c_char, c_int, c_void, CStr};

/// Библиотека QtWebEngine для Qt Quick (пакет `qt6-webengine`).
const WEB_ENGINE_QUICK_LIB: &CStr = c"libQt6WebEngineQuick.so.6";

/// Искажённое имя `QtWebEngineQuick::initialize()`.
const INITIALIZE_SYMBOL: &CStr = c"_ZN16QtWebEngineQuick10initializeEv";

const RTLD_NOW: c_int = 0x2;
/// Глобально: QML-плагин `QtWebEngine`, который грузится позже, обязан
/// увидеть ТУ ЖЕ копию библиотеки, что уже подняла Chromium.
const RTLD_GLOBAL: c_int = 0x100;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

fn dl_error() -> String {
    // SAFETY: dlerror возвращает либо NULL, либо строку, живущую до
    // следующего вызова dl* в этом потоке; копируем её сразу.
    let raw = unsafe { dlerror() };
    if raw.is_null() {
        "причина не названа".to_string()
    } else {
        unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned()
    }
}

/// Открыть QtWebEngineQuick и найти в ней `initialize`, НЕ вызывая.
fn resolve_initialize() -> Result<extern "C" fn(), String> {
    let lib = WEB_ENGINE_QUICK_LIB.to_string_lossy();
    // SAFETY: имя библиотеки — строка с нулём на конце; дескриптор
    // намеренно не закрывается: библиотека нужна процессу до выхода.
    let handle = unsafe { dlopen(WEB_ENGINE_QUICK_LIB.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
    if handle.is_null() {
        return Err(format!(
            "QtWebEngine не загрузился ({}): веб-обоям нужна {lib} из пакета qt6-webengine",
            dl_error()
        ));
    }
    // SAFETY: дескриптор получен выше, имя символа — строка с нулём.
    let symbol = unsafe { dlsym(handle, INITIALIZE_SYMBOL.as_ptr()) };
    if symbol.is_null() {
        return Err(format!(
            "в {lib} нет QtWebEngineQuick::initialize ({})",
            dl_error()
        ));
    }
    // SAFETY: символ — `void QtWebEngineQuick::initialize()`, свободная
    // функция без аргументов и возврата (см. разбор в начале файла).
    Ok(unsafe { std::mem::transmute::<*mut c_void, extern "C" fn()>(symbol) })
}

/// Поднять QtWebEngine. Звать ТОЛЬКО в веб-режиме и ДО `QGuiApplication`:
/// подсистема выставляет общий контекст OpenGL для всех окон процесса, а
/// после создания приложения это уже поздно.
pub fn init_web_engine() -> Result<(), String> {
    let initialize = resolve_initialize()?;
    initialize();
    Ok(())
}

// ── Разбор аргументов ──────────────────────────────────────────────────────

/// Ключи командной строки хоста.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cli {
    /// Ссылка (`http://`, `https://`, `file://`), путь к HTML или каталог
    /// обоев с `project.json`.
    pub target: String,
    pub windowed: bool,
    pub verbose: bool,
    /// Самопроверка: через сколько миллисекунд окно отчитается и закроется.
    pub selftest_ms: i64,
}

/// Умолчание паузы самопроверки. Больше, чем у видео (3 с): Chromium
/// поднимается медленнее проигрывателя, и на холодном запуске трёх секунд не
/// хватает даже на пустую страницу — отчёт выходил бы «не загрузилось» там,
/// где загружалось.
pub const DEFAULT_SELFTEST_MS: i64 = 6000;

/// Разбор аргументов веб-режима (ключ `--mode` уже снят
/// [`crate::wallpaper_host::take_mode`]). Ошибка — готовая строка для
/// человека; `--help` отвечает общей справкой хоста.
pub fn parse_args(argv: &[String]) -> Result<Cli, String> {
    let mut cli = Cli::default();
    for arg in argv {
        match arg.as_str() {
            "-w" | "--windowed" => cli.windowed = true,
            "-v" | "--verbose" => cli.verbose = true,
            "-h" | "--help" => return Err(USAGE.to_string()),
            "--audio" | "--no-audio" | "--volume" => {
                // Звук веб-обоев ведёт сама страница; молча проглотить ключ
                // значило бы сделать вид, что он что-то включил.
                return Err(format!("ключ {arg} есть только у режима video"));
            }
            "--selftest" => cli.selftest_ms = DEFAULT_SELFTEST_MS,
            other if other.starts_with("--selftest=") => {
                cli.selftest_ms = other["--selftest=".len()..]
                    .parse()
                    .map_err(|_| "после --selftest= нужны миллисекунды".to_string())?;
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("неизвестный ключ: {other}"));
            }
            other => {
                if !cli.target.is_empty() {
                    return Err(format!("лишний адрес: {other}"));
                }
                cli.target = other.to_string();
            }
        }
    }
    if cli.target.is_empty() {
        return Err("не указана ссылка, HTML-файл или каталог обоев".to_string());
    }
    Ok(cli)
}

use crate::wallpaper_host::USAGE;

#[cfg(test)]
mod tests {
    use super::*;

    /// Библиотека и символ, которые открывает веб-режим, на месте — без
    /// вызова: поднимать Chromium в процессе тестов незачем. Краснеет, если
    /// Qt сменит имя библиотеки или искажение имени функции.
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
        assert!(parse_args(&["--нетакого".into(), "/a.html".into()]).is_err());
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
        assert!(parse_args(&["/a.html".into(), "--selftest=скоро".into()]).is_err());
    }

    #[test]
    fn справка_не_ошибка_разбора() {
        assert_eq!(parse_args(&["--help".to_string()]), Err(USAGE.to_string()));
    }
}
