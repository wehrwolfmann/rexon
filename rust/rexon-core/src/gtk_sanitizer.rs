//! Порт `rexon/qt_appearance/gtk_sanitizer.py` — санитайзер GTK/KDE-конфигов.
//!
//! Зачем модуль вообще есть: после применения Plasma Global Theme в
//! `gtk-{3,4}.0/settings.ini` и `kdeglobals` остаются ссылки на темы, которых
//! в системе нет. GTK4 4.22 на таких ссылках уходит в рекурсию резолва и
//! падает SIGSEGV — вместе с Firefox и любым GTK4-приложением. Санитайзер
//! ищет битые ссылки и подменяет их безопасным дефолтом.
//!
//! Здесь — ЧИСТАЯ логика: разбор ini-текста, правила «тема существует /
//! курсорная тема безопасна», выбор дефолта, формирование НОВОГО текста
//! `settings.ini`, план правок `kdeglobals`/`kcminputrc` и тексты журнала.
//! Наблюдения файловой системы приходят через трейт [`GtkFs`] (ленивый: ровно
//! те же обращения к ФС, что делал оригинал), а сами записи на диск и запуск
//! `kwriteconfig6` остаются исполнителю — ядро ничего не пишет.
//!
//! ⚠️ ЭТОТ путь КАНОНИЧЕН: в нём починены три дефекта оригинала (каждый
//! помечен `Починка:` по месту) —
//!   1. пути `~/.local/share/{icons,sounds,themes,color-schemes}` строились
//!      от `Path.home()`, минуя `XDG_DATA_HOME`;
//!   2. строка журнала называлась просто `settings.ini` — правка gtk-3.0 и
//!      gtk-4.0 выглядела одинаково;
//!   3. в `kdeglobals` пустой дефолт писался как значение (в ветке GTK такая
//!      защита была).
//!
//! Сверка с живым Python — `rust/rexon-core/tests/gtk_sanitizer_parity.rs`
//! (на РЕАЛЬНЫХ конфигах и каталогах тем этой машины, только чтение) и
//! `rust/rexon-py/tests/gtk_sanitizer_roundtrip.py`.

use std::collections::HashMap;

use crate::i18n::{tr, trf};
use crate::installed_scan::FsSnapshot;

// ============================================================================
// Наблюдения файловой системы
// ============================================================================

/// Три операции ФС, которых хватает санитайзеру. `&mut self` — чтобы живой
/// исполнитель мог запомнить свою первую ошибку и поднять её после (трейт
/// синхронный и без `Result`, как у `WaywallenIo`).
pub trait GtkFs {
    fn is_dir(&mut self, path: &str) -> bool;
    fn is_file(&mut self, path: &str) -> bool;
    /// `None` — файла нет либо чтение не удалось (аналог `except OSError`).
    fn read_text(&mut self, path: &str) -> Option<String>;
}

impl GtkFs for FsSnapshot {
    fn is_dir(&mut self, path: &str) -> bool {
        FsSnapshot::is_dir(self, path)
    }
    fn is_file(&mut self, path: &str) -> bool {
        FsSnapshot::is_file(self, path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        FsSnapshot::read_text(self, path).map(str::to_string)
    }
}

// ============================================================================
// Таблицы дефолтов и путей
// ============================================================================

/// Типы тем в порядке проверки — порт ключей `SAFE_DEFAULTS`.
pub const THEME_TYPES: &[&str] = &["icon-theme", "cursor-theme", "gtk-theme", "sound-theme"];

/// Небезопасные курсорные темы. ЕДИНЫЙ список на весь проект — реэкспорт
/// [`crate::themes::UNSAFE_CURSOR_THEMES`], чтобы pre-flight применения темы
/// и санитайзер не могли разойтись.
pub use crate::themes::UNSAFE_CURSOR_THEMES;

/// Порт `SAFE_DEFAULTS` — крайний случай, когда ни один fallback не найден.
pub fn safe_default(theme_type: &str) -> &'static str {
    match theme_type {
        "icon-theme" => "breeze",
        "cursor-theme" => "breeze_cursors",
        "gtk-theme" => "Adwaita",
        "sound-theme" => "ocean",
        _ => "",
    }
}

