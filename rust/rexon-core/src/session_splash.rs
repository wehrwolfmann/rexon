//! Чистая логика заставки входа — порт `rexon/session_splash.py`.
//!
//! Оригинал (Qt-free, ~1600 строк) проигрывает выбранный webm полноэкранно
//! СРАЗУ ПОСЛЕ ВХОДА в сессию (как `deck_startup.webm` на Steam Deck):
//! скан заставок Steam (встроенные + инвентарь Points Shop), конфиг,
//! «гибрид KSplash» (тема-постер + ранний systemd-юнит + эстафета
//! ksplash→mpv), правило KWin «поверх всего», три пути прогона (Wayland
//! Rust-хелпер rexon-splash-overlay / X11 override-redirect слой / оконный
//! mpv) и autostart.
//!
//! Здесь — ТОЛЬКО детерминированная часть: вход → выход, без файловой
//! системы, сокетов и процессов. Наблюдения за средой (наличие бинарей,
//! содержимое файлов, переменные окружения, вывод xrandr, байты из
//! mpv-IPC-сокета) подаются ПАРАМЕТРАМИ; исполнение (Popen mpv/ffmpeg/
//! systemctl/kwriteconfig6, unix-сокеты, flock, запись файлов, ожидания
//! с таймаутами) остаётся в Python-шиме, который прогоняет построенные
//! здесь argv/тексты/решения — образец подхода см. `system_plan.rs`.
//!
//! Принципиально непортируемое в ядро (живёт в Python / отдельном крейте):
//!   * `_X11SplashOverlay` целиком — живой X11-клиент на ctypes+libX11
//!     (окно/pixmap/XPutImage/событийный raise-цикл, время жизни = живое
//!     соединение с X-сервером) + Pillow-масштабирование постера;
//!   * сама эстафета `_hybrid_wait_and_unpause` и все poll-ожидания
//!     (`time.monotonic`-циклы) — здесь только их константы-таймауты.
//!
//! Паритет байт-в-байт с живым Python — `tests/session_splash_parity.rs`
//! (эталон снят read-only: HOME/XDG → temp, subprocess/socket замоканы,
//! mpv/ffmpeg/systemctl/KWin НЕ запускались; пути нормализованы в /HOME и
//! /usr/local/lib/rexon).
//!
//! ⚠️ Починенные при порте баги оригинала (правило #16 — не консервируем):
//!   1. `_ensure_kwin_splash_rule`: при `[General]` без строк `rules=`/
//!      `count=` Python выполнял `re.sub` вхолостую — новый блок правила
//!      дописывался, но НЕ регистрировался в `rules=` (KWin его игнорирует).
//!      Здесь недостающие ключи вписываются в секцию `[General]`.
//!   2. `_mpv_ipc_get`: строка сокета с валидным JSON-НЕ-объектом (например
//!      `[1,2]`) роняла `msg.get` AttributeError, который не перехватывался
//!      (ловится только OSError) и мог уронить процесс заставки посреди
//!      ожидания EOF. Здесь такие строки пропускаются.
//!   3. `scan_startup_movies`: у файла инвентаря без «_» id брался из
//!      ПОЛНОГО имени с расширением («weird.webm» → id/имя с «.webm»).
//!      Здесь id берётся из стема («weird»); для нормальных имён
//!      `<id>_<hash>.webm` поведение не меняется.
//!   4. `_startup_movie_names`: `communityitemid: null` в JSON давал в
//!      Python ключ-строку `"None"` (str(None)); здесь null/отсутствие —
//!      пропуск записи, как и пустая строка.
//!   5. Пути пользовательских конфигов игнорировали `$XDG_CONFIG_HOME`:
//!      `~/.config` был вшит в `CONFIG_PATH`, `AUTOSTART_PATH`,
//!      `SYSTEMD_UNIT_PATH`, ручной симлинк `plasma-workspace.target.wants`
//!      и `kwinrulesrc`, хотя соседний `_ksplashrc_get` переменную уважал.
//!      При нестандартном `XDG_CONFIG_HOME` юнит и `.desktop` писались
//!      туда, где systemd/XDG их не ищут (заставка молча не включалась), а
//!      правило KWin — мимо живого `kwinrulesrc`. Корень теперь один на
//!      весь проект — [`crate::we_client::config_home`] (тот же класс
//!      дефекта уже чинился там же, правило #12).
//!   6. `preview_frame`: имя файла кэша кадра витрины считалось питоновским
//!      `hash((abspath, mtime))`, а хеш строк в CPython солится
//!      PYTHONHASHSEED и РАЗНЫЙ в каждом процессе — попадания в кэш не было
//!      никогда, ffmpeg запускался на каждое открытие витрины, а каталог
//!      кэша копил мусор. Здесь отпечаток детерминированный (FNV-1a).
//!   7. `_appinfo_names`: рекурсивный разбор дерева на битом appinfo.vdf
//!      упирался в RecursionError, а перехватывались только ValueError/
//!      IndexError/struct.error — процесс падал. Здесь глубина ограничена.
//!
//! ⚠️ Самолечение связки ksplashrc (09.09.2026, доказано живьём). Гибрид
//! держится на ОДНОЙ строке чужого файла — `Theme=` в группе `[KSplash]`
//! пользовательского `ksplashrc`. Этот ключ пишем не только мы: его
//! перезаписывает применение глобальной темы Plasma (KCM «Глобальная
//! тема» / `plasma-apply-lookandfeel`, а также своя же вкладка
//! «Оформление» — `ainst_lnf_component_plan`), а бывает и просто теряет,
//! оставляя файл пустым. Тогда KSplash молча берёт тему из глобальной
//! (`LookAndFeelPackage` в kdeglobals) и при входе на секунду показывает
//! ШТАТНУЮ заставку Plasma — чёрный фон с логотипом KDE, — после чего
//! поверх неё выпрыгивает наш ролик: бесшовная передача KSplash → mpv
//! умирает, а конфиг гибрида, юнит и тема при этом целы, так что ни один
//! признак «сломалось» наружу не выходит. Чинить руками бесполезно —
//! следующее применение темы сломает снова. Поэтому реальный прогон
//! заставки (`play`, ветка `blocking && hybrid` — то есть запуск из
//! systemd-юнита при входе) СНАЧАЛА восстанавливает связку и только потом
//! играет: нет каталога темы — перегенерировать, стоит чужая тема —
//! вернуть свою (чужое значение при этом уходит в
//! `prev_ksplash_theme`/`prev_ksplash_engine`, чтобы откат гибрида отдал
//! покупателю его НАСТОЯЩУЮ тему, а не нашу). Неудача самолечения — только
//! строка в журнал: ролик показать важнее.
//!
//! ⚠️ У самолечения есть обратная сторона, и она закрыта ЯВНЫМ ВОПРОСОМ, а
//! не умолчанием: во вкладке «Оформление» покупатель волен выбрать СВОЮ
//! заставку KDE, и молчаливый возврат нашей темы отменил бы его выбор.
//! Разбор спора за слот — [`splash_slot_conflict`] ниже.

use crate::i18n::{n_, tr, trf};
use crate::we_client::config_home;
use serde_json::{Map, Value};

// ============================================================================
// Константы и пути (порт модульных констант; $HOME — параметром)
// ============================================================================

/// ID LnF-темы KSplash гибрида — порт `KSPLASH_THEME_ID`.
pub const KSPLASH_THEME_ID: &str = "org.rexon.sessionsplash";
/// Имя systemd user-юнита — порт `SYSTEMD_UNIT_NAME`.
pub const SYSTEMD_UNIT_NAME: &str = "rexon-session-splash.service";
/// app_id окна mpv на Wayland (правило KWin матчится по нему) — порт
/// `_KWIN_APP_ID`.
pub const KWIN_APP_ID: &str = "rexon-session-splash";
/// Имя Rust-хелпера Wayland-оверлея.
pub const SPLASH_OVERLAY_BINARY: &str = "rexon-splash-overlay";

/// Человеческие имена встроенных стартовых видео Steam — порт
/// `_BUILTIN_NAMES` (порядок = порядок обхода при скане). Имя — `msgid`,
/// переводится при скане (`tr`); латинские имена в каталог не попадают.
pub const BUILTIN_NAMES: &[(&str, &str)] = &[
    // TRANSLATORS: name of Steam's standard built-in startup movie.
    ("deck_startup", n_("Steam Deck (Default)")),
    ("oled_startup", "Steam Deck OLED"),
    ("bigpicture_startup", "Big Picture"),
];

/// Бинари, обязательные для гибрида — порт списка в `hybrid_supported`.
pub const HYBRID_REQUIRED_BINARIES: &[&str] =
    &["ffmpeg", "kwriteconfig6", "kreadconfig6", "ksplashqml", "systemctl"];

/// request_id запроса `get_property` в mpv-IPC — порт литерала 700.
pub const MPV_IPC_REQUEST_ID: i64 = 700;

// ── Таймауты ожиданий (сами циклы — в исполнителе) ─────────────────────────

/// Ожидание IPC-сокета mpv в эстафете гибрида (`< TimeoutStartSec=15`).
pub const HYBRID_SOCK_WAIT_SEC: f64 = 6.0;
/// Ожидание появления ksplashqml после нашего старта.
pub const KSPLASH_APPEAR_WAIT_SEC: f64 = 3.0;
/// Потолок ожидания смерти ksplashqml (его свои страховки — 30 c + 40 c).
pub const KSPLASH_DEATH_WAIT_SEC: f64 = 45.0;
/// Потолок прогона видео (mpv/хелпер), после — принудительное гашение.
pub const PLAY_TIMEOUT_SEC: f64 = 120.0;
/// Ранняя диагностика хелпера: сокет не поднялся за это время → фолбэк.
/// Крах ловится отдельно и мгновенно, здесь ждём «жив, но не готов»:
/// на холодном первом входе (пустой кэш libmpv/EGL/glslang, холодный GPU,
/// первая компиляция шейдеров) инициализация раздувается, а убийство
/// живого хелпера роняет заставку в дырявый оконный фолбэк, где панель
/// пробивается поверх (инцидент 2026-07-25). Задержки не видно — всё это
/// время ksplashqml держит постер. Инвариант: `<= HYBRID_SOCK_WAIT_SEC`,
/// а худший случай до READY (6 + 6 = 12 c) < `TimeoutStartSec=15`.
pub const HELPER_SOCK_WAIT_SEC: f64 = 6.0;
/// Ожидание композитора на холодном X11-входе (`wait_compositor`).
pub const COMPOSITOR_WAIT_SEC: f64 = 2.5;
/// Страховочный интервал raise-цикла X11-слоя.
pub const RAISE_INTERVAL_SEC: f64 = 0.5;

/// `$XDG_CONFIG_HOME/rexon/session_splash.json` — порт `CONFIG_PATH`
/// (починка №5: корень конфигов уважает переменную).
pub fn config_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/rexon/session_splash.json")
}

/// `$XDG_CONFIG_HOME/autostart/rexon-session-splash.desktop` — порт
/// `AUTOSTART_PATH` (починка №5).
pub fn autostart_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/autostart/rexon-session-splash.desktop")
}

/// Каталог LnF-темы гибрида — порт `LNF_THEME_DIR`.
pub fn lnf_theme_dir(home: &str) -> String {
    format!("{home}/.local/share/plasma/look-and-feel/{KSPLASH_THEME_ID}")
}

/// Путь systemd user-юнита — порт `SYSTEMD_UNIT_PATH` (починка №5:
/// при нестандартном `$XDG_CONFIG_HOME` юнит писался туда, где systemd
/// его не ищет, и `enable` не находил юнит).
pub fn systemd_unit_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/systemd/user/{SYSTEMD_UNIT_NAME}")
}

/// Каталог ручного enable-фолбэка `plasma-workspace.target.wants`.
pub fn systemd_wants_dir(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/systemd/user/plasma-workspace.target.wants")
}

/// Ручной симлинк enable-фолбэка в `plasma-workspace.target.wants`.
pub fn systemd_wants_link(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/{SYSTEMD_UNIT_NAME}",
        systemd_wants_dir(xdg_config_home, home)
    )
}

/// Кандидаты каталога Steam — порт кортежа в `steam_root` (исполнитель
/// прогоняет их через realpath и проверяет наличие `config/`).
pub fn steam_root_candidates(home: &str) -> Vec<String> {
    vec![
        format!("{home}/.local/share/Steam"),
        format!("{home}/.steam/steam"),
        format!("{home}/.steam/root"),
    ]
}

/// Путь пользовательского ksplashrc — порт вычисления в `_ksplashrc_get`
/// (`$XDG_CONFIG_HOME` или `~/.config`; пустая строка = не задан).
pub fn ksplashrc_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/ksplashrc")
}

/// Путь kwinrulesrc — порт `os.path.expanduser("~/.config/kwinrulesrc")`
/// (починка №5: правило KWin писалось мимо живого файла при нестандартном
/// `$XDG_CONFIG_HOME`).
pub fn kwinrulesrc_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/kwinrulesrc")
}

/// Runtime-каталог — порт `_runtime_dir` (`$XDG_RUNTIME_DIR` или `/tmp`;
/// пустая строка — falsy, как в Python).
pub fn runtime_dir(xdg_runtime_dir: Option<&str>) -> String {
    match xdg_runtime_dir {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => "/tmp".to_string(),
    }
}

/// flock-файл «один сплеш на сессию» — порт `_acquire_play_lock`.
pub fn play_lock_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon-session-splash.lock")
}

/// IPC-сокет mpv/хелпера — порт `ipc_sock` в `play`.
pub fn ipc_sock_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon-splash-mpv.sock")
}

/// Свежевыжатый постер в runtime — порт `out` в `_poster_for_video`.
pub fn runtime_poster_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon-splash-poster.png")
}

/// Готовый постер темы KSplash — порт `theme_poster` в `_poster_for_video`.
pub fn theme_poster_path(home: &str) -> String {
    format!("{}/contents/splash/images/poster.png", lnf_theme_dir(home))
}

