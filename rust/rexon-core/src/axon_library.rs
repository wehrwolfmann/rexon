//! Библиотека обоев Razer Axon, поставленного под Wine.
//!
//! Razer Axon — программа живых обоев для Windows. Под Linux её ставят в
//! префикс Wine (свой `~/.wine`, бутылка Bottles, префикс Lutris или
//! Proton), и скачанные в ней обои лежат внутри префикса. Модуль только
//! ЧИТАЕТ эту библиотеку: какие обои у человека есть, как они называются,
//! кто автор, видео это или картинка, есть ли звук, скачаны ли они и где
//! лежат их файлы. Сам этот модуль префикс Axon не меняет никогда — ни
//! запись, ни переименование, ни удаление; подготовку обоев заранее прямо в
//! библиотеке Axon (по согласию человека, с резервной копией описания)
//! делает отдельный модуль [`crate::axon_prepare`].
//!
//! Раскладка внутри префикса (`<prefix>/drive_c/users/<user>/AppData/Local/
//! Razer/RazerAxon/`):
//!
//! * `<учётная запись>/WallPaperData` — JSON-массив библиотеки (UTF-8 с BOM):
//!   `Id`, `Title`, `Author.author_name`, `Type` либо `WallPaperType`
//!   (`VIDEO`/`IMAGE`/`WEB`…), `EffectType`, `Audible` (`"1"`/`"0"`),
//!   `Resolution` (`3840x2160`), `WallPaperStatus`, `PlayResoucePath`
//!   (так, с опечаткой, пишет сам Axon), `ThumbnailSource`, `PreviewSource`;
//! * `<учётная запись>/PlayListSetting` — плейлисты по экранам: какие обои и
//!   какие из них сейчас на экране;
//! * `<учётная запись>/UserSetting` — `SourceSavedPath`, где лежат скачанные;
//! * `WallpaperSource/{Id}{Высота}{Ширина}/` — скачанные обои:
//!   `ResourceConfig.txt` (вид и путь к файлу) и `Resource/…` — контейнер,
//!   который перед показом нужно подготовить: это работа [`AxonExtractor`]
//!   (встроенная — [`BuiltinExtractor`], `crate::axon_extract`).
//!
//! Учётных записей в одном каталоге бывает несколько (`RZR_<uid>` после
//! входа, `guest-<…>` до него): берутся все, у кого есть библиотека, а
//! одинаковые обои — из самой свежей.
//!
//! Искать префиксы по всему диску модуль не будет: смотрит в известные места
//! (`$WINEPREFIX`, `~/.wine`, Bottles, Lutris, Heroic, umu, PlayOnLinux,
//! префиксы Proton в библиотеках Steam) на один уровень вглубь.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::i18n::{tr, trf};

/// Путь от каталога пользователя Windows до корня Axon.
pub const AXON_SUBDIR: &[&str] = &["AppData", "Local", "Razer", "RazerAxon"];
/// Файл библиотеки в каталоге учётной записи.
pub const LIBRARY_FILE: &str = "WallPaperData";
/// Плейлисты учётной записи.
pub const PLAYLIST_FILE: &str = "PlayListSetting";
/// Настройки учётной записи (`SourceSavedPath`).
pub const USER_SETTING_FILE: &str = "UserSetting";
/// Описание скачанных обоев в их каталоге.
pub const RESOURCE_CONFIG: &str = "ResourceConfig.txt";
/// Каталог скачанного по умолчанию (когда `UserSetting` молчит).
pub const DEFAULT_SOURCE_DIR: &str = "WallpaperSource";


/// Отметка готового файла в каталоге кэша.
pub const READY_STAMP: &str = "ready.json";

// ============================================================================
// Окружение
// ============================================================================

/// Всё, что модуль берёт из окружения процесса. Сверки подставляют своё.
#[derive(Debug, Clone, Default)]
pub struct AxonEnv {
    pub home: PathBuf,
    /// `$WINEPREFIX`, если задан.
    pub wineprefix: Option<PathBuf>,
    /// `$XDG_CACHE_HOME`, если задан.
    pub cache_home: Option<PathBuf>,
    /// `$XDG_DATA_HOME`, если задан.
    pub data_home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, если задан.
    pub config_home: Option<PathBuf>,
    /// Папки с обоями (`WallpaperSource`), указанные человеком вручную
    /// (настройка [`SETTINGS_KEY`]).
    pub source_dirs: Vec<PathBuf>,
}

impl AxonEnv {
    /// Окружение живого процесса.
    pub fn from_process() -> AxonEnv {
        let dir = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        AxonEnv {
            home: dir("HOME").unwrap_or_default(),
            wineprefix: dir("WINEPREFIX"),
            cache_home: dir("XDG_CACHE_HOME"),
            data_home: dir("XDG_DATA_HOME"),
            config_home: dir("XDG_CONFIG_HOME"),
            source_dirs: Vec::new(),
        }
    }

    /// Окружение «только дом» — для сверок.
    pub fn with_home(home: impl Into<PathBuf>) -> AxonEnv {
        AxonEnv { home: home.into(), ..AxonEnv::default() }
    }

    fn data_home(&self) -> PathBuf {
        self.data_home.clone().unwrap_or_else(|| self.home.join(".local/share"))
    }

    fn config_home(&self) -> PathBuf {
        self.config_home.clone().unwrap_or_else(|| self.home.join(".config"))
    }

    /// Резервные копии исходных описаний (`ResourceConfig.txt`) обоев,
    /// подготовленных заранее: `~/.local/share/rexon/axon-originals`.
    pub fn originals_root(&self) -> PathBuf {
        self.data_home().join("rexon").join("axon-originals")
    }

    /// Каталог кэша Rexon для обоев Axon: `~/.cache/rexon/axon`.
    pub fn cache_root(&self) -> PathBuf {
        self.cache_home
            .clone()
            .unwrap_or_else(|| self.home.join(".cache"))
            .join("rexon")
            .join("axon")
    }
}

