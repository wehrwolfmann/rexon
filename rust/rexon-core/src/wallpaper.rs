//! Решения точки входа обоев: что запускать и с какими настройками.
//!
//! Портировался с `rexon/wallpaper.py` — прежней ЕДИНОЙ точки входа обоев.
//! Её в дереве больше нет: обои всех типов, кроме видео, рисует движок
//! waywallen, а видео-хост стал точкой входа сам. Python-хоста тоже нет
//! (снят 2026-08-23) — зовёт отсюда бинарь `rexon-wallpaper-host` крейта
//! `rexon-gui`. Поэтому здесь осталось ровно то, что видео-хост зовёт:
//!
//!   * [`use_parent_dir`] — путь ведёт на файл ВНУТРИ каталога обоев;
//!   * [`mmap_size`] и умолчания [`DEFAULT_MMAP_WIDTH`]/[`DEFAULT_MMAP_HEIGHT`]
//!     — размер кадра mmap из окружения, с откатом при мусоре;
//!   * [`parse_user_props_override`] — переопределение свойств из окружения;
//!   * решения самого видео-хоста: какой QML-файл грузить
//!     ([`video_qml_file`]), что и с какими умолчаниями класть в контекст
//!     QML ([`video_context_properties`]), берётся ли обоина этим хостом
//!     вообще ([`video_host_reject_reason`]).
//!
//! Выбор исполнителя (`runner_id`/`runner_source`, переменная
//! `REXON_WP_RUNNER`) снят: он обслуживал обои-приложения, а их движок
//! и пакет исполнителей удалены — переменную не читает никто, кроме
//! исторических записок в `docs/`.
//!
//! Сам запуск рендера, чтение `project.json` и работа с файловой системой
//! остаются исполнителю.

use serde_json::{Map, Value};

/// Расширения, при которых путь считается указанием на файл ВНУТРИ каталога
/// обоев, а не на сам каталог: старые точки входа принимали `index.html`,
/// `scene.json` и подобное, и такие вызовы должны продолжать работать.
pub const LEGACY_FILE_SUFFIXES: &[&str] =
    &[".html", ".htm", ".json", ".mp4", ".webm", ".exe"];

/// Ширина кадра mmap по умолчанию.
pub const DEFAULT_MMAP_WIDTH: i64 = 1920;
/// Высота кадра mmap по умолчанию.
pub const DEFAULT_MMAP_HEIGHT: i64 = 1080;

/// Нужно ли подняться к родительскому каталогу: путь указывает на файл со
/// «старым» расширением. Регистр расширения не важен.
pub fn use_parent_dir(path: &str, is_file: bool) -> bool {
    if !is_file {
        return false;
    }
    let lower = path.to_lowercase();
    LEGACY_FILE_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

/// Результат разбора переопределения пользовательских свойств.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserPropsOverride {
    /// Переопределения нет — берутся значения из `project.json`.
    None,
    /// Разобранные свойства в каноническом виде `{ключ: {"value": …}}`.
    Props(Map<String, Value>),
    /// Значение не разобралось — исполнитель предупреждает и идёт на
    /// значения по умолчанию, а не падает.
    Broken,
}

/// Разобрать переопределение свойств из окружения.
///
/// Конфигуратор кладёт сохранённый JSON в переменную окружения. Формат
/// приводится к каноническому `{ключ: {"value": …}}`: голое значение
/// заворачивается, уже завёрнутое остаётся как есть. Пустая строка и `{}`
/// означают «переопределения нет».
///
/// Валидный JSON, который не является объектом (список, строка, `null`),
/// трактуется как отсутствие переопределения — так же, как в оригинале.
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

/// Размер кадра mmap из окружения.
///
/// ⚠️ Намеренное расхождение с оригиналом (починка при порте): нечисловое
/// значение переменной там роняло запуск обоев необработанным `ValueError`
/// прямо на старте. Здесь мусор откатывается к значению по умолчанию —
/// кривая переменная окружения не должна оставлять пользователя без обоев.
/// Пробелы по краям срезаются, как это делал `int()` Python.
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

// ── видео-хост: что показывать и с какими настройками ──────────────────────
//
// Решения, снятые с прежнего Python-хоста (`rexon/video_wallpaper.py`,
// снят с дерева 2026-08-23). Сам Qt (QQmlApplicationEngine, LayerShellQt,
// QtMultimedia) и протокол композитора живут в крейте `rexon-gui`; здесь —
// то, что решается до первого Qt-вызова: какой режим показа выбрать, какие
// свойства класть в контекст QML и с какими значениями, и берётся ли эта
// обоина этим хостом вообще.

/// ⚠️ Три константы ниже — ТОКЕНЫ режима показа, а не имена файлов на
/// диске: файлов с такими именами в дереве нет с тех пор, как сняли
/// Python-хост. Rust-хост рисует одним `WallpaperVideo.qml`, а токен
/// переводит в режим `rexon-gui::wallpaper_host::placement`.
///
/// Офскрин-режим: окна на экране нет, кадры уходят расширению оболочки
/// через mmap (GNOME/Cinnamon на Wayland — там нет layer-shell).
pub const QML_OFFSCREEN: &str = "VideoWallpaperOffscreen.qml";
/// QML-файл Wayland-режима: окно на слое BACKGROUND через LayerShellQt.
pub const QML_WAYLAND: &str = "VideoWallpaper.qml";
/// QML-файл X11-режима: обычное окно + `_NET_WM_WINDOW_TYPE_DESKTOP`.
pub const QML_X11: &str = "VideoWallpaperX11.qml";

