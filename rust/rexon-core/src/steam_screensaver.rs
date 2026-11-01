//! Заставка Rexon для экрана простоя Steam Big Picture (этап 1 заставки при
//! простое).
//!
//! Steam ищет свои заставки в папках
//! `<Steam>/config/uioverrides/screensavers/<имя>/index.html` и показывает их
//! в «Настройки → Персонализация → Заставка» как «<имя> (User Created)».
//! Страницу Steam открывает в своём браузере, на отдельном хосте; доступа к
//! диску у неё нет, а адреса относительные. Поэтому Rexon кладёт в папку
//! УМЕНЬШЕННЫЕ КОПИИ картинок плоским списком рядом с `index.html` (как
//! устроена встроенная заставка Steam: файлы рядом со страницей) и
//! `manifest.js` с их списком. Скрипт, а не JSON: `<script src>` грузится и
//! в Steam, и в обычном браузере с `file://`, где `fetch` локального JSON
//! запрещён.
//!
//! Что берём: арт недавних игр (`LastPlayed` в `localconfig.vdf`, картинки
//! `appcache/librarycache/<appid>/`, свой арт из `userdata/<id>/config/grid`
//! важнее) и последние скриншоты (`userdata/<id>/760/screenshots.vdf`).
//!
//! ⛔ Выбор заставки в Steam (`screensaver_current_id`) Rexon НЕ трогает:
//! заставку выбирает сам человек в настройках Steam. Чужие папки заставок не
//! трогаются: своя папка опознаётся по файлу-метке [`MARKER_FILE`], без неё
//! папку с нашим именем Rexon не перепишет и не удалит. Логотип Steam Rexon
//! не рисует (товарный знак Valve) — только свой логотип и логотипы игр.
//!
//! Разбор и план — чистые функции; исполнение ([`install`], [`remove`],
//! [`status`]) — в конце модуля.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::i18n::{tr, trf};

/// Имя папки заставки — его Steam и покажет в списке: «Rexon (User Created)».
pub const DIR_NAME: &str = "Rexon";
/// ID, под которым Steam запоминает выбор этой заставки (`uioverride-<имя>`).
/// Только для подсказок: сами мы его никуда не пишем.
pub const STEAM_ID: &str = "uioverride-Rexon";
/// Метка «папку сделал Rexon» + настройки, с которыми её собрали.
pub const MARKER_FILE: &str = "rexon-screensaver.json";
/// Версия раскладки папки.
pub const LAYOUT_VERSION: u32 = 1;

/// Страница, стили и скрипт заставки — в бинаре, чтобы папка собиралась
/// одинаково из дерева и из пакета.
pub const PAGE_HTML: &str = include_str!("../../../data/steam-screensaver/index.html");
pub const PAGE_CSS: &str = include_str!("../../../data/steam-screensaver/screensaver.css");
pub const PAGE_JS: &str = include_str!("../../../data/steam-screensaver/screensaver.js");
/// Логотип Rexon для «прыгающего логотипа».
pub const REXON_LOGO_PNG: &[u8] = include_bytes!("../../../rexon_icon_512.png");
pub const REXON_LOGO_NAME: &str = "rexon-logo.png";

/// Кадр слайд-шоу вписывается в это (Full HD хватает и Деку, и телевизору).
pub const PHOTO_MAX: (u32, u32) = (1920, 1080);
/// Логотип игры поверх кадра.
pub const LOGO_MAX: (u32, u32) = (960, 540);
/// Размытый фон под вертикальной обложкой: мелкий, всё равно размыт.
pub const BACKDROP_MAX: (u32, u32) = (640, 360);
pub const JPEG_QUALITY: u8 = 85;

// ============================================================================
// Настройки
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Арт недавних игр и скриншоты.
    #[default]
    Slideshow,
    /// Прыгающий логотип.
    Bounce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogoChoice {
    /// Логотип Rexon (по умолчанию).
    #[default]
    Rexon,
    /// Логотипы недавних игр по очереди.
    Games,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub mode: Mode,
    pub logo: LogoChoice,
    pub game_art: bool,
    pub screenshots: bool,
    pub clock: bool,
    /// Длина кадра слайд-шоу, секунд.
    pub interval_s: u32,
    pub max_games: usize,
    pub max_screenshots: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Slideshow,
            logo: LogoChoice::Rexon,
            game_art: true,
            screenshots: true,
            clock: true,
            interval_s: 9,
            max_games: 16,
            max_screenshots: 16,
        }
    }
}

// ============================================================================
// Текстовый VDF (KeyValues)
// ============================================================================

/// Узел текстового VDF: строка или вложенный блок (порядок ключей сохранён).
#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Str(String),
    Obj(Vec<(String, Kv)>),
}

impl Kv {
    /// Дочерний узел по ключу без учёта регистра (Steam пишет ключи по-разному).
    pub fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Obj(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::Str(_) => None,
        }
    }
    pub fn path(&self, keys: &[&str]) -> Option<&Kv> {
        keys.iter().try_fold(self, |node, k| node.get(k))
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Kv::Str(s) => Some(s),
            Kv::Obj(_) => None,
        }
    }
    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Kv::str)
    }
    pub fn entries(&self) -> &[(String, Kv)] {
        match self {
            Kv::Obj(items) => items,
            Kv::Str(_) => &[],
        }
    }
}