/// Порт `DEFAULT_FALLBACKS` — кандидаты в порядке предпочтения.
pub fn default_fallbacks(theme_type: &str) -> &'static [&'static str] {
    match theme_type {
        "icon-theme" => &["breeze", "Adwaita", "hicolor"],
        "cursor-theme" => &["breeze_cursors", "Adwaita", "default"],
        "gtk-theme" => &["Adwaita", "Breeze", "Adwaita-dark"],
        "sound-theme" => &["ocean", "freedesktop"],
        _ => &[],
    }
}

/// Порт `THEME_DIRS` — где искать тему данного типа, в порядке приоритета.
///
/// `data_home` — корень пользовательских данных (`$XDG_DATA_HOME` либо
/// `~/.local/share`). ⚠️ Починка: оригинал строил эти пути от `Path.home()`
/// литералом, минуя XDG. При нестандартном `XDG_DATA_HOME` (Flatpak,
/// Steam-runtime, кастомный профиль) санитайзер не находил темы, которые
/// РЕАЛЬНО стоят у пользователя, объявлял рабочую ссылку битой и подменял её
/// дефолтом — то есть сам ломал оформление, вместо того чтобы его починить.
/// `~/.icons` и `~/.themes` — не XDG-каталоги, они остаются от `home`.
pub fn theme_dirs(theme_type: &str, home: &str, data_home: &str) -> Vec<String> {
    let home = home.trim_end_matches('/');
    let data = data_home.trim_end_matches('/');
    match theme_type {
        // `~/.icons` GTK читает и для иконок, не только для курсоров: без
        // него набор, лежащий там, санитайзер объявлял несуществующим и
        // подменял по умолчанию (16.10.2026).
        "icon-theme" => vec![
            format!("{home}/.icons"),
            format!("{data}/icons"),
            "/usr/share/icons".to_string(),
        ],
        "cursor-theme" => vec![
            format!("{home}/.icons"),
            format!("{data}/icons"),
            "/usr/share/icons".to_string(),
        ],
        "gtk-theme" => vec![
            format!("{home}/.themes"),
            format!("{data}/themes"),
            "/usr/share/themes".to_string(),
        ],
        "sound-theme" => vec![format!("{data}/sounds"), "/usr/share/sounds".to_string()],
        _ => Vec::new(),
    }
}

/// Каталоги, в которых ищется `cursors/` родителя из `Inherits=` — порт
/// пары из `_is_cursor_theme_chain_safe` (та же починка XDG, что в
/// [`theme_dirs`]).
pub fn cursor_parent_dirs(data_home: &str) -> Vec<String> {
    vec![
        "/usr/share/icons".to_string(),
        format!("{}/icons", data_home.trim_end_matches('/')),
    ]
}

// ============================================================================
// Существование и безопасность темы
// ============================================================================

/// Порт `_is_cursor_theme_chain_safe`: не уведёт ли `Inherits=` курсорной
/// темы GTK4 в рекурсию. Безопасна, если есть `cursor.theme`; либо нет
/// `index.theme`; либо `index.theme` не читается; либо нет `Inherits=`; либо
/// хоть один не-`hicolor` родитель имеет свой `cursors/`.
///
/// То же правило уже портировано как [`crate::themes::is_cursor_theme_chain_safe`]
/// (pre-flight применения курсора, поверх `FsSnapshot`). Разница только в
/// абстракции ФС; согласие двух реализаций проверяется в
/// `gtk_sanitizer_parity.rs`, чтобы правила не разъехались.
pub fn is_cursor_theme_chain_safe<F: GtkFs + ?Sized>(
    fs: &mut F,
    theme_dir: &str,
    data_home: &str,
) -> bool {
    if fs.is_file(&format!("{theme_dir}/cursor.theme")) {
        return true;
    }
    let idx = format!("{theme_dir}/index.theme");
    if !fs.is_file(&idx) {
        return true; // самодостаточная, без наследования
    }
    let Some(content) = fs.read_text(&idx) else {
        return true; // не прочли — считаем безопасной (как оригинал)
    };
    let mut parents: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Inherits=") {
            parents = rest.split(',').map(|p| p.trim().to_string()).collect();
            break;
        }
    }
    if parents.is_empty() {
        return true;
    }
    let bases = cursor_parent_dirs(data_home);
    for parent in &parents {
        if parent == "hicolor" {
            // Для иконок это терминатор, для курсоров — тупик: своего
            // `cursors/` у hicolor нет никогда.
            continue;
        }
        for base in &bases {
            if fs.is_dir(&format!("{base}/{parent}/cursors")) {
                return true;
            }
        }
    }
    false
}