// ============================================================================
// Steam: имена из localconfig.vdf + скан заставок
// ============================================================================

/// Python-истинность JSON-значения (`or`-цепочки, `if not x`).
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

/// `dict.get(key, default)` — значение, если ключ есть (даже null).
fn get_or(obj: &Value, key: &str, default: Value) -> Value {
    obj.get(key).cloned().unwrap_or(default)
}

/// Найти в тексте VDF значение `"GetStartupMovies"` — семантика регекса
/// `"GetStartupMovies"\s*"(.*?)"\s*[\r\n]` (re.S, lazy): закрывающая кавычка —
/// первая, за которой идут только пробельные символы до `\r`/`\n`.
fn find_startup_movies_raw(txt: &str) -> Option<&str> {
    const NEEDLE: &str = "\"GetStartupMovies\"";
    let bytes = txt.as_bytes();
    let mut from = 0;
    while let Some(rel) = txt[from..].find(NEEDLE) {
        let mut i = from + rel + NEEDLE.len();
        // \s* до открывающей кавычки
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while let Some(rel_q) = txt[j..].find('"') {
                let q = j + rel_q;
                // после кавычки: \s*[\r\n]
                let mut k = q + 1;
                loop {
                    if k >= bytes.len() {
                        break; // EOF без \r\n → эта кавычка не подходит
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

/// communityitemid → {name, item_title, appid} из текста localconfig.vdf —
/// порт разбора ОДНОГО файла в `_startup_movie_names` (обход userdata и
/// слияние по пользователям — у исполнителя). Нет ключа/битый JSON → пусто.
pub fn parse_startup_movie_names(vdf_text: &str) -> Value {
    let mut out = Map::new();
    let Some(raw) = find_startup_movies_raw(vdf_text) else {
        return Value::Object(out);
    };
    // VDF-строка: внутренние кавычки экранированы \" → распакуем в JSON
    // (последовательные replace — как в оригинале).
    let unescaped = raw.replace("\\\"", "\"").replace("\\\\", "\\");
    let Ok(parsed) = serde_json::from_str::<Value>(&unescaped) else {
        return Value::Object(out);
    };
    let Some(entries) = parsed.as_array() else {
        return Value::Object(out);
    };
    for e in entries {
        // Порт `str(e.get("communityitemid", ""))`; починка №4: null →
        // пропуск (в Python получался ключ "None").
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

/// Порт `os.path.splitext(...)[1].lower()`: расширение с точкой; ведущая
/// точка скрытого файла расширением не считается.
fn splitext_ext_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(pos) if pos > 0 && !name[..pos].chars().all(|c| c == '.') => {
            name[pos..].to_lowercase()
        }
        _ => String::new(),
    }
}

/// Доступные стартовые видео — порт `scan_startup_movies`, наблюдения
/// параметрами: `builtin_present` — имена файлов в `<root>/steamui/movies`,
/// `inventory_files` — листинг `<root>/config/communityitemscache/
/// startupmovies` (несортированный, сортировка здесь — как `sorted()`),
/// `names` — результат [`parse_startup_movie_names`] (слитый по юзерам).
/// Каждый элемент: `{id, name, path, builtin}`.
pub fn scan_movies(
    root: &str,
    builtin_present: &[String],
    inventory_files: &[String],
    names: &Value,
) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    // Встроенные стартовые видео.
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

    // Инвентарь (Points Shop) — с человеческими именами из кэша.
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
        // Починка №3: id из СТЕМА (без расширения) — в Python
        // `fn.split("_", 1)[0]` у имени без «_» захватывал «.webm».
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
    result
}

// ============================================================================
// Витрина заставок: метаданные предмета Steam, постер, избранное, ротация
// ============================================================================

/// Ролик, встроенный в клиент Steam.
pub const ORIGIN_BUILTIN: &str = "builtin";
/// Ролик, купленный в Points Shop (инвентарь).
pub const ORIGIN_SHOP: &str = "shop";
/// Файл, выбранный покупателем вручную.
pub const ORIGIN_CUSTOM: &str = "custom";

/// Человеческая подпись происхождения — без жаргона (правило #13).
/// Неизвестное значение даёт пустую строку: пустая метка в карточке не
/// рисуется, а «origin=xyz» покупателю не объяснить.
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

/// communityitemid → ПОЛНАЯ запись предмета из кэша GetStartupMovies.
///
/// Отличие от [`parse_startup_movie_names`]: отдаёт запись целиком (обложка,
/// item_class, локализованный заголовок) — витрине нужны все поля, а формат
/// скана менять нельзя. Обход `userdata/<id>/` и слияние — у исполнителя.
///
/// Записи с `communityitemid` пустым, `null` или строкой «none» (в любом
/// регистре) пропускаются: у них нет ключа, по которому витрина найдёт
/// метаданные.
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

// ── appcache/appinfo.vdf: appid → название игры ───────────────────────

/// Версия appinfo.vdf со строковой таблицей (ключи — индексы в ней).
const APPINFO_MAGIC_STRING_TABLE: u32 = 0x0756_4429;
/// Версии, где ключи лежат строками прямо в дереве.
const APPINFO_MAGIC_INLINE: [u32; 2] = [0x0756_4427, 0x0756_4428];
/// Служебный заголовок записи приложения перед деревом ключей-значений.
const APPINFO_APP_HEADER: usize = 60;
/// Предел вложенности дерева. У appinfo.vdf реальная глубина — единицы;
/// ⚠️ починка: в Python рекурсия по битому файлу упиралась в
/// RecursionError, который НЕ перехватывался (ловились только ValueError/
/// IndexError/struct.error) и ронял процесс — здесь разбор просто
/// останавливается.
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

/// Строка до нулевого байта начиная с `pos` → (строка, позиция за нулём).
/// Нет нуля до конца — None (в Python это ValueError от `bytes.index`).
fn cstr_at(data: &[u8], pos: usize) -> Option<(String, usize)> {
    let rest = data.get(pos..)?;
    let end = rest.iter().position(|b| *b == 0)?;
    Some((
        String::from_utf8_lossy(&rest[..end]).into_owned(),
        pos + end + 1,
    ))
}

/// Строковая таблица версии 0x07564429: количество + подряд C-строки.
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

/// Ключ поля: индекс в строковой таблице либо C-строка на месте.
fn appinfo_key(
    blob: &[u8],
    pos: usize,
    strings: Option<&[String]>,
) -> Option<(String, usize)> {
    match strings {
        Some(tbl) => {
            let idx = le_u32(blob, pos)? as usize;
            // Индекс за пределами таблицы — пустой ключ (как в Python).
            Some((tbl.get(idx).cloned().unwrap_or_default(), pos + 4))
        }
        None => cstr_at(blob, pos),
    }
}

/// Название приложения из дерева ключей-значений: первое поле `name` не
/// глубже второго уровня (`common/name`). Заодно двигает курсор — дерево
/// приходится проходить целиком, чтобы понять, где кончается раздел.
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
            // Конец раздела.
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
            // Неизвестный тип — дальше идти нельзя, смещения поедут.
            _ => return Some((found, blob.len())),
        }
    }
    Some((found, pos))
}

/// appid → название игры из байтов `appcache/appinfo.vdf`.
///
/// Двоичный VDF Steam-клиента: заголовок (magic + universe), у версии
/// 0x07564429 — ещё смещение строковой таблицы, дальше подряд записи
/// «appid, размер, 60 байт служебного заголовка, дерево». Разбираем ТОЛЬКО
/// дерево запрошенных appid'ов — файл на 8 МБ целиком парсить незачем.
///
/// Нужно для подписи «Видеозаставка из <игра>» БЕЗ сети и Steam Web API:
/// покупатель может сидеть офлайн. Незнакомая версия/оборванная запись →
/// то, что успели разобрать (в Python — тот же best-effort).
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
            // Таблица не разобрана — без неё ключи не прочитать.
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
            // Срез как в Python: границы подрезаются по длине буфера,
            // «конец раньше начала» даёт пустой срез, а не панику.
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

// ── Обогащение записей скана метаданными предмета ─────────────────────

/// `int(x)` для JSON-значения там, где Python принимает и число, и строку.
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

/// Первое «истинное» строковое значение из перечисленных ключей.
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

/// Дополнить записи скана метаданными предмета Steam (всё локально).
///
/// Единая точка обогащения для ОБОИХ маршрутов скана (мост и Python) —
/// иначе они разъедутся. Добавляет: title (крупная подпись), game
/// (игра-источник), appid, origin/origin_label, image (обложка предмета на
/// CDN сообщества), item_class. Отсутствующие данные остаются пустыми
/// строками — карточка их просто не рисует (правило #13).
///
/// `items` — результат [`parse_startup_movie_items`], `names` — результат
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
        let origin = if builtin { ORIGIN_BUILTIN } else { ORIGIN_SHOP };
        rec.insert("origin".into(), Value::from(origin));
        rec.insert("origin_label".into(), Value::from(origin_label(origin)));

        // Ключ метаданных — id записи скана: у инвентаря это
        // communityitemid, у встроенных ролика такого предмета нет.
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

        // Локализованный item_title лучше английского name: интерфейс
        // покупателя русский (правило #13).
        let mut title = first_truthy_str(meta, &["item_title", "name"]);
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

/// Запись витрины для файла, выбранного покупателем вручную. `basename` и
/// `stem` (имя без расширения) считает исполнитель — он же владеет путём.
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

// ── Кадр для витрины (ffprobe/ffmpeg) ─────────────────────────────────

/// Доля длительности, на которой берём «представительный» кадр: к этому
/// моменту заставка дорисовала логотип, но ещё не начала гаснуть.
pub const PREVIEW_AT: f64 = 0.75;

/// Каталог кэша кадров витрины. ⚠️ НЕ путать с постером гибрида KSplash
/// ([`poster_ffmpeg_argv`]) — тому нужен строго ПЕРВЫЙ кадр, иначе
/// эстафета ksplash→mpv перестанет быть бесшовной; витрине первый кадр
/// бесполезен, почти у всех роликов он чёрный.
pub fn preview_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    let base = match xdg_cache_home {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => format!("{home}/.cache"),
    };
    format!("{base}/rexon/steam_splash_preview")
}

/// argv ffprobe для длительности ролика.
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

/// Длительность из вывода ffprobe: отрицательные и нечисловые → 0.0.
pub fn parse_duration(stdout: &str) -> f64 {
    stdout.trim().parse::<f64>().map(|v| v.max(0.0)).unwrap_or(0.0)
}

/// FNV-1a 64 — детерминированный отпечаток для имени файла кэша.
///
/// ⚠️ Починка бага оригинала: ключ кэша считался питоновским
/// `hash((abspath, mtime))`, а хеш строк в CPython солится PYTHONHASHSEED и
/// РАЗНЫЙ в каждом процессе. Имя файла кэша поэтому не совпадало с
/// прошлым запуском: попадания в кэш не было НИКОГДА, ffmpeg дёргался на
/// каждое открытие витрины, а каталог копил мусор. Здесь отпечаток
/// стабильный, кэш начинает работать.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Санитайзер имени для файловой системы: всё, кроме букв, цифр, `_`, `.`
/// и `-`, заменяется на `_`; берутся последние 64 символа. Семантика
/// `re.sub(r"[^\w.-]", "_", name)[-64:]` (Python `\w` Unicode-aware, здесь
/// это `char::is_alphanumeric` + `_`).
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

/// Путь файла кэша кадра. Ключ — путь + mtime, поэтому подменённый ролик
/// перегенерируется сам.
pub fn preview_cache_path(cache_dir: &str, video_abs: &str, mtime: i64) -> String {
    let basename = video_abs.rsplit('/').next().unwrap_or(video_abs);
    let key = preview_key(basename);
    let digest = fnv1a64(format!("{video_abs}\0{mtime}").as_bytes());
    format!("{cache_dir}/{key}.{digest:x}.png")
}

/// argv ffmpeg для кадра витрины. `seek` ≤ 0.05 c — без `-ss` (ролик
/// короче ожидаемого): так же ведёт себя повторная попытка «первый кадр».
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

// ── Избранное и ротация роликов ───────────────────────────────────────

/// Пути роликов со звёздочкой из конфига (не-строки отбрасываются).
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

/// Конфиг с обновлённой звёздочкой → (новый конфиг, актуальный список).
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

/// Включена ли «случайная заставка при каждом входе».
pub fn rotation_enabled(cfg: &Value) -> bool {
    cfg.get("rotate").map(truthy).unwrap_or(false)
}

/// Конфиг с переключённой ротацией.
pub fn config_with_rotation(cfg: &Value, enabled: bool) -> Value {
    let mut obj = cfg.as_object().cloned().unwrap_or_default();
    obj.insert("rotate".into(), Value::from(enabled));
    Value::Object(obj)
}

/// Сколько роликов нужно, чтобы «каждый вход — новый ролик» вообще что-то
/// меняло.
///
/// ⭐ Одна правда на всех. Число «два» стояло литералом в пяти местах: выбор
/// пула, жребий, короткое замыкание скана, чип состояния вкладки «Steam» и
/// отказ её переключателя (в Python — ещё и `len(self._sv_items) < 2`).
/// Разойдись они — чип обещал бы ротацию, которой при входе не случится.
/// Спрашивать через функции ниже, а не сравнивать с числом на месте.
pub const ROTATION_MIN_POOL: usize = 2;

/// Крутится ли избранное (а не все найденные ролики).
pub fn rotation_uses_favorites(favorites: usize) -> bool {
    favorites >= ROTATION_MIN_POOL
}

/// Размер пула ротации по числу избранных и всех найденных роликов — то же
/// решение, что [`rotation_pool`], только на числах (витрине файлы не нужны).
pub fn rotation_pool_size(favorites: usize, all: usize) -> usize {
    if rotation_uses_favorites(favorites) {
        favorites
    } else {
        all
    }
}

