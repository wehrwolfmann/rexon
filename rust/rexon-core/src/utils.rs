//! Порт `rexon/utils.py` — общих утилит: журналирование, нормализация путей
//! пользователя, ЕДИНЫЙ поиск внешних инструментов и обращения к сессионной
//! шине D-Bus.
//!
//! ⚠️ Переключателя `REXON_RUST_BACKEND=off` в проекте больше НЕТ. Он уводил
//! шимы на Python-копии логики, но копий не осталось ни в одном модуле:
//! выключать нечего, а единственным действием переменной стало бы «сломать
//! программу». Вместе с копиями убрана и она — и здешний её эталон.
//!
//! Остальное — настоящая логика (разбор путей, перебор синонимов бинарей,
//! разбор ответа `dbus-send`, сборка argv), и она портирована целиком.
//! Запуск процессов и настройка `logging` остаются на Python: там ядро
//! ничего не добавляет, кроме лишнего перехода через мост.
//!
//! ⚠️ Отличие от Python в ОДНОМ месте: право на исполнение проверяется по
//! биту `x` в правах файла, а не вызовом `access(2)` (у ядра нет зависимости
//! на libc, а `std` доступа к `access` не даёт). Для бинарей дистрибутива
//! (режим 0755) это то же самое; разойтись может лишь файл с правами вида
//! 0700 от другого владельца. Поэтому Python, получив от ядра «не нашёл»,
//! всё равно доискивает сам — ложного «инструмента нет» из-за ядра быть не
//! может (см. `rexon/utils.py::resolve_tool`).

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// ============================================================================
// Журналирование
// ============================================================================

/// Имя корневого журнала приложения.
pub const LOGGER_NAME: &str = "rexon";

/// Формат записи журнала.
pub const LOG_FORMAT: &str = "%(asctime)s [%(name)s] %(levelname)s: %(message)s";

/// Формат отметки времени.
pub const LOG_DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Уровень журнала: числовые значения совпадают с `logging` Python.
pub const LEVEL_DEBUG: i32 = 10;
/// Предупреждение.
pub const LEVEL_WARNING: i32 = 30;

// ============================================================================
// Пути пользователя (ЕДИНЫЙ расширитель на всё ядро)
// ============================================================================

/// Каталог без хвостовых `/`, но корень остаётся корнем.
///
/// Соответствует тому, что делает `os.path.expanduser` со значением `$HOME`
/// (`userhome.rstrip('/')`), и заодно приводит к одному виду значения
/// `$XDG_*_HOME`: с хвостовым слешем в переменной пути потом склеивались
/// через `//`.
fn norm_dir(dir: &str) -> &str {
    if !dir.is_empty() && dir.chars().all(|c| c == '/') {
        return "/";
    }
    dir.trim_end_matches('/')
}

/// Склейка пути ровно как `os.path.join(a, b)` для относительного `b`.
///
/// Отдельная функция, потому что наивный `format!("{a}/{b}")` на корне даёт
/// `//x`, а на пустом `b` — `a/` вместо `a` (или наоборот): расхождение с
/// Python вылезло бы уже в путях к конфигам.
fn join_path(dir: &str, tail: &str) -> String {
    if tail.starts_with('/') {
        return tail.to_string();
    }
    if dir.is_empty() || dir.ends_with('/') {
        return format!("{dir}{tail}");
    }
    format!("{dir}/{tail}")
}

/// Корень пользовательских конфигов: `$XDG_CONFIG_HOME` либо `~/.config`.
///
/// ⚠️ ЕДИНАЯ точка. Раньше `~/.config` вшивался в каждый модуль по месту, и
/// при нестандартном `XDG_CONFIG_HOME` пути уезжали мимо реальных файлов:
/// systemd-юнит вставал туда, где `systemctl --user enable` его не ищет
/// (автозапуск демона обоев и заставки молча не включался), autostart-
/// .desktop и правило KWin писались мимо живых файлов, профили
/// `mimeapps.list` — мимо каталога, откуда их читают DE. Новые пути к
/// пользовательским конфигам строить ТОЛЬКО отсюда (правило #12).
pub fn config_home(xdg_config_home: Option<&str>, home: &str) -> String {
    match xdg_config_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".config"),
    }
}

/// Корень пользовательских данных: `$XDG_DATA_HOME` либо `~/.local/share`.
///
/// Пара к [`config_home`] и такой же ЕДИНЫЙ источник. Заведена по тем же
/// причинам: путь вшивался литералом больше чем в сотне мест, и при
/// нестандартном `XDG_DATA_HOME` (Flatpak, Steam-runtime, кастомные профили)
/// темы, курсоры, шрифты, плазмоиды и расширения `.rxn` ставились мимо
/// каталога, который читает система, — установка «проходила», а результата
/// не было.
pub fn data_home(xdg_data_home: Option<&str>, home: &str) -> String {
    match xdg_data_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".local/share"),
    }
}

/// Корень пользовательского кэша: `$XDG_CACHE_HOME` либо `~/.cache`. Третий
/// в паре к [`config_home`] и [`data_home`], с тем же разбором.
pub fn cache_home(xdg_cache_home: Option<&str>, home: &str) -> String {
    match xdg_cache_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".cache"),
    }
}

/// Имя папки Rexon внутри корня конфигов. Одно на весь продукт: и путь
/// настроек, и список удаления берут его отсюда.
pub const REXON_DIR_NAME: &str = "rexon";

/// Каталог настроек Rexon: `<`[`config_home`]`>/rexon`.
///
/// ⚠️ ЕДИНАЯ точка для ВСЕХ хранилищ Rexon в конфигах (`settings.json`,
/// `favorites.json`, `bridge.json`, `bridge_commands/`, `screenshots/`,
/// `extensions/`, `themes.json`, `ruffle/`, `crashes/`…). Раньше половина
/// модулей склеивала путь сама — часть через `expand_user`/`config_home`
/// (то есть по `$XDG_CONFIG_HOME`), часть литералом `~/.config/rexon` от
/// голого `$HOME`. На машине с нестандартным `XDG_CONFIG_HOME` получались
/// ДВЕ разные папки настроек: одна половина продукта писала в одну, другая
/// читала другую, а удаление Rexon сносило только XDG-шную. Новые пути к
/// данным Rexon в конфигах строить ТОЛЬКО отсюда (правило #12).
pub fn rexon_config_dir_at(xdg_config_home: Option<&str>, home: &str) -> String {
    join_path(&config_home(xdg_config_home, home), REXON_DIR_NAME)
}

/// Корень конфигов по ОКРУЖЕНИЮ процесса (`$XDG_CONFIG_HOME`, иначе
/// `$HOME/.config`; пустой/отсутствующий `$HOME` → текущий каталог, как у
/// `os.path.expanduser`).
///
/// Единственное место в ядре, где корень конфигов снимается с окружения:
/// раньше эти четыре строки были скопированы в `settings.rs`,
/// `qt_extensions.rs`, `favorites.rs` и `constants.rs` — с разным разбором
/// пустой строки.
pub fn config_home_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let home = if home.is_empty() { ".".to_string() } else { home };
    config_home(env_nonempty("XDG_CONFIG_HOME").as_deref(), &home)
}

/// [`rexon_config_dir_at`] по окружению процесса.
pub fn rexon_config_dir() -> PathBuf {
    PathBuf::from(join_path(&config_home_env(), REXON_DIR_NAME))
}

// ============================================================================
// Запись в профиль покупателя — ОДНА дверь
// ============================================================================

/// Суффикс имени временного файла атомарной записи. Имя начинается с точки
/// (скрытый файл) и НЕ повторяет имя цели целиком, поэтому обходы каталогов,
/// отбирающие файлы по префиксу или расширению (`crash_*.log`, `*.json`),
/// недописанный файл не видят.
fn atomic_tmp_name(name: &str) -> String {
    format!(".{name}.part-{}", std::process::id())
}

/// Атомарно записать текст: временный файл в том же каталоге → `fsync` →
/// `rename`. При ошибке временный файл убирается, окончательный не тронут.
///
/// ⚠️ **ЕДИНСТВЕННАЯ дверь записи в профиль покупателя** (`~/.config/rexon`,
/// `~/.local/share/rexon`, а также настройки рабочего стола и приложений по
/// умолчанию, которые Rexon переписывает от имени человека). Обычный
/// `fs::write` файл сначала ОБРЕЗАЕТ и только потом наполняет: закрытое в эту
/// миллисекунду окно или пропавшее питание оставляют на диске половину.
/// Половина `settings.json` — это потерянные настройки покупателя, половина
/// `bridge.json` — переставший отвечать мост, половина `.desktop` автозапуска
/// — мёртвая строка, которая оживает уже при входе в сеанс, когда объяснить
/// поломку некому.
///
/// Временный файл кладётся РЯДОМ с целевым, а не в `/tmp`: `rename` атомарен
/// только в пределах одной файловой системы, а `/tmp` здесь — ОЗУ (tmpfs), то
/// есть другая. Через `/tmp` вместо переименования вышло бы копирование, и
/// обрезанный файл вернулся бы ровно туда, откуда его убирали.
///
/// `fsync` до переименования — не перестраховка: без него `rename` может
/// попасть на диск раньше содержимого, и после сбоя питания на месте целого
/// старого файла окажется целый новый — из нулей.
///
/// Каталог НЕ создаётся: отсутствие каталога — ошибка, а не повод молча
/// завести его в неожиданном месте. Кому нужен `mkdir -p`, делает его сам.
pub fn atomic_write(path: &str, text: &str) -> std::io::Result<()> {
    atomic_write_bytes(path, text.as_bytes())
}

/// Куда на самом деле писать: символьная ссылка разворачивается в свою цель.
///
/// ⚠️ Это НЕ придирка. Подмена имени бьёт по самой ссылке: обычная запись шла
/// сквозь неё и наполняла файл на том конце, а `rename` положил бы на её
/// место простой файл — и у человека, который держит конфиги в своём
/// хранилище и расставляет их ссылками, связь с хранилищем молча оборвалась
/// бы. Разворачиваем только существующую ссылку; битая ссылка (цели нет)
/// остаётся как есть, её и заменяем.
fn atomic_target(path: &Path) -> std::borrow::Cow<'_, Path> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => match fs::canonicalize(path) {
            Ok(real) => std::borrow::Cow::Owned(real),
            Err(_) => std::borrow::Cow::Borrowed(path),
        },
        _ => std::borrow::Cow::Borrowed(path),
    }
}

/// То же, что [`atomic_write`], но для сырых байт (снимки экрана, превью).
///
/// Права уже лежащего файла сохраняются: подмена имени заводит НОВЫЙ файл, и
/// без этого закрытый покупателем конфиг (`0600`) после первого же сохранения
/// стал бы доступен на чтение всем.
pub fn atomic_write_bytes(path: &str, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let target = atomic_target(Path::new(path));
    let target = target.as_ref();
    let name = target
        .file_name()
        .ok_or_else(|| std::io::Error::other("у пути нет имени файла"))?
        .to_string_lossy()
        .into_owned();
    let tmp = target.with_file_name(atomic_tmp_name(&name));
    let keep_mode = fs::metadata(target).ok().map(|m| m.permissions().mode());
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        if let Some(mode) = keep_mode {
            f.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        f.sync_all()?;
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// [`atomic_write`] с сообщением об ошибке строкой — форма для исполнителей,
/// у которых в договоре `Result<(), String>`.
pub fn atomic_write_str_err(path: &str, text: &str) -> Result<(), String> {
    atomic_write(path, text).map_err(|e| e.to_string())
}

/// Переменная окружения, если она задана и НЕ пуста (пустая = не задана —
/// так её трактует спецификация XDG).
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `os.path.expanduser` с поправкой на XDG: `~` → `home`, ветки
/// `~/.config/…` и `~/.local/share/…` уходят в [`config_home`] и
/// [`data_home`], остальное раскрывается как обычно.
///
/// Совместимая обёртка над [`expand_user_xdg`] без `$XDG_DATA_HOME` (то есть
/// `~/.local/share` = `{home}/.local/share` — ровно то, что и получалось до
/// появления ветки данных).
pub fn expand_user(path: &str, home: &str, xdg_config_home: Option<&str>) -> String {
    expand_user_xdg(path, home, xdg_config_home, None)
}

/// Полный расширитель: обе XDG-ветки.
///
/// ⚠️ Границы префиксов проверяются ТОЧНО (`~/.config` целиком либо
/// `~/.config/…`). В оригинале стояло `startswith("~/.config")`, и реальные
/// пути-соседи резались посередине: `~/.config-old/x` превращался в
/// `~/.config/-old/x`, а `~/.local/shareX` — в `~/.local/share/X`. Правило
/// #16: баг оригинала правится в порте, а не консервируется.
pub fn expand_user_xdg(
    path: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    xdg_data_home: Option<&str>,
) -> String {
    if path == "~" {
        let h = norm_dir(home);
        return if h.is_empty() { "/".to_string() } else { h.to_string() };
    }
    let Some(rest) = path.strip_prefix("~/") else {
        // Всё, что не начинается с `~/`, Python отдаёт как есть (кроме
        // `~имя`, которое ядру не по силам — см. [`expand_user_checked`]).
        return path.to_string();
    };
    if let Some(tail) = xdg_tail(rest, ".config") {
        let base = config_home(xdg_config_home, home);
        return if tail.is_empty() { base } else { join_path(&base, tail) };
    }
    if let Some(tail) = xdg_tail(rest, ".local/share") {
        let base = data_home(xdg_data_home, home);
        return if tail.is_empty() { base } else { join_path(&base, tail) };
    }
    join_path(norm_dir(home), rest)
}

/// Хвост после XDG-префикса, если путь и правда лежит под ним.
///
/// `Some("")` — сам каталог, `Some("x/y")` — вложенное, `None` — префикс не
/// совпал по ГРАНИЦЕ (`.config-old` ≠ `.config`).
fn xdg_tail<'a>(rest: &'a str, prefix: &str) -> Option<&'a str> {
    if rest == prefix {
        return Some("");
    }
    rest.strip_prefix(prefix)
        .filter(|t| t.starts_with('/'))
        .map(|t| t.trim_start_matches('/'))
}