/// Порт `_theme_exists`: есть ли тема физически (и безопасна ли она, если
/// это курсор). Пустое имя — `false`.
pub fn theme_exists<F: GtkFs + ?Sized>(
    fs: &mut F,
    theme_type: &str,
    name: &str,
    home: &str,
    data_home: &str,
) -> bool {
    if name.is_empty() {
        return false;
    }
    if theme_type == "cursor-theme" && UNSAFE_CURSOR_THEMES.contains(&name) {
        return false;
    }
    for base in theme_dirs(theme_type, home, data_home) {
        if !fs.is_dir(&base) {
            continue;
        }
        let candidate = format!("{base}/{name}");
        if !fs.is_dir(&candidate) {
            continue;
        }
        if theme_type == "cursor-theme" {
            if !fs.is_dir(&format!("{candidate}/cursors")) {
                continue; // не курсорная тема в этом каталоге — ищем дальше
            }
            return is_cursor_theme_chain_safe(fs, &candidate, data_home);
        }
        return true;
    }
    false
}

/// Порт `_pick_safe_default`: первый существующий fallback, иначе
/// [`safe_default`] (даже если его в системе нет — крайний случай).
pub fn pick_safe_default<F: GtkFs + ?Sized>(
    fs: &mut F,
    theme_type: &str,
    home: &str,
    data_home: &str,
) -> String {
    for fallback in default_fallbacks(theme_type) {
        if theme_exists(fs, theme_type, fallback, home, data_home) {
            return (*fallback).to_string();
        }
    }
    safe_default(theme_type).to_string()
}

// ============================================================================
// gtk-{3,4}.0/settings.ini
// ============================================================================

/// Версии GTK, чьи `settings.ini` санируются — порт кортежа в `sanitize_all`.
pub const GTK_VERSIONS: &[&str] = &["3.0", "4.0"];

/// Путь `settings.ini` данной версии GTK. `config_home` — `$XDG_CONFIG_HOME`
/// либо `~/.config` (см. [`crate::utils::config_home`]).
pub fn gtk_ini_path(config_home: &str, version: &str) -> String {
    format!("{}/gtk-{version}/settings.ini", config_home.trim_end_matches('/'))
}