/// Будет ли ротация на самом деле менять ролик при входе.
pub fn rotation_possible(favorites: usize, all: usize) -> bool {
    rotation_pool_size(favorites, all) >= ROTATION_MIN_POOL
}

/// Из чего крутим: избранное, а если его меньше двух — всё найденное.
/// Существование файлов проверяет исполнитель (I/O-граница).
///
/// Защита от дурака (правило #13): включённая ротация с ОДНОЙ звёздочкой не
/// должна молча превращаться в «каждый вход один и тот же ролик».
pub fn rotation_pool(existing_favorites: &[String], existing_all: &[String]) -> Vec<String> {
    if rotation_uses_favorites(existing_favorites.len()) {
        return existing_favorites.to_vec();
    }
    existing_all.to_vec()
}

/// Из чего выбирать ролик на СЛЕДУЮЩИЙ вход: пул без текущего, а если
/// после исключения ничего не осталось — весь пул. Пустой список = крутить
/// нечего (пул меньше двух элементов); сам жребий бросает исполнитель —
/// генератор случайных чисел это I/O-граница.
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
// Конфиг (какой webm + звук + служебные ключи гибрида)
// ============================================================================

/// Порт `load_config`: текст файла → dict; нет файла/битый JSON/не-dict → {}.
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

/// Порт `save_config`: обновить `path`/`sound`, НЕ перезаписывая служебные
/// ключи (prev_ksplash_theme и т.п.). Возвращает новый конфиг.
pub fn config_with_selection(cfg: &Value, path: &str, sound: bool) -> Value {
    let mut m = cfg.as_object().cloned().unwrap_or_default();
    m.insert("path".into(), Value::from(path));
    m.insert("sound".into(), Value::from(sound));
    Value::Object(m)
}

/// Порт `save_hybrid_pref`: запомнить галку «гибрид вкл/выкл».
pub fn config_with_hybrid_pref(cfg: &Value, enabled: bool) -> Value {
    let mut m = cfg.as_object().cloned().unwrap_or_default();
    m.insert("hybrid".into(), Value::from(enabled));
    Value::Object(m)
}

/// Сериализация конфига — `json.dump(cfg, ensure_ascii=False, indent=2)`
/// (порт `_write_config`; атомарная запись .tmp+rename — у исполнителя).
pub fn config_json(cfg: &Value) -> String {
    serde_json::to_string_pretty(cfg).expect("Value сериализуется")
}

/// Порт `hybrid_installed`: флаг в конфиге И юнит-файл на месте.
pub fn hybrid_installed(cfg: &Value, unit_file_exists: bool) -> bool {
    truthy(&get_or(cfg, "hybrid_installed", Value::Null)) && unit_file_exists
}

/// Обновлять ли постер темы при сохранении выбора — порт условия в
/// `save_config` (гибрид установлен и каталог темы существует).
pub fn should_refresh_hybrid_poster(cfg: &Value, theme_dir_exists: bool) -> bool {
    truthy(&get_or(cfg, "hybrid_installed", Value::Null)) && theme_dir_exists
}

// ============================================================================
// Гибрид KSplash: тема-постер, ksplashrc, systemd-юнит
// ============================================================================

/// Splash.qml темы-постера — порт `_SPLASH_QML` (без Kirigami, мгновенный
/// показ первого кадра ролика с PreserveAspectFit на чёрном).
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

/// metadata.json LnF-пакета — порт `meta` в `_write_lnf_theme`
/// (`json.dump(..., ensure_ascii=False, indent=4)`).
pub fn lnf_metadata_json() -> String {
    // Описание — на всех языках каталога (`Description[ru]`): его показывают
    // «Параметры системы» KDE, и язык они выбирают сами по локали сеанса.
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
    // indent=4 — как в оригинале (у json.dump там 4 пробела, не 2).
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(&meta, &mut ser).expect("Value сериализуется");
    String::from_utf8(buf).expect("JSON — валидный UTF-8")
}

/// argv ffmpeg «первый кадр → PNG» — порт cmd в `_extract_poster`.
/// `ffmpeg` — путь из `shutil.which` (наблюдение исполнителя).
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

/// Порт `_ksplashrc_get`: значение ключа из группы `[KSplash]`
/// ПОЛЬЗОВАТЕЛЬСКОГО ksplashrc (текст файла параметром). Намеренно не
/// kconfig-каскад: для бэкапа/отката важен факт «ключа не было» ('').
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

/// argv kwriteconfig6 — порт `_ksplashrc_set` (пустое value = удалить ключ).
/// `exe` — путь из `shutil.which("kwriteconfig6")`.
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

/// argv `systemctl --user …` — порт `_systemctl_user`.
pub fn systemctl_user_argv(exe: &str, args: &[&str]) -> Vec<String> {
    let mut cmd: Vec<String> = vec![exe.into(), "--user".into()];
    cmd.extend(args.iter().map(|a| (*a).to_string()));
    cmd
}

/// Текст systemd user-юнита — порт `_systemd_unit_content`.
///
/// After=kwin (обе ветки, несуществующая игнорируется), Type=notify +
/// Before=plasma-plasmashell.service — plasmashell не стартует, пока mpv не
/// поднял IPC-сокет (READY=1), поэтому ksplash не гаснет раньше первого
/// кадра. ⚠️ НЕ добавлять After=plasma-ksplash.service — взаимная
/// блокировка (oneshot-job завершается ВЫХОДОМ ksplashqml; инцидент
/// 2026-07-16, Wayland-вход виснул на чёрном сплэше).
///
/// `Description` — по-английски и не из каталога: у systemd нет
/// локализованных ключей (`Description[ru]` он не знает), а видна строка
/// только в `systemctl`/журнале. Подписи для человека — в записи автозапуска.
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
         # страховки: сломанный mpv задержит вход максимум на ~15 с\n\
         # (overlay-порог 6 c + фолбэк-mpv 6 c = 12 c до READY, + слак).\n\
         TimeoutStartSec=15\n\
         TimeoutStopSec=5\n\
         \n\
         [Install]\n\
         WantedBy=plasma-workspace.target\n"
    )
}

/// Команда запуска заставки при Python-входе — порт `_launcher_exec`.
/// `rexon_path` — результат `shutil.which("rexon")`; пусто — заводить
/// нечем, и тогда строка ПУСТАЯ: писать её нельзя.
///
/// ⛔`python_executable` больше не участвует ни в одном ответе. Прежде на
/// пустом `rexon_path` отсюда выходило `python3 -m rexon.app
/// --session-splash` — строка, которую `.desktop`-автозапуск и
/// systemd-юнит выполняют без рабочего каталога, а пакета `rexon` в
/// `sys.path` системного интерпретатора нет: заставка при входе молча не
/// показывалась. Выбор целиком переехал в [`crate::app::launch_program`].
/// ⚠️Параметр оставлен подписи ради — её держит мост `rexon-py`
/// (`ss_launcher_exec`); уйдёт вместе с мостом.
pub fn launcher_exec(rexon_path: Option<&str>, _python_executable: &str) -> String {
    crate::app::launch_command(
        &crate::installer::LaunchEntry::Python,
        rexon_path,
        &[SESSION_SPLASH_FLAG],
    )
    .unwrap_or_default()
}

/// Флаг маршрута заставки при входе (его разбирает `app::route`).
pub const SESSION_SPLASH_FLAG: &str = "--session-splash";

/// Команда запуска заставки для `Exec=` автозапуска (формат Desktop Entry).
///
/// ⚠️ Вход выбирает ТА ЖЕ функция, что у ярлыка и лотка
/// ([`crate::installer::launch_entry_live`]), — сюда он приходит готовым.
/// Rust-вход — бинарь окна с [`SESSION_SPLASH_FLAG`]: маршрут отрабатывает
/// ядро без Qt (`cli::run_headless`), интерпретатор при входе не нужен.
/// Python-вход (откат) — лаунчер `rexon` полным путём.
///
/// Пустая строка — заводить нечем ([`crate::app::launch_program`]); писать
/// её в `Exec=` нельзя, вызывающий обязан отказать вслух.
/// ⚠️`python_executable` не используется — см. [`launcher_exec`].
pub fn desktop_launcher_exec(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    _python_executable: &str,
) -> String {
    crate::app::launch_command(entry, rexon_path, &[SESSION_SPLASH_FLAG]).unwrap_or_default()
}

/// То же для `ExecStart=` systemd-юнита гибрида.
///
/// Отдельно от [`desktop_launcher_exec`], потому что правила кавычек у
/// systemd другие: у Desktop Entry `\`, `$`, `` ` `` и `"` внутри кавычек
/// удваивают косую ещё раз, а systemd прочёл бы лишнюю косую буквально.
pub fn unit_launcher_exec(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    _python_executable: &str,
) -> String {
    crate::app::unit_launch_command(entry, rexon_path, &[SESSION_SPLASH_FLAG]).unwrap_or_default()
}

/// Аргумент командной строки `ExecStart=` по `systemd.service(5)`:
/// `%` — спецификатор (`%%`), `$` — подстановка переменной (`$$`); слово с
/// пробелом, кавычкой, `\` или `;` берётся в двойные кавычки, внутри
/// которых `"` и `\` экранируются косой.
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

/// «KDE ли десктоп» — порт `"KDE" in XDG_CURRENT_DESKTOP.upper()`.
pub fn is_kde_desktop(xdg_current_desktop: Option<&str>) -> bool {
    xdg_current_desktop
        .unwrap_or("")
        .to_uppercase()
        .contains("KDE")
}

/// Решение `hybrid_supported`: KDE + все бинари + активный
/// plasma-workspace.target (наличие бинарей и is-active — наблюдения).
pub fn hybrid_supported_decision(
    xdg_current_desktop: Option<&str>,
    all_binaries_present: bool,
    plasma_target_active: bool,
) -> bool {
    is_kde_desktop(xdg_current_desktop) && all_binaries_present && plasma_target_active
}

/// Бэкапить ли текущую тему ksplashrc перед установкой гибрида — порт
/// условия в `_install_hybrid` (повторное включение не затирает бэкап).
pub fn should_backup_prev_theme(cur_theme: &str) -> bool {
    cur_theme != KSPLASH_THEME_ID
}

/// Откатывать ли ksplashrc при снятии гибрида — порт условия в
/// `_uninstall_hybrid` (чужую тему, выбранную позже, не трогаем).
pub fn should_rollback_ksplashrc(cur_theme: &str) -> bool {
    cur_theme == KSPLASH_THEME_ID
}

/// Чинить ли связку ksplashrc перед реальным показом заставки: гибрид
/// числится установленным, а в `[KSplash] Theme` стоит НЕ наша тема (в том
/// числе пусто — ключ потеряли, и KSplash возьмёт тему из глобальной).
///
/// Причина самолечения — в шапке модуля: ключ перезаписывает любое
/// применение глобальной темы Plasma, и вход молча теряет бесшовность.
pub fn should_heal_ksplashrc(cfg: &Value, cur_theme: &str) -> bool {
    truthy(&get_or(cfg, "hybrid_installed", Value::Null)) && cur_theme != KSPLASH_THEME_ID
}

// ============================================================================
// Спор за слот KSplash: видеозаставка входа против заставки KDE
// ============================================================================
//
// ⚠️ Место при входе ОДНО. Самолечение выше возвращает нашу тему в
// `ksplashrc` при каждом входе — и ровно поэтому оно обязано знать про
// вкладку «Оформление»: там покупатель может выбрать СВОЮ заставку KDE
// (`ainst::lnf_component_plan`, вид `splash`) или оформление целиком
// (`global`, его применяет `plasma-apply-lookandfeel` — тот же ключ). Не
// спроси мы ничего — человек выбрал бы заставку, а при следующем входе
// продукт МОЛЧА вернул бы свою: осознанный выбор покупателя отменён без
// единого слова, и виноватым выглядит его собственный выбор.
//
// Поэтому здесь не «умное» правило, а честный вопрос с двумя исходами:
// место остаётся за роликом либо уходит выбранной заставке, и тогда гибрид
// снимается по-настоящему (`uninstall_hybrid`), чтобы возвращать было
// нечему. Решение и все тексты — тут; интерфейсу остаётся показать вопрос
// и исполнить ответ.

/// Займёт ли применение этого вида оформления слот заставки KSplash.
///
/// `splash` пишет `ksplashrc[KSplash]Theme` напрямую, `global` — тот же ключ
/// руками `plasma-apply-lookandfeel`. Остальные виды (курсоры, иконки, GTK,
/// обои, экран блокировки) слота не касаются, и спрашивать о них нечего.
pub fn apply_kind_takes_ksplash_slot(apply_kind: &str) -> bool {
    matches!(apply_kind, "splash" | "global")
}

/// Вопрос покупателю и оба его исхода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashSlotConflict {
    /// Заголовок окна вопроса.
    pub title: &'static str,
    /// Объяснение и сам вопрос. «Да» — отдать место выбранной заставке,
    /// «Нет» — оставить видеозаставку Rexon.
    pub question: String,
    /// Что сказать покупателю, когда он оставил видеозаставку.
    pub kept: String,
    /// Можно ли применить ОСТАЛЬНОЕ, оставив видеозаставку.
    ///
    /// У целого оформления заставка — лишь одна из частей, и отказывать во
    /// всей теме из-за неё незачем: тема применяется, а слот заставки мы
    /// сразу возвращаем себе (`heal_hybrid`). У вида `splash` возвращать
    /// нечего — там вся суть действия и есть заставка, поэтому «Нет»
    /// означает «не применять».
    pub keep_applies_rest: bool,
}

/// Заголовок вопроса о слоте.
///
/// `&'static str`, а не `String`: поле [`SplashSlotConflict::title`] так
/// объявлено, и хозяева окна берут его как есть. Перевод на каждый язык
/// процесса оседает один раз (языков — единицы, утечка ограничена).
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