/// Расширитель для моста: `None` — «ядро за это не отвечает, решай сам».
///
/// Отдаёт `None` на путях вида `~имя` (нужен `pwd`, которого у ядра нет) и
/// при неизвестном/пустом `$HOME`. Так живой Python в этих случаях уходит на
/// `os.path.expanduser` и результат не может разойтись с оригиналом.
pub fn expand_user_checked(
    path: &str,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
    xdg_data_home: Option<&str>,
) -> Option<String> {
    let home = home.filter(|h| !h.is_empty())?;
    if path.starts_with('~') && path != "~" && !path.starts_with("~/") {
        return None;
    }
    Some(expand_user_xdg(path, home, xdg_config_home, xdg_data_home))
}

/// Корень состояния программ: `$XDG_STATE_HOME` либо `~/.local/state`.
///
/// Четвёртый в паре к [`config_home`]/[`data_home`]/[`cache_home`]. Журнал —
/// это состояние, а не настройка: по спецификации XDG ему место здесь, и
/// при переносе настроек на другую машину он не едет вместе с ними.
pub fn state_home(xdg_state_home: Option<&str>, home: &str) -> String {
    match xdg_state_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".local/state"),
    }
}

/// Каталог журнала: `<state>/rexon`.
pub fn log_dir(home: &str) -> String {
    log_dir_xdg(None, home)
}

/// Каталог журнала с учётом `$XDG_STATE_HOME`.
///
/// ⚠️ До 08.10.2026 журнал лежал в `~/.config/rexon/rexon.log` и рос без
/// предела (1,5 МБ за месяц у владельца). Прежний файл не переносится и не
/// удаляется: это чужая уже история, а не наш рабочий файл.
pub fn log_dir_xdg(xdg_state_home: Option<&str>, home: &str) -> String {
    join_path(&state_home(xdg_state_home, home), REXON_DIR_NAME)
}

/// Файл журнала (`<state>/rexon/rexon.log`) — без учёта окружения.
pub fn log_file(home: &str) -> String {
    join_path(&log_dir(home), "rexon.log")
}

/// Файл журнала по ОКРУЖЕНИЮ процесса — единственный путь, которым пишет
/// `crate::applog`.
pub fn log_file_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let home = if home.is_empty() { ".".to_string() } else { home };
    join_path(
        &log_dir_xdg(env_nonempty("XDG_STATE_HOME").as_deref(), &home),
        "rexon.log",
    )
}

// ============================================================================
// Запуск внешних команд
// ============================================================================

/// Таймаут внешней команды по умолчанию, секунды — класс «быстрый опрос».
///
/// ⚠️ Имя оставлено прежним, но смысл у него теперь узкий: это дефолт ТОЛЬКО
/// для быстрых команд. Единого дефолта на все вызовы больше нет — см.
/// `default_timeout_secs`.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Быстрый опрос: `pacman -Q`, `systemctl is-enabled`, `gsettings get`,
/// `xdg-mime query`. Секунды. Не ответила за полминуты — что-то не так.
pub const QUICK_TIMEOUT_SECS: u64 = 30;

/// Скачивание и установка из репозиториев: зависит от сети и размера
/// набора пакетов. Полчаса — с запасом на медленное зеркало и полный стол.
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 1800;

/// Сборка из исходников (AUR, Gentoo): десятки минут и целиком зависит от
/// машины. Четыре часа — не «сколько нужно», а «столько ждать бессмысленно
/// даже на слабой машине»: это защита от намертво вставшего процесса, а не
/// оценка времени сборки.
pub const BUILD_TIMEOUT_SECS: u64 = 14400;

/// Класс времени работы внешней команды.
///
/// ЗАЧЕМ. Дефолт был один на весь проект — 30 секунд, — и им же меряли
/// сборку девяти пакетов из AUR. Установка стола MATE 2026-09-01 упала
/// целиком: `yay` убивали через полминуты, покупатель читал «не
/// установились AUR (9 из 9)» и про время не знал ничего. Классов ровно
/// три, и различает их не имя вызывающего, а то, ЧТО команда делает.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunDuration {
    /// Опрос состояния: отвечает сразу или не отвечает вовсе.
    Quick,
    /// Скачивание и установка готовых пакетов: минуты, зависит от сети.
    Download,
    /// Сборка из исходников: десятки минут, зависит от машины.
    Build,
}

impl RunDuration {
    /// Таймаут класса, секунды.
    pub fn timeout_secs(self) -> u64 {
        match self {
            RunDuration::Quick => QUICK_TIMEOUT_SECS,
            RunDuration::Download => DOWNLOAD_TIMEOUT_SECS,
            RunDuration::Build => BUILD_TIMEOUT_SECS,
        }
    }

    /// Короткое имя класса — для журнала и для моста в Python.
    pub fn as_str(self) -> &'static str {
        match self {
            RunDuration::Quick => "quick",
            RunDuration::Download => "download",
            RunDuration::Build => "build",
        }
    }
}

/// Обёртки, за которыми прячется настоящая команда: их снимаем перед
/// разбором. `env` умеет ещё и `VAR=VAL` перед именем — их тоже пропускаем.
pub const RUN_WRAPPERS: &[&str] = &["pkexec", "sudo", "doas", "env", "nice", "ionice", "stdbuf"];

/// Инструменты, которые СОБИРАЮТ из исходников: помощники AUR, makepkg,
/// portage. Время работы задаёт машина, а не сеть.
pub const BUILD_TOOLS: &[&str] = &[
    "yay", "paru", "trizen", "pikaur", "aurman", "aura", "pacaur", "makepkg", "pamac-build",
    "emerge", "nix-env", "nix",
];

/// Пакетные менеджеры готовых пакетов и их подкоманды, меняющие систему.
/// Всё остальное у них (`search`, `info`, `list`, `-Q`) — быстрый опрос.
pub const DOWNLOAD_TOOLS: &[(&str, &[&str])] = &[
    ("apt", &["install", "upgrade", "full-upgrade", "dist-upgrade", "remove", "purge", "build-dep", "update", "reinstall"]),
    ("apt-get", &["install", "upgrade", "full-upgrade", "dist-upgrade", "remove", "purge", "build-dep", "update", "reinstall"]),
    ("dnf", &["install", "upgrade", "update", "remove", "reinstall", "group", "groupinstall", "distro-sync"]),
    ("yum", &["install", "upgrade", "update", "remove", "reinstall", "groupinstall"]),
    ("zypper", &["install", "in", "update", "up", "dup", "remove", "rm", "refresh", "ref", "patch"]),
    ("xbps-install", &[]),
    ("xbps-remove", &[]),
    ("apk", &["add", "del", "upgrade", "update", "fix"]),
    ("eopkg", &["install", "it", "remove", "rm", "upgrade", "up", "update-repo", "ur"]),
    ("swupd", &["bundle-add", "bundle-remove", "update"]),
    ("flatpak", &["install", "update", "uninstall"]),
    ("snap", &["install", "refresh", "remove"]),
    ("rpm-ostree", &["install", "upgrade", "uninstall", "update"]),
    ("pamac", &["install", "update", "remove", "upgrade"]),
    ("distrobox-create", &[]),
    ("git", &["clone", "fetch", "pull", "submodule"]),
    ("curl", &[]),
    ("wget", &[]),
];

/// Кусочки путей наших привилегированных обёрток: за `pkexec bash
/// /tmp/rexon_install_*.sh` прячется установка целого стола, и по argv её
/// иначе не отличить от любого другого скрипта.
pub const REXON_INSTALL_MARKERS: &[&str] =
    &["rexon-de-helper", "rexon_install_", "rexon_steamos_", "rexon-stage-helper"];

/// Снять обёртки (`pkexec`, `sudo`, `env VAR=VAL`, …) — вернуть индекс
/// первого аргумента настоящей команды.
fn strip_wrappers(argv: &[String]) -> usize {
    let mut i = 0;
    while i < argv.len() {
        let base = basename(&argv[i]);
        if !RUN_WRAPPERS.contains(&base) {
            break;
        }
        i += 1;
        // `env`/`sudo` допускают присваивания перед именем команды.
        while i < argv.len() && argv[i].contains('=') && !argv[i].starts_with('-') {
            i += 1;
        }
        // Флаги самой обёртки (`nice -n 19`, `sudo -u user`) пропускаем
        // вместе с их значением — иначе примем флаг за имя команды.
        while i < argv.len() && argv[i].starts_with('-') {
            i += 1;
        }
    }
    i
}

/// Последний компонент пути — имя исполняемого файла.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Меняет ли `pacman` систему (установка/удаление/обновление) — в отличие
/// от опроса (`-Q…`, `-Ss`, `-Si`, `-T`, `-F`).
fn pacman_is_mutating(args: &[String]) -> bool {
    for a in args {
        if let Some(long) = a.strip_prefix("--") {
            match long {
                "sync" | "upgrade" | "remove" | "database" => return true,
                "query" | "search" | "info" | "deptest" | "files" | "version" | "help" => {
                    return false
                }
                _ => continue,
            }
        }
        let Some(letters) = a.strip_prefix('-') else {
            continue;
        };
        let mut chars = letters.chars();
        let Some(op) = chars.next() else { continue };
        match op {
            'Q' | 'T' | 'F' | 'V' | 'h' => return false,
            'S' => {
                // `-Ss`/`-Si`/`-Sg`/`-Sl`/`-Sp` — это опросы, не установка.
                return !letters[1..].chars().any(|c| matches!(c, 's' | 'i' | 'g' | 'l' | 'p'));
            }
            'U' | 'R' | 'D' => return true,
            _ => continue,
        }
    }
    false
}

/// Только опрос состояния — ни установки, ни сборки.
fn is_query_only(args: &[String]) -> bool {
    args.iter().any(|a| {
        a.starts_with("-Q")
            || matches!(
                a.as_str(),
                "-Ss" | "-Si" | "-T" | "-F" | "-V" | "-h"
                    | "--query" | "--search" | "--info" | "--version" | "--help"
            )
    })
}

/// Класс времени работы команды по её argv.
///
/// Разбор идёт по СМЫСЛУ команды, а не по тому, кто её позвал: одно и то же
/// `pacman -Q` быстро и в проверке соответствия, и в установке стола, а
/// `yay -S` долго везде.
pub fn run_duration(argv: &[String]) -> RunDuration {
    if argv.is_empty() {
        return RunDuration::Quick;
    }
    // Наши привилегированные обёртки: имя скрипта говорит, что внутри.
    if argv
        .iter()
        .any(|a| REXON_INSTALL_MARKERS.iter().any(|m| a.contains(m)))
    {
        return RunDuration::Download;
    }
    let start = strip_wrappers(argv);
    let Some(head) = argv.get(start) else {
        return RunDuration::Quick;
    };
    let tool = basename(head);
    let rest = &argv[(start + 1).min(argv.len())..];

    if BUILD_TOOLS.contains(&tool) {
        // Помощник AUR умеет и опрашивать: `yay -Q`, `yay -Ss`. Всё
        // остальное у него — сборка, и предполагать иное нельзя: ошибка в
        // эту сторону убивает сборку по таймауту, ошибка в другую — лишь
        // дольше ждёт зависший опрос.
        return if is_query_only(rest) {
            RunDuration::Quick
        } else {
            RunDuration::Build
        };
    }
    if tool == "pacman" || tool == "pacman-key" {
        return if pacman_is_mutating(rest) {
            RunDuration::Download
        } else {
            RunDuration::Quick
        };
    }
    for (name, subcommands) in DOWNLOAD_TOOLS {
        if tool != *name {
            continue;
        }
        if subcommands.is_empty() {
            return RunDuration::Download;
        }
        if rest
            .iter()
            .any(|a| subcommands.contains(&a.as_str()))
        {
            return RunDuration::Download;
        }
        return RunDuration::Quick;
    }
    RunDuration::Quick
}

/// Таймаут по умолчанию для команды, секунды, — когда вызывающий свой не
/// задал.
pub fn default_timeout_secs(argv: &[String]) -> u64 {
    run_duration(argv).timeout_secs()
}

/// Что нужно доставить в вызов внешней команды сверх переданного.
///
/// Захват вывода без явной кодировки даёт байты и падает на нелатинице в
/// выводе пакетных менеджеров, поэтому при захвате всегда навязывается
/// текстовый режим с заменой негодных байт.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunDefaults {
    /// Включить текстовый режим с utf-8 и заменой негодных байт.
    pub text_mode: bool,
    /// Таймаут, если вызывающий его не задал.
    pub timeout_secs: Option<u64>,
}