// ============================================================================
// Поиск префиксов
// ============================================================================

/// Похож ли каталог на префикс Wine.
fn is_prefix(dir: &Path) -> bool {
    dir.join("drive_c").is_dir()
}

/// Подкаталоги `dir` (один уровень, по имени), без ошибок наружу.
fn children(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

/// Каталоги библиотек Steam: основная и перечисленные в
/// `steamapps/libraryfolders.vdf` (строки `"path" "…"`).
fn steam_libraries(env: &AxonEnv) -> Vec<PathBuf> {
    let roots = [
        env.data_home().join("Steam"),
        env.home.join(".steam/steam"),
        env.home.join(".steam/root"),
        env.home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        env.home.join("snap/steam/common/.local/share/Steam"),
    ];
    let mut libs: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        libs.push(root.clone());
        let vdf = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            // `\t\t"path"\t\t"/mnt/games/SteamLibrary"` → 5 кусков.
            if parts.len() >= 4 && parts[1] == "path" {
                libs.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
            }
        }
    }
    libs
}

/// Префиксы Lutris из описаний игр (`prefix:` в `games/*.yml`).
fn lutris_prefixes(env: &AxonEnv) -> Vec<PathBuf> {
    let dirs = [
        env.config_home().join("lutris/games"),
        env.data_home().join("lutris/games"),
        env.home.join(".var/app/net.lutris.Lutris/config/lutris/games"),
        env.home.join(".var/app/net.lutris.Lutris/data/lutris/games"),
    ];
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("yml") {
                continue;
            }
            for line in fs::read_to_string(&p).unwrap_or_default().lines() {
                if let Some(rest) = line.trim_start().strip_prefix("prefix:") {
                    let v = rest.trim().trim_matches(|c| c == '\'' || c == '"');
                    if !v.is_empty() {
                        let v = match v.strip_prefix("~/") {
                            Some(r) => env.home.join(r),
                            None => PathBuf::from(v),
                        };
                        out.push(v);
                    }
                }
            }
        }
    }
    out
}

/// Префиксы Wine, в которых МОЖЕТ стоять Axon, — без проверки на Axon.
///
/// Порядок — от явного к общему: `$WINEPREFIX`, `~/.wine`, затем менеджеры
/// игр. Повторы (тот же префикс по двум дорогам) убраны.
pub fn candidate_prefixes(env: &AxonEnv) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if is_prefix(&p) && !found.contains(&p) {
            found.push(p);
        }
    };
    if let Some(p) = &env.wineprefix {
        push(p.clone());
    }
    push(env.home.join(".wine"));
    let data = env.data_home();
    let one_level = [
        data.join("wineprefixes"),
        data.join("bottles/bottles"),
        env.home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
        env.home.join("Games"),
        env.home.join("Games/umu"),
        env.home.join("Games/Heroic/Prefixes"),
        env.home.join("Games/Heroic/Prefixes/default"),
        env.home.join(".PlayOnLinux/wineprefix"),
    ];
    for dir in one_level {
        for child in children(&dir) {
            push(child);
        }
    }
    for p in lutris_prefixes(env) {
        push(p);
    }
    for lib in steam_libraries(env) {
        for app in children(&lib.join("steamapps/compatdata")) {
            push(app.join("pfx"));
        }
    }
    found
}

/// Каталог Axon одного пользователя Windows в префиксе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxonRoot {
    /// Префикс Wine.
    pub prefix: PathBuf,
    /// Корень Axon: `<prefix>/drive_c/users/<user>/AppData/Local/Razer/RazerAxon`.
    pub root: PathBuf,
}

/// Каталоги Axon в префиксе — по одному на пользователя Windows.
pub fn roots_in_prefix(prefix: &Path) -> Vec<AxonRoot> {
    children(&prefix.join("drive_c/users"))
        .into_iter()
        .map(|user| AXON_SUBDIR.iter().fold(user, |p, part| p.join(part)))
        .filter(|root| root.is_dir())
        .map(|root| AxonRoot { prefix: prefix.to_path_buf(), root })
        .collect()
}

/// Каталоги учётных записей Axon с библиотекой (`WallPaperData`).
pub fn account_dirs(root: &Path) -> Vec<PathBuf> {
    children(root).into_iter().filter(|d| d.join(LIBRARY_FILE).is_file()).collect()
}

/// Все каталоги Axon с библиотекой в известных местах.
pub fn find_roots(env: &AxonEnv) -> Vec<AxonRoot> {
    candidate_prefixes(env)
        .iter()
        .flat_map(|p| roots_in_prefix(p))
        .filter(|r| !account_dirs(&r.root).is_empty())
        .collect()
}

// ============================================================================
// Пути Windows → Linux
// ============================================================================

/// Путь Windows (`C:\users\…`) внутри префикса → путь Linux.
///
/// `C:` — `drive_c`; другие буквы — через `dosdevices/<x>:` (так Wine их и
/// хранит; `Z:` обычно ведёт в `/`). Регистр букв в именах каталогов Wine
/// на диске сохраняет, поэтому сегменты берутся как есть, а несовпадение
/// регистра поправляется поиском без учёта регистра.
pub fn windows_to_linux(prefix: &Path, win: &str) -> Option<PathBuf> {
    let win = win.trim();
    let bytes = win.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let letter = (bytes[0] as char).to_ascii_lowercase();
    let mut base = if letter == 'c' {
        prefix.join("drive_c")
    } else {
        let dev = prefix.join("dosdevices").join(format!("{letter}:"));
        match fs::read_link(&dev) {
            Ok(target) if target.is_absolute() => target,
            Ok(target) => prefix.join("dosdevices").join(target),
            Err(_) if dev.exists() => dev,
            Err(_) => return None,
        }
    };
    for seg in win[2..].split(['\\', '/']).filter(|s| !s.is_empty()) {
        if seg == "." {
            continue;
        }
        if seg == ".." {
            base.pop();
            continue;
        }
        let exact = base.join(seg);
        base = if exact.exists() {
            exact
        } else {
            find_case_insensitive(&base, seg).unwrap_or(exact)
        };
    }
    Some(base)
}

fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = name.to_lowercase();
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().to_lowercase() == want)
        .map(|e| e.path())
}

/// Относительный путь из `ResourceConfig.txt` (`Resource\\a.mp4`) внутри
/// каталога обоев. Выход за каталог (`..`, абсолютный путь) — `None`.
pub fn resource_relative(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    let p = Path::new(&rel);
    if p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    Some(dir.join(p))
}

// ============================================================================
// Разбор файлов Axon
// ============================================================================

/// Вид обоев.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxonKind {
    /// Ролик (`VIDEO`).
    Video,
    /// Картинка (`IMAGE`).
    Image,
    /// Веб-страница (`WEB`, `LOCALWEB`).
    Web,
    /// Прочее — как написал Axon.
    Other(String),
}

impl AxonKind {
    pub fn parse(raw: &str) -> AxonKind {
        match raw.trim().to_ascii_uppercase().as_str() {
            "VIDEO" | "LOCALVIDEO" => AxonKind::Video,
            "IMAGE" | "LOCALIMAGE" | "PICTURE" => AxonKind::Image,
            "WEB" | "LOCALWEB" => AxonKind::Web,
            _ => AxonKind::Other(raw.trim().to_string()),
        }
    }

    /// Подпись вида для плитки.
    pub fn label(&self) -> String {
        match self {
            AxonKind::Video => tr("🎞️ Video"),
            AxonKind::Image => tr("🖼️ Picture"),
            AxonKind::Web => tr("🌐 Web page"),
            AxonKind::Other(s) if s.is_empty() => tr("Unknown kind"),
            AxonKind::Other(s) => s.clone(),
        }
    }
}

/// Тип обоев из записи: `WallPaperType` (новые версии) либо `Type` (старые).
pub fn kind_field(v: &Value) -> Option<&str> {
    ["WallPaperType", "Type"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()))
}

/// Текст файла Axon: без BOM в начале.
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str(strip_bom(&text)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Строка из поля, где Axon пишет то число, то строку.
fn loose_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Одна запись `WallPaperData` — как её записал Axon.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryEntry {
    pub id: String,
    pub title: String,
    pub author: String,
    /// Тип из записи (`VIDEO`…); пусто — Axon не написал.
    pub kind: String,
    pub effect: String,
    pub audible: bool,
    /// `3840x2160` → (3840, 2160).
    pub resolution: Option<(u32, u32)>,
    pub status: String,
    /// Путь Windows к файлу показа (`PlayResoucePath`).
    pub play_path: String,
    pub thumbnail_url: String,
    pub preview_url: String,
    pub description: String,
    pub tags: String,
}

/// `3840x2160` → (ширина, высота).
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.trim().split_once(['x', 'X', '×'])?;
    let w = w.trim().parse().ok()?;
    let h = h.trim().parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Разбор `WallPaperData`. Записи без `Id` пропускаются; файл, который не
/// JSON-массив, — ошибка (её текст — для журнала).
pub fn parse_library(text: &str) -> Result<Vec<LibraryEntry>, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    let list = v.as_array().ok_or_else(|| "WallPaperData: ожидался массив".to_string())?;
    let mut out = Vec::with_capacity(list.len());
    for raw in list {
        let id = loose_str(raw.get("Id"));
        if id.is_empty() {
            continue;
        }
        let author = match raw.get("Author") {
            Some(Value::Object(a)) => loose_str(a.get("author_name")),
            other => loose_str(other),
        };
        out.push(LibraryEntry {
            title: loose_str(raw.get("Title")),
            author,
            kind: kind_field(raw).unwrap_or("").trim().to_string(),
            effect: loose_str(raw.get("EffectType")),
            audible: matches!(loose_str(raw.get("Audible")).as_str(), "1" | "true" | "True"),
            resolution: parse_resolution(&loose_str(raw.get("Resolution"))),
            status: loose_str(raw.get("WallPaperStatus")),
            play_path: loose_str(raw.get("PlayResoucePath")),
            thumbnail_url: loose_str(raw.get("ThumbnailSource")),
            preview_url: loose_str(raw.get("PreviewSource")),
            description: loose_str(raw.get("Description")),
            tags: loose_str(raw.get("AllTags")),
            id,
        });
    }
    Ok(out)
}

/// Плейлист одного экрана.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayList {
    pub monitor: String,
    pub wallpaper_ids: Vec<String>,
    pub current_index: usize,
    pub mute: bool,
}

impl PlayList {
    /// Обои, которые Axon показывает на этом экране сейчас.
    pub fn current(&self) -> Option<&str> {
        self.wallpaper_ids.get(self.current_index).map(String::as_str)
    }
}

/// Разбор `PlayListSetting`.
pub fn parse_playlists(text: &str) -> Result<Vec<PlayList>, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    let lists = v
        .get("PlayLists")
        .and_then(Value::as_array)
        .ok_or_else(|| "PlayListSetting: нет PlayLists".to_string())?;
    Ok(lists
        .iter()
        .map(|p| PlayList {
            monitor: loose_str(p.get("MonitorId")),
            wallpaper_ids: p
                .get("WallpaperIds")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|x| loose_str(Some(x))).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default(),
            current_index: p.get("CurrentPlayIndex").and_then(Value::as_u64).unwrap_or(0) as usize,
            mute: p.get("IsMute").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect())
}

/// Описание скачанных обоев (`ResourceConfig.txt`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceConfig {
    pub kind: AxonKind,
    /// Путь к файлу показа относительно каталога обоев, как записан.
    pub source: String,
}