/// Разбор текстового VDF в корневой блок. Битый хвост не роняет разбор:
/// что успели прочитать — то и возвращаем (файлы Steam пишет сам, но
/// обрезанный при сбое файл не должен ломать всю заставку).
pub fn parse_kv(text: &str) -> Kv {
    let tokens = kv_tokens(text);
    let mut pos = 0;
    Kv::Obj(kv_block(&tokens, &mut pos))
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

fn kv_tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '"' => break,
                        '\\' => match it.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        o => s.push(o),
                    }
                }
                out.push(Tok::Str(s));
            }
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '/' if it.peek() == Some(&'/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                // Ключ без кавычек (встречается в старых файлах).
                let mut s = String::from(c);
                while let Some(&n) = it.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    it.next();
                }
                out.push(Tok::Str(s));
            }
        }
    }
    out
}

fn kv_block(tokens: &[Tok], pos: &mut usize) -> Vec<(String, Kv)> {
    let mut items = Vec::new();
    while *pos < tokens.len() {
        match &tokens[*pos] {
            Tok::Close => {
                *pos += 1;
                return items;
            }
            Tok::Open => {
                // Блок без ключа — пропускаем его содержимое.
                *pos += 1;
                kv_block(tokens, pos);
            }
            Tok::Str(key) => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Tok::Str(v)) => {
                        items.push((key.clone(), Kv::Str(v.clone())));
                        *pos += 1;
                    }
                    Some(Tok::Open) => {
                        *pos += 1;
                        let child = kv_block(tokens, pos);
                        items.push((key.clone(), Kv::Obj(child)));
                    }
                    _ => {}
                }
            }
        }
    }
    items
}

// ============================================================================
// Что лежит у Steam
// ============================================================================

/// Скриншот из описи `760/screenshots.vdf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub appid: String,
    /// Путь относительно `userdata/<id>/760/remote/`.
    pub filename: String,
    /// Время снимка (unix).
    pub creation: i64,
}

/// Опись скриншотов, свежие первыми. Ярлыки не-Steam игр (огромные id) и
/// записи без файла пропускаются не здесь, а при сборке плана — по наличию
/// файла.
pub fn parse_screenshots(text: &str) -> Vec<Shot> {
    let root = parse_kv(text);
    let Some(shots) = root.get("screenshots") else { return Vec::new() };
    let mut out = Vec::new();
    for (appid, block) in shots.entries() {
        if !appid.chars().all(|c| c.is_ascii_digit()) {
            continue; // shortcutnames и прочие служебные блоки
        }
        for (_, item) in block.entries() {
            let Some(filename) = item.str_at("filename") else { continue };
            if filename.is_empty() || filename.contains("..") {
                continue;
            }
            let creation = item.str_at("creation").and_then(|s| s.parse().ok()).unwrap_or(0);
            out.push(Shot { appid: appid.clone(), filename: filename.to_string(), creation });
        }
    }
    out.sort_by(|a, b| b.creation.cmp(&a.creation).then_with(|| a.filename.cmp(&b.filename)));
    out
}