/// Дополнения к вызову внешней команды.
///
/// Таймаут подставляется ПО КЛАССУ команды (`run_duration`), а не один на
/// всех: тем же дефолтом, которым меряют `pacman -Q`, нельзя мерить сборку
/// из AUR.
pub fn run_defaults(argv: &[String], has_capture_output: bool, has_timeout: bool) -> RunDefaults {
    RunDefaults {
        text_mode: has_capture_output,
        timeout_secs: if has_timeout {
            None
        } else {
            Some(default_timeout_secs(argv))
        },
    }
}

/// Сообщение о таймауте команды.
pub fn msg_timeout(command: &str, timeout: &str) -> String {
    // TRANSLATORS: %1 is a command, %2 the time limit in seconds.
    crate::i18n::trf("Command %1: timed out (%2 s)", &[&command, &timeout])
}

/// Сообщение об ошибке запуска.
pub fn msg_run_failed(command: &str, error: &str) -> String {
    // TRANSLATORS: %1 is a command, %2 the system's error text.
    crate::i18n::trf("Couldn't run the command %1: %2", &[&command, &error])
}

// ============================================================================
// Внешние инструменты — ЕДИНАЯ точка поиска на весь проект
// ============================================================================
//
// Правило #12 (чинить движок, а не случаи). Один и тот же класс дефекта
// ловился трижды: код зовёт бинарь по ОДНОМУ имени, а в целевом дистрибутиве
// он называется иначе или лежит вне PATH — и провал молчаливый: в журнал
// уходит строчка, а UI рапортует об успехе.
//
//   • `qdbus` — такого имени нет ни на одной целевой системе (qdbus6 /
//     qdbus-qt6), а на Kubuntu он ещё и вне PATH (Launchpad #2084278);
//   • `kwriteconfig6`/`kreadconfig6` — на Ubuntu/Kubuntu 24.04 LTS (Plasma 5)
//     их нет вообще, есть только `*5`: там молча не применялись тема, цвета,
//     шрифты, курсор, KSplash, обои экрана блокировки и правила KWin;
//   • `swww` — апстрим переименовал проект в `awww` (окт. 2025), пакет кладёт
//     только `awww`/`awww-daemon`.
//
// Поэтому имена бинарей проходят через ОДИН резолвер: перебор синонимов →
// PATH → каталоги вне PATH → одно понятное сообщение с именем пакета для
// ТЕКУЩЕГО дистрибутива.

/// Наборы синонимов: каноническое имя → (кандидаты, каталоги вне PATH).
///
/// Каноническое имя — то, которое пишут в argv вызывающие модули. Порядок
/// кандидатов значим: первый найденный и берётся.
pub const TOOL_SETS: &[(&str, &[&str], &[&str])] = &[
    // KDE Frameworks: 6 → 5 (Ubuntu/Kubuntu 24.04 LTS = Plasma 5)
    ("kwriteconfig6", &["kwriteconfig6", "kwriteconfig5"], &[]),
    ("kreadconfig6", &["kreadconfig6", "kreadconfig5"], &[]),
    ("kpackagetool6", &["kpackagetool6", "kpackagetool5"], &[]),
    ("kbuildsycoca6", &["kbuildsycoca6", "kbuildsycoca5"], &[]),
    ("kquitapp6", &["kquitapp6", "kquitapp5"], &[]),
    // Запуск программы отдельно от вызывающего: в Plasma 6 бинарь зовётся
    // `kstart`, в Plasma 5 — `kstart5`.
    ("kstart", &["kstart", "kstart5"], &[]),
    ("kdialog", &["kdialog"], &[]),
    // Обои под Wayland: swww переименован в awww, аргументы совместимы
    ("swww", &["swww", "awww"], &[]),
    ("swww-daemon", &["swww-daemon", "awww-daemon"], &[]),
    // ImageMagick: на IM7 `magick` — drop-in замена `convert`,
    // на Ubuntu 24.04 (IM6) наоборот есть только `convert`
    ("convert", &["convert", "magick"], &[]),
    // 7-Zip: пакет `7zip` кладёт `7zz` (Debian/Ubuntu), старый p7zip — `7za`;
    // аргументы `x -o<каталог>` у всех троих одинаковые
    ("7z", &["7z", "7zz", "7za"], &[]),
    // Plymouth: на Debian/openSUSE бинарь лежит в /usr/sbin, которого нет
    // в PATH обычного пользователя
    (
        "plymouth-set-default-theme",
        &["plymouth-set-default-theme"],
        &["/usr/sbin", "/sbin"],
    ),
    (
        "update-alternatives",
        &["update-alternatives"],
        &["/usr/sbin", "/sbin"],
    ),
    (
        "update-initramfs",
        &["update-initramfs"],
        &["/usr/sbin", "/sbin"],
    ),
    // ⚠️ pactl СИНОНИМОВ НЕ ИМЕЕТ. wpctl (wireplumber) решает ту же задачу,
    // но у него другой набор команд и другой вывод — подменять argv[0]
    // нельзя, это дало бы мусор вместо списка устройств. Ветка wpctl —
    // в `audio_capture::wpctl_*`, а не подстановкой имени.
    // Qt-клиент D-Bus: имя своё в каждом дистрибутиве, на Kubuntu ещё и
    // вне PATH.
    (
        "qdbus6",
        &["qdbus6", "qdbus-qt6", "qdbus", "qdbus-qt5"],
        &[
            "/usr/lib/qt6/bin",
            "/usr/lib64/qt6/bin",
            "/usr/lib/*/qt6/bin",
        ],
    ),
];

/// Инструменты, у которых своего бинаря может не быть вовсе — только
/// подкоманда общего.
///
/// ImageMagick 7 постепенно убирает отдельные бинари: `convert` ещё живёт в
/// наборе синонимов выше (там это ОДНО слово: `magick` — drop-in замена), а
/// `identify` в IM7 существует как `magick identify`, то есть ДВА слова, и
/// подменой argv[0] это не лечится. Значение — полный префикс команды.
pub const TOOL_SUBCOMMANDS: &[(&str, &[&str])] = &[("identify", &["magick", "identify"])];

/// Имя пакета по бинарю и семейству пакетного менеджера.
///
/// Чтобы и в журнале, и в UI (правило #13) говорить человеку, ЧТО поставить,
/// а не «команда не найдена». Ключи семейств — значения `system::PkgFamily`.
/// Порядок семейств внутри записи значим: при неизвестном семействе берётся
/// `debian`, а если и его нет — ПЕРВОЕ значение записи.
pub const TOOL_PACKAGES: &[(&str, &[(&str, &str)])] = &[
    (
        "convert",
        &[
            ("arch", "imagemagick"),
            ("fedora", "ImageMagick"),
            ("suse", "ImageMagick"),
            ("debian", "imagemagick"),
        ],
    ),
    (
        "magick",
        &[
            ("arch", "imagemagick"),
            ("fedora", "ImageMagick"),
            ("suse", "ImageMagick"),
            ("debian", "imagemagick"),
        ],
    ),
    (
        "identify",
        &[
            ("arch", "imagemagick"),
            ("fedora", "ImageMagick"),
            ("suse", "ImageMagick"),
            ("debian", "imagemagick"),
        ],
    ),
    // bsdtar (libarchive) распаковывает и 7z. Имена сверены по
    // репозиториям 19.10.2026: Arch — в libarchive (от него зависит pacman),
    // Debian/Ubuntu — libarchive-tools, Fedora и openSUSE — отдельный bsdtar.
    (
        "bsdtar",
        &[
            ("arch", "libarchive"),
            ("fedora", "bsdtar"),
            ("suse", "bsdtar"),
            ("debian", "libarchive-tools"),
        ],
    ),
    // Невидимый X-сервер для снимков виджетов Conky (`conky_preview`) —
    // обязательная зависимость пакета Rexon; на SteamOS — пакет Arch внутри
    // sysext Rexon. Имена — официальные пакеты дистрибутивов.
    (
        "Xvfb",
        &[
            ("arch", "xorg-server-xvfb"),
            ("fedora", "xorg-x11-server-Xvfb"),
            ("suse", "xorg-x11-server-Xvfb"),
            ("debian", "xvfb"),
        ],
    ),
    (
        "7z",
        &[
            ("arch", "7zip"),
            ("fedora", "7zip"),
            ("suse", "7zip"),
            ("debian", "7zip"),
        ],
    ),
    (
        "7zz",
        &[
            ("arch", "7zip"),
            ("fedora", "7zip"),
            ("suse", "7zip"),
            ("debian", "7zip"),
        ],
    ),
    (
        "wrestool",
        &[
            ("arch", "icoutils"),
            ("fedora", "icoutils"),
            ("suse", "icoutils"),
            ("debian", "icoutils"),
        ],
    ),
    (
        "icotool",
        &[
            ("arch", "icoutils"),
            ("fedora", "icoutils"),
            ("suse", "icoutils"),
            ("debian", "icoutils"),
        ],
    ),
    (
        "xcursorgen",
        &[
            ("arch", "xorg-xcursorgen"),
            ("fedora", "xcursorgen"),
            ("suse", "xcursorgen"),
            ("debian", "x11-apps"),
        ],
    ),
    (
        "cabextract",
        &[
            ("arch", "cabextract"),
            ("fedora", "cabextract"),
            ("suse", "cabextract"),
            ("debian", "cabextract"),
        ],
    ),
    // Переключатель стиля Qt для окружений без своего (всё, кроме KDE и
    // LXQt): даёт Qt-программам стиль Kvantum вместе со шрифтами и значками.
    // Имена сверены по repology и репозиториям дистрибутивов; Gentoo хранит
    // имя с категорией, как и рабочие столы в `_DE_DATABASE`.
    (
        "qt6ct",
        &[
            ("arch", "qt6ct"),
            ("fedora", "qt6ct"),
            ("suse", "qt6ct"),
            ("debian", "qt6ct"),
            ("gentoo", "gui-apps/qt6ct"),
            ("alpine", "qt6ct"),
            ("void", "qt6ct"),
            ("solus", "qt6ct"),
            ("alt", "qt6ct"),
            // Бандла с qt6ct в Clear Linux нет, в NixOS пакеты ставятся
            // правкой конфигурации — там работает запасной путь через
            // QT_STYLE_OVERRIDE, ничего доставлять не нужно.
            ("clear", "—"),
            ("nix", "—"),
        ],
    ),
    // Своего пакета нет нигде, кроме AUR: везде остаётся установка из PyPI
    (
        "win2xcur",
        &[
            ("arch", "win2xcur (из AUR)"),
            ("fedora", "win2xcur (pip install win2xcur)"),
            ("suse", "win2xcur (pip install win2xcur)"),
            ("debian", "win2xcur (pip install win2xcur)"),
        ],
    ),
    (
        "ffmpeg",
        &[
            ("arch", "ffmpeg"),
            ("fedora", "ffmpeg-free"),
            ("suse", "ffmpeg-7"),
            ("debian", "ffmpeg"),
        ],
    ),
    (
        "7za",
        &[
            ("arch", "7zip"),
            ("fedora", "7zip"),
            ("suse", "7zip"),
            ("debian", "7zip"),
        ],
    ),
    (
        "notify-send",
        &[
            ("arch", "libnotify"),
            ("fedora", "libnotify"),
            ("suse", "libnotify-tools"),
            ("debian", "libnotify-bin"),
        ],
    ),
    // pw-play/pw-cat: Fedora и openSUSE вынесли их из основного пакета
    (
        "pw-play",
        &[
            ("arch", "pipewire"),
            ("fedora", "pipewire-utils"),
            ("suse", "pipewire-tools"),
            ("debian", "pipewire-bin"),
        ],
    ),
    (
        "paplay",
        &[
            ("arch", "libpulse"),
            ("fedora", "pulseaudio-utils"),
            ("suse", "pulseaudio-utils"),
            ("debian", "pulseaudio-utils"),
        ],
    ),
    (
        "aplay",
        &[
            ("arch", "alsa-utils"),
            ("fedora", "alsa-utils"),
            ("suse", "alsa-utils"),
            ("debian", "alsa-utils"),
        ],
    ),
    (
        "canberra-gtk-play",
        &[
            ("arch", "libcanberra"),
            ("fedora", "libcanberra-gtk3"),
            ("suse", "libcanberra-gtk3"),
            ("debian", "gnome-session-canberra"),
        ],
    ),
    (
        "glib-compile-schemas",
        &[
            ("arch", "glib2"),
            ("fedora", "glib2"),
            ("suse", "glib2-tools"),
            ("debian", "libglib2.0-bin"),
        ],
    ),
    (
        "dbus-send",
        &[
            ("arch", "dbus"),
            ("fedora", "dbus-tools"),
            ("suse", "dbus-1-tools"),
            ("debian", "dbus-bin"),
        ],
    ),
    (
        "pactl",
        &[
            ("arch", "libpulse"),
            ("fedora", "pulseaudio-utils"),
            ("suse", "pulseaudio-utils"),
            ("debian", "pulseaudio-utils"),
        ],
    ),
    // Есть везде, где есть PipeWire: wireplumber — обязательная часть стека.
    (
        "wpctl",
        &[
            ("arch", "wireplumber"),
            ("fedora", "wireplumber"),
            ("suse", "wireplumber"),
            ("debian", "wireplumber"),
        ],
    ),
    // В официальных репозиториях Arch НЕТ (только AUR); в Fedora — с 42;
    // в Debian/Ubuntu/openSUSE есть, но не ставится ни по умолчанию, ни по
    // зависимостям — «из коробки» её нет нигде.
    (
        "xdg-terminal-exec",
        &[
            ("arch", "xdg-terminal-exec (из AUR)"),
            ("fedora", "xdg-terminal-exec (Fedora 42+)"),
            ("suse", "xdg-terminal-exec"),
            ("debian", "xdg-terminal-exec"),
        ],
    ),
    // На openSUSE pkexec — отдельный подпакет, тянется лишь через Recommends
    (
        "pkexec",
        &[
            ("arch", "polkit"),
            ("fedora", "polkit"),
            ("suse", "polkit"),
            ("debian", "pkexec"),
        ],
    ),
    (
        "xprop",
        &[
            ("arch", "xorg-xprop"),
            ("fedora", "xprop"),
            ("suse", "xprop"),
            ("debian", "x11-utils"),
        ],
    ),
    (
        "xrandr",
        &[
            ("arch", "xorg-xrandr"),
            ("fedora", "xrandr"),
            ("suse", "xrandr"),
            ("debian", "x11-xserver-utils"),
        ],
    ),
    (
        "git",
        &[
            ("arch", "git"),
            ("fedora", "git-core"),
            ("suse", "git-core"),
            ("debian", "git"),
        ],
    ),
    (
        "kvantummanager",
        &[
            ("arch", "kvantum"),
            ("fedora", "kvantum"),
            ("suse", "kvantum-manager"),
            ("debian", "qt6-style-kvantum"),
        ],
    ),
    (
        "cvlc",
        &[
            ("arch", "vlc-cli"),
            ("fedora", "vlc-cli"),
            ("suse", "vlc-noX"),
            ("debian", "vlc-bin"),
        ],
    ),
    (
        "qdbus6",
        &[
            ("arch", "qt6-tools"),
            ("fedora", "qt6-qttools"),
            ("suse", "qt6-tools-qdbus"),
            ("debian", "qdbus-qt6"),
        ],
    ),
    (
        "kwriteconfig6",
        &[
            ("arch", "kconfig"),
            ("fedora", "kf6-kconfig"),
            ("suse", "kf6-kconfig"),
            ("debian", "libkf6config-bin"),
        ],
    ),
    (
        "kreadconfig6",
        &[
            ("arch", "kconfig"),
            ("fedora", "kf6-kconfig"),
            ("suse", "kf6-kconfig"),
            ("debian", "libkf6config-bin"),
        ],
    ),
    (
        "swww",
        &[
            ("arch", "awww"),
            ("fedora", "awww"),
            ("suse", "awww"),
            ("debian", "awww"),
        ],
    ),
    (
        "mpvpaper",
        &[
            ("arch", "mpvpaper"),
            ("fedora", "mpvpaper (COPR)"),
            ("suse", "mpvpaper"),
            ("debian", "—"),
        ],
    ),
    (
        "yt-dlp",
        &[
            ("arch", "yt-dlp"),
            ("fedora", "yt-dlp"),
            ("suse", "yt-dlp"),
            ("debian", "yt-dlp"),
        ],
    ),
    (
        "xwinwrap",
        &[
            ("arch", "xwinwrap-git (из AUR)"),
            ("fedora", "—"),
            ("suse", "—"),
            ("debian", "—"),
        ],
    ),
    (
        "mpv",
        &[
            ("arch", "mpv"),
            ("fedora", "mpv"),
            ("suse", "mpv"),
            ("debian", "mpv"),
        ],
    ),
    (
        "hyprpaper",
        &[
            ("arch", "hyprpaper"),
            ("fedora", "hyprpaper"),
            ("suse", "hyprpaper"),
            ("debian", "—"),
        ],
    ),
    (
        "ruffle",
        &[
            ("arch", "ruffle (из AUR)"),
            ("fedora", "—"),
            ("suse", "ruffle"),
            ("debian", "—"),
        ],
    ),
    (
        "gtk-update-icon-cache",
        &[
            ("arch", "gtk-update-icon-cache"),
            ("fedora", "gtk-update-icon-cache"),
            ("suse", "gtk3-tools"),
            ("debian", "libgtk-3-bin"),
        ],
    ),
    (
        "update-desktop-database",
        &[
            ("arch", "desktop-file-utils"),
            ("fedora", "desktop-file-utils"),
            ("suse", "desktop-file-utils"),
            ("debian", "desktop-file-utils"),
        ],
    ),
    // Программы, которые чаще всего зовут виджеты Conky (`${exec sensors}`,
    // погода через wget/curl, плеер) — проверка здоровья виджета
    // (`conky_safety::health`) ставит их пакет перед запуском. Датчики: lm_sensors в
    // Arch и Fedora, lm-sensors в Debian/Ubuntu, sensors в openSUSE. wget в
    // Fedora с 40-го выпуска — wget2, а прежнее имя программы даёт пакет
    // wget2-wget. nvidia-smi сюда намеренно не входит: он приходит с
    // драйвером NVIDIA, и ставить его отдельным пакетом нельзя.
    (
        "sensors",
        &[
            ("arch", "lm_sensors"),
            ("fedora", "lm_sensors"),
            ("suse", "sensors"),
            ("debian", "lm-sensors"),
        ],
    ),
    (
        "wget",
        &[
            ("arch", "wget"),
            ("fedora", "wget2-wget"),
            ("suse", "wget"),
            ("debian", "wget"),
        ],
    ),
    (
        "curl",
        &[
            ("arch", "curl"),
            ("fedora", "curl"),
            ("suse", "curl"),
            ("debian", "curl"),
        ],
    ),
    (
        "jq",
        &[
            ("arch", "jq"),
            ("fedora", "jq"),
            ("suse", "jq"),
            ("debian", "jq"),
        ],
    ),
    // Звуковой визуализатор виджетов-эквалайзеров (`io.popen('cava -p …')`
    // в Lua). Сверено по репозиториям 24.10.2026: cava есть в Arch (extra),
    // Debian/Ubuntu, Fedora и openSUSE.
    (
        "cava",
        &[
            ("arch", "cava"),
            ("fedora", "cava"),
            ("suse", "cava"),
            ("debian", "cava"),
        ],
    ),
    (
        "bc",
        &[
            ("arch", "bc"),
            ("fedora", "bc"),
            ("suse", "bc"),
            ("debian", "bc"),
        ],
    ),
    (
        "playerctl",
        &[
            ("arch", "playerctl"),
            ("fedora", "playerctl"),
            ("suse", "playerctl"),
            ("debian", "playerctl"),
        ],
    ),
];