/// Разбор `ResourceConfig.txt`: оба имени поля типа и BOM.
pub fn parse_resource_config(text: &str) -> Result<ResourceConfig, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    Ok(ResourceConfig {
        kind: AxonKind::parse(kind_field(&v).unwrap_or("")),
        source: loose_str(v.get("Source")),
    })
}

// ============================================================================
// Модель обоев
// ============================================================================

/// Одни обои библиотеки Axon — всё, что нужно плитке и «Применить».
#[derive(Debug, Clone, PartialEq)]
pub struct AxonWallpaper {
    pub id: String,
    pub title: String,
    pub author: String,
    pub kind: AxonKind,
    /// `Dynamic` — «динамические» обои Axon.
    pub effect: String,
    /// Звук у ролика есть и задуман.
    pub audible: bool,
    pub resolution: Option<(u32, u32)>,
    /// Обложка в сети (CDN Axon).
    pub cover_url: String,
    /// Обложка на диске, если Axon её сохранил.
    pub cover_local: Option<PathBuf>,
    /// Каталог скачанных обоев (`WallpaperSource/{Id}{H}{W}`), если есть.
    pub resource_dir: Option<PathBuf>,
    /// Файл-контейнер внутри него, если есть.
    pub container: Option<PathBuf>,
    /// Скачаны ли обои целиком (каталог, описание и контейнер на месте).
    pub downloaded: bool,
    /// Показывает ли их Axon сейчас (по плейлисту).
    pub playing: bool,
    /// Состояние из библиотеки (`WallPaperStatus`: `Installed`, `Playing`…).
    pub status: String,
    pub description: String,
    /// Префикс Wine, из которого обои.
    pub prefix: PathBuf,
}

impl AxonWallpaper {
    /// Ключ каталога кэша: номер Axon без символов, которым не место в
    /// имени каталога.
    pub fn cache_key(&self) -> String {
        sanitize_id(&self.id)
    }
}

fn sanitize_id(id: &str) -> String {
    let s: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() { "_".to_string() } else { s }
}

/// Каталог скачанных обоев: путь показа из библиотеки, а без него —
/// `{Id}{H}{W}` в хранилище, затем любой `{Id}…` с описанием внутри.
fn locate_resource_dir(prefix: &Path, store: &Path, e: &LibraryEntry) -> Option<PathBuf> {
    let has_config = |d: &Path| d.join(RESOURCE_CONFIG).is_file();
    if !e.play_path.is_empty() {
        if let Some(file) = windows_to_linux(prefix, &e.play_path) {
            // …\{Id}{H}{W}\Resource\x.mp4 → …\{Id}{H}{W}
            let dir = file.parent().and_then(Path::parent).map(Path::to_path_buf);
            if let Some(d) = dir.filter(|d| has_config(d)) {
                return Some(d);
            }
        }
    }
    if let Some((w, h)) = e.resolution {
        let d = store.join(format!("{}{h}{w}", e.id));
        if has_config(&d) {
            return Some(d);
        }
    }
    // Без разрешения в записи: самый длинный `{Id}<цифры>` — родная
    // раскладка Axon; сам `{Id}` — раскладка сторонних загрузчиков.
    let mut best: Option<PathBuf> = None;
    for d in children(store) {
        let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(rest) = name.strip_prefix(&e.id) else { continue };
        if !rest.chars().all(|c| c.is_ascii_digit()) || !has_config(&d) {
            continue;
        }
        let longer = best
            .as_ref()
            .map(|b| b.file_name().map_or(0, |n| n.len()) < name.len())
            .unwrap_or(true);
        if longer {
            best = Some(d);
        }
    }
    best
}

/// Где у учётной записи хранилище скачанного.
fn store_dir(prefix: &Path, root: &Path, account: &Path) -> PathBuf {
    read_json(&account.join(USER_SETTING_FILE))
        .ok()
        .and_then(|v| v.get("SourceSavedPath").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| windows_to_linux(prefix, &s))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| root.join(DEFAULT_SOURCE_DIR))
}

/// Обложка, сохранённая Axon (`WallpaperCovers/<Id>.*`), если есть.
fn local_cover(root: &Path, id: &str) -> Option<PathBuf> {
    let dir = root.join("WallpaperCovers");
    let rd = fs::read_dir(&dir).ok()?;
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| p.file_stem().map(|s| s.to_string_lossy() == id).unwrap_or(false))
}

/// Описание и контейнер скачанных обоев в каталоге `dir`.
fn read_resource(dir: &Path) -> (Option<ResourceConfig>, Option<PathBuf>) {
    let config = fs::read(dir.join(RESOURCE_CONFIG))
        .ok()
        .and_then(|b| parse_resource_config(&String::from_utf8_lossy(&b)).ok());
    let container = config
        .as_ref()
        .filter(|c| !c.source.is_empty())
        .and_then(|c| resource_relative(dir, &c.source));
    (config, container)
}

/// Обои из записи библиотеки (если есть) и каталога скачанного (если есть).
fn build(
    prefix: &Path,
    root: Option<&Path>,
    id: &str,
    entry: Option<&LibraryEntry>,
    resource_dir: Option<PathBuf>,
    playing: bool,
) -> AxonWallpaper {
    let (config, container) = match &resource_dir {
        Some(d) => read_resource(d),
        None => (None, None),
    };
    let downloaded = container.as_ref().is_some_and(|c| c.is_file());
    // Вид — из описания скачанного (оно точнее), иначе из библиотеки.
    let kind = match &config {
        Some(c) if !matches!(&c.kind, AxonKind::Other(s) if s.is_empty()) => c.kind.clone(),
        _ => AxonKind::parse(entry.map_or("", |e| e.kind.as_str())),
    };
    // Разрешение: из библиотеки, иначе из имени файла (`3840x2160.mp4`).
    let resolution = entry.and_then(|e| e.resolution).or_else(|| {
        container
            .as_ref()
            .and_then(|c| c.file_stem())
            .and_then(|s| parse_resolution(&s.to_string_lossy()))
    });
    let e = entry.cloned().unwrap_or_default();
    AxonWallpaper {
        cover_local: root.and_then(|r| local_cover(r, id)),
        playing,
        id: id.to_string(),
        title: e.title,
        author: e.author,
        kind,
        effect: e.effect,
        audible: e.audible,
        resolution,
        cover_url: if e.thumbnail_url.is_empty() { e.preview_url } else { e.thumbnail_url },
        resource_dir,
        container,
        downloaded,
        status: e.status,
        description: e.description,
        prefix: prefix.to_path_buf(),
    }
}