/// Недавно запущенные игры из `localconfig.vdf`: `(appid, LastPlayed)`,
/// свежие первыми; без `LastPlayed` или с нулём — не «недавние».
pub fn parse_last_played(text: &str) -> Vec<(String, i64)> {
    let root = parse_kv(text);
    let Some(apps) = root.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else {
        return Vec::new();
    };
    let mut out: Vec<(String, i64)> = apps
        .entries()
        .iter()
        .filter(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(id, node)| {
            let t: i64 = node.str_at("LastPlayed")?.parse().ok()?;
            (t > 0).then(|| (id.clone(), t))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Пути библиотек из `steamapps/libraryfolders.vdf`.
pub fn parse_library_paths(text: &str) -> Vec<String> {
    let root = parse_kv(text);
    let Some(folders) = root.get("libraryfolders") else { return Vec::new() };
    folders
        .entries()
        .iter()
        .filter_map(|(_, node)| node.str_at("path").map(str::to_string))
        .filter(|p| !p.is_empty())
        .collect()
}

/// Название игры из `appmanifest_<appid>.acf`.
pub fn parse_manifest_name(text: &str) -> Option<String> {
    let root = parse_kv(text);
    let name = root.get("AppState")?.str_at("name")?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Картинки одной игры: имена файлов в её папке кэша или полные пути.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Art {
    pub hero: Option<String>,
    pub logo: Option<String>,
    pub portrait: Option<String>,
    pub header: Option<String>,
}

impl Art {
    pub fn is_empty(&self) -> bool {
        self.hero.is_none() && self.portrait.is_none() && self.header.is_none()
    }
}

/// Выбор картинок из листинга `appcache/librarycache/<appid>/`: сначала
/// вариант на языке Steam (`*_russian.*`), потом базовый. Размытая копия hero
/// (`library_hero_blur`) героем не считается.
pub fn pick_cache_art(names: &[String], steam_lang: &str) -> Art {
    let set: HashSet<&str> = names.iter().map(String::as_str).collect();
    let find = |stems: &[&str], exts: &[&str]| -> Option<String> {
        for stem in stems {
            let mut variants = Vec::new();
            if !steam_lang.is_empty() && steam_lang != "english" {
                variants.push(format!("{stem}_{steam_lang}"));
            }
            variants.push((*stem).to_string());
            for v in &variants {
                for ext in exts {
                    let name = format!("{v}.{ext}");
                    if set.contains(name.as_str()) {
                        return Some(name);
                    }
                }
            }
        }
        None
    };
    Art {
        hero: find(&["library_hero"], &["jpg", "png"]),
        logo: find(&["logo"], &["png"]),
        portrait: find(&["library_600x900", "library_capsule"], &["jpg", "png"]),
        header: find(&["header", "library_header"], &["jpg", "png"]),
    }
}

/// Свой арт из `userdata/<id>/config/grid` (кнопка «Изменить картинку» в
/// библиотеке): `<appid>_hero.*`, `<appid>_logo.*`, `<appid>p.*`, `<appid>.*`.
pub fn pick_grid_art(names: &[String], appid: &str) -> Art {
    let find = |stem: String| -> Option<String> {
        ["png", "jpg", "jpeg", "webp"]
            .iter()
            .map(|ext| format!("{stem}.{ext}"))
            .find(|n| names.iter().any(|x| x == n))
    };
    Art {
        hero: find(format!("{appid}_hero")),
        logo: find(format!("{appid}_logo")),
        portrait: find(format!("{appid}p")),
        header: find(appid.to_string()),
    }
}

/// Свой арт поверх кэша: что задал человек, то и берём.
pub fn merge_art(grid: Art, cache: Art) -> Art {
    Art {
        hero: grid.hero.or(cache.hero),
        logo: grid.logo.or(cache.logo),
        portrait: grid.portrait.or(cache.portrait),
        header: grid.header.or(cache.header),
    }
}

// ============================================================================
// План: что показать и куда скопировать
// ============================================================================

/// Как уменьшать файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Кадр: вписать в [`PHOTO_MAX`], JPEG.
    Photo,
    /// Логотип: вписать в [`LOGO_MAX`], PNG с прозрачностью.
    Logo,
    /// Размытый фон под обложкой: [`BACKDROP_MAX`], размыть, JPEG.
    Backdrop,
}

/// Одна копия: откуда, под каким именем в папке заставки и как ужать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub src: PathBuf,
    pub name: String,
    pub fit: Fit,
}

/// Игра-кандидат: найденный арт с полными путями.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameArt {
    pub appid: String,
    pub title: String,
    pub art: Art,
}

/// Скриншот с полным путём.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotFile {
    pub appid: String,
    pub title: String,
    pub path: PathBuf,
    pub creation: i64,
}

/// Итог плана: список копий и манифест для страницы.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub copies: Vec<Copy>,
    pub manifest: Value,
}

/// Собрать план из найденного. Кадры идут вперемешку: арт игры, скриншот,
/// арт игры… — пока не кончится одно, потом остаток другого.
pub fn build_plan(games: &[GameArt], shots: &[ShotFile], opts: &Options, locale: &str) -> Plan {
    let mut copies: Vec<Copy> = Vec::new();
    let mut by_src: HashMap<(PathBuf, &'static str), String> = HashMap::new();
    let mut add = |copies: &mut Vec<Copy>, src: &Path, fit: Fit| -> String {
        let tag = match fit {
            Fit::Photo => "img",
            Fit::Logo => "logo",
            Fit::Backdrop => "blur",
        };
        if let Some(name) = by_src.get(&(src.to_path_buf(), tag)) {
            return name.clone();
        }
        let ext = if fit == Fit::Logo { "png" } else { "jpg" };
        let name = format!("{tag}-{:03}.{ext}", copies.iter().filter(|c| c.fit == fit).count() + 1);
        copies.push(Copy { src: src.to_path_buf(), name: name.clone(), fit });
        by_src.insert((src.to_path_buf(), tag), name.clone());
        name
    };

    let mut art_slides = Vec::new();
    let mut logos = Vec::new();
    for g in games.iter().take(opts.max_games) {
        let logo = g.art.logo.as_ref().map(|p| add(&mut copies, Path::new(p), Fit::Logo));
        if let Some(l) = &logo {
            logos.push(l.clone());
        }
        if !opts.game_art {
            continue;
        }
        let slide = if let Some(hero) = &g.art.hero {
            json!({"kind": "art", "appid": g.appid, "title": g.title,
                   "image": add(&mut copies, Path::new(hero), Fit::Photo),
                   "logo": logo, "framed": false})
        } else if let Some(cover) = g.art.portrait.as_ref().or(g.art.header.as_ref()) {
            json!({"kind": "art", "appid": g.appid, "title": g.title,
                   "image": add(&mut copies, Path::new(cover), Fit::Photo),
                   "backdrop": add(&mut copies, Path::new(cover), Fit::Backdrop),
                   "logo": Value::Null, "framed": true})
        } else {
            continue;
        };
        art_slides.push(slide);
    }

    let mut shot_slides = Vec::new();
    if opts.screenshots {
        for s in shots.iter().take(opts.max_screenshots) {
            shot_slides.push(json!({"kind": "shot", "appid": s.appid, "title": s.title,
                "image": add(&mut copies, &s.path, Fit::Photo), "when": s.creation}));
        }
    }

    let mut slides = Vec::new();
    let (mut a, mut b) = (art_slides.into_iter(), shot_slides.into_iter());
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() {
            break;
        }
        slides.extend(x);
        slides.extend(y);
    }

    let manifest = json!({
        "version": LAYOUT_VERSION,
        "mode": opts.mode,
        "logo": opts.logo,
        "clock": opts.clock,
        "interval": opts.interval_s.clamp(4, 60),
        "locale": locale,
        "rexonLogo": REXON_LOGO_NAME,
        "logos": logos,
        "slides": slides,
    });
    Plan { copies, manifest }
}