/// Кандидаты и каталоги вне PATH для инструмента.
///
/// Неизвестное имя — сам себе единственный кандидат (ровно как
/// `_TOOL_SETS.get(tool, ((tool,), ()))` в оригинале).
pub fn tool_candidates(tool: &str) -> (Vec<&str>, Vec<&str>) {
    for (name, names, dirs) in TOOL_SETS {
        if *name == tool {
            return (names.to_vec(), dirs.to_vec());
        }
    }
    (vec![tool], Vec::new())
}

/// Многословный префикс инструмента-подкоманды (`identify` → `magick
/// identify`), если он описан.
pub fn tool_subcommand(tool: &str) -> Option<&'static [&'static str]> {
    TOOL_SUBCOMMANDS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, prefix)| *prefix)
}

/// Известны ли для имени варианты подмены — то, что проверяет `resolved_argv`
/// перед тем, как вообще что-то искать.
pub fn tool_has_variants(tool: &str) -> bool {
    TOOL_SETS.iter().any(|(name, _, _)| *name == tool) || tool_subcommand(tool).is_some()
}

/// Имя пакета с этим инструментом для указанного семейства, либо `""`.
///
/// Единственный источник имён пакетов в UI и в журнале (правило #13:
/// покупателю нельзя советовать `pacman -S` на Fedora). Семейство приходит
/// параметром: его определяет `system::DistroDetector`, а не эта таблица.
pub fn tool_package_name(binary: &str, family: &str) -> String {
    let Some((_, table)) = TOOL_PACKAGES.iter().find(|(name, _)| *name == binary) else {
        return String::new();
    };
    let lookup = |key: &str| {
        table
            .iter()
            .find(|(f, _)| *f == key)
            .map(|(_, pkg)| *pkg)
            .filter(|pkg| !pkg.is_empty())
    };
    // Семейство не в таблице — назовём пакет так, как он зовётся в
    // большинстве дистрибутивов, но без команды установки.
    let pkg = lookup(family)
        .or_else(|| lookup("debian"))
        .or_else(|| table.first().map(|(_, pkg)| *pkg))
        .unwrap_or("");
    // «—» в таблице значит «пакета нет вовсе» — советовать нечего.
    if pkg.is_empty() || pkg == "—" {
        String::new()
    } else if let Some(name) = pkg.strip_suffix(AUR_ONLY_MARK) {
        // Пометка таблицы — признак, подпись — на языке интерфейса. Скобка
        // остаётся в начале пометки: по ней `dep_package_name` отрезает имя.
        // TRANSLATORS: %1 is a package that exists only in the AUR (Arch User Repository).
        crate::i18n::trf("%1 (from the AUR)", &[&name])
    } else {
        pkg.to_string()
    }
}

/// Пометка «пакет только в AUR» в таблице [`TOOL_PACKAGES`] — признак, а не
/// подпись: показывает её [`tool_package_name`] на языке интерфейса.
pub(crate) const AUR_ONLY_MARK: &str = " (из AUR)";

/// Все пакеты таблицы [`TOOL_PACKAGES`] для семейства — и репозиторные, и
/// помеченные «только AUR» (без пометки). Белый список установки помощника
/// пакетов (`system_plan::pkg_helper_install_allowlist`): программы виджетов
/// Conky и распаковщик 7z ставятся только отсюда.
pub fn tool_packages_for(family: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (_, table) in TOOL_PACKAGES {
        for (f, pkg) in table.iter() {
            if *f != family {
                continue;
            }
            let pkg = pkg.strip_suffix(AUR_ONLY_MARK).unwrap_or(pkg);
            if !pkg.is_empty() && pkg != "—" && !pkg.contains('(') && !out.contains(&pkg) {
                out.push(pkg);
            }
        }
    }
    out
}

/// «Установите пакет X» для указанного семейства, либо `""`.
pub fn tool_package_hint(binary: &str, family: &str) -> String {
    let pkg = tool_package_name(binary, family);
    if pkg.is_empty() {
        String::new()
    } else {
        // TRANSLATORS: %1 is a package name; the sentence tells the user what to install.
        crate::i18n::trf("Install the “%1” package", &[&pkg])
    }
}

// ----------------------------------------------------------------------------
// Проигрывание звука: ОДИН выбор проигрывателя на весь проект
// ----------------------------------------------------------------------------
//
// ⚠️ Почему это здесь, а не по месту. Хвост «paplay → ffplay → aplay →
// canberra-gtk-play» подводил сразу дважды: `paplay` лежит в
// `pulseaudio-utils`, которых на чистом PipeWire (Ubuntu 24.04, Kubuntu
// 24.04, Debian 13) нет, а `canberra-gtk-play` на Debian/Ubuntu лежит не в
// `libcanberra-gtk*-module`, как обычно пишут в зависимостях, а в
// `gnome-session-canberra`, который тоже не ставится. Плюс `aplay` (его
// советовали «надёжным хвостом») понимает ТОЛЬКО WAV, а звуковые темы
// freedesktop состоят из `.oga` — то есть «хвост» молча ничего не играл.
// Отсюда таблица: у каждого проигрывателя перечислено, ЧТО он умеет.

/// Проигрыватели звуковых файлов: (бинарь, аргументы до пути, расширения).
///
/// Порядок — от «есть везде, где есть звук» к узким. Пустой список
/// расширений значит «умеет всё».
pub const SOUND_PLAYERS: &[(&str, &[&str], &[&str])] = &[
    // pipewire: есть всюду, где PipeWire (то есть на всех целевых)
    ("pw-play", &[], &["oga", "ogg", "opus", "wav", "flac"]),
    // pulseaudio-utils: там, где стек ещё pulse
    ("paplay", &[], &["oga", "ogg", "opus", "wav", "flac"]),
    // libcanberra: играет файл ключом -f (не путать с событием -i)
    ("canberra-gtk-play", &["-f"], &["oga", "ogg", "wav"]),
    // ffmpeg: понимает всё, но ставится не везде
    ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"], &[]),
    // alsa-utils: последний рубеж, но ТОЛЬКО WAV
    ("aplay", &["-q"], &["wav"]),
];

/// Расширение файла в нижнем регистре, без точки.
fn file_ext(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// argv проигрывания ФАЙЛА первым подходящим проигрывателем.
///
/// `available` — бинари, найденные исполнителем в PATH. `None` — играть
/// нечем: ни один установленный проигрыватель не понимает этот формат.
pub fn sound_play_argv(available: &[String], file_path: &str) -> Option<Vec<String>> {
    let ext = file_ext(file_path);
    for (binary, args, exts) in SOUND_PLAYERS {
        if !available.iter().any(|b| b == binary) {
            continue;
        }
        if !exts.is_empty() && !exts.contains(&ext.as_str()) {
            continue;
        }
        let mut argv = vec![(*binary).to_string()];
        argv.extend(args.iter().map(|a| (*a).to_string()));
        argv.push(file_path.to_string());
        return Some(argv);
    }
    None
}

/// argv проигрывания СОБЫТИЯ темы (`canberra-gtk-play -i`) — путь на случай,
/// когда файла события в теме не нашлось.
pub fn sound_play_event_argv(available: &[String], event_id: &str) -> Option<Vec<String>> {
    if available.iter().any(|b| b == "canberra-gtk-play") {
        return Some(vec![
            "canberra-gtk-play".to_string(),
            "-i".to_string(),
            event_id.to_string(),
        ]);
    }
    None
}

/// Что советовать поставить, если играть нечем: для WAV достаточно
/// `alsa-utils`, для тем freedesktop (`.oga`) нужен настоящий декодер.
pub fn sound_player_suggestion(file_path: &str) -> &'static str {
    if file_ext(file_path) == "wav" {
        "aplay"
    } else {
        "pw-play"
    }
}