/// Единственный тип обоев, который рисует этот хост. Остальные рисует
/// движок waywallen.
pub const VIDEO_HOST_TYPE: &str = "video";

/// Звук у WE-обоев включён по умолчанию (`audioenabled`).
pub const DEFAULT_AUDIO_ENABLED: bool = true;
/// Громкость по умолчанию в долях единицы (`volume`; WE хранит 0..100).
pub const DEFAULT_VOLUME: f64 = 0.5;
/// Зацикливание по умолчанию (`loopvideo`).
pub const DEFAULT_LOOP: bool = true;

/// Какой QML-файл грузит видео-хост — порт `VideoWallpaper.get_qml_file`.
///
/// Порядок проверок значим: офскрин-режим перекрывает всё, потому что при
/// заданном mmap-выводе окно на экран не выводится вовсе и обслуживать
/// LayerShellQt там некому.
pub fn video_qml_file(has_mmap_output: bool, wayland: bool) -> &'static str {
    if has_mmap_output {
        QML_OFFSCREEN
    } else if wayland {
        QML_WAYLAND
    } else {
        QML_X11
    }
}

/// Значение свойства контекста QML в том виде, в каком его надо положить.
#[derive(Debug, Clone, PartialEq)]
pub enum QmlValue {
    /// Локальный путь, который шим оборачивает в `QUrl.fromLocalFile`.
    LocalFileUrl(String),
    /// Готовая ссылка (`http://`, `https://`, `file://`) — её заворачивать
    /// в `fromLocalFile` НЕЛЬЗЯ: получится «file:///https:/example.com».
    Url(String),
    Bool(bool),
    /// Вещественное (QML `real`).
    Real(f64),
    Int(i64),
}

/// Одно свойство контекста QML: имя и значение.
#[derive(Debug, Clone, PartialEq)]
pub struct QmlContextProperty {
    pub name: String,
    pub value: QmlValue,
}

/// Свойства контекста QML для видео-обоев — порт
/// `VideoWallpaper.setup_renderer` целиком (имена, порядок, умолчания и
/// приведение типов).
///
/// `target_file` — абсолютный путь к видеофайлу; `user_props` — свойства из
/// `project.json` в формате `{ключ: {"value": …}}` либо плоские значения
/// (принимается и то, и другое: в оригинале сюда приходил `defaults_for_js`,
/// а из конфигуратора — голые значения).
///
/// ⚠️ Имя `audioVolume`, а НЕ `volume`: одноимённое с `AudioOutput.volume`
/// свойство контекста уводит QML в рекурсию привязок.
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
        // Размер кадра для офскрин-варианта: окна на экране нет, поэтому
        // Screen.width/height в QML брать неоткуда.
        named("renderWidth", QmlValue::Int(mmap_width)),
        named("renderHeight", QmlValue::Int(mmap_height)),
    ]
}

/// Свойство WE как булево — порт `_as_bool`.
///
/// WE хранит флаги и настоящим `true`, и числом, и строкой «true»/«1»/«on».
/// Всё, что не опознано (в т.ч. отсутствие свойства и `null`), даёт
/// умолчание.
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

