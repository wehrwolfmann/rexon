//! Сканер РЕАЛЬНО установленных тем оформления — порт
//! `rexon/appearance/installed_scan.py`.
//!
//! Горячий модуль локальных категорий вкладки «Оформление» (подпереключатель
//! «Активные | Скачанные | Системные» + под-выборы «Стиль приложений» /
//! «Kvantum» / «Сплэш» / «Экран блокировки»). Возвращает `Vec<ThemeItem>`
//! с category_id реальной OCS-категории — кнопка «Применить» переиспользует
//! installer.apply() как есть.
//!
//! Принцип порта (как system.rs): ФАЙЛОВАЯ СИСТЕМА — ЗА ИНТЕРФЕЙСОМ.
//! Вход сканеров — `FsSnapshot`: снимок дерева каталогов (порядок имён =
//! порядок os.scandir, который видел Python-эталон) + содержимое файлов
//! метаданных. Разбор конфигов (metadata.json / metadata.desktop /
//! index.theme / *.colors / gtk settings.ini / kvantum.kvconfig /
//! sddm-Current / plymouth-Theme / вывод pacman -Qm/-Qo) — чистые функции.
//!
//! Побочные зависимости Python вынесены в параметры:
//!   * `_active_map()` (ThemeManager + kreadconfig6 + конфиги) — карта
//!     подаётся готовой; её сборка из ГОТОВЫХ значений источников — чистая
//!     `active_map_from_sources` (сверена с Python на замоканных источниках);
//!   * pacman: `has_pacman` — bool, `pacman -Qm` — `foreign` (None =
//!     недоступен → `ScanUnavailable`, как SystemScanUnavailable),
//!     `pacman -Qo` — карта владельцев (парсер `parse_qo_output` чистый);
//!   * кеш магазина (`appearance.cache`) — список (name, preview_url) +
//!     замыкание `preview_file` (get_preview: url → локальный файл);
//!   * `QStyleFactory.keys()` — список стилей параметром (фолбэк —
//!     `WIDGET_STYLES`).
//!
//! НЕ портировано (исполнение — при подключении к живому Python): вызовы
//! pacman/kreadconfig6/QStyleFactory, ThemeManager (чтение kdeglobals/kwinrc),
//! ретраи `_foreign_pkgs` по db.lck (политика ретраев портирована чистой
//! функцией `foreign_pkgs_from_attempts`, сам sleep/subprocess — нет),
//! батчинг `_owner_pkgs` по 200 путей (парсер чистый, разрезание на пачки —
//! деталь вызова), SQL-кеш превью, логгирование.
//!
//! Golden-parity: `tests/installed_scan_parity.rs` — снимок реального дерева
//! тем машины Wehrwolfmann (+синтетика: битые metadata, тема без Name,
//! курсорная тема в icons) и эталоны с живого Python на той же песочнице,
//! списки тем сверяются ПОЛЕ-В-ПОЛЕ включая порядок.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::appearance_page::local_active_badge;
use crate::i18n::{n_, tr, trc};
use crate::models::ThemeItem;

// ============================================================================
// FsSnapshot — снимок файловой системы
// ============================================================================

/// Снимок дерева: каталог → имена элементов (в порядке os.scandir снимка),
/// файл → текстовое содержимое (бинарные превью в фикстурах — пустышки).
pub struct FsSnapshot {
    dirs: HashMap<String, Vec<String>>,
    files: HashMap<String, String>,
}

impl FsSnapshot {
    /// Из JSON-фикстуры `{"dirs": {path: [names]}, "files": {path: text}}`.
    pub fn from_json(v: &Value) -> Self {
        let mut dirs = HashMap::new();
        if let Some(obj) = v["dirs"].as_object() {
            for (k, names) in obj {
                let list = names
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|n| n.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                dirs.insert(k.clone(), list);
            }
        }
        let mut files = HashMap::new();
        if let Some(obj) = v["files"].as_object() {
            for (k, text) in obj {
                files.insert(k.clone(), text.as_str().unwrap_or("").to_string());
            }
        }
        FsSnapshot { dirs, files }
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.dirs.contains_key(path)
    }

    pub fn is_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn read_text(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Имена элементов каталога в порядке снимка ([] — каталога нет).
    pub fn scandir(&self, path: &str) -> &[String] {
        self.dirs.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn join(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

// ============================================================================
// Таблица типов — порт _Type / TYPES
// ============================================================================

/// apply_kind → (cat_id, подпапка, способ перечисления) — порт `_Type`.
pub struct ScanType {
    pub apply_kind: &'static str,
    pub cat_id: i64,
    pub label: &'static str,
    pub icon: &'static str,
    pub user_dir: String,
    pub sys_dir: String,
    /// "dir" | "colors" | "sub" (каталог темы с подкаталогом `probe`) |
    /// "file" (каталог темы с файлом `probe` в корне) |
    /// "files" (файлы с окончанием `probe`) | "meta" (каталог с
    /// метаданными) | "conky" (каталог виджета Conky).
    pub entry: &'static str,
    /// Уточнение к `entry` (имя подкаталога, окончание файла); обычно пусто.
    pub probe: &'static str,
}

/// Имя OCS-превью, которое Rexon кладёт рядом с темой при установке из
/// Магазина — порт `REXON_PREVIEW_NAME`.
pub const REXON_PREVIEW_NAME: &str = ".rexon-preview.png";

/// Стили-движки Qt (фолбэк без QStyleFactory) — порт `_WIDGET_STYLES`.
pub const WIDGET_STYLES: &[&str] = &[
    "Breeze", "Fusion", "kvantum", "kvantum-dark", "Windows", "Oxygen",
];

/// Доступные стили Qt — порт `_qstyle_factory`: живой список
/// `QStyleFactory::keys()` (встроенные стили плюс установленные плагины), а
/// если Qt ничего не ответил — запас [`WIDGET_STYLES`].
///
/// Живой список обязателен: запас показывал чипы стилей, которых в системе
/// нет (Oxygen без плагина), и не показывал установленные (qt6ct-style и
/// любые сторонние).
pub fn available_widget_styles(live_keys: &[String]) -> Vec<String> {
    let live: Vec<String> = live_keys
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if live.is_empty() {
        WIDGET_STYLES.iter().map(|s| s.to_string()).collect()
    } else {
        live
    }
}

/// Таблица TYPES для данного home/usr-корня (live: usr_root = "/usr").
pub fn types_for(home: &str, usr_root: &str) -> Vec<ScanType> {
    let t = |apply_kind, cat_id, label, icon, user: String, sys: String, entry| {
        ScanType { apply_kind, cat_id, label, icon, user_dir: user, sys_dir: sys, entry, probe: "" }
    };
    vec![
        t("global", 722, "Global Theme", "💎",
          format!("{home}/.local/share/plasma/look-and-feel"),
          format!("{usr_root}/share/plasma/look-and-feel"), "dir"),
        t("plasma", 104, "Plasma Style", "🖥️",
          format!("{home}/.local/share/plasma/desktoptheme"),
          format!("{usr_root}/share/plasma/desktoptheme"), "dir"),
        t("colors", 112, "Color Scheme", "🌈",
          format!("{home}/.local/share/color-schemes"),
          format!("{usr_root}/share/color-schemes"), "colors"),
        t("aurorae", 114, "Window Decorations (Aurorae)", "🪟",
          format!("{home}/.local/share/aurorae/themes"),
          format!("{usr_root}/share/aurorae/themes"), "dir"),
        t("icons", 132, "Icons", "📁",
          format!("{home}/.local/share/icons"),
          format!("{usr_root}/share/icons"), "dir"),
        t("cursor", 107, "Cursor", "🖱️",
          format!("{home}/.local/share/icons"),
          format!("{usr_root}/share/icons"), "dir"),
        t("kvantum", 123, "Kvantum", "🔲",
          format!("{home}/.config/Kvantum"),
          format!("{usr_root}/share/Kvantum"), "dir"),
        t("gtk", 135, "GTK Theme", "🎨",
          format!("{home}/.local/share/themes"),
          format!("{usr_root}/share/themes"), "dir"),
        // Темы звуков: без этой строки они не попадали ни в «Активные», ни
        // в «Загруженные», ни в «Системные» (живой случай 03.10.2026).
        t("sounds", 316, "Sound Theme", "🔔",
          format!("{home}/.local/share/sounds"),
          format!("{usr_root}/share/sounds"), "dir"),
        t("sddm", 101, "SDDM (Login Screen)", "🔐",
          String::new(), format!("{usr_root}/share/sddm/themes"), "dir"),
        t("plymouth", 108, "Plymouth", "⚡",
          String::new(), format!("{usr_root}/share/plymouth/themes"), "dir"),
    ]
}

/// Типы разделов окружений (`categories::DE_SECTIONS`), видимых в `de`
/// (пусто — все). Каталоги, способ перечисления и подпись — из матрицы и
/// таблицы типов: нового списка здесь нет (правило #12).
///
/// Темы рамок и оболочек (подкаталог в каталоге темы) ищутся и в
/// `~/.themes`, и в `~/.local/share/themes` — оба каталога читают сами
/// окружения, а установка из «Оформления» кладёт GTK-темы во второй.
pub fn de_types(home: &str, usr_root: &str, de: &str) -> Vec<ScanType> {
    use crate::de_customization::Payload;
    let mut out = Vec::new();
    for (cat_id, key) in crate::categories::DE_SECTIONS {
        if !de.is_empty() && !crate::categories::category_available(key, de) {
            continue;
        }
        let Some(kind) = crate::categories::category_kind(key) else { continue };
        let Some(row) = crate::de_customization::row(kind) else { continue };
        let (entry, probe) = match row.payload {
            Payload::ThemeSubdir { sub } => ("sub", sub),
            Payload::Files { ext } => ("files", ext),
            Payload::DirUuid { .. } | Payload::KPackage { .. } => ("meta", ""),
            Payload::Dir if kind == crate::de_apply::CONKY_KIND => ("conky", ""),
            // Тема LXQt — каталог с `lxqt-panel.qss`: без него LXQt её в
            // своём списке не показывает. Признак — тот же, по которому
            // установка ищет корни тем в архиве (один источник).
            Payload::Dir => match crate::appearance_installer::de_theme_marker(key) {
                Some(crate::appearance_installer::DeThemeMarker::File(file)) => ("file", file),
                _ => ("dir", ""),
            },
        };
        let label = crate::constants::type_labels_ordered()
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, l)| *l)
            .unwrap_or(kind);
        let icon = crate::constants::type_icon(kind).unwrap_or("🧩");
        let base = ScanType {
            apply_kind: key,
            cat_id: *cat_id,
            label,
            icon,
            user_dir: row.dest_path(home),
            sys_dir: crate::de_apply::system_dir(row, usr_root),
            entry,
            probe,
        };
        let xdg_themes = (entry == "sub").then(|| ScanType {
            user_dir: format!("{home}/.local/share/themes"),
            sys_dir: String::new(),
            ..base_clone(&base)
        });
        out.push(base);
        out.extend(xdg_themes);
    }
    out
}

/// Типы одного раздела для [`LiveScan::section`]: раздел окружения — из
/// [`de_types`], раздел KDE — строка [`types_for`] плюс прежние каталоги,
/// которые читают и KDE, и GTK: `~/.icons` у значков и курсоров,
/// `~/.themes` у тем GTK (обзор «Загруженные» их не смотрит, а темы,
/// поставленные руками или чужими установщиками, лежат часто именно там).
pub fn section_types(home: &str, usr_root: &str, apply_kind: &str) -> Vec<ScanType> {
    let mut types: Vec<ScanType> = types_for(home, usr_root)
        .into_iter()
        .chain(de_types(home, usr_root, ""))
        .filter(|t| t.apply_kind == apply_kind)
        .collect();
    let legacy = match apply_kind {
        "icons" | "cursor" => Some(format!("{home}/.icons")),
        "gtk" => Some(format!("{home}/.themes")),
        _ => None,
    };
    if let (Some(dir), Some(first)) = (legacy, types.first()) {
        let extra = ScanType { user_dir: dir, sys_dir: String::new(), ..base_clone(first) };
        types.push(extra);
    }
    types
}

/// Отметка каталогов, которые читают обзоры диска: время изменения каждого
/// корня (каталоги тем всех видов и окружений, `~/.themes`, `~/.icons`,
/// Kvantum). Новая или снятая тема в корне меняет время самого корня —
/// отметка другая, и кеш сканов страницы надо выбросить: иначе тема,
/// положенная мимо Rexon при открытом окне, не появлялась при повторном
/// заходе в раздел, только по «Обновить».
pub fn roots_stamp(home: &str, usr_root: &str) -> String {
    let mut roots: Vec<String> = Vec::new();
    let mut add = |d: &str| {
        if !d.is_empty() && !roots.iter().any(|r| r == d) {
            roots.push(d.to_string());
        }
    };
    for t in types_for(home, usr_root).iter().chain(de_types(home, usr_root, "").iter()) {
        add(&t.user_dir);
        add(&t.sys_dir);
    }
    for d in kvantum_dirs(home, usr_root) {
        add(&d);
    }
    add(&legacy_themes_dir(home));
    add(&format!("{home}/.icons"));
    roots
        .iter()
        .map(|d| {
            std::fs::metadata(d)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or_else(|| "-".to_string(), |t| t.as_nanos().to_string())
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn base_clone(t: &ScanType) -> ScanType {
    ScanType {
        apply_kind: t.apply_kind,
        cat_id: t.cat_id,
        label: t.label,
        icon: t.icon,
        user_dir: t.user_dir.clone(),
        sys_dir: t.sys_dir.clone(),
        entry: t.entry,
        probe: t.probe,
    }
}

// ── Ссылки в `~/.themes` на темы из `~/.local/share/themes` ─────────────
//
// «Оформление» ставит темы GTK в `~/.local/share/themes` (раздел `gtk`,
// `categories`). Его читают GTK 3 и 4, xfwm4 и окно оформления XFCE
// (libxfce4util), Cinnamon и его настройки, Openbox, labwc, User Themes
// GNOME и Marco 1.29 — но окно «Внешний вид» MATE (mate-control-center 1.28,
// `capplets/common/mate-theme-info.c`) смотрит только `~/.themes` и
// `$XDG_DATA_DIRS/themes`: такой темы в его списке нет, и выбрать её там
// нельзя (стенд 16.10.2026). Поэтому у каждой темы оттуда в `~/.themes`
// лежит ссылка с тем же именем. Ссылка относительная — переживает переезд
// домашнего каталога. Занятое имя не трогаем, ссылку на пропавшую тему
// снимаем; в списках Rexon ссылка не показывается — тема видна один раз,
// из своего настоящего каталога.

/// Каталог, куда «Оформление» ставит темы GTK.
pub fn xdg_themes_dir(home: &str) -> String {
    format!("{}/.local/share/themes", home.trim_end_matches('/'))
}

/// Прежний каталог тем, который читают все окружения.
pub fn legacy_themes_dir(home: &str) -> String {
    format!("{}/.themes", home.trim_end_matches('/'))
}

/// Цель нашей ссылки `~/.themes/<name>` — относительно самого `~/.themes`.
fn theme_link_target(name: &str) -> std::path::PathBuf {
    std::path::Path::new("../.local/share/themes").join(name)
}

/// Наша ли ссылка `~/.themes/<name>`: ведёт ровно на
/// `~/.local/share/themes/<name>` (см. [`sync_theme_links`]).
pub fn is_theme_link(home: &str, name: &str) -> bool {
    std::fs::read_link(join(&legacy_themes_dir(home), name))
        .is_ok_and(|t| t == theme_link_target(name))
}

/// Тема ли лежит в каталоге: `index.theme`, стили GTK 2/3/4 или подкаталог
/// рамок и оболочек из матрицы (`metacity-1`, `xfwm4`, `cinnamon`, …).
fn looks_like_theme(dir: &std::path::Path) -> bool {
    use crate::de_customization::{Payload, ROWS};
    let subs = ROWS.iter().filter_map(|r| match r.payload {
        Payload::ThemeSubdir { sub } => Some(sub),
        _ => None,
    });
    dir.join("index.theme").is_file()
        || ["gtk-2.0"].into_iter().chain(GTK_PARTS.iter().copied()).chain(subs).any(|s| dir.join(s).is_dir())
}

/// Привести ссылки `~/.themes` в соответствие с `~/.local/share/themes`:
/// поставить недостающие, снять ведущие в пустоту. Возвращает строки для
/// журнала (пусто — делать было нечего).
pub fn sync_theme_links(home: &str) -> Vec<String> {
    let xdg = xdg_themes_dir(home);
    let legacy = legacy_themes_dir(home);
    let mut log = Vec::new();
    for name in scandir_names(std::path::Path::new(&legacy)) {
        if is_theme_link(home, &name)
            && !std::path::Path::new(&join(&xdg, &name)).is_dir()
            && std::fs::remove_file(join(&legacy, &name)).is_ok()
        {
            log.push(format!("снята ссылка ~/.themes/{name}: темы больше нет"));
        }
    }
    let mut names = scandir_names(std::path::Path::new(&xdg));
    names.sort();
    for name in names {
        let src = std::path::PathBuf::from(join(&xdg, &name));
        if name.starts_with('.') || !src.is_dir() || !looks_like_theme(&src) {
            continue;
        }
        let dst = join(&legacy, &name);
        if std::fs::symlink_metadata(&dst).is_ok() {
            continue; // имя занято: своей темой, ссылкой или чем угодно
        }
        let made = std::fs::create_dir_all(&legacy)
            .and_then(|()| std::os::unix::fs::symlink(theme_link_target(&name), &dst));
        match made {
            Ok(()) => log.push(format!("тема {name} видна в ~/.themes (ссылка)")),
            Err(e) => log.push(format!("не удалось поставить ссылку ~/.themes/{name}: {e}")),
        }
    }
    log
}

/// Снять нашу ссылку `~/.themes/<имя>` вслед за снятой темой `removed`
/// (путь в `~/.local/share/themes`). Не наша ссылка или чужой путь — ничего.
pub fn drop_theme_link(home: &str, removed: &str) -> bool {
    let Some((dir, name)) = removed.trim_end_matches('/').rsplit_once('/') else { return false };
    dir == xdg_themes_dir(home)
        && is_theme_link(home, name)
        && std::fs::remove_file(join(&legacy_themes_dir(home), name)).is_ok()
}

/// Имена объектов типа в каталоге: для "files" — файлы с нужным окончанием
/// (имя целиком, как его знает окружение), иначе [`list_entries`].
pub fn entries_of(fs: &FsSnapshot, directory: &str, t: &ScanType) -> Vec<String> {
    if t.entry != "files" {
        return list_entries(fs, directory, t.entry);
    }
    if directory.is_empty() || !fs.is_dir(directory) {
        return Vec::new();
    }
    fs.scandir(directory)
        .iter()
        .filter(|n| n.ends_with(t.probe) && fs.is_file(&join(directory, n)))
        .cloned()
        .collect()
}

/// Имена включённого вида из карты активных: у тем одно, у дополнений и
/// виджетов Conky — сколько угодно (в карте — через перевод строки).
pub fn active_names<'m>(
    active_map: &'m HashMap<String, String>,
    apply_kind: &str,
) -> Vec<&'m str> {
    active_map
        .get(apply_kind)
        .map(|v| v.split('\n').filter(|n| !n.is_empty()).collect())
        .unwrap_or_default()
}

fn is_active(active_map: &HashMap<String, String>, apply_kind: &str, name: &str) -> bool {
    active_names(active_map, apply_kind).contains(&name)
}

/// Каталоги Kvantum-тем (user + system) — порт `_kvantum_dirs`.
pub fn kvantum_dirs(home: &str, usr_root: &str) -> [String; 2] {
    kvantum_dirs_xdg(None, home, usr_root)
}

/// То же с учётом `$XDG_CONFIG_HOME`.
pub fn kvantum_dirs_xdg(
    xdg_config_home: Option<&str>,
    home: &str,
    usr_root: &str,
) -> [String; 2] {
    [
        format!("{}/Kvantum", crate::utils::config_home(xdg_config_home, home)),
        format!("{usr_root}/share/Kvantum"),
    ]
}

/// Каталоги look-and-feel (user + system) — порт `_lnf_dirs`.
pub fn lnf_dirs(home: &str, usr_root: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/plasma/look-and-feel"),
        format!("{usr_root}/share/plasma/look-and-feel"),
    ]
}

// ============================================================================
// Сайдкар-превью — порт _sidecar_key / sidecar_preview_path
// ============================================================================

/// Безопасное имя файла сайдкар-превью — порт `_sidecar_key` (юникод-буквы
/// остаются: Python `str.isalnum`).
pub fn sidecar_key(apply_kind: &str, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{apply_kind}__{safe}.png")
}

/// Каталог картинок магазина для тем без своего каталога (цветовые схемы) —
/// в кеше «Оформления» ([`crate::appearance_cache::cache_dir`]).
pub fn sidecar_preview_dir(home: &str) -> String {
    format!("{}/local_previews", crate::appearance_cache::cache_dir(home))
}

/// Абсолютный путь сайдкар-превью — порт `sidecar_preview_path`.
pub fn sidecar_preview_path(home: &str, apply_kind: &str, name: &str) -> String {
    format!("{}/{}", sidecar_preview_dir(home), sidecar_key(apply_kind, name))
}

// ============================================================================
// Перечисление и валидация элементов — порт _list_entries / _is_valid_lnf /
// _entry_valid / _is_cursor_dir
// ============================================================================

/// Имена тем в каталоге — порт `_list_entries`. entry="colors" → файлы
/// *.colors (без суффикса), иначе подпапки. Тихо к отсутствию каталога.
pub fn list_entries(fs: &FsSnapshot, directory: &str, entry: &str) -> Vec<String> {
    let mut out = Vec::new();
    if directory.is_empty() || !fs.is_dir(directory) {
        return out;
    }
    for name in fs.scandir(directory) {
        let full = join(directory, name);
        if entry == "colors" {
            if fs.is_file(&full) {
                if let Some(stem) = name.strip_suffix(".colors") {
                    out.push(stem.to_string());
                }
            }
        } else if fs.is_dir(&full) {
            out.push(name.clone());
        }
    }
    out
}

/// look-and-feel валидна только с осмысленным Name в metadata.json/.desktop —
/// порт `_is_valid_lnf` (отсев junk-подкаталогов без metadata).
///
/// Нюанс Python: metadata.json с НЕ-объектом верхнего уровня (строка/число)
/// уронил бы скан AttributeError — здесь трактуем как «не тема» (false).
pub fn is_valid_lnf(fs: &FsSnapshot, theme_dir: &str) -> bool {
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        let Some(text) = fs.read_text(&mj) else {
            return false;
        };
        let Ok(data) = serde_json::from_str::<Value>(text) else {
            return false; // битый json → не тема
        };
        let name = data["KPlugin"]["Name"].as_str().unwrap_or("");
        return !name.trim().is_empty();
    }
    let md = join(theme_dir, "metadata.desktop");
    if fs.is_file(&md) {
        if let Some(text) = fs.read_text(&md) {
            for line in text.lines() {
                let s = line.trim();
                if let Some(v) = s.strip_prefix("Name=") {
                    if !v.trim().is_empty() {
                        return true;
                    }
                }
            }
        }
        return false;
    }
    false // ни metadata.json, ни metadata.desktop → не тема
}

/// Ключи имени по языку покупателя — от самого точного к общему:
/// `ru_RU.UTF-8` → `["Name[ru_RU]", "Name[ru]", "Name"]`.
///
/// И `metadata.json`, и `metadata.desktop` KDE помечают перевод одинаково —
/// суффиксом в квадратных скобках с ПОДЧЁРКИВАНИЕМ (`Name[pt_BR]`), поэтому
/// дефис в локали нормализуется в подчёркивание. Хвост `.UTF-8` и модификатор
/// `@valencia` отбрасываются: в ключах их не пишут.
pub fn locale_name_keys(locale: &str) -> Vec<String> {
    locale_keys("Name", locale)
}

/// То же правило для любого ключа: `Comment`, `Description`.
pub(crate) fn locale_keys(base: &str, locale: &str) -> Vec<String> {
    let cleaned = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('-', "_");
    let cleaned = cleaned.trim();
    let mut keys = Vec::new();
    if !cleaned.is_empty() {
        keys.push(format!("{base}[{cleaned}]"));
        if let Some((lang, _region)) = cleaned.split_once('_') {
            if !lang.is_empty() {
                keys.push(format!("{base}[{lang}]"));
            }
        }
    }
    keys.push(base.to_string());
    keys
}

/// Имя из ini-подобного файла по УЖЕ посчитанным ключам языка — общий
/// разбор для `metadata.desktop`, `index.theme` и `*.colors`.
///
/// Секция не спрашивается намеренно (обоснование — у [`theme_display_name`]).
/// Живая проверка на этой машине: у иконок и курсоров имя стоит в
/// `[Icon Theme]`, у GTK-тем — то в `[Desktop Entry]` (WhiteSur, Dracula,
/// Materia), то в `[X-GNOME-Metatheme]` (Adwaita, adw-gtk3), у цветовых
/// схем KDE — в `[General]` одиннадцатой по счёту. Привязка к группе
/// потеряла бы каждый второй вид оформления.
///
/// Порядок перебора — по ключам, а не по строкам файла: перевод под язык
/// покупателя выигрывает у общего имени, даже если стоит ниже.
fn name_from_ini(text: &str, keys: &[String]) -> Option<String> {
    for key in keys {
        let prefix = format!("{key}=");
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix(&prefix) {
                if !v.trim().is_empty() {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Человеческое имя из ОДНОГО ini-файла — для видов оформления, у которых
/// каталога темы нет вовсе: цветовая схема KDE это файл `<имя>.colors`.
///
/// Правило выбора языка и откат — те же, что у [`theme_display_name`]:
/// вторых копий логики нет, обе двери ведут в `locale_name_keys` +
/// `name_from_ini`.
pub fn file_display_name(
    fs: &FsSnapshot,
    file_path: &str,
    fallback: &str,
    locale: &str,
) -> String {
    if file_path.is_empty() || !fs.is_file(file_path) {
        return fallback.to_string();
    }
    let keys = locale_name_keys(locale);
    fs.read_text(file_path)
        .and_then(|text| name_from_ini(text, &keys))
        .unwrap_or_else(|| fallback.to_string())
}

/// Человеческое имя темы для карточки: `metadata.json` → `KPlugin.Name`,
/// иначе `metadata.desktop` → `Name=`, иначе `index.theme` → `Name=`.
/// Перевод под язык покупателя предпочитается общему (`Name[ru]` вперёд
/// `Name`).
///
/// Три источника, а не два, потому что метаданные KDE лежат далеко не у
/// всех видов оформления: у иконок, курсоров и GTK-тем рядом с темой
/// только `index.theme`. Порядок — от самого точного к самому общему:
/// `metadata.*` пишет сам автор темы под KDE, `index.theme` бывает и
/// сгенерированным.
///
/// `fallback` — имя каталога. Оно же и возвращается, когда метаданных нет,
/// имя пустое или файл битый: покупателю лучше сырой идентификатор, чем
/// карточка без подписи. Идентификатор карточки (`local:<вид>:<имя>`), пути
/// установки и сопоставление с активной темой на это имя НЕ смотрят — оно
/// только для показа.
///
/// Секцию файла не спрашиваем намеренно: у look-and-feel имя лежит в
/// `[Desktop Entry]`, а у тем SDDM — в `[SddmGreeterTheme]`, и привязка к
/// одной группе потеряла бы половину тем. Берётся первое совпадение по
/// ключу, а ключи перебираются по языку — поэтому перевод выигрывает у
/// общего имени, даже если стоит в файле ниже.
pub fn theme_display_name(
    fs: &FsSnapshot,
    theme_dir: &str,
    fallback: &str,
    locale: &str,
) -> String {
    if theme_dir.is_empty() {
        return fallback.to_string();
    }
    let keys = locale_name_keys(locale);
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        if let Some(Ok(data)) = fs.read_text(&mj).map(serde_json::from_str::<Value>) {
            for key in &keys {
                let from_kp = data
                    .get("KPlugin")
                    .and_then(Value::as_object)
                    .and_then(|kp| kp.get(key));
                if let Some(Value::String(v)) = from_kp.or_else(|| data.get(key)) {
                    if !v.trim().is_empty() {
                        return v.trim().to_string();
                    }
                }
            }
            // Расширения GNOME и апплеты/десклеты/расширения Cinnamon пишут
            // имя строчной `name` (без перевода): иначе карточка выходила с
            // uuid вместо названия (живые проверки GNOME 51.1 и Cinnamon
            // 6.6, 16.10.2026).
            if let Some(Value::String(v)) = data.get("name") {
                if !v.trim().is_empty() {
                    return v.trim().to_string();
                }
            }
        }
    }
    for path in [join(theme_dir, "metadata.desktop"), join(theme_dir, "index.theme"), plymouth_desc_path(theme_dir)] {
        if !fs.is_file(&path) {
            continue;
        }
        if let Some(name) = fs.read_text(&path).and_then(|t| name_from_ini(t, &keys)) {
            return name;
        }
    }
    fallback.to_string()
}

/// Описание темы заставки загрузки — `<каталог>/<имя каталога>.plymouth`
/// (группа `[Plymouth Theme]`: `Name=`, `Description=`, с переводами). Его же
/// читает kcm plymouth для подписи плитки. У каталога другого вида такого
/// файла нет, и проба просто промахивается.
fn plymouth_desc_path(theme_dir: &str) -> String {
    let dir = theme_dir.trim_end_matches('/');
    let name = dir.rsplit('/').next().unwrap_or("");
    join(dir, &format!("{name}.plymouth"))
}

/// Имя темы без перевода — для поиска по оригинальному названию.
///
/// Те же источники и тот же порядок, что у [`theme_display_name`], только
/// язык не спрашивается: берётся голый `Name=` (`KPlugin.Name`). Пусто,
/// когда оно совпадает с показанным именем — искать по нему уже и так ищут.
pub fn theme_source_name(fs: &FsSnapshot, theme_dir: &str, fallback: &str, shown: &str) -> String {
    unless_shown(theme_display_name(fs, theme_dir, fallback, ""), shown)
}

/// То же для темы-файла (`*.colors`) — пара к [`file_display_name`].
pub fn file_source_name(fs: &FsSnapshot, file_path: &str, fallback: &str, shown: &str) -> String {
    unless_shown(file_display_name(fs, file_path, fallback, ""), shown)
}

fn unless_shown(source: String, shown: &str) -> String {
    if source == shown {
        String::new()
    } else {
        source
    }
}

/// Описание темы для карточки — те же три источника и то же правило языка,
/// что у [`theme_display_name`]: `metadata.json` → `KPlugin.Description`,
/// иначе `Comment=` из `metadata.desktop`, иначе из `index.theme` (у тем
/// звуков, значков, курсоров и GTK описание лежит только там). Нет — пусто:
/// карточка сама скажет «Нет описания».
pub fn theme_description(fs: &FsSnapshot, theme_dir: &str, locale: &str) -> String {
    if theme_dir.is_empty() {
        return String::new();
    }
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        if let Some(Ok(data)) = fs.read_text(&mj).map(serde_json::from_str::<Value>) {
            for key in &locale_keys("Description", locale) {
                let from_kp = data
                    .get("KPlugin")
                    .and_then(Value::as_object)
                    .and_then(|kp| kp.get(key));
                if let Some(Value::String(v)) = from_kp.or_else(|| data.get(key)) {
                    if !v.trim().is_empty() {
                        return v.trim().to_string();
                    }
                }
            }
        }
    }
    let keys = locale_keys("Comment", locale);
    for rel in ["metadata.desktop", "index.theme"] {
        let path = join(theme_dir, rel);
        if !fs.is_file(&path) {
            continue;
        }
        if let Some(c) = fs.read_text(&path).and_then(|t| name_from_ini(t, &keys)) {
            return c;
        }
    }
    let ply = plymouth_desc_path(theme_dir);
    if fs.is_file(&ply) {
        let keys = locale_keys("Description", locale);
        if let Some(c) = fs.read_text(&ply).and_then(|t| name_from_ini(t, &keys)) {
            return c;
        }
    }
    String::new()
}

fn entry_valid(t: &ScanType, fs: &FsSnapshot, base_dir: &str, name: &str) -> bool {
    if t.apply_kind == "global" {
        return is_valid_lnf(fs, &join(base_dir, name));
    }
    match t.entry {
        "sub" => return fs.is_dir(&join(&join(base_dir, name), t.probe)),
        "file" => return fs.is_file(&join(&join(base_dir, name), t.probe)),
        "meta" => {
            let dir = join(base_dir, name);
            return fs.is_file(&join(&dir, "metadata.json"))
                || fs.is_file(&join(&dir, "metadata.desktop"));
        }
        "conky" => {
            // Файл настроек сверху либо вложенная папка (конфиг глубже):
            // окончательно его ищет кнопка (`de_apply::conky_conf`). Имя
            // файла бывает любым (`Jhalak.conf`, `Gotham`) — тогда виджет
            // узнаётся по содержимому (`conky_scan`).
            let dir = join(base_dir, name);
            return fs.scandir(&dir).iter().any(|f| {
                f == "conky.conf"
                    || f.ends_with(".conkyrc")
                    || (f.to_lowercase().contains("conky") && f.ends_with(".conf"))
                    || fs.is_dir(&join(&dir, f))
            }) || !crate::conky_scan::config_files(&crate::animations::SystemIo, &dir).is_empty();
        }
        _ => {}
    }
    // Тема заставки загрузки — каталог с описанием `<имя>.plymouth`: только
    // такие видят `plymouth-set-default-theme --list` и kcm plymouth. Пустой
    // каталог остаётся после удаления пакета темы, и включать его нечего
    // (`system_plan::plymouth_installed_themes`).
    if t.apply_kind == "plymouth" {
        return fs.is_file(&join(&join(base_dir, name), &format!("{name}.plymouth")));
    }
    // Тема экрана входа — каталог с `metadata.desktop`: из него SDDM берёт
    // главный файл темы, и kcm sddm без него тему не показывает.
    if t.apply_kind == "sddm" {
        return fs.is_file(&join(&join(base_dir, name), "metadata.desktop"));
    }
    if t.apply_kind == "sounds" {
        // Тема звуков — только каталог с `index.theme` и `[Sound Theme]`:
        // `alsa`, `speech-dispatcher`, наборы GNOME без index.theme и обёртки
        // архивов темами для системы не являются и применить их нельзя.
        return fs
            .read_text(&join(&join(base_dir, name), "index.theme"))
            .map(|t| t.lines().any(|l| l.trim() == "[Sound Theme]"))
            .unwrap_or(false);
    }
    true
}

/// Курсорная тема ~ наличие cursors/ — порт `_is_cursor_dir` (разделяет
/// общий icons/ между типами 'icons' и 'cursor').
pub fn is_cursor_dir(fs: &FsSnapshot, path: &str) -> bool {
    fs.is_dir(&join(path, "cursors"))
}

/// Чисто курсорная тема: есть `cursors/`, а каталогов значков в
/// `index.theme` нет. Набор значков со своими курсорами (Adwaita,
/// elementary — значки XFCE по умолчанию) курсорной темой НЕ считается и
/// виден в «Значках»: раньше любое `cursors/` уводило набор только в
/// «Курсоры», и стандартных значков окружения в списке не было (проверки
/// GNOME и XFCE, 16.10.2026).
pub fn is_cursor_only(fs: &FsSnapshot, path: &str) -> bool {
    is_cursor_dir(fs, path)
        && !fs
            .read_text(&join(path, "index.theme"))
            .is_some_and(|text| icon_theme_info(text).has_dirs)
}

// ============================================================================
// Превью — порт _find_preview / _preview_from_metadata
// ============================================================================

/// Best-effort локальное превью темы — порт `_find_preview`.
/// prefer="splash"/"lockscreen" — категория-зависимый приоритет
/// contents/previews/<prefer>.png (после сайдкара .rexon-preview.png).
pub fn find_preview(fs: &FsSnapshot, theme_dir: &str, prefer: Option<&str>) -> String {
    find_preview_in(fs, theme_dir, prefer, &[])
}

/// Подкаталоги темы, в которых её вид хранит свою миниатюру
/// (`<подкаталог>/thumbnail.png` — так поставляют картинку темы GTK, Cinnamon,
/// рамок Metacity и оболочки GNOME).
pub fn preview_subdirs(t: &ScanType) -> Vec<&'static str> {
    match (t.apply_kind, t.entry) {
        ("gtk", _) => vec!["gtk-3.0", "gtk-4.0", "gtk-3.20", "gtk-2.0"],
        (_, "sub") => vec![t.probe],
        _ => Vec::new(),
    }
}

/// [`find_preview`] с миниатюрами вида в подкаталогах `subs`
/// ([`preview_subdirs`]).
pub fn find_preview_in(
    fs: &FsSnapshot,
    theme_dir: &str,
    prefer: Option<&str>,
    subs: &[&str],
) -> String {
    if !fs.is_dir(theme_dir) {
        return String::new();
    }
    let mut cands: Vec<String> = vec![REXON_PREVIEW_NAME.to_string()];
    if let Some(p) = prefer {
        if p == "splash" || p == "lockscreen" {
            cands.push(format!("contents/previews/{p}.png"));
        }
    }
    cands.extend(
        [
            "screenshot.png", "preview.png", "thumbnail.png",
            "fullscreenpreview.png",
            "contents/previews/preview.png",
            "contents/previews/fullscreenpreview.png",
            "contents/screenshot.png",
            "contents/previews/lockscreen.png",
            "contents/previews/splash.png",
            "metadata/preview.png",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    cands.extend(subs.iter().map(|sub| format!("{sub}/thumbnail.png")));
    for c in &cands {
        let p = join(theme_dir, c);
        if fs.is_file(&p) {
            return p;
        }
    }
    let mp = preview_from_metadata(fs, theme_dir);
    if !mp.is_empty() {
        return mp;
    }
    // любой *.png/*.jpg в корне, похожий на превью (screen/preview/thumb/…)
    for name in fs.scandir(theme_dir) {
        let full = join(theme_dir, name);
        if !fs.is_file(&full) {
            continue;
        }
        let low = name.to_lowercase();
        if (low.ends_with(".png") || low.ends_with(".jpg"))
            && ["screen", "preview", "thumb", "snapshot"]
                .iter()
                .any(|k| low.contains(k))
        {
            return full;
        }
    }
    String::new()
}

/// Поле Preview/Image из metadata.json/.desktop KPackage-темы — порт
/// `_preview_from_metadata` (путь относительно contents/, затем корня).
pub fn preview_from_metadata(fs: &FsSnapshot, theme_dir: &str) -> String {
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        if let Some(Ok(data)) = fs
            .read_text(&mj)
            .map(serde_json::from_str::<Value>)
        {
            for key in ["Preview", "Image"] {
                let rel = data["KPlugin"][key]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| data[key].as_str().filter(|s| !s.is_empty()));
                if let Some(rel) = rel {
                    for base in [join(theme_dir, "contents"), theme_dir.to_string()] {
                        let p = join(&base, rel);
                        if fs.is_file(&p) {
                            return p;
                        }
                    }
                }
            }
        }
    }
    let md = join(theme_dir, "metadata.desktop");
    if fs.is_file(&md) {
        if let Some(text) = fs.read_text(&md) {
            for line in text.lines() {
                let s = line.trim();
                // `Screenshot=` — снимок темы экрана входа SDDM.
                let rel = s
                    .strip_prefix("Preview=")
                    .or_else(|| s.strip_prefix("Image="))
                    .or_else(|| s.strip_prefix("Screenshot="));
                if let Some(rel) = rel {
                    let rel = rel.trim();
                    for base in [join(theme_dir, "contents"), theme_dir.to_string()] {
                        let p = join(&base, rel);
                        if fs.is_file(&p) {
                            return p;
                        }
                    }
                }
            }
        }
    }
    String::new()
}

// ============================================================================
// pacman-классификация: чистые парсеры — порт _foreign_pkgs / _owner_pkgs
// ============================================================================

/// Результат одной попытки `pacman -Qm` для чистой политики ретраев.
pub enum QmAttempt {
    /// returncode == 0: stdout.
    Ok(String),
    /// returncode != 0: stderr.
    Fail(String),
    /// OSError / SubprocessError.
    Error,
}

/// Разбор stdout `pacman -Qm`: имена foreign-пакетов.
pub fn parse_qm_output(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .filter(|ln| !ln.trim().is_empty())
        .filter_map(|ln| ln.split_whitespace().next())
        .map(String::from)
        .collect()
}

/// Политика `_foreign_pkgs` на ГОТОВЫХ попытках (subprocess/sleep — у
/// исполнителя): до 2 попыток; Fail с "lck"/"lock" в stderr → ретрай, иная
/// ошибка → стоп; ничего не вышло → None (→ ScanUnavailable у scan_system).
/// Возвращает (результат, сколько попыток израсходовано).
pub fn foreign_pkgs_from_attempts(
    has_pacman: bool,
    attempts: &[QmAttempt],
) -> (Option<HashSet<String>>, usize) {
    if !has_pacman {
        return (Some(HashSet::new()), 0); // вне Arch foreign-понятия нет
    }
    let mut used = 0;
    for attempt in attempts.iter().take(2) {
        used += 1;
        match attempt {
            QmAttempt::Error => continue,
            QmAttempt::Ok(stdout) => return (Some(parse_qm_output(stdout)), used),
            QmAttempt::Fail(stderr) => {
                let low = stderr.to_lowercase();
                if low.contains("lck") || low.contains("lock") {
                    continue; // db.lck → подождать и повторить
                }
                break; // иная ошибка → ретрай не поможет
            }
        }
    }
    (None, used)
}

/// Разбор stdout `LANG=C pacman -Qo`: «<path> is owned by <pkg> <ver>» →
/// path.rstrip('/') → pkg — порт парсера `_owner_pkgs` (regex `_QO_LINE`).
/// Строки без владельца/ошибки/мусор игнорируются.
pub fn parse_qo_output(stdout: &str) -> HashMap<String, String> {
    let mut owners = HashMap::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        // `^(.*\S)\s+is owned by\s+(\S+)\s+\S+\s*$` — жадный path: ищем
        // ПОСЛЕДНЕЕ вхождение "is owned by" с пробелами вокруг и ровно двумя
        // токенами (pkg ver) после него.
        let needle = "is owned by";
        let mut found: Option<(String, String)> = None;
        let mut idx = 0;
        while let Some(pos) = line[idx..].find(needle) {
            let at = idx + pos;
            idx = at + needle.len();
            let before = &line[..at];
            let after = &line[at + needle.len()..];
            if !before.ends_with(char::is_whitespace)
                || !after.starts_with(char::is_whitespace)
            {
                continue;
            }
            let path = before.trim_end();
            if path.is_empty() {
                continue;
            }
            let toks: Vec<&str> = after.split_whitespace().collect();
            if toks.len() == 2 {
                found = Some((
                    path.trim_end_matches('/').to_string(),
                    toks[0].to_string(),
                ));
            }
        }
        if let Some((path, pkg)) = found {
            owners.insert(path, pkg);
        }
    }
    owners
}

// ============================================================================
// Активные темы: сборка карты из готовых источников — порт _active_map
// ============================================================================

/// `_clean`: пусто/«—» → "".
fn clean(v: &str) -> &str {
    let v = v.trim();
    if v.is_empty() || v == "—" {
        ""
    } else {
        v
    }
}

/// Источники `_active_map` в ГОТОВОМ виде: значения ThemeManager/kreadconfig6
/// (kreadconfig6-поля — Some(stdout) при rc==0) и содержимое конфигов
/// (None = файла нет). `sddm_confs` — содержимое СУЩЕСТВУЮЩИХ файлов в
/// порядке чтения SDDM (`system_plan::sddm_theme_files`); `plymouth_confs` —
/// по возрастанию веса ([`plymouth_confs_by_weight`]).
#[derive(Default)]
pub struct ActiveMapSources<'a> {
    pub kde_global: &'a str,
    pub kde_plasma_style: &'a str,
    pub kde_colors: &'a str,
    pub icons: &'a str,
    pub mouse_cursor: &'a str,
    pub kcminput_cursor: &'a str,
    pub xcursor_env: &'a str,
    pub gtk_settings_ini: Option<&'a str>,
    pub widget_style: &'a str,
    pub splash: Option<&'a str>,
    pub lockscreen: Option<&'a str>,
    pub kwin_decoration_theme: Option<&'a str>,
    /// `kwinrc [org.kde.kdecoration2] library` (None — не читали).
    pub kwin_decoration_library: Option<&'a str>,
    pub kvantum_kvconfig: Option<&'a str>,
    pub sddm_confs: Vec<&'a str>,
    pub plymouth_confs: Vec<&'a str>,
    /// `$QT_STYLE_OVERRIDE` текущего сеанса — Qt читает её раньше всего
    /// остального, поэтому и мы читаем.
    pub qt_style_override_env: &'a str,
    /// Содержимое `~/.config/environment.d/rexon-qt-style.conf` (None — файла
    /// нет). Переменные оттуда попадут в сеанс только после перезахода, но
    /// выбор человек уже сделал: не показать его — значит соврать, что
    /// «ничего не произошло».
    pub qt_style_env_file: Option<&'a str>,
    /// `~/.config/lxqt/lxqt.conf` — `[Qt] style` (LXQt).
    pub lxqt_conf: Option<&'a str>,
    /// `~/.config/qt6ct/qt6ct.conf` либо qt5ct — `[Appearance] style`.
    pub qtct_conf: Option<&'a str>,
    /// Темы GTK, значков и курсора из РОДНОГО хранилища окружения
    /// (gsettings у GNOME/Cinnamon/MATE/Budgie/Pantheon/COSMIC, xfconf у XFCE,
    /// DConfig у Deepin — [`native_role_values`]). Это тот же ключ, в который
    /// их пишет применение, поэтому значение отсюда побеждает kdeglobals и
    /// `settings.ini`. None — окружение своего хранилища не имеет (KDE, LXQt,
    /// тайловые WM) или ключ не прочитался: тогда прежняя цепочка.
    pub native_gtk: Option<&'a str>,
    pub native_icons: Option<&'a str>,
    pub native_cursor: Option<&'a str>,
    /// Ответ команды чтения звуковой темы текущего окружения
    /// (`qt_settings::sound_read_argv_for`, None — читать нечем).
    pub sound_theme: Option<&'a str>,
    /// Текущие обои рабочего стола: путь либо `file://`-URI (None — узнать
    /// не вышло). Из него берётся ИМЯ ПАЧКИ — см. [`wallpaper_pack_name`].
    pub wallpaper_current: Option<&'a str>,
    /// Домашний каталог — нужен, чтобы узнать пачку обоев по пути картинки.
    pub home: &'a str,
    /// Корень usr (live: `/usr`) — вторая половина той же задачи.
    pub usr_root: &'a str,
}