/// Сообщение UI, когда проиграть нечем (правило #13 — без сырых имён
/// бинарей в начале фразы). `hint` — «Установите пакет …» либо пусто.
pub fn msg_no_sound_player(hint: &str) -> String {
    if hint.is_empty() {
        crate::i18n::tr("Can't play the sound: no audio player is installed")
    } else {
        // TRANSLATORS: %1 is a hint like “Install the “pipewire” package”.
        crate::i18n::trf("Can't play the sound: no audio player is installed. %1", &[&hint])
    }
}

/// ОДНА понятная запись в журнал вместо «команда не найдена».
///
/// Собирается здесь, чтобы текст был один и тот же и в журнале, и в UI:
/// имя, все опробованные синонимы и пакет для текущего дистрибутива.
pub fn msg_missing_tool(names: &[&str], hint: &str) -> String {
    // Целыми фразами, а не склейкой кусков: переводчику нужен весь порядок
    // слов, а «(пробовали: …)» и подсказка в другом языке встанут иначе.
    let tool = names.first().copied().unwrap_or("");
    let tried = names.join(", ");
    use crate::i18n::trf;
    match (names.len() > 1, hint.is_empty()) {
        // TRANSLATORS: %1 is a program name.
        (false, true) => trf("Tool %1 not found", &[&tool]),
        // TRANSLATORS: %1 is a program name, %2 the other names that were tried.
        (true, true) => trf("Tool %1 not found (tried: %2)", &[&tool, &tried]),
        // TRANSLATORS: %1 is a program name, %2 a hint like “Install the “x” package”.
        (false, false) => trf("Tool %1 not found. %2", &[&tool, &hint]),
        // TRANSLATORS: %1 is a program name, %2 the other names that were tried, %3 a hint like “Install the “x” package”.
        (true, false) => trf("Tool %1 not found (tried: %2). %3", &[&tool, &tried, &hint]),
    }
}

// ----------------------------------------------------------------------------
// Поиск бинаря
// ----------------------------------------------------------------------------

/// PATH, когда переменной нет вовсе: то же, что отдаёт `confstr("CS_PATH")`
/// на glibc и что берёт `shutil.which`.
pub const CS_PATH_FALLBACK: &str = "/bin:/usr/bin";

/// Разбор пути как `os.path.split`: (каталог, имя).
fn path_split(p: &str) -> (&str, &str) {
    match p.rfind('/') {
        None => ("", p),
        Some(i) => {
            let (head, tail) = p.split_at(i + 1);
            // Каталог из одних слешей остаётся как есть (`/ls` → `/`).
            let head = if head.chars().all(|c| c == '/') {
                head
            } else {
                head.trim_end_matches('/')
            };
            (head, tail)
        }
    }
}