/// Текст `manifest.js`. `</` экранируется, чтобы название игры не закрыло
/// тег скрипта.
pub fn manifest_js(manifest: &Value) -> String {
    let body = serde_json::to_string_pretty(manifest).unwrap_or_else(|_| "{}".into());
    format!(
        "// Создано Rexon. Набор картинок заставки — обновляется из Rexon.\n\
         window.REXON_SCREENSAVER = {};\n",
        body.replace("</", "<\\/")
    )
}

/// Язык интерфейса → тег для `Intl` страницы (`ru_RU.UTF-8` → `ru-RU`).
pub fn page_locale(ui_language: &str) -> String {
    let base = ui_language.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    if base.is_empty() || base == "C" || base == "POSIX" {
        "en".into()
    } else {
        base
    }
}

/// Папка заставок Steam в корне `root`.
pub fn screensavers_dir(root: &Path) -> PathBuf {
    root.join("config").join("uioverrides").join("screensavers")
}

/// Папка заставки Rexon.
pub fn target_dir(root: &Path) -> PathBuf {
    screensavers_dir(root).join(DIR_NAME)
}

/// Корни Steam, куда можно поставить заставку: обычный и Flatpak (общий
/// список [`crate::steam_token::steam_roots`], уже без дублей по realpath) —
/// только настоящие установки клиента, где есть `config/`.
pub fn steam_roots(home: &str) -> Vec<PathBuf> {
    crate::steam_token::steam_roots(home).into_iter().filter(|r| r.join("config").is_dir()).collect()
}

// ============================================================================
// Исполнение
// ============================================================================

/// Метка папки: кто собрал, с какими настройками, что создали попутно.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Marker {
    pub owner: String,
    pub layout: u32,
    pub options: Options,
    pub slides: usize,
    pub updated: i64,
    /// Папки, которых не было до Rexon (`uioverrides`, `screensavers`):
    /// при удалении убираем их, если они пусты.
    pub created_dirs: Vec<String>,
}

/// Состояние в одном корне Steam.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub root: String,
    pub installed: bool,
    /// Папка с нашим именем есть, но её сделал не Rexon — не трогаем.
    pub foreign: bool,
    pub slides: usize,
    pub updated: i64,
    pub options: Options,
}

fn read_marker(dir: &Path) -> Option<Marker> {
    let text = std::fs::read_to_string(dir.join(MARKER_FILE)).ok()?;
    let m: Marker = serde_json::from_str(&text).ok()?;
    (m.owner == "rexon").then_some(m)
}

pub fn status(root: &Path) -> Status {
    let dir = target_dir(root);
    let marker = read_marker(&dir);
    Status {
        root: root.display().to_string(),
        installed: marker.is_some(),
        foreign: marker.is_none() && dir.exists(),
        slides: marker.as_ref().map_or(0, |m| m.slides),
        updated: marker.as_ref().map_or(0, |m| m.updated),
        options: marker.map(|m| m.options).unwrap_or_default(),
    }
}

fn list_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// Аккаунт Steam (папка `userdata/<accountid>`) — тот, чей `localconfig.vdf`
/// свежее; выбор тот же, что у вкладки ([`crate::qt_steam::pick_account`]).
pub fn active_account(root: &Path) -> Option<String> {
    let userdata = root.join("userdata");
    let entries: Vec<(String, f64)> = list_names(&userdata)
        .into_iter()
        .filter_map(|id| {
            let m = std::fs::metadata(userdata.join(&id).join("config/localconfig.vdf")).ok()?;
            let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
            Some((id, t))
        })
        .collect();
    let id64: i64 = crate::qt_steam::pick_account(&entries)?.parse().ok()?;
    Some((id64 - crate::qt_steam::STEAMID64_BASE).to_string())
}

/// Названия игр по манифестам всех библиотек.
fn game_titles(root: &Path) -> HashMap<String, String> {
    let mut libs = vec![root.display().to_string()];
    if let Ok(t) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        libs.extend(parse_library_paths(&t));
    }
    let mut out = HashMap::new();
    let mut seen = HashSet::new();
    for lib in libs {
        let dir = Path::new(&lib).join("steamapps");
        if !seen.insert(dir.clone()) {
            continue;
        }
        for name in list_names(&dir) {
            let Some(id) = name.strip_prefix("appmanifest_").and_then(|n| n.strip_suffix(".acf")) else {
                continue;
            };
            if let Some(title) = std::fs::read_to_string(dir.join(&name)).ok().and_then(|t| parse_manifest_name(&t)) {
                out.insert(id.to_string(), title);
            }
        }
    }
    out
}

