//! Виджеты Conky, которые лежат на диске не там, куда их кладёт Rexon.
//!
//! ЗАЧЕМ. Rexon ставит виджеты в `~/.config/conky/<имя>/`, и раздел «Виджеты
//! Conky» видел только их. А виджеты, поставленные руками, архивом с форума
//! или чужой программой, лежат в `~/.conky/`, `~/.Conky/`,
//! `~/.config/Conky/`, одним файлом `~/.conkyrc` или прямо в
//! `~/.config/conky/conky.conf` (его conky читает сам, без `-c`). Человек,
//! пришедший из другой программы, не находил своих виджетов и ставил её
//! обратно. Теперь эти места просматриваются, и раздел показывает плитки
//! «Найдено на диске» — запустить, остановить, включить при входе.
//!
//! Что считается виджетом — решает СОДЕРЖИМОЕ, а не имя: файл без двоичных
//! байтов, где есть `conky.config`, `conky.text` (синтаксис 1.10) либо
//! строка `TEXT` (1.9) ([`looks_like_config`]). Имя у настоящего виджета
//! бывает любым (`Gotham`, `Jhalak.conf`, `conkyrc_seamod`), а рядом лежат
//! файлы с похожими именами, которые виджетами не являются (`cava.conf`
//! визуализатора у Jhalak). `conky.config` не обязан стоять первой строкой:
//! перед ним бывает шапка комментариев. Сценарии Lua, картинки, шрифты и
//! архивы не читаются вовсе — по окончанию имени ([`skipped_name`]).
//!
//! Обход ограничен глубиной [`MAX_DEPTH`] и размером файла
//! [`MAX_CONFIG_BYTES`]: в темах лежат каталоги шрифтов и картинок, и
//! рекурсивный поиск без границ делал открытие раздела медленным.
//!
//! Номер найденной плитки — путь к каталогу темы (или к файлу, если виджет —
//! один файл): `local:conky:/home/…/.conky/Gotham`. По нему кнопки
//! [`crate::de_apply`] находят файл настроек и процессы виджета, а
//! «Запускать при входе» — свой файл автозапуска.
//!
//! Слежка за этими местами — [`Watch`] (inotify, без опроса): новый виджет,
//! распакованный в `~/.conky`, появляется в разделе сам.

use std::path::Path;

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{tr, trf};
use crate::models::ThemeItem;

/// Насколько глубоко искать файлы настроек в каталоге темы (сам каталог —
/// первый уровень): `тема/файл`, `тема/вариант/файл`, `тема/a/b/файл`.
pub const MAX_DEPTH: usize = 3;

/// Файл больше этого — не файл настроек (журнал, снимок, данные).
pub const MAX_CONFIG_BYTES: usize = 256 * 1024;

/// Ключ настроек Rexon: свои папки с виджетами (список путей).
pub const EXTRA_DIRS_KEY: &str = "conky_extra_dirs";

/// Окончания имён, которые файлом настроек не бывают: сценарии (Lua
/// подключается из настроек, а не запускается сам — conky-manager #18),
/// картинки, шрифты, архивы, документы.
const SKIPPED_EXTS: &[&str] = &[
    "lua", "sh", "py", "pl", "rb", "js", "png", "jpg", "jpeg", "gif", "svg", "svgz", "xpm",
    "bmp", "webp", "ico", "tif", "tiff", "ttf", "otf", "pcf", "woff", "woff2", "bdf", "zip",
    "tar", "gz", "tgz", "xz", "bz2", "zst", "7z", "rar", "deb", "rpm", "md", "rtf", "pdf",
    "html", "htm", "css", "json", "xml", "desktop", "cmtheme", "log", "so", "mp3", "ogg",
    "wav", "mp4", "webm", "mkv",
];

/// Не читать ли файл по имени: окончание из [`SKIPPED_EXTS`], копия
/// редактора (`conky.conf~`, `.bak`, `.swp`), служебный `.DS_Store` из
/// архивов, собранных на macOS.
pub fn skipped_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == ".ds_store" || lower.ends_with('~') || lower.ends_with(".bak") || lower.ends_with(".swp") || lower.ends_with(".orig") {
        return true;
    }
    match lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => SKIPPED_EXTS.contains(&ext),
        _ => false,
    }
}

/// Файл ли настроек Conky этот текст. Признаки — по строкам, без учёта
/// отступов и комментариев (`--` у 1.10, `#` у 1.9):
/// * `conky.config` или `conky.text` перед `=` — синтаксис 1.10;
/// * строка `TEXT` — граница настроек и текста у 1.9.
///
/// Двоичный байт (NUL) — не текст вовсе.
pub fn looks_like_config(text: &str) -> bool {
    if text.contains('\0') {
        return false;
    }
    text.lines().any(|line| {
        let s = line.trim();
        if s.starts_with("--") || s.starts_with('#') {
            return false;
        }
        if s == "TEXT" {
            return true;
        }
        ["conky.config", "conky.text"].iter().any(|key| {
            s.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
        })
    })
}

/// Читается ли файл как файл настроек: имя не из пропускаемых, размер в
/// пределах, содержимое — [`looks_like_config`].
fn is_config_file(io: &dyn LookIo, path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if skipped_name(name) {
        return false;
    }
    io.read(path)
        .is_some_and(|text| text.len() <= MAX_CONFIG_BYTES && looks_like_config(&text))
}

/// Каталог ли это. У [`LookIo`] нет отдельного вопроса: каталог — то, в чём
/// есть имена; пустой каталог виджета всё равно не содержит.
fn is_dir(io: &dyn LookIo, path: &str) -> bool {
    !io.list_dir(path).is_empty()
}

/// Порядок предпочтения файлов настроек внутри темы: сначала имена, которые
/// conky и установщики называют «главными», затем по глубине и имени.
fn preference(path: &str, depth: usize) -> (usize, usize, String) {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let rank = if name == "conky.conf" || name == ".conkyrc" {
        0
    } else if name.ends_with(".conkyrc") || name.starts_with("conkyrc") {
        1
    } else if name.contains("conky") {
        2
    } else {
        3
    };
    (depth, rank, path.to_string())
}

/// Все файлы настроек Conky в каталоге темы — части виджета: у Gotham он
/// один, у Seamod и наборов «вариантов» — несколько. Главный — первым
/// ([`preference`]); дальше по глубине и имени. Глубина — [`MAX_DEPTH`],
/// скрытые каталоги (`.git`) не смотрятся.
pub fn config_files(io: &dyn LookIo, dir: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    walk(io, dir.trim_end_matches('/'), 1, &mut found);
    let mut keyed: Vec<((usize, usize, String), String)> =
        found.into_iter().map(|(d, p)| (preference(&p, d), p)).collect();
    keyed.sort();
    keyed.into_iter().map(|(_, p)| p).collect()
}

fn walk(io: &dyn LookIo, dir: &str, depth: usize, out: &mut Vec<(usize, String)>) {
    for name in io.list_dir(dir) {
        let path = format!("{dir}/{name}");
        if is_dir(io, &path) {
            if depth < MAX_DEPTH && !name.starts_with('.') {
                walk(io, &path, depth + 1, out);
            }
        } else if is_config_file(io, &path) {
            out.push((depth, path));
        }
    }
}

/// Одно место, где лежат виджеты.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub dir: String,
    /// Смотреть только файлы верхнего уровня: каталоги внутри
    /// `~/.config/conky` — виджеты, которые раздел и так показывает (туда
    /// ставит Rexon), а `conky.conf` прямо в нём — виджет, который conky
    /// запускает без `-c`.
    pub files_only: bool,
}

/// Каталог настроек пользователя (`$XDG_CONFIG_HOME` или `~/.config`).
pub fn config_home(ctx: &Ctx) -> String {
    crate::utils::config_home(ctx.xdg_config_home.as_deref(), &ctx.home)
}