/// Годен ли файл на запуск: существует, не каталог, есть бит `x`.
///
/// ⚠️ Про `access(2)` — см. предупреждение в начале модуля.
fn is_executable_file(path: &str) -> bool {
    match fs::metadata(path) {
        Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Путь к команде по PATH — порт `shutil.which(cmd)`.
///
/// `path_env` — значение `$PATH` (`None` — переменной нет; пустая строка,
/// как и в Python, значит «не искать нигде»). Имя с косой чертой ищется
/// только в своём каталоге, PATH при этом не смотрится.
pub fn which(cmd: &str, path_env: Option<&str>) -> Option<String> {
    let (dirname, base) = path_split(cmd);
    let dirs: Vec<String> = if !dirname.is_empty() {
        vec![dirname.to_string()]
    } else {
        let path = path_env.unwrap_or(CS_PATH_FALLBACK);
        // PATH='' не совпадает ни с чем, а PATH=':' смотрит в текущий
        // каталог — тот же квирк, что у shutil.which.
        if path.is_empty() {
            return None;
        }
        path.split(':').map(str::to_string).collect()
    };
    let mut seen: HashSet<&str> = HashSet::new();
    for dir in &dirs {
        if !seen.insert(dir.as_str()) {
            continue;
        }
        let cand = join_path(dir, base);
        if is_executable_file(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Есть ли в шаблоне то, что glob раскрывает.
///
/// Класс символов `[...]` ядро не поддерживает (в наборах синонимов таких
/// шаблонов нет и не планируется) — шаблон с ним считается буквальным.
fn has_glob_magic(part: &str) -> bool {
    part.contains('*') || part.contains('?')
}

/// Совпадение имени с шаблоном из `*` и `?`.
fn glob_match(name: &str, pattern: &str) -> bool {
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    // Классический двумерный проход: m[i][j] — «первые i символов имени
    // покрыты первыми j символами шаблона».
    let mut prev = vec![false; p.len() + 1];
    prev[0] = true;
    for j in 1..=p.len() {
        prev[j] = prev[j - 1] && p[j - 1] == '*';
    }
    for i in 1..=n.len() {
        let mut cur = vec![false; p.len() + 1];
        for j in 1..=p.len() {
            cur[j] = match p[j - 1] {
                '*' => cur[j - 1] || prev[j],
                '?' => prev[j - 1],
                c => prev[j - 1] && n[i - 1] == c,
            };
        }
        prev = cur;
    }
    prev[p.len()]
}

/// Раскрытие шаблона каталога вместе с именем файла — порт
/// `glob.glob(os.path.join(pattern, name))`.
///
/// Результат НЕ сортируется (как и в Python); сортирует вызывающий.
fn glob_paths(pattern: &str) -> Vec<String> {
    if !pattern.split('/').any(has_glob_magic) {
        return if Path::new(pattern).exists() {
            vec![pattern.to_string()]
        } else {
            Vec::new()
        };
    }
    let absolute = pattern.starts_with('/');
    let parts: Vec<&str> = pattern.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur: Vec<String> = vec![if absolute { "/".into() } else { String::new() }];
    for (idx, part) in parts.iter().enumerate() {
        let last = idx + 1 == parts.len();
        let mut next = Vec::new();
        for dir in &cur {
            if has_glob_magic(part) {
                let read_root = if dir.is_empty() { "." } else { dir.as_str() };
                let Ok(entries) = fs::read_dir(read_root) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    // Скрытые файлы `*` не ловит — как и glob.
                    if name.starts_with('.') && !part.starts_with('.') {
                        continue;
                    }
                    if !glob_match(&name, part) {
                        continue;
                    }
                    let full = join_path(dir, &name);
                    if last || Path::new(&full).is_dir() {
                        next.push(full);
                    }
                }
            } else {
                let full = join_path(dir, part);
                if if last {
                    Path::new(&full).exists()
                } else {
                    Path::new(&full).is_dir()
                } {
                    next.push(full);
                }
            }
        }
        cur = next;
    }
    cur
}

/// Поиск бинаря по всем синонимам: PATH, затем каталоги вне PATH.
///
/// Ровно порядок оригинала: сперва все имена по PATH, и только если ни одно
/// не нашлось — перебор каталогов (в них шаблон `*` раскрывается, кандидаты
/// сортируются, берётся первый исполняемый).
pub fn lookup_tool(names: &[&str], dirs: &[&str], path_env: Option<&str>) -> Option<String> {
    for name in names {
        if let Some(exe) = which(name, path_env) {
            return Some(exe);
        }
    }
    // PATH не помог — смотрим в каталоги вне PATH напрямую.
    for pattern in dirs {
        for name in names {
            let mut cands = glob_paths(&join_path(pattern, name));
            cands.sort();
            if let Some(exe) = cands.into_iter().find(|c| is_executable_file(c)) {
                return Some(exe);
            }
        }
    }
    None
}

/// Путь к внешнему инструменту по каноническому имени, либо `None`.
///
/// Без кеша: кеш живёт в вызывающем (в живом Python — `utils._tool_cache`,
/// с ключом по PATH), чтобы не было двух источников правды.
pub fn resolve_tool(tool: &str, extra_dirs: &[&str], path_env: Option<&str>) -> Option<String> {
    let (names, mut dirs) = tool_candidates(tool);
    dirs.extend_from_slice(extra_dirs);
    lookup_tool(&names, &dirs, path_env)
}

/// Готовый argv внешнего инструмента либо `None`, если его нет вовсе.
///
/// Знает и синонимы (`convert` → `magick`), и многословные варианты
/// (`identify` → `magick identify`). Отсюда же берётся ответ на вопрос «а
/// есть ли инструмент» — чтобы проверка и запуск не могли разойтись.
pub fn tool_argv(tool: &str, args: &[String], path_env: Option<&str>) -> Option<Vec<String>> {
    if let Some(exe) = resolve_tool(tool, &[], path_env) {
        let mut argv = vec![exe];
        argv.extend(args.iter().cloned());
        return Some(argv);
    }
    let prefix = tool_subcommand(tool)?;
    let exe = resolve_tool(prefix[0], &[], path_env)?;
    let mut argv = vec![exe];
    argv.extend(prefix[1..].iter().map(|s| (*s).to_string()));
    argv.extend(args.iter().cloned());
    Some(argv)
}

/// argv с уже разрешённым бинарём.
///
/// Если инструмента нет вовсе, argv[0] остаётся каноническим именем — вызов
/// честно провалится, но причина уже понятна из журнала. Так вызывающий код
/// не обязан менять форму.
pub fn tool_cmd(tool: &str, args: &[String], path_env: Option<&str>) -> Vec<String> {
    if let Some(argv) = tool_argv(tool, args, path_env) {
        return argv;
    }
    let mut argv = vec![tool.to_string()];
    argv.extend(args.iter().cloned());
    argv
}

/// argv с подменённым инструментом, если для него известны варианты.
///
/// Всё, чего нет в наборах синонимов, проходит насквозь — накладных расходов
/// ноль. Подмена достаётся и argv-планам, собранным ядром: процесс всё равно
/// запускает исполнитель, а он зовёт этот же резолвер.
pub fn resolved_argv(cmd: &[String], path_env: Option<&str>) -> Vec<String> {
    let Some(head) = cmd.first() else {
        return cmd.to_vec();
    };
    if !tool_has_variants(head) {
        return cmd.to_vec();
    }
    match tool_argv(head, &[], path_env) {
        // Ничего не изменилось — отдаём исходное, чтобы не плодить копии.
        Some(argv) if argv.len() == 1 && argv[0] == *head => cmd.to_vec(),
        Some(argv) => {
            let mut out = argv;
            out.extend(cmd[1..].iter().cloned());
            out
        }
        None => cmd.to_vec(),
    }
}

// ============================================================================
// Сессионная шина D-Bus — ЕДИНАЯ точка вызова на весь проект
// ============================================================================
//
// Почему это здесь, а не по месту вызова (правило #12): половина модулей
// раньше звала бинарь `qdbus` напрямую, а такого бинаря на большинстве
// целевых систем нет вовсе:
//   • Arch / CachyOS / SteamOS ....... qdbus6      (пакет qt6-tools)
//   • Debian / Ubuntu / Kubuntu ...... qdbus-qt6   (пакет qdbus-qt6)
//   • Fedora / openSUSE .............. qdbus-qt6   (пакет qt6-qttools)
//   • эпоха Qt5 (и NixOS) ............ qdbus / qdbus-qt5
// и вдобавок на Kubuntu бинарь кладут в /usr/lib/qt6/bin/, куда PATH не
// смотрит вовсе — перебором имён это не лечится.
//
// Поэтому ОСНОВНОЙ путь — `dbus-send`: он из пакета `dbus`, то есть лежит на
// любой машине, где сессионная шина вообще есть, и называется везде
// одинаково. `qdbus` остаётся подстраховкой.
//
// Форматы вывода у них РАЗНЫЕ, поэтому ответ приводится к одному виду («как
// печатает qdbus»): голое значение без типа и кавычек.

/// Порядок предпочтения: (канон. имя инструмента, вид для вызывающего).
pub const DBUS_TOOL_PREFERENCE: &[(&str, &str)] = &[("dbus-send", "dbus-send"), ("qdbus6", "qdbus")];

/// Сообщение, когда на шину сходить нечем.
pub fn msg_no_dbus_tool(qdbus_names: &[&str]) -> String {
    crate::i18n::trf(
        // TRANSLATORS: %1 lists the qdbus program names that were looked for.
        "Neither dbus-send nor qdbus (%1) is installed, so Rexon can't talk to the \
         D-Bus session bus. Install the dbus package (dbus-bin on Debian/Ubuntu, \
         dbus-tools on Fedora).",
        &[&qdbus_names.join(", ")],
    )
}

/// Аргумент метода D-Bus в том виде, в каком его принимает `dbus-send`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbusArg {
    /// Логическое значение (`boolean:true`).
    Bool(bool),
    /// Целое (`int32:…`).
    Int(i64),
    /// Всё остальное — строкой, как его печатает Python.
    Str(String),
}

impl DbusArg {
    /// Запись аргумента для `dbus-send` (`string:…`).
    pub fn to_dbus_send(&self) -> String {
        match self {
            // Порядок важен: в Python bool — подкласс int, и проверяется он
            // первым, иначе True уехало бы в int32:1.
            DbusArg::Bool(v) => format!("boolean:{}", if *v { "true" } else { "false" }),
            DbusArg::Int(v) => format!("int32:{v}"),
            DbusArg::Str(v) => format!("string:{v}"),
        }
    }

    /// Запись аргумента для `qdbus` — просто значение строкой.
    pub fn to_qdbus(&self) -> String {
        match self {
            DbusArg::Bool(v) => (if *v { "True" } else { "False" }).to_string(),
            DbusArg::Int(v) => v.to_string(),
            DbusArg::Str(v) => v.clone(),
        }
    }
}

/// Ответ `dbus-send` → голое значение, как его напечатал бы qdbus.
///
/// Первая строка служебная («method return time=…»), дальше идут значения с
/// указанием типа; составные (`array [`, `dict entry(`) пропускаем — нужны
/// скаляры. Кавычки вокруг строкового значения снимаются.
pub fn parse_dbus_send_reply(out: &str) -> String {
    let mut values: Vec<&str> = Vec::new();
    for line in out.lines().skip(1) {
        let Some(val) = dbus_scalar_value(line) else {
            continue;
        };
        let val = if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
            &val[1..val.len() - 1]
        } else {
            val
        };
        values.push(val);
    }
    values.join("\n")
}

/// Типы скаляров в выводе `dbus-send`. `object path` — из двух слов.
const DBUS_SCALAR_TYPES: &[&str] = &[
    "string",
    "object path",
    "signature",
    "boolean",
    "byte",
    "double",
    "int16",
    "int32",
    "int64",
    "uint16",
    "uint32",
    "uint64",
];

/// Значение из строки вида «   variant   string "x"  », если она скалярная.
///
/// Порт регулярного выражения оригинала; отдельной функцией — чтобы у ядра
/// не появилась зависимость на движок регулярок ради одного шаблона.
fn dbus_scalar_value(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    // `variant` — необязательная обёртка, после неё обязателен пробел.
    let rest = match rest.strip_prefix("variant") {
        Some(r) if starts_with_space(r) => r.trim_start(),
        _ => rest,
    };
    for ty in DBUS_SCALAR_TYPES {
        // `object path` в выводе разделён произвольным числом пробелов.
        let after = if let Some((first, second)) = ty.split_once(' ') {
            let Some(r) = rest.strip_prefix(first) else {
                continue;
            };
            if !starts_with_space(r) {
                continue;
            }
            match r.trim_start().strip_prefix(second) {
                Some(r) => r,
                None => continue,
            }
        } else {
            match rest.strip_prefix(*ty) {
                Some(r) => r,
                None => continue,
            }
        };
        // За типом обязан идти пробел, иначе это другое слово («stringy»).
        if !starts_with_space(after) {
            continue;
        }
        return Some(after.trim());
    }
    None
}

/// Начинается ли строка с пробельного символа.
fn starts_with_space(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_whitespace)
}

/// argv вызова метода на сессионной шине.
///
/// `kind` — что за бинарь (`dbus-send` либо `qdbus`, см.
/// [`DBUS_TOOL_PREFERENCE`]): у них разный порядок и разная запись
/// аргументов, и именно поэтому сборка argv живёт в одном месте.
pub fn dbus_call_argv(
    kind: &str,
    exe: &str,
    dest: &str,
    path: &str,
    method: &str,
    args: &[DbusArg],
) -> Vec<String> {
    let mut cmd: Vec<String> = if kind == "dbus-send" {
        vec![
            exe.to_string(),
            "--session".into(),
            "--print-reply".into(),
            format!("--dest={dest}"),
            "--type=method_call".into(),
            path.to_string(),
            method.to_string(),
        ]
    } else {
        vec![
            exe.to_string(),
            dest.to_string(),
            path.to_string(),
            method.to_string(),
        ]
    };
    if kind == "dbus-send" {
        cmd.extend(args.iter().map(DbusArg::to_dbus_send));
    } else {
        cmd.extend(args.iter().map(DbusArg::to_qdbus));
    }
    cmd
}

/// Служба, объект и метод перечитывания конфига живым KWin.
pub const KWIN_RECONFIGURE: (&str, &str, &str) = ("org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure");

/// Служба, объект и метод выполнения JS в plasmashell.
pub const PLASMA_EVALUATE_SCRIPT: (&str, &str, &str) = (
    "org.kde.plasmashell",
    "/PlasmaShell",
    "org.kde.PlasmaShell.evaluateScript",
);

/// Имя приложения в уведомлениях.
pub const NOTIFY_APP_NAME: &str = "Rexon";

/// argv показа уведомления прямо через шину.
///
/// Идём на `org.freedesktop.Notifications`, а не через бинарь
/// `notify-send`: его пакет (libnotify / libnotify-bin) не ставится по
/// умолчанию ни на Debian 13, ни на openSUSE, ни на голом Arch — уведомления
/// там просто не появлялись, причём молча. Сама служба уведомлений есть в
/// любой графической среде.
///
/// Сигнатура `Notify` — `susssasa{sv}i`: пустые массив действий и словарь
/// подсказок записываются как `array:string:` и `dict:string:string:`.
pub fn notify_dbus_argv(
    exe: &str,
    summary: &str,
    body: &str,
    icon: &str,
    timeout_ms: i64,
) -> Vec<String> {
    vec![
        exe.to_string(),
        "--session".into(),
        "--print-reply".into(),
        "--dest=org.freedesktop.Notifications".into(),
        "--type=method_call".into(),
        "/org/freedesktop/Notifications".into(),
        "org.freedesktop.Notifications.Notify".into(),
        format!("string:{NOTIFY_APP_NAME}"),
        "uint32:0".into(),
        format!("string:{icon}"),
        format!("string:{summary}"),
        format!("string:{body}"),
        "array:string:".into(),
        "dict:string:string:".into(),
        format!("int32:{timeout_ms}"),
    ]
}

/// argv запасного пути через бинарь `notify-send`.
pub fn notify_send_argv(exe: &str, summary: &str, body: &str, icon: &str) -> Vec<String> {
    vec![
        exe.to_string(),
        format!("--app-name={NOTIFY_APP_NAME}"),
        "--icon".into(),
        icon.to_string(),
        summary.to_string(),
        body.to_string(),
    ]
}

/// Показать уведомление рабочего стола (служба уведомлений по шине, иначе
/// `notify-send`). `false` — показать нечем. Для сообщений без окна:
/// например, «Rexon не перезапустился», когда своего окна уже нет.
pub fn notify_live(summary: &str, body: &str) -> bool {
    let path_env = std::env::var("PATH").ok();
    let argv = if let Some(exe) = which("dbus-send", path_env.as_deref()) {
        notify_dbus_argv(&exe, summary, body, "rexon", 0)
    } else if let Some(exe) = which("notify-send", path_env.as_deref()) {
        notify_send_argv(&exe, summary, body, "rexon")
    } else {
        return false;
    };
    let spec = RunSpec { argv, output: RunOutput::Capture, timeout_secs: Some(10), ..RunSpec::default() };
    run(&spec, path_env.as_deref()).outcome == RunOutcome::Ok
}

/// argv `notify-send`, который ждёт ответа: кнопки `(ключ, подпись)`
/// (`--action`), нажатая придёт в stdout своим ключом
/// ([`notify_answer`]). Важное — не гаснет само (`--urgency=critical`).
pub fn notify_ask_argv(exe: &str, summary: &str, body: &str, actions: &[(String, String)]) -> Vec<String> {
    let mut argv = vec![
        exe.to_string(),
        format!("--app-name={NOTIFY_APP_NAME}"),
        "--icon=rexon".to_string(),
        "--urgency=critical".to_string(),
        "--wait".to_string(),
    ];
    for (key, label) in actions {
        argv.push(format!("--action={key}={label}"));
    }
    argv.push(summary.to_string());
    argv.push(body.to_string());
    argv
}

/// Нажатая кнопка из вывода `notify-send --wait`: последняя непустая
/// строка, если это ключ одной из `actions`. `None` — закрыли без ответа.
pub fn notify_answer(stdout: &str, actions: &[(String, String)]) -> Option<String> {
    let last = stdout.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    actions.iter().find(|(k, _)| k == last).map(|(k, _)| k.clone())
}

/// Не знает ли `notify-send` кнопок: libnotify до 0.7.9 (Ubuntu 20.04 и
/// старше) отвечает на `--action`/`--wait` «Unknown option».
pub fn notify_send_lacks_actions(stderr: &str) -> bool {
    let e = stderr.to_lowercase();
    e.contains("unknown option") && (e.contains("action") || e.contains("wait"))
}

/// Уведомление, которое ждёт действия человека: с кнопками `actions`, если
/// `notify-send` их умеет, — и ответ (ключ нажатой) после её нажатия.
/// Кнопок нет (нет `notify-send` либо он старый) — то же уведомление
/// обычным путём ([`notify_live`]) с запасным текстом `no_buttons`: что
/// сделать самому. `wait_secs` — сколько ждать ответа. Звать не из потока
/// интерфейса: ждёт.
pub fn notify_ask_live(
    summary: &str,
    body: &str,
    actions: &[(String, String)],
    no_buttons: &str,
    wait_secs: u64,
) -> Option<String> {
    let path_env = std::env::var("PATH").ok();
    if let Some(exe) = which("notify-send", path_env.as_deref()) {
        let spec = RunSpec {
            argv: notify_ask_argv(&exe, summary, body, actions),
            output: RunOutput::Capture,
            timeout_secs: Some(wait_secs),
            ..RunSpec::default()
        };
        let report = run(&spec, path_env.as_deref());
        if !notify_send_lacks_actions(&String::from_utf8_lossy(&report.stderr)) {
            return notify_answer(&String::from_utf8_lossy(&report.stdout), actions);
        }
    }
    let text = if no_buttons.is_empty() { body.to_string() } else { format!("{body}\n\n{no_buttons}") };
    notify_live(summary, &text);
    None
}

// ============================================================================
// Исполнитель внешних команд — ЕДИНСТВЕННОЕ место, где Rexon поднимает процесс
// ============================================================================
//
// ЗАЧЕМ ЭТО ЗДЕСЬ. Решения о запуске уже жили в ядре: какой команде какой
// таймаут (`run_duration`), под каким именем на этой машине лежит бинарь
// (`resolved_argv`), какую команду вообще собрать (планы `system_plan`).
// А сам запуск оставался на Python — `subprocess.run` в `rexon/utils.py`. Из-за
// этого шва Rust-часть продукта не могла довести до конца ни одного действия:
// план собирался в ядре и упирался в границу, за которой нужен живой Python.
//
// Теперь запуск — здесь, и он один на весь продукт. Python не держит своей
// копии: `rexon/utils.py::safe_run` зовёт эту функцию через мост и только
// приводит ответ к форме `subprocess.CompletedProcess`.
//
// ⚠️ ЧТО ИМЕННО ПЕРЕЕХАЛО, кроме самого `spawn`. Три вещи, каждая — цена
// прошлого дефекта:
//   • таймаут по КЛАССУ команды, а не один на всё (`pacman -Q` и сборка
//     девяти пакетов из AUR мерились одними тридцатью секундами);
//   • `LANG=C`/`LC_ALL=C` для команд, чей вывод РАЗБИРАЕТСЯ: на русской
//     локали пакетные менеджеры печатают переведённые поля, и регулярные
//     выражения ядра не находят ничего. Прежде эту пару подставлял по месту
//     сам Python, глядя на флаг `lang_c` в плане, — то есть решение ядра
//     исполняли две разные строки в двух файлах;
//   • `stdin` в /dev/null. Ни одна наша команда не читает клавиатуру, но
//     stdin ребёнку доставался по наследству, и вопрос вроде «Enter a
//     selection (default=all):» висел бы на живом, но молчащем вводе до
//     конца таймаута — а таймаут сборки четыре часа.

/// Исход запуска — чтобы «не смогли» и «не дождались» нельзя было спутать.
///
/// Прежний `safe_run` на оба случая отдавал `None`, и вызывающий честно
/// писал человеку «не установилось» там, где просто кончилось время.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// Команда отработала, код возврата 0.
    Ok,
    /// Команда отработала и вернула ненулевой код.
    Failed,
    /// Не уложилась в таймаут — это НЕ то же, что «не смогла».
    Timeout,
    /// Не запустилась вовсе: нет бинаря, нет прав, не открылся файл вывода.
    Error,
}

impl RunOutcome {
    /// Короткое имя исхода — для журнала и для моста в Python.
    pub fn as_str(self) -> &'static str {
        match self {
            RunOutcome::Ok => "ok",
            RunOutcome::Failed => "failed",
            RunOutcome::Timeout => "timeout",
            RunOutcome::Error => "error",
        }
    }
}

/// Куда девать вывод команды.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RunOutput {
    /// Наследовать вывод родителя (`subprocess.run` без `capture_output`).
    #[default]
    Inherit,
    /// Забрать в память ТЕКСТОМ: utf-8 с заменой негодных байт и переводом
    /// строк по-универсальному — ровно то, что делает `text=True`.
    Capture,
    /// Забрать в память БАЙТАМИ: так работает `run_tool` (конвертеры отдают
    /// не только текст), и перевод строк там был бы порчей данных.
    CaptureBytes,
    /// Писать в файл. Так идёт долгая установка: вывод уходит в файл, а
    /// живой ход дела читают из него отдельно, не мешая процессу.
    File(String),
}