/// Спорит ли применение оформления с видеозаставкой входа.
///
/// `None` — спорить не о чем: вид оформления слота не касается, гибрид не
/// установлен, либо применяют саму нашу тему-постер.
pub fn splash_slot_conflict(
    apply_kind: &str,
    theme_name: &str,
    hybrid_installed: bool,
) -> Option<SplashSlotConflict> {
    if !hybrid_installed || !apply_kind_takes_ksplash_slot(apply_kind) {
        return None;
    }
    // Наша же тема-постер: она и так стоит в слоте, отбирать место не у кого.
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
    // Плитка «Нет» — выключить заставку KDE совсем. Ролик тоже держится на
    // ней (тема-постер), поэтому спросить надо, но другими словами: имени
    // «None» человек нигде не видел.
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

/// Покупатель оставил видеозаставку, а остальное оформление применено.
pub fn msg_slot_kept_video() -> String {
    tr("The splash screen was left alone — the Rexon startup movie still plays at login.")
}

/// Покупатель оставил видеозаставку, выбранная заставка НЕ применена.
pub fn msg_slot_kept_instead_of(theme_name: &str) -> String {
    trf(
        "The “%1” splash screen wasn't applied — the Rexon startup movie stays at login.",
        &[&theme_name],
    )
}

/// Покупатель отдал место выбранной заставке — гибрид снят.
pub fn msg_slot_released() -> String {
    tr("The Rexon startup movie is off — the chosen splash screen now shows at login.")
}

/// Место отдать не вышло: гибрид не снялся.
///
/// Применять выбранную заставку в таком случае НЕЛЬЗЯ: гибрид числится
/// установленным, и при следующем входе самолечение вернуло бы нашу тему —
/// покупатель увидел бы, что выбор «не сохранился», без всякой причины.
pub fn msg_slot_release_failed(detail: &str) -> String {
    trf(
        "Couldn't turn off the Rexon startup movie: %1. The chosen splash screen \
         wasn't applied — otherwise Rexon would bring its own back at the next login.",
        &[&detail],
    )
}

/// Ответ покупателя на спор за слот, переведённый в план применения.
///
/// Один на все хозяева вкладки «Оформление» (Rust-окно и Python-хозяин
/// живого продукта): ветвление «Да/Нет → что делать» живёт здесь, а
/// хозяевам остаётся показать вопрос и исполнить план (правило #12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplashSlotPlan {
    /// Применять. `keep_video` — вернуть слот заставки себе сразу после
    /// применения (путь ролика), `note` — что дописать к итогу.
    Apply {
        keep_video: Option<String>,
        note: String,
    },
    /// Не применять — отчитаться этим текстом.
    Stop(String),
}

impl SplashSlotPlan {
    /// Спора нет — обычное применение без примечаний.
    pub fn plain() -> Self {
        SplashSlotPlan::Apply {
            keep_video: None,
            note: String::new(),
        }
    }
}

/// Итог применения с примечанием о слоте заставки.
///
/// Примечание дописывается только к УДАЧЕ: отказ установщика говорит сам за
/// себя, и «ролик остался на месте» рядом с ним читалось бы как успех.
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
// Правило KWin «поверх всего» (kwinrulesrc)
// ============================================================================

/// Название правила — его человек видит в «Параметрах системы → Управление
/// окнами → Правила окон» KDE, поэтому оно на языке интерфейса.
// TRANSLATORS: name of a KDE window rule that keeps Rexon's startup movie
// above the panel; people see it in System Settings → Window Rules.
pub const KWIN_RULE_TITLE: &str = n_("Rexon: startup movie on top of everything");

/// Первая строка правила, какой её писал оригинал (`_KWIN_RULE_DESC`), — по-
/// русски для всех языков. Новые правила так больше не пишутся
/// ([`kwin_rule_desc_line`]); константа осталась, чтобы узнавать уже
/// установленные правила, и для моста в старый Python.
pub const KWIN_RULE_DESC: &str = "Description=Rexon: заставка входа поверх всего\n";

/// Название из [`KWIN_RULE_DESC`] без ключа и перевода строки.
const KWIN_RULE_TITLE_LEGACY_RU: &str = "Rexon: заставка входа поверх всего";

/// Первая строка правила на текущем языке интерфейса.
pub fn kwin_rule_desc_line() -> String {
    format!("Description={}\n", tr(KWIN_RULE_TITLE))
}

/// Все названия, под которыми наше правило могло быть записано: исходное
/// английское, прежнее русское (до каталога перевода) и перевод на каждый
/// язык каталога, установленный на машине, плюс текущий.
///
/// Правило ищется прежде всего по `wmclass` ([`KWIN_APP_ID`]); названия —
/// второй признак (правило без `wmclass`, поправленное руками) и, главное,
/// разрешение переписать название на текущий язык: название не из этого
/// списка — своё, данное человеком, и его не трогаем.
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
/// Снятие FSP (0=None, 2=Force) — порт `_KWIN_RULE_FSP`. Ключевой момент:
/// keepAbove НЕ поднимает окно над панелью (DockLayer выше AboveLayer);
/// над панелью — только АКТИВНЫЙ фулскрин, а активацию mpv получает лишь
/// со снятым focus-stealing-prevention (инцидент 2026-07-17 19:57).
pub const KWIN_RULE_FSP: &str = "fsplevel=0\nfsplevelrule=2\n";

/// Тело правила KWin (без заголовка `[N]`) — порт `_KWIN_RULE_BODY`.
pub fn kwin_rule_body() -> String {
    kwin_rule_body_with(&kwin_rule_desc_line())
}

/// [`kwin_rule_body`] с заданной первой строкой (`Description=…\n`).
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

/// Кандидаты команды reconfigure живого KWin — порт `_kwin_reconfigure`
/// (исполнитель берёт первый существующий бинарь; reconfigure асинхронный —
/// после него пауза ~0.5 c до старта mpv).
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

/// Разбить текст kwinrulesrc на секции — семантика `re.split(r"(?m)^(?=\[)")`
/// (границы ПЕРЕД каждым `[Header]` в начале строки; заголовки сохраняются).
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

/// Номер `[N]` в начале строки — семантика `^\[(\d+)\]` (re.M).
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

/// Байтовое смещение начала и конца ПЕРВОЙ строки с данным префиксом
/// (семантика `re.search(..., re.M)` для `^prefix...$`).
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

/// Первая строка вида `count=<цифры>` целиком — семантика `^count=\d+$`.
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

/// Идемпотентное правило KWin для окна заставки — порт чистой части
/// `_ensure_kwin_splash_rule` (текст файла → новый текст файла).
///
/// * правило уже есть и актуально (`fsplevel=`, название на текущем языке)
///   → `None` (файл не трогаем);
/// * старое правило без fsplevel → миграция на месте (врезка снятия FSP);
/// * название — один из прежних вариантов ([`kwin_rule_known_titles`]), но
///   не на текущем языке → переписывается на текущий;
/// * правила нет → новый блок `[max+1]` + регистрация в `[General]`.
///
/// Возврат `Some(text)` = исполнитель перезаписывает файл и дёргает
/// reconfigure живого KWin ([`kwin_reconfigure_candidates`]).
///
/// Починка №1: если `[General]` есть, но в нём нет `rules=`/`count=`,
/// Python терял регистрацию правила (re.sub вхолостую) — здесь недостающие
/// ключи вписываются сразу после заголовка `[General]`.
///
/// Починка №2 (01.10.2026): своё правило узнаётся по строке `wmclass=`
/// ЦЕЛИКОМ (раньше — подстрокой, и чужое `wmclass=rexon-session-splash-x`
/// сошло бы за наше) либо по одному из известных названий. Миграция
/// врезает снятие FSP после строки `Description=` ЛЮБОГО языка: оригинал
/// искал свою русскую строку дословно, и при другом названии миграция
/// молча не происходила — файл переписывался на каждом запуске без толку.
pub fn ensure_kwin_rule_text(existing: Option<&str>) -> Option<String> {
    ensure_kwin_rule_text_with(existing, &tr(KWIN_RULE_TITLE), &kwin_rule_known_titles())
}

/// Значение первой строки `key=` секции (без `\r`).
fn section_value<'a>(sec: &'a str, key: &str) -> Option<&'a str> {
    sec.lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .find_map(|l| l.strip_prefix(key))
}