/// Где искать виджеты: прежние каталоги conky и Conky Manager, каталог
/// Rexon (только файлы верхнего уровня) и свои папки человека (`extra`).
/// Повторы и пустые строки отбрасываются.
pub fn roots(ctx: &Ctx, extra: &[String]) -> Vec<Root> {
    let home = ctx.home.trim_end_matches('/');
    let config = config_home(ctx);
    let mut out: Vec<Root> = Vec::new();
    let mut push = |dir: String, files_only: bool| {
        let dir = dir.trim_end_matches('/').to_string();
        if !dir.is_empty() && !out.iter().any(|r| r.dir == dir) {
            out.push(Root { dir, files_only });
        }
    };
    push(crate::de_apply::conky_root(ctx), true);
    push(format!("{home}/.conky"), false);
    push(format!("{home}/.Conky"), false);
    push(format!("{config}/conky"), true);
    push(format!("{config}/Conky"), false);
    for dir in extra {
        let dir = dir.trim();
        let dir = match dir.strip_prefix("~/") {
            Some(rest) => format!("{home}/{rest}"),
            None => dir.to_string(),
        };
        push(dir, false);
    }
    out
}

/// Одиночные файлы настроек вне каталогов: `~/.conkyrc` — его conky читает
/// без `-c`, если нет `~/.config/conky/conky.conf`.
pub fn loose_files(ctx: &Ctx) -> Vec<String> {
    vec![format!("{}/.conkyrc", ctx.home.trim_end_matches('/'))]
}

/// Виджет, найденный на диске.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Каталог темы либо сам файл, если виджет — один файл. Он же — имя в
    /// номере плитки (`local:conky:<путь>`).
    pub path: String,
    /// Подпись: имя каталога или файла.
    pub title: String,
    /// Файлы настроек (части), главный — первым.
    pub confs: Vec<String>,
}

/// Все виджеты вне каталогов Rexon ([`roots`] + [`loose_files`]). Один и тот
/// же файл, видный по двум путям (`~/.Conky` — ссылка на `~/.conky`), — одна
/// плитка: по настоящему пути.
pub fn found(io: &dyn LookIo, ctx: &Ctx, extra: &[String]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut keep = |f: Found, out: &mut Vec<Found>| {
        let real = std::fs::canonicalize(&f.path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| f.path.clone());
        if !seen.contains(&real) {
            seen.push(real);
            out.push(f);
        }
    };
    for file in loose_files(ctx) {
        if is_config_file(io, &file) {
            keep(single(&file), &mut out);
        }
    }
    for root in roots(ctx, extra) {
        for name in io.list_dir(&root.dir) {
            let path = format!("{}/{name}", root.dir);
            if is_dir(io, &path) {
                if root.files_only || name.starts_with('.') {
                    continue;
                }
                let confs = config_files(io, &path);
                if !confs.is_empty() {
                    keep(Found { path, title: name, confs }, &mut out);
                }
            } else if is_config_file(io, &path) {
                keep(single(&path), &mut out);
            }
        }
    }
    out
}

fn single(path: &str) -> Found {
    let title = path.rsplit('/').next().unwrap_or(path).to_string();
    Found { path: path.to_string(), title, confs: vec![path.to_string()] }
}

/// Номер ли это найденной плитки (имя — абсолютный путь), а не виджета в
/// каталоге Rexon.
pub fn is_found_name(name: &str) -> bool {
    name.starts_with('/')
}

/// Какому найденному виджету принадлежит файл настроек процесса. Без
/// обхода диска: виджет — первый уровень под одним из [`roots`] (файл
/// верхнего уровня — сам по себе виджет) либо [`loose_files`]. `None` —
/// файл не из этих мест.
pub fn owner_of_conf(ctx: &Ctx, extra: &[String], conf: &str) -> Option<String> {
    if loose_files(ctx).iter().any(|f| f == conf) {
        return Some(conf.to_string());
    }
    for root in roots(ctx, extra) {
        let Some(rest) = conf.strip_prefix(&format!("{}/", root.dir)) else { continue };
        match rest.split_once('/') {
            None if !rest.is_empty() => return Some(conf.to_string()),
            Some((top, _)) if !root.files_only && !top.is_empty() => {
                return Some(format!("{}/{top}", root.dir));
            }
            _ => {}
        }
    }
    None
}

/// Подпись источника на плитке найденного виджета.
pub fn found_badge() -> String {
    tr("🔎 Found on disk")
}