/// Язык покупателя строкой, как его задаёт окружение: `LANGUAGE`, `LC_ALL`,
/// `LC_MESSAGES`, `LANG` — в порядке, в котором их читает сама система
/// локалей. `LANGUAGE` — список через двоеточие, берётся первый.
///
/// `C`/`POSIX` — это «локали нет», и подставлять туда `ru` «потому что
/// продукт русский» нельзя: на английской машине вылез бы русский текст.
/// Поэтому пусто отвечает `en`.
///
/// Живёт здесь, а не у вызывающих: тот же цикл нужен и переводу подписей
/// плагинов в Qt-слое, и человеческим именам тем в сканере установленного
/// (правило #12 — один движок, не копия на каждое место).
pub fn env_locale() -> String {
    for name in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            let first = value.split(':').next().unwrap_or("").trim();
            if !first.is_empty() && first != "C" && first != "POSIX" {
                return first.to_string();
            }
        }
    }
    String::from("en")
}

/// Переменные, которыми глушится локаль, когда вывод команды РАЗБИРАЕТСЯ.
///
/// Обе, а не одна: `LC_ALL` перебивает `LC_MESSAGES` пользователя, `LANG`
/// закрывает случай, когда `LC_ALL` в системе не установлена вовсе.
pub const PARSED_OUTPUT_ENV: &[(&str, &str)] = &[("LANG", "C"), ("LC_ALL", "C")];

/// Переменные, которые СНИМАЮТСЯ, когда вывод команды разбирается.
///
/// `LANGUAGE` окно Rexon ставит само — язык интерфейса Rexon
/// (`cli::qt_ui_language_env`), и её наследует каждый дочерний процесс.
/// GNU gettext при `LC_ALL=C` её не читает, но Qt и KDE берут из неё язык
/// интерфейса мимо `LC_ALL`, — разбираемому выводу она не нужна вовсе.
pub const PARSED_OUTPUT_ENV_REMOVE: &[&str] = &["LANGUAGE"];

/// Окружение для команды, чей вывод РАЗБИРАЕТСЯ кодом: C-локаль
/// ([`PARSED_OUTPUT_ENV`]) и без языка интерфейса
/// ([`PARSED_OUTPUT_ENV_REMOVE`]).
///
/// Одна точка на весь продукт (правило #12): её зовёт [`run`] при
/// `lang_c`, и её же — места, что строят `Command` сами. ⛔ Не для команд,
/// которые ЗАПУСКАЮТ программу для человека (оболочку стола, проигрыватель,
/// окно настроек): та унаследовала бы C-локаль и заговорила по-английски.
/// И не для команд, чей вывод только ПОКАЗЫВАЕТСЯ, — он остаётся на языке
/// человека.
pub fn parsed_output_locale(cmd: &mut std::process::Command) -> &mut std::process::Command {
    for name in PARSED_OUTPUT_ENV_REMOVE {
        cmd.env_remove(name);
    }
    for (k, v) in PARSED_OUTPUT_ENV {
        cmd.env(k, v);
    }
    cmd
}

/// Заказ на запуск внешней команды.
#[derive(Debug, Clone, Default)]
pub struct RunSpec {
    /// Команда как есть, до подмены синонимов бинаря.
    pub argv: Vec<String>,
    /// Куда девать вывод.
    pub output: RunOutput,
    /// Таймаут, секунды. `None` — взять по классу команды.
    pub timeout_secs: Option<u64>,
    /// Вывод команды будет разбираться → глушим локаль (`PARSED_OUTPUT_ENV`).
    pub lang_c: bool,
    /// ДОБАВКИ к окружению (не замена ему): пары «имя, значение».
    pub env: Vec<(String, String)>,
    /// stderr в тот же поток, что stdout (`stderr=subprocess.STDOUT`).
    pub stderr_to_stdout: bool,
}

/// Что именно будет запущено — БЕЗ запуска.
///
/// Отдельно от [`run`] потому, что половину наших команд нельзя выполнить в
/// проверке: за `pkexec` стоит установка стола и запрос пароля. Мерило
/// такого кода — текстовый паритет собираемой команды, и вот он.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// argv после подмены синонимов бинаря (`kwriteconfig6` → `kwriteconfig5`).
    pub argv: Vec<String>,
    /// Итоговый таймаут, секунды.
    pub timeout_secs: u64,
    /// Класс времени работы, по которому взят таймаут.
    pub duration: RunDuration,
    /// Добавки к окружению в порядке применения: сначала глушение локали,
    /// потом то, что просил вызывающий.
    pub env: Vec<(String, String)>,
    /// Текстовый режим с заменой негодных байт.
    pub text_mode: bool,
    /// stdin закрыт в /dev/null (всегда: клавиатуры у наших команд нет).
    pub stdin_null: bool,
    /// Файл, в который уходит stdout, если он задан.
    pub stdout_file: Option<String>,
    /// stderr слит в stdout.
    pub stderr_to_stdout: bool,
}

/// Разложить заказ в план запуска. Ничего не запускает и не меняет.
pub fn run_plan(spec: &RunSpec, path_env: Option<&str>) -> RunPlan {
    let argv = resolved_argv(&spec.argv, path_env);
    let duration = run_duration(&argv);
    let mut env: Vec<(String, String)> = Vec::new();
    if spec.lang_c {
        env.extend(
            PARSED_OUTPUT_ENV
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string())),
        );
    }
    env.extend(spec.env.iter().cloned());
    RunPlan {
        timeout_secs: spec.timeout_secs.unwrap_or_else(|| duration.timeout_secs()),
        duration,
        env,
        text_mode: matches!(spec.output, RunOutput::Capture),
        stdin_null: true,
        stdout_file: match &spec.output {
            RunOutput::File(p) => Some(p.clone()),
            _ => None,
        },
        stderr_to_stdout: spec.stderr_to_stdout,
        argv,
    }
}

/// Итог запуска.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// Что случилось с командой.
    pub outcome: RunOutcome,
    /// Код возврата; `None` — процесс не дошёл до кода (таймаут, не запустился).
    pub returncode: Option<i32>,
    /// Собранный stdout (пусто, если вывод не забирали в память).
    pub stdout: Vec<u8>,
    /// Собранный stderr.
    pub stderr: Vec<u8>,
    /// Таймаут, с которым шли, — чтобы сообщение человеку называло число.
    pub timeout_secs: u64,
    /// Текст ошибки запуска (только при [`RunOutcome::Error`]).
    pub error: String,
}

impl Default for RunOutcome {
    fn default() -> Self {
        RunOutcome::Error
    }
}

/// Байты вывода → строка так, как их отдал бы Python в текстовом режиме.
///
/// `errors="replace"` — то же, что `from_utf8_lossy` (оба ставят U+FFFD).
/// Перевод строк универсальный (`\r\n` и одиночный `\r` → `\n`): его делает
/// `text=True`, и без него строки живого хода сборки различались бы.
pub fn decode_text(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    if !s.contains('\r') {
        return s.into_owned();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Запустить внешнюю команду. ЕДИНСТВЕННАЯ точка запуска процесса в продукте.
///
/// Таймаут — по классу команды, если вызывающий не задал свой; локаль
/// глушится, когда вывод будет разбираться; stdin всегда закрыт. Ни одна
/// ветка не паникует: «не запустилось» — это [`RunOutcome::Error`] с текстом
/// причины, а не исключение.
pub fn run(spec: &RunSpec, path_env: Option<&str>) -> RunReport {
    let plan = run_plan(spec, path_env);
    let mut report = RunReport {
        timeout_secs: plan.timeout_secs,
        ..RunReport::default()
    };
    let Some((bin, rest)) = plan.argv.split_first() else {
        report.error = crate::i18n::tr("empty command");
        return report;
    };

    // Журнал: команда с повышением прав — всегда (что запускается и чем
    // кончилось), остальные — только в подробном журнале (`REXON_LOG=debug`).
    let privileged = crate::applog::is_privileged(&plan.argv);
    let log_level = if privileged { crate::applog::Level::Info } else { crate::applog::Level::Debug };
    let logged = crate::applog::enabled(log_level);
    if logged {
        crate::applog::write(
            log_level,
            if privileged { "privileged" } else { "run" },
            &format!("запуск: {}", crate::applog::redact_argv(&plan.argv)),
        );
    }
    let run_started = std::time::Instant::now();
    let report = run_inner(spec, &plan, bin, rest, report);
    if logged {
        crate::applog::write(
            log_level,
            if privileged { "privileged" } else { "run" },
            &format!(
                "{bin}: {:?}, код {} за {:.1} с{}",
                report.outcome,
                report.returncode.map_or_else(|| "—".to_string(), |c| c.to_string()),
                run_started.elapsed().as_secs_f64(),
                if report.error.is_empty() { String::new() } else { format!(" — {}", report.error) }
            ),
        );
    }
    report
}

fn run_inner(spec: &RunSpec, plan: &RunPlan, bin: &String, rest: &[String], mut report: RunReport) -> RunReport {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let capture = matches!(spec.output, RunOutput::Capture | RunOutput::CaptureBytes);
    let mut cmd = Command::new(bin);
    cmd.args(rest).stdin(Stdio::null());
    if spec.lang_c {
        // `plan.env` уже несёт C-локаль; здесь — ещё и снятие `LANGUAGE`.
        parsed_output_locale(&mut cmd);
    }
    for (k, v) in &plan.env {
        cmd.env(k, v);
    }
    match &spec.output {
        RunOutput::Inherit => {
            cmd.stdout(Stdio::inherit());
        }
        RunOutput::Capture | RunOutput::CaptureBytes => {
            cmd.stdout(Stdio::piped());
        }
        // ⚠️ Файл открывается ОДИН раз и в режиме дозаписи, а stderr
        // получает копию того же дескриптора. Прежде stderr открывался
        // отдельно: у двух открытий разные позиции записи, и stdout
        // затирал строки stderr поверх (из журнала помощника AUR пропадали
        // как раз сообщения об ошибке — sudo/pkexec пишут в stderr).
        RunOutput::File(path) => match fs::File::create(path)
            .and_then(|_| fs::OpenOptions::new().append(true).open(path))
        {
            Ok(f) => {
                if spec.stderr_to_stdout {
                    match f.try_clone() {
                        Ok(dup) => {
                            cmd.stderr(Stdio::from(dup));
                        }
                        Err(e) => {
                            report.error = msg_run_failed(bin, &e.to_string());
                            return report;
                        }
                    }
                }
                cmd.stdout(Stdio::from(f));
            }
            Err(e) => {
                report.error = msg_run_failed(bin, &e.to_string());
                return report;
            }
        },
    }
    if spec.stderr_to_stdout {
        // Тот же приёмник, что у stdout: в Python это `stderr=STDOUT`.
        match &spec.output {
            // Уже отдан копией дескриптора stdout (см. выше).
            RunOutput::File(_) => {}
            RunOutput::Capture | RunOutput::CaptureBytes => {
                cmd.stderr(Stdio::piped());
            }
            RunOutput::Inherit => {
                cmd.stderr(Stdio::inherit());
            }
        }
    } else if capture {
        cmd.stderr(Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            report.error = msg_run_failed(bin, &e.to_string());
            return report;
        }
    };

    // Читатели в своих потоках: полный пайп подвесил бы и ребёнка, и нас.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let t_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let code = wait_secs(&mut child, plan.timeout_secs);
    report.stdout = t_out.join().unwrap_or_default();
    report.stderr = t_err.join().unwrap_or_default();
    // stderr, слитый в stdout по пайпу, дописывается к stdout — как это
    // делает общий дескриптор при `stderr=STDOUT`.
    if spec.stderr_to_stdout && capture {
        let tail = std::mem::take(&mut report.stderr);
        report.stdout.extend_from_slice(&tail);
    }
    match code {
        Some(rc) => {
            report.returncode = Some(rc);
            report.outcome = if rc == 0 {
                RunOutcome::Ok
            } else {
                RunOutcome::Failed
            };
        }
        None => {
            report.outcome = RunOutcome::Timeout;
            // Собранное до таймаута НЕ отдаём: `subprocess.run` в этом случае
            // бросает исключение, и вызывающий не видел вывода никогда.
            report.stdout.clear();
            report.stderr.clear();
        }
    }
    report
}

/// Дождаться процесса не дольше `timeout` секунд; `None` — не дождались.
///
/// По таймауту процесс добивается и дожидается, чтобы не оставить сироту, —
/// ровно как `subprocess.run(timeout=…)`.
fn wait_secs(child: &mut std::process::Child, timeout: u64) -> Option<i32> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Some(exit_code(st)),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

/// Код возврата так, как его видит Python: убитый сигналом процесс — это
/// отрицательный номер сигнала (`-15`), а не пустое место.
fn exit_code(st: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    st.code().unwrap_or_else(|| -st.signal().unwrap_or(0))
}

// ============================================================================
// Корзина (freedesktop.org Trash 1.0)
// ============================================================================
//
// ЗАЧЕМ СВОЯ, а не `gio trash`. Корзина нужна там, где Rexon убирает то, что
// поставил не он (виджет Conky из `~/.conky`): ошибиться можно, и вещь
// обязана вернуться из корзины файлового менеджера. `gio` — часть GLib и на
// урезанной машине (SteamOS, KDE без GTK-программ) может отсутствовать, а
// ответ его кодом возврата не говорит, переехал ли файл или был скопирован.
// Спецификация простая, и здесь она исполняется целиком в тех местах, где
// важна надёжность:
//   * файл ПЕРЕНОСИТСЯ (`rename(2)`), а не копируется: на том же разделе это
//     одна атомарная операция, вещь не может «потеряться посередине»;
//   * на другом разделе — корзина того раздела (`$topdir/.Trash/$uid`, если
//     администратор её завёл как положено, иначе `$topdir/.Trash-$uid`), тоже
//     переносом. Копировать между разделами и стирать оригинал НЕ будем
//     никогда: не удалось перенести — вещь остаётся на месте, и сказано почему;
//   * имя в корзине занимается созданием `info/<имя>.trashinfo` с `O_EXCL`
//     ДО переноса (так требует спецификация): две программы, кладущие
//     одноимённое одновременно, не затрут друг друга. Перенос не удался —
//     запись снимается;
//   * ссылка (symlink) уходит в корзину сама, цель её не трогается.

/// Каталог «домашней» корзины: `$XDG_DATA_HOME/Trash` (иначе
/// `~/.local/share/Trash`).
pub fn home_trash_dir(xdg_data_home: Option<&str>, home: &str) -> String {
    join_path(&data_home(xdg_data_home, home), "Trash")
}

/// Путь для строки `Path=` записи корзины: байты вне «безопасных» для URI
/// (`A-Z a-z 0-9 - _ . ~` и `/`) — `%XX`, как требует спецификация (RFC
/// 2396). Пробел — `%20`, кириллица — по байтам UTF-8.
pub fn trash_escape_path(path: &[u8]) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Текст записи `.trashinfo`. `path` — уже экранированный путь
/// ([`trash_escape_path`]), `date` — местное время `ГГГГ-ММ-ДДTчч:мм:сс`.
pub fn trash_info_text(path: &str, date: &str) -> String {
    format!("[Trash Info]\nPath={path}\nDeletionDate={date}\n")
}

/// Метка `DeletionDate` для «сейчас» — местное время, без часового пояса
/// (так в спецификации). Часы не читаются — полночь 1970 года: запись
/// корзины без даты файловые менеджеры не показывают вовсе.
pub fn trash_deletion_date_now() -> String {
    crate::qt_app::Timestamp::now_local()
        .map(|t| format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second))
        .unwrap_or_else(|| "1970-01-01T00:00:00".to_string())
}

/// Куда положено при переносе в корзину.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    /// Вещь в корзине (`<корзина>/files/<имя>`).
    pub file: PathBuf,
    /// Её запись (`<корзина>/info/<имя>.trashinfo`).
    pub info: PathBuf,
}