/// Порт `_read_ini_settings`: секция `[Settings]` из текста `settings.ini`.
/// `None` (файла нет / не прочли) — пустая карта. Ключи и значения
/// обрезаются по краям, строки-комментарии (`#`) и пустые пропускаются.
pub fn read_ini_settings(text: Option<&str>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(text) = text else {
        return result;
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if section == "Settings" {
            if let Some((k, v)) = line.split_once('=') {
                result.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    result
}

/// Порт `_write_ini_setting` — НОВЫЙ текст файла (запись делает исполнитель).
///
/// `existing = None` — файла нет: получаем минимальный `[Settings]`-файл.
/// Иначе значение заменяется в существующей секции `[Settings]`; если ключа
/// там не было — дописывается в конец секции; если самой секции нет — секция
/// создаётся в конце файла.
pub fn write_ini_setting(existing: Option<&str>, key: &str, value: &str) -> String {
    write_ini_setting_in(existing, "Settings", key, value)
}

/// То же для ЛЮБОЙ секции ini-файла.
///
/// Правило #12: тот же движок обслуживает и `settings.ini` GTK, и
/// `kdeglobals` KDE — второй копии «найти секцию, заменить или дописать» в
/// проекте быть не должно. Прежняя копия во вкладке «Настройки»
/// (`_kde_set_terminal_konsole`, `_patch_gtk_ini`) дописывала ключ по индексу
/// «строка после первой найденной секции» и при отсутствии нужной секции в
/// начале файла попадала в чужую.
pub fn write_ini_setting_in(
    existing: Option<&str>,
    section: &str,
    key: &str,
    value: &str,
) -> String {
    let Some(text) = existing else {
        return format!("[{section}]\n{key}={value}\n");
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut replaced = false;
    let prefix = format!("{key}=");
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            // Секция кончилась, а ключа в ней не было — дописываем перед
            // заголовком следующей.
            if in_section && !replaced {
                out.push(format!("{key}={value}"));
                replaced = true;
            }
            in_section = &stripped[1..stripped.len() - 1] == section;
            out.push(line.to_string());
            continue;
        }
        if in_section && stripped.starts_with(&prefix) {
            out.push(format!("{key}={value}"));
            replaced = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !replaced {
        if !in_section {
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key}={value}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Ключ `settings.ini` → тип темы, в порядке проверки — порт `keys_to_check`.
pub const INI_KEYS: &[(&str, &str)] = &[
    ("gtk-icon-theme-name", "icon-theme"),
    ("gtk-cursor-theme-name", "cursor-theme"),
    ("gtk-theme-name", "gtk-theme"),
    ("gtk-sound-theme-name", "sound-theme"),
];

/// Типы тем, ссылку на которые санитайзер НЕ заменяет, даже если темы нет.
///
/// ⚠️ Починка (13.10.2026): звуковая тема подменялась на `ocean` наравне со
/// значками и курсорами. Причины оставить её как есть:
///   * отсутствующая тема звуков ничего не роняет — libcanberra и KDE молча
///     играют `freedesktop`; санитайзер существует ради падений GTK4 на
///     битых темах значков и курсоров, к звукам это не относится;
///   * правилась только пара `gtk-{3,4}.0/settings.ini`, а `kdeglobals`
///     `[Sounds] Theme`, gsettings и `.gtkrc-2.0` оставались прежними —
///     настройка расходилась сама с собой;
///   * в KDE служба `gtkconfig` (kded) при каждом входе в сеанс переносит
///     `[Sounds] Theme` в файлы GTK — подмена жила до следующего входа и
///     повторялась при первом же запуске окна, значение «прыгало»;
///   * выбор человека молча менялся на тему, которую он не выбирал.
///
/// О такой ссылке санитайзер только сообщает ([`plan_gtk_ini_notes`]); сам
/// раздел «Звуки событий» показывает её как отсутствующую тему.
pub const KEPT_TYPES: &[&str] = &["sound-theme"];

/// Есть ли звуковая тема `name`: `<каталог>/<name>/index.theme` с разделом
/// `[Sound Theme]` — то же правило, что у применения
/// ([`crate::qt_settings::is_sound_theme_dir`]), поверх [`GtkFs`]. Голый
/// каталог без `index.theme` (обёртка архива, `/usr/share/sounds/mate`) —
/// не тема.
pub fn sound_theme_exists<F: GtkFs + ?Sized>(
    fs: &mut F,
    name: &str,
    home: &str,
    data_home: &str,
) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return false;
    }
    theme_dirs("sound-theme", home, data_home).iter().any(|base| {
        fs.read_text(&format!("{base}/{name}/index.theme"))
            .map(|t| t.lines().any(|l| l.trim() == "[Sound Theme]"))
            .unwrap_or(false)
    })
}

/// Замечания по ключам из [`KEPT_TYPES`]: ссылка ведёт на тему, которой нет,
/// но значение ОСТАВЛЕНО как есть. Строки готовы для журнала; файл не
/// меняется ни здесь, ни исполнителем.
pub fn plan_gtk_ini_notes<F: GtkFs + ?Sized>(
    fs: &mut F,
    ini_path: &str,
    settings: &HashMap<String, String>,
    home: &str,
    data_home: &str,
) -> Vec<String> {
    let label = ini_label(ini_path);
    let mut notes = Vec::new();
    for (key, theme_type) in INI_KEYS {
        if *theme_type != "sound-theme" {
            continue;
        }
        let value = settings.get(*key).map(|v| v.trim()).unwrap_or("");
        if value.is_empty() || sound_theme_exists(fs, value, home, data_home) {
            continue;
        }
        // TRANSLATORS: %1 is a config file and key, e.g.
        // "gtk-3.0/settings.ini: gtk-sound-theme-name"; %2 is a theme name.
        notes.push(trf(
            "%1: sound theme “%2” is not installed; the setting was left as is",
            &[&format!("{label}: {key}"), &value],
        ));
    }
    notes
}

/// Одна правка `settings.ini`: что записать и что об этом сказать в журнал.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniEdit {
    pub key: String,
    pub value: String,
    /// Строка журнала — добавляется вызывающим ТОЛЬКО если запись удалась.
    pub change: String,
}

/// Имя для журнала: `gtk-3.0/settings.ini`.
///
/// ⚠️ Починка: оригинал брал `ini_path.name`, то есть всегда просто
/// `settings.ini`. Правки gtk-3.0 и gtk-4.0 в журнале и в отчёте UI были
/// неотличимы — при разборе жалобы «Firefox всё ещё падает» нельзя было
/// понять, какой из двух конфигов уже починен.
pub fn ini_label(ini_path: &str) -> String {
    let mut it = ini_path.rsplitn(3, '/');
    let name = it.next().unwrap_or("");
    // Пустой родитель — файл в корне либо без каталога вовсе: `Path.parent.name`
    // в этих случаях тоже пуст, и остаётся одно имя файла.
    match it.next() {
        Some(parent) if !parent.is_empty() => format!("{parent}/{name}"),
        _ => name.to_string(),
    }
}

/// Порт решений `sanitize_gtk_settings_ini`: какие ключи битые и чем их
/// заменить. Пустое/отсутствующее значение и живая тема пропускаются; если
/// подставить нечего — правки нет. Ключи типов из [`KEPT_TYPES`] (звуковая
/// тема) не правятся никогда — о них говорит [`plan_gtk_ini_notes`].
///
/// Записи здесь НЕ происходит: исполнитель для каждой правки зовёт
/// [`write_ini_setting`] на СВЕЖЕМ содержимом файла и пишет результат — ровно
/// как оригинал, который перечитывал файл на каждый ключ.
pub fn plan_gtk_ini_edits<F: GtkFs + ?Sized>(
    fs: &mut F,
    ini_path: &str,
    settings: &HashMap<String, String>,
    home: &str,
    data_home: &str,
) -> Vec<IniEdit> {
    let label = ini_label(ini_path);
    let mut edits = Vec::new();
    for (key, theme_type) in INI_KEYS {
        if KEPT_TYPES.contains(theme_type) {
            continue;
        }
        let value = settings.get(*key).map(|v| v.trim()).unwrap_or("");
        if value.is_empty() {
            continue;
        }
        if theme_exists(fs, theme_type, value, home, data_home) {
            continue;
        }
        let safe = pick_safe_default(fs, theme_type, home, data_home);
        if safe.is_empty() {
            continue;
        }
        edits.push(IniEdit {
            key: (*key).to_string(),
            value: safe.clone(),
            change: format!("{label}: {key}: '{value}' → '{safe}'"),
        });
    }
    edits
}

// ============================================================================
// kdeglobals / kcminputrc
// ============================================================================

/// Порт `_read_kdeglobals`: `{секция: {ключ: значение}}`. Ключи до первой
/// секции отбрасываются, комментарии (`#`) и пустые строки пропускаются.
pub fn read_kde_ini(text: Option<&str>) -> HashMap<String, HashMap<String, String>> {
    let mut result: HashMap<String, HashMap<String, String>> = HashMap::new();
    let Some(text) = text else {
        return result;
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            result.entry(section.clone()).or_default();
        } else if !section.is_empty() {
            if let Some((k, v)) = line.split_once('=') {
                result
                    .entry(section.clone())
                    .or_default()
                    .insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    result
}

/// Значение `секция/ключ` либо `""` — порт `config.get(s, {}).get(k, "")`.
pub fn kde_value(cfg: &HashMap<String, HashMap<String, String>>, section: &str, key: &str) -> String {
    cfg.get(section)
        .and_then(|m| m.get(key))
        .cloned()
        .unwrap_or_default()
}

/// Порт `_read_kcminput_cursor`: первый `cursorTheme=` в секции `[Mouse]`.
/// Комментарии здесь НЕ пропускаются — как в оригинале.
pub fn read_kcminput_cursor(text: Option<&str>) -> String {
    let Some(text) = text else {
        return String::new();
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if section == "Mouse" {
            if let Some(rest) = line.strip_prefix("cursorTheme=") {
                return rest.trim().to_string();
            }
        }
    }
    String::new()
}

/// Порт `_colorscheme_exists`: где лежат `.colors` цветовой схемы (та же
/// починка XDG, что в [`theme_dirs`]).
pub fn colorscheme_paths(name: &str, data_home: &str) -> Vec<String> {
    vec![
        format!("{}/color-schemes/{name}.colors", data_home.trim_end_matches('/')),
        format!("/usr/share/color-schemes/{name}.colors"),
    ]
}

/// Порт `_colorscheme_exists`.
pub fn colorscheme_exists<F: GtkFs + ?Sized>(fs: &mut F, name: &str, data_home: &str) -> bool {
    colorscheme_paths(name, data_home)
        .iter()
        .any(|p| fs.is_file(p))
}

/// Дефолт цветовой схемы, когда текущей нет — порт литерала `"BreezeDark"`.
pub const COLORSCHEME_DEFAULT: &str = "BreezeDark";

/// Таймаут запуска `kwriteconfig6` (секунды) — как `timeout=10` в Python.
pub const KWRITECONFIG_TIMEOUT: u64 = 10;

/// Одна правка KDE-конфига: что передать `kwriteconfig6` и что сказать
/// журналу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdeEdit {
    pub file: String,
    pub group: String,
    pub key: String,
    pub value: String,
    /// Строка журнала — добавляется ТОЛЬКО если `kwriteconfig6` отработал.
    pub change: String,
}

/// argv записи в KDE-конфиг — порт `_kwriteconfig`. `tool` — уже разрешённый
/// бинарь (`kwriteconfig6`/`kwriteconfig5`, см. `rexon.utils.resolve_tool`).
pub fn kwriteconfig_argv(tool: &str, edit: &KdeEdit) -> Vec<String> {
    vec![
        tool.to_string(),
        "--file".to_string(),
        edit.file.clone(),
        "--group".to_string(),
        edit.group.clone(),
        "--key".to_string(),
        edit.key.clone(),
        edit.value.clone(),
    ]
}

/// Порт решений `sanitize_kdeglobals`: ColorScheme, иконки и курсор в
/// `kdeglobals` плюс курсор в `kcminputrc` (Plasma 6 предпочитает его).
///
/// ⚠️ Починка: оригинал проверял «нечего подставить» только в GTK-ветке, а в
/// KDE-ветке писал что дали — пустой дефолт уезжал в конфиг значением и
/// Plasma получала `Theme=`/`cursorTheme=` без темы вовсе. Здесь пустой
/// дефолт правку отменяет, как и для GTK.
pub fn plan_kde_edits<F: GtkFs + ?Sized>(
    fs: &mut F,
    kdeglobals: &HashMap<String, HashMap<String, String>>,
    kcminput_cursor: &str,
    home: &str,
    data_home: &str,
) -> Vec<KdeEdit> {
    let mut edits = Vec::new();

    // 1. ColorScheme в [General] — у него дефолт литеральный, не по ФС.
    let cs = kde_value(kdeglobals, "General", "ColorScheme");
    if !cs.is_empty() && !colorscheme_exists(fs, &cs, data_home) {
        edits.push(KdeEdit {
            file: "kdeglobals".into(),
            group: "General".into(),
            key: "ColorScheme".into(),
            value: COLORSCHEME_DEFAULT.into(),
            change: format!(
                "kdeglobals[General]ColorScheme: '{cs}' → '{COLORSCHEME_DEFAULT}'"
            ),
        });
    }

    // 2. Icon Theme в [Icons]
    let icons = kde_value(kdeglobals, "Icons", "Theme");
    if !icons.is_empty() && !theme_exists(fs, "icon-theme", &icons, home, data_home) {
        let safe = pick_safe_default(fs, "icon-theme", home, data_home);
        if !safe.is_empty() {
            edits.push(KdeEdit {
                file: "kdeglobals".into(),
                group: "Icons".into(),
                key: "Theme".into(),
                value: safe.clone(),
                change: format!("kdeglobals[Icons]Theme: '{icons}' → '{safe}'"),
            });
        }
    }

    // 3. Cursor в [Mouse] — kdeglobals
    let cursor = kde_value(kdeglobals, "Mouse", "cursorTheme");
    if !cursor.is_empty() && !theme_exists(fs, "cursor-theme", &cursor, home, data_home) {
        let safe = pick_safe_default(fs, "cursor-theme", home, data_home);
        if !safe.is_empty() {
            edits.push(KdeEdit {
                file: "kdeglobals".into(),
                group: "Mouse".into(),
                key: "cursorTheme".into(),
                value: safe.clone(),
                change: format!("kdeglobals[Mouse]cursorTheme: '{cursor}' → '{safe}'"),
            });
        }
    }

    // 4. Cursor в [Mouse] — kcminputrc
    if !kcminput_cursor.is_empty()
        && !theme_exists(fs, "cursor-theme", kcminput_cursor, home, data_home)
    {
        let safe = pick_safe_default(fs, "cursor-theme", home, data_home);
        if !safe.is_empty() {
            edits.push(KdeEdit {
                file: "kcminputrc".into(),
                group: "Mouse".into(),
                key: "cursorTheme".into(),
                value: safe.clone(),
                change: format!(
                    "kcminputrc[Mouse]cursorTheme: '{kcminput_cursor}' → '{safe}'"
                ),
            });
        }
    }

    edits
}

// ============================================================================
// Отчёт
// ============================================================================

/// Порт печати `sanitize_all(verbose=True)` — весь текст целиком (с
/// завершающим переводом строки), чтобы исполнителю осталось одно `print`.
pub fn verbose_report(changes: &[String]) -> String {
    if changes.is_empty() {
        return tr("✅ All GTK/KDE configs are OK, nothing was fixed") + "\n";
    }
    let mut out = tr("✅ Fixed broken links:") + "\n";
    for c in changes {
        out.push_str("  • ");
        out.push_str(c);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(v: serde_json::Value) -> FsSnapshot {
        FsSnapshot::from_json(&v)
    }

    #[test]
    fn ini_label_берёт_каталог_версии() {
        assert_eq!(ini_label("/home/u/.config/gtk-3.0/settings.ini"), "gtk-3.0/settings.ini");
        assert_eq!(ini_label("settings.ini"), "settings.ini");
    }

    #[test]
    fn write_ini_setting_создаёт_файл() {
        assert_eq!(
            write_ini_setting(None, "gtk-theme-name", "Adwaita"),
            "[Settings]\ngtk-theme-name=Adwaita\n"
        );
    }

    #[test]
    fn write_ini_setting_дописывает_ключ_в_конец_секции() {
        let src = "[Settings]\na=1\n\n[Other]\nb=2\n";
        let out = write_ini_setting(Some(src), "gtk-theme-name", "Breeze");
        assert_eq!(out, "[Settings]\na=1\n\ngtk-theme-name=Breeze\n[Other]\nb=2\n");
    }

    #[test]
    fn write_ini_setting_создаёт_секцию_если_её_нет() {
        let out = write_ini_setting(Some("[Other]\nb=2\n"), "k", "v");
        assert_eq!(out, "[Other]\nb=2\n[Settings]\nk=v\n");
    }

    #[test]
    fn пустой_дефолт_не_попадает_в_kde_правку() {
        // Ни одной иконочной темы в системе → pick_safe_default отдаст
        // литерал "breeze"; при пустом типе — пустую строку, правки нет.
        let mut fs = snap(json!({"dirs": {}, "files": {}}));
        assert_eq!(pick_safe_default(&mut fs, "нет-такого", "/h", "/h/.local/share"), "");
    }

    /// Живой случай 13.10.2026: `gtk-sound-theme-name=Sounds`, темы «Sounds»
    /// нет. Значение не правится (в плане правок его нет), о нём — замечание.
    #[test]
    fn звуковая_тема_которой_нет_не_подменяется() {
        let mut fs = snap(json!({
            "dirs": {
                "/usr/share/sounds": ["ocean", "mate"],
                "/usr/share/sounds/ocean": ["index.theme"],
                "/usr/share/sounds/mate": [],
                "/h/.local/share/sounds": ["Zorin"],
                "/h/.local/share/sounds/Zorin": ["index.theme"],
                "/usr/share/icons": ["breeze"],
                "/usr/share/icons/breeze": []
            },
            "files": {
                "/usr/share/sounds/ocean/index.theme": "[Sound Theme]\nName=Ocean\n",
                "/h/.local/share/sounds/Zorin/index.theme": "[Sound Theme]\nName=Zorin \n"
            }
        }));
        let ini = "/h/.config/gtk-3.0/settings.ini";
        let settings: HashMap<String, String> = [
            ("gtk-sound-theme-name", "Sounds"),
            ("gtk-icon-theme-name", "НетЗначков"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let edits = plan_gtk_ini_edits(&mut fs, ini, &settings, "/h", "/h/.local/share");
        let keys: Vec<&str> = edits.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["gtk-icon-theme-name"], "значки чинятся, звук — нет");

        let notes = plan_gtk_ini_notes(&mut fs, ini, &settings, "/h", "/h/.local/share");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("gtk-3.0/settings.ini: gtk-sound-theme-name"), "{}", notes[0]);
        assert!(notes[0].contains("Sounds"), "{}", notes[0]);
    }

    /// Существующая тема (своя или системная) и пустое значение — без
    /// замечаний; каталог без `index.theme` темой не считается.
    #[test]
    fn замечание_о_звуковой_теме_только_когда_темы_нет() {
        let mut fs = snap(json!({
            "dirs": {
                "/usr/share/sounds": ["ocean", "mate"],
                "/usr/share/sounds/ocean": ["index.theme"],
                "/usr/share/sounds/mate": [],
                "/d/sounds": ["Zorin"],
                "/d/sounds/Zorin": ["index.theme"]
            },
            "files": {
                "/usr/share/sounds/ocean/index.theme": "[Sound Theme]\nName=Ocean\n",
                "/d/sounds/Zorin/index.theme": "[Sound Theme]\nName=Zorin\n"
            }
        }));
        let notes_for = |fs: &mut FsSnapshot, value: &str| {
            let settings: HashMap<String, String> =
                [("gtk-sound-theme-name".to_string(), value.to_string())].into_iter().collect();
            plan_gtk_ini_notes(fs, "/h/.config/gtk-4.0/settings.ini", &settings, "/h", "/d")
        };
        for ok in ["ocean", "Zorin", ""] {
            assert!(notes_for(&mut fs, ok).is_empty(), "«{ok}» — замечания быть не должно");
        }
        for bad in ["Sounds", "mate", "Sounds/Zorin", ".."] {
            assert_eq!(notes_for(&mut fs, bad).len(), 1, "«{bad}» — темы нет");
        }
    }

    #[test]
    fn unsafe_курсор_считается_битым() {
        let mut fs = snap(json!({
            "dirs": {"/usr/share/icons": [], "/usr/share/icons/Adwaita": [],
                     "/usr/share/icons/Adwaita/cursors": []},
            "files": {}
        }));
        assert!(!theme_exists(&mut fs, "cursor-theme", "Adwaita", "/h", "/h/.local/share"));
    }
}