/// Что Axon показывает сейчас по плейлистам учётной записи.
fn playing_ids(account: &Path) -> Vec<String> {
    fs::read(account.join(PLAYLIST_FILE))
        .ok()
        .and_then(|b| parse_playlists(&String::from_utf8_lossy(&b)).ok())
        .map(|lists| lists.iter().filter_map(|p| p.current().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn read_entries(account: &Path) -> Result<Vec<LibraryEntry>, String> {
    let bytes = fs::read(account.join(LIBRARY_FILE)).map_err(|e| e.to_string())?;
    parse_library(&String::from_utf8_lossy(&bytes))
}

/// Обои одной учётной записи.
pub fn read_account(prefix: &Path, root: &Path, account: &Path) -> Result<Vec<AxonWallpaper>, String> {
    let entries = read_entries(account)?;
    let playing = playing_ids(account);
    let store = store_dir(prefix, root, account);
    Ok(entries
        .iter()
        .map(|e| {
            let dir = locate_resource_dir(prefix, &store, e);
            build(prefix, Some(root), &e.id, Some(e), dir, playing.contains(&e.id))
        })
        .collect())
}

fn mtime(p: &Path) -> SystemTime {
    fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

// ── Папка с обоями, указанная вручную ──────────────────────────────────

/// Ключ настройки Rexon: папки с обоями Axon, указанные вручную (массив
/// строк в `settings.json`).
pub const SETTINGS_KEY: &str = "axon_wallpaper_dirs";

/// Папки из настроек Rexon (только строки, пустые и повторы — прочь).
pub fn saved_source_dirs(settings: &serde_json::Map<String, Value>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for v in settings.get(SETTINGS_KEY).and_then(Value::as_array).into_iter().flatten() {
        if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            let p = PathBuf::from(s);
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// Список для настроек после добавления `dir` (без повторов, в конец).
pub fn with_source_dir(saved: &[PathBuf], dir: &Path) -> Value {
    let mut list: Vec<PathBuf> = saved.to_vec();
    if !list.iter().any(|p| p == dir) {
        list.push(dir.to_path_buf());
    }
    Value::Array(list.iter().map(|p| Value::from(p.to_string_lossy().into_owned())).collect())
}

/// Каталоги обоев (`{Id}{H}{W}` с `ResourceConfig.txt`) в папке.
fn resource_dirs_in(dir: &Path) -> Vec<PathBuf> {
    children(dir).into_iter().filter(|d| d.join(RESOURCE_CONFIG).is_file()).collect()
}

/// Проверить папку, которую выбрал человек: это должна быть папка с обоями
/// Axon (`WallpaperSource`). Выбран каталог одних обоев — берётся папка над
/// ним. Ошибка — текст для человека.
pub fn check_source_dir(dir: &Path) -> Result<PathBuf, String> {
    if !dir.is_dir() {
        return Err(folder_missing_text());
    }
    let dir = if dir.join(RESOURCE_CONFIG).is_file() {
        dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf())
    } else {
        dir.to_path_buf()
    };
    if resource_dirs_in(&dir).is_empty() {
        return Err(folder_not_axon_text());
    }
    Ok(dir)
}

/// Высоты и ширины экранов, которыми Axon подписывает каталоги
/// (`{Id}{Высота}{Ширина}`), — чтобы отделить номер, когда библиотеки рядом нет.
const HEIGHTS: &[u32] = &[720, 768, 800, 900, 1080, 1200, 1440, 1600, 1800, 2160, 2400, 2880, 4320];
const WIDTHS: &[u32] = &[1280, 1366, 1440, 1600, 1920, 2048, 2560, 2880, 3440, 3840, 5120, 7680];

/// Номер обоев из имени каталога `{Id}{H}{W}`, когда библиотеки нет: по
/// разрешению из имени файла, иначе по известным разрешениям экрана, иначе
/// всё имя.
pub fn id_from_dir_name(name: &str, resolution: Option<(u32, u32)>) -> String {
    let strip = |w: u32, h: u32| {
        name.strip_suffix(&format!("{h}{w}"))
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
    };
    if let Some(id) = resolution.and_then(|(w, h)| strip(w, h)) {
        return id;
    }
    for &w in WIDTHS {
        for &h in HEIGHTS {
            if let Some(id) = strip(w, h) {
                return id;
            }
        }
    }
    name.to_string()
}

/// Обои из папки `WallpaperSource`, указанной вручную. Название, автора и
/// обложку берём из библиотеки рядом (папка над ней → `*/WallPaperData`),
/// если она есть; иначе — что знает `ResourceConfig.txt`, и номер.
pub fn read_source_dir(dir: &Path) -> Result<Vec<AxonWallpaper>, String> {
    let dir = check_source_dir(dir)?;
    let root = dir.parent().map(Path::to_path_buf);
    let prefix = dir
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "drive_c"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    // Библиотека рядом: самая свежая запись побеждает.
    let mut entries: Vec<LibraryEntry> = Vec::new();
    let mut playing: Vec<String> = Vec::new();
    if let Some(root) = &root {
        let mut accounts = account_dirs(root);
        accounts.sort_by_key(|a| std::cmp::Reverse(mtime(&a.join(LIBRARY_FILE))));
        for acc in accounts {
            playing.extend(playing_ids(&acc));
            for e in read_entries(&acc).unwrap_or_default() {
                if !entries.iter().any(|x| x.id == e.id) {
                    entries.push(e);
                }
            }
        }
    }
    let mut out: Vec<AxonWallpaper> = Vec::new();
    for res in resource_dirs_in(&dir) {
        let name = res.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Запись библиотеки, чей номер — начало имени, а остальное — цифры;
        // из нескольких — с самым длинным номером.
        let entry = entries
            .iter()
            .filter(|e| name.strip_prefix(&e.id).is_some_and(|r| r.chars().all(|c| c.is_ascii_digit())))
            .max_by_key(|e| e.id.len());
        let id = match entry {
            Some(e) => e.id.clone(),
            None => {
                let (_, container) = read_resource(&res);
                let resolution = container
                    .as_ref()
                    .and_then(|c| c.file_stem())
                    .and_then(|s| parse_resolution(&s.to_string_lossy()));
                id_from_dir_name(&name, resolution)
            }
        };
        let w = build(&prefix, root.as_deref(), &id, entry, Some(res), playing.contains(&id));
        merge(&mut out, w);
    }
    Ok(out)
}

/// Добавить обои в список: такие же (тот же номер) уже есть — оставить
/// скачанные, а из двух скачанных — родную раскладку Axon (длинное имя
/// каталога `{Id}{H}{W}`), не сторонний `{Id}`.
fn merge(list: &mut Vec<AxonWallpaper>, w: AxonWallpaper) {
    let name_len = |x: &AxonWallpaper| {
        x.resource_dir.as_ref().and_then(|d| d.file_name()).map_or(0, |n| n.len())
    };
    match list.iter().position(|x| x.id == w.id) {
        None => list.push(w),
        Some(i) => {
            let old = &list[i];
            let better = (!old.downloaded && w.downloaded)
                || (old.downloaded == w.downloaded && name_len(&w) > name_len(old));
            if better {
                // Сведения библиотеки не теряем, если у новой записи их нет.
                let mut w = w;
                if w.title.is_empty() {
                    w.title = old.title.clone();
                    w.author = old.author.clone();
                    w.cover_url = old.cover_url.clone();
                    w.audible = old.audible;
                }
                list[i] = w;
            }
        }
    }
}

/// Библиотека Axon на машине: все найденные каталоги и учётные записи.
#[derive(Debug, Clone, Default)]
pub struct AxonLibrary {
    pub wallpapers: Vec<AxonWallpaper>,
    /// Папки с обоями (`WallpaperSource`), откуда они взяты: найденные и
    /// указанные вручную, без повторов. Первая — для «Открыть папку».
    pub source_dirs: Vec<PathBuf>,
    /// Найдена ли хоть одна библиотека или папка с обоями.
    pub found: bool,
    /// Что не прочиталось (для журнала): файл и причина.
    pub problems: Vec<String>,
}

impl AxonLibrary {
    pub fn get(&self, id: &str) -> Option<&AxonWallpaper> {
        self.wallpapers.iter().find(|w| w.id == id)
    }
}

/// Прочитать библиотеку из найденных каталогов. Одинаковые обои (тот же
/// `Id` у двух учётных записей или префиксов) — из самой свежей
/// библиотеки, но если там они не скачаны, а в другой скачаны — скачанные.
/// Порядок — как в библиотеке Axon (свежая учётная запись первой).
pub fn read_roots(roots: &[AxonRoot]) -> AxonLibrary {
    let mut accounts: Vec<(SystemTime, &AxonRoot, PathBuf)> = Vec::new();
    for r in roots {
        for acc in account_dirs(&r.root) {
            accounts.push((mtime(&acc.join(LIBRARY_FILE)), r, acc));
        }
    }
    accounts.sort_by(|a, b| b.0.cmp(&a.0));
    let mut lib = AxonLibrary { found: !accounts.is_empty(), ..AxonLibrary::default() };
    for (_, r, acc) in accounts {
        let store = store_dir(&r.prefix, &r.root, &acc);
        if store.is_dir() && !lib.source_dirs.contains(&store) {
            lib.source_dirs.push(store);
        }
        match read_account(&r.prefix, &r.root, &acc) {
            Ok(list) => {
                for w in list {
                    merge(&mut lib.wallpapers, w);
                }
            }
            Err(why) => lib.problems.push(format!("{}: {why}", acc.join(LIBRARY_FILE).display())),
        }
    }
    lib
}

/// Библиотека Axon машины: найденные префиксы плюс папки, указанные вручную.
pub fn load_library(env: &AxonEnv) -> AxonLibrary {
    let mut lib = read_roots(&find_roots(env));
    for dir in &env.source_dirs {
        match read_source_dir(dir) {
            Ok(list) => {
                lib.found = true;
                let dir = check_source_dir(dir).unwrap_or_else(|_| dir.clone());
                if !lib.source_dirs.contains(&dir) {
                    lib.source_dirs.push(dir);
                }
                for w in list {
                    merge(&mut lib.wallpapers, w);
                }
            }
            Err(why) => lib.problems.push(format!("{}: {why}", dir.display())),
        }
    }
    lib
}

// ============================================================================
// Подготовка к показу
// ============================================================================

/// Почему обои не удалось подготовить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxonExtractError {
    /// Подготовить нечем: в этой сборке такой возможности нет.
    Unavailable,
    /// Такой вид обоев подготовка не знает.
    Unsupported(String),
    /// Файлы обоев неполные или испорчены.
    Damaged(String),
    /// Прочая неудача — текст для журнала.
    Failed(String),
}

impl std::fmt::Display for AxonExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AxonExtractError::Unavailable => write!(f, "unavailable"),
            AxonExtractError::Unsupported(s) => write!(f, "unsupported: {s}"),
            AxonExtractError::Damaged(s) => write!(f, "damaged: {s}"),
            AxonExtractError::Failed(s) => write!(f, "failed: {s}"),
        }
    }
}

/// Подготовка скачанных обоев к показу.
///
/// Получает каталог обоев Axon (`WallpaperSource/{Id}{H}{W}`, только
/// чтение) и пустой либо прежний каталог кэша; кладёт туда готовый к
/// показу файл (ролик или картинку) и отдаёт путь к нему. Каталог Axon
/// трогать нельзя.
pub trait AxonExtractor: Send + Sync {
    fn extract(&self, resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, AxonExtractError>;
}

/// Встроенная подготовка (`crate::axon_extract`).
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltinExtractor;

impl AxonExtractor for BuiltinExtractor {
    fn extract(&self, resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, AxonExtractError> {
        use crate::axon_extract::{extract, ExtractError};
        extract(resource_dir, out_dir).map_err(|e| match e {
            ExtractError::Config(m) => AxonExtractError::Damaged(m),
            // Неизвестная схема или не тот пароль — такой вид обоев мы ещё
            // не знаем; прочее (не ZIP, обрыв) — файлы испорчены.
            ExtractError::Format(m) if m.starts_with("unsupported") || m.contains("wrong password") => {
                AxonExtractError::Unsupported(m)
            }
            ExtractError::Format(m) => AxonExtractError::Damaged(m),
            ExtractError::Write(m) => AxonExtractError::Failed(m),
        })
    }
}

/// Чем готовить обои в этой сборке — встроенной подготовкой.
pub fn default_extractor() -> Box<dyn AxonExtractor> {
    Box::new(BuiltinExtractor)
}

/// Как применять готовый файл: формат общего исполнителя обоев
/// (`WallpaperExec::apply_with_audio`). `None` — такой файл обоями не
/// поставить.
pub fn apply_format(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if ext == "gif" {
        return Some("animated");
    }
    if crate::wallpaper_engine::VALID_VIDEO_EXTS.iter().any(|e| e.trim_start_matches('.') == ext) {
        return Some("video");
    }
    match ext.as_str() {
        "gif" => Some("animated"),
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "avif" | "jxl" | "tif" | "tiff" => Some("static"),
        _ => None,
    }
}

/// Готовые к применению обои.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub path: PathBuf,
    /// Формат общего исполнителя: `video`, `static`, `animated`.
    pub format: &'static str,
    /// Звук ролика — как задумано в Axon (`Audible`).
    pub audio: bool,
    pub name: String,
}

/// Отметка кэша: из какого контейнера сделан готовый файл.
fn stamp_of(container: &Path) -> Option<Value> {
    let m = fs::metadata(container).ok()?;
    let secs = m.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    Some(serde_json::json!({
        "container": container.to_string_lossy(),
        "size": m.len(),
        "mtime": secs,
    }))
}

/// Готовый файл из прошлого раза, если контейнер с тех пор не менялся.
fn cached(out_dir: &Path, container: &Path) -> Option<PathBuf> {
    let saved = read_json(&out_dir.join(READY_STAMP)).ok()?;
    let now = stamp_of(container)?;
    for k in ["container", "size", "mtime"] {
        if saved.get(k) != now.get(k) {
            return None;
        }
    }
    let file = out_dir.join(saved.get("file")?.as_str()?);
    (file.is_file() && fs::metadata(&file).map(|m| m.len() > 0).unwrap_or(false)).then_some(file)
}

/// Подготовить обои к применению: отказы без подготовки, кэш, подготовка,
/// проверка ответа. Ошибка — текст для человека.
pub fn prepare(
    w: &AxonWallpaper,
    extractor: Option<&dyn AxonExtractor>,
    cache_root: &Path,
) -> Result<Prepared, String> {
    let name = display_title(w);
    if w.kind == AxonKind::Web {
        return Err(web_unsupported_text(&name));
    }
    let (Some(dir), Some(container)) = (&w.resource_dir, &w.container) else {
        return Err(not_downloaded_text(&name));
    };
    if !w.downloaded {
        return Err(not_downloaded_text(&name));
    }
    let out_dir = cache_root.join(w.cache_key());
    let audio = w.audible && w.kind == AxonKind::Video;
    if let Some(path) = cached(&out_dir, container) {
        if let Some(format) = apply_format(&path) {
            return Ok(Prepared { path, format, audio, name });
        }
    }
    let Some(extractor) = extractor else {
        return Err(unavailable_text());
    };
    fs::create_dir_all(&out_dir).map_err(|e| {
        crate::log_warn!("axon", "каталог кэша {} не создан: {e}", out_dir.display());
        prepare_failed_text(&name)
    })?;
    let path = match extractor.extract(dir, &out_dir) {
        Ok(p) => p,
        Err(e) => {
            crate::log_warn!("axon", "обои {} ({}) не подготовлены: {e}", w.id, dir.display());
            return Err(match e {
                AxonExtractError::Unavailable => unavailable_text(),
                AxonExtractError::Unsupported(_) => kind_unsupported_text(&name),
                AxonExtractError::Damaged(_) => damaged_text(&name),
                AxonExtractError::Failed(_) => prepare_failed_text(&name),
            });
        }
    };
    // Ответу верим, только если файл настоящий и лежит в выданном каталоге.
    let inside = match (path.canonicalize(), out_dir.canonicalize()) {
        (Ok(p), Ok(o)) => p.starts_with(&o) && p.is_file(),
        _ => false,
    };
    if !inside {
        crate::log_warn!("axon", "подготовка вернула негодный путь: {}", path.display());
        return Err(prepare_failed_text(&name));
    }
    let Some(format) = apply_format(&path) else {
        crate::log_warn!("axon", "подготовленный файл неизвестного вида: {}", path.display());
        return Err(kind_unsupported_text(&name));
    };
    if let (Some(mut stamp), Some(file)) = (stamp_of(container), path.file_name()) {
        stamp["file"] = Value::from(file.to_string_lossy().into_owned());
        let _ = fs::write(out_dir.join(READY_STAMP), stamp.to_string());
    }
    Ok(Prepared { path, format, audio, name })
}

// ============================================================================
// Плитки раздела
// ============================================================================

/// Название для показа (без названия — номер).
pub fn display_title(w: &AxonWallpaper) -> String {
    if w.title.trim().is_empty() {
        format!("Razer Axon {}", w.id)
    } else {
        w.title.trim().to_string()
    }
}

/// Строка под названием плитки: вид, звук, разрешение; у нескачанных —
/// что их нет на диске.
pub fn tile_line(w: &AxonWallpaper) -> String {
    let mut parts = vec![w.kind.label()];
    if w.audible && w.kind == AxonKind::Video {
        parts.push(tr("🔊 Sound"));
    }
    if let Some((x, y)) = w.resolution {
        parts.push(format!("{x}×{y}"));
    }
    if !w.downloaded {
        parts.push(tr("not downloaded"));
    }
    parts.join(" · ")
}

/// Отбор по строке поиска: название, автор.
pub fn matches(w: &AxonWallpaper, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || format!("{} {}", w.title, w.author).to_lowercase().contains(&q)
}

// ── Тексты ─────────────────────────────────────────────────────────────

/// Обои ещё не скачаны в Axon.
pub fn not_downloaded_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("“%1” is not downloaded yet. Download it in Razer Axon first.", &[&name])
}

/// Веб-обои Rexon не показывает.
pub fn web_unsupported_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("“%1” is a web page wallpaper — Rexon can't show this kind from Razer Axon yet.", &[&name])
}