/// Конфиги SDDM, из которых карта активных тем берёт `[Theme] Current=`.
///
/// `plasma_login` — экран входа рисует Plasma Login Manager
/// (`system_plan::is_plasma_login`): у него тем нет, и никакая SDDM-тема не
/// активна, что бы ни осталось в `/etc/sddm.conf.d` от прежнего SDDM.
pub fn sddm_confs_for_active_map(confs: Vec<&str>, plasma_login: bool) -> Vec<&str> {
    if plasma_login {
        Vec::new()
    } else {
        confs
    }
}

/// Каталоги, внутри которых лежат пачки обоев (пользовательский и системный).
pub fn wallpaper_roots(home: &str, usr_root: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/wallpapers"),
        format!("{usr_root}/share/wallpapers"),
    ]
}

/// Раскодировать `%XX` в `file://`-URI (gsettings отдаёт обои именно так, и
/// путь с кириллицей без раскодирования не совпадёт ни с одной пачкой).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Имя ПАЧКИ обоев, внутри которой лежит картинка текущих обоев.
///
/// Это и есть ответ на вопрос «применена ли пачка прямо сейчас»: карточка
/// сверяет своё имя с этим значением ровно так же, как иконки сверяют своё с
/// активной темой иконок — одним механизмом на все виды, без отдельного
/// костыля для обоев.
///
/// Берётся первый сегмент пути после каталога обоев, поэтому одинаково
/// работает и для пачки (`…/wallpapers/Пачка/contents/images/1920x1080.png`
/// → `Пачка`), и для одиночной картинки (`…/wallpapers/Горы.png` → `Горы.png`
/// — именно под таким именем она и лежит в журнале). Обои вне каталогов
/// обоев (свой файл из «Изображений») пачкой не считаются — пусто.
pub fn wallpaper_pack_name(current: &str, home: &str, usr_root: &str) -> String {
    let raw = current.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    let path = match raw.strip_prefix("file://") {
        Some(rest) => percent_decode(rest),
        None => raw.to_string(),
    };
    for root in wallpaper_roots(home, usr_root) {
        let Some(rest) = path.strip_prefix(&format!("{root}/")) else {
            continue;
        };
        let seg = rest.split('/').next().unwrap_or("");
        if !seg.is_empty() {
            return seg.to_string();
        }
    }
    String::new()
}