/// Собрать кандидатов с диска Steam (только чтение).
pub fn collect(root: &Path, steam_lang: &str) -> (Vec<GameArt>, Vec<ShotFile>) {
    let Some(account) = active_account(root) else { return (Vec::new(), Vec::new()) };
    let user = root.join("userdata").join(&account);
    let titles = game_titles(root);
    let title_of = |id: &str| titles.get(id).cloned().unwrap_or_default();

    let grid_dir = user.join("config/grid");
    let grid_names = list_names(&grid_dir);
    let cache = root.join("appcache/librarycache");
    let recent = std::fs::read_to_string(user.join("config/localconfig.vdf"))
        .map(|t| parse_last_played(&t))
        .unwrap_or_default();
    let mut games = Vec::new();
    for (appid, _) in recent {
        let cdir = cache.join(&appid);
        let full = |base: &Path, n: Option<String>| n.map(|n| base.join(n).display().to_string());
        let c = pick_cache_art(&list_names(&cdir), steam_lang);
        let g = pick_grid_art(&grid_names, &appid);
        let cache_art = Art {
            hero: full(&cdir, c.hero),
            logo: full(&cdir, c.logo),
            portrait: full(&cdir, c.portrait),
            header: full(&cdir, c.header),
        };
        let grid_art = Art {
            hero: full(&grid_dir, g.hero),
            logo: full(&grid_dir, g.logo),
            portrait: full(&grid_dir, g.portrait),
            header: full(&grid_dir, g.header),
        };
        let art = merge_art(grid_art, cache_art);
        if art.is_empty() {
            continue; // инструменты и рантаймы без арта
        }
        games.push(GameArt { title: title_of(&appid), appid, art });
    }

    let remote = user.join("760/remote");
    let shots = std::fs::read_to_string(user.join("760/screenshots.vdf"))
        .map(|t| parse_screenshots(&t))
        .unwrap_or_default()
        .into_iter()
        .map(|s| ShotFile { title: title_of(&s.appid), path: remote.join(&s.filename), appid: s.appid, creation: s.creation })
        .filter(|s| s.path.is_file())
        .collect();
    (games, shots)
}