/// [`ensure_kwin_rule_text`] с явными названиями: `title` — текущее,
/// `known` — все, под которыми правило могло быть записано.
pub fn ensure_kwin_rule_text_with(
    existing: Option<&str>,
    title: &str,
    known: &[String],
) -> Option<String> {
    let txt = existing.unwrap_or("");
    let desc_line = format!("Description={title}\n");
    // Наше правило уже есть? Ищем свой блок среди секций.
    let sections = split_sections(txt);
    let is_ours = |sec: &str| {
        // Секция правила — любая, кроме [General]: KWin новых версий
        // называет правила не номерами, а UUID.
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
        // Название — на текущий язык, если оно наше прежнее, а не данное
        // человеком.
        if let Some((s, e)) = find_line_with_prefix(&updated, "Description=") {
            let current = &updated[s + "Description=".len()..e];
            if current != title && known.iter().any(|k| k == current) {
                updated.replace_range(s + "Description=".len()..e, title);
            }
        }
        // Миграция старого правила: врезаем снятие FSP после названия (или
        // сразу после заголовка, если названия нет).
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
            return None; // актуально, ничего не делаем
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

    // Правила ещё нет — добавляем новый блок.
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
            // Починка №1: вписать недостающие ключи сразу после [General].
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

/// JSON-литерал строки в стиле `json.dumps` (ensure_ascii=True).
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

/// `json.dumps(v)` с дефолтными разделителями Python (`", "` / `": "`).
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

/// Полезная нагрузка `_mpv_ipc_send`: `json.dumps({"command": cmd}) + "\n"`.
pub fn mpv_ipc_command_payload(command: &Value) -> String {
    let mut m = Map::new();
    m.insert("command".into(), command.clone());
    let mut out = String::new();
    py_dumps(&Value::Object(m), &mut out);
    out.push('\n');
    out
}

/// Полезная нагрузка запроса `get_property` в `_mpv_ipc_get`
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

/// Разбор потока ответа mpv-IPC — порт цикла в `_mpv_ipc_get`.
///
/// В сокет вперемешку с ответом сыплются события: строки разбираются по
/// порядку, незавершённый хвост (без `\n`) игнорируется; первый объект с
/// нашим `request_id` решает: `error == "success"` → `Some(data)`, иначе
/// `None`. Починка №2: валидный JSON-не-объект пропускается (в Python
/// `msg.get` ронял AttributeError мимо перехвата).
pub fn parse_mpv_ipc_response(received: &str, request_id: i64) -> Option<Value> {
    let mut parts: Vec<&str> = received.split('\n').collect();
    parts.pop(); // незавершённый хвост
    for line in parts {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !msg.is_object() {
            continue; // починка №2
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

/// Адрес сокета `sd_notify` — порт трансформации в `_sd_notify_ready`:
/// пусто → None (no-op), `@…` (abstract namespace) → `\0…`.
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

/// Полезная нагрузка READY менеджеру systemd.
pub const SD_NOTIFY_READY: &str = "READY=1";

// ============================================================================
// X11: геометрия primary-монитора (сам оверлей — непортируемый ctypes-код)
// ============================================================================

/// `\b` перед позицией `pos` (semantics Python: слева не-словесный символ
/// или начало строки; `connected` начинается со словесного).
fn word_boundary_before(txt: &str, pos: usize) -> bool {
    match txt[..pos].chars().next_back() {
        None => true,
        Some(c) => !(c.is_alphanumeric() || c == '_'),
    }
}

/// Прочитать `\d+` с позиции; возврат (число, конец) или None.
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

/// Ожидаемый одиночный символ на позиции; возврат позиции после него.
fn expect_char(txt: &str, at: usize, ch: char) -> Option<usize> {
    if txt[at..].starts_with(ch) {
        Some(at + ch.len_utf8())
    } else {
        None
    }
}

/// Геометрия `(\d+)x(\d+)\+(\d+)\+(\d+)` с позиции.
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

/// Первое вхождение `\b<needle><geometry>` — общая часть обоих регексов.
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

/// (x, y, w, h) primary-монитора из вывода `xrandr --query` — порт разбора в
/// `_x11_primary_geometry`. Без primary-флага — первый connected-выход
/// (покрывать весь виртуальный root при мультимониторе нельзя); ничего не
/// нашли → None.
pub fn parse_xrandr_geometry(stdout: &str) -> Option<(i64, i64, i64, i64)> {
    let (w, h, x, y) = find_geometry_after(stdout, "connected primary ")
        .or_else(|| find_geometry_after(stdout, "connected "))?;
    Some((x, y, w, h))
}

/// argv `xrandr --query` (порт вызова в `_x11_primary_geometry`).
pub fn xrandr_query_argv(exe: &str) -> Vec<String> {
    vec![exe.into(), "--query".into()]
}

// ============================================================================
// Прогон: маршруты, argv mpv и rexon-splash-overlay, сообщения
// ============================================================================

/// Порт `_is_x11_session`: есть DISPLAY и нет WAYLAND_DISPLAY (на Plasma
/// Wayland у user-юнитов есть ОБА — там остаётся Wayland-путь).
pub fn is_x11_session(display: Option<&str>, wayland_display: Option<&str>) -> bool {
    display.map(|d| !d.is_empty()).unwrap_or(false)
        && !wayland_display.map(|w| !w.is_empty()).unwrap_or(false)
}

/// Эффективный режим гибрида — порт `hybrid=None` в `play` (авто: гибрид
/// установлен И реальный прогон; тест из UI передаёт Some(false)).
pub fn effective_hybrid(param: Option<bool>, hybrid_installed: bool, blocking: bool) -> bool {
    param.unwrap_or(hybrid_installed && blocking)
}

/// Пробовать ли Rust-хелпер rexon-splash-overlay — порт условия в `play`
/// (только реальный прогон на Wayland; тест из UI идёт оконным mpv).
pub fn wants_wayland_helper(
    blocking: bool,
    display: Option<&str>,
    wayland_display: Option<&str>,
) -> bool {
    blocking
        && !is_x11_session(display, wayland_display)
        && wayland_display.map(|w| !w.is_empty()).unwrap_or(false)
}

/// Кандидаты бинаря rexon-splash-overlay — порт `_find_splash_overlay_binary`
/// (workspace-таргет rust/target и таргет крейта, release затем debug;
/// PATH-фолбэк — наблюдение исполнителя). `repo_root` — realpath корня.
pub fn splash_overlay_candidates(repo_root: &str) -> Vec<String> {
    vec![
        format!("{repo_root}/rust/target/release/{SPLASH_OVERLAY_BINARY}"),
        format!("{repo_root}/rust/target/debug/{SPLASH_OVERLAY_BINARY}"),
        format!("{repo_root}/rust/{SPLASH_OVERLAY_BINARY}/target/release/{SPLASH_OVERLAY_BINARY}"),
        format!("{repo_root}/rust/{SPLASH_OVERLAY_BINARY}/target/debug/{SPLASH_OVERLAY_BINARY}"),
    ]
}

/// argv rexon-splash-overlay — порт cmd в `_play_wayland_overlay`
/// (протокол хелпера = протокол mpv: --ipc/--paused/unpause/quit).
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

/// Куда рендерит mpv — ветки построения cmd в `play`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpvTarget {
    /// X11: рисуем в свой override-redirect слой через `--wid`
    /// (`--keep-open=yes`: закрываем слой ПЕРВЫМ, постер не мелькает).
    X11Overlay { window_id: u64 },
    /// Обычное окно `--fullscreen --ontop`; на Wayland добавляется app_id
    /// (иначе «поверх всего» не удержать — компенсирует правило KWin).
    Window { wayland: bool },
}

/// argv mpv — порт построения `cmd` в `play` (одна функция на обе ветки;
/// `ipc_sock` попадает в argv по правилам оригинала: overlay — всегда,
/// окно — только в гибриде).
pub fn mpv_argv(
    path: &str,
    sound: bool,
    hybrid: bool,
    target: &MpvTarget,
    ipc_sock: &str,
) -> Vec<String> {
    let mut cmd: Vec<String> = vec![
        "mpv".into(),
        "--no-config".into(), // игнор пользовательского mpv.conf
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
                // --ontop на Wayland мёртв — поверх держит правило KWin по
                // app_id (см. ensure_kwin_rule_text).
                cmd.push(format!("--wayland-app-id={KWIN_APP_ID}"));
            }
            if hybrid {
                cmd.push(format!("--input-ipc-server={ipc_sock}"));
            }
        }
    }
    if hybrid {
        // Старт НА ПАУЗЕ: первый кадр (= постер темы) под оверлеем
        // ksplashqml; unpause — после его гашения.
        cmd.push("--pause".into());
    }
    cmd.push("--".into());
    cmd.push(path.into());
    cmd
}

// ── Сообщения play/автозапуска (тексты — как в оригинале) ──────────────────

/// «Файл не задан/не найден» в начале `play`.
pub fn msg_no_file() -> String {
    tr("The startup movie file isn't set or can't be found")
}
/// Нет mpv (и нет пригодного хелпера).
pub fn msg_no_mpv() -> String {
    tr("mpv isn't installed (it's needed for playback)")
}
/// Второй запуск при живом flock-локе.
pub fn msg_already_playing() -> String {
    tr("The startup movie is already playing (second launch skipped)")
}
/// Успешный прогон.
pub fn msg_played() -> String {
    tr("Played")
}
/// Non-blocking запуск (тест из UI).
pub fn msg_launched() -> String {
    tr("Started")
}
/// mpv не завершился за [`PLAY_TIMEOUT_SEC`].
pub fn msg_mpv_timeout() -> String {
    tr("mpv didn't finish within 120 s — stopped")
}
/// Хелпер не завершился за [`PLAY_TIMEOUT_SEC`].
pub fn msg_helper_timeout() -> String {
    tr("The startup movie didn't finish within 120 s — stopped")
}
/// В `install_autostart` не задан ролик.
///
/// `msgid`, а не готовый текст: витрина узнаёт этот отказ по нему
/// (`qt_steam::autostart_refusal_text`) — см. [`msg_choose_first`].
pub const MSG_CHOOSE_FIRST: &str = n_("Choose a startup movie first");
/// [`MSG_CHOOSE_FIRST`] на языке интерфейса.
pub fn msg_choose_first() -> String {
    tr(MSG_CHOOSE_FIRST)
}
/// Autostart-.desktop включён.
pub fn msg_autostart_enabled() -> String {
    tr("Startup movie at login is on")
}
/// Заставка при входе полностью отключена.
pub fn msg_autostart_disabled() -> String {
    tr("Startup movie at login is off")
}
/// Гибрид включён (маршрут hybrid в `install_autostart`).
pub fn msg_hybrid_route_ok() -> String {
    tr("Turned on (KSplash hybrid — no desktop flash)")
}
/// `_install_hybrid` успешен.
pub fn msg_hybrid_installed() -> String {
    tr("KSplash hybrid is on")
}
/// `_uninstall_hybrid` успешен.
pub fn msg_hybrid_uninstalled() -> String {
    tr("KSplash hybrid is off")
}
/// `_write_lnf_theme` успешен.
pub fn msg_lnf_updated() -> String {
    tr("KSplash theme updated")
}

/// `f"mpv вернул код {rc}"`.
pub fn mpv_exit_message(rc: i64) -> String {
    trf("mpv exited with code %1", &[&rc])
}

/// `f"rexon-splash-overlay вернул код {rc}"`.
pub fn helper_exit_message(rc: i64) -> String {
    trf("rexon-splash-overlay exited with code %1", &[&rc])
}

/// Сообщение фолбэка «гибрид не удался» — порт хвоста `install_autostart`.
pub fn hybrid_fallback_message(hybrid_msg: &str, plain_ok: bool, plain_msg: &str) -> String {
    if plain_ok {
        trf("The hybrid didn't work (%1) — regular autostart is on instead", &[&hybrid_msg])
    } else {
        plain_msg.to_string()
    }
}

/// Содержимое autostart-.desktop (фаза 1) — порт `_install_desktop_autostart`.
///
/// `Comment` — на всех языках каталога ([`crate::system_l10n::desktop_key`]):
/// прежде он был только русским, и английский покупатель видел русский.
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

/// Маршрут `install_autostart` (сами шаги — у исполнителя).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartRoute {
    /// Ролик не выбран → (false, [`MSG_CHOOSE_FIRST`]).
    NoPath,
    /// Гибрид: `_install_hybrid`; успех → снять .desktop и
    /// [`msg_hybrid_route_ok`]; неудача → полный откат гибрида + фаза 1
    /// ([`hybrid_fallback_message`]).
    Hybrid,
    /// Чистая фаза 1 (autostart-.desktop); `rollback_hybrid` — сперва
    /// откатить остатки гибрида (`hybrid_installed` в конфиге).
    Plain { rollback_hybrid: bool },
}

/// Решение маршрута — порт развилки в `install_autostart`
/// (галка гибрида по умолчанию ВКЛ: `cfg.get("hybrid", True)`).
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

/// Порт `is_enabled`: включён любой из двух механизмов.
pub fn is_enabled(autostart_file_exists: bool, unit_file_exists: bool) -> bool {
    autostart_file_exists || unit_file_exists
}

/// Источник постера для X11-слоя — порт `_poster_for_video`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosterSource {
    /// Готовый постер темы KSplash (критический путь входа без ~0.3 c
    /// ffmpeg) — ролик из конфига и файл на месте.
    Theme(String),
    /// Свежий кадр ffmpeg-ом в runtime-каталог.
    Extract(String),
}

/// Решение — откуда постер: тема (если `cfg.path == video` и постер темы
/// существует) или извлечение в runtime.
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
// Раскладка каталогов Steam и LnF-пакета (пути — здесь, листинги — исполнитель)
// ============================================================================

/// Каталог встроенных заставок клиента Steam.
pub fn steam_movies_dir(root: &str) -> String {
    format!("{root}/steamui/movies")
}

/// Имена файлов встроенных заставок (порядок = порядок обхода при скане).
pub fn builtin_movie_filenames() -> Vec<String> {
    BUILTIN_NAMES
        .iter()
        .map(|(key, _)| format!("{key}.webm"))
        .collect()
}

/// Каталог инвентарных заставок (купленное в Points Shop уже лежит файлами).
pub fn steam_inventory_dir(root: &str) -> String {
    format!("{root}/config/communityitemscache/startupmovies")
}

/// Каталог профилей Steam (`userdata/<steamid>`).
pub fn steam_userdata_dir(root: &str) -> String {
    format!("{root}/userdata")
}

/// `localconfig.vdf` одного профиля — кэш ответа Points Shop.
pub fn steam_localconfig_path(root: &str, steam_id: &str) -> String {
    format!("{root}/userdata/{steam_id}/config/localconfig.vdf")
}

/// `appcache/appinfo.vdf` — appid → название игры (двоичный VDF).
pub fn steam_appinfo_path(root: &str) -> String {
    format!("{root}/appcache/appinfo.vdf")
}

/// Пути внутри LnF-пакета темы KSplash — порт раскладки `_write_lnf_theme`.
/// Второй копии этих имён на Python-стороне нет: тема и постер обязаны
/// лежать там, где их ищет `theme_poster_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnfLayout {
    /// `contents/splash` — каталог Splash.qml.
    pub splash_dir: String,
    /// `contents/splash/images` — каталог постера.
    pub images_dir: String,
    /// `contents/previews` — превью для списка тем в Параметрах системы.
    pub previews_dir: String,
    /// `contents/splash/images/poster.png` — первый кадр ролика.
    pub poster_png: String,
    /// `contents/previews/splash.png` — копия постера для списка тем.
    pub preview_png: String,
    /// `metadata.json` пакета.
    pub metadata_json: String,
    /// `contents/splash/Splash.qml`.
    pub splash_qml: String,
}

/// Раскладка LnF-пакета по каталогу темы.
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
// Константы исполнения: имена, таймауты подпроцессов, шаги poll-ожиданий
// ============================================================================

/// Цель systemd, по которой гибрид определяет живую сессию Plasma и в
/// `.wants` которой лежит ручной enable-фолбэк.
pub const PLASMA_WORKSPACE_TARGET: &str = "plasma-workspace.target";
/// Значение `Engine` в ksplashrc для QML-заставки.
pub const KSPLASH_ENGINE: &str = "KSplashQML";
/// Имя процесса заставки Plasma (по нему видно, погас ли оверлей).
pub const KSPLASH_PROCESS: &str = "ksplashqml";
/// Суффикс временного файла атомарной записи (`.tmp` + `os.replace`).
pub const TMP_SUFFIX: &str = ".tmp";
/// Суффикс недописанного кадра витрины (переименовывается по успеху ffmpeg).
pub const PREVIEW_TMP_SUFFIX: &str = ".part.png";
/// Глубины X11-визуала, при которых постер кладётся в слой как BGRX-байты.
/// 30-битный (10bpc) визуал такими байтами рисуется криво — постер тогда не
/// ставим, слой остаётся чёрным (не фатально).
pub const X11_POSTER_DEPTHS: &[i64] = &[24, 32];

/// Таймаут `ffprobe` (длительность ролика).
pub const FFPROBE_TIMEOUT_SEC: f64 = 15.0;
/// Таймаут `ffmpeg` (кадр витрины и постер темы).
pub const FFMPEG_TIMEOUT_SEC: f64 = 30.0;
/// Таймаут `kwriteconfig6`.
pub const KWRITECONFIG_TIMEOUT_SEC: f64 = 10.0;
/// Таймаут `systemctl --user`.
pub const SYSTEMCTL_TIMEOUT_SEC: f64 = 15.0;
/// Таймаут `xrandr --query`.
pub const XRANDR_TIMEOUT_SEC: f64 = 5.0;
/// Таймаут пробы «жив ли ksplashqml».
pub const PGREP_TIMEOUT_SEC: f64 = 5.0;
/// Таймаут вызова reconfigure у живого KWin.
pub const DBUS_TIMEOUT_SEC: f64 = 5.0;
/// Пауза после reconfigure: он асинхронный, а правило применяется при
/// создании окна — дать KWin перечитать kwinrulesrc до старта mpv.
pub const KWIN_RECONFIGURE_SETTLE_SEC: f64 = 0.5;

/// Шаг ожидания IPC-сокета mpv в эстафете гибрида.
pub const SOCK_POLL_SEC: f64 = 0.1;
/// Шаг ожидания IPC-сокета Rust-хелпера (он поднимается заметно быстрее).
pub const HELPER_POLL_SEC: f64 = 0.05;
/// Шаг проб «жив ли ksplashqml».
pub const KSPLASH_POLL_SEC: f64 = 0.1;
/// Шаг опроса `eof-reached` через mpv-IPC.
pub const EOF_POLL_SEC: f64 = 0.15;
/// Шаг ожидания композитора на холодном X11-входе.
pub const COMPOSITOR_POLL_SEC: f64 = 0.1;
/// Пауза «дать композитору отрисовать первую сцену» после его появления.
pub const COMPOSITOR_SETTLE_SEC: f64 = 0.2;
/// Страховочный поллинг raise-цикла, когда событийная подписка не удалась.
pub const RAISE_POLL_FALLBACK_SEC: f64 = 0.08;
/// Ожидание чистого выхода после `quit` по IPC.
pub const QUIT_WAIT_SEC: f64 = 3.0;
/// Ожидание после SIGTERM, перед SIGKILL.
pub const TERM_WAIT_SEC: f64 = 2.0;
/// Ожидание после SIGTERM в ветке «mpv не завершился за PLAY_TIMEOUT_SEC».
pub const MPV_TERM_WAIT_SEC: f64 = 5.0;
/// Срок ожидания на unix-сокетах исполнителя (mpv-IPC и sd_notify).
///
/// Своей копии у исполнителя больше нет: ровно на параллельных копиях
/// сроков однажды разъехалось ожидание сокета хелпера с `TimeoutStartSec`
/// юнита и панель встала поверх заставки (инцидент 2026-07-25).
pub const IPC_SOCK_TIMEOUT_SEC: f64 = 2.0;
/// Сколько ждать завершения потока raise-цикла при остановке слоя.
///
/// Потолок, а не пауза: поток будят немедленно, и обычно он уходит за доли
/// миллисекунды. Значение важно тем, что ПРЕВЫШАЕТ страховочный шаг цикла
/// ([`RAISE_INTERVAL_SEC`]) — иначе остановка возвращалась бы раньше, чем
/// поток дошёл до проверки признака, и слой закрывался бы у него под руками.
pub const RAISE_JOIN_WAIT_SEC: f64 = 1.0;