/// Подготовить нечем.
pub fn unavailable_text() -> String {
    tr("Razer Axon wallpapers can't be shown in this version of Rexon yet.")
}

/// Вид обоев подготовка не знает.
pub fn kind_unsupported_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("“%1”: this kind of Razer Axon wallpaper is not supported yet.", &[&name])
}

/// Файлы обоев неполные.
pub fn damaged_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("The files of “%1” look incomplete. Download the wallpaper again in Razer Axon.", &[&name])
}

/// Прочая неудача подготовки.
pub fn prepare_failed_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("Could not prepare “%1” for playback.", &[&name])
}

/// Кнопка входа в «Расширениях» — пока для проверки.
pub fn entry_button_text() -> String {
    tr("Razer Axon (test)")
}

/// Заголовок окна библиотеки.
pub fn window_title() -> String {
    tr("Razer Axon wallpapers (test)")
}

/// Подготовка пошла.
pub fn preparing_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a Razer Axon wallpaper.
    trf("Preparing “%1”…", &[&name])
}

/// Пустая библиотека.
pub fn empty_text() -> String {
    tr("The Razer Axon library is empty: add wallpapers in Razer Axon.")
}

/// Библиотеки нет ни в известных местах, ни в указанных папках.
pub fn not_found_text() -> String {
    tr("Razer Axon library not found. If Razer Axon is installed under Wine, \
        choose the folder with its wallpapers (WallpaperSource).")
}