/// Значение переменной из файла `environment.d` (`ИМЯ=значение`, комментарии
/// с `#` пропускаются).
fn env_file_value(content: Option<&str>, name: &str) -> Option<String> {
    let text = content?;
    let prefix = format!("{name}=");
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('#') {
            continue;
        }
        if let Some(rest) = s.strip_prefix(&prefix) {
            let v = clean(rest.trim_matches('"'));
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Значение ключа ini-файла в указанной секции (`lxqt.conf`, `qt6ct.conf`).
fn ini_value(content: Option<&str>, section: &str, key: &str) -> Option<String> {
    let text = content?;
    let mut here = false;
    let prefix = format!("{key}=");
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == section;
            continue;
        }
        if !here || s.starts_with('#') {
            continue;
        }
        if let Some(rest) = s.strip_prefix(&prefix) {
            let v = clean(rest);
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Темы GTK, значков и курсора из родного хранилища окружения `de` —
/// `[gtk, icons, cursor]` для [`ActiveMapSources::native_gtk`] и соседей.
///
/// Команды — [`crate::themes::native_role_read_cmd`] (та же таблица схем,
/// что у применения); `read` исполняет команду и отдаёт stdout при успехе
/// (стенды подменяют его). У окружения без своего хранилища `read` не
/// зовётся вовсе — KDE не платит ни одним лишним процессом.
pub fn native_role_values(
    de: &str,
    mut read: impl FnMut(&crate::themes::Cmd) -> Option<String>,
) -> [Option<String>; 3] {
    let mut out: [Option<String>; 3] = Default::default();
    for (slot, role) in out.iter_mut().zip(crate::themes::NATIVE_STORE_ROLES) {
        let Some(cmd) = crate::themes::native_role_read_cmd(de, role) else {
            continue;
        };
        *slot = read(&cmd)
            .map(|raw| crate::themes::parse_native_role_output(de, &raw))
            .filter(|v| !clean(v).is_empty());
    }
    out
}

/// apply_kind → имя активной темы — порт `_active_map` (порядок вставки
/// ключей повторяет Python-код; сериализуется как его dict).
pub fn active_map_from_sources(src: &ActiveMapSources) -> Map<String, Value> {
    let mut out = Map::new();
    let put = |m: &mut Map<String, Value>, k: &str, v: &str| {
        m.insert(k.to_string(), Value::from(v));
    };
    if !clean(src.kde_global).is_empty() {
        put(&mut out, "global", clean(src.kde_global));
    }
    if !clean(src.kde_plasma_style).is_empty() {
        put(&mut out, "plasma", clean(src.kde_plasma_style));
    }
    if !clean(src.kde_colors).is_empty() {
        put(&mut out, "colors", clean(src.kde_colors));
    }
    // Значение родного хранилища окружения (см. `native_*`) — первым.
    // Раньше вне KDE значки и курсор брались только из kdeglobals, и в MATE
    // применённые через gsettings темы не считались активными: удаление
    // сносило применённую тему (живая проверка MATE, 16.10.2026).
    fn native(v: Option<&str>) -> Option<&str> {
        v.map(clean).filter(|v| !v.is_empty())
    }
    let icons = native(src.native_icons).unwrap_or_else(|| clean(src.icons));
    if !icons.is_empty() {
        put(&mut out, "icons", icons);
    }
    // cursor: родное хранилище → Mouse/cursorTheme → kcminput → $XCURSOR_THEME
    let cur = if let Some(v) = native(src.native_cursor) {
        v
    } else {
        let a = clean(src.mouse_cursor);
        if !a.is_empty() {
            a
        } else {
            let b = clean(src.kcminput_cursor);
            if !b.is_empty() {
                b
            } else {
                clean(src.xcursor_env)
            }
        }
    };
    if !cur.is_empty() {
        put(&mut out, "cursor", cur);
    }
    // gtk — родное хранилище, иначе gtk-3.0/settings.ini [Settings]
    // gtk-theme-name (первая строка).
    if let Some(v) = native(src.native_gtk) {
        put(&mut out, "gtk", v);
    } else if let Some(ini) = src.gtk_settings_ini {
        for line in ini.lines() {
            let s = line.trim();
            if s.starts_with("gtk-theme-name") {
                let v = match s.split_once('=') {
                    Some((_, rest)) => clean(rest),
                    None => "",
                };
                if !v.is_empty() {
                    put(&mut out, "gtk", v);
                }
                break;
            }
        }
    }
    // widgetstyle — «каким стилем нарисованы Qt-программы». Источник у него
    // РАЗНЫЙ в разных столах, и порядок здесь — порядок самого Qt: сперва
    // QT_STYLE_OVERRIDE (её Qt читает раньше платформенной темы), потом наш
    // файл environment.d (выбор уже сделан, подхватится при следующем входе),
    // потом kdeglobals (KDE), lxqt.conf (LXQt) и конфиг qt6ct.
    //
    // Раньше источник был один — kdeglobals: вне KDE он пуст, и бейдж
    // «Активна» не появлялся ни у одной темы Kvantum, хотя тема применена.
    let widget_style = {
        let env = clean(src.qt_style_override_env);
        if !env.is_empty() {
            env.to_string()
        } else if let Some(v) = env_file_value(src.qt_style_env_file, "QT_STYLE_OVERRIDE") {
            v
        } else if !clean(src.widget_style).is_empty() {
            clean(src.widget_style).to_string()
        } else if let Some(v) = ini_value(src.lxqt_conf, "Qt", "style") {
            v
        } else {
            ini_value(src.qtct_conf, "Appearance", "style").unwrap_or_default()
        }
    };
    if !widget_style.is_empty() {
        put(&mut out, "widgetstyle", &widget_style);
    }
    if let Some(v) = src.splash {
        if !clean(v.trim()).is_empty() {
            put(&mut out, "splash", clean(v.trim()));
        }
    }
    if let Some(v) = src.lockscreen {
        if !clean(v.trim()).is_empty() {
            put(&mut out, "lockscreen", clean(v.trim()));
        }
    }
    // aurorae — kwinrc theme = "__aurorae__svg__Name" (суффикс БЕЗ чистки:
    // пустой суффикс даёт пустое значение — поведение Python сохранено).
    // Под модулем Breeze/Oxygen и др. — `kdecoration` = `library/theme`
    // (`kdecoration::current`): плитку встроенной рамки подсвечивает раздел.
    if let Some(theme) = src.kwin_decoration_theme {
        match crate::kdecoration::current(src.kwin_decoration_library, Some(theme)) {
            crate::kdecoration::Current::Svg(dir) => put(&mut out, "aurorae", &dir),
            crate::kdecoration::Current::Builtin(entry) => put(&mut out, "kdecoration", &entry),
            crate::kdecoration::Current::Unknown => {}
        }
    }
    // kvantum — kvantum.kvconfig [General] theme= (первая строка theme=).
    if let Some(cfg) = src.kvantum_kvconfig {
        for line in cfg.lines() {
            let s = line.trim();
            if let Some(rest) = s.strip_prefix("theme=") {
                let v = clean(rest);
                if !v.is_empty() {
                    put(&mut out, "kvantum", v);
                }
                break;
            }
        }
    }
    // sddm — [Theme] Current=, последний выигрывает.
    //
    // ⭐ Починка 28.09.2026. Прежде здесь читались ещё /etc/plasmalogin.conf*,
    // а ключ ловился в любой группе. У Plasma Login Manager тем нет вовсе (и
    // ключа `[Theme] Current` тоже), зато после перехода на него остаётся
    // старый `/etc/sddm.conf.d/kde_settings.conf` с `Current=breeze` — и
    // вкладка отмечала «breeze» активной темой экрана входа, который рисует
    // вовсе не SDDM (живая машина владельца, снимок 28.09). При Plasma Login
    // Manager активной SDDM-темы не бывает — источники гасит
    // [`sddm_confs_for_active_map`]; `Current=` вне группы `[Theme]` темой не
    // считается — SDDM его там и не прочтёт.
    if let Some(v) = sddm_current_from(&src.sddm_confs) {
        put(&mut out, "sddm", &v);
    }
    // sounds — звуковая тема текущего окружения (kdeglobals[Sounds]Theme в
    // KDE, org.gnome.desktop.sound в остальных). Читает её тот же движок,
    // что и вкладка «Настройки», — второго способа спросить нет.
    if let Some(v) = src.sound_theme {
        let v = crate::qt_settings::parse_sound_theme(v);
        if !clean(&v).is_empty() {
            put(&mut out, "sounds", clean(&v));
        }
    }
    // wallpapers — не имя темы, а имя ПАЧКИ, внутри которой лежит картинка
    // текущих обоев: «применена» пачка ровно тогда, когда фон рабочего стола
    // взят из неё.
    if let Some(v) = src.wallpaper_current {
        let name = wallpaper_pack_name(v, src.home, src.usr_root);
        if !name.is_empty() {
            put(&mut out, "wallpapers", &name);
        }
    }
    // plymouth — plymouthd.conf [Daemon] Theme=, последний выигрывает.
    if let Some(v) = plymouth_current_from(&src.plymouth_confs) {
        put(&mut out, "plymouth", &v);
    }
    out
}

/// Тема экрана входа по файлам SDDM (в порядке чтения): `[Theme] Current=`,
/// последний выигрывает.
pub fn sddm_current_from(confs: &[&str]) -> Option<String> {
    let mut found = None;
    for conf in confs {
        let mut in_theme = false;
        for line in conf.lines() {
            let s = line.trim();
            if s.starts_with('[') {
                in_theme = s == "[Theme]";
                continue;
            }
            if !in_theme {
                continue;
            }
            if let Some(rest) = s.strip_prefix("Current=") {
                let v = clean(rest);
                if !v.is_empty() {
                    found = Some(v.to_string());
                }
            }
        }
    }
    found
}

/// Тема заставки загрузки по `plymouthd.conf`: `Theme=`, последний выигрывает.
pub fn plymouth_current_from(confs: &[&str]) -> Option<String> {
    let mut found = None;
    for conf in confs {
        for line in conf.lines() {
            if let Some(rest) = line.trim().strip_prefix("Theme=") {
                let v = clean(rest);
                if !v.is_empty() {
                    found = Some(v.to_string());
                }
            }
        }
    }
    found
}

/// Настройки plymouthd по возрастанию веса — вход [`plymouth_current_from`]
/// (последний `Theme=` выигрывает): умолчание пакета `plymouthd.defaults`,
/// затем выбор через `update-alternatives` (`default.plymouth` у семейства
/// Debian, `alt_theme` — имя каталога, куда ведёт ссылка), затем прежний
/// `/usr/share/plymouth/plymouthd.conf` и, сильнее всех, `/etc/plymouth/
/// plymouthd.conf` — его пишет `plymouth-set-default-theme`.
///
/// ⭐ Починка 31.10.2026: прежде читались только два файла, и
/// `/usr/share/…/plymouthd.conf` перекрывал `/etc`, а без `Theme=` в `/etc`
/// (так на CachyOS, пока тему не меняли) текущей не было вовсе — сетка
/// раздела не выделяла тему, которую plymouthd берёт из умолчания пакета.
pub fn plymouth_confs_by_weight(
    defaults: Option<String>,
    alt_theme: Option<&str>,
    legacy_conf: Option<String>,
    etc_conf: Option<String>,
) -> Vec<String> {
    let alt = alt_theme
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("[Daemon]\nTheme={t}\n"));
    [defaults, alt, legacy_conf, etc_conf].into_iter().flatten().collect()
}

/// Выбранная сейчас системная тема вида `kind` (`sddm` | `plymouth`) — для
/// запрета «Удалить» у активной. Те же файлы, что у карты активных тем, плюс
/// системные умолчания (`/usr/lib/sddm/sddm.conf.d`, `plymouthd.defaults`) и
/// выбор через `update-alternatives` (`default.plymouth`): здесь лишний
/// отказ безопаснее, чем снятая активная тема. Помощник от root проверяет
/// то же самое ещё раз.
pub fn live_system_theme_active(kind: &str) -> Vec<String> {
    let read_dir_sorted = |dir: &str| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path().to_string_lossy().into_owned())
                    .filter(|p| p.ends_with(".conf"))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    let read = |paths: &[String]| -> Vec<String> {
        paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect()
    };
    let mut out = Vec::new();
    match kind {
        "sddm" => {
            let mut paths = read_dir_sorted("/usr/lib/sddm/sddm.conf.d");
            paths.extend(read_dir_sorted("/etc/sddm.conf.d"));
            paths.push("/etc/sddm.conf".to_string());
            // Каждый файл отдельно: любое упоминание — уже «выбрана».
            for text in read(&paths) {
                out.extend(sddm_current_from(&[&text]));
            }
        }
        "plymouth" => {
            let paths: Vec<String> = [
                "/etc/plymouth/plymouthd.conf",
                "/usr/share/plymouth/plymouthd.defaults",
                "/usr/share/plymouth/plymouthd.conf",
            ]
            .iter()
            .map(|p| p.to_string())
            .collect();
            for text in read(&paths) {
                out.extend(plymouth_current_from(&[&text]));
            }
            let alt = format!("{}/default.plymouth", crate::system_plan::PLYMOUTH_THEMES_DIR);
            if let Ok(real) = std::fs::canonicalize(&alt) {
                let themes = std::fs::canonicalize(crate::system_plan::PLYMOUTH_THEMES_DIR)
                    .unwrap_or_else(|_| std::path::PathBuf::from(crate::system_plan::PLYMOUTH_THEMES_DIR));
                if let Ok(rest) = real.strip_prefix(&themes) {
                    if let Some(first) = rest.components().next() {
                        out.push(first.as_os_str().to_string_lossy().into_owned());
                    }
                }
            }
        }
        _ => {}
    }
    out
}

// ============================================================================
// Бэкофилл превью из кеша магазина — порт _normalize_theme_name /
// _backfill_preview
// ============================================================================

/// Нормализация имени темы для нечёткого матча папка↔магазин — порт
/// `_normalize_theme_name` (юникод-буквы остаются: Python `str.isalnum`).
pub fn normalize_theme_name(name: &str) -> String {
    let mut s = name.to_lowercase();
    for noise in [
        "gtk", "theme", "themes", "kde", "plasma", "icons", "icon",
        "cursors", "cursor", "color", "colors", "scheme", "global",
        "aurorae", "kvantum", "sddm", "plymouth",
    ] {
        s = s.replace(noise, "");
    }
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Окружение сканеров: пути + снимок ФС + кеш магазина.
pub struct ScanEnv<'a> {
    /// Домашний каталог (live: `$HOME`; в фикстурах — /sb/home).
    pub home: &'a str,
    /// Корень usr (live: "/usr"; в фикстурах — /sb/usr).
    pub usr_root: &'a str,
    pub fs: &'a FsSnapshot,
    /// (name, preview_url) закешированных OCS-тем — `cache.all_items_min()`.
    pub ocs_cache: &'a [(String, String)],
    /// `cache.get_preview(url)` → путь локального файла превью (или None).
    pub preview_file: &'a dyn Fn(&str) -> Option<String>,
    /// Что магазин знает об установленном: раздел каждой картинки кеша и
    /// реестр установок KNewStuff ([`StoreRecords`]).
    pub store: &'a StoreRecords,
    /// Язык покупателя (`ru_RU.UTF-8`) — им выбирается перевод имени темы.
    /// Параметром, а не чтением окружения внутри: иначе списки тем зависели
    /// бы от локали машины и голдены перестали бы что-либо доказывать.
    pub locale: &'a str,
    /// Стандартная тема значков окружения ([`LiveScan::with_de`]): её плитка
    /// видна, даже если набор помечен `Hidden=true` (Adwaita) — иначе
    /// вернуть значки окружения по умолчанию было бы нечем.
    pub standard_icons: Option<&'a str>,
}

/// Что магазин знает об установленных темах — сырьё второго шага цепочки
/// превью (`local_preview`): картинка из записи магазина.
#[derive(Debug, Default, Clone)]
pub struct StoreRecords {
    /// Ссылка на картинку кеша магазина → ключ раздела её товара
    /// (`categories`: `icons`, `kvantum`, …). Пусто — раздел неизвестен, и
    /// сопоставление по имени идёт без него (прежнее поведение).
    pub sections: HashMap<String, String>,
    /// Реестр установок KNewStuff: установленный путь → ссылка на картинку
    /// товара ([`parse_kns_registry`]).
    pub installed: Vec<(String, String)>,
    /// Пути из журнала установленных Rexon
    /// (`appearance_installer::installed_log_paths`). `None` — журнал не
    /// читали (сканы без магазина, эталоны): тогда каждая тема в домашнем
    /// каталоге подписана «Скачано», как прежде.
    pub rexon_paths: Option<Vec<String>>,
}

impl StoreRecords {
    /// Подпись происхождения темы в каталоге `path`: «Скачано» — если её
    /// поставил Rexon (журнал) или магазин KDE (реестр KNewStuff), иначе
    /// «Установлена вручную». Темы, положенные руками или прежним `.rxn`,
    /// «скачанными» не были, и называть их так — неправда.
    fn origin_label(&self, path: &str) -> &'static str {
        let Some(journal) = &self.rexon_paths else {
            return DOWNLOADED_LABEL;
        };
        let path = path.trim_end_matches('/');
        let ours = |p: &str| {
            let p = p.trim_end_matches('/');
            p == path || p.starts_with(&format!("{path}/"))
        };
        if journal.iter().any(|p| ours(p)) || self.installed.iter().any(|(p, _)| ours(p)) {
            DOWNLOADED_LABEL
        } else {
            MANUAL_LABEL
        }
    }
}

/// Короткая пометка на плитке GTK-темы, когда значков или курсора, на
/// которые тема рассчитана (`index.theme`: `IconTheme=`, `CursorTheme=`), в
/// системе нет. `items` — план `themes::plan_theme_deps` (его же спрашивает
/// «Применить»). `None` — всё на месте или тема ничего не просит.
pub fn missing_deps_note(items: &[crate::themes::DepItem]) -> Option<String> {
    use crate::themes::{DepRole, DepState};
    let missing = |role: DepRole| {
        items.iter().any(|i| i.role == role && !matches!(i.state, DepState::Installed(_)))
    };
    match (missing(DepRole::Icons), missing(DepRole::Cursor)) {
        (true, true) => Some(tr("⚠️ the theme's icons and cursor are missing")),
        (true, false) => Some(tr("⚠️ the theme's icons are missing")),
        (false, true) => Some(tr("⚠️ the theme's cursor is missing")),
        (false, false) => None,
    }
}

/// Дописать [`missing_deps_note`] в подпись плиток GTK-тем. `plan` — план
/// просьб темы по имени её каталога (живой — `LiveInstaller::theme_deps_plan`).
pub fn stamp_missing_deps(items: &mut [ThemeItem], plan: &dyn Fn(&str) -> Vec<crate::themes::DepItem>) {
    for it in items.iter_mut() {
        let Some(name) = it.id.strip_prefix("local:gtk:") else { continue };
        if let Some(note) = missing_deps_note(&plan(name)) {
            it.author = format!("{} · {note}", it.author);
        }
    }
}

/// Подпись темы, поставленной Rexon или магазином.
const DOWNLOADED_LABEL: &str = n_("⬇️ Downloaded");
/// Подпись темы в домашнем каталоге, которую ни Rexon, ни магазин не ставили.
const MANUAL_LABEL: &str = n_("🗂️ Installed manually");

impl StoreRecords {
    /// Пустые записи — для сканов без магазина.
    pub fn none() -> &'static StoreRecords {
        static NONE: std::sync::OnceLock<StoreRecords> = std::sync::OnceLock::new();
        NONE.get_or_init(StoreRecords::default)
    }
}

/// Записи реестра установок KNewStuff (`~/.local/share/knewstuff3/*.knsregistry`):
/// пары «установленный путь — ссылка на картинку товара». Реестр ведут
/// «Загрузить…» в настройках KDE и Discover; у записи `<stuff>` пути лежат в
/// `<installedfile>`, картинка — в `<preview>` (уменьшенная) и `<previewBig>`.
pub fn parse_kns_registry(xml: &str) -> Vec<(String, String)> {
    fn tags<'t>(block: &'t str, tag: &str) -> Vec<&'t str> {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        block
            .split(open.as_str())
            .skip(1)
            .filter_map(|rest| rest.split(close.as_str()).next())
            .collect()
    }
    let unescape = |v: &str| {
        v.trim()
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    };
    let mut out = Vec::new();
    for block in xml.split("<stuff").skip(1) {
        let block = block.split("</stuff>").next().unwrap_or("");
        let url = tags(block, "preview")
            .into_iter()
            .chain(tags(block, "previewBig"))
            .map(unescape)
            .find(|u| !u.is_empty());
        let Some(url) = url else { continue };
        for path in tags(block, "installedfile") {
            // Каталог записан с хвостом `/*` — «всё, что внутри».
            let path = unescape(path);
            let path = path.trim_end_matches('*').trim_end_matches('/');
            if path.starts_with('/') {
                out.push((path.to_string(), url.clone()));
            }
        }
    }
    out
}

/// Картинка товара, из которого установлен `path` (каталог темы или её файл),
/// по реестру установок — если она уже лежит в кеше магазина.
pub fn installed_record_preview(env: &ScanEnv, path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return String::new();
    }
    let inside = format!("{path}/");
    env.store
        .installed
        .iter()
        .filter(|(installed, _)| installed == path || installed.starts_with(&inside))
        .find_map(|(_, url)| (env.preview_file)(url).filter(|local| env.fs.is_file(local)))
        .unwrap_or_default()
}

/// Best-effort превью для установленной темы из кеша магазина — порт
/// `_backfill_preview`: консервативный нечёткий матч (подстрока в обе
/// стороны, стержень ≥4), неоднозначность (≠1 url) → "".
pub fn backfill_preview(env: &ScanEnv, name: &str) -> String {
    backfill_preview_for(env, "", name)
}

/// [`backfill_preview`] в пределах раздела `kind`: картинка товара другого
/// раздела теме не подходит, даже если имена похожи. Без этого тема Kvantum
/// «Win7OS» показывала картинку набора значков «win7-icon-theme», а все
/// апплеты `…@cinnamon.org` — картинку одной глобальной темы (12.10.2026).
/// Раздел картинки берётся из [`StoreRecords::sections`]; пустой `kind` или
/// пустая карта — сопоставление без раздела, прежним правилом (подстрока в
/// обе стороны). В пределах раздела имена обязаны совпасть целиком.
pub fn backfill_preview_for(env: &ScanEnv, kind: &str, name: &str) -> String {
    let by_section = !kind.is_empty() && !env.store.sections.is_empty();
    let target = normalize_theme_name(name);
    if target.chars().count() < 4 {
        return String::new();
    }
    let mut matched: HashSet<&str> = HashSet::new();
    for (cache_name, url) in env.ocs_cache {
        if url.is_empty() {
            continue;
        }
        if by_section && env.store.sections.get(url).map(String::as_str) != Some(kind) {
            continue;
        }
        let norm = normalize_theme_name(cache_name);
        if norm.chars().count() < 4 {
            continue;
        }
        // В пределах раздела — только то же самое имя: с образцом из
        // содержимого темы картинка похожего по названию товара («Orang» и
        // «Glassy-OS-3D-GTK - [ Orange ]») хуже нарисованной по самой теме.
        let same = if by_section { target == norm } else { target.contains(&norm) || norm.contains(&target) };
        if same {
            matched.insert(url.as_str());
        }
    }
    if matched.len() != 1 {
        return String::new(); // 0 — не нашли; >1 — неоднозначно
    }
    let url = matched.into_iter().next().unwrap();
    match (env.preview_file)(url) {
        Some(local) if env.fs.is_file(&local) => local,
        _ => String::new(),
    }
}

/// Превью для локальной темы — порт `_resolve_preview`: файл в теме →
/// сайдкар-кеш → бэкофилл из магазина → "" (UI рисует плейсхолдер).
pub fn resolve_preview(
    env: &ScanEnv,
    apply_kind: &str,
    name: &str,
    theme_dir: &str,
    prefer: Option<&str>,
) -> String {
    resolve_preview_in(env, apply_kind, name, theme_dir, theme_dir, prefer, &[])
}

/// Первые два шага единой цепочки превью (`local_preview`) для ЛЮБОГО вида
/// темы: своя картинка (файл в каталоге темы, названный метаданными,
/// миниатюра вида в подкаталоге из `subs`), затем картинка из записи
/// магазина — положенная при установке (сайдкар), найденная по реестру
/// установок для `install_path` и, последним, по имени в кеше магазина в
/// пределах раздела `apply_kind`. Пусто — картинку рисует `local_preview`.
pub fn resolve_preview_in(
    env: &ScanEnv,
    apply_kind: &str,
    name: &str,
    theme_dir: &str,
    install_path: &str,
    prefer: Option<&str>,
    subs: &[&str],
) -> String {
    if !theme_dir.is_empty() {
        let p = find_preview_in(env.fs, theme_dir, prefer, subs);
        if !p.is_empty() {
            return p;
        }
    }
    let side = sidecar_preview_path(env.home, apply_kind, name);
    if env.fs.is_file(&side) {
        return side;
    }
    let recorded = installed_record_preview(env, install_path);
    if !recorded.is_empty() {
        return recorded;
    }
    backfill_preview_for(env, apply_kind, name)
}

// ============================================================================
// Построение ThemeItem — порт _mk_item / _mk_local_item
// ============================================================================

fn mk_item(
    env: &ScanEnv,
    t: &ScanType,
    name: &str,
    base_dir: &str,
    active: bool,
    source_label: &str,
) -> ThemeItem {
    let theme_dir = if t.entry == "colors" {
        String::new()
    } else {
        join(base_dir, name)
    };
    let install_path = if theme_dir.is_empty() {
        join(base_dir, name)
    } else {
        theme_dir.clone()
    };
    // У цветовой схемы реестр установок помнит сам файл `<имя>.colors`.
    let recorded_as = if t.entry == "colors" {
        format!("{install_path}.colors")
    } else {
        install_path.clone()
    };
    let preview = resolve_preview_in(
        env, t.apply_kind, name, &theme_dir, &recorded_as, None, &preview_subdirs(t),
    );
    let badge = if active { local_active_badge() } else { tr(source_label) };
    // В номере — имя каталога (по нему тема применяется), в подписи —
    // человеческое имя из метаданных. У цветовой схемы каталога нет вовсе:
    // тема это один файл `<имя>.colors`, и имя лежит внутри него.
    let (display, source_name) = if t.entry == "colors" {
        let file = format!("{}.colors", join(base_dir, name));
        let display = file_display_name(env.fs, &file, name, env.locale);
        let source = file_source_name(env.fs, &file, name, &display);
        (display, source)
    } else if t.entry == "files" {
        (name.strip_suffix(t.probe).unwrap_or(name).to_string(), String::new())
    } else {
        let display = theme_display_name(env.fs, &theme_dir, name, env.locale);
        let source = theme_source_name(env.fs, &theme_dir, name, &display);
        (display, source)
    };
    let description = theme_description(env.fs, &theme_dir, env.locale);
    ThemeItem {
        id: format!("local:{}:{name}", t.apply_kind),
        name: display,
        description,
        author: format!("{} {} · {badge}", t.icon, type_label(t)),
        category_id: t.cat_id,
        installed: true,
        install_path,
        preview_local: preview,
        local_kind: active_kind(active),
        source_name,
        ..Default::default()
    }
}

// Сигнатура повторяет kwargs Python `_mk_local_item` 1-в-1 (см. паритет).
//
// `display` — то, что увидит покупатель; `name` — имя каталога, из которого
// собран номер карточки. Считает его вызывающий: у стиля приложений каталога
// темы нет вовсе, а у Kvantum и look-and-feel он свой.
#[allow(clippy::too_many_arguments)]
fn mk_local_item(
    kind: &str,
    cat_id: i64,
    name: &str,
    display: &str,
    active: bool,
    label: &str,
    icon: &str,
    install_path: &str,
    preview: &str,
) -> ThemeItem {
    // label уже содержит эмодзи типа; активному бейджу префиксуем icon
    // (иначе был баг «🎛 🎛 …» — см. Python-комментарий).
    let author = if active {
        format!("{icon} {}", local_active_badge())
    } else {
        tr(label)
    };
    ThemeItem {
        id: format!("local:{kind}:{name}"),
        name: display.to_string(),
        author,
        category_id: cat_id,
        installed: true,
        install_path: install_path.to_string(),
        preview_local: preview.to_string(),
        local_kind: active_kind(active),
        ..Default::default()
    }
}