/// Громкость WE в долях единицы — порт `_as_float01`.
///
/// WE хранит громкость в 0..100, а `AudioOutput.volume` в QML ждёт 0..1:
/// значение больше единицы делится на сто. Неразобранное — умолчание,
/// результат зажимается в 0..1.
pub fn as_volume01(value: Option<&Value>, default: f64) -> f64 {
    let raw = match value {
        Some(Value::Number(n)) => n.as_f64(),
        // Строку оригинал разбирал тем же `float()`, что и число.
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

/// Берётся ли этот тип обоев видео-хостом. `None` — берётся; `Some(текст)`
/// — отказ с готовым объяснением (порт сообщения `_build_ctx_from_dir`).
///
/// Отказ честный и внятный: пустое окно вместо обоев человеку ничего не
/// объясняет (правило #13).
pub fn video_host_reject_reason(wp_type: &str, project_json: &str) -> Option<String> {
    if wp_type == VIDEO_HOST_TYPE {
        return None;
    }
    // Кавычки одинарные — как их печатал `repr()` оригинала.
    let shown = if wp_type.is_empty() { "?" } else { wp_type };
    Some(format!(
        "Обои типа '{shown}' рисует движок waywallen, а не этот хост: {project_json}"
    ))
}

// ── веб-обои ───────────────────────────────────────────────────────────────
//
// Решения веб-режима хоста обоев (`rexon-wallpaper-host --mode web`, крейт
// `rexon-gui`; до 15.09.2026 — отдельный бинарь `rexon-web-host`). Тот же
// раздел обязанностей, что у видео выше: здесь всё, что считается ДО первого
// вызова Qt, — что показывать, какие свойства класть в корень QML, какой JS
// подмешивать в страницу и берётся ли обоина этим хостом.
//
// ⚠️ Почему это вообще пишется заново. Прежний хост (`rexon/web_wallpaper.py`,
// 791 строка) удалён 30.07.2026 как «ненужный, рисует waywallen»; вместе с
// ним ушёл и разбор. А строку запуска `python -m rexon.web_wallpaper`
// движок обоев продолжал строить — то есть формат «веб» с того дня не
// показывался НИГДЕ и ни на одном рабочем столе, при том что зависимость
// `qt6-webengine` продолжала ставиться каждому покупателю.

/// Тип обоев, который рисует хост веб-обоев.
pub const WEB_HOST_TYPE: &str = "web";

/// Точка входа веб-обоев внутри каталога — так её зовёт Wallpaper Engine.
pub const WEB_ENTRY_FILE: &str = "index.html";

/// Ссылка ли это (а не путь к файлу).
///
/// Одно решение на продукт: тем же вопросом задаётся движок обоев
/// (`wallpaper_engine::is_url`), и второй копии правила быть не должно.
pub fn is_web_url(path: &str) -> bool {
    crate::wallpaper_engine::is_url(path) || path.starts_with("file://")
}

/// Свойства корня QML для веб-обоев — тот же приём, что у видео.
///
/// `target` — абсолютный путь к HTML либо готовая ссылка (`http://`,
/// `https://`, `file://`): у ссылки локального файла нет, и заворачивать её
/// в `QUrl::fromLocalFile` нельзя — получится «file:///https:/example.com».
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

/// Свойства обоев Wallpaper Engine в том виде, в каком их ждёт JS страницы:
/// `{"имя": {"value": …}}`. Порт `_load_web_defaults` прежнего хоста вместе
/// с его правилом.
///
/// ⚠️ Правило неочевидное и важное: свойство БЕЗ значения (или с пустой
/// строкой) уезжает как пустой объект `{}`, а не пропускается. У обоев с
/// файловым свойством (картинка, которую человек не выбирал) от этого
/// зависит целая ветка их собственного кода: увидев `{}`, обоина грузит
/// свой запасной файл, а не остаётся пустой.
pub fn web_defaults_for_js(user_props: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (name, raw) in user_props {
        // Плоское значение (из конфигуратора) заворачивается так же, как
        // пришедшее из project.json: дальше их не различить.
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

/// Те же свойства одной строкой JSON — в таком виде их принимает JS.
pub fn web_defaults_json(user_props: &Map<String, Value>) -> String {
    serde_json::to_string(&Value::Object(web_defaults_for_js(user_props)))
        .unwrap_or_else(|_| "{}".to_string())
}

/// Заглушки глобальных функций Wallpaper Engine, подмешиваемые в страницу.
///
/// Без них обои Мастерской падают ещё при разборе HTML: они проверяют
/// `typeof window.wallpaperRegisterAudioListener === 'function'` синхронно, и
/// отсутствие функции для них — не «звука не будет», а обрыв всего скрипта.
/// Поэтому сценарий подмешивается точкой `DocumentCreation`, до первого
/// скрипта самой страницы.
///
/// Слушателя звука заглушка НЕ выбрасывает, а сохраняет: обоина
/// перерисовывает холст именно из него, и молчаливый `noop` оставил бы на
/// экране первый кадр с запасными цветами вместо настроенных.
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

/// Первый тик звука пустым спектром — после него холст перерисовывается уже
/// с настройками человека.
///
/// Тик ОДИН, а не поток: настоящего спектра хосту сейчас взять негде
/// (прежний путь тянул его с Python-стороны через stdin, и вместе с
/// Python-хостом ушёл). Один тик — это разница между «обои нарисовались
/// настроенными цветами» и «обои застыли на запасных»; поток без данных был
/// бы обманом.
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

/// Сценарий передачи свойств обоев в страницу: `applyUserProperties`.
///
/// Обёрнут в `try`, потому что слушателя у обоины может не быть вовсе, а
/// исключение в подмешанном сценарии оборвало бы и наш тик звука следом.
pub fn web_apply_defaults_js(defaults_json: &str) -> String {
    format!(
        "(function(d){{try{{if(window.wallpaperPropertyListener && \
         window.wallpaperPropertyListener.applyUserProperties)\
         window.wallpaperPropertyListener.applyUserProperties(d);\
         }}catch(e){{}}}})({defaults_json});"
    )
}

/// Флаги Chromium для офскрин-режима.
///
/// `--in-process-gpu` и НИКАКОГО `--disable-gpu`: с выключенным GPU Chromium
/// не сводит холст (canvas2D/WebGL) в граф сцены Qt, и снятый с окна кадр
/// содержал бы только DOM — прежнему хосту из-за этого приходилось тащить
/// обходной мост через `canvas.toDataURL` (PNG + base64 на каждый кадр).
pub const WEB_OFFSCREEN_CHROMIUM_FLAGS: &str = "--in-process-gpu";

/// Берётся ли этот тип обоев веб-хостом. `None` — берётся.
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