/// Маркер строки-ОТВЕТА в потоке mpv-IPC: события таким полем не подписаны,
/// поэтому исполнитель читает сокет до первой строки с ним (иначе `recv`
/// висел бы на живом соединении).
pub const MPV_IPC_REPLY_MARKER: &str = "\"request_id\"";
/// Предел чтения ответа из mpv-IPC — порт `while len(buf) < 65536`.
pub const MPV_IPC_READ_LIMIT: usize = 65536;

/// argv пробы «жив ли ksplashqml» — порт `_ksplash_running`.
/// `exe` — путь из `shutil.which("pgrep")`.
pub fn pgrep_argv(exe: &str, process: &str) -> Vec<String> {
    vec![exe.into(), "-x".into(), process.into()]
}

/// Порт `round()` CPython (половина — к чётному): в letterbox-математике
/// ровно .5 попадается на «круглых» разрешениях, и рамка постера съезжала бы
/// на пиксель относительно кадра mpv.
fn py_round(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

/// Вписать картинку в слой с сохранением пропорций (letterbox — как у mpv
/// `--fullscreen`): (x, y, ширина, высота) внутри окна `dst_*`. Поля вокруг
/// остаются чёрными. Порт масштабирования в `_X11SplashOverlay.set_poster`.
pub fn letterbox_rect(src_w: i64, src_h: i64, dst_w: i64, dst_h: i64) -> (i64, i64, i64, i64) {
    if src_w <= 0 || src_h <= 0 {
        return (0, 0, dst_w.max(1), dst_h.max(1));
    }
    let scale = (dst_w as f64 / src_w as f64).min(dst_h as f64 / src_h as f64);
    let sw = (py_round(src_w as f64 * scale) as i64).max(1);
    let sh = (py_round(src_h as f64 * scale) as i64).max(1);
    ((dst_w - sw) / 2, (dst_h - sh) / 2, sw, sh)
}

// ── Тексты статусов, которых у оригинала не было в одном месте ─────────────

/// Не удалось создать каталог темы KSplash.
pub fn msg_theme_dir_failed(err: &str) -> String {
    trf("Couldn't create the theme folder: %1", &[&err])
}
/// Не удалось записать файлы темы KSplash.
pub fn msg_theme_write_failed(err: &str) -> String {
    trf("Couldn't write the theme: %1", &[&err])
}
/// ffmpeg не установлен (постер извлечь нечем).
pub fn msg_no_ffmpeg() -> String {
    tr("ffmpeg isn't installed")
}
/// ffmpeg не запустился.
pub fn msg_ffmpeg_failed(err: &str) -> String {
    format!("ffmpeg: {err}")
}
/// ffmpeg вернул ошибку: первые 200 символов stderr либо «не смог».
pub fn msg_ffmpeg_stderr(stderr: &str) -> String {
    let trimmed = stderr.trim();
    let short: String = trimmed.chars().take(200).collect();
    if short.is_empty() {
        tr("ffmpeg: failed")
    } else {
        format!("ffmpeg: {short}")
    }
}
/// Постер извлечён (порт `return True, "OK"`).
pub const MSG_OK: &str = "OK";
/// ksplashrc не записался (нет kwriteconfig6 либо он вернул ошибку).
pub fn msg_ksplashrc_failed() -> String {
    tr("Couldn't write ksplashrc (kwriteconfig6)")
}
/// Самолечение: связка ksplashrc была сбита (тема в скобках — чужая),
/// вернули свою.
pub fn msg_heal_ksplashrc_restored(prev_theme: &str) -> String {
    let was = if prev_theme.is_empty() {
        "ключ потерян".to_string()
    } else {
        format!("была «{prev_theme}»")
    };
    format!("Связка заставки в ksplashrc восстановлена ({was})")
}
/// Самолечение: вернуть ключи в ksplashrc не вышло — ролик всё равно играем.
pub const MSG_HEAL_KSPLASHRC_FAILED: &str =
    "Не восстановить связку в ksplashrc — при входе мелькнёт штатная заставка Plasma";
/// Самолечение: каталога темы-постера не было, перегенерировали.
pub const MSG_HEAL_THEME_REBUILT: &str = "Тема-постер KSplash отсутствовала — пересоздана";
/// Самолечение: пересоздать тему-постер не вышло.
pub fn msg_heal_theme_failed(err: &str) -> String {
    format!("Не пересоздать тему-постер KSplash: {err}")
}
/// Самолечение: конфиг с бэкапом чужой темы не записался.
pub fn msg_heal_backup_failed(err: &str) -> String {
    format!("Не сохранить прежнюю тему KSplash: {err}")
}
/// systemd-юнит не записался.
pub fn msg_unit_write_failed(err: &str) -> String {
    trf("Couldn't write the systemd unit: %1", &[&err])
}
/// systemd-юнит не включился (и ручной симлинк тоже не удался).
pub fn msg_unit_enable_failed(err: &str) -> String {
    trf("Couldn't enable the systemd unit: %1", &[&err])
}
/// Сводка проблем отката гибрида (`problems` — уже через «; »).
pub fn msg_rollback_problems(problems: &str) -> String {
    trf("Hybrid rollback finished with errors: %1", &[&problems])
}
/// Проблема с юнитом при откате.
pub fn msg_problem_unit(err: &str) -> String {
    // TRANSLATORS: one problem in a list; %1 is a system error.
    trf("unit: %1", &[&err])
}
/// Проблема с ksplashrc при откате.
pub fn msg_problem_ksplashrc() -> String {
    tr("ksplashrc: kwriteconfig6 failed")
}
/// Проблема с каталогом темы при откате.
pub fn msg_problem_theme(err: &str) -> String {
    // TRANSLATORS: one problem in a list; %1 is a system error.
    trf("theme: %1", &[&err])
}
/// Проблема со снятием autostart-.desktop.
pub fn msg_problem_autostart(err: &str) -> String {
    format!("autostart: {err}")
}
/// Не удалось включить фазу 1 (autostart-.desktop).
pub fn msg_autostart_write_failed(err: &str) -> String {
    trf("Couldn't turn on: %1", &[&err])
}
/// Не удалось сохранить настройки заставки (⚠️ починка: в Python OSError на
/// записи конфига летел исключением из середины установки гибрида и
/// покупатель видел трейсбек вместо причины).
pub fn msg_config_write_failed(err: &str) -> String {
    trf("Couldn't save the startup movie settings: %1", &[&err])
}
/// mpv не запустился (текст исключения — от исполнителя).
pub fn msg_mpv_spawn_failed(err: &str) -> String {
    trf("Couldn't start mpv: %1", &[&err])
}

// ============================================================================
// Шов «ядро ведёт поток»: трейт исполнителя
// ============================================================================

/// Результат `subprocess.run(argv, capture_output=True, text=True,
/// timeout=…)`; `None` у [`SplashIo::run`] — OSError/SubprocessError.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    /// `returncode`.
    pub rc: i64,
    /// `stdout` (пустая строка, если нечего).
    pub stdout: String,
    /// `stderr` (пустая строка, если нечего).
    pub stderr: String,
}

/// Интерфейс исполнителя: каждый метод — ровно один побочный эффект или
/// наблюдение Python-оригинала. Ядро ведёт весь поток заставки (порядок,
/// решения, тексты, таймауты), а РЕАЛЬНОЕ исполнение — Popen mpv/ffmpeg/
/// systemctl, unix-сокеты, flock, запись файлов, X11-оверлей на ctypes и
/// живые потоки — остаётся в шиме. Ошибки файловых операций возвращаются
/// текстом (`None` = успех), чтобы поток мог объяснить отказ покупателю, а
/// не падать исключением из середины установки.
pub trait SplashIo {
    /// `os.environ.get(name, "")`.
    fn env(&mut self, name: &str) -> String;
    /// `shutil.which(name) or ""`.
    fn which(&mut self, name: &str) -> String;
    /// Какой вход продукта сейчас выбран — у живого исполнителя это
    /// [`crate::installer::launch_entry_live`], то же правило, что у ярлыка
    /// и лотка. Наблюдение, а не решение ядра: смотрит на PATH и диски.
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
    /// `os.listdir(path)`; OSError → пусто.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// Текст файла (`errors="replace"`); OSError → None.
    fn read_text(&mut self, path: &str) -> Option<String>;
    /// Байты файла; OSError → None.
    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>>;
    /// `int(os.path.getmtime(path))`; OSError → None.
    fn mtime(&mut self, path: &str) -> Option<i64>;
    /// `os.path.getsize(path)`; OSError → 0.
    fn file_size(&mut self, path: &str) -> i64;
    /// `os.makedirs(path, exist_ok=True)`; текст OSError при неудаче.
    fn makedirs(&mut self, path: &str) -> Option<String>;
    /// Запись текста; текст OSError при неудаче.
    fn write_text(&mut self, path: &str, content: &str) -> Option<String>;
    /// Атомарная запись (`.tmp` + `os.replace`); текст OSError при неудаче.
    fn write_atomic(&mut self, path: &str, content: &str) -> Option<String>;
    /// `os.replace(src, dst)`; текст OSError при неудаче.
    fn replace(&mut self, src: &str, dst: &str) -> Option<String>;
    /// `os.remove(path)` (файла может не быть); текст OSError при неудаче.
    fn remove_file(&mut self, path: &str) -> Option<String>;
    /// `shutil.rmtree(path)`; текст OSError при неудаче.
    fn remove_tree(&mut self, path: &str) -> Option<String>;
    /// `shutil.copyfile(src, dst)`; текст OSError при неудаче.
    fn copy_file(&mut self, src: &str, dst: &str) -> Option<String>;
    /// `os.symlink(target, link)`; текст OSError при неудаче.
    fn symlink(&mut self, target: &str, link: &str) -> Option<String>;
    /// `subprocess.run(argv, capture_output=True, text=True, timeout=…)`.
    fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut>;
    /// `subprocess.Popen(argv[, start_new_session=True])` → дескриптор
    /// процесса у исполнителя; текст исключения при неудаче.
    fn spawn(&mut self, argv: &[String], detached: bool) -> Result<i64, String>;
    /// `proc.poll()` — код возврата либо None (ещё жив).
    fn poll(&mut self, proc: i64) -> Option<i64>;
    /// `proc.wait(timeout)` — код возврата либо None по таймауту.
    fn wait(&mut self, proc: i64, timeout: f64) -> Option<i64>;
    /// `proc.terminate()`.
    fn terminate(&mut self, proc: i64);
    /// `proc.kill()`.
    fn kill(&mut self, proc: i64);
    /// `time.monotonic()`.
    fn monotonic(&mut self) -> f64;
    /// `time.sleep(seconds)`.
    fn sleep(&mut self, seconds: f64);
    /// `random.choice(items)` — пустой список даёт None.
    fn choice(&mut self, items: &[String]) -> Option<String>;
    /// Одна команда в mpv-IPC без ответа; False — сокет недоступен.
    fn ipc_send(&mut self, sock: &str, payload: &str) -> bool;
    /// Запрос в mpv-IPC: отправить payload и прочитать поток до строки с
    /// `reply_marker` (или до конца/предела). None — сокет недоступен.
    fn ipc_request(&mut self, sock: &str, payload: &str, reply_marker: &str) -> Option<String>;
    /// Датаграмма менеджеру systemd (адрес уже нормализован ядром).
    fn notify(&mut self, addr: &str, message: &str);
    /// `flock(LOCK_EX|LOCK_NB)` на файл-замок; False — замок занят.
    fn acquire_lock(&mut self, path: &str) -> bool;
    /// reconfigure живого KWin через сессионную шину (общий хелпер).
    fn kwin_reconfigure_dbus(&mut self, timeout: f64) -> bool;
    /// Создать X11-слой (override-redirect окно во весь экран); None —
    /// нет libX11/X-сервера.
    fn x11_create(&mut self) -> Option<i64>;
    /// `window_id` слоя — уходит в `--wid` mpv.
    fn x11_window_id(&mut self, overlay: i64) -> i64;
    /// Нарисовать постер фоном слоя; False — не удалось (остаётся чёрный).
    fn x11_set_poster(&mut self, overlay: i64, png: &str) -> bool;
    /// Показать слой (`XMapRaised`).
    fn x11_map(&mut self, overlay: i64);
    /// Владелец `_NET_WM_CM_S<n>` уже есть (композитор работает).
    fn x11_compositor_present(&mut self, overlay: i64) -> bool;
    /// Запустить фоновый raise-цикл слоя (событийный, в своём потоке).
    fn x11_raise_start(&mut self, overlay: i64);
    /// Остановить raise-цикл и дождаться потока.
    fn x11_raise_stop(&mut self, overlay: i64);
    /// Уничтожить слой и закрыть X-соединение.
    fn x11_close(&mut self, overlay: i64);
    /// `os.access(path, os.X_OK)`.
    fn is_executable(&mut self, path: &str) -> bool;
    /// Составной шаг «поставить гибрид» (`_install_hybrid` шима).
    ///
    /// ⚠️ Пять методов ниже — единственные СОСТАВНЫЕ шаги трейта: их
    /// внутренности тоже живут в ядре ([`Splash::install_hybrid`] и
    /// соседи), но маршрут `install_autostart` дёргает их ЧЕРЕЗ
    /// исполнителя. Так шов остаётся ровно там, где он был у оригинала:
    /// сверка маршрутов подменяет шаги у шима и сравнивает
    /// последовательность с записанным эталоном, не поднимая живой
    /// ffmpeg/systemctl.
    fn step_install_hybrid(&mut self, video: &str) -> (bool, String);
    /// Составной шаг «откатить гибрид» (`_uninstall_hybrid` шима).
    fn step_uninstall_hybrid(&mut self) -> (bool, String);
    /// Составной шаг «включить фазу 1» (`_install_desktop_autostart`).
    fn step_install_desktop_autostart(&mut self) -> (bool, String);
    /// Составной шаг «снять autostart-.desktop».
    fn step_remove_desktop_autostart(&mut self);
    /// Составной шаг «возможен ли гибрид» (`hybrid_supported` шима).
    fn step_hybrid_supported(&mut self) -> bool;
    /// Строка в журнал: `level` — `warning`/`info`/`debug`.
    fn log(&mut self, level: &str, message: &str);