/// Кнопка выбора папки.
pub fn pick_folder_text() -> String {
    tr("📁 Choose Axon wallpaper folder…")
}

/// Подсказка кнопки выбора папки и заголовок окна выбора.
pub fn pick_folder_tooltip() -> String {
    tr("The WallpaperSource folder of Razer Axon: …/AppData/Local/Razer/RazerAxon/WallpaperSource \
        inside the Wine prefix where Razer Axon is installed.")
}

/// Заголовок окна выбора папки.
pub fn pick_folder_title() -> String {
    tr("Folder with Razer Axon wallpapers")
}

/// Кнопка «Открыть папку» у раздела и у плитки.
pub fn open_folder_text() -> String {
    tr("📂 Open folder")
}

/// Подсказка «Открыть папку».
pub fn open_folder_tooltip() -> String {
    tr("Open the folder with these wallpapers in the file manager")
}

/// Выбранной папки нет.
pub fn folder_missing_text() -> String {
    tr("This folder doesn't exist anymore.")
}

/// В выбранной папке нет обоев Axon.
pub fn folder_not_axon_text() -> String {
    tr("No Razer Axon wallpapers in this folder. Choose the WallpaperSource folder \
        (…/AppData/Local/Razer/RazerAxon/WallpaperSource).")
}