/// Уменьшить картинку под заставку.
pub fn write_fitted(src: &Path, out: &Path, fit: Fit) -> Result<(), String> {
    let img = image::ImageReader::open(src)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let (mw, mh) = match fit {
        Fit::Photo => PHOTO_MAX,
        Fit::Logo => LOGO_MAX,
        Fit::Backdrop => BACKDROP_MAX,
    };
    let img = if img.width() > mw || img.height() > mh {
        img.resize(mw, mh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    match fit {
        Fit::Logo => img.to_rgba8().write_to(&mut w, image::ImageFormat::Png).map_err(|e| e.to_string()),
        Fit::Photo | Fit::Backdrop => {
            let rgb = if fit == Fit::Backdrop { img.blur(18.0).to_rgb8() } else { img.to_rgb8() };
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, JPEG_QUALITY);
            rgb.write_with_encoder(enc).map_err(|e| e.to_string())
        }
    }
}

/// Итог установки в одном корне.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstallReport {
    pub root: String,
    pub dir: String,
    pub slides: usize,
    pub skipped: usize,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Записать готовую папку заставки (страница, логотип, копии, манифест,
/// метка) в `dir`, которой ещё нет. Кадры, чьи картинки не открылись,
/// из манифеста выкидываются. Возвращает (кадров, пропущено).
pub fn write_folder(dir: &Path, plan: &Plan, marker: &Marker) -> Result<(usize, usize), String> {
    std::fs::create_dir_all(dir).map_err(|e| trf("couldn't create %1: %2", &[&dir.display(), &e]))?;
    let put = |name: &str, data: &[u8]| {
        std::fs::write(dir.join(name), data).map_err(|e| trf("couldn't write %1: %2", &[&name, &e]))
    };
    put("index.html", PAGE_HTML.as_bytes())?;
    put("screensaver.css", PAGE_CSS.as_bytes())?;
    put("screensaver.js", PAGE_JS.as_bytes())?;
    put(REXON_LOGO_NAME, REXON_LOGO_PNG)?;

    let mut failed: HashSet<String> = HashSet::new();
    for c in &plan.copies {
        if write_fitted(&c.src, &dir.join(&c.name), c.fit).is_err() {
            let _ = std::fs::remove_file(dir.join(&c.name));
            failed.insert(c.name.clone());
        }
    }
    let mut manifest = plan.manifest.clone();
    let mut skipped = 0;
    if let Some(slides) = manifest.get_mut("slides").and_then(Value::as_array_mut) {
        let before = slides.len();
        slides.retain(|s| {
            ["image", "backdrop"].iter().all(|k| s.get(*k).and_then(Value::as_str).is_none_or(|n| !failed.contains(n)))
        });
        for s in slides.iter_mut() {
            if s.get("logo").and_then(Value::as_str).is_some_and(|n| failed.contains(n)) {
                s["logo"] = Value::Null;
            }
        }
        skipped = before - slides.len();
    }
    if let Some(logos) = manifest.get_mut("logos").and_then(Value::as_array_mut) {
        logos.retain(|l| l.as_str().is_some_and(|n| !failed.contains(n)));
    }
    let slides = manifest.get("slides").and_then(Value::as_array).map_or(0, Vec::len);
    put("manifest.js", manifest_js(&manifest).as_bytes())?;
    let marker = Marker { slides, ..marker.clone() };
    let text = serde_json::to_string_pretty(&marker).unwrap_or_default();
    put(MARKER_FILE, text.as_bytes())?;
    Ok((slides, skipped))
}

/// Поставить или обновить заставку в корне Steam `root`.
///
/// Папка собирается рядом во временной и подменяется целиком: Steam,
/// открывший заставку в эту минуту, не увидит полупустую папку. Чужую папку
/// с нашим именем (без метки) не трогаем.
pub fn install(root: &Path, opts: &Options, steam_lang: &str, locale: &str) -> Result<InstallReport, String> {
    let parent = screensavers_dir(root);
    let target = target_dir(root);
    let old_marker = read_marker(&target);
    if target.exists() && old_marker.is_none() {
        return Err(trf(
            "The folder %1 was not made by Rexon — it was left untouched.",
            &[&target.display()],
        ));
    }
    // Какие папки создаём впервые — чтобы при удалении вернуть как было.
    let mut created_dirs = old_marker.as_ref().map(|m| m.created_dirs.clone()).unwrap_or_default();
    if old_marker.is_none() {
        for d in [root.join("config/uioverrides"), parent.clone()] {
            if !d.exists() {
                created_dirs.push(d.display().to_string());
            }
        }
    }
    std::fs::create_dir_all(&parent).map_err(|e| trf("couldn't create %1: %2", &[&parent.display(), &e]))?;

    let (games, shots) = collect(root, steam_lang);
    let plan = build_plan(&games, &shots, opts, locale);
    let pid = std::process::id();
    let staging = parent.join(format!(".{DIR_NAME}.new-{pid}"));
    let _ = std::fs::remove_dir_all(&staging);
    let marker = Marker {
        owner: "rexon".into(),
        layout: LAYOUT_VERSION,
        options: opts.clone(),
        slides: 0,
        updated: now_unix(),
        created_dirs,
    };
    let (slides, skipped) = match write_folder(&staging, &plan, &marker) {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let old = parent.join(format!(".{DIR_NAME}.old-{pid}"));
    if target.exists() {
        std::fs::rename(&target, &old).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            trf("couldn't replace %1: %2", &[&target.display(), &e])
        })?;
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        // Вернуть прежнюю папку на место.
        let _ = std::fs::rename(&old, &target);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(trf("couldn't replace %1: %2", &[&target.display(), &e]));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(InstallReport { root: root.display().to_string(), dir: target.display().to_string(), slides, skipped })
}

/// Удалить заставку Rexon из корня `root`. `Ok(false)` — её там не было.
/// Папки `uioverrides`/`screensavers`, созданные Rexon, убираются, только
/// если в них больше ничего нет.
pub fn remove(root: &Path) -> Result<bool, String> {
    let target = target_dir(root);
    let Some(marker) = read_marker(&target) else {
        if target.exists() {
            return Err(trf(
                "The folder %1 was not made by Rexon — it was left untouched.",
                &[&target.display()],
            ));
        }
        return Ok(false);
    };
    std::fs::remove_dir_all(&target).map_err(|e| trf("couldn't remove %1: %2", &[&target.display(), &e]))?;
    // Сначала вложенная (screensavers), потом uioverrides; remove_dir не
    // тронет непустую папку.
    let mut dirs = marker.created_dirs.clone();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for d in dirs {
        let p = PathBuf::from(&d);
        if p.starts_with(root.join("config")) {
            let _ = std::fs::remove_dir(p);
        }
    }
    Ok(true)
}

/// Подсказка человеку: где выбрать заставку в Steam.
pub fn how_to_select() -> String {
    tr("In Steam Big Picture open Settings → Customization → Screensaver and choose «Rexon (User Created)». \
        Steam keeps your choice; Rexon never changes it.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    const SHOTS_VDF: &str = r#""screenshots"
{
	"4734170"
	{
		"0"
		{
			"type"		"1"
			"filename"		"4734170/screenshots/20260623225805_1.jpg"
			"creation"		"1782248285"
		}
		"1"
		{
			"filename"		"4734170/screenshots/20260623225350_1.jpg"
			"creation"		"1782248030"
		}
	}
	"20"
	{
		"0" { "filename" "20/screenshots/a.jpg" "creation" "1782248300" }
		"1" { "filename" "../etc/passwd" "creation" "1" }
	}
	"shortcutnames"
	{
		"12375318956259409920"		"EEKeeper.exe"
	}
}"#;

    #[test]
    fn screenshots_newest_first_and_safe() {
        let v = parse_screenshots(SHOTS_VDF);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].filename, "20/screenshots/a.jpg");
        assert_eq!(v[1].creation, 1782248285);
        assert!(v.iter().all(|s| !s.filename.contains("..")));
    }

    #[test]
    fn last_played_from_localconfig() {
        let text = r#""UserLocalConfigStore"
{
	"Software" { "Valve" { "steam" { "apps" {
		"20" { "LastPlayed" "1474676422" "Playtime" "1" }
		"70" { "LastPlayed" "1590152417" }
		"80" { "Playtime" "5" }
		"cloud" { "LastPlayed" "9" }
		"90" { "LastPlayed" "0" }
	} } } }
}"#;
        assert_eq!(
            parse_last_played(text),
            vec![("70".to_string(), 1590152417), ("20".to_string(), 1474676422)]
        );
    }

    #[test]
    fn kv_escapes_comments_and_broken_tail() {
        let k = parse_kv("// c\n\"a\" { \"b\" \"x\\\"y\" key value } \"c\" { \"d\"");
        assert_eq!(k.path(&["a", "b"]).and_then(Kv::str), Some("x\"y"));
        assert_eq!(k.path(&["a", "key"]).and_then(Kv::str), Some("value"));
        assert!(k.get("c").is_some());
    }

    #[test]
    fn library_paths_and_manifest_name() {
        let lf = "\"libraryfolders\" { \"0\" { \"path\" \"/home/u/.local/share/Steam\" } \"1\" { \"path\" \"/mnt/g\" } }";
        assert_eq!(parse_library_paths(lf), s(&["/home/u/.local/share/Steam", "/mnt/g"]));
        assert_eq!(
            parse_manifest_name("\"AppState\" { \"appid\" \"70\" \"name\" \"Half-Life\" }").as_deref(),
            Some("Half-Life")
        );
    }

    #[test]
    fn cache_art_prefers_language_and_skips_blur() {
        let names = s(&["header.jpg", "library_hero_blur.jpg", "logo.png", "logo_russian.png", "library_600x900.jpg"]);
        let a = pick_cache_art(&names, "russian");
        assert_eq!(a.hero, None);
        assert_eq!(a.logo.as_deref(), Some("logo_russian.png"));
        assert_eq!(a.portrait.as_deref(), Some("library_600x900.jpg"));
        assert_eq!(pick_cache_art(&names, "english").logo.as_deref(), Some("logo.png"));
    }

    #[test]
    fn grid_art_wins_over_cache() {
        let g = pick_grid_art(&s(&["70_hero.png", "70p.jpg", "80_hero.png"]), "70");
        let c = Art { hero: Some("h.jpg".into()), logo: Some("l.png".into()), ..Art::default() };
        let m = merge_art(g, c);
        assert_eq!(m.hero.as_deref(), Some("70_hero.png"));
        assert_eq!(m.logo.as_deref(), Some("l.png"));
        assert_eq!(m.portrait.as_deref(), Some("70p.jpg"));
    }

    fn game(id: &str, hero: bool) -> GameArt {
        GameArt {
            appid: id.into(),
            title: format!("Game {id}"),
            art: Art {
                hero: hero.then(|| format!("/c/{id}/library_hero.jpg")),
                logo: Some(format!("/c/{id}/logo.png")),
                portrait: Some(format!("/c/{id}/library_600x900.jpg")),
                header: None,
            },
        }
    }

    #[test]
    fn plan_interleaves_and_names_flat_files() {
        let shots = vec![ShotFile { appid: "70".into(), title: "Game 70".into(), path: "/s/1.jpg".into(), creation: 5 }];
        let p = build_plan(&[game("70", true), game("20", false)], &shots, &Options::default(), "ru-RU");
        let slides = p.manifest["slides"].as_array().unwrap();
        let kinds: Vec<&str> = slides.iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["art", "shot", "art"]);
        assert_eq!(slides[0]["logo"], "logo-001.png");
        assert_eq!(slides[2]["framed"], true);
        assert_eq!(slides[2]["backdrop"], "blur-001.jpg");
        // Все копии плоско рядом со страницей, без подпапок и выхода наружу.
        assert!(p.copies.iter().all(|c| !c.name.contains('/') && !c.name.starts_with('.')));
        assert_eq!(p.manifest["logos"].as_array().unwrap().len(), 2);
        assert_eq!(p.manifest["mode"], "slideshow");
        assert_eq!(p.manifest["logo"], "rexon");
    }

    #[test]
    fn plan_respects_switches_and_limits() {
        let opts = Options { game_art: false, screenshots: false, mode: Mode::Bounce, logo: LogoChoice::Games, ..Options::default() };
        let p = build_plan(&[game("70", true)], &[], &opts, "en");
        assert!(p.manifest["slides"].as_array().unwrap().is_empty());
        // Логотипы игр нужны прыгающему логотипу и без арта в слайд-шоу.
        assert_eq!(p.manifest["logos"].as_array().unwrap().len(), 1);
        let many: Vec<GameArt> = (0..40).map(|i| game(&i.to_string(), true)).collect();
        let p = build_plan(&many, &[], &Options::default(), "en");
        assert_eq!(p.manifest["slides"].as_array().unwrap().len(), Options::default().max_games);
    }

    #[test]
    fn manifest_js_cannot_close_script() {
        let js = manifest_js(&json!({"slides": [{"title": "</script><b>"}]}));
        assert!(js.starts_with("// "));
        assert!(js.contains("window.REXON_SCREENSAVER = {"));
        assert!(!js.contains("</script>"));
    }

    #[test]
    fn locale_tag() {
        assert_eq!(page_locale("ru_RU.UTF-8"), "ru-RU");
        assert_eq!(page_locale("C"), "en");
        assert_eq!(page_locale("de"), "de");
    }

    #[test]
    fn paths_follow_steam_layout() {
        let r = Path::new("/h/.local/share/Steam");
        assert_eq!(target_dir(r), Path::new("/h/.local/share/Steam/config/uioverrides/screensavers/Rexon"));
        assert_eq!(STEAM_ID, format!("uioverride-{DIR_NAME}"));
    }

    #[test]
    fn page_is_offline_and_relative() {
        for text in [PAGE_HTML, PAGE_CSS, PAGE_JS] {
            assert!(!text.contains("http://") && !text.contains("https://"), "внешний адрес в странице");
        }
        assert!(PAGE_HTML.contains("src=\"manifest.js\""));
        assert!(PAGE_HTML.contains("src=\"screensaver.js\""));
        assert!(REXON_LOGO_PNG.starts_with(b"\x89PNG"));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-ss-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Мини-Steam: аккаунт, недавняя игра с артом, скриншот.
    fn fake_steam(name: &str) -> PathBuf {
        let root = scratch(name);
        let user = root.join("userdata/1234");
        std::fs::create_dir_all(user.join("config")).unwrap();
        std::fs::write(
            user.join("config/localconfig.vdf"),
            "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" { \"70\" { \"LastPlayed\" \"100\" } } } } } }",
        )
        .unwrap();
        let cache = root.join("appcache/librarycache/70");
        std::fs::create_dir_all(&cache).unwrap();
        image::RgbImage::from_pixel(3000, 1000, image::Rgb([200, 20, 20])).save(cache.join("library_hero.jpg")).unwrap();
        image::RgbaImage::from_pixel(64, 32, image::Rgba([255, 255, 255, 128])).save(cache.join("logo.png")).unwrap();
        std::fs::create_dir_all(user.join("760/remote/70/screenshots")).unwrap();
        image::RgbImage::from_pixel(64, 36, image::Rgb([0, 90, 200])).save(user.join("760/remote/70/screenshots/a.jpg")).unwrap();
        std::fs::write(
            user.join("760/screenshots.vdf"),
            "\"screenshots\" { \"70\" { \"0\" { \"filename\" \"70/screenshots/a.jpg\" \"creation\" \"5\" } \"1\" { \"filename\" \"70/screenshots/missing.jpg\" \"creation\" \"6\" } } }",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps/appmanifest_70.acf"), "\"AppState\" { \"name\" \"Half-Life\" }").unwrap();
        root
    }

    #[test]
    fn install_update_remove_roundtrip() {
        let root = fake_steam("roundtrip");
        // Чужая заставка рядом — не трогается.
        let other = screensavers_dir(&root).join("Other");
        assert!(!root.join("config").exists());
        let r = install(&root, &Options::default(), "english", "en").unwrap();
        assert_eq!(r.slides, 2);
        let dir = target_dir(&root);
        for f in ["index.html", "screensaver.js", "screensaver.css", "manifest.js", REXON_LOGO_NAME, MARKER_FILE] {
            assert!(dir.join(f).is_file(), "{f}");
        }
        let hero = image::open(dir.join("img-001.jpg")).unwrap();
        assert!(hero.width() <= PHOTO_MAX.0 && hero.height() <= PHOTO_MAX.1);
        let js = std::fs::read_to_string(dir.join("manifest.js")).unwrap();
        assert!(js.contains("Half-Life") && !js.contains(&root.display().to_string()), "в манифесте только свои имена");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("index.html"), "x").unwrap();

        // Обновление: настройки меняются, папка подменяется целиком.
        let opts = Options { mode: Mode::Bounce, ..Options::default() };
        install(&root, &opts, "english", "en").unwrap();
        let st = status(&root);
        assert!(st.installed && !st.foreign);
        assert_eq!(st.options.mode, Mode::Bounce);
        assert!(list_names(&screensavers_dir(&root)).iter().all(|n| !n.starts_with('.')), "без хвостов временных папок");

        assert!(remove(&root).unwrap());
        assert!(!dir.exists());
        assert!(other.join("index.html").is_file(), "чужая заставка цела");
        // screensavers не пуста (Other) — осталась.
        assert!(screensavers_dir(&root).is_dir());
        assert!(!remove(&root).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_restores_missing_parents() {
        let root = fake_steam("parents");
        install(&root, &Options::default(), "english", "en").unwrap();
        assert!(remove(&root).unwrap());
        assert!(!root.join("config/uioverrides").exists(), "созданные Rexon папки убраны");
        assert!(root.join("config").is_dir() || !root.join("config").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn foreign_folder_with_our_name_is_left_alone() {
        let root = fake_steam("foreign");
        let dir = target_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "theirs").unwrap();
        assert!(install(&root, &Options::default(), "english", "en").is_err());
        assert!(remove(&root).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("index.html")).unwrap(), "theirs");
        assert!(status(&root).foreign);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_image_drops_only_its_slide() {
        let root = fake_steam("broken");
        std::fs::write(root.join("appcache/librarycache/70/library_hero.jpg"), b"not an image").unwrap();
        let r = install(&root, &Options::default(), "english", "en").unwrap();
        assert_eq!((r.slides, r.skipped), (1, 1));
        let _ = std::fs::remove_dir_all(&root);
    }
}