/// Плитки «Найдено на диске» для раздела «Виджеты Conky». `running` —
/// номера запущенных сейчас (их плитки — «Остановить»).
pub fn found_items(found: &[Found], running: &[String]) -> Vec<ThemeItem> {
    let section = crate::categories::CONKY_SECTION_ID;
    let label = crate::constants::type_labels_ordered()
        .iter()
        .find(|(k, _)| *k == crate::de_apply::CONKY_KIND)
        .map(|(_, l)| tr(l))
        .unwrap_or_default();
    found
        .iter()
        .map(|f| ThemeItem {
            id: format!("local:conky:{}", f.path),
            name: f.title.clone(),
            author: format!("📊 {label} · {}", found_badge()),
            category_id: section,
            installed: true,
            install_path: f.path.clone(),
            local_kind: running.contains(&f.path).then(|| "active".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Свои папки с виджетами из настроек Rexon ([`EXTRA_DIRS_KEY`]).
pub fn live_extra_dirs() -> Vec<String> {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    settings
        .get(EXTRA_DIRS_KEY)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

// ── Виджет по имени плитки ────────────────────────────────────────────────
//
// Имя плитки раздела — либо имя каталога в `~/.config/conky` (виджет Rexon),
// либо абсолютный путь найденного ([`is_found_name`]). Кнопки `de_apply`
// спрашивают здесь, где он лежит, из каких частей состоит и чьи процессы.

/// Каталог виджета (у найденного одним файлом — сам файл).
pub fn widget_path(ctx: &Ctx, name: &str) -> String {
    if is_found_name(name) {
        name.trim_end_matches('/').to_string()
    } else {
        format!("{}/{name}", crate::de_apply::conky_root(ctx))
    }
}

/// Подпись виджета для сообщений: имя каталога или файла.
pub fn widget_title(name: &str) -> String {
    name.trim_end_matches('/').rsplit('/').next().unwrap_or(name).to_string()
}

/// Части виджета — его файлы настроек; `primary` (главный, его выбрала
/// кнопка по имени файла) — первым, остальные — по содержимому
/// ([`config_files`]). Виджет-файл — одна часть, он сам.
pub fn widget_confs(io: &dyn LookIo, ctx: &Ctx, name: &str, primary: Option<String>) -> Vec<String> {
    let path = widget_path(ctx, name);
    if !is_dir(io, &path) {
        return if is_found_name(name) && io.read(&path).is_some() { vec![path] } else { primary.into_iter().collect() };
    }
    let mut out: Vec<String> = primary.into_iter().collect();
    for c in config_files(io, &path) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Принадлежит ли файл настроек процесса виджету `name`.
pub fn widget_owns(ctx: &Ctx, name: &str, conf: &str) -> bool {
    let path = widget_path(ctx, name);
    conf == path || conf.starts_with(&format!("{path}/"))
}

/// Свои папки с виджетами — только для домашнего каталога этого процесса.
pub fn extra_dirs_for(ctx: &Ctx) -> Vec<String> {
    let own = std::env::var("HOME").ok().is_some_and(|h| h.trim_end_matches('/') == ctx.home.trim_end_matches('/'));
    if own { live_extra_dirs() } else { Vec::new() }
}

/// Часть имени файла автозапуска (`rexon-conky-<часть>.desktop`): у виджета
/// Rexon — его имя (как писала установка), у найденного — `found-` и
/// отпечаток пути (в имени файла косой черты быть не может).
pub fn autostart_key(name: &str) -> String {
    if !is_found_name(name) {
        return name.to_string();
    }
    // FNV-1a, 64 бита: стабилен между запусками и выпусками.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.trim_end_matches('/').bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("found-{h:016x}")
}

/// Файл настроек, который conky читает без `-c`:
/// `$XDG_CONFIG_HOME/conky/conky.conf`, если он есть, иначе `~/.conkyrc`.
pub fn default_conf(io: &dyn LookIo, ctx: &Ctx) -> String {
    let xdg = format!("{}/conky/conky.conf", config_home(ctx));
    if io.read(&xdg).is_some() {
        xdg
    } else {
        format!("{}/.conkyrc", ctx.home.trim_end_matches('/'))
    }
}

/// Найденные виджеты, у которых сейчас есть процесс conky (номера плиток —
/// пути, как в [`found_items`]).
pub fn running_found(io: &dyn LookIo, ctx: &Ctx, extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in crate::conky_procs::all(io) {
        let conf = p.conf.unwrap_or_else(|| default_conf(io, ctx));
        if let Some(owner) = owner_of_conf(ctx, extra, &conf) {
            if !out.contains(&owner) {
                out.push(owner);
            }
        }
    }
    out
}

/// Плитки «Найдено на диске» живой машины для домашнего каталога `home`.
/// `$XDG_CONFIG_HOME` и свои папки из настроек берутся, только если `home`
/// — домашний каталог этого процесса (скан чужого дерева их не касается).
pub fn live_found_items(home: &str) -> Vec<ThemeItem> {
    let io = crate::animations::SystemIo;
    let own = std::env::var("HOME").ok().is_some_and(|h| h.trim_end_matches('/') == home.trim_end_matches('/'));
    let ctx = Ctx {
        home: home.to_string(),
        xdg_config_home: std::env::var("XDG_CONFIG_HOME").ok().filter(|v| own && !v.is_empty()),
        wayland: false,
        de: String::new(),
    };
    let extra = if own { live_extra_dirs() } else { Vec::new() };
    let found = found(&io, &ctx, &extra);
    if found.is_empty() {
        return Vec::new();
    }
    found_items(&found, &running_found(&io, &ctx, &extra))
}

// ════════════════════════════════════════════════════════════════════════════
//  «Удалить» найденного виджета
// ════════════════════════════════════════════════════════════════════════════
//
// Виджет, поставленный не через Rexon, снимается осторожнее своего: после
// вопроса, в КОРЗИНУ (`utils::move_to_trash`) — его можно вернуть, — и только
// он сам. Корень места (`~/.conky`, `~/.config/conky`, `~/.Conky`,
// `~/.config/Conky`, своя папка человека) целиком не уходит никогда: там
// лежат и другие виджеты. Одиночный `~/.conkyrc` или
// `~/.config/conky/conky.conf` — один файл. Ссылка уходит сама, её цель
// остаётся на месте. Перед переносом гаснут процессы виджета (по номерам,
// как «Остановить»), после — снимается его запись автозапуска Rexon.

/// Что «Удалить» снимет у найденного виджета.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundRemoval {
    /// Папка виджета или его файл (как в номере плитки).
    pub path: String,
    /// Подпись: имя папки или файла.
    pub title: String,
    /// Что уходит в корзину: папка, файл или ссылка.
    pub shape: RemovalShape,
    /// Своя папка человека (`conky_extra_dirs`), где лежит виджет, — тогда
    /// переспросить.
    pub own_folder: Option<String>,
}

/// Что именно уходит в корзину.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalShape {
    Folder,
    File,
    Link,
}

/// Подсказка на недоступной «Удалить» и отказ при снятии.
pub fn found_removal_refused() -> String {
    tr("Rexon only removes the widget's own folder or file — not a folder with other widgets in it, \
        and not your home folder.")
}

/// Можно ли снять найденный виджет `path` и что именно. `Ok(None)` — его уже
/// нет на диске. Решение по пути, без доверия номеру плитки: путь — прямой
/// потомок одного из [`roots`] (у каталога Rexon — только файл верхнего
/// уровня) или [`loose_files`], не сам корень и не каталог над корнем или над
/// домашним, и на диске там по-прежнему виджет ([`config_files`] /
/// [`looks_like_config`]).
pub fn found_removal(io: &dyn LookIo, ctx: &Ctx, extra: &[String], path: &str) -> Result<Option<FoundRemoval>, String> {
    let refused = || Err(found_removal_refused());
    let p = path.trim_end_matches('/');
    if !is_found_name(p) || p.split('/').skip(1).any(|c| c.is_empty() || c == "." || c == "..") {
        return refused();
    }
    let home = ctx.home.trim_end_matches('/');
    let all = roots(ctx, extra);
    let above = |dir: &str| dir == p || dir.starts_with(&format!("{p}/"));
    if above(home) || above(&config_home(ctx)) || all.iter().any(|r| above(&r.dir)) {
        return refused();
    }
    let meta = match std::fs::symlink_metadata(p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return refused(),
    };
    let shape = if meta.file_type().is_symlink() {
        RemovalShape::Link
    } else if meta.is_dir() {
        RemovalShape::Folder
    } else {
        RemovalShape::File
    };
    let points_to_dir = std::fs::metadata(p).is_ok_and(|m| m.is_dir());
    let title = widget_title(p);
    let still_widget = if points_to_dir { !config_files(io, p).is_empty() } else { is_config_file(io, p) };
    if !still_widget {
        return refused();
    }
    if loose_files(ctx).iter().any(|f| f == p) {
        return if points_to_dir { refused() } else { Ok(Some(FoundRemoval { path: p.to_string(), title, shape, own_folder: None })) };
    }
    let Some((parent, _)) = p.rsplit_once('/') else { return refused() };
    let Some(root) = all.iter().find(|r| r.dir == parent) else { return refused() };
    // Папка в каталоге Rexon — свой виджет, у него своё «Удалить».
    if root.files_only && points_to_dir {
        return refused();
    }
    let builtin = roots(ctx, &[]);
    let own_folder = if builtin.iter().any(|b| b.dir == root.dir) {
        None
    } else {
        // Своя папка, которая сама домашний каталог или выше: «виджетом» там
        // оказалась бы любая папка с файлом настроек внутри.
        if root.dir == home || home.starts_with(&format!("{}/", root.dir)) {
            return refused();
        }
        Some(root.dir.clone())
    };
    Ok(Some(FoundRemoval { path: p.to_string(), title, shape, own_folder }))
}

/// Вопросы перед «Удалить» найденного виджета, по порядку: `(заголовок,
/// текст)`. Второй — если виджет лежит в своей папке человека.
pub fn found_removal_questions(r: &FoundRemoval) -> Vec<(String, String)> {
    let text = match r.shape {
        RemovalShape::Folder => trf(
            // TRANSLATORS: %1 is a Conky widget name.
            "Remove the “%1” widget? It wasn't installed through Rexon. The folder will go to the trash — you can restore it from there.",
            &[&r.title],
        ),
        RemovalShape::File => trf(
            // TRANSLATORS: %1 is a Conky widget name.
            "Remove the “%1” widget? It wasn't installed through Rexon. The file will go to the trash — you can restore it from there.",
            &[&r.title],
        ),
        RemovalShape::Link => trf(
            // TRANSLATORS: %1 is a Conky widget name.
            "Remove the “%1” widget? It wasn't installed through Rexon. The link to it will go to the trash — you can restore it from there; what the link points to stays in place.",
            &[&r.title],
        ),
    };
    let mut out = vec![(tr("Remove Widget?"), text)];
    if let Some(dir) = &r.own_folder {
        out.push((
            tr("Widget in Your Own Folder"),
            trf(
                // TRANSLATORS: %1 is a Conky widget name, %2 a folder path.
                "“%1” is in a folder you added to Rexon yourself:\n%2\n\nMove it to the trash anyway?",
                &[&r.title, dir],
            ),
        ));
    }
    out
}

/// Снять найденный виджет: погасить его процессы (по номерам), перенести в
/// корзину `home_trash` (`date` — метка записи корзины), снять запись
/// автозапуска Rexon. `(получилось, сообщение)`. Не погасился или не
/// перенёсся — на диске он остаётся как был.
pub fn remove_found(
    io: &dyn LookIo,
    ctx: &Ctx,
    extra: &[String],
    path: &str,
    home_trash: &Path,
    date: &str,
) -> (bool, String) {
    let r = match found_removal(io, ctx, extra, path) {
        Ok(Some(r)) => r,
        Ok(None) => return (true, String::new()),
        Err(reason) => return (false, reason),
    };
    stop_and_trash(io, ctx, &r.path, &r.title, home_trash, date)
}

/// Общий хвост «Удалить» виджета Conky — найденного и своего запущенного:
/// погасить процессы ЭТОГО виджета (`name` — его имя, как у
/// [`widget_owns`]) по номерам и дождаться выхода, перенести `path` в
/// корзину, снять запись автозапуска Rexon. Не погасился или не перенёсся —
/// на диске он остаётся как был, автозапуск тоже.
fn stop_and_trash(io: &dyn LookIo, ctx: &Ctx, name: &str, title: &str, home_trash: &Path, date: &str) -> (bool, String) {
    let path = widget_path(ctx, name);
    let pids = crate::conky_procs::pids_where(io, ctx, &|conf| widget_owns(ctx, name, conf));
    if !pids.is_empty() {
        let (ok, msg) = crate::conky_procs::stop_pids(io, title, pids.clone());
        if !ok {
            return (false, msg);
        }
        // conky выходит по сигналу не мгновенно: перенос — после выхода.
        for _ in 0..20 {
            let alive = pids.iter().any(|pid| io.read(&format!("/proc/{pid}/cmdline")).is_some_and(|c| !c.is_empty()));
            if !alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    if let Err(e) = crate::utils::move_to_trash(Path::new(&path), home_trash, date) {
        // TRANSLATORS: %1 is a Conky widget name, %2 the system error.
        return (false, trf("Couldn't move “%1” to the trash: %2. The widget stays where it was.", &[&title, &e]));
    }
    crate::de_apply::conky_autostart_forget(ctx, name);
    // TRANSLATORS: %1 is a Conky widget name.
    (true, trf("🗑️ “%1” moved to the trash", &[&title]))
}

// ── «Удалить» своего запущенного виджета ──────────────────────────────
//
// Свой виджет (папка в каталоге Rexon), который сейчас работает, снимается
// так же, как найденный: после вопроса гаснут его процессы (по номерам),
// уходит запись автозапуска, папка — в корзину.

/// Имя своего виджета по пути его папки: прямой потомок каталога Rexon,
/// папка (не ссылка и не файл). Иначе `None`.
pub fn own_widget_name(ctx: &Ctx, path: &str) -> Option<String> {
    let root = format!("{}/", crate::de_apply::conky_root(ctx));
    let id = path.trim_end_matches('/').strip_prefix(&root)?;
    if id.is_empty() || id == "." || id == ".." || id.contains('/') {
        return None;
    }
    let meta = std::fs::symlink_metadata(format!("{root}{id}")).ok()?;
    meta.is_dir().then(|| id.to_string())
}

/// Вопрос перед «Удалить» своего запущенного виджета: `(заголовок, текст)`.
pub fn running_removal_question(name: &str) -> (String, String) {
    (
        tr("Remove Widget?"),
        trf(
            // TRANSLATORS: %1 is a Conky widget name.
            "The “%1” widget is running. Stop it and remove it? The folder will go to the trash — you can restore it from there.",
            &[&widget_title(name)],
        ),
    )
}

/// Снять свой запущенный виджет по пути папки `path`: погасить его
/// процессы, снять автозапуск, папку — в корзину. `(получилось, сообщение)`.
pub fn remove_running_own(io: &dyn LookIo, ctx: &Ctx, path: &str, home_trash: &Path, date: &str) -> (bool, String) {
    if std::fs::symlink_metadata(path.trim_end_matches('/')).is_err() {
        return (true, String::new());
    }
    let Some(name) = own_widget_name(ctx, path) else {
        return (false, found_removal_refused());
    };
    stop_and_trash(io, ctx, &name, &widget_title(&name), home_trash, date)
}

/// [`remove_running_own`] на этой машине: корзина — `$XDG_DATA_HOME/Trash`.
pub fn live_remove_running_own(path: &str) -> (bool, String) {
    let ctx = Ctx::live();
    let xdg_data = std::env::var("XDG_DATA_HOME").ok();
    let trash = crate::utils::home_trash_dir(xdg_data.as_deref(), &ctx.home);
    let date = crate::utils::trash_deletion_date_now();
    remove_running_own(&crate::animations::SystemIo, &ctx, path, Path::new(&trash), &date)
}

/// [`found_removal`] на этой машине.
pub fn live_found_removal(path: &str) -> Result<Option<FoundRemoval>, String> {
    let ctx = Ctx::live();
    found_removal(&crate::animations::SystemIo, &ctx, &extra_dirs_for(&ctx), path)
}

/// [`remove_found`] на этой машине: корзина — `$XDG_DATA_HOME/Trash`.
pub fn live_remove_found(path: &str) -> (bool, String) {
    let ctx = Ctx::live();
    let xdg_data = std::env::var("XDG_DATA_HOME").ok();
    let trash = crate::utils::home_trash_dir(xdg_data.as_deref(), &ctx.home);
    let date = crate::utils::trash_deletion_date_now();
    remove_found(&crate::animations::SystemIo, &ctx, &extra_dirs_for(&ctx), path, Path::new(&trash), &date)
}

// ════════════════════════════════════════════════════════════════════════════
//  Слежка
// ════════════════════════════════════════════════════════════════════════════

/// Слежка — общая на крейт (`fs_watch`); здесь только цели раздела.
pub use crate::fs_watch::{Watch, WatchTarget};

/// Что слушать: каждое место виджетов целиком (новая папка темы, новый
/// файл) и родителей мест — чтобы заметить, как появился сам `~/.conky`
/// или `~/.conkyrc`. Правка уже найденного файла список не меняет и слежки
/// не требует.
pub fn watch_targets(ctx: &Ctx, extra: &[String]) -> Vec<WatchTarget> {
    let mut by_dir: Vec<WatchTarget> = Vec::new();
    let mut add = |dir: &str, name: Option<&str>| {
        let i = match by_dir.iter().position(|t| t.dir == dir) {
            Some(i) => i,
            None => {
                by_dir.push(WatchTarget { dir: dir.to_string(), names: Vec::new() });
                by_dir.len() - 1
            }
        };
        match name {
            // Слежка за любым именем поглощает слежку за отдельными.
            None => by_dir[i].names = vec![String::new()],
            Some(n) if by_dir[i].names != [String::new()] && !by_dir[i].names.iter().any(|x| x == n) => {
                by_dir[i].names.push(n.to_string());
            }
            _ => {}
        }
    };
    let split = |p: &str| p.rsplit_once('/').map(|(d, n)| (d.to_string(), n.to_string()));
    for root in roots(ctx, extra) {
        add(&root.dir, None);
        if let Some((parent, name)) = split(&root.dir) {
            add(&parent, Some(&name));
        }
    }
    for file in loose_files(ctx) {
        if let Some((parent, name)) = split(&file) {
            add(&parent, Some(&name));
        }
    }
    for t in &mut by_dir {
        if t.names == [String::new()] {
            t.names.clear();
        }
    }
    by_dir
}


// ============================================================================
// Картинка и источник виджета (плитка и карточка)
// ============================================================================

/// Окончания картинок, которые кладут рядом с виджетом.
const PICTURE_EXTS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Картинка виджета в его папке, если общий поиск превью ничего не нашёл.
///
/// Авторы виджетов кладут снимок рядом с файлом настроек и называют его так
/// же: `Gotham` → `Gotham.jpg`, `conky_seamod` → `conky_seamod.jpg`,
/// `clock.conf` → `clock.png` или `clock.conf.png`. Порядок: для каждого
/// файла настроек (главный — первым) — снимок с его именем, затем с именем
/// без окончания; затем `preview.*`, затем `screenshot*.*`. `path` — папка
/// виджета либо сам файл (виджет одним файлом — снимок рядом с ним).
pub fn widget_picture(io: &dyn LookIo, path: &str, confs: &[String]) -> Option<String> {
    let path = path.trim_end_matches('/');
    let folder = is_dir(io, path);
    let (dir, names): (String, Vec<String>) = if folder {
        (path.to_string(), io.list_dir(path))
    } else {
        let (d, _) = path.rsplit_once('/')?;
        (d.to_string(), io.list_dir(d))
    };
    let has = |name: &str| names.iter().any(|n| n == name);
    let picture_of = |stem: &str| {
        PICTURE_EXTS.iter().map(|e| format!("{stem}.{e}")).find(|n| has(n)).map(|n| format!("{dir}/{n}"))
    };
    for conf in confs {
        let Some(rel) = conf.strip_prefix(&format!("{dir}/")) else { continue };
        // Снимок лежит рядом с файлом: и в подпапке темы тоже.
        let (sub, file) = rel.rsplit_once('/').map(|(s, f)| (format!("{dir}/{s}"), f)).unwrap_or((dir.clone(), rel));
        let sub_names = if sub == dir { names.clone() } else { io.list_dir(&sub) };
        let near = |stem: &str| {
            PICTURE_EXTS
                .iter()
                .map(|e| format!("{stem}.{e}"))
                .find(|n| sub_names.iter().any(|x| x == n))
                .map(|n| format!("{sub}/{n}"))
        };
        if let Some(p) = near(file) {
            return Some(p);
        }
        if let Some((stem, _)) = file.rsplit_once('.') {
            if !stem.is_empty() {
                if let Some(p) = near(stem) {
                    return Some(p);
                }
            }
        }
    }
    // Виджет одним файлом лежит среди чужих файлов (`~/.conkyrc` — в
    // домашней папке): общие имена там не про него.
    if !folder {
        return None;
    }
    if let Some(p) = picture_of("preview") {
        return Some(p);
    }
    let mut shots: Vec<&String> = names
        .iter()
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l.starts_with("screenshot") && PICTURE_EXTS.iter().any(|e| l.ends_with(&format!(".{e}")))
        })
        .collect();
    shots.sort();
    shots.first().map(|n| format!("{dir}/{n}"))
}

/// Откуда виджет и кто его автор — по `source.txt` и `credits.txt` в его
/// папке (так их кладут пакеты тем). Готовый кусок разметки для описания
/// карточки: адрес — ссылкой (только `http`/`https`), остальное — текстом.
/// `None` — ни того, ни другого.
pub fn widget_credits_html(io: &dyn LookIo, path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    // У виджета одним файлом своей папки нет — и своего source.txt тоже.
    if !is_dir(io, path) {
        return None;
    }
    let dir = path.to_string();
    let esc = crate::translator::html_escape;
    let mut parts: Vec<String> = Vec::new();
    if let Some(text) = io.read(&format!("{dir}/source.txt")) {
        let url = text.split_whitespace().find(|w| w.starts_with("https://") || w.starts_with("http://"));
        if let Some(url) = url {
            let url = esc(url);
            parts.push(trf("Source: %1", &[&format!("<a href=\"{url}\">{url}</a>")]));
        }
    }
    if let Some(text) = io.read(&format!("{dir}/credits.txt")) {
        let text = text.trim();
        if !text.is_empty() && text.len() <= 2000 {
            parts.push(trf("Credits: %1", &[&esc(text).replace('\n', "<br>")]));
        }
    }
    (!parts.is_empty()).then(|| parts.join("<br>"))
}

/// Плитка виджета Conky: картинка из папки виджета, если её ещё нет, и
/// «Источник»/«Авторы» в описании, если описания нет.
pub fn decorate_item(io: &dyn LookIo, ctx: &Ctx, item: &mut ThemeItem) {
    let Some(name) = item.id.strip_prefix("local:conky:") else { return };
    let path = widget_path(ctx, name);
    if item.preview_local.is_empty() {
        let confs = widget_confs(io, ctx, name, None);
        // Своя картинка автора, иначе снимок Rexon (`conky_preview`): пока
        // он снимается или переснимается, плитка показывает прежний.
        match widget_picture(io, &path, &confs) {
            Some(p) => item.preview_local = p,
            None => {
                if let Some(p) = crate::conky_preview::cached(ctx, &confs) {
                    item.preview_local = p;
                }
                crate::conky_preview::want(ctx, name);
            }
        }
    }
    if item.description.is_empty() {
        if let Some(html) = widget_credits_html(io, &path) {
            item.description = html;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animations::SystemIo;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-conky-scan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &Path, rel: &str, text: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn ctx(home: &Path) -> Ctx {
        Ctx { home: home.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: String::new() }
    }

    /// Картинка виджета: рядом с файлом настроек под его именем (с
    /// окончанием и без), затем `preview.*`, затем `screenshot*`; у виджета
    /// одним файлом — только под его именем.
    #[test]
    fn widget_picture_follows_the_config_name() {
        let d = tmp("picture");
        let s = |p: &Path| p.to_string_lossy().into_owned();
        put(&d, "Teejee/CPU Panel (2-core)", b"TEXT\nx\n");
        put(&d, "Teejee/CPU Panel (2-core).jpg", b"j");
        put(&d, "Teejee/nvidia.jpg", b"j");
        put(&d, "Clock/variants/clock.conf", b"conky.text = [[x]]\n");
        put(&d, "Clock/variants/clock.png", b"p");
        put(&d, "Clock/preview.jpeg", b"p");
        put(&d, "Shots/main.conf", b"conky.text = [[x]]\n");
        put(&d, "Shots/screenshot-2.png", b"p");
        put(&d, "Shots/screenshot-1.png", b"p");
        put(&d, "Bare/w.conf", b"conky.text = [[x]]\n");
        put(&d, "home/.conkyrc", b"conky.text = [[x]]\n");
        put(&d, "home/screenshot.png", b"p");
        let io = SystemIo;
        let pic = |dir: &str| {
            let dir = s(&d.join(dir));
            widget_picture(&io, &dir, &config_files(&io, &dir))
        };
        assert_eq!(pic("Teejee"), Some(s(&d.join("Teejee/CPU Panel (2-core).jpg"))));
        assert_eq!(pic("Clock"), Some(s(&d.join("Clock/variants/clock.png"))));
        assert_eq!(pic("Shots"), Some(s(&d.join("Shots/screenshot-1.png"))));
        assert_eq!(pic("Bare"), None);
        let rc = s(&d.join("home/.conkyrc"));
        assert_eq!(widget_picture(&io, &rc, &[rc.clone()]), None, "снимок экрана в домашней папке — не картинка виджета");
        assert_eq!(widget_credits_html(&io, &rc), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn config_is_told_by_content_not_by_name() {
        assert!(looks_like_config("conky.config = {\n}\nconky.text = [[x]]"));
        // Шапка комментариев до `conky.config` (conky-manager #52).
        assert!(looks_like_config("-- Gotham\n-- by someone\n\nconky.config={ }\n"));
        assert!(looks_like_config("  conky.text   = [[ ${time} ]]"));
        // Синтаксис 1.9.
        assert!(looks_like_config("alignment top_right\ngap_x 10\nTEXT\n${time}\n"));
        // Не настройки: визуализатор, закомментированные строки, двоичный файл.
        assert!(!looks_like_config("[general]\nbars = 128\n"));
        assert!(!looks_like_config("-- conky.text = [[x]]\n# TEXT\n"));
        assert!(!looks_like_config("conky.text = [[x]]\0\u{1}"));
        assert!(!looks_like_config("local conky_config_path = 1"));
    }

    #[test]
    fn scripts_pictures_and_backups_are_not_read() {
        for n in ["rings.lua", "bg.PNG", "font.ttf", "theme.tar.gz", "conky.conf~", "x.bak", "README.md", ".DS_Store"] {
            assert!(skipped_name(n), "{n}");
        }
        for n in ["conky.conf", ".conkyrc", "Gotham", "conkyrc_seamod", "Jhalak.conf", "cava.conf"] {
            assert!(!skipped_name(n), "{n}");
        }
    }

    /// Дерево-фикстура: темы в `~/.conky`, `~/.Conky`, `~/.config/Conky`,
    /// одиночный `~/.conkyrc`, `conky.conf` прямо в `~/.config/conky`, свои
    /// папки; рядом Lua, картинки, копии редактора, чужие `.conf`.
    #[test]
    fn widgets_are_found_in_all_places() {
        let h = tmp("all");
        let cfg = "conky.config = { }\nconky.text = [[ ${time} ]]\n";
        put(&h, ".conkyrc", b"TEXT\n${time}\n");
        put(&h, ".conky/Gotham/Gotham", b"# Gotham\nalignment tr\nTEXT\n${time}\n");
        put(&h, ".conky/Gotham/Gotham~", cfg.as_bytes());
        put(&h, ".conky/Gotham/rings.lua", b"conky.text = [[x]]");
        put(&h, ".conky/Seamod/conkyrc_seamod", cfg.as_bytes());
        put(&h, ".conky/Seamod/lua/seamod_rings.lua", b"x");
        put(&h, ".conky/Seamod/variants/small/conky.conf", cfg.as_bytes());
        // Слишком глубоко — не часть темы (четвёртый уровень).
        put(&h, ".conky/Seamod/a/b/c/deep.conf", cfg.as_bytes());
        put(&h, ".conky/Pics/bg.png", b"\x89PNG\0\0");
        put(&h, ".conky/.git/config", cfg.as_bytes());
        put(&h, ".Conky/Old/conkyrc", cfg.as_bytes());
        put(&h, ".config/Conky/Mimosa/mimosa.conf", cfg.as_bytes());
        put(&h, ".config/conky/conky.conf", cfg.as_bytes());
        // Каталог Rexon: его темы раздел показывает и так.
        put(&h, ".config/conky/Jhalak/Jhalak.conf", format!("-- JHALAK\n{cfg}").as_bytes());
        put(&h, ".config/conky/Jhalak/cava.conf", b"[general]\nbars = 128\n");
        put(&h, "mine/Clock/clock.conf", cfg.as_bytes());
        let c = ctx(&h);
        let home = c.home.clone();
        let got = found(&SystemIo, &c, &[format!("{home}/mine")]);
        let paths: Vec<String> = got.iter().map(|f| f.path.trim_start_matches(&home).to_string()).collect();
        assert_eq!(
            paths,
            [
                "/.conkyrc",
                "/.config/conky/conky.conf",
                "/.conky/Gotham",
                "/.conky/Seamod",
                "/.Conky/Old",
                "/.config/Conky/Mimosa",
                "/mine/Clock",
            ]
        );
        let seamod = got.iter().find(|f| f.title == "Seamod").unwrap();
        let parts: Vec<String> = seamod.confs.iter().map(|p| p.trim_start_matches(&home).to_string()).collect();
        assert_eq!(parts, ["/.conky/Seamod/conkyrc_seamod", "/.conky/Seamod/variants/small/conky.conf"]);
        let gotham = got.iter().find(|f| f.title == "Gotham").unwrap();
        assert_eq!(gotham.confs.len(), 1, "копия редактора и Lua — не части: {:?}", gotham.confs);
        // В теме Rexon часть — только настоящий виджет, не `cava.conf`.
        let jh = config_files(&SystemIo, &format!("{home}/.config/conky/Jhalak"));
        assert_eq!(jh, [format!("{home}/.config/conky/Jhalak/Jhalak.conf")]);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn link_to_the_same_place_gives_one_tile() {
        let h = tmp("link");
        put(&h, ".conky/Gotham/Gotham", b"TEXT\n");
        std::os::unix::fs::symlink(h.join(".conky"), h.join(".Conky")).unwrap();
        let got = found(&SystemIo, &ctx(&h), &[]);
        assert_eq!(got.len(), 1, "{got:?}");
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn process_config_is_mapped_to_its_widget() {
        let c = ctx(Path::new("/home/u"));
        let extra = vec!["~/w".to_string()];
        assert_eq!(owner_of_conf(&c, &extra, "/home/u/.conky/Gotham/Gotham").as_deref(), Some("/home/u/.conky/Gotham"));
        assert_eq!(owner_of_conf(&c, &extra, "/home/u/.conky/one.conf").as_deref(), Some("/home/u/.conky/one.conf"));
        assert_eq!(owner_of_conf(&c, &extra, "/home/u/.conkyrc").as_deref(), Some("/home/u/.conkyrc"));
        assert_eq!(
            owner_of_conf(&c, &extra, "/home/u/.config/conky/conky.conf").as_deref(),
            Some("/home/u/.config/conky/conky.conf")
        );
        // Тема Rexon — не «найденная»; чужие места — ничьи.
        assert_eq!(owner_of_conf(&c, &extra, "/home/u/.config/conky/Jhalak/Jhalak.conf"), None);
        assert_eq!(owner_of_conf(&c, &extra, "/etc/conky/conky.conf"), None);
        assert_eq!(owner_of_conf(&c, &extra, "/home/u/w/C/c.conf").as_deref(), Some("/home/u/w/C"));
    }

    #[test]
    fn found_tiles_carry_the_path_and_running_state() {
        let f = vec![
            Found { path: "/h/.conky/A".into(), title: "A".into(), confs: vec!["/h/.conky/A/a".into()] },
            Found { path: "/h/.conkyrc".into(), title: ".conkyrc".into(), confs: vec!["/h/.conkyrc".into()] },
        ];
        let items = found_items(&f, &["/h/.conkyrc".to_string()]);
        assert_eq!(items[0].id, "local:conky:/h/.conky/A");
        assert_eq!(crate::de_apply::apply_kind_of_local_id(&items[0].id), Some("conky"));
        assert_eq!(items[0].local_kind, None);
        assert_eq!(items[1].local_kind.as_deref(), Some("active"));
        assert_eq!(items[0].category_id, crate::categories::CONKY_SECTION_ID);
        assert!(items[0].author.contains(&found_badge()));
    }

    #[test]
    fn widget_names_resolve_to_places_and_parts() {
        let h = tmp("parts");
        let c = ctx(&h);
        let home = c.home.clone();
        put(&h, ".config/conky/Seamod/conky.conf", b"conky.text = [[a]]");
        put(&h, ".config/conky/Seamod/variants/small.conf", b"conky.text = [[b]]");
        put(&h, ".config/conky/Seamod/cava.conf", b"[general]");
        put(&h, ".conkyrc", b"TEXT\n");
        let primary = Some(format!("{home}/.config/conky/Seamod/conky.conf"));
        assert_eq!(
            widget_confs(&SystemIo, &c, "Seamod", primary),
            [format!("{home}/.config/conky/Seamod/conky.conf"), format!("{home}/.config/conky/Seamod/variants/small.conf")]
        );
        let rc = format!("{home}/.conkyrc");
        assert_eq!(widget_confs(&SystemIo, &c, &rc, None), [rc.clone()]);
        assert!(widget_owns(&c, "Seamod", &format!("{home}/.config/conky/Seamod/variants/small.conf")));
        assert!(!widget_owns(&c, "Seam", &format!("{home}/.config/conky/Seamod/conky.conf")));
        assert!(widget_owns(&c, &rc, &rc));
        assert_eq!(widget_title(&rc), ".conkyrc");
        assert_eq!(autostart_key("Seamod"), "Seamod");
        let k = autostart_key("/h/.conky/Gotham");
        assert!(k.starts_with("found-") && !k.contains('/') && k == autostart_key("/h/.conky/Gotham/"), "{k}");
        assert_ne!(k, autostart_key("/h/.conky/Gotham2"));
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn watch_targets_cover_places_and_their_parents() {
        let c = ctx(Path::new("/home/u"));
        let t = watch_targets(&c, &[]);
        let get = |d: &str| t.iter().find(|x| x.dir == d).map(|x| x.names.clone());
        assert_eq!(get("/home/u/.conky"), Some(vec![]));
        let home = get("/home/u").unwrap();
        for n in [".conky", ".Conky", ".conkyrc"] {
            assert!(home.contains(&n.to_string()), "{home:?}");
        }
        let cfg = get("/home/u/.config").unwrap();
        assert!(cfg.contains(&"conky".to_string()) && cfg.contains(&"Conky".to_string()), "{cfg:?}");
    }

    #[test]
    fn watch_wakes_on_new_widget_and_on_created_place() {
        let h = tmp("watch");
        let c = ctx(&h);
        let mut w = Watch::new(watch_targets(&c, &[])).expect("inotify");
        assert!(!w.wait(50));
        // Появился сам `~/.conky` — заметить, и дальше следить внутри.
        std::fs::create_dir_all(h.join(".conky")).unwrap();
        assert!(w.wait(500));
        std::fs::create_dir_all(h.join(".conky/New")).unwrap();
        assert!(w.wait(500));
        // Посторонний файл в домашнем каталоге — не повод.
        std::fs::write(h.join("notes.txt"), "x").unwrap();
        assert!(!w.wait(200));
        let _ = std::fs::remove_dir_all(&h);
    }

    /// Машина стенда: файлы — настоящие (во временном HOME), процессы —
    /// подставные (`procs`: номер → командная строка conky), `kill` гасит
    /// подставной процесс и запоминает, был ли виджет `watch` ещё на месте.
    struct Bench {
        procs: std::cell::RefCell<Vec<(String, String)>>,
        kills: std::cell::RefCell<Vec<Vec<String>>>,
        watch: String,
        present_at_kill: std::cell::Cell<Option<bool>>,
    }

    impl Bench {
        fn new(watch: &str, procs: &[(&str, &str)]) -> Self {
            Bench {
                procs: std::cell::RefCell::new(procs.iter().map(|(p, c)| (p.to_string(), c.to_string())).collect()),
                kills: Default::default(),
                watch: watch.to_string(),
                present_at_kill: Default::default(),
            }
        }
    }

    impl LookIo for Bench {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.kills.borrow_mut().push(a.to_vec());
            self.present_at_kill.set(Some(std::fs::symlink_metadata(&self.watch).is_ok()));
            self.procs.borrow_mut().retain(|(p, _)| !a[1..].contains(p));
            (true, String::new())
        }
        fn have(&self, _t: &str) -> bool {
            true
        }
        fn read(&self, p: &str) -> Option<String> {
            if let Some(rest) = p.strip_prefix("/proc/") {
                let (pid, what) = rest.split_once('/')?;
                let procs = self.procs.borrow();
                let (_, cmd) = procs.iter().find(|(x, _)| x == pid)?;
                return Some(if what == "comm" { "conky\n".to_string() } else { cmd.clone() });
            }
            SystemIo.read(p)
        }
        fn write(&self, p: &str, t: &str) -> bool {
            SystemIo.write(p, t)
        }
        fn list_dir(&self, p: &str) -> Vec<String> {
            if p == "/proc" {
                return self.procs.borrow().iter().map(|(x, _)| x.clone()).collect();
            }
            SystemIo.list_dir(p)
        }
    }

    fn trash_names(trash: &Path, sub: &str) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(trash.join(sub))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Корень места, каталог над ним, домашний каталог, папка виджета Rexon
    /// и вложенная глубже папка — не снимаются; кнопка недоступна с причиной.
    #[test]
    fn removal_never_takes_a_whole_place() {
        let h = tmp("rm-roots");
        let cfg = b"conky.text = [[x]]\n";
        for rel in [".conky/Gotham/Gotham", ".Conky/Old/conkyrc", ".config/Conky/Mimosa/m.conf", ".config/conky/Jhalak/Jhalak.conf", "mine/Clock/c.conf"] {
            put(&h, rel, cfg);
        }
        let c = ctx(&h);
        let home = c.home.clone();
        let extra = vec![format!("{home}/mine")];
        for rel in ["", "/.config", "/.conky", "/.Conky", "/.config/conky", "/.config/Conky", "/mine", "/.config/conky/Jhalak", "/.conky/Gotham/Gotham", "/.conky/./Gotham", "/.conky/x/../Gotham"] {
            let p = format!("{home}{rel}");
            assert_eq!(found_removal(&SystemIo, &c, &extra, &p), Err(found_removal_refused()), "{p}");
            let before = std::fs::symlink_metadata(&p).is_ok();
            let trash = h.join("Trash");
            assert!(!remove_found(&Bench::new(&p, &[]), &c, &extra, &p, &trash, "d").0, "{p}");
            assert_eq!(std::fs::symlink_metadata(&p).is_ok(), before, "{p}");
            assert!(trash_names(&trash, "files").is_empty(), "{p}");
        }
        // Папка без файла настроек (не виджет) и путь вне мест — тоже нет.
        put(&h, ".conky/Pics/bg.png", b"png");
        assert!(found_removal(&SystemIo, &c, &extra, &format!("{home}/.conky/Pics")).is_err());
        put(&h, "elsewhere/W/w.conf", cfg);
        assert!(found_removal(&SystemIo, &c, &extra, &format!("{home}/elsewhere/W")).is_err());
        // Своя папка = домашний каталог: «виджетом» была бы любая папка.
        put(&h, "Documents/notes/c.conf", cfg);
        assert!(found_removal(&SystemIo, &c, std::slice::from_ref(&home), &format!("{home}/Documents")).is_err());
        // Виджета уже нет — снимать нечего.
        assert_eq!(found_removal(&SystemIo, &c, &extra, &format!("{home}/.conky/Gone")), Ok(None));
        // А сам виджет в месте — можно, без второго вопроса.
        let ok = found_removal(&SystemIo, &c, &extra, &format!("{home}/.conky/Gotham")).unwrap().unwrap();
        assert_eq!((ok.shape, ok.own_folder.as_deref(), ok.title.as_str()), (RemovalShape::Folder, None, "Gotham"));
        let q = found_removal_questions(&ok);
        assert_eq!(q.len(), 1);
        assert!(q[0].1.contains("Gotham"), "{q:?}");
        let _ = std::fs::remove_dir_all(&h);
    }

    /// `~/.conkyrc` и `~/.config/conky/conky.conf` уходят в корзину одним
    /// файлом; соседи (виджеты Rexon в `~/.config/conky`) остаются.
    #[test]
    fn single_file_widgets_go_to_trash_as_one_file() {
        let h = tmp("rm-file");
        put(&h, ".conkyrc", b"TEXT\n${time}\n");
        put(&h, ".config/conky/conky.conf", b"conky.text = [[x]]\n");
        put(&h, ".config/conky/Jhalak/Jhalak.conf", b"conky.text = [[x]]\n");
        let c = ctx(&h);
        let home = c.home.clone();
        let trash = h.join("data/Trash");
        for (rel, name) in [("/.conkyrc", ".conkyrc"), ("/.config/conky/conky.conf", "conky.conf")] {
            let p = format!("{home}{rel}");
            let r = found_removal(&SystemIo, &c, &[], &p).unwrap().unwrap();
            assert_eq!(r.shape, RemovalShape::File);
            assert!(found_removal_questions(&r)[0].1.contains("file"), "{:?}", found_removal_questions(&r));
            let (ok, msg) = remove_found(&Bench::new(&p, &[]), &c, &[], &p, &trash, "2026-10-19T10:00:00");
            assert!(ok, "{msg}");
            assert!(!Path::new(&p).exists());
            assert!(trash.join("files").join(name).is_file(), "{name}");
        }
        assert!(h.join(".config/conky/Jhalak/Jhalak.conf").is_file());
        assert!(h.join(".config/conky").is_dir());
        let _ = std::fs::remove_dir_all(&h);
    }

    /// Виджет-ссылка: в корзину уходит ссылка, папка, куда она смотрит, цела.
    #[test]
    fn linked_widget_goes_to_trash_as_the_link() {
        let h = tmp("rm-link");
        put(&h, "store/Gotham/Gotham", b"TEXT\n");
        std::fs::create_dir_all(h.join(".conky")).unwrap();
        std::os::unix::fs::symlink(h.join("store/Gotham"), h.join(".conky/Gotham")).unwrap();
        let c = ctx(&h);
        let p = format!("{}/.conky/Gotham", c.home);
        let r = found_removal(&SystemIo, &c, &[], &p).unwrap().unwrap();
        assert_eq!(r.shape, RemovalShape::Link);
        let trash = h.join("Trash");
        assert!(remove_found(&Bench::new(&p, &[]), &c, &[], &p, &trash, "d").0);
        assert!(std::fs::symlink_metadata(&p).is_err());
        assert!(std::fs::symlink_metadata(trash.join("files/Gotham")).unwrap().file_type().is_symlink());
        assert!(h.join("store/Gotham/Gotham").is_file());
        let _ = std::fs::remove_dir_all(&h);
    }

    /// Процессы виджета гаснут ДО переноса и только его; запись автозапуска
    /// Rexon снимается, чужая остаётся; запись корзины — с путём в
    /// URL-кодировке (пробел, кириллица) и датой; своя папка — второй вопрос.
    #[test]
    fn removal_stops_widget_first_and_writes_trash_info() {
        let h = tmp("rm-full");
        put(&h, "Мои виджеты/Часы Gotham/Gotham", b"TEXT\n${time}\n");
        put(&h, ".conky/Other/o.conf", b"TEXT\n");
        let c = Ctx { xdg_config_home: Some(h.join("cfg").to_string_lossy().into_owned()), ..ctx(&h) };
        let home = c.home.clone();
        let extra = vec!["~/Мои виджеты".to_string()];
        let p = format!("{home}/Мои виджеты/Часы Gotham");
        let auto = crate::de_apply::conky_autostart_path(&c, &p);
        let other_auto = crate::de_apply::conky_autostart_path(&c, &format!("{home}/.conky/Other"));
        put(Path::new("/"), &auto, b"[Desktop Entry]\n");
        put(Path::new("/"), &other_auto, b"[Desktop Entry]\n");

        let r = found_removal(&SystemIo, &c, &extra, &p).unwrap().unwrap();
        assert_eq!(r.own_folder.as_deref(), Some(format!("{home}/Мои виджеты").as_str()));
        let q = found_removal_questions(&r);
        assert_eq!(q.len(), 2, "своя папка — второй вопрос: {q:?}");
        assert!(q[1].1.contains(&format!("{home}/Мои виджеты")), "{q:?}");

        let bench = Bench::new(
            &p,
            &[
                ("42", &format!("conky\0-c\0{p}/Gotham\0")),
                ("43", &format!("conky\0-c\0{home}/.conky/Other/o.conf\0")),
            ],
        );
        let trash = h.join("data/Trash");
        let (ok, msg) = remove_found(&bench, &c, &extra, &p, &trash, "2026-10-19T09:08:07");
        assert!(ok, "{msg}");
        assert!(msg.contains("Часы Gotham"), "{msg}");
        assert_eq!(*bench.kills.borrow(), vec![vec!["kill".to_string(), "42".to_string()]]);
        assert_eq!(bench.present_at_kill.get(), Some(true), "погашен до переноса");
        assert!(!Path::new(&p).exists());
        assert!(trash.join("files/Часы Gotham/Gotham").is_file());
        assert_eq!(trash_names(&trash, "info"), ["Часы Gotham.trashinfo"]);
        let info = std::fs::read_to_string(trash.join("info/Часы Gotham.trashinfo")).unwrap();
        let esc = crate::utils::trash_escape_path(home.as_bytes());
        assert_eq!(
            info,
            format!(
                "[Trash Info]\nPath={esc}/%D0%9C%D0%BE%D0%B8%20%D0%B2%D0%B8%D0%B4%D0%B6%D0%B5%D1%82%D1%8B/%D0%A7%D0%B0%D1%81%D1%8B%20Gotham\nDeletionDate=2026-10-19T09:08:07\n"
            )
        );
        assert!(!Path::new(&auto).exists(), "автозапуск Rexon снят");
        assert!(Path::new(&other_auto).exists(), "чужой автозапуск цел");
        assert!(h.join(".conky/Other/o.conf").is_file());
        assert!(h.join("Мои виджеты").is_dir(), "своя папка остаётся");
        let _ = std::fs::remove_dir_all(&h);
    }

    /// Свой запущенный виджет: гаснут только его процессы (по номерам, не по
    /// имени — второй conky с тем же именем папки в другом месте и соседний
    /// свой виджет живы), его автозапуск снят, соседний цел, папка — в
    /// корзине. Путь не своего виджета — отказ, ничего не гаснет.
    #[test]
    fn running_own_widget_stops_only_itself_and_goes_to_trash() {
        let h = tmp("rm-own-running");
        put(&h, ".config/conky/Jhalak/Jhalak.conf", b"conky.text = [[x]]");
        put(&h, ".config/conky/Jhalak/sub/clock.conf", b"conky.text = [[y]]");
        put(&h, ".config/conky/Neighbour/conky.conf", b"conky.text = [[z]]");
        put(&h, ".conky/Jhalak/Jhalak.conf", b"conky.text = [[x]]");
        let c = Ctx { xdg_config_home: Some(h.join("cfg").to_string_lossy().into_owned()), ..ctx(&h) };
        let home = c.home.clone();
        let p = format!("{home}/.config/conky/Jhalak");
        assert_eq!(own_widget_name(&c, &p).as_deref(), Some("Jhalak"));
        assert_eq!(own_widget_name(&c, &format!("{p}/sub")), None, "вложенная — не виджет");
        assert_eq!(own_widget_name(&c, &format!("{home}/.conky/Jhalak")), None, "найденный — не свой");
        let (title, text) = running_removal_question(&p);
        assert!(!title.is_empty());
        assert!(text.contains("“Jhalak”"), "{text}");

        let auto = crate::de_apply::conky_autostart_path(&c, "Jhalak");
        let other_auto = crate::de_apply::conky_autostart_path(&c, "Neighbour");
        put(Path::new("/"), &auto, b"[Desktop Entry]\n");
        put(Path::new("/"), &other_auto, b"[Desktop Entry]\n");
        let bench = Bench::new(
            &p,
            &[
                ("51", &format!("conky\0-c\0{p}/Jhalak.conf\0")),
                ("52", &format!("conky\0-c\0{p}/sub/clock.conf\0")),
                ("53", &format!("conky\0-c\0{home}/.config/conky/Neighbour/conky.conf\0")),
                ("54", &format!("conky\0-c\0{home}/.conky/Jhalak/Jhalak.conf\0")),
            ],
        );
        let trash = h.join("data/Trash");

        // Не свой виджет — отказ, процессы не тронуты.
        let (ok, msg) = remove_running_own(&bench, &c, &format!("{home}/.conky/Jhalak"), &trash, "2026-10-20T10:00:00");
        assert!(!ok);
        assert_eq!(msg, found_removal_refused());
        assert!(bench.kills.borrow().is_empty());

        let (ok, msg) = remove_running_own(&bench, &c, &p, &trash, "2026-10-20T10:00:00");
        assert!(ok, "{msg}");
        assert!(msg.contains("Jhalak"), "{msg}");
        assert_eq!(
            *bench.kills.borrow(),
            vec![vec!["kill".to_string(), "51".to_string(), "52".to_string()]],
            "гаснут только номера этого виджета"
        );
        assert_eq!(bench.present_at_kill.get(), Some(true), "погашен до переноса");
        assert!(!Path::new(&p).exists());
        assert!(trash.join("files/Jhalak/Jhalak.conf").is_file());
        assert!(!Path::new(&auto).exists(), "автозапуск снят");
        assert!(Path::new(&other_auto).exists(), "автозапуск соседа цел");
        assert!(h.join(".config/conky/Neighbour/conky.conf").is_file());
        assert!(h.join(".conky/Jhalak/Jhalak.conf").is_file());
        let _ = std::fs::remove_dir_all(&h);
    }
}