/// Вид темы в подписи карточки на языке интерфейса.
///
/// Перевод — с контекстом «theme kind»: те же английские слова («GTK Theme»)
/// в других местах программы переведены иначе, а здесь русский обязан
/// остаться прежним. Имена собственные (Kvantum, Plymouth) — как есть.
pub fn type_label(t: &ScanType) -> String {
    match t.apply_kind {
        "global" => trc("theme kind", "Global Theme"),
        "plasma" => trc("theme kind", "Plasma Style"),
        "colors" => trc("theme kind", "Color Scheme"),
        "aurorae" => trc("theme kind", "Window Decorations (Aurorae)"),
        "icons" => trc("theme kind", "Icons"),
        "cursor" => trc("theme kind", "Cursor"),
        "gtk" => trc("theme kind", "GTK Theme"),
        "sddm" => trc("theme kind", "SDDM (Login Screen)"),
        "sounds" => trc("theme kind", "Sound Theme"),
        // Разделы окружений: подпись — `msgid` таблицы типов.
        _ if crate::categories::is_de_section(t.cat_id) => tr(t.label),
        _ => t.label.to_string(),
    }
}

/// Признак «тема включена сейчас» для страницы — отдельно от подписи.
///
/// Подпись автора («… · ✅ Активна») переводится, и узнавать активную тему
/// поиском бейджа в ней значило бы привязать поведение к языку. Неактивным
/// признака не ставим: вид такой плитки решает раздел, в котором её показали
/// (`appearance_page::scan_local_kind`).
fn active_kind(active: bool) -> Option<String> {
    active.then(|| "active".to_string())
}

// ============================================================================
// Публичные сканеры
// ============================================================================

/// Темы, применённые СЕЙЧАС (карта из конфига KDE) — порт `scan_active`.
pub fn scan_active(env: &ScanEnv, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
    scan_active_in(env, &types_for(env.home, env.usr_root), active_map)
}

/// [`scan_active`] по заданному набору типов (живой скан добавляет разделы
/// окружений — [`de_types`]).
pub fn scan_active_in(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (ti, t) in types.iter().enumerate() {
        // Тот же вид ниже по списку (раздел смотрит ещё и `~/.icons`,
        // `~/.themes`): не нашли тему здесь — пусть ищет он, а не плитка с
        // путём в каталог, где темы нет.
        let later_same_kind = types[ti + 1..].iter().any(|o| o.apply_kind == t.apply_kind);
        for name in active_names(active_map, t.apply_kind) {
            // каталог, где реально лежит активная тема (user → sys)
            let mut base = String::new();
            for d in [&t.user_dir, &t.sys_dir] {
                if d.is_empty() {
                    continue;
                }
                let hit = if t.entry == "colors" {
                    env.fs.is_file(&join(d, &format!("{name}.colors")))
                } else if t.entry == "files" {
                    env.fs.is_file(&join(d, name))
                } else {
                    env.fs.is_dir(&join(d, name))
                };
                if hit {
                    base = d.clone();
                    break;
                }
            }
            if base.is_empty() && later_same_kind {
                continue;
            }
            if base.is_empty() {
                base = if !t.user_dir.is_empty() {
                    t.user_dir.clone()
                } else {
                    t.sys_dir.clone()
                };
            }
            if !entry_valid(t, env.fs, &base, name) {
                continue;
            }
            let key = format!("{}:{name}", t.apply_kind);
            if !seen.insert(key) {
                continue;
            }
            items.push(mk_item(env, t, name, &base, true, ""));
        }
    }
    items
}

/// Пользовательские темы в ~/.local/share/…, НЕ активные — порт
/// `scan_downloaded`.
pub fn scan_downloaded(
    env: &ScanEnv,
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    scan_downloaded_in(env, &types_for(env.home, env.usr_root), active_map)
}

/// [`scan_downloaded`] по заданному набору типов.
pub fn scan_downloaded_in(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    for t in types {
        if t.user_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.user_dir, t) {
            if t.apply_kind == "cursor" {
                if !is_cursor_dir(env.fs, &join(&t.user_dir, &name)) {
                    continue;
                }
            } else if t.apply_kind == "icons"
                && is_cursor_only(env.fs, &join(&t.user_dir, &name))
            {
                continue; // чисто курсорные темы в icons/ не дублируем в «Иконки»
            }
            if is_active(active_map, t.apply_kind, &name) {
                continue;
            }
            if !entry_valid(t, env.fs, &t.user_dir, &name)
                || !entry_listed(env, t.apply_kind, &join(&t.user_dir, &name))
            {
                continue;
            }
            let label = env.store.origin_label(&join(&t.user_dir, &name));
            items.push(mk_item(env, t, &name, &t.user_dir, false, label));
        }
    }
    items
}

/// Результат `scan_system`: элементы + пути, ушедшие в `pacman -Qo`
/// (для сверки и для самого вызова исполнителем).
#[derive(Debug)]
pub struct SystemScan {
    pub items: Vec<ThemeItem>,
    pub qo_paths: Vec<String>,
}

/// Темы в /usr/share/… с НАТИВНЫМ пакетом-владельцем — порт `scan_system`.
///
/// `foreign` = None означает «pacman -Qm недоступен» → Err с текстом
/// `SystemScanUnavailable` (UI показывает явный статус, AUR-темы не
/// протекают в «Системные»). Без pacman классификация невозможна → Ok(пусто).
/// `owners` — карта path→пакет из `parse_qo_output` (ключи с rstrip('/')).
pub fn scan_system(
    env: &ScanEnv,
    active_map: &HashMap<String, String>,
    has_pacman: bool,
    foreign: Option<&HashSet<String>>,
    owners: &HashMap<String, String>,
) -> Result<SystemScan, String> {
    if !has_pacman {
        return Ok(SystemScan { items: Vec::new(), qo_paths: Vec::new() });
    }
    let Some(foreign) = foreign else {
        return Err(
            tr("Couldn't tell where packages came from (pacman -Qm is unavailable). \
                System themes are hidden so that AUR themes are not shown as system ones."),
        );
    };

    let types = types_for(env.home, env.usr_root);
    scan_system_in(env, &types, active_map, foreign, owners)
}

/// [`scan_system`] по заданному набору типов (pacman уже есть, происхождение
/// пакетов известно).
pub fn scan_system_in(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
    foreign: &HashSet<String>,
    owners: &HashMap<String, String>,
) -> Result<SystemScan, String> {
    // 1) кандидаты (тип, имя, полный путь)
    let mut cands: Vec<(usize, String, String)> = Vec::new();
    for (ti, t) in types.iter().enumerate() {
        if t.sys_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.sys_dir, t) {
            if t.apply_kind == "cursor" {
                if !is_cursor_dir(env.fs, &join(&t.sys_dir, &name)) {
                    continue;
                }
            } else if t.apply_kind == "icons"
                && is_cursor_only(env.fs, &join(&t.sys_dir, &name))
            {
                continue;
            }
            if is_active(active_map, t.apply_kind, &name) {
                continue;
            }
            if !entry_valid(t, env.fs, &t.sys_dir, &name)
                || !entry_listed(env, t.apply_kind, &join(&t.sys_dir, &name))
            {
                continue;
            }
            let full = if t.entry == "colors" {
                join(&t.sys_dir, &format!("{name}.colors"))
            } else {
                join(&t.sys_dir, &name)
            };
            cands.push((ti, name, full));
        }
    }
    let qo_paths: Vec<String> = cands.iter().map(|c| c.2.clone()).collect();

    // 2) фильтр по владельцам (один batch -Qo снаружи)
    let mut items = Vec::new();
    for (ti, name, full) in &cands {
        let Some(pkg) = owners.get(full.trim_end_matches('/')) else {
            continue; // вручную скопировано → не системная
        };
        if foreign.contains(pkg) {
            continue; // AUR (foreign) → идёт через «Магазин: AUR»
        }
        let t = &types[*ti];
        items.push(mk_item(env, t, name, &t.sys_dir, false, n_("📦 System")));
    }
    Ok(SystemScan { items, qo_paths })
}

/// Раздел целиком (окружения или KDE): включённые сейчас, затем свои
/// (каталог пользователя), затем системные. Системные здесь не делятся по
/// пакетам, как в обзоре «Системные»: раздел показывает всё, что можно
/// включить, и происхождение файла на кнопку не влияет. Одна тема — одна
/// плитка: имя, найденное в двух каталогах (`~/.icons` и
/// `~/.local/share/icons`), показывается по первому.
pub fn scan_section(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let downloaded = scan_downloaded_in(env, types, active_map);
    for item in scan_active_in(env, types, active_map).into_iter().chain(downloaded) {
        if seen.insert(item.id.clone()) {
            items.push(item);
        }
    }
    for t in types {
        if t.sys_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.sys_dir, t) {
            if is_active(active_map, t.apply_kind, &name) || !entry_valid(t, env.fs, &t.sys_dir, &name) {
                continue;
            }
            // Курсоры и значки делят каталог icons/: чисто курсорная тема —
            // только в «Курсорах»; набор значков со своими курсорами (Adwaita,
            // elementary) — в обоих разделах (как в «Загруженных»).
            let dir = join(&t.sys_dir, &name);
            if (t.apply_kind == "cursor" && !is_cursor_dir(env.fs, &dir))
                || (t.apply_kind == "icons" && is_cursor_only(env.fs, &dir))
            {
                continue;
            }
            let item = mk_item(env, t, &name, &t.sys_dir, false, n_("📦 System"));
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
    }
    // Включённую сейчас тему не прячем никогда — человек обязан видеть, что
    // у него стоит; остальное — по общему правилу [`entry_listed`].
    items.retain(|it| {
        let kind = crate::de_apply::apply_kind_of_local_id(&it.id).unwrap_or("");
        it.local_kind.is_some() || entry_listed(env, kind, &it.install_path)
    });
    items
}

/// Тема ли вида `kind` лежит в каталоге `dir` — одно правило для обзоров
/// («Загруженные», «Системные») и для сетки раздела.
///
/// Каталоги `…/share/themes` и `…/share/icons` общие для нескольких видов:
/// в первом лежат и темы GTK, и темы, у которых есть только рамки окон
/// (xfwm4, Metacity, Openbox) или оболочка; во втором — служебные наборы
/// значков (`hicolor` со `Hidden=true`, `default` без своих каталогов).
/// Такая запись темой GTK или набором значков не является: включить её
/// нельзя, и страница KDE её в сетке не показывает. Рамки и оболочки
/// показывают свои разделы — по своему подкаталогу ([`de_types`]).
fn entry_listed(env: &ScanEnv, kind: &str, dir: &str) -> bool {
    let fs = env.fs;
    match kind {
        "icons" => fs.read_text(&join(dir, "index.theme")).is_some_and(|text| {
            let info = icon_theme_info(text);
            // Стандартный набор окружения виден и со `Hidden=true` (Adwaita
            // в GNOME): иначе вернуть значки по умолчанию было бы нечем.
            let standard = env.standard_icons.is_some_and(|s| dir.rsplit('/').next() == Some(s));
            info.has_group && info.has_dirs && (!info.hidden || standard)
        }),
        // Тема GTK — там, где GTK найдёт таблицу стилей `gtk.css`. Каталог
        // с одним `gtk-keys.css` (Default и Emacs из GTK — раскладки
        // клавиш) темой оформления не является: выбрать его — ничего не
        // изменить. Default остаётся в «Рамках окон» XFCE — по xfwm4.
        "gtk" => GTK_PARTS.iter().any(|part| fs.is_file(&join(dir, &format!("{part}/gtk.css")))),
        _ => true,
    }
}

/// Подкаталоги, по которым каталог в `…/share/themes` считается темой GTK:
/// таблицы стилей GTK 3 и GTK 4. Тему из одного GTK 2 нынешние окружения не
/// применяют (страница KDE её тоже не показывает).
pub const GTK_PARTS: &[&str] = &["gtk-3.0", "gtk-4.0"];

/// Что `index.theme` говорит о наборе значков (спецификация freedesktop
/// Icon Theme): группа `[Icon Theme]`, непустой список каталогов значков,
/// `Hidden=true`. Видимый набор — с группой и каталогами и не скрытый.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct IconThemeInfo {
    has_group: bool,
    has_dirs: bool,
    hidden: bool,
}

fn icon_theme_info(text: &str) -> IconThemeInfo {
    let mut in_group = false;
    let mut info = IconThemeInfo::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Icon Theme]";
            info.has_group |= in_group;
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if key == "Hidden" && value.eq_ignore_ascii_case("true") {
            info.hidden = true;
        }
        if (key == "Directories" || key == "ScaledDirectories") && !value.is_empty() {
            info.has_dirs = true;
        }
    }
    info
}

/// Системные темы (экран входа, заставка загрузки) без пакета-владельца —
/// в «Загруженные».
///
/// В «Системные» они не попадают (там только темы пакетов из репозиториев);
/// а установка теперь только кладёт тему (выбирает её «Применить», решение
/// 04.10.2026) — без этого скана поставленную тему негде было бы найти и
/// применить. Сюда идут два рода тем:
///   * поставленные самим Rexon — путь есть в журнале установленных
///     (`journal_paths` — все пути журнала);
///   * положенные руками или чужим установщиком — `loose`, пути каталогов,
///     о которых пакетная база ответила «ничьё» ([`LiveScan::loose_system_themes`]).
///     Прежде такая тема не была видна нигде, и убрать её человек не мог
///     (приёмка 10.10.2026).
pub fn scan_rexon_system(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
    journal_paths: &[String],
    loose: &HashSet<String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    for t in types {
        if !t.user_dir.is_empty() || t.sys_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.sys_dir, t) {
            let full = join(&t.sys_dir, &name);
            if !loose.contains(&full)
                && !journal_paths.iter().any(|p| p.trim_end_matches('/') == full)
            {
                continue;
            }
            if is_active(active_map, t.apply_kind, &name) || !entry_valid(t, env.fs, &t.sys_dir, &name) {
                continue;
            }
            let label = env.store.origin_label(&full);
            items.push(mk_item(env, t, &name, &t.sys_dir, false, label));
        }
    }
    items
}

/// Стиль приложений (Qt widget style) — порт `scan_widget_styles`.
/// `styles` — QStyleFactory.keys() (live) или `WIDGET_STYLES`-фолбэк.
pub fn scan_widget_styles(
    active_map: &HashMap<String, String>,
    styles: &[String],
    qt_only: bool,
) -> Vec<ThemeItem> {
    let active = active_map
        .get("widgetstyle")
        .map(|s| s.trim())
        .unwrap_or("");
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in styles {
        let low = name.to_lowercase();
        if seen.contains(&low) {
            continue;
        }
        if qt_only && low.starts_with("kvantum") {
            continue; // движок Kvantum — отдельный под-выбор
        }
        seen.insert(low.clone());
        let is_active = !active.is_empty() && low == active.to_lowercase();
        // Стиль приложений — движок Qt, а не каталог на диске: метаданных
        // с человеческим именем у него нет и быть не может.
        items.push(mk_local_item(
            "widgetstyle", 901, name, name, is_active,
            n_("🎛️ Application Style"), "🎛️", "", "",
        ));
    }
    items
}

/// Список Kvantum-тем (user + system) — порт `scan_kvantum_themes`.
/// Активная — когда widgetStyle == kvantum* И тема kvconfig совпадает.
pub fn scan_kvantum_themes(
    env: &ScanEnv,
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let ws = active_map
        .get("widgetstyle")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    let kv_on = ws.starts_with("kvantum");
    let active_theme = if kv_on {
        active_map.get("kvantum").map(|s| s.trim()).unwrap_or("")
    } else {
        ""
    };
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for base in kvantum_dirs(env.home, env.usr_root) {
        for name in list_entries(env.fs, &base, "dir") {
            if !seen.insert(name.clone()) {
                continue;
            }
            let theme_dir = join(&base, &name);
            let is_active = !active_theme.is_empty() && name == active_theme;
            let preview =
                resolve_preview_in(env, "kvantum", &name, &theme_dir, &theme_dir, None, &[]);
            let display =
                theme_display_name(env.fs, &theme_dir, &name, env.locale);
            let mut item = mk_local_item(
                "kvantum", 123, &name, &display, is_active, "🔲 Kvantum", "🔲",
                &theme_dir, &preview,
            );
            item.description = theme_description(env.fs, &theme_dir, env.locale);
            item.source_name = theme_source_name(env.fs, &theme_dir, &name, &display);
            items.push(item);
        }
    }
    items
}

/// Заставка (splash) / экран блокировки (lockscreen) — часть look-and-feel;
/// порт `scan_lnf_component`.
pub fn scan_lnf_component(
    env: &ScanEnv,
    kind: &str,
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let component = if kind == "splash" { "splash" } else { "lockscreen" };
    let (cat_id, icon, label) = if kind == "splash" {
        (902, "🌅", n_("🌅 KDE Splash Screen"))
    } else {
        (903, "🔒", n_("🔒 Lock Screen"))
    };
    let active = active_map.get(kind).map(|s| s.trim()).unwrap_or("");
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for base in lnf_dirs(env.home, env.usr_root) {
        for name in list_entries(env.fs, &base, "dir") {
            if seen.contains(&name) {
                continue;
            }
            let theme_dir = join(&base, &name);
            if !env
                .fs
                .is_dir(&join(&theme_dir, &format!("contents/{component}")))
            {
                continue;
            }
            seen.insert(name.clone());
            let is_active = !active.is_empty() && name == active;
            let preview = resolve_preview_in(
                env, kind, &name, &theme_dir, &theme_dir, Some(component), &[],
            );
            let display =
                theme_display_name(env.fs, &theme_dir, &name, env.locale);
            let mut item = mk_local_item(
                kind, cat_id, &name, &display, is_active, label, icon,
                &theme_dir, &preview,
            );
            item.description = theme_description(env.fs, &theme_dir, env.locale);
            item.source_name = theme_source_name(env.fs, &theme_dir, &name, &display);
            items.push(item);
        }
    }
    // Плитка «Нет» — заставка при входе не показывается (как у модуля KDE,
    // последней в сетке). Каталога у неё нет: ни удаления, ни показа.
    if kind == "splash" {
        let none = crate::appearance_installer::SPLASH_NONE;
        let mut item = mk_local_item(
            kind, cat_id, none, &crate::appearance_installer::splash_none_name(),
            active == none, label, icon, "", "",
        );
        item.description = crate::appearance_installer::splash_none_description();
        item.source_name = none.to_string();
        items.push(item);
    }
    items
}

// ============================================================================
// Живой обзор диска — порт исполнителя из
// `rexon/appearance/installed_scan.py` (`_rs_snapshot`, `_rs_active_sources`,
// `_rs_foreign_pkgs`, `_rs_owner_pkgs` и обёртки `_rs_scan_*`)
// ============================================================================
//
// Всё выше в этом модуле — ПРАВИЛА над готовым сырьём. Здесь — единственное
// место, где сырьё добывается с настоящей машины: os.scandir по дереву тем,
// чтение конфигов, запуск kreadconfig6/gsettings/пакетного менеджера.
// Второй копии правил тут нет и быть не должно: живой исполнитель только
// собирает вход и зовёт те же `scan_*`, что и мост Python.
//
// Без него вкладка «Оформление» в процессе без Python показывала пустые
// разделы: сканеры есть, а подать им снимок и карту активных тем некому.