/// Папка добавлена.
pub fn folder_added_text(count: usize) -> String {
    // TRANSLATORS: %1 is a number of wallpapers found in the chosen folder.
    trf("Razer Axon folder added: wallpapers found — %1.", &[&count])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_parses() {
        assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
        assert_eq!(parse_resolution(" 1920 X 1080 "), Some((1920, 1080)));
        assert_eq!(parse_resolution("None"), None);
        assert_eq!(parse_resolution("0x10"), None);
    }

    #[test]
    fn kind_parses_both_spellings() {
        assert_eq!(AxonKind::parse("video"), AxonKind::Video);
        assert_eq!(AxonKind::parse("LOCALWEB"), AxonKind::Web);
        assert_eq!(AxonKind::parse("X"), AxonKind::Other("X".into()));
    }

    #[test]
    fn relative_never_escapes() {
        let d = Path::new("/a/b");
        assert_eq!(resource_relative(d, "Resource\\x.mp4"), Some(PathBuf::from("/a/b/Resource/x.mp4")));
        assert_eq!(resource_relative(d, "..\\x.mp4"), None);
        assert_eq!(resource_relative(d, "/etc/passwd"), None);
    }

    #[test]
    fn sanitize_keeps_digits() {
        assert_eq!(sanitize_id("12034"), "12034");
        assert_eq!(sanitize_id("../x"), "___x");
        assert_eq!(sanitize_id(""), "_");
    }

    #[test]
    fn formats_by_extension() {
        assert_eq!(apply_format(Path::new("/c/a.MP4")), Some("video"));
        assert_eq!(apply_format(Path::new("/c/a.jpg")), Some("static"));
        assert_eq!(apply_format(Path::new("/c/a.gif")), Some("animated"));
        assert_eq!(apply_format(Path::new("/c/a.zip")), None);
    }
}