    /// Строки автозапуска в конфигах композиторов, которые `.desktop` не
    /// исполняют (`crate::compositor_autostart`). `Some(запись)` — поставить
    /// и вернуть окончательный текст записи (с `NotShowIn=`) и отказ для
    /// текущего сеанса; `None` — снять отовсюду. По умолчанию — ничего не
    /// трогать: подставные исполнители сверок живых конфигов не касаются.
    fn login_lines(&mut self, desktop: Option<&str>) -> (Option<String>, Option<String>) {
        (desktop.map(str::to_string), None)
    }
    /// Сработает ли запись по пути `desktop_path` в текущем сеансе (у
    /// композиторов без XDG autostart — только при своей строке в их
    /// конфиге).
    fn login_works(&mut self, desktop_path: &str) -> bool {
        let _ = desktop_path;
        true
    }
}

/// Поток заставки: ядро ведёт, [`SplashIo`] исполняет.
///
/// `home` — `os.path.expanduser("~")` (наблюдение исполнителя); всё
/// остальное (XDG-корни, пути, тексты, таймауты) ядро считает само, чтобы
/// параллельных копий значений не осталось нигде.
pub struct Splash<'a, IO: SplashIo> {
    io: &'a mut IO,
    home: String,
}

impl<'a, IO: SplashIo> Splash<'a, IO> {
    /// Новый поток над исполнителем.
    pub fn new(io: &'a mut IO, home: &str) -> Self {
        Self {
            io,
            home: home.to_string(),
        }
    }

    // ── Пути (XDG читается каждый раз: у юнита и у UI-процесса он разный) ──

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

    // ── Обнаружение Steam и скан заставок ──────────────────────────────

    /// Каталог установки Steam — порт `steam_root` (кандидаты ядра через
    /// realpath, признак — наличие `config/`).
    pub fn steam_root(&mut self) -> Option<String> {
        for cand in steam_root_candidates(&self.home.clone()) {
            let p = self.io.realpath(&cand);
            if self.io.is_dir(&format!("{p}/config")) {
                return Some(p);
            }
        }
        None
    }

    /// Обойти профили Steam и слить разобранные записи `GetStartupMovies`.
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

    /// communityitemid → метаданные — порт `_startup_movie_names`.
    pub fn startup_movie_names(&mut self, root: &str) -> Value {
        self.merge_localconfig(root, false)
    }

    /// communityitemid → ПОЛНАЯ запись предмета — порт `_startup_movie_items`.
    pub fn startup_movie_items(&mut self, root: &str) -> Value {
        self.merge_localconfig(root, true)
    }

    /// appid → название игры из `appcache/appinfo.vdf` — порт `_appinfo_names`.
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

    /// Базовый скан `{id, name, path, builtin}` — порт `_scan_startup_movies_raw`.
    fn scan_movies_raw(&mut self) -> Vec<Value> {
        let Some(root) = self.steam_root() else {
            return Vec::new();
        };
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
        scan_movies(&root, &builtin_present, &inventory, &names)
    }

    /// Дополнить записи скана метаданными предмета — порт `_enrich_movies`.
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

    /// Доступные стартовые видео Steam — порт `scan_startup_movies`.
    pub fn scan_startup_movies(&mut self) -> Vec<Value> {
        let raw = self.scan_movies_raw();
        self.enrich_movies(&raw)
    }

    // ── Кадр для витрины ───────────────────────────────────────────────

    /// Длительность ролика — порт `_video_duration` (0.0 = не определить).
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

    /// Кадр ролика для витрины (кэшируется) — порт `preview_frame`.
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
            // Ролик короче ожидаемого / битый — пробуем самый первый кадр
            // (тот же argv без -ss).
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

    // ── Конфиг, избранное, ротация ─────────────────────────────────────

    /// Конфиг заставки — порт `load_config`.
    pub fn load_config(&mut self) -> Value {
        let path = self.config_path();
        let text = self.io.read_text(&path);
        parse_config(text.as_deref())
    }

    /// Атомарная запись конфига — порт `_write_config`; текст ошибки при
    /// неудаче (исполнитель поднимает его исключением, как оригинал).
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

    /// Пути роликов со звёздочкой — порт `favorites`.
    pub fn favorites(&mut self) -> Vec<String> {
        let cfg = self.load_config();
        favorites_from_config(&cfg)
    }

    /// Отметить/снять звёздочку — порт `set_favorite`.
    pub fn set_favorite(&mut self, path: &str, on: bool) -> (Vec<String>, Option<String>) {
        let cfg = self.load_config();
        let (new_cfg, fav) = config_with_favorite(&cfg, path, on);
        let err = self.write_config(&new_cfg);
        (fav, err)
    }

    /// Включена ли ротация — порт `rotation_enabled`.
    pub fn rotation_enabled(&mut self) -> bool {
        let cfg = self.load_config();
        rotation_enabled(&cfg)
    }

    /// Переключить ротацию — порт `set_rotation`.
    pub fn set_rotation(&mut self, enabled: bool) -> Option<String> {
        let cfg = self.load_config();
        let new_cfg = config_with_rotation(&cfg, enabled);
        self.write_config(&new_cfg)
    }

    /// Из чего крутим — порт `rotation_pool`.
    pub fn rotation_pool(&mut self) -> Vec<String> {
        let fav: Vec<String> = self
            .favorites()
            .into_iter()
            .filter(|p| self.io.is_file(p))
            .collect();
        // Полный скан дорогой (ffmpeg/VDF) — только когда избранного мало и
        // правило всё равно уведёт в «всё найденное».
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

    /// Выбрать ролик на СЛЕДУЮЩИЙ вход и сохранить — порт `advance_rotation`.
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

    /// Сохранить выбор ролика (и обновить постер темы) — порт `save_config`.
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

    /// Запомнить галку «гибрид вкл/выкл» — порт `save_hybrid_pref`.
    pub fn save_hybrid_pref(&mut self, enabled: bool) -> Option<String> {
        let cfg = self.load_config();
        let new_cfg = config_with_hybrid_pref(&cfg, enabled);
        self.write_config(&new_cfg)
    }

    // ── Гибрид KSplash ─────────────────────────────────────────────────

    /// Гибрид возможен — порт `hybrid_supported` (short-circuit сохранён:
    /// не KDE → бинарей не ищем, нет бинарей → systemctl не дёргаем).
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

    /// Гибрид установлен — порт `hybrid_installed`.
    pub fn hybrid_installed(&mut self) -> bool {
        let cfg = self.load_config();
        let unit = self.unit_path();
        let exists = self.io.is_file(&unit);
        hybrid_installed(&cfg, exists)
    }

    /// Первый кадр ролика → PNG — порт `_extract_poster`.
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

    /// Сгенерировать/обновить LnF-пакет темы KSplash — порт `_write_lnf_theme`.
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
        // превью для списка тем в Параметрах системы → Заставка
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

    /// Значение из пользовательского ksplashrc — порт `_ksplashrc_get`.
    pub fn ksplashrc_get(&mut self, key: &str) -> String {
        let x = self.cfg_home();
        let path = ksplashrc_path(x.as_deref(), &self.home.clone());
        match self.io.read_text(&path) {
            Some(text) => ksplashrc_get(&text, key),
            None => String::new(),
        }
    }

    /// Записать/удалить ключ в ksplashrc — порт `_ksplashrc_set`.
    pub fn ksplashrc_set(&mut self, key: &str, value: &str) -> bool {
        let exe = self.io.which("kwriteconfig6");
        if exe.is_empty() {
            return false;
        }
        let argv = kwriteconfig_argv(&exe, key, value);
        matches!(self.io.run(&argv, KWRITECONFIG_TIMEOUT_SEC), Some(r) if r.rc == 0)
    }

    /// `systemctl --user …` — порт `_systemctl_user`.
    pub fn systemctl_user(&mut self, args: &[&str]) -> bool {
        let exe = self.io.which("systemctl");
        if exe.is_empty() {
            return false;
        }
        let argv = systemctl_user_argv(&exe, args);
        matches!(self.io.run(&argv, SYSTEMCTL_TIMEOUT_SEC), Some(r) if r.rc == 0)
    }

    /// Команда запуска заставки для `Exec=` автозапуска — порт
    /// `_launcher_exec`, вход — по правилу ярлыка ([`desktop_launcher_exec`]).
    pub fn launcher_exec(&mut self, python_executable: &str) -> String {
        let (entry, rexon) = self.entry_and_launcher();
        desktop_launcher_exec(&entry, rexon.as_deref(), python_executable)
    }

    /// То же для `ExecStart=` юнита гибрида ([`unit_launcher_exec`]).
    pub fn unit_launcher_exec(&mut self, python_executable: &str) -> String {
        let (entry, rexon) = self.entry_and_launcher();
        unit_launcher_exec(&entry, rexon.as_deref(), python_executable)
    }

    /// Вход продукта и — только при Python-входе — лаунчер `rexon` из `PATH`.
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

    /// Самолечение связки гибрида перед РЕАЛЬНЫМ показом заставки —
    /// причина и симптом описаны в шапке модуля.
    ///
    /// Ничего не возвращает и ничего не отменяет: каждая неудача — строка в
    /// журнал, ролик показать важнее исправности связки.
    pub fn heal_hybrid(&mut self, video: &str) {
        // Каталог темы мог унести `plasma-apply-lookandfeel --resetLayout`,
        // чистка `~/.local/share` или переезд профиля — постера тогда нет
        // вовсе, и вход остаётся чёрным до первого кадра mpv.
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
        // Чужое значение — в бэкап по ТОМУ ЖЕ условию, что и при установке
        // (`_install_hybrid`): откат гибрида обязан вернуть покупателю его
        // настоящую тему, а не нашу. Пустая строка — законное значение:
        // на откате она удалит ключ, вернув состояние «ключа не было».
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

    /// Поставить гибрид: тема-постер + ksplashrc + systemd-юнит — порт
    /// `_install_hybrid`.
    pub fn install_hybrid(&mut self, video: &str, python_executable: &str) -> (bool, String) {
        // ⛔Сперва — ЕСТЬ ЛИ ЧЕМ ЗАВОДИТЬ. Проверка стоит до единой правки на
        // диске нарочно: отказать на полпути (тема-постер записана,
        // ksplashrc уже переключен на нашу тему, юнита нет) — значит
        // оставить вход в сеанс в состоянии хуже исходного.
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
        // Бэкап оригинала — только если сейчас стоит НЕ наша тема (повторное
        // включение не должно затирать настоящий бэкап нашим же ID).
        // Персистим ДО правки ksplashrc: упади установка дальше — откату
        // будет из чего восстанавливать.
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
            // фолбэк: руками симлинк в plasma-workspace.target.wants
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

    /// Полный откат гибрида: юнит, ksplashrc, тема — порт `_uninstall_hybrid`.
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
        // подчистить ручной симлинк, если enable шёл через фолбэк
        let link = self.wants_link();
        if problems.is_empty() && self.io.is_link(&link) {
            if let Some(e) = self.io.remove_file(&link) {
                problems.push(msg_problem_unit(&e));
            }
        }
        self.systemctl_user(&["daemon-reload"]);

        let mut cfg = self.load_config();
        // ksplashrc откатываем ТОЛЬКО если там всё ещё наша тема
        // (пользователь мог выбрать другую в Параметрах системы).
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

    // ── Спор за слот заставки при применении оформления ────────────────

    /// Разобрать спор за слот KSplash перед применением оформления.
    ///
    /// `ask` показывает вопрос покупателю и возвращает «Да» (`true`) —
    /// диалог у каждого хозяина свой, решение одно. Зовётся ДО применения:
    /// при «Да» гибрид снимается сразу, чтобы самолечение не вернуло нашу
    /// тему при следующем входе. `ask` не зовётся вовсе, когда спорить не о
    /// чем; у видов, слота не касающихся, не читается даже конфиг.
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
            // Место остаётся за роликом. У целого оформления это не отказ:
            // применяем всё, а слот возвращаем тем же самолечением, каким
            // гибрид держится при входе (второго механизма возврата нет).
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
        // Место отдано выбранной заставке — гибрид снимаем ЧЕСТНО.
        let (ok, detail) = self.uninstall_hybrid();
        if !ok {
            return SplashSlotPlan::Stop(msg_slot_release_failed(&detail));
        }
        // У «Нет» показывать при входе нечего — и «выбранная заставка» в
        // примечании была бы неправдой.
        let note = if theme_name == crate::appearance_installer::SPLASH_NONE {
            tr("The Rexon startup movie is off.")
        } else {
            msg_slot_released()
        };
        SplashSlotPlan::Apply { keep_video: None, note }
    }

    /// Довести план до конца после применения: вернуть слот ролику, если
    /// покупатель его оставил, и отдать итог с примечанием.
    ///
    /// Лечим ВСЕГДА, а не только при удаче: `plasma-apply-lookandfeel`
    /// успевает записать ksplashrc и в тех прогонах, которые сам считает
    /// неудачными.
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

    // ── Автозапуск при входе ───────────────────────────────────────────

    /// Включён ли автозапуск заставки — порт `is_enabled`.
    pub fn is_enabled(&mut self) -> bool {
        let a = self.autostart_path();
        let u = self.unit_path();
        let af = self.io.is_file(&a);
        // Запись есть, но этот композитор её не исполнит — не «включено».
        let af = af && self.io.login_works(&a);
        let uf = self.io.is_file(&u);
        is_enabled(af, uf)
    }

    /// Фаза 1: autostart-.desktop — порт `_install_desktop_autostart`.
    pub fn install_desktop_autostart(&mut self, python_executable: &str) -> (bool, String) {
        let exec = self.launcher_exec(python_executable);
        // ⛔Пустая строка — заводить заставку нечем. Прежде сюда приезжал
        // мёртвый `python3 -m rexon.app --session-splash`, и запись
        // автозапуска рапортовала успех, ничего не показывая при входе.
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
        // Запись для других окружений легла, но здесь не сработает.
        if let Some(msg) = refusal {
            return (false, msg);
        }
        (true, msg_autostart_enabled())
    }

    /// Снять autostart-.desktop — порт `_remove_desktop_autostart`.
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

    /// Включить заставку при входе — порт `install_autostart` (шаги
    /// `install_hybrid`/`uninstall_hybrid`/фаза 1 — через исполнителя,
    /// чтобы у шима остался ровно тот же шов для сверки).
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
                self.io.step_uninstall_hybrid(); // не оставлять пол-гибрида
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

    /// Отключить заставку при входе — порт `uninstall_autostart`.
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

// ── Прогон заставки: постер, хелпер, mpv-IPC, эстафета, три пути ──────────

/// Нужен ли полный скан для ротации — порт короткого замыкания в
/// `rotation_pool` (скан дорогой: ffmpeg + разбор VDF, и на критическом пути
/// входа он не нужен, если избранного и так хватает).
pub fn rotation_needs_full_scan(existing_favorites: usize) -> bool {
    !rotation_uses_favorites(existing_favorites)
}

/// Команда mpv-IPC «снять паузу».
pub fn unpause_command() -> Value {
    Value::Array(vec![
        Value::from("set_property"),
        Value::from("pause"),
        Value::from(false),
    ])
}

/// Команда mpv-IPC «выйти».
pub fn quit_command() -> Value {
    Value::Array(vec![Value::from("quit")])
}

/// Свойство mpv, по которому опознаётся конец ролика.
pub const MPV_EOF_PROPERTY: &str = "eof-reached";

/// Формат `%g` для секунд в сообщениях (6.0 → «6»).
fn fmt_g(v: f64) -> String {
    let s = format!("{v}");
    s
}

/// X11-слой не создался — откат на управляемое окно mpv.
pub const MSG_X11_OVERLAY_FALLBACK: &str =
    "x11 overlay не создан — откат на --fullscreen/--ontop (панель/попапы могут пробиться)";
/// Rust-хелпер не взлетел — откат на оконный mpv.
pub const MSG_HELPER_NO_LIFTOFF: &str =
    "rexon-splash-overlay не взлетел — фолбэк на mpv-окно";
/// Ни шина, ни бинари не дали reconfigure живому KWin.
pub const MSG_KWIN_RECONFIGURE_FAILED: &str =
    "KWin reconfigure не прошёл: ни сессионная шина, ни бинари qdbus/gdbus не \
сработали — правило окна заставки KWin подхватит при следующем входе";

impl<IO: SplashIo> Splash<'_, IO> {
    /// PNG первого кадра ролика — порт `_poster_for_video`.
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

    /// Собранный rexon-splash-overlay — порт `_find_splash_overlay_binary`
    /// (`repo_root` — realpath корня дерева, наблюдение исполнителя).
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

    /// Команда в mpv-IPC без ответа — порт `_mpv_ipc_send`.
    pub fn mpv_ipc_send(&mut self, sock: &str, command: &Value) -> bool {
        let payload = mpv_ipc_command_payload(command);
        self.io.ipc_send(sock, &payload)
    }

    /// Свойство mpv через IPC — порт `_mpv_ipc_get`.
    pub fn mpv_ipc_get(&mut self, sock: &str, prop: &str) -> Option<Value> {
        let payload = mpv_ipc_get_payload(prop);
        let received = self.io.ipc_request(sock, &payload, MPV_IPC_REPLY_MARKER)?;
        parse_mpv_ipc_response(&received, MPV_IPC_REQUEST_ID)
    }

    /// READY=1 менеджеру systemd — порт `_sd_notify_ready`.
    pub fn sd_notify_ready(&mut self) {
        let ns = self.io.env("NOTIFY_SOCKET");
        let addr = notify_socket_addr(if ns.is_empty() { None } else { Some(&ns) });
        if let Some(a) = addr {
            self.io.notify(&a, SD_NOTIFY_READY);
        }
    }

    /// Жив ли ksplashqml — порт `_ksplash_running`.
    fn ksplash_running(&mut self) -> bool {
        let pg = self.io.which("pgrep");
        if pg.is_empty() {
            return false;
        }
        let argv = pgrep_argv(&pg, KSPLASH_PROCESS);
        matches!(self.io.run(&argv, PGREP_TIMEOUT_SEC), Some(r) if r.rc == 0)
    }

    /// Правило KWin «поверх всего» — порт `_ensure_kwin_splash_rule`.
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
            return true; // актуально, файл не трогаем
        };
        if let Some(e) = self.io.write_text(&path, &new_text) {
            self.io
                .log("warning", &format!("kwin-правило заставки: {e}"));
            return false;
        }
        self.kwin_reconfigure();
        true
    }