/// `$HOME` — тот же, что подставляет Python (`os.path.expanduser("~")`).
fn live_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// Переменная окружения строкой (нет переменной — пусто), как `os.environ.get`.
fn env_str(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// `shutil.which` через общий резолвер ядра (учитывает `$PATH` процесса).
fn which_live(name: &str) -> Option<String> {
    crate::utils::which(name, std::env::var("PATH").ok().as_deref())
}

/// Текст файла для снимка: любая ошибка чтения → пусто (порт `_rs_read_text`).
/// Ядро тогда трактует metadata как битую — ровно как ветки ошибок Python.
fn snap_read_text(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Содержимое файла либо `None` — порт `_rs_read_optional` (нет файла / не
/// читается). Пустой файл и отсутствующий — РАЗНЫЕ ответы: на них расходятся
/// ветки `active_map_from_sources`.
fn read_optional(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if !p.is_file() {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// Имена элементов каталога в порядке `readdir` — тот же порядок, который
/// видел `os.scandir` Python-эталона. Сортировки здесь НЕТ намеренно:
/// «первое подходящее превью» выбирается по этому порядку, и сортировка
/// увела бы живой обзор от голденов моста.
fn scandir_names(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect()
}

/// Кандидаты превью глубже первого уровня темы — порт `_RS_PREVIEW_PROBES`.
/// В снимок кладутся только существующие: `find_preview` спрашивает по ним
/// `is_file`, а он отвечает ПО НАЛИЧИЮ КЛЮЧА.
const PREVIEW_PROBES: &[&str] = &[
    "contents/previews/preview.png",
    "contents/previews/fullscreenpreview.png",
    "contents/screenshot.png",
    "contents/previews/lockscreen.png",
    "contents/previews/splash.png",
    "metadata/preview.png",
    // Таблицы стилей GTK: по ним каталог считается темой GTK
    // ([`entry_listed`]).
    "gtk-3.0/gtk.css",
    "gtk-4.0/gtk.css",
    // Миниатюры видов в подкаталогах темы ([`preview_subdirs`]).
    "gtk-3.0/thumbnail.png",
    "gtk-4.0/thumbnail.png",
    "gtk-3.20/thumbnail.png",
    "gtk-2.0/thumbnail.png",
    "cinnamon/thumbnail.png",
    "metacity-1/thumbnail.png",
    "xfwm4/thumbnail.png",
    "gnome-shell/thumbnail.png",
];

/// Компоненты look-and-feel — порт `_RS_COMPONENT_PROBES`
/// (`scan_lnf_component` спрашивает по ним `is_dir`).
const COMPONENT_PROBES: &[&str] = &["contents/splash", "contents/lockscreen"];

/// Похоже ли значение JSON на «истинное» по правилам Python (`x or y`):
/// null / false / 0 / "" — ложь, остальное истина.
fn json_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Относительные пути `Preview=`/`Image=` из metadata — порт
/// `_rs_meta_preview_rels`. Суперсет того, что прочитает
/// [`preview_from_metadata`]: цель — зарегистрировать файл превью в снимке.
fn meta_preview_rels(meta_json: Option<&str>, meta_desktop: Option<&str>) -> Vec<String> {
    let mut rels: Vec<String> = Vec::new();
    if let Some(text) = meta_json.filter(|t| !t.is_empty()) {
        if let Ok(Value::Object(data)) = serde_json::from_str::<Value>(text) {
            let kp = data.get("KPlugin").and_then(Value::as_object);
            for key in ["Preview", "Image"] {
                let from_kp = kp.and_then(|k| k.get(key)).filter(|v| json_truthy(v));
                let rel = from_kp.or_else(|| data.get(key));
                if let Some(Value::String(s)) = rel {
                    if !s.is_empty() {
                        rels.push(s.clone());
                    }
                }
            }
        }
    }
    if let Some(text) = meta_desktop.filter(|t| !t.is_empty()) {
        for line in text.lines() {
            let s = line.trim();
            if s.starts_with("Preview=") || s.starts_with("Image=") || s.starts_with("Screenshot=") {
                if let Some((_, rest)) = s.split_once('=') {
                    let rel = rest.trim();
                    if !rel.is_empty() {
                        rels.push(rel.to_string());
                    }
                }
            }
        }
    }
    rels
}

/// Части снимка до сборки в [`FsSnapshot`] (порт словарей `dirs`/`files`).
#[derive(Default)]
struct SnapParts {
    dirs: HashMap<String, Vec<String>>,
    files: HashMap<String, String>,
}

/// Каталог ОДНОЙ темы в снимок — порт `_rs_snap_theme`: дети (файлы и
/// подпапки) плюс точечные пробы превью и компонентов на глубине 2-3.
fn snap_theme(theme_dir: &std::path::Path, parts: &mut SnapParts) {
    let key = theme_dir.to_string_lossy().into_owned();
    if parts.dirs.contains_key(&key) {
        return;
    }
    let names = scandir_names(theme_dir);
    parts.dirs.insert(key, names.clone());
    let mut meta_json: Option<String> = None;
    let mut meta_desktop: Option<String> = None;
    for name in &names {
        let path = theme_dir.join(name);
        let path_key = path.to_string_lossy().into_owned();
        if path.is_dir() {
            parts.dirs.entry(path_key).or_default();
        } else if path.is_file() {
            if name == "metadata.json" || name == "metadata.desktop" {
                let text = snap_read_text(&path);
                if name == "metadata.json" {
                    meta_json = Some(text.clone());
                } else {
                    meta_desktop = Some(text.clone());
                }
                parts.files.insert(path_key, text);
            } else if name == "index.theme" || name.ends_with(".plymouth") {
                // Иконки, курсоры и GTK-темы метаданных KDE не носят: их
                // человеческое имя лежит только здесь. У темы заставки
                // загрузки имя и описание — в `<имя>.plymouth`.
                parts.files.insert(path_key, snap_read_text(&path));
            } else {
                // Текст обычных файлов не читается: сканерам от них нужен
                // только факт существования, а темы носят мегабайтные .svg.
                parts.files.insert(path_key, String::new());
            }
        }
    }
    for rel in PREVIEW_PROBES {
        let p = theme_dir.join(rel);
        if p.is_file() {
            parts.files.insert(p.to_string_lossy().into_owned(), String::new());
        }
    }
    for rel in COMPONENT_PROBES {
        let p = theme_dir.join(rel);
        if p.is_dir() {
            parts.dirs.entry(p.to_string_lossy().into_owned()).or_default();
        }
    }
    for rel in meta_preview_rels(meta_json.as_deref(), meta_desktop.as_deref()) {
        if rel.starts_with('/') {
            continue; // абсолютный Preview= — экзотика, вне снимка (как в оригинале)
        }
        for base in [theme_dir.join("contents"), theme_dir.to_path_buf()] {
            let p = base.join(&rel);
            if p.is_file() {
                parts.files.insert(p.to_string_lossy().into_owned(), String::new());
            }
        }
    }
}

/// Живой снимок дерева тем — порт `_rs_snapshot`.
///
/// Глубина ровно та, к которой обращаются сканеры (см. заголовок модуля):
/// корни, их дети-темы, метаданные текстом и точечные пробы. Обходить
/// `~/.local/share/icons` целиком нельзя — это десятки тысяч файлов на
/// каждое открытие вкладки.
///
/// `extra_files` — локальные файлы превью из кеша магазина: цель бэкофилла
/// проверяется через `is_file`, и незарегистрированный файл молча стал бы
/// «несуществующим».
pub fn live_snapshot(roots: &[String], extra_files: &[String]) -> FsSnapshot {
    let mut parts = SnapParts::default();
    for root in roots {
        if root.is_empty() || parts.dirs.contains_key(root) {
            continue;
        }
        let root_path = std::path::Path::new(root);
        if !root_path.is_dir() {
            continue;
        }
        let mut names = scandir_names(root_path);
        // Наши ссылки `~/.themes` → `~/.local/share/themes` темой не
        // считаются: тема та же, и в списках она была бы дважды.
        if let Some(home) = root.trim_end_matches('/').strip_suffix("/.themes") {
            names.retain(|n| !is_theme_link(home, n));
        }
        parts.dirs.insert(root.clone(), names.clone());
        for name in &names {
            let path = root_path.join(name);
            if path.is_file() {
                // Цветовая схема — файл, а не каталог: имя для человека
                // лежит внутри него, и без текста карточка осталась бы
                // подписанной именем файла.
                let text = if name.ends_with(".colors") {
                    snap_read_text(&path)
                } else {
                    String::new()
                };
                parts.files.insert(path.to_string_lossy().into_owned(), text);
            } else if path.is_dir() {
                snap_theme(&path, &mut parts);
            }
        }
    }
    // сайдкар-кеш превью (типы без каталога темы — цвет-схемы)
    let sidecar = sidecar_preview_dir(&live_home());
    let sidecar_path = std::path::Path::new(&sidecar);
    if sidecar_path.is_dir() {
        for name in scandir_names(sidecar_path) {
            let p = sidecar_path.join(name);
            if p.is_file() {
                parts.files.insert(p.to_string_lossy().into_owned(), String::new());
            }
        }
    }
    for p in extra_files {
        parts.files.insert(p.clone(), String::new());
    }
    FsSnapshot::from_parts(parts.dirs, parts.files)
}

impl FsSnapshot {
    /// Снимок из живых частей: каталог → имена, файл → текст.
    ///
    /// Пара к [`FsSnapshot::from_json`] для исполнителей, которые собирают
    /// снимок сами (живой обзор диска, установщик). Поля приватные не просто
    /// так: `is_dir`/`is_file` отвечают ПО НАЛИЧИЮ КЛЮЧА, и класть сюда
    /// половину дерева — значит молча получить «файла нет».
    pub fn from_parts(
        dirs: HashMap<String, Vec<String>>,
        files: HashMap<String, String>,
    ) -> Self {
        FsSnapshot { dirs, files }
    }
}

// ── Запуск внешних команд ────────────────────────────────────────────────

/// Ответ внешней команды: код возврата, stdout, stderr.
///
/// `None` вместо ответа — спросить не вышло (бинаря нет, таймаут, процесс
/// не завёлся). Это НЕ ненулевой код: у `pacman -Qm` «не спросили» и
/// «спросили, ответил ошибкой» ведут к РАЗНЫМ решениям (порт `_run_pkg_query`).
fn run_capture(argv: &[String], timeout_secs: u64, lang_c: bool) -> Option<(i32, String, String)> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let (head, args) = argv.split_first()?;
    // Та же ЕДИНАЯ точка подмены имени, что у `safe_run`: на Plasma 5
    // `kreadconfig6` называется `kreadconfig5`.
    let resolved = crate::utils::tool_cmd(head, args, std::env::var("PATH").ok().as_deref());
    let (bin, rest) = resolved.split_first()?;
    let mut command = Command::new(bin);
    command
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if lang_c {
        // Русская локаль переводит и ответ менеджера, и сообщение о занятой
        // базе — разбор в ядре тогда не находит ничего.
        crate::utils::parsed_output_locale(&mut command);
    }
    let mut child = command.spawn().ok()?;
    // Обе трубы читаются ПАРАЛЛЕЛЬНО: `pacman -Qo` на две сотни путей пишет
    // в stderr строку на каждый ничей файл, и чтение по очереди упёрлось бы
    // в переполненный буфер трубы — процесс встал бы навсегда.
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = out_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = err_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => break None,
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let code = status?.code()?;
    Some((
        code,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

/// Запуск с таймаутом по классу команды (`utils::default_timeout_secs`) —
/// так же, как его подставляет `safe_run` без явного `timeout`.
fn run_default(argv: &[String]) -> Option<(i32, String, String)> {
    let t = crate::utils::default_timeout_secs(argv);
    run_capture(argv, t, false)
}

/// Запрос движка пакетов — порт `_run_pkg_query`: проверить бинарь, взять
/// argv, окружение и таймаут из плана ядра, запустить. Решений здесь нет.
fn run_pkg_query(query: &crate::system_plan::InstalledPkgQuery) -> Option<(i32, String, String)> {
    which_live(&query.binary)?;
    let cmd = &query.cmd;
    let timeout = cmd
        .timeout_secs
        .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
    run_capture(&cmd.argv, timeout, cmd.lang_c)
}

// ── Пакетный менеджер: происхождение пакетов и владельцы файлов ──────────

/// Одна попытка `pacman -Qm` в том же виде, в каком её копил Python-эталон
/// («ok»/«fail»/«error» + полезная нагрузка). Хранится так, а не готовым
/// [`QmAttempt`], потому что политике повторов список подаётся ДВАЖДЫ: сам
/// по себе и с виртуальной удачной попыткой на конце.
type AttemptRaw = (&'static str, String);

fn to_qm_attempts(raw: &[AttemptRaw]) -> Vec<QmAttempt> {
    raw.iter()
        .map(|(kind, payload)| match *kind {
            "ok" => QmAttempt::Ok(payload.clone()),
            "fail" => QmAttempt::Fail(payload.clone()),
            _ => QmAttempt::Error,
        })
        .collect()
}

/// Множество пакетов не из репозиториев — порт `_rs_foreign_pkgs`.
///
/// `Some` — определили (пустой набор тоже ответ). `None` — достоверно
/// определить не удалось, и тогда «Системные» показывать нельзя: AUR-темы
/// протекли бы в них молча.
fn live_foreign_pkgs(query: &crate::system_plan::InstalledPkgQuery) -> Option<HashSet<String>> {
    let mut attempts: Vec<AttemptRaw> = Vec::new();
    for _ in 0..2 {
        match run_pkg_query(query) {
            None => attempts.push(("error", String::new())),
            Some((0, out, _)) => attempts.push(("ok", out)),
            Some((_, _, err)) => attempts.push(("fail", err)),
        }
        let (result, _used) = foreign_pkgs_from_attempts(true, &to_qm_attempts(&attempts));
        if let Some(set) = result {
            return Some(set);
        }
        // None: стоп-ошибка или «повторить»? Спрашиваем ту же политику
        // виртуальной удачной попыткой: продолжила бы — значит нужен ретрай.
        let mut probe = attempts.clone();
        probe.push(("ok", String::new()));
        let (probe_result, _) = foreign_pkgs_from_attempts(true, &to_qm_attempts(&probe));
        if probe_result.is_none() {
            break; // не-lck ошибка → повтор не поможет
        }
        if attempts.last().map(|a| a.0) == Some("fail") {
            // база занята чужой транзакцией → подождать и повторить
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    }
    None
}

/// `путь → пакет-владелец` — порт `_rs_owner_pkgs`: команды и разрезание на
/// пачки задаёт движок, разбор ответа — [`parse_qo_output`], здесь запуск.
///
/// Ненулевой код — норма: он означает лишь, что у части путей владельца нет
/// («error: No package owns …» уходит в stderr и разбору не мешает).
/// Пути из `paths`, у которых нет пакета-владельца. Любая неясность (нет
/// запроса для семейства, нет программы, запрос не выполнился) — пусто.
fn live_loose_paths(family: crate::system::PkgFamily, paths: &[String]) -> HashSet<String> {
    let none = HashSet::new();
    if paths.is_empty() {
        return none;
    }
    let Some(queries) = crate::system_plan::file_owner_queries(family, paths) else {
        return none;
    };
    // «Ничьё» — только то, о чём база сказала это сама (строка ошибки с
    // путём): молчание или сбой запроса ничьим путь не делают.
    let mut loose = HashSet::new();
    for query in &queries {
        let Some((_, _, err)) = run_pkg_query(query) else {
            return none;
        };
        loose.extend(parse_qo_unowned(&err));
    }
    paths.iter().filter(|p| loose.contains(p.trim_end_matches('/'))).cloned().collect()
}

/// Пути, о которых `pacman -Qo` (под `LANG=C`) ответил «No package owns …».
pub fn parse_qo_unowned(stderr: &str) -> HashSet<String> {
    const MARK: &str = "No package owns ";
    stderr
        .lines()
        .filter_map(|line| line.find(MARK).map(|at| &line[at + MARK.len()..]))
        .map(|p| p.trim().trim_end_matches('/').to_string())
        .filter(|p| p.starts_with('/'))
        .collect()
}

fn live_owner_pkgs(
    family: crate::system::PkgFamily,
    paths: &[String],
) -> HashMap<String, String> {
    let mut owners = HashMap::new();
    let Some(queries) = crate::system_plan::file_owner_queries(family, paths) else {
        return owners;
    };
    for query in &queries {
        let out = run_pkg_query(query).map(|(_, out, _)| out).unwrap_or_default();
        owners.extend(parse_qo_output(&out));
    }
    owners
}

// ── Источники карты активных тем ─────────────────────────────────────────

/// Прочитанные с машины источники `active_map` — владеющая пара к
/// [`ActiveMapSources`] (та держит только ссылки).
#[derive(Default)]
struct LiveSources {
    kde_global: String,
    kde_plasma_style: String,
    kde_colors: String,
    icons: String,
    mouse_cursor: String,
    kcminput_cursor: String,
    xcursor_env: String,
    widget_style: String,
    gtk_settings_ini: Option<String>,
    splash: Option<String>,
    lockscreen: Option<String>,
    kwin_decoration_theme: Option<String>,
    kwin_decoration_library: Option<String>,
    kvantum_kvconfig: Option<String>,
    qt_style_override_env: String,
    qt_style_env_file: Option<String>,
    lxqt_conf: Option<String>,
    qtct_conf: Option<String>,
    native_gtk: Option<String>,
    native_icons: Option<String>,
    native_cursor: Option<String>,
    sddm_confs: Vec<String>,
    plymouth_confs: Vec<String>,
    plasma_login: bool,
    sound_theme: Option<String>,
    wallpaper_current: Option<String>,
}

/// Живой обзор диска: снимок ФС и опрос машины на месте вызова.
///
/// Объект действий вкладки «Оформление» держит его между обновлениями
/// списка: кеш магазина берётся один раз (`LiveCache::all_items_min` /
/// `all_previews_min`), а дерево тем и карта активных тем перечитываются на
/// каждый скан — темы ставят и применяют прямо во время работы вкладки.
pub struct LiveScan {
    pub home: String,
    pub usr_root: String,
    /// (имя, ссылка превью) закешированных тем магазина — сырьё бэкофилла.
    pub ocs_cache: Vec<(String, String)>,
    /// `ссылка превью → локальный файл` (только существующие файлы).
    pub preview_files: HashMap<String, String>,
    /// Записи магазина об установленном ([`LiveScan::with_store_records`]).
    pub store: StoreRecords,
    /// Рисует ли экран входа Plasma Login Manager. `None` — спросить машину
    /// на месте (`appearance_installer::active_login_manager`, может звать
    /// `systemctl`); вызывающий из потока интерфейса, которому SDDM-тема не
    /// нужна, ставит ответ заранее и в `systemctl` не ходит.
    pub plasma_login: Option<bool>,
    /// Рабочий стол, под который смотрит вкладка (`None` — без разделов
    /// окружений, как прежде). С ним в обзоры диска входят разделы
    /// окружений этого стола ([`de_types`]) с правдивым «включено сейчас».
    pub de: Option<String>,
    /// Поднимать ли при скане темы, которые прежние установки положили на
    /// уровень глубже (`appearance_installer::migrate_nested_themes_once`).
    /// Стенды, которым положено только читать диск, снимают это
    /// [`LiveScan::read_only`].
    pub migrate: bool,
    /// Стандартная тема значков окружения `de` (умолчание схемы, см.
    /// [`crate::appearance_installer::live_standard_theme`]); `None` — не
    /// узнавали или у окружения её нет.
    pub standard_icons: Option<String>,
}

impl LiveScan {
    /// `ocs_cache`/`preview_files` — из живого кеша магазина; пустые
    /// допустимы (тогда бэкофилл превью просто не сработает).
    pub fn new(
        ocs_cache: Vec<(String, String)>,
        preview_files: HashMap<String, String>,
    ) -> Self {
        LiveScan {
            home: live_home(),
            usr_root: "/usr".to_string(),
            ocs_cache,
            preview_files,
            store: StoreRecords::default(),
            plasma_login: None,
            de: None,
            migrate: true,
            standard_icons: None,
        }
    }

    /// Тот же скан с записями магазина об установленном: `sections` — пары
    /// «ссылка на картинку кеша — номер категории её товара»
    /// (`LiveCache::all_preview_categories`), реестр установок KNewStuff
    /// читается с диска здесь же.
    pub fn with_store_records(mut self, sections: Vec<(String, i64)>) -> Self {
        let home = std::path::Path::new(&self.home);
        let cats = crate::categories::categories(home);
        let key_of = |id: i64| {
            cats.iter()
                .find(|c| c.id == id)
                .or_else(|| cats.iter().find(|c| c.id == crate::categories::section_id_for(id)))
                .map(|c| c.key.clone())
        };
        self.store.sections = sections
            .into_iter()
            .filter(|(url, _)| !url.is_empty())
            .map(|(url, id)| (url, key_of(id).unwrap_or_default()))
            .collect();
        self.store.rexon_paths = Some(crate::appearance_installer::installed_log_paths());
        let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &self.home);
        let registry = std::path::Path::new(&data).join("knewstuff3");
        for name in scandir_names(&registry) {
            if name.ends_with(".knsregistry") {
                self.store.installed.extend(parse_kns_registry(&snap_read_text(&registry.join(&name))));
            }
        }
        self
    }

    /// Тот же скан, но диск только читается: вложенные темы не поднимаются.
    pub fn read_only(mut self) -> Self {
        self.migrate = false;
        self
    }

    /// Подъём вложенных тем перед сканом (раз на процесс), если он не снят.
    fn migrate_nested(&self) {
        if self.migrate {
            crate::appearance_installer::migrate_nested_themes_once(&self.home);
        }
        self.link_themes();
    }

    /// Ссылки `~/.themes` на темы из `~/.local/share/themes`
    /// ([`sync_theme_links`]) — на каждом скане, если он не только читает.
    fn link_themes(&self) {
        if self.migrate {
            for line in sync_theme_links(&self.home) {
                crate::log_info!("theme", "{line}");
            }
        }
    }

    /// Тот же скан с разделами окружения `de` (ключ рабочего стола Rexon).
    pub fn with_de(mut self, de: &str) -> Self {
        self.de = (!de.is_empty()).then(|| de.to_string());
        self.standard_icons = crate::appearance_installer::live_standard_theme(de, "icons");
        self
    }

    /// Типы обзоров диска: прежние плюс разделы окружения, если оно задано.
    fn types(&self) -> Vec<ScanType> {
        let mut types = types_for(&self.home, &self.usr_root);
        if let Some(de) = &self.de {
            types.extend(de_types(&self.home, &self.usr_root, de));
        }
        types
    }

    /// Что включено сейчас в разделах окружения — у каждого раздела свой
    /// читатель (`de_apply::active_names`), значения через перевод строки.
    fn de_active(&self) -> Vec<(String, String)> {
        let Some(de) = &self.de else { return Vec::new() };
        let ctx = crate::de_apply::Ctx {
            home: self.home.clone(),
            xdg_config_home: std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty()),
            // Тема рамок LXQt читается из `rc.xml` того менеджера, что выбран
            // для сеанса: `window_manager` на X11, `compositor` на Wayland.
            wayland: crate::de_apply::Ctx::live().wayland,
            de: de.clone(),
        };
        let mut out = Vec::new();
        for (_, key) in crate::categories::DE_SECTIONS {
            if !crate::categories::category_available(key, de) {
                continue;
            }
            let Some(kind) = crate::categories::category_kind(key) else { continue };
            let names = crate::de_apply::active_names(&crate::animations::SystemIo, &ctx, kind);
            if !names.is_empty() {
                out.push((key.to_string(), names.join("\n")));
            }
        }
        out
    }

    /// Все каталоги типов (user + system) без повторов, в порядке TYPES —
    /// порт `_rs_scan_roots`.
    fn scan_roots(&self) -> Vec<String> {
        let mut roots: Vec<String> = Vec::new();
        for t in self.types() {
            for d in [t.user_dir, t.sys_dir] {
                if !d.is_empty() && !roots.contains(&d) {
                    roots.push(d);
                }
            }
        }
        roots
    }

    /// `expand_user` проекта: ведущий `~/.config` уходит в `$XDG_CONFIG_HOME`.
    fn expand(&self, path: &str) -> String {
        crate::utils::expand_user(
            path,
            &self.home,
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        )
    }

    /// Снимок + окружение сканеров — порт `_rs_env`.
    ///
    /// `with_cache = false` — скан без кеша магазина (превью только из
    /// каталога темы).
    fn with_env<R>(
        &self,
        roots: &[String],
        with_cache: bool,
        body: impl FnOnce(&ScanEnv) -> R,
    ) -> R {
        let no_cache: Vec<(String, String)> = Vec::new();
        let extra: Vec<String> = if with_cache {
            self.preview_files.values().cloned().collect()
        } else {
            Vec::new()
        };
        let fs = live_snapshot(roots, &extra);
        let preview_file = |url: &str| {
            if with_cache {
                self.preview_files.get(url).cloned()
            } else {
                None
            }
        };
        // Имя темы — на языке ИНТЕРФЕЙСА Rexon, а не локали системы: человек
        // выбрал язык программы, и `Name[<язык>]` должно ему следовать.
        let locale = crate::i18n::current_language();
        let env = ScanEnv {
            home: &self.home,
            usr_root: &self.usr_root,
            fs: &fs,
            ocs_cache: if with_cache { &self.ocs_cache } else { &no_cache },
            preview_file: &preview_file,
            store: if with_cache { &self.store } else { StoreRecords::none() },
            locale: &locale,
            standard_icons: self.standard_icons.as_deref(),
        };
        body(&env)
    }

    /// Порт `ThemeManager._kde_get`: сперва `kreadconfig6` (он подхватывает
    /// системные defaults и активный Look-and-Feel), затем прямой разбор
    /// `kdeglobals`. Пусто — значения нет нигде.
    fn kde_get(&self, section: &str, key: &str) -> String {
        let cmd = crate::themes::kde_read_cmd(section, key);
        let timeout = cmd
            .timeout
            .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
        if let Some((0, out, _)) = run_capture(&cmd.argv, timeout, false) {
            let value = out.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
        let Some(content) = read_optional(&self.expand("~/.config/kdeglobals")) else {
            return String::new();
        };
        crate::themes::kdeglobals_value(&content, section, key).unwrap_or_default()
    }

    /// Порт `_rs_kread`: stdout `kreadconfig6` при коде 0, иначе `None`.
    /// Файл здесь ЛЮБОЙ (ksplashrc, kwinrc), поэтому argv свой, а не
    /// `themes::kde_read_cmd` — тот прибит к `kdeglobals` и к `--default ""`.
    fn kread(&self, file: &str, group: &str, key: &str) -> Option<String> {
        let argv: Vec<String> = ["kreadconfig6", "--file", file, "--group", group, "--key", key]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        match run_default(&argv) {
            Some((0, out, _)) => Some(out),
            _ => None,
        }
    }

    /// Ответ команды чтения звуковой темы — порт `_rs_sound_theme`.
    /// Чем читать, знает движок звука вкладки «Настройки»: второго способа
    /// спросить нет.
    fn sound_theme(&self, de: &str) -> Option<String> {
        let kde_tool = which_live("kreadconfig6").unwrap_or_default();
        let de = if de.is_empty() { "kde" } else { de };
        let argv = crate::qt_settings::sound_read_argv_for(
            de,
            crate::qt_settings::SoundSetting::Theme,
            &kde_tool,
        )?;
        if argv.is_empty() {
            return None;
        }
        match run_default(&argv) {
            Some((0, out, _)) => Some(out),
            _ => None,
        }
    }

    /// Текущие обои рабочего стола — порт `_rs_current_wallpaper`.
    ///
    /// KDE: все контейнменты `plasma-org.kde.plasma.desktop-appletsrc`
    /// (номер рабочего стола не угадывается — разбор в движке обоев).
    /// Остальные столы: та же ветка gsettings, которой пользуется движок.
    fn current_wallpaper(&self, de: &str) -> Option<String> {
        if crate::wallpaper_engine::uses_plasma_config(de) {
            let text =
                read_optional(&self.expand("~/.config/plasma-org.kde.plasma.desktop-appletsrc"));
            let images =
                crate::wallpaper_engine::kde_wallpaper_images(text.as_deref().unwrap_or(""));
            return images.into_iter().next();
        }
        let (schema, key) = crate::wallpaper_engine::gsettings_schema(de)?;
        let argv = crate::wallpaper_engine::gsettings_get_argv(schema, key);
        match run_default(&argv) {
            Some((0, out, _)) if !out.is_empty() => Some(out),
            _ => None,
        }
    }

    /// Все источники карты активных тем — порт `_rs_active_sources`.
    ///
    /// Порядок и ЛЕНИВОСТЬ побочных вызовов те же, что у эталона: цепочка
    /// курсора спрашивает `kcminputrc` и `$XCURSOR_THEME` только когда
    /// предыдущий источник пуст, иначе вкладка на каждом обновлении читала
    /// бы файлы, ответ которых заведомо не понадобится.
    fn active_sources(&self) -> LiveSources {
        let mut src = LiveSources::default();
        let kde_cmds = crate::themes::get_current_kde_cmds();
        let mut kde_values: Vec<String> = Vec::with_capacity(3);
        for cmd in kde_cmds.iter() {
            let timeout = cmd
                .timeout
                .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
            let value = match run_capture(&cmd.argv, timeout, false) {
                Some((0, out, _)) if !out.is_empty() => out.trim().to_string(),
                // «—» — тот же признак «не прочиталось», что у Python-эталона;
                // `clean` выше по модулю превращает его в пусто.
                _ => "—".to_string(),
            };
            kde_values.push(value);
        }
        src.kde_global = kde_values[0].clone();
        src.kde_plasma_style = kde_values[1].clone();
        src.kde_colors = kde_values[2].clone();

        let de = crate::system::detect_current_de(
            &env_str("XDG_CURRENT_DESKTOP"),
            &env_str("DESKTOP_SESSION"),
            !env_str("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default();
        // Окружение со своим хранилищем оформления (gsettings, xfconf,
        // DConfig) спрашивается первым; kdeglobals и прочие запасные
        // источники читаются, только если оно ответа не дало.
        let [native_gtk, native_icons, native_cursor] = native_role_values(&de, |cmd| {
            let timeout = cmd
                .timeout
                .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
            match run_capture(&cmd.argv, timeout, false) {
                Some((0, out, _)) if !out.trim().is_empty() => Some(out),
                _ => None,
            }
        });
        if native_icons.is_none() {
            src.icons = self.kde_get("Icons", "Theme");
        }
        let mouse = if native_cursor.is_none() {
            self.kde_get("Mouse", "cursorTheme")
        } else {
            String::new()
        };
        if native_cursor.is_none() && clean(&mouse).is_empty() {
            let kcminputrc = read_optional(&self.expand("~/.config/kcminputrc"));
            src.kcminput_cursor = crate::themes::kde_kcminput_cursor(kcminputrc.as_deref());
            if clean(&src.kcminput_cursor).is_empty() {
                src.xcursor_env = env_str("XCURSOR_THEME");
            }
        }
        src.mouse_cursor = mouse;
        src.native_gtk = native_gtk;
        src.native_icons = native_icons;
        src.native_cursor = native_cursor;
        src.widget_style = self.kde_get("KDE", "widgetStyle");

        src.gtk_settings_ini = read_optional(&self.expand("~/.config/gtk-3.0/settings.ini"));
        // Заставка выключена (`Engine` не KSplashQML) — текущая плитка «Нет».
        let engine = self.kread("ksplashrc", "KSplash", "Engine");
        src.splash = crate::appearance_installer::splash_active_value(
            self.kread("ksplashrc", "KSplash", "Theme"),
            engine.as_deref(),
        );
        src.lockscreen = self.kread("kscreenlockerrc", "Greeter", "Theme");
        src.kwin_decoration_theme = self.kread("kwinrc", "org.kde.kdecoration2", "theme");
        src.kwin_decoration_library = self.kread("kwinrc", "org.kde.kdecoration2", "library");
        src.kvantum_kvconfig = read_optional(&self.expand("~/.config/Kvantum/kvantum.kvconfig"));
        src.qt_style_override_env = env_str("QT_STYLE_OVERRIDE");
        src.qt_style_env_file =
            read_optional(&self.expand("~/.config/environment.d/rexon-qt-style.conf"));
        src.lxqt_conf = read_optional(&self.expand("~/.config/lxqt/lxqt.conf"));
        src.qtct_conf = ["qt6ct", "qt5ct"]
            .iter()
            .find_map(|t| read_optional(&self.expand(&format!("~/.config/{t}/{t}.conf"))));

        // Файлы Plasma Login Manager сюда больше не входят: ключа
        // `[Theme] Current` у него нет (см. `active_map_from_sources`).
        // Порядок чтения — как у самого SDDM (`/usr/lib/sddm/sddm.conf.d`,
        // `/etc/sddm.conf.d`, `/etc/sddm.conf` последним): иначе `Current=`
        // из `/etc/sddm.conf` (CachyOS ставит туда `breeze`) проигрывал
        // файлам каталога, и выделялась не та тема, что на экране входа.
        let list = |dir: &str| scandir_names(std::path::Path::new(dir));
        let sddm_paths = crate::system_plan::sddm_theme_files(&list);
        src.sddm_confs = sddm_paths.iter().filter_map(|p| read_optional(p)).collect();
        src.plasma_login = self.plasma_login.unwrap_or_else(|| {
            crate::system_plan::is_plasma_login(
                &crate::appearance_installer::active_login_manager(),
            )
        });
        let alt = format!("{}/default.plymouth", crate::system_plan::PLYMOUTH_THEMES_DIR);
        let alt_theme = std::fs::canonicalize(&alt)
            .ok()
            .and_then(|real| crate::system_plan::plymouth_theme_from_link(real.to_str()));
        src.plymouth_confs = plymouth_confs_by_weight(
            read_optional(crate::system_plan::PLYMOUTH_DEFAULTS),
            alt_theme.as_deref(),
            read_optional("/usr/share/plymouth/plymouthd.conf"),
            read_optional("/etc/plymouth/plymouthd.conf"),
        );

        src.sound_theme = self.sound_theme(&de);
        src.wallpaper_current = self.current_wallpaper(&de);
        src
    }

    /// Карта применённых сейчас тем (порт `_rs_active_map`).
    pub fn active_map(&self) -> HashMap<String, String> {
        let raw = self.active_sources();
        let sources = ActiveMapSources {
            kde_global: &raw.kde_global,
            kde_plasma_style: &raw.kde_plasma_style,
            kde_colors: &raw.kde_colors,
            icons: &raw.icons,
            mouse_cursor: &raw.mouse_cursor,
            kcminput_cursor: &raw.kcminput_cursor,
            xcursor_env: &raw.xcursor_env,
            gtk_settings_ini: raw.gtk_settings_ini.as_deref(),
            widget_style: &raw.widget_style,
            splash: raw.splash.as_deref(),
            lockscreen: raw.lockscreen.as_deref(),
            kwin_decoration_theme: raw.kwin_decoration_theme.as_deref(),
            kwin_decoration_library: raw.kwin_decoration_library.as_deref(),
            kvantum_kvconfig: raw.kvantum_kvconfig.as_deref(),
            sddm_confs: sddm_confs_for_active_map(
                raw.sddm_confs.iter().map(String::as_str).collect(),
                raw.plasma_login,
            ),
            plymouth_confs: raw.plymouth_confs.iter().map(String::as_str).collect(),
            qt_style_override_env: &raw.qt_style_override_env,
            qt_style_env_file: raw.qt_style_env_file.as_deref(),
            lxqt_conf: raw.lxqt_conf.as_deref(),
            qtct_conf: raw.qtct_conf.as_deref(),
            native_gtk: raw.native_gtk.as_deref(),
            native_icons: raw.native_icons.as_deref(),
            native_cursor: raw.native_cursor.as_deref(),
            sound_theme: raw.sound_theme.as_deref(),
            wallpaper_current: raw.wallpaper_current.as_deref(),
            home: &self.home,
            usr_root: &self.usr_root,
        };
        let mut map: HashMap<String, String> = active_map_from_sources(&sources)
            .into_iter()
            .map(|(k, v)| (k, v.as_str().unwrap_or("").to_string()))
            .collect();
        map.extend(self.de_active());
        map
    }

    /// Темы, применённые СЕЙЧАС (любой источник).
    pub fn active(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        self.migrate_nested();
        let types = self.types();
        let mut items = self.with_env(&self.scan_roots(), true, |env| scan_active_in(env, &types, active_map));
        // Встроенная рамка KDE (Breeze по умолчанию) — только в сеансе KDE:
        // в другом столе `kwinrc` ничего не рисует.
        let kde = self.de.as_deref().and_then(crate::de_customization::de_from_id)
            == Some(crate::de_customization::De::Kde);
        if kde && active_map.contains_key("kdecoration") {
            items.extend(self.kdecorations(active_map).into_iter().filter(|it| it.local_kind.is_some()));
        }
        items
    }

    /// Пользовательские темы в `~/.local/share/…`, НЕ активные.
    pub fn downloaded(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        // Темы, которые прежние установки положили на уровень глубже, видны
        // только после подъёма (раз на процесс; см. appearance_installer).
        self.migrate_nested();
        let types = self.types();
        let journal = crate::appearance_installer::installed_log_paths();
        let loose = self.loose_system_themes();
        self.with_env(&self.scan_roots(), true, |env| {
            let mut items = scan_downloaded_in(env, &types, active_map);
            items.extend(scan_rexon_system(env, &types, active_map, &journal, &loose));
            items
        })
    }

    /// Каталоги тем экрана входа и заставки загрузки, у которых нет
    /// пакета-владельца: их положили руками или чужим установщиком.
    ///
    /// Пусто, если пакетная база на вопрос «чей это файл» не отвечает
    /// (семейство без такого запроса, нет самой программы, запрос не
    /// выполнился): без ответа «ничьё» от «пакетного» не отличить, а тема
    /// пакета в «Загруженных» с кнопкой «Удалить» — обман.
    pub fn loose_system_themes(&self) -> HashSet<String> {
        let mut paths: Vec<String> = Vec::new();
        for t in self.types() {
            if !t.user_dir.is_empty() || t.sys_dir.is_empty() {
                continue;
            }
            let Ok(dir) = std::fs::read_dir(&t.sys_dir) else { continue };
            for entry in dir.flatten() {
                // Ссылку не разыменовываем: помощник снимает только обычный каталог.
                if entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                    paths.push(join(&t.sys_dir, &entry.file_name().to_string_lossy()));
                }
            }
        }
        live_loose_paths(crate::system::DistroDetector::detect_live().family_live(), &paths)
    }

    /// Темы в `/usr/share/…` с пакетом ИЗ РЕПОЗИТОРИЕВ, НЕ активные.
    ///
    /// `Err(текст)` — происхождение пакетов не определилось (порт
    /// `SystemScanUnavailable`): вкладка показывает явный статус, а AUR-темы
    /// не протекают в «Системные». Система, которая о происхождении пакета
    /// не знает в принципе, честно отдаёт пустой список.
    pub fn system(
        &self,
        active_map: &HashMap<String, String>,
    ) -> Result<Vec<ThemeItem>, String> {
        let family = crate::system::DistroDetector::detect_live().family_live();
        let Some(query) = crate::system_plan::foreign_packages_query(family) else {
            return Ok(Vec::new()); // семейство не умеет отвечать о происхождении
        };
        if which_live(&query.binary).is_none() {
            return Ok(Vec::new());
        }
        let foreign = live_foreign_pkgs(&query);
        // Скан двухфазный: первый вызов с пустой картой владельцев отдаёт
        // пути для вопроса «чей это файл», второй — уже элементы.
        let types = self.types();
        self.with_env(&self.scan_roots(), true, |env| {
            let Some(foreign) = foreign.as_ref() else {
                // Тот же отказ, что у `scan_system` без ответа pacman -Qm.
                return scan_system(env, active_map, true, None, &HashMap::new()).map(|s| s.items);
            };
            let empty: HashMap<String, String> = HashMap::new();
            let first = scan_system_in(env, &types, active_map, foreign, &empty)?;
            let owners = live_owner_pkgs(family, &first.qo_paths);
            let second = scan_system_in(env, &types, active_map, foreign, &owners)?;
            Ok(second.items)
        })
    }

    /// Раздел `apply_kind` целиком ([`scan_section`]): раздел окружения
    /// (`categories::DE_SECTIONS`) либо раздел KDE с сеткой установленного
    /// (`categories::KDE_INSTALLED_SECTIONS`). Текущая тема — из
    /// `active_map` (ключ — тот же `apply_kind`), с `local_kind = "active"`.
    pub fn section(&self, apply_kind: &str, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        let types = section_types(&self.home, &self.usr_root, apply_kind);
        if crate::categories::KDE_INSTALLED_SECTIONS.iter().any(|(_, k)| *k == apply_kind) {
            // Как у «Загруженных»: темы, что прежние установки положили на
            // уровень глубже, видны только после подъёма.
            self.migrate_nested();
        } else {
            self.link_themes();
        }
        let mut roots: Vec<String> = Vec::new();
        for t in &types {
            for d in [&t.user_dir, &t.sys_dir] {
                if !d.is_empty() && !roots.contains(d) {
                    roots.push(d.clone());
                }
            }
        }
        let mut items = self.with_env(&roots, true, |env| scan_section(env, &types, active_map));
        // Встроенные рамки KDE (Breeze, Oxygen, темы QML Aurorae) — рядом с
        // темами SVG, как в окне KDE; текущая — по `library` из `kwinrc`.
        if apply_kind == "aurorae" {
            let builtin = self.kdecorations(active_map);
            let (current, rest): (Vec<_>, Vec<_>) = builtin.into_iter().partition(|it| it.local_kind.is_some());
            items.splice(0..0, current);
            items.extend(rest);
        }
        // Виджеты Conky вне каталога Rexon (`~/.conky`, `~/.conkyrc`, свои
        // папки) — плитками «Найдено на диске» после своих.
        if apply_kind == "conky" {
            items.extend(crate::conky_scan::live_found_items(&self.home));
            // Картинка рядом с файлом настроек и «Источник»/«Авторы» из
            // source.txt/credits.txt — и своим, и найденным на диске.
            // `$XDG_CONFIG_HOME` сеанса — только для своего дома: скан
            // чужого дерева живые каталоги не читает.
            let live = crate::de_apply::Ctx::live();
            let ctx = if live.home.trim_end_matches('/') == self.home.trim_end_matches('/') {
                live
            } else {
                crate::de_apply::Ctx { home: self.home.clone(), xdg_config_home: None, wayland: false, de: String::new() }
            };
            for it in &mut items {
                crate::conky_scan::decorate_item(&crate::animations::SystemIo, &ctx, it);
            }
        }
        items
    }

    /// Плитки встроенных рамок KDE ([`crate::kdecoration`]); включённую
    /// раздел ставит первой, перед темами SVG.
    fn kdecorations(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        use crate::kdecoration as kd;
        let roots = if self.usr_root == "/usr" {
            kd::Roots::live(&self.home)
        } else {
            kd::Roots::under(&self.home, &self.usr_root)
        };
        let (decos, _) = kd::decorations(&kd::scan(&roots), &crate::i18n::current_language());
        let current = active_map.get("kdecoration").map(String::as_str).unwrap_or("");
        let Some(t) = types_for(&self.home, &self.usr_root).into_iter().find(|t| t.apply_kind == "aurorae") else {
            return Vec::new();
        };
        kd::section_items(&decos, current, t.cat_id, t.icon)
    }

    /// Kvantum-темы (`~/.config/Kvantum` + `/usr/share/Kvantum`).
    pub fn kvantum(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        let roots = kvantum_dirs(&self.home, &self.usr_root).to_vec();
        self.with_env(&roots, true, |env| scan_kvantum_themes(env, active_map))
    }

    /// Заставка (`kind = "splash"`) либо экран блокировки
    /// (`kind = "lockscreen"`) из look-and-feel.
    pub fn lnf(&self, kind: &str, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        let roots = lnf_dirs(&self.home, &self.usr_root).to_vec();
        self.with_env(&roots, true, |env| scan_lnf_component(env, kind, active_map))
    }

    /// Стиль приложений (Qt widget style). `styles` — имена Qt-стилей;
    /// у процесса без QtWidgets это [`WIDGET_STYLES`]. Диска не касается:
    /// список стилей даёт сам Qt.
    pub fn widget_styles(
        &self,
        active_map: &HashMap<String, String>,
        styles: &[String],
        qt_only: bool,
    ) -> Vec<ThemeItem> {
        scan_widget_styles(active_map, styles, qt_only)
    }
}

// ============================================================================
// Сверки живого исполнителя
// ============================================================================
//
// Проверяется ровно то, чего не поймает ни один голден на фикстуре: как
// живой снимок кладёт КЛЮЧИ. `is_dir`/`is_file` у [`FsSnapshot`] отвечают по
// наличию ключа, поэтому забытая регистрация подкаталога или файла превью —
// это не «чуть иначе», а молчаливое «такого нет», и раздел вкладки пустеет
// без единой ошибки.
#[cfg(test)]
mod tests {
    /// Новая тема в `~/.themes` меняет отметку корней.
    #[test]
    fn roots_stamp_sees_new_theme() {
        let home = std::env::temp_dir().join(format!("rexon-roots-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".themes")).unwrap();
        let h = home.to_string_lossy().to_string();
        let before = roots_stamp(&h, "/nonexistent-usr");
        assert_eq!(before, roots_stamp(&h, "/nonexistent-usr"), "без изменений — та же");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(home.join(".themes/Fresh/gtk-3.0")).unwrap();
        assert_ne!(before, roots_stamp(&h, "/nonexistent-usr"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Живой список Qt побеждает запас; пустой ответ — запас целиком.
    #[test]
    fn available_widget_styles_prefers_live_qt_keys() {
        let live: Vec<String> = ["Windows", "Fusion", " Breeze ", "", "qt6ct-style"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            super::available_widget_styles(&live),
            vec!["Windows", "Fusion", "Breeze", "qt6ct-style"]
        );
        let fallback: Vec<String> =
            super::WIDGET_STYLES.iter().map(|s| s.to_string()).collect();
        assert_eq!(super::available_widget_styles(&[]), fallback);
        assert_eq!(super::available_widget_styles(&[" ".to_string()]), fallback);
    }

    use super::*;

    /// Свой каталог под сверку в `std::env::temp_dir()`.
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "rexon-installed-scan-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("временный каталог");
        path
    }

    fn write(path: &std::path::Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("каталог файла");
        }
        std::fs::write(path, text).expect("запись файла");
    }

    fn s(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// Темы из `~/.local/share/themes` получают ссылку в `~/.themes` (его
    /// одного читает окно «Внешний вид» MATE): занятое имя не трогается,
    /// не-тема ссылки не получает, ссылка на пропавшую тему снимается, в
    /// снимке скана ссылки нет (тема видна один раз), а снятие темы уносит
    /// и её ссылку.
    #[test]
    fn theme_links_mirror_xdg_themes_into_dot_themes() {
        let home = tmp_dir("links");
        let h = s(&home);
        let xdg = home.join(".local/share/themes");
        write(&xdg.join("Nord/gtk-3.0/gtk.css"), "x");
        write(&xdg.join("Frame/metacity-1/metacity-theme-3.xml"), "x");
        write(&xdg.join("Taken/gtk-3.0/gtk.css"), "x");
        write(&xdg.join("junk/readme.txt"), "x");
        write(&home.join(".themes/Taken/gtk-3.0/gtk.css"), "свой");
        std::fs::create_dir_all(home.join(".themes")).unwrap();
        std::os::unix::fs::symlink("../.local/share/themes/Gone", home.join(".themes/Gone")).unwrap();

        let log = sync_theme_links(&h);
        assert_eq!(log.len(), 3, "{log:?}");
        let legacy = home.join(".themes");
        for name in ["Nord", "Frame"] {
            assert!(is_theme_link(&h, name), "{name}");
            assert_eq!(
                std::fs::read_link(legacy.join(name)).unwrap(),
                std::path::Path::new("../.local/share/themes").join(name)
            );
            assert!(legacy.join(name).join(if name == "Nord" { "gtk-3.0" } else { "metacity-1" }).is_dir());
        }
        assert!(legacy.join("Taken").is_dir() && !is_theme_link(&h, "Taken"), "своя тема не тронута");
        assert!(std::fs::symlink_metadata(legacy.join("junk")).is_err(), "не тема — без ссылки");
        assert!(std::fs::symlink_metadata(legacy.join("Gone")).is_err(), "ссылка в пустоту снята");
        assert!(sync_theme_links(&h).is_empty(), "второй прогон ничего не меняет");

        let snap = live_snapshot(&[s(&legacy), s(&xdg)], &[]);
        assert_eq!(snap.scandir(&s(&legacy)), &["Taken".to_string()]);
        assert!(snap.scandir(&s(&xdg)).contains(&"Nord".to_string()));

        std::fs::remove_dir_all(xdg.join("Nord")).unwrap();
        assert!(drop_theme_link(&h, &s(&xdg.join("Nord"))));
        assert!(std::fs::symlink_metadata(legacy.join("Nord")).is_err());
        assert!(!drop_theme_link(&h, &s(&legacy.join("Taken"))), "чужой путь — ничего");
        assert!(legacy.join("Taken").is_dir());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Снимок живого дерева: подкаталог темы зарегистрирован ПУСТЫМ, текст
    /// читается только у metadata, а цель `Preview=` попадает в снимок.
    ///
    /// Ловит три разных промаха разом: не зарегистрировать подкаталог
    /// (`contents/` пропал бы, и `scan_lnf_component` не нашёл бы ни одной
    /// темы), прочитать текстом обычный файл (темы носят мегабайтные `.svg`,
    /// и вкладка встала бы на каждом открытии) и не зарегистрировать файл из
    /// `Preview=` (превью тихо стало бы плейсхолдером).
    #[test]
    fn live_snapshot_keys_dirs_files_and_preview_target() {
        let root = tmp_dir("snap");
        let theme = root.join("Alpha");
        let meta = "[Desktop Entry]\nName=Alpha\nPreview=shot.png\n";
        write(&theme.join("metadata.desktop"), meta);
        write(&theme.join("big.svg"), "<svg/>");
        write(&theme.join("contents/shot.png"), "PNG");
        write(&root.join("loose.txt"), "x");

        let snap = live_snapshot(&[s(&root)], &[]);

        assert!(snap.is_dir(&s(&root)));
        let names = snap.scandir(&s(&root));
        assert!(names.contains(&"Alpha".to_string()));
        assert!(names.contains(&"loose.txt".to_string()));
        assert!(snap.is_dir(&s(&theme)));
        // подкаталог темы зарегистрирован, но вглубь не ходили
        assert!(snap.is_dir(&s(&theme.join("contents"))));
        assert!(snap.scandir(&s(&theme.join("contents"))).is_empty());
        // metadata — текстом, обычный файл — только фактом существования
        assert_eq!(snap.read_text(&s(&theme.join("metadata.desktop"))), Some(meta));
        assert!(snap.is_file(&s(&theme.join("big.svg"))));
        assert_eq!(snap.read_text(&s(&theme.join("big.svg"))), Some(""));
        // цель `Preview=` ищется относительно contents/ — и найдена
        assert!(snap.is_file(&s(&theme.join("contents/shot.png"))));
        assert_eq!(
            find_preview(&snap, &s(&theme), None),
            s(&theme.join("contents/shot.png"))
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// Несуществующий корень — пустой снимок без паники, а поданные извне
    /// файлы превью в него всё равно попадают.
    ///
    /// Оба случая живые: каталога `/usr/share/sddm/themes` на машине без
    /// SDDM просто нет, а файлы превью приходят из кеша магазина уже
    /// проверенными и на диске лежат вне корней обхода.
    #[test]
    fn live_snapshot_missing_root_is_empty_but_keeps_extra_files() {
        let missing = "/rexon-нет-такого-каталога/themes".to_string();
        let extra = "/var/cache/rexon/preview-42.png".to_string();

        let snap = live_snapshot(&[missing.clone()], std::slice::from_ref(&extra));

        assert!(!snap.is_dir(&missing));
        assert!(snap.scandir(&missing).is_empty());
        assert!(snap.is_file(&extra));
    }

    /// `from_parts` и `from_json` на одних данных отвечают одинаково.
    ///
    /// Живой обзор идёт первым конструктором, голдены моста — вторым; разойдись
    /// они хоть в одном ответе, сверки на фикстурах остались бы зелёными, а
    /// вкладка показывала бы другое.
    #[test]
    fn from_parts_answers_like_from_json() {
        let dirs = HashMap::from([
            ("/t/themes".to_string(), vec!["A".to_string(), "note.txt".to_string()]),
            ("/t/themes/A".to_string(), vec!["metadata.json".to_string()]),
            ("/t/themes/A/contents".to_string(), Vec::new()),
        ]);
        let files = HashMap::from([
            ("/t/themes/note.txt".to_string(), String::new()),
            ("/t/themes/A/metadata.json".to_string(), "{\"x\":1}".to_string()),
        ]);
        let from_parts = FsSnapshot::from_parts(dirs.clone(), files.clone());
        let json = serde_json::json!({"dirs": dirs, "files": files});
        let from_json = FsSnapshot::from_json(&json);

        for path in [
            "/t/themes",
            "/t/themes/A",
            "/t/themes/A/contents",
            "/t/themes/note.txt",
            "/t/themes/A/metadata.json",
            "/t/нет",
        ] {
            assert_eq!(from_parts.is_dir(path), from_json.is_dir(path), "is_dir {path}");
            assert_eq!(from_parts.is_file(path), from_json.is_file(path), "is_file {path}");
            assert_eq!(
                from_parts.read_text(path),
                from_json.read_text(path),
                "read_text {path}"
            );
            assert_eq!(from_parts.scandir(path), from_json.scandir(path), "scandir {path}");
        }
    }

    /// Живой обзор look-and-feel: тема с `contents/splash` попадает в
    /// «Сплэш-экран» и помечается активной, а в «Экран блокировки» — нет.
    ///
    /// Это сквозная сверка через настоящий диск: снимок собран живым
    /// обходом, а решения приняли те же `scan_lnf_component`, что и мост.
    /// Пропусти исполнитель пробу `contents/splash` — раздел оказался бы пуст.
    #[test]
    fn live_lnf_sees_splash_component_on_disk() {
        let base = tmp_dir("lnf");
        let home = base.join("home");
        let usr = base.join("usr");
        let theme = home.join(".local/share/plasma/look-and-feel/MyTheme");
        write(
            &theme.join("metadata.json"),
            "{\"KPlugin\": {\"Name\": \"My Theme\"}}",
        );
        std::fs::create_dir_all(theme.join("contents/splash")).expect("contents/splash");

        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([("splash".to_string(), "MyTheme".to_string())]);

        let splash = scan.lnf("splash", &active);
        // Тема и плитка «Нет» (она всегда последняя).
        assert_eq!(splash.len(), 2, "тема со splash обязана быть в списке");
        assert_eq!(splash[1].id, "local:splash:None");
        // В карточке — имя из метаданных, в номере — имя каталога.
        assert_eq!(splash[0].name, "My Theme");
        assert_eq!(splash[0].id, "local:splash:MyTheme");
        // Активную узнают по признаку, а не по подписи: подпись переводится.
        assert_eq!(splash[0].local_kind.as_deref(), Some("active"), "автор: {}", splash[0].author);
        assert_eq!(splash[0].install_path, s(&theme));

        // того же каталога lockscreen у темы нет — и в списке её быть не должно
        assert!(scan.lnf("lockscreen", &active).is_empty());

        std::fs::remove_dir_all(&base).ok();
    }

    // ── Человеческое имя темы ──────────────────────────────────────────
    //
    // Покупатель во вкладке «Оформление» видел сырой идентификатор каталога
    // (`org.kde.breeze.desktop`), хотя человеческое имя лежит рядом в
    // метаданных. Ветки ниже — весь порядок разрешения имени и КАЖДЫЙ откат:
    // пустой откат превратил бы карточку в безымянную.

    /// Снимок из пар «путь → текст» (каталоги выводятся из путей файлов).
    fn snap_of(theme_dir: &str, files: &[(&str, &str)]) -> FsSnapshot {
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        let mut fmap: HashMap<String, String> = HashMap::new();
        dirs.insert(
            theme_dir.to_string(),
            files.iter().map(|(n, _)| n.to_string()).collect(),
        );
        for (name, text) in files {
            fmap.insert(join(theme_dir, name), (*text).to_string());
        }
        FsSnapshot { dirs, files: fmap }
    }

    #[test]
    fn description_comes_from_metadata_or_index_theme() {
        let fs = snap_of(
            "/t/ocean",
            &[("index.theme", "[Sound Theme]\nName=Ocean\nComment=Sounds of the sea\nComment[ru]=Звуки моря\n")],
        );
        assert_eq!(theme_description(&fs, "/t/ocean", "ru_RU.UTF-8"), "Звуки моря");
        assert_eq!(theme_description(&fs, "/t/ocean", "de_DE.UTF-8"), "Sounds of the sea");
        let fs = snap_of(
            "/t/lnf",
            &[("metadata.json", r#"{"KPlugin":{"Name":"A","Description":"Dark look"}}"#)],
        );
        assert_eq!(theme_description(&fs, "/t/lnf", "ru_RU.UTF-8"), "Dark look");
        let fs = snap_of("/t/bare", &[("index.theme", "[Icon Theme]\nName=Bare\n")]);
        assert_eq!(theme_description(&fs, "/t/bare", "ru_RU.UTF-8"), "");
        assert_eq!(theme_description(&fs, "", "ru_RU.UTF-8"), "");
    }

    #[test]
    fn display_name_prefers_json_kplugin_name() {
        let fs = snap_of(
            "/t/org.rexon.sessionsplash",
            &[(
                "metadata.json",
                "{\"KPlugin\": {\"Name\": \"Rexon Session Splash\"}}",
            )],
        );
        assert_eq!(
            theme_display_name(
                &fs,
                "/t/org.rexon.sessionsplash",
                "org.rexon.sessionsplash",
                "ru_RU.UTF-8",
            ),
            "Rexon Session Splash"
        );
    }

    #[test]
    fn display_name_falls_back_to_desktop_file() {
        // У тем SDDM имя лежит в `[SddmGreeterTheme]`, а не в `[Desktop
        // Entry]`: привязка к одной группе потеряла бы весь экран входа.
        let fs = snap_of(
            "/t/breeze",
            &[("metadata.desktop", "[SddmGreeterTheme]\nName=Breeze\n")],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/breeze", "breeze", "en_US.UTF-8"),
            "Breeze"
        );
    }

    #[test]
    fn display_name_prefers_translation_for_current_language() {
        let json = "{\"KPlugin\": {\"Name\": \"Breeze\", \"Name[ru]\": \"Бриз\"}}";
        let desktop = "[Desktop Entry]\nName=Elarun\nName[ru]=Эларун\n";
        let fs = snap_of("/t/a", &[("metadata.json", json)]);
        assert_eq!(theme_display_name(&fs, "/t/a", "a", "ru_RU.UTF-8"), "Бриз");
        // язык другой → общее имя
        assert_eq!(theme_display_name(&fs, "/t/a", "a", "de_DE.UTF-8"), "Breeze");
        // в desktop-файле перевод стоит НИЖЕ общего имени и всё равно
        // выигрывает: ключи перебираются по языку, а не по строкам файла
        let fs = snap_of("/t/b", &[("metadata.desktop", desktop)]);
        assert_eq!(theme_display_name(&fs, "/t/b", "b", "ru"), "Эларун");
    }

    #[test]
    fn display_name_reads_lowercase_name_of_shell_addons() {
        // metadata.json расширения GNOME и апплета Cinnamon: имя — `name`.
        let json = "{\"uuid\": \"rexon-wallpaper@rexon.app\", \"name\": \"Rexon Wallpaper Engine\"}";
        let fs = snap_of("/t/x", &[("metadata.json", json)]);
        assert_eq!(
            theme_display_name(&fs, "/t/x", "rexon-wallpaper@rexon.app", "ru_RU.UTF-8"),
            "Rexon Wallpaper Engine"
        );
    }

    #[test]
    fn display_name_keeps_directory_name_when_metadata_is_useless() {
        // битый json
        let fs = snap_of("/t/x", &[("metadata.json", "{not json")]);
        assert_eq!(theme_display_name(&fs, "/t/x", "x", "ru_RU.UTF-8"), "x");
        // имя есть, но пустое (пробелы) — и json, и desktop
        let fs = snap_of("/t/y", &[("metadata.json", "{\"KPlugin\": {\"Name\": \"   \"}}")]);
        assert_eq!(theme_display_name(&fs, "/t/y", "y", "ru_RU.UTF-8"), "y");
        let fs = snap_of("/t/z", &[("metadata.desktop", "[Desktop Entry]\nName=\n")]);
        assert_eq!(theme_display_name(&fs, "/t/z", "z", "ru_RU.UTF-8"), "z");
        // json есть, но имени в нём нет вовсе → пробуем desktop рядом
        let fs = snap_of(
            "/t/w",
            &[
                ("metadata.json", "{\"KPlugin\": {\"Id\": \"w\"}}"),
                ("metadata.desktop", "[Desktop Entry]\nName=Запасное\n"),
            ],
        );
        assert_eq!(theme_display_name(&fs, "/t/w", "w", "ru_RU.UTF-8"), "Запасное");
        // метаданных нет вовсе — ни KDE-шных, ни index.theme
        let fs = snap_of("/t/v", &[("cursors", "")]);
        assert_eq!(theme_display_name(&fs, "/t/v", "v", "ru_RU.UTF-8"), "v");
        // index.theme есть, но имени в нём нет (у GTK-тем встречается
        // голый [X-GNOME-Metatheme] с одним GtkTheme=)
        let fs = snap_of(
            "/t/u",
            &[("index.theme", "[X-GNOME-Metatheme]\nGtkTheme=u\n")],
        );
        assert_eq!(theme_display_name(&fs, "/t/u", "u", "ru_RU.UTF-8"), "u");
        // каталога темы нет (цветовые схемы — файл, а не папка)
        let fs = snap_of("/t/none", &[]);
        assert_eq!(theme_display_name(&fs, "", "BreezeDark", "ru_RU.UTF-8"), "BreezeDark");
    }

    // ── Иконки, курсоры, GTK-темы: имя из index.theme ──────────────────
    //
    // Метаданных KDE у них нет вовсе, и покупатель до сих пор читал имя
    // каталога: «breeze_cursors» вместо «Breeze Dark», «adw-gtk3» вместо
    // «adw-gtk3». Образцы взяты с ЖИВОЙ машины Wehrwolfmann.

    #[test]
    fn display_name_reads_index_theme_of_icons_and_cursors() {
        // /usr/share/icons/Papirus — секция [Icon Theme]
        let fs = snap_of(
            "/t/Papirus",
            &[(
                "index.theme",
                "[Icon Theme]\nName=Papirus\nComment=Papirus icon theme\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/Papirus", "Papirus", "ru_RU.UTF-8"),
            "Papirus"
        );
        // /usr/share/icons/breeze_cursors — имя каталога и имя темы РАЗНЫЕ
        let fs = snap_of(
            "/t/breeze_cursors",
            &[(
                "index.theme",
                "[Icon Theme]\nName=Breeze Dark\nName[de]=Breeze Dunkel\n\
                 Name[ru]=Breeze, тёмный вариант\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/breeze_cursors", "breeze_cursors", "en_US.UTF-8"),
            "Breeze Dark"
        );
        // перевод под язык покупателя выигрывает, хотя стоит ниже
        assert_eq!(
            theme_display_name(&fs, "/t/breeze_cursors", "breeze_cursors", "ru_RU.UTF-8"),
            "Breeze, тёмный вариант"
        );
    }

    #[test]
    fn display_name_reads_index_theme_of_gtk_themes() {
        // GTK-темы кладут имя в РАЗНЫЕ группы — привязка к одной потеряла
        // бы половину: adw-gtk3 в [X-GNOME-Metatheme], WhiteSur в [Desktop
        // Entry] (проверено на /usr/share/themes этой машины).
        let fs = snap_of(
            "/t/adw-gtk3-dark",
            &[(
                "index.theme",
                "[X-GNOME-Metatheme]\nName=adw-gtk3-dark\nGtkTheme=adw-gtk3-dark\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/adw-gtk3-dark", "adw-gtk3-dark", "ru_RU.UTF-8"),
            "adw-gtk3-dark"
        );
        let fs = snap_of(
            "/t/ContrastHigh",
            &[(
                "index.theme",
                "[Desktop Entry]\nType=X-GNOME-Metatheme\nName=High Contrast\n\
                 \n[X-GNOME-Metatheme]\nGtkTheme=ContrastHigh\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/ContrastHigh", "ContrastHigh", "ru_RU.UTF-8"),
            "High Contrast"
        );
    }

    #[test]
    fn display_name_keeps_kde_metadata_ahead_of_index_theme() {
        // Порядок источников: metadata.* пишет автор темы под KDE,
        // index.theme бывает сгенерированным — у KDE-метаданных приоритет.
        let fs = snap_of(
            "/t/both",
            &[
                ("metadata.json", "{\"KPlugin\": {\"Name\": \"Из KDE\"}}"),
                ("index.theme", "[Icon Theme]\nName=Из index\n"),
            ],
        );
        assert_eq!(theme_display_name(&fs, "/t/both", "both", "ru_RU.UTF-8"), "Из KDE");
        // metadata.desktop без имени → идём дальше, в index.theme
        let fs = snap_of(
            "/t/next",
            &[
                ("metadata.desktop", "[Desktop Entry]\nComment=нет имени\n"),
                ("index.theme", "[Icon Theme]\nName=Из index\n"),
            ],
        );
        assert_eq!(theme_display_name(&fs, "/t/next", "next", "ru_RU.UTF-8"), "Из index");
    }

    // ── Цветовые схемы: имя из самого файла *.colors ───────────────────
    //
    // Каталога темы у них нет вовсе — тема это один файл. Имя лежит в
    // [General], одиннадцатой группе по счёту (проверено на
    // /usr/share/color-schemes этой машины), поэтому группа не спрашивается.

    #[test]
    fn colors_display_name_comes_from_the_file_itself() {
        let text = "[Colors:Window]\nBackgroundNormal=27,30,32\n\n\
                    [General]\nName=Breeze Dark\nName[de]=Breeze Dunkel\n\
                    Name[ru]=Breeze, тёмный вариант\nColorScheme=BreezeDark\n";
        let mut files = HashMap::new();
        files.insert("/sb/usr/share/color-schemes/BreezeDark.colors".to_string(), text.to_string());
        let fs = FsSnapshot { dirs: HashMap::new(), files };
        let path = "/sb/usr/share/color-schemes/BreezeDark.colors";
        assert_eq!(
            file_display_name(&fs, path, "BreezeDark", "en_US.UTF-8"),
            "Breeze Dark"
        );
        assert_eq!(
            file_display_name(&fs, path, "BreezeDark", "ru_RU.UTF-8"),
            "Breeze, тёмный вариант"
        );
        // файла нет → имя файла (безымянных карточек не бывает)
        assert_eq!(
            file_display_name(&fs, "/sb/usr/share/color-schemes/Нет.colors", "Нет", "ru"),
            "Нет"
        );
        // путь пустой → откат без обращения к диску
        assert_eq!(file_display_name(&fs, "", "Нет", "ru"), "Нет");
        // файл есть, а имени в нём нет (обрезанная схема) → имя файла
        let mut files = HashMap::new();
        files.insert("/c/Пустая.colors".to_string(), "[General]\nColorScheme=X\n".to_string());
        let fs = FsSnapshot { dirs: HashMap::new(), files };
        assert_eq!(file_display_name(&fs, "/c/Пустая.colors", "Пустая", "ru"), "Пустая");
    }

    #[test]
    fn locale_name_keys_go_from_exact_to_general() {
        assert_eq!(
            locale_name_keys("ru_RU.UTF-8"),
            vec!["Name[ru_RU]", "Name[ru]", "Name"]
        );
        // дефис нормализуется в подчёркивание — в ключах KDE только оно
        assert_eq!(locale_name_keys("pt-BR"), vec!["Name[pt_BR]", "Name[pt]", "Name"]);
        assert_eq!(locale_name_keys("ru"), vec!["Name[ru]", "Name"]);
        // модификатор отбрасывается
        assert_eq!(locale_name_keys("ca@valencia"), vec!["Name[ca]", "Name"]);
        // языка нет — остаётся одно общее имя
        assert_eq!(locale_name_keys(""), vec!["Name"]);
    }

    /// Скан «Системных» на ЭТОЙ машине доходит до ответа.
    ///
    /// Проверяется живой путь целиком: определение семейства пакетов, запрос
    /// «что не из репозиториев», двухфазный вопрос о владельцах файлов. Ошибка
    /// здесь означает `SystemScanUnavailable` во вкладке — то есть раздел,
    /// закрытый статусом вместо тем.
    #[test]
    #[cfg_attr(
        feature = "isolated-checks",
        ignore = "нужна живая машина: спрашивает настоящий пакетный менеджер о происхождении пакетов"
    )]
    fn live_system_scan_reaches_an_answer() {
        let scan = LiveScan::new(Vec::new(), HashMap::new());
        let items = scan
            .system(&HashMap::new())
            .expect("происхождение пакетов на этой машине определяется");
        // Всё, что вернулось, обязано лежать под системным корнем: иначе в
        // «Системные» уехали пользовательские темы.
        for item in &items {
            assert!(
                item.install_path.starts_with(&scan.usr_root),
                "не системный путь: {}",
                item.install_path
            );
        }
    }

    /// Разделы окружений: рамки XFCE ищутся в ~/.themes и
    /// ~/.local/share/themes по подкаталогу `xfwm4`, действия Nemo — файлами,
    /// включённое (карта активных, у дополнений — несколько имён) — с
    /// признаком «активна»; чужое окружение разделов не получает.
    #[test]
    fn de_sections_scan_by_matrix() {
        let base = tmp_dir("desect");
        let home = base.join("home");
        let usr = base.join("usr");
        write(&home.join(".themes/Arc/xfwm4/themerc"), "x");
        write(&home.join(".local/share/themes/Nord/xfwm4/themerc"), "x");
        write(&home.join(".themes/GtkOnly/gtk-3.0/gtk.css"), "x");
        write(&usr.join("share/themes/Default/xfwm4/themerc"), "x");
        write(&home.join(".local/share/nemo/actions/a.nemo_action"), "x");
        write(&home.join(".local/share/nemo/actions/b.nemo_action"), "x");
        write(&home.join(".local/share/nemo/actions/readme.txt"), "x");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: Some("xfce".into()),
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([
            ("xfwm4".to_string(), "Nord".to_string()),
            ("nemoactions".to_string(), "a.nemo_action\nb.nemo_action".to_string()),
        ]);
        let got: Vec<(String, Option<String>)> = scan
            .section("xfwm4", &active)
            .into_iter()
            .map(|it| (it.id, it.local_kind))
            .collect();
        assert_eq!(
            got,
            vec![
                ("local:xfwm4:Nord".to_string(), Some("active".to_string())),
                ("local:xfwm4:Arc".to_string(), None),
                ("local:xfwm4:Default".to_string(), None),
            ]
        );
        let nemo: Vec<String> = scan.section("nemoactions", &active).into_iter().map(|it| it.id).collect();
        assert_eq!(nemo, vec!["local:nemoactions:a.nemo_action", "local:nemoactions:b.nemo_action"]);
        // «Загруженные» XFCE видят рамки, но не действия Nemo (они — Cinnamon).
        let dl: Vec<String> = scan.downloaded(&active).into_iter().map(|it| it.id).collect();
        assert!(dl.contains(&"local:xfwm4:Arc".to_string()), "{dl:?}");
        assert!(!dl.iter().any(|i| i.starts_with("local:nemoactions")), "{dl:?}");
    }

    /// Тема LXQt — каталог с `lxqt-panel.qss`, как в списке тем самого
    /// LXQt (`LXQtTheme::allThemes`): каталог без него темой не считается
    /// ни в доме, ни в системе.
    #[test]
    fn lxqt_themes_need_panel_stylesheet() {
        let base = tmp_dir("lxqt-themes");
        let home = base.join("home");
        let usr = base.join("usr");
        write(&home.join(".local/share/lxqt/themes/Frost/lxqt-panel.qss"), "LXQtPanel {}");
        write(&home.join(".local/share/lxqt/themes/Frost/preview.png"), "x");
        write(&home.join(".local/share/lxqt/themes/Junk/readme.txt"), "x");
        write(&home.join(".local/share/lxqt/themes/Junk/openbox/themerc"), "x");
        write(&usr.join("share/lxqt/themes/ambiance/lxqt-panel.qss"), "x");
        write(&usr.join("share/lxqt/themes/wallpapers/a.png"), "x");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: Some("lxqt".into()),
            migrate: false,
            standard_icons: None,
        };
        let mut ids: Vec<String> =
            scan.section("lxqttheme", &HashMap::new()).into_iter().map(|it| it.id).collect();
        ids.sort();
        assert_eq!(ids, vec!["local:lxqttheme:Frost".to_string(), "local:lxqttheme:ambiance".to_string()]);
    }

    /// Раздел «Виджеты Conky» показывает и виджеты вне каталога Rexon —
    /// плитками «Найдено на диске» после своих, без повторов.
    #[test]
    fn conky_section_shows_widgets_found_on_disk() {
        let base = tmp_dir("conky-found");
        let home = base.join("home");
        let usr = base.join("usr");
        write(&home.join(".config/conky/Jhalak/Jhalak.conf"), "conky.text = [[x]]");
        write(&home.join(".conky/Gotham/Gotham"), "# Gotham\nTEXT\n${time}\n");
        write(&home.join(".conky/Gotham/rings.lua"), "x");
        write(&home.join(".conkyrc"), "conky.config = {}\n");
        write(&home.join(".conky/notes/readme.txt"), "просто текст");
        std::fs::create_dir_all(&usr).unwrap();
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: Some("mate".into()),
            migrate: false,
            standard_icons: None,
        };
        // Картинка рядом с файлом настроек (с его именем и без окончания) и
        // источник из source.txt — у своего и у найденного виджета.
        write(&home.join(".config/conky/Jhalak/Jhalak.png"), "png");
        write(&home.join(".conky/Gotham/Gotham.jpg"), "jpg");
        write(&home.join(".conky/Gotham/source.txt"), "http://psyjunta.deviantart.com/art/Gotham-Conky-config-205465419\n");
        write(&home.join(".conky/Gotham/credits.txt"), "psyjunta <3");
        let h = s(&home);
        let items = scan.section("conky", &HashMap::new());
        let ids: Vec<String> = items.iter().map(|it| it.id.clone()).collect();
        assert_eq!(
            ids,
            vec![
                "local:conky:Jhalak".to_string(),
                format!("local:conky:{h}/.conkyrc"),
                format!("local:conky:{h}/.conky/Gotham"),
            ]
        );
        assert_eq!(items[0].preview_local, format!("{h}/.config/conky/Jhalak/Jhalak.png"));
        assert_eq!(items[2].preview_local, format!("{h}/.conky/Gotham/Gotham.jpg"));
        let d = &items[2].description;
        assert!(d.contains("<a href=\"http://psyjunta.deviantart.com/art/Gotham-Conky-config-205465419\">"), "{d}");
        assert!(d.contains("psyjunta &lt;3"), "{d}");
        assert!(items[1].description.is_empty());
    }

    /// Темы звуков видны в «Загруженных» и «Активных»; обёртка архива и
    /// каталог без `[Sound Theme]` темой не считаются.
    #[test]
    fn sound_themes_are_scanned_only_on_top_level_with_index() {
        let base = tmp_dir("sounds");
        let home = base.join("home");
        let usr = base.join("usr");
        let snd = home.join(".local/share/sounds");
        write(&snd.join("Ocean/index.theme"), "[Sound Theme]\nName=Ocean\n");
        write(&snd.join("Zorin/index.theme"), "[Sound Theme]\nName=Zorin\n");
        write(&snd.join("zorinos2/sounds/Zorin2/index.theme"), "[Sound Theme]\nName=Z2\n");
        write(&snd.join("gnome/default/alerts/bark.ogg"), "x");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([("sounds".to_string(), "Zorin".to_string())]);
        let ids = |items: Vec<ThemeItem>| -> Vec<String> {
            let mut v: Vec<String> = items
                .into_iter()
                .filter(|i| i.install_path.contains("/sounds/"))
                .map(|i| i.install_path.rsplit('/').next().unwrap_or("").to_string())
                .collect();
            v.sort();
            v
        };
        assert_eq!(ids(scan.downloaded(&active)), ["Ocean"]);
        assert_eq!(ids(scan.active(&active)), ["Zorin"]);
    }


    // ── Разделы KDE с сеткой установленного (Т3а) ─────────────────────
    //
    // Каждый раздел — во временном HOME и подставном корне `/usr`
    // (`LiveScan::usr_root`): живой профиль и живые темы не читаются.

    /// Скан раздела в подставных HOME/`/usr`: (номер, «активна»), текущая —
    /// первой, остальные — по алфавиту (порядок каталога зависит от ФС).
    fn kde_section(
        tag: &str,
        key: &str,
        current: &str,
        fill: impl FnOnce(&std::path::Path, &std::path::Path),
    ) -> Vec<(String, bool)> {
        let base = tmp_dir(tag);
        let home = base.join("home");
        let usr = base.join("usr");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&usr).expect("usr");
        fill(&home, &usr);
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([(key.to_string(), current.to_string())]);
        let items = scan.section(key, &active);
        // Раздел знает ядро: ключ разбирается в скан раздела.
        assert_eq!(
            crate::appearance_page::local_scan_plan(key),
            ("section".to_string(), key.to_string())
        );
        let cat = crate::categories::KDE_INSTALLED_SECTIONS
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(id, _)| *id)
            .expect("раздел в списке KDE_INSTALLED_SECTIONS");
        let mut got: Vec<(String, bool)> = items
            .iter()
            .map(|it| {
                assert_eq!(it.category_id, cat, "{}", it.id);
                assert!(it.installed, "{}", it.id);
                (it.id.clone(), it.local_kind.as_deref() == Some("active"))
            })
            .collect();
        if got.len() > 1 {
            got[1..].sort();
        }
        got
    }

    fn row(id: &str, active: bool) -> (String, bool) {
        (id.to_string(), active)
    }

    fn lnf(dir: &std::path::Path, name: &str) {
        write(
            &dir.join("metadata.json"),
            &format!("{{\"KPlugin\": {{\"Name\": \"{name}\", \"Id\": \"{name}\"}}}}"),
        );
    }

    const ICON_INDEX: &str = "[Icon Theme]\nName=X\nDirectories=48x48/apps\n";

    #[test]
    fn kde_section_global_themes() {
        let got = kde_section("kde-global", "global", "org.kde.breezedark.desktop", |home, usr| {
            let user = home.join(".local/share/plasma/look-and-feel");
            lnf(&user.join("com.example.Mine"), "Mine");
            std::fs::create_dir_all(user.join("junk-without-metadata")).unwrap();
            let sys = usr.join("share/plasma/look-and-feel");
            lnf(&sys.join("org.kde.breezedark.desktop"), "Breeze Dark");
            lnf(&sys.join("org.kde.breeze.desktop"), "Breeze");
        });
        assert_eq!(
            got,
            vec![
                row("local:global:org.kde.breezedark.desktop", true),
                row("local:global:com.example.Mine", false),
                row("local:global:org.kde.breeze.desktop", false),
            ]
        );
    }

    #[test]
    fn kde_section_plasma_styles() {
        let got = kde_section("kde-plasma", "plasma", "Mine", |home, usr| {
            let user = home.join(".local/share/plasma/desktoptheme");
            write(&user.join("Mine/metadata.json"), "{\"KPlugin\": {\"Name\": \"Mine\"}}");
            let sys = usr.join("share/plasma/desktoptheme");
            write(&sys.join("default/metadata.json"), "{\"KPlugin\": {\"Name\": \"Breeze\"}}");
            write(&sys.join("oxygen/metadata.json"), "{\"KPlugin\": {\"Name\": \"Oxygen\"}}");
        });
        assert_eq!(
            got,
            vec![
                row("local:plasma:Mine", true),
                row("local:plasma:default", false),
                row("local:plasma:oxygen", false),
            ]
        );
    }

    #[test]
    fn kde_section_color_schemes() {
        let got = kde_section("kde-colors", "colors", "BreezeDark", |home, usr| {
            let user = home.join(".local/share/color-schemes");
            write(&user.join("Nord.colors"), "[General]\nName=Nord\n");
            write(&user.join("notes.txt"), "x");
            let sys = usr.join("share/color-schemes");
            write(&sys.join("BreezeDark.colors"), "[General]\nName=Breeze Dark\n");
            write(&sys.join("BreezeLight.colors"), "[General]\nName=Breeze Light\n");
        });
        assert_eq!(
            got,
            vec![
                row("local:colors:BreezeDark", true),
                row("local:colors:BreezeLight", false),
                row("local:colors:Nord", false),
            ]
        );
    }

    #[test]
    fn kde_section_aurorae_decorations() {
        let got = kde_section("kde-aurorae", "aurorae", "Sweet", |home, usr| {
            let user = home.join(".local/share/aurorae/themes");
            write(&user.join("Sweet/metadata.desktop"), "[Desktop Entry]\nName=Sweet\n");
            write(&user.join("Layan/metadata.desktop"), "[Desktop Entry]\nName=Layan\n");
            write(&usr.join("share/aurorae/themes/Plastik/metadata.desktop"), "[Desktop Entry]\nName=Plastik\n");
        });
        assert_eq!(
            got,
            vec![
                row("local:aurorae:Sweet", true),
                row("local:aurorae:Layan", false),
                row("local:aurorae:Plastik", false),
            ]
        );
    }

    /// Значки: курсорные темы и служебные наборы (`hicolor`, `default`) в
    /// сетку не идут; `~/.icons` смотрится вместе с `~/.local/share/icons`,
    /// одно имя в двух местах — одна плитка; текущая — и из `~/.icons`.
    #[test]
    fn kde_section_icons() {
        let got = kde_section("kde-icons", "icons", "Papirus", |home, usr| {
            write(&home.join(".icons/Papirus/index.theme"), ICON_INDEX);
            write(&home.join(".local/share/icons/Tela/index.theme"), ICON_INDEX);
            write(&home.join(".icons/Tela/index.theme"), ICON_INDEX);
            write(&home.join(".local/share/icons/Bibata/cursors/left_ptr"), "x");
            let sys = usr.join("share/icons");
            write(&sys.join("breeze/index.theme"), ICON_INDEX);
            write(&sys.join("hicolor/index.theme"), "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n");
            write(&sys.join("default/index.theme"), "[Icon Theme]\nInherits=breeze_cursors\n");
            write(&sys.join("breeze_cursors/cursors/left_ptr"), "x");
            write(&sys.join("breeze_cursors/index.theme"), "[Icon Theme]\nName=Breeze\n");
        });
        assert_eq!(
            got,
            vec![
                row("local:icons:Papirus", true),
                row("local:icons:Tela", false),
                row("local:icons:breeze", false),
            ]
        );
    }

    /// Значки окружения по умолчанию видны: набор со своими курсорами
    /// (elementary в XFCE, Adwaita) — в «Значках» и в «Курсорах»; скрытый
    /// (`Hidden=true`) — только если это стандартный набор окружения
    /// (Adwaita в GNOME), прочие скрытые (hicolor) — нет.
    #[test]
    fn section_icons_shows_de_standard_and_icon_sets_with_cursors() {
        let base = tmp_dir("de-std-icons");
        let home = base.join("home");
        let usr = base.join("usr");
        let sys = usr.join("share/icons");
        write(&home.join(".icons/Papirus/index.theme"), ICON_INDEX);
        write(&sys.join("Adwaita/index.theme"), "[Icon Theme]\nName=Adwaita\nInherits=AdwaitaLegacy,hicolor\nHidden=true\nDirectories=16x16/apps\n");
        write(&sys.join("Adwaita/cursors/left_ptr"), "x");
        write(&sys.join("elementary/index.theme"), ICON_INDEX);
        write(&sys.join("elementary/cursors/left_ptr"), "x");
        write(&sys.join("hicolor/index.theme"), "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n");
        write(&sys.join("breeze_cursors/cursors/left_ptr"), "x");
        write(&sys.join("breeze_cursors/index.theme"), "[Icon Theme]\nName=Breeze\n");
        let ids = |standard: Option<&str>, key: &str, current: &str| {
            let scan = LiveScan {
                home: s(&home),
                usr_root: s(&usr),
                ocs_cache: Vec::new(),
                preview_files: HashMap::new(),
                store: StoreRecords::default(),
                plasma_login: Some(false),
                de: None,
                migrate: false,
                standard_icons: standard.map(str::to_string),
            };
            let active = HashMap::from([(key.to_string(), current.to_string())]);
            let mut v: Vec<String> = scan.section(key, &active).into_iter().map(|it| it.id).collect();
            v.sort();
            v
        };
        assert_eq!(
            ids(Some("Adwaita"), "icons", "Papirus"),
            ["local:icons:Adwaita", "local:icons:Papirus", "local:icons:elementary"]
        );
        assert_eq!(ids(None, "icons", "Papirus"), ["local:icons:Papirus", "local:icons:elementary"]);
        assert_eq!(
            ids(None, "cursor", "breeze_cursors"),
            ["local:cursor:Adwaita", "local:cursor:breeze_cursors", "local:cursor:elementary"]
        );
        let fs = live_snapshot(&[s(&sys)], &[]);
        assert!(is_cursor_only(&fs, &s(&sys.join("breeze_cursors"))));
        assert!(!is_cursor_only(&fs, &s(&sys.join("Adwaita"))));
    }

    /// Курсоры: только темы с `cursors/`, свои и системные.
    #[test]
    fn kde_section_cursors() {
        let got = kde_section("kde-cursor", "cursor", "breeze_cursors", |home, usr| {
            write(&home.join(".local/share/icons/Bibata/cursors/left_ptr"), "x");
            write(&home.join(".icons/Capitaine/cursors/left_ptr"), "x");
            write(&home.join(".local/share/icons/Tela/index.theme"), ICON_INDEX);
            let sys = usr.join("share/icons");
            write(&sys.join("breeze_cursors/cursors/left_ptr"), "x");
            write(&sys.join("Adwaita/cursors/left_ptr"), "x");
            write(&sys.join("breeze/index.theme"), ICON_INDEX);
        });
        assert_eq!(
            got,
            vec![
                row("local:cursor:breeze_cursors", true),
                row("local:cursor:Adwaita", false),
                row("local:cursor:Bibata", false),
                row("local:cursor:Capitaine", false),
            ]
        );
    }

    /// GTK: `~/.themes` тоже; темы без GTK 3/4 (одни рамки) не показываются.
    #[test]
    fn kde_section_gtk_themes() {
        let got = kde_section("kde-gtk", "gtk", "Breeze", |home, usr| {
            write(&home.join(".local/share/themes/Arc/gtk-3.0/gtk.css"), "x");
            write(&home.join(".themes/Orchis/gtk-4.0/gtk.css"), "x");
            write(&home.join(".themes/OnlyXfwm/xfwm4/themerc"), "x");
            let sys = usr.join("share/themes");
            write(&sys.join("Breeze/gtk-3.0/gtk.css"), "x");
            write(&sys.join("Adwaita/gtk-3.0/gtk.css"), "x");
            write(&sys.join("Default/xfwm4/themerc"), "x");
        });
        assert_eq!(
            got,
            vec![
                row("local:gtk:Breeze", true),
                row("local:gtk:Adwaita", false),
                row("local:gtk:Arc", false),
                row("local:gtk:Orchis", false),
            ]
        );
    }

    /// Обзоры («Загруженные», «Системные») отбирают темы тем же правилом,
    /// что сетка раздела: каталог в `share/themes` без таблиц стилей GTK 3/4
    /// (одни рамки xfwm4/Metacity/Openbox, одно GTK 2) темой GTK не считается,
    /// служебный набор значков — набором значков. Рамки при этом видны своим
    /// видом — по своему подкаталогу.
    #[test]
    fn overviews_list_only_real_gtk_themes_and_icon_sets() {
        let base = tmp_dir("overview-gtk");
        let (home, usr) = (base.join("home"), base.join("usr"));
        let user = home.join(".local/share/themes");
        write(&user.join("Arc/gtk-3.0/gtk.css"), "x");
        write(&user.join("Frames/xfwm4/themerc"), "x");
        write(&user.join("Old/gtk-2.0/gtkrc"), "x");
        let sys = usr.join("share/themes");
        write(&sys.join("Adwaita/gtk-4.0/gtk.css"), "x");
        write(&sys.join("Bear2/openbox-3/themerc"), "x");
        write(&sys.join("Atlanta/metacity-1/metacity-theme-1.xml"), "x");
        write(&sys.join("Both/gtk-3.0/gtk.css"), "x");
        write(&sys.join("Both/xfwm4/themerc"), "x");
        write(&home.join(".local/share/icons/Tela/index.theme"), ICON_INDEX);
        write(&home.join(".local/share/icons/hicolor/48x48/apps/x.png"), "x");
        let icons = usr.join("share/icons");
        write(&icons.join("breeze/index.theme"), ICON_INDEX);
        write(&icons.join("hicolor/index.theme"), "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n");
        write(&icons.join("default/index.theme"), "[Icon Theme]\nInherits=breeze_cursors\n");

        let (home_s, usr_s) = (s(&home), s(&usr));
        let mut types = types_for(&home_s, &usr_s);
        types.extend(de_types(&home_s, &usr_s, "xfce"));
        let mut roots: Vec<String> = Vec::new();
        for t in &types {
            for d in [&t.user_dir, &t.sys_dir] {
                if !d.is_empty() && !roots.contains(d) {
                    roots.push(d.clone());
                }
            }
        }
        let fs = live_snapshot(&roots, &[]);
        let no_cache: Vec<(String, String)> = Vec::new();
        let preview_file = |_: &str| None;
        let env = ScanEnv {
            home: &home_s,
            usr_root: &usr_s,
            fs: &fs,
            ocs_cache: &no_cache,
            preview_file: &preview_file,
            store: StoreRecords::none(),
            locale: "en",
            standard_icons: None,
        };
        let ids = |items: Vec<ThemeItem>| {
            let mut ids: Vec<String> = items.into_iter().map(|it| it.id).collect();
            ids.sort();
            ids
        };
        let nothing_active: HashMap<String, String> = HashMap::new();
        assert_eq!(
            ids(scan_downloaded_in(&env, &types, &nothing_active)),
            ["local:gtk:Arc", "local:icons:Tela", "local:xfwm4:Frames"]
        );
        // «Системные»: владелец каждого пути — пакет из репозиториев.
        let first = scan_system_in(&env, &types, &nothing_active, &HashSet::new(), &HashMap::new()).unwrap();
        let owners: HashMap<String, String> =
            first.qo_paths.iter().map(|p| (p.clone(), "pkg".to_string())).collect();
        let system = scan_system_in(&env, &types, &nothing_active, &HashSet::new(), &owners).unwrap();
        assert_eq!(
            ids(system.items),
            ["local:gtk:Adwaita", "local:gtk:Both", "local:icons:breeze", "local:xfwm4:Both"]
        );
        // Включённая сейчас тема видна всегда, даже без таблиц стилей GTK 3/4.
        let active = HashMap::from([("gtk".to_string(), "Old".to_string())]);
        assert_eq!(ids(scan_active_in(&env, &types, &active)), ["local:gtk:Old"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn kde_section_sound_themes() {
        let got = kde_section("kde-sounds", "sounds", "ocean", |home, usr| {
            let user = home.join(".local/share/sounds");
            write(&user.join("Zorin/index.theme"), "[Sound Theme]\nName=Zorin\n");
            write(&user.join("gnome/default/alerts/bark.ogg"), "x");
            let sys = usr.join("share/sounds");
            write(&sys.join("ocean/index.theme"), "[Sound Theme]\nName=Ocean\n");
            write(&sys.join("freedesktop/index.theme"), "[Sound Theme]\nName=Freedesktop\n");
            write(&sys.join("alsa/Front_Center.wav"), "x");
        });
        assert_eq!(
            got,
            vec![
                row("local:sounds:ocean", true),
                row("local:sounds:Zorin", false),
                row("local:sounds:freedesktop", false),
            ]
        );
    }

    /// Текущей нет в карте (ничего не применено) — сетка та же, без отметки;
    /// разделы KDE без сетки установленного (обои, шрифты, виджеты) скана
    /// раздела не получают.
    #[test]
    fn kde_section_without_current_and_other_sections() {
        let base = tmp_dir("kde-nocur");
        let home = base.join("home");
        write(&home.join(".local/share/color-schemes/Nord.colors"), "[General]\nName=Nord\n");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&base.join("usr")),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let items = scan.section("colors", &HashMap::new());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].local_kind, None);
        for key in ["wallpapers", "fonts", "plasmoids"] {
            assert_eq!(
                crate::appearance_page::local_scan_plan(key),
                (String::new(), String::new()),
                "{key}"
            );
        }
        assert_eq!(crate::categories::KDE_INSTALLED_SECTIONS.len(), 10);
    }

    /// Экран входа (как kcm sddm): все темы SDDM из системного каталога,
    /// текущая первой и выделена; каталог без `metadata.desktop` темой не
    /// считается.
    #[test]
    fn kde_section_sddm_themes() {
        let got = kde_section("kde-sddm", "sddm", "maldives", |_home, usr| {
            let sys = usr.join("share/sddm/themes");
            for t in ["breeze", "maldives", "maya"] {
                write(&sys.join(t).join("metadata.desktop"), &format!("[SddmGreeterTheme]\nName={t}-shown\nMainScript=Main.qml\n"));
            }
            write(&sys.join("leftover/Main.qml"), "x");
        });
        assert_eq!(
            got,
            vec![
                row("local:sddm:maldives", true),
                row("local:sddm:breeze", false),
                row("local:sddm:maya", false),
            ]
        );
    }

    /// Заставка загрузки (как kcm plymouth): темы с `<имя>.plymouth`,
    /// текущая первой и выделена; пустой каталог от снятого пакета не
    /// показывается. Имя и описание — из `.plymouth` на языке интерфейса.
    #[test]
    fn kde_section_plymouth_themes() {
        let got = kde_section("kde-ply", "plymouth", "bgrt", |_home, usr| {
            let sys = usr.join("share/plymouth/themes");
            for t in ["bgrt", "spinner", "details"] {
                write(&sys.join(t).join(format!("{t}.plymouth")), "[Plymouth Theme]\nName=X\nModuleName=two-step\n");
            }
            std::fs::create_dir_all(sys.join("removed-package")).unwrap();
        });
        assert_eq!(
            got,
            vec![
                row("local:plymouth:bgrt", true),
                row("local:plymouth:details", false),
                row("local:plymouth:spinner", false),
            ]
        );

        let dir = tmp_dir("ply-name").join("cachyos-bootanimation");
        write(
            &dir.join("cachyos-bootanimation.plymouth"),
            "[Plymouth Theme]\nName=CachyOS Boot\nName[ru]=Загрузка CachyOS\n\
             Description=Animated logo\nDescription[ru]=Живой логотип\nModuleName=script\n\n\
             [script]\nImageDir=/x\n",
        );
        let d = s(&dir);
        let snap = live_snapshot(&[s(dir.parent().unwrap())], &[]);
        assert_eq!(theme_display_name(&snap, &d, "cachyos-bootanimation", "ru_RU"), "Загрузка CachyOS");
        assert_eq!(theme_display_name(&snap, &d, "cachyos-bootanimation", ""), "CachyOS Boot");
        assert_eq!(theme_description(&snap, &d, "ru"), "Живой логотип");
        assert_eq!(theme_description(&snap, &d, "de"), "Animated logo");
    }

    /// Текущая тема заставки загрузки — как у plymouthd: `/etc` сильнее
    /// выбора через alternatives, тот сильнее умолчания пакета; прежний
    /// `/usr/share/plymouth/plymouthd.conf` `/etc` не перекрывает.
    #[test]
    fn plymouth_current_follows_plymouthd_weight() {
        let cur = |defaults: Option<&str>, alt: Option<&str>, legacy: Option<&str>, etc: Option<&str>| {
            let confs = plymouth_confs_by_weight(
                defaults.map(str::to_string),
                alt,
                legacy.map(str::to_string),
                etc.map(str::to_string),
            );
            plymouth_current_from(&confs.iter().map(String::as_str).collect::<Vec<_>>())
        };
        let defaults = Some("[Daemon]\nTheme=bgrt\nShowDelay=0\n");
        // Тему не меняли: в `/etc` только задержка — текущая из умолчания пакета.
        assert_eq!(cur(defaults, None, None, Some("[Daemon]\nShowDelay=5\n")).as_deref(), Some("bgrt"));
        assert_eq!(cur(defaults, None, None, Some("[Daemon]\nTheme=spinner\n")).as_deref(), Some("spinner"));
        assert_eq!(cur(defaults, Some("ubuntu-logo"), None, None).as_deref(), Some("ubuntu-logo"));
        assert_eq!(cur(defaults, Some(" "), None, None).as_deref(), Some("bgrt"), "пустая ссылка не в счёт");
        assert_eq!(cur(defaults, Some("ubuntu-logo"), None, Some("[Daemon]\nTheme=glow\n")).as_deref(), Some("glow"));
        assert_eq!(cur(None, None, Some("[Daemon]\nTheme=old\n"), Some("[Daemon]\nTheme=new\n")).as_deref(), Some("new"));
        assert_eq!(cur(None, None, None, None), None);
    }

    /// Реестр установок KNewStuff: пути записи и ссылка на её картинку.
    #[test]
    fn kns_registry_gives_installed_paths_with_preview() {
        let xml = r#"<?xml version="1.0"?><hotnewstuffregistry>
<stuff category="Plasma Themes"><name>Nordic</name><installedfile>/h/.local/share/plasma/desktoptheme/Nordic/*</installedfile>
<installedfile>/h/.local/share/plasma/desktoptheme/Nordic-dark/*</installedfile>
<preview>https://x/a.png?x=1&amp;y=2</preview><previewBig>https://x/big.png</previewBig><status>installed</status></stuff>
<stuff category="Colors"><name>Sweet</name><installedfile>/h/.local/share/color-schemes/Sweet.colors</installedfile>
<preview></preview><previewBig>https://x/sweet.png</previewBig></stuff>
<stuff><name>Без картинки</name><installedfile>/h/x</installedfile></stuff></hotnewstuffregistry>"#;
        assert_eq!(
            parse_kns_registry(xml),
            vec![
                ("/h/.local/share/plasma/desktoptheme/Nordic".to_string(), "https://x/a.png?x=1&y=2".to_string()),
                ("/h/.local/share/plasma/desktoptheme/Nordic-dark".to_string(), "https://x/a.png?x=1&y=2".to_string()),
                ("/h/.local/share/color-schemes/Sweet.colors".to_string(), "https://x/sweet.png".to_string()),
            ]
        );
        assert!(parse_kns_registry("<hotnewstuffregistry/>").is_empty());
    }

    /// Второй шаг цепочки превью: запись реестра установок находит картинку
    /// по пути темы, а сопоставление по имени не выходит за свой раздел и
    /// требует того же имени.
    #[test]
    fn store_picture_follows_install_record_and_section() {
        let fs = FsSnapshot::from_parts(
            HashMap::from([("/h/themes/Nordic".to_string(), Vec::new())]),
            ["/c/nordic.png", "/c/icons.png", "/c/orange.png"]
                .iter()
                .map(|p| (p.to_string(), String::new()))
                .collect(),
        );
        let files = HashMap::from([
            ("u-nordic".to_string(), "/c/nordic.png".to_string()),
            ("u-icons".to_string(), "/c/icons.png".to_string()),
            ("u-orange".to_string(), "/c/orange.png".to_string()),
        ]);
        let preview_file = |url: &str| files.get(url).cloned();
        let store = StoreRecords {
            sections: HashMap::from([
                ("u-icons".to_string(), "icons".to_string()),
                ("u-orange".to_string(), "gtk".to_string()),
            ]),
            installed: vec![("/h/themes/Nordic".to_string(), "u-nordic".to_string())],
            rexon_paths: None,
        };
        let ocs = vec![
            ("win7-icon-theme".to_string(), "u-icons".to_string()),
            ("Glassy GTK - [ Orange ]".to_string(), "u-orange".to_string()),
        ];
        let env = ScanEnv {
            home: "/h",
            usr_root: "/usr",
            fs: &fs,
            ocs_cache: &ocs,
            preview_file: &preview_file,
            store: &store,
            locale: "ru",
            standard_icons: None,
        };
        assert_eq!(resolve_preview_in(&env, "plasma", "Nordic", "/h/themes/Nordic", "/h/themes/Nordic", None, &[]), "/c/nordic.png");
        // Тема Kvantum «Win7» картинку набора значков не берёт…
        assert_eq!(backfill_preview_for(&env, "kvantum", "Win7"), "");
        // …и в своём разделе похожего имени мало: «Orang» — не «…Orange».
        assert_eq!(backfill_preview_for(&env, "gtk", "Orang"), "");
        assert_eq!(backfill_preview_for(&env, "icons", "Win7 Icon Theme"), "/c/icons.png");
        // Без раздела — прежнее правило подстроки.
        assert_eq!(backfill_preview(&env, "Win7"), "/c/icons.png");
    }

    /// Первый шаг цепочки: миниатюра вида в подкаталоге темы и снимок,
    /// названный метаданными темы экрана входа.
    #[test]
    fn own_picture_is_found_in_kind_subdir_and_sddm_metadata() {
        let home = tmp_dir("own-picture");
        let themes = home.join("themes");
        write(&themes.join("XP/gtk-3.0/thumbnail.png"), "");
        write(&themes.join("XP/gtk-3.0/gtk.css"), "");
        write(&themes.join("XP/cinnamon/thumbnail.png"), "");
        write(&themes.join("Login/metadata.desktop"), "[SddmGreeterTheme]\nName=Login\nScreenshot=shots/login.jpg\n");
        write(&themes.join("Login/shots/login.jpg"), "");
        let snap = live_snapshot(&[s(&themes)], &[]);
        let xp = s(&themes.join("XP"));
        assert_eq!(find_preview_in(&snap, &xp, None, &["gtk-3.0", "gtk-2.0"]), format!("{xp}/gtk-3.0/thumbnail.png"));
        assert_eq!(find_preview_in(&snap, &xp, None, &["cinnamon"]), format!("{xp}/cinnamon/thumbnail.png"));
        assert_eq!(find_preview_in(&snap, &xp, None, &["xfwm4"]), "", "чужая миниатюра виду не подходит");
        let login = s(&themes.join("Login"));
        assert_eq!(find_preview(&snap, &login, None), format!("{login}/shots/login.jpg"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// «Ничьё» — только то, о чём pacman сказал это сам; принадлежащее пакету
    /// и чужие строки ошибок ничьими не становятся.
    #[test]
    fn qo_unowned_takes_only_what_pacman_named() {
        let err = "error: No package owns /usr/share/sddm/themes/Apple-Sonoma-v2\n\
                   error: No package owns /usr/share/plymouth/themes/by-hand/\n\
                   error: failed to read file '/usr/share/sddm/themes/x': Permission denied\n";
        let got = parse_qo_unowned(err);
        assert_eq!(got.len(), 2);
        assert!(got.contains("/usr/share/sddm/themes/Apple-Sonoma-v2"));
        assert!(got.contains("/usr/share/plymouth/themes/by-hand"));
        assert!(parse_qo_unowned("").is_empty());
    }

    /// Окружения со своим хранилищем оформления: роли gtk/icons/cursor
    /// читаются оттуда, по той же таблице схем, что у применения (у MATE
    /// курсор — `org.mate.peripherals-mouse`); KDE, LXQt и тайловые WM
    /// хранилище не спрашивают вовсе.
    #[test]
    fn native_role_values_reads_de_store() {
        let mut asked: Vec<Vec<String>> = Vec::new();
        let got = native_role_values("mate", |cmd| {
            asked.push(cmd.argv.clone());
            Some(match cmd.argv[3].as_str() {
                "gtk-theme" => "'Menta'\n".to_string(),
                "icon-theme" => "'Papirus'\n".to_string(),
                _ => "'Bibata-Modern-Ice'\n".to_string(),
            })
        });
        assert_eq!(
            got,
            [Some("Menta".into()), Some("Papirus".into()), Some("Bibata-Modern-Ice".into())]
        );
        assert_eq!(asked[0], ["gsettings", "get", "org.mate.interface", "gtk-theme"]);
        assert_eq!(asked[1], ["gsettings", "get", "org.mate.interface", "icon-theme"]);
        assert_eq!(asked[2], ["gsettings", "get", "org.mate.peripherals-mouse", "cursor-theme"]);

        for (de, schema) in [
            ("gnome", "org.gnome.desktop.interface"),
            ("cinnamon", "org.cinnamon.desktop.interface"),
            ("budgie", "org.gnome.desktop.interface"),
            ("pantheon", "org.gnome.desktop.interface"),
            ("cosmic", "org.gnome.desktop.interface"),
        ] {
            let mut first = None;
            native_role_values(de, |cmd| {
                first.get_or_insert_with(|| cmd.argv.clone());
                None
            });
            assert_eq!(first.unwrap()[2], schema, "{de}");
        }

        let xf = native_role_values("xfce", |cmd| Some(format!("{}\n", cmd.argv[4])));
        assert_eq!(
            xf,
            [
                Some("/Net/ThemeName".into()),
                Some("/Net/IconThemeName".into()),
                Some("/Gtk/CursorThemeName".into())
            ]
        );
        let dd = native_role_values("deepin", |_| Some("s \"bloom\"\n".into()));
        assert_eq!(dd[1].as_deref(), Some("bloom"));

        for de in ["kde", "lxqt", "hyprland", "sway", "niri", ""] {
            let got = native_role_values(de, |_| panic!("{de}: хранилище спрошено"));
            assert_eq!(got, [None, None, None], "{de}");
        }
        // Пустой ответ и «—» значением не считаются.
        assert_eq!(native_role_values("mate", |_| Some("''\n".into())), [None, None, None]);
    }

    /// Значение родного хранилища побеждает kdeglobals и `settings.ini`;
    /// без него — прежняя цепочка.
    #[test]
    fn active_map_prefers_native_store() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\n";
        let base = || ActiveMapSources {
            icons: "breeze",
            mouse_cursor: "breeze_cursors",
            gtk_settings_ini: Some(ini),
            ..ActiveMapSources::default()
        };
        let key = |src: &ActiveMapSources, k: &str| {
            active_map_from_sources(src).get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        let mate = ActiveMapSources {
            native_gtk: Some("Menta"),
            native_icons: Some("Papirus"),
            native_cursor: Some("Bibata-Modern-Ice"),
            ..base()
        };
        assert_eq!(key(&mate, "gtk"), "Menta");
        assert_eq!(key(&mate, "icons"), "Papirus");
        assert_eq!(key(&mate, "cursor"), "Bibata-Modern-Ice");
        let old = base();
        assert_eq!(key(&old, "gtk"), "Adwaita");
        assert_eq!(key(&old, "icons"), "breeze");
        assert_eq!(key(&old, "cursor"), "breeze_cursors");
        let blank = ActiveMapSources { native_icons: Some(" "), native_cursor: Some("—"), ..base() };
        assert_eq!(key(&blank, "icons"), "breeze");
        assert_eq!(key(&blank, "cursor"), "breeze_cursors");
    }

    /// «Скачано» — только то, что поставил Rexon или магазин; тема,
    /// положенная руками (или прежним .rxn), — «Установлена вручную».
    #[test]
    fn origin_label_tells_downloaded_from_manual() {
        let store = StoreRecords {
            installed: vec![("/h/.local/share/themes/FromKns".to_string(), String::new())],
            rexon_paths: Some(vec!["/h/.local/share/themes/FromRexon/".to_string()]),
            ..StoreRecords::default()
        };
        assert_eq!(store.origin_label("/h/.local/share/themes/FromRexon"), DOWNLOADED_LABEL);
        assert_eq!(store.origin_label("/h/.local/share/themes/FromKns"), DOWNLOADED_LABEL);
        assert_eq!(store.origin_label("/h/.local/share/themes/Windows XP Luna"), MANUAL_LABEL);
        // Журнал не читали — прежняя подпись.
        assert_eq!(StoreRecords::default().origin_label("/h/x"), DOWNLOADED_LABEL);
    }

    /// Пометка «нет значков/курсора темы» — по плану просьб темы.
    #[test]
    fn missing_deps_note_names_what_is_missing() {
        use crate::themes::{DepItem, DepRole, DepState};
        let item = |role, state| DepItem { role, wanted: "X".into(), state };
        let both = [item(DepRole::Icons, DepState::Missing), item(DepRole::Cursor, DepState::Installable("p".into()))];
        assert!(missing_deps_note(&both).is_some_and(|n| n.contains("icons and cursor")));
        let icons = [item(DepRole::Icons, DepState::Missing), item(DepRole::Cursor, DepState::Installed("C".into()))];
        assert!(missing_deps_note(&icons).is_some_and(|n| n.contains("icons are")));
        assert_eq!(missing_deps_note(&[item(DepRole::Icons, DepState::Installed("I".into()))]), None);
        let mut items = vec![
            ThemeItem { id: "local:gtk:WhiteSur-Dark-nord".into(), author: "a".into(), ..Default::default() },
            ThemeItem { id: "local:marco:WhiteSur-Dark-nord".into(), author: "b".into(), ..Default::default() },
        ];
        stamp_missing_deps(&mut items, &|_| both.to_vec());
        assert!(items[0].author.starts_with("a · ⚠️"));
        assert_eq!(items[1].author, "b");
    }
}