/// Перенести `path` в корзину. `home_trash` — [`home_trash_dir`]; `date` —
/// [`trash_deletion_date_now`] (подаётся снаружи ради сверок). `Err` — текст
/// причины; тогда на диске ничего не изменилось.
pub fn move_to_trash(path: &Path, home_trash: &Path, date: &str) -> Result<Trashed, String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("not an absolute path: {}", path.display()));
    }
    let name = path.file_name().ok_or_else(|| format!("no file name: {}", path.display()))?;
    let parent = path.parent().ok_or_else(|| format!("no parent folder: {}", path.display()))?;
    fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    // Раздел определяется по каталогу, где лежит запись (по ссылке на каталог
    // — по настоящему каталогу), а не по цели ссылки-виджета.
    let dev = fs::metadata(parent).map_err(|e| e.to_string())?.dev();
    let (trash, info_path) = match ensure_trash(home_trash) {
        Ok(()) if fs::metadata(home_trash).map(|m| m.dev()).ok() == Some(dev) => {
            (home_trash.to_path_buf(), trash_escape_path(path.as_os_str().as_bytes()))
        }
        _ => {
            let real_parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
            let top = mount_top(&real_parent, dev);
            let trash = topdir_trash(&top)?;
            let rel = real_parent.join(name);
            let rel = rel.strip_prefix(&top).unwrap_or(&rel);
            (trash, trash_escape_path(rel.as_os_str().as_bytes()))
        }
    };
    let text = trash_info_text(&info_path, date);
    let stem = name.to_string_lossy().into_owned();
    for n in 1..=1000u32 {
        let candidate = if n == 1 { stem.clone() } else { format!("{stem}.{n}") };
        let file = trash.join("files").join(&candidate);
        let info = trash.join("info").join(format!("{candidate}.trashinfo"));
        if fs::symlink_metadata(&file).is_ok() {
            continue;
        }
        let created = fs::OpenOptions::new().write(true).create_new(true).open(&info);
        let mut f = match created {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        };
        use std::io::Write;
        if let Err(e) = f.write_all(text.as_bytes()).and_then(|()| f.sync_all()) {
            let _ = fs::remove_file(&info);
            return Err(e.to_string());
        }
        drop(f);
        return match fs::rename(path, &file) {
            Ok(()) => Ok(Trashed { file, info }),
            Err(e) => {
                let _ = fs::remove_file(&info);
                Err(e.to_string())
            }
        };
    }
    Err("no free name in the trash".to_string())
}

/// Завести `files/` и `info/` корзины (права 0700 — содержимое корзины
/// чужим не видно).
fn ensure_trash(trash: &Path) -> std::io::Result<()> {
    for sub in ["files", "info"] {
        let dir = trash.join(sub);
        if !dir.is_dir() {
            fs::create_dir_all(&dir)?;
            let _ = fs::set_permissions(trash, fs::Permissions::from_mode(0o700));
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Точка монтирования раздела `dev`, на котором лежит `dir` (настоящий путь).
fn mount_top(dir: &Path, dev: u64) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let mut top = dir.to_path_buf();
    while let Some(up) = top.parent() {
        if fs::metadata(up).map(|m| m.dev()).ok() != Some(dev) {
            break;
        }
        top = up.to_path_buf();
    }
    top
}

/// Корзина раздела с корнем `top`: `$top/.Trash/$uid`, если `.Trash` —
/// настоящий каталог (не ссылка) с битом sticky, иначе `$top/.Trash-$uid`.
/// Чужой или не-каталог под этим именем — отказ.
fn topdir_trash(top: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid не принимает аргументов и не может завершиться ошибкой.
    let uid = unsafe { libc::getuid() };
    let shared = top.join(".Trash");
    if let Ok(m) = fs::symlink_metadata(&shared) {
        if m.is_dir() && m.mode() & 0o1000 != 0 {
            let mine = shared.join(uid.to_string());
            if ensure_trash(&mine).is_ok() && owned_dir(&mine, uid) {
                return Ok(mine);
            }
        }
    }
    let own = top.join(format!(".Trash-{uid}"));
    if fs::symlink_metadata(&own).is_err() {
        fs::create_dir(&own).map_err(|e| format!("{}: {e}", own.display()))?;
        fs::set_permissions(&own, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    if !owned_dir(&own, uid) {
        return Err(format!("{} is not a trash folder of this user", own.display()));
    }
    ensure_trash(&own).map_err(|e| format!("{}: {e}", own.display()))?;
    Ok(own)
}

/// Каталог (не ссылка) и принадлежит `uid`.
fn owned_dir(p: &Path, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && m.uid() == uid)
}

#[cfg(test)]
mod notify_ask_tests {
    use super::*;

    /// Уведомление с кнопками: argv `notify-send --wait --action=…`, ответ —
    /// ключ нажатой; старый `notify-send` без кнопок узнаётся по ответу.
    #[test]
    fn notify_with_buttons_and_answer() {
        let actions = vec![("open".to_string(), "Открыть".to_string())];
        let argv = notify_ask_argv("/usr/bin/notify-send", "T", "B", &actions);
        assert_eq!(argv[0], "/usr/bin/notify-send");
        assert!(argv.contains(&"--wait".to_string()) && argv.contains(&"--action=open=Открыть".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["T", "B"]);
        assert_eq!(notify_answer("\nopen\n", &actions).as_deref(), Some("open"));
        assert_eq!(notify_answer("", &actions), None, "закрыли без ответа");
        assert_eq!(notify_answer("other\n", &actions), None);
        assert!(notify_send_lacks_actions("Unknown option --action=open=Открыть"));
        assert!(notify_send_lacks_actions("notify-send: Unknown option --wait"));
        assert!(!notify_send_lacks_actions(""));
    }
}

#[cfg(test)]
mod trash_tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rexon-trash-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn escape_keeps_slashes_and_encodes_space_and_cyrillic() {
        assert_eq!(trash_escape_path(b"/home/u/.conky/Gotham"), "/home/u/.conky/Gotham");
        assert_eq!(trash_escape_path("/home/u/Мои виджеты/a%b".as_bytes()), "/home/u/%D0%9C%D0%BE%D0%B8%20%D0%B2%D0%B8%D0%B4%D0%B6%D0%B5%D1%82%D1%8B/a%25b");
        assert_eq!(trash_info_text("/a", "2026-10-19T12:00:05"), "[Trash Info]\nPath=/a\nDeletionDate=2026-10-19T12:00:05\n");
        let now = trash_deletion_date_now();
        assert_eq!(now.len(), 19, "{now}");
        assert_eq!(&now[10..11], "T");
    }

    /// Папка переносится целиком, запись корзины верна; одноимённая вторая —
    /// под свободным именем, первая не затирается.
    #[test]
    fn folder_moves_with_info_and_names_do_not_clash() {
        let base = sandbox("dir");
        let trash = base.join("data/Trash");
        let w = base.join("Мои виджеты/Gotham");
        fs::create_dir_all(&w).unwrap();
        fs::write(w.join("Gotham"), "TEXT\n").unwrap();
        let t = move_to_trash(&w, &trash, "2026-10-19T01:02:03").unwrap();
        assert!(!w.exists());
        assert_eq!(t.file, trash.join("files/Gotham"));
        assert_eq!(fs::read_to_string(t.file.join("Gotham")).unwrap(), "TEXT\n");
        let want = format!(
            "[Trash Info]\nPath={}\nDeletionDate=2026-10-19T01:02:03\n",
            trash_escape_path(w.as_os_str().as_encoded_bytes())
        );
        assert_eq!(fs::read_to_string(&t.info).unwrap(), want);
        assert!(want.contains("%D0%9C%D0%BE%D0%B8%20"), "{want}");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(trash.join("info")).unwrap().permissions().mode() & 0o777, 0o700);

        fs::create_dir_all(&w).unwrap();
        let t2 = move_to_trash(&w, &trash, "2026-10-19T01:02:04").unwrap();
        assert_eq!(t2.file, trash.join("files/Gotham.2"));
        assert_eq!(t2.info, trash.join("info/Gotham.2.trashinfo"));
        assert!(t.file.join("Gotham").exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// Ссылка уходит в корзину сама; цель остаётся на месте нетронутой.
    #[test]
    fn symlink_goes_to_trash_not_its_target() {
        let base = sandbox("link");
        let trash = base.join("Trash");
        let target = base.join("real/Gotham");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("conf"), "TEXT\n").unwrap();
        let link = base.join(".conky/Gotham");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let t = move_to_trash(&link, &trash, "2026-10-19T01:02:03").unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(fs::symlink_metadata(&t.file).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(target.join("conf")).unwrap(), "TEXT\n");
        let _ = fs::remove_dir_all(&base);
    }

    /// Вещь на другом разделе (`/dev/shm` — отдельный tmpfs) — в корзину ТОГО
    /// раздела (`$topdir/.Trash-$uid`) переносом, путь в записи — от корня
    /// раздела. Нет отдельного раздела или его корзина уже есть — проверка
    /// пропускается (чужую корзину не трогаем).
    #[test]
    fn other_partition_uses_its_own_trash_by_move() {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        let base = sandbox("xdev");
        let uid = unsafe { libc::getuid() };
        let topdir_trash = shm.join(format!(".Trash-{uid}"));
        let other_dev = fs::metadata(shm).map(|m| m.dev()).ok().filter(|d| Some(*d) != fs::metadata(&base).map(|m| m.dev()).ok());
        if other_dev.is_none() || fs::symlink_metadata(&topdir_trash).is_ok() {
            let _ = fs::remove_dir_all(&base);
            return;
        }
        let w = shm.join(format!("rexon-trash-xdev-{}", std::process::id()));
        fs::create_dir_all(&w).unwrap();
        fs::write(w.join("c"), "TEXT\n").unwrap();
        let res = move_to_trash(&w, &base.join("Trash"), "2026-10-19T00:00:00");
        let moved = res.as_ref().map(|t| t.file.join("c").is_file()).unwrap_or(false);
        let info = res.as_ref().ok().and_then(|t| fs::read_to_string(&t.info).ok()).unwrap_or_default();
        let home_trash_used = base.join("Trash/files").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false);
        let _ = fs::remove_dir_all(&w);
        let _ = fs::remove_dir_all(&topdir_trash);
        let _ = fs::remove_dir_all(&base);
        let t = res.unwrap();
        assert!(t.file.starts_with(&topdir_trash), "{t:?}");
        assert!(moved && !home_trash_used);
        let name = w.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(info, format!("[Trash Info]\nPath={name}\nDeletionDate=2026-10-19T00:00:00\n"));
    }

    /// Нечего переносить или путь не абсолютный — отказ, записи не остаётся.
    #[test]
    fn missing_or_relative_is_refused_without_leftovers() {
        let base = sandbox("missing");
        let trash = base.join("Trash");
        assert!(move_to_trash(&base.join("nope"), &trash, "d").is_err());
        assert!(move_to_trash(Path::new("rel/x"), &trash, "d").is_err());
        assert!(move_to_trash(&base.join("a/../b"), &trash, "d").is_err());
        let infos = fs::read_dir(trash.join("info")).map(|d| d.count()).unwrap_or(0);
        assert_eq!(infos, 0);
        let _ = fs::remove_dir_all(&base);
    }
}