    /// Дёрнуть reconfigure живого KWin — порт `_kwin_reconfigure`.
    pub fn kwin_reconfigure(&mut self) {
        if self.io.kwin_reconfigure_dbus(DBUS_TIMEOUT_SEC) {
            // reconfigure асинхронный, а правила применяются при создании
            // окна — дать KWin перечитать их до старта mpv.
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

    /// Эстафета гибрида: дождаться гашения ksplashqml → снять паузу mpv.
    /// Порт `_hybrid_wait_and_unpause`: все ожидания жёстко ограничены,
    /// unpause делается БЕЗУСЛОВНО в конце (mpv никогда не остаётся на
    /// паузе, даже если выше что-то пошло не так).
    pub fn hybrid_wait_and_unpause(&mut self, proc: i64, sock: &str) {
        // 1. mpv поднял IPC-сокет (первый кадр вот-вот/уже отрисован под
        //    оверлеем ksplash). Потолок строго < TimeoutStartSec юнита.
        let mut deadline = self.io.monotonic() + HYBRID_SOCK_WAIT_SEC;
        while self.io.monotonic() < deadline && self.io.poll(proc).is_none() {
            if self.io.exists(sock) {
                break;
            }
            self.io.sleep(SOCK_POLL_SEC);
        }
        // 2. Отпускаем plasmashell (Type=notify). БЕЗУСЛОВНО, даже без
        //    сокета: иначе TimeoutStartSec убьёт юнит вместе с mpv.
        self.sd_notify_ready();
        // 3. ksplashqml мог стартовать чуть позже нас — подождать появления
        //    (сплеш выключен в системе → просто едем дальше).
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
        // 4. Ждём его смерти: счётчик стадий Plasma гасит его после
        //    готовности plasmashell (стадия «desktop»).
        if seen {
            deadline = self.io.monotonic() + KSPLASH_DEATH_WAIT_SEC;
            while self.io.monotonic() < deadline {
                if !self.ksplash_running() {
                    break;
                }
                self.io.sleep(KSPLASH_POLL_SEC);
            }
        }
        // 5. Постер ksplash исчез → под ним тот же кадр в mpv → пускаем видео.
        if self.io.poll(proc).is_none() {
            let cmd = unpause_command();
            self.mpv_ipc_send(sock, &cmd);
        }
    }

    /// Ожидание конца ролика по mpv-IPC — общий финал overlay-путей
    /// (Wayland-хелпер и X11-слой ведут себя одинаково: последний кадр
    /// держится `keep-open=yes`, гасим МЫ по EOF).
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

    /// Гашение живого проигрывателя: `quit` по IPC → SIGTERM → SIGKILL.
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

    /// Дождаться работающего композитора на холодном X11-входе — порт
    /// `_X11SplashOverlay.wait_compositor` (сам цикл был на Python).
    fn wait_compositor(&mut self, overlay: i64) -> bool {
        let deadline = self.io.monotonic() + COMPOSITOR_WAIT_SEC;
        while self.io.monotonic() < deadline {
            if self.io.x11_compositor_present(overlay) {
                // дать композитору отрисовать первую сцену
                self.io.sleep(COMPOSITOR_SETTLE_SEC);
                return true;
            }
            self.io.sleep(COMPOSITOR_POLL_SEC);
        }
        false
    }

    /// Прогон заставки Rust-хелпером rexon-splash-overlay (Wayland) — порт
    /// `_play_wayland_overlay`. None — хелпер не взлетел (умер до сокета),
    /// вызывающий откатывается на оконный mpv.
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
        // Ранняя диагностика: сокет появляется после mpv_initialize, т.е.
        // фатальные отказы среды (нет Wayland/layer-shell/EGL) уже позади.
        // Крах ловится отдельно и мгновенно, дедлайн ждёт «жив, но не готов».
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
            self.sd_notify_ready(); // вдруг нас всё же запустил notify-юнит
        }
        let res = self.wait_eof(proc, ipc_sock, true);
        // Чистый выход: quit по IPC, страховки — SIGTERM (хелпер его ловит и
        // корректно освобождает mpv/EGL), затем SIGKILL.
        self.quit_player(proc, ipc_sock);
        Some(res)
    }

    /// Проиграть заставку как полноэкранный сплеш — порт `play`.
    ///
    /// `path`/`sound` по умолчанию из конфига; `blocking=true` — ждать конца
    /// (autostart/юнит), `false` — вернуть управление сразу (тест из UI);
    /// `hybrid=None` — авто. `repo_root` — корень дерева для поиска хелпера.
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
        // Реальный прогон (запуск из systemd-юнита при входе) — сперва
        // починить связку, потом играть: чужая тема в ksplashrc даёт секунду
        // ШТАТНОЙ заставки Plasma перед нашим роликом (шапка модуля).
        // В НЕблокирующем режиме (тест из UI) ksplashrc трогать незачем.
        if blocking && hybrid {
            self.heal_hybrid(&path);
        }
        let display = self.io.env("DISPLAY");
        let wayland = self.io.env("WAYLAND_DISPLAY");
        let disp_opt = if display.is_empty() { None } else { Some(display.as_str()) };
        let wl_opt = if wayland.is_empty() { None } else { Some(wayland.as_str()) };
        // Wayland + реальный прогон: предпочтителен Rust-хелпер (layer-shell
        // OVERLAY — поверх панелей KDE, свой libmpv внутри). Тест из UI идёт
        // оконным mpv: хелпер держит последний кадр до нашего гашения, а в
        // non-blocking режиме гасить некому.
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
        // X11 (только реальный прогон): свой override-redirect слой + --wid —
        // WM-независимо, панель/попапы не пробьются.
        let mut overlay: Option<i64> = None;
        if blocking && is_x11_session(disp_opt, wl_opt) {
            match self.io.x11_create() {
                None => self.io.log("warning", MSG_X11_OVERLAY_FALLBACK),
                Some(h) => {
                    overlay = Some(h);
                    // На X11 эстафета ksplash→mpv не используется:
                    // ksplashqml гаснет через ~0.4 c после нашего READY, а
                    // слой и так перекрывает его окно. Постер фоном слоя
                    // держит экран ~1.5 c инициализации mpv, и видео
                    // начинается с того же кадра без чёрной паузы.
                    hybrid = false;
                    self.sd_notify_ready();
                    let poster = self.poster_for_video(&path);
                    // Постер ДО маппинга: фоновый pixmap рисует сам
                    // X-сервер, композитор ему не нужен — холодный вход
                    // обходится без чёрного зазора.
                    if let Some(p) = poster {
                        self.io.x11_set_poster(h, &p);
                    }
                    self.io.x11_map(h);
                    if !self.io.x11_compositor_present(h) {
                        // холодный вход: видео пускать после перехвата
                        // композитинга
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
            // Хелпер не взлетел (умер до IPC-сокета) → оконный mpv ниже.
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
                    // --ontop на Wayland мёртв — поверх (в т.ч. над панелью
                    // Plasma) держит правило KWin по app_id.
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
            self.sd_notify_ready(); // вдруг нас всё же запустил notify-юнит
        }
        let res = if overlay.is_some() {
            // Финал в правильном порядке: mpv стоит на последнем кадре
            // (--keep-open=yes), по EOF закрываем СНАЧАЛА слой — с ним
            // рушится и окно mpv, постер не мелькает.
            self.wait_eof(proc, &ipc_sock, false)
        } else {
            match self.io.wait(proc, PLAY_TIMEOUT_SEC) {
                None => {
                    self.io.terminate(proc); // страховка: не блокировать сессию
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
            // слой уже закрыт — гасим mpv, стоявший на последнем кадре
            self.quit_player(proc, &ipc_sock);
        }
        res
    }

    /// CLI-точка (`rexon --session-splash`) — порт `main`.
    ///
    /// Ролик на следующий вход выбирается ПОСЛЕ показа: вход уже отдан
    /// рабочему столу, и ffmpeg для постера ничего не задерживает.
    pub fn main(&mut self, repo_root: &str) -> i64 {
        let (ok, msg) = self.play(None, None, true, None, repo_root);
        if !ok {
            // тихо: autostart не должен мешать входу в сессию
            self.io.log("warning", &format!("session-splash: {msg}"));
        }
        if let Some(nxt) = self.advance_rotation() {
            self.io.log("info", &format!("следующая заставка: {nxt}"));
        }
        0
    }
}
