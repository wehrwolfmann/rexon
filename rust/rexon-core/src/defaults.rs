//! Защита приложений по умолчанию при смене DE — порт
//! `rexon/defaults.py` (класс `SystemDefaultsProtector`).
//!
//! Модуль сохраняет/восстанавливает «приложения по умолчанию» (файловый
//! менеджер, текстовый редактор, браузер, почта, просмотрщики, плееры,
//! архиватор, терминал) при установке соседнего DE: снимок текущих defaults
//! через `xdg-mime`, восстановление старого выбора после установки + запрос
//! пользователю «появилось новое приложение, переключиться?».
//!
//! Принцип порта (как `system_plan.rs` / `installed_scan.rs`): вся ЧИСТАЯ
//! логика — детерминированные функции вход→выход; побочные эффекты (реальный
//! subprocess `xdg-mime`/`gsettings`/`kwriteconfig6`, чтение/запись
//! `~/.config`) НЕ исполняются здесь. Они выражаются либо ПЛАНАМИ команд/
//! записей ([`TermStep`], [`SetDefault`], [`ProtectDeOutcome`]), либо приходят
//! готовыми значениями/колбэками (содержимое файлов, `desktop_file_exists`,
//! вывод `xdg-mime`/`gsettings`). Живое исполнение остаётся на Python-стороне
//! / будущем мосте.
//!
//! Паритет БАЙТ-В-БАЙТ с Python: та же сериализация snapshot/rules/pending
//! (`json.dump(indent=2, ensure_ascii=False)` — совпадает с
//! `serde_json::to_string_pretty` при feature `preserve_order`), тот же argv
//! команд, та же генерация `*-mimeapps.list`, тот же разбор
//! `xdg-terminals.list`/`kdeglobals`/`mimeapps.list`. Эталоны сняты с живого
//! Python read-only (`tests/golden/defaults_cases.json`).
//!
//! Сохранённые квирки оригинала (намеренное расхождение НЕ вводится, паритет):
//!   * `TerminalService` в `kdeglobals` возвращается «как есть», а
//!     `TerminalApplication` дополняется суффиксом `.desktop` — асимметрия
//!     оригинала сохранена;
//!   * `binary = terminal_desktop.replace(".desktop", "")` затирает ВСЕ
//!     вхождения подстроки `.desktop`, затем берётся хвост после последней
//!     точки — сохранено;
//!   * `action = "updated" if existing else "created"` в `protect_de_mimeapps`
//!     смотрит на НЕПУСТОТУ разобранного словаря, а не на факт наличия файла
//!     — сохранено.
//!
//! НЕ портировано (исполнение — при подключении к живому Python):
//!   реальные `subprocess.run`/`safe_run` (`xdg-mime query/default`,
//!   `gsettings`, `kwriteconfig6`), реальные чтение/запись файлов
//!   `~/.config/rexon/*.json`, `~/.config/xdg-terminals.list`,
//!   `~/.config/kdeglobals`, `~/.config/environment.d/rexon-terminal.conf`,
//!   `~/.config/*-mimeapps.list`, `os.makedirs`, логирование.

use crate::i18n::{n_, tr};
use serde::Serialize;
use serde_json::{Map, Value};

// ============================================================================
// Таблицы-константы (порядок = как в Python dict, важен для сериализации)
// ============================================================================

/// `SystemDefaultsProtector.MIME_TYPES` — тип приложения → MIME.
pub const MIME_TYPES: &[(&str, &str)] = &[
    ("file_manager", "inode/directory"),
    ("text_editor", "text/plain"),
    ("browser", "x-scheme-handler/http"),
    ("email", "x-scheme-handler/mailto"),
    ("image_viewer", "image/jpeg"),
    ("video_player", "video/mp4"),
    ("pdf_viewer", "application/pdf"),
    ("archive_manager", "application/zip"),
    ("music_player", "audio/mpeg"),
];

/// MIME для типа приложения (`MIME_TYPES.get`).
pub fn mime_for(app_type: &str) -> Option<&'static str> {
    MIME_TYPES.iter().find(|(k, _)| *k == app_type).map(|(_, v)| *v)
}

/// Человеческие названия типов приложений — то, что читает покупатель в
/// «Настройках». Терминал стоит первым: он единственный настраивается не
/// через MIME, и в интерфейсе его называют так же, как остальные.
/// Подписи — `msgid`, переводит [`app_type_label`].
pub const APP_TYPE_LABELS: &[(&str, &str)] = &[
    ("terminal", n_("🖥️ Terminal")),
    ("file_manager", n_("📁 File Manager")),
    ("text_editor", n_("📝 Text Editor")),
    ("browser", n_("🌐 Web Browser")),
    ("email", n_("📧 Email Client")),
    ("image_viewer", n_("🖼️ Image Viewer")),
    ("video_player", n_("🎬 Video Player")),
    ("pdf_viewer", n_("📄 PDF Viewer")),
    ("archive_manager", n_("📦 Archive Manager")),
    ("music_player", n_("🎵 Music Player")),
];

/// Название типа приложения для человека. Незнакомый тип возвращается как
/// есть — так же, как это делал `APP_TYPE_LABELS.get(key, key)`.
pub fn app_type_label(app_type: &str) -> String {
    APP_TYPE_LABELS
        .iter()
        .find(|(k, _)| *k == app_type)
        .map(|(_, v)| tr(v))
        .unwrap_or_else(|| app_type.to_string())
}

/// Порядок ключей rules по умолчанию + значения — порт `self.rules`.
pub const DEFAULT_RULES: &[(&str, bool)] = &[
    ("enabled", true),
    ("terminal", true),
    ("file_manager", true),
    ("text_editor", true),
    ("browser", false), // браузер не трогаем по умолчанию
    ("email", false),   // email не трогаем
    ("image_viewer", true),
    ("video_player", true),
    ("pdf_viewer", true),
    ("archive_manager", true),
    ("music_player", true),
];

/// XDG_CURRENT_DESKTOP → имя файла `*-mimeapps.list` — порт `DE_DESKTOP_NAMES`.
pub const DE_DESKTOP_NAMES: &[(&str, &str)] = &[
    ("kde", "kde"),
    ("gnome", "gnome"),
    ("cinnamon-mint", "cinnamon"),
    ("cinnamon", "cinnamon"),
    ("xfce", "xfce"),
    ("mate", "mate"),
    ("budgie", "budgie"),
    ("pantheon", "pantheon"),
    // Deepin: `XDG_CURRENT_DESKTOP=DDE` → `dde-mimeapps.list`
    // (dde-application-manager `applicationmimeinfo.cpp`: `<desktop>-mimeapps.list`).
    ("deepin", "dde"),
    ("cosmic", "cosmic"),
    ("hyprland", "hyprland"),
];

fn desktop_name_for(de_id: &str) -> &str {
    DE_DESKTOP_NAMES
        .iter()
        .find(|(k, _)| *k == de_id)
        .map(|(_, v)| *v)
        .unwrap_or(de_id)
}

/// Родные приложения каждого DE — порт `DE_NATIVE_APPS`. Порядок пар внутри
/// DE важен (порядок формирования valid_entries до сортировки на записи).
pub const DE_NATIVE_APPS: &[(&str, &[(&str, &str)])] = &[
    (
        "kde",
        &[
            ("inode/directory", "org.kde.dolphin.desktop"),
            ("text/plain", "org.kde.kate.desktop"),
            ("application/pdf", "okularApplication_pdf.desktop"),
            ("image/jpeg", "org.kde.gwenview.desktop"),
            ("image/png", "org.kde.gwenview.desktop"),
            ("image/gif", "org.kde.gwenview.desktop"),
            ("image/webp", "org.kde.gwenview.desktop"),
            ("image/svg+xml", "org.kde.gwenview.desktop"),
            ("image/avif", "org.kde.gwenview.desktop"),
            ("image/heif", "org.kde.gwenview.desktop"),
            ("image/tiff", "org.kde.gwenview.desktop"),
            ("image/bmp", "org.kde.gwenview.desktop"),
            ("image/jxl", "org.kde.gwenview.desktop"),
            ("video/mp4", "org.kde.haruna.desktop"),
            ("video/x-matroska", "org.kde.haruna.desktop"),
            ("video/webm", "org.kde.haruna.desktop"),
            ("video/mpeg", "org.kde.haruna.desktop"),
            ("video/quicktime", "org.kde.haruna.desktop"),
            ("video/x-msvideo", "org.kde.haruna.desktop"),
            ("audio/mpeg", "org.kde.elisa.desktop"),
            ("audio/x-flac", "org.kde.elisa.desktop"),
            ("audio/flac", "org.kde.elisa.desktop"),
            ("audio/ogg", "org.kde.elisa.desktop"),
            ("audio/x-wav", "org.kde.elisa.desktop"),
            ("audio/aac", "org.kde.elisa.desktop"),
            ("audio/mp4", "org.kde.elisa.desktop"),
            ("application/zip", "org.kde.ark.desktop"),
            ("application/gzip", "org.kde.ark.desktop"),
            ("application/x-7z-compressed", "org.kde.ark.desktop"),
            ("application/vnd.rar", "org.kde.ark.desktop"),
            ("application/x-tar", "org.kde.ark.desktop"),
            ("application/x-compressed-tar", "org.kde.ark.desktop"),
            ("application/x-bzip2-compressed-tar", "org.kde.ark.desktop"),
            ("application/x-xz-compressed-tar", "org.kde.ark.desktop"),
            ("application/epub+zip", "org.kde.okular.desktop"),
            ("image/vnd.djvu", "org.kde.okular.desktop"),
            ("application/oxps", "org.kde.okular.desktop"),
            ("text/markdown", "org.kde.kate.desktop"),
            ("application/json", "org.kde.kate.desktop"),
            ("application/x-yaml", "org.kde.kate.desktop"),
            ("text/html", "org.kde.kate.desktop"),
            ("text/xml", "org.kde.kate.desktop"),
            ("text/calendar", "org.kde.merkuro.calendar.desktop"),
            ("text/vcard", "org.kde.kaddressbook.desktop"),
            ("x-scheme-handler/mailto", "org.kde.kmail2.desktop"),
        ],
    ),
    (
        "gnome",
        &[
            ("inode/directory", "org.gnome.Nautilus.desktop"),
            ("text/plain", "org.gnome.TextEditor.desktop"),
            ("application/pdf", "org.gnome.Papers.desktop"),
            ("image/jpeg", "org.gnome.Loupe.desktop"),
            ("image/png", "org.gnome.Loupe.desktop"),
            ("image/gif", "org.gnome.Loupe.desktop"),
            ("image/webp", "org.gnome.Loupe.desktop"),
            ("image/svg+xml", "org.gnome.Loupe.desktop"),
            ("image/avif", "org.gnome.Loupe.desktop"),
            ("image/heif", "org.gnome.Loupe.desktop"),
            ("image/heic", "org.gnome.Loupe.desktop"),
            ("image/tiff", "org.gnome.Loupe.desktop"),
            ("image/bmp", "org.gnome.Loupe.desktop"),
            ("image/jxl", "org.gnome.Loupe.desktop"),
            ("video/mp4", "org.gnome.Showtime.desktop"),
            ("video/x-matroska", "org.gnome.Showtime.desktop"),
            ("video/webm", "org.gnome.Showtime.desktop"),
            ("video/mpeg", "org.gnome.Showtime.desktop"),
            ("video/ogg", "org.gnome.Showtime.desktop"),
            ("audio/mpeg", "org.gnome.Decibels.desktop"),
            ("audio/flac", "org.gnome.Decibels.desktop"),
            ("audio/ogg", "org.gnome.Decibels.desktop"),
            ("audio/mp4", "org.gnome.Decibels.desktop"),
            ("audio/x-wav", "org.gnome.Decibels.desktop"),
            ("application/zip", "org.gnome.FileRoller.desktop"),
            ("application/gzip", "org.gnome.FileRoller.desktop"),
            ("application/x-7z-compressed", "org.gnome.FileRoller.desktop"),
            ("application/x-rar", "org.gnome.FileRoller.desktop"),
            ("application/x-tar", "org.gnome.FileRoller.desktop"),
            ("application/x-compressed-tar", "org.gnome.FileRoller.desktop"),
            ("application/epub+zip", "org.gnome.Papers.desktop"),
            ("image/vnd.djvu", "org.gnome.Papers.desktop"),
            ("text/calendar", "org.gnome.Calendar.desktop"),
            ("x-scheme-handler/webcal", "org.gnome.Calendar.desktop"),
            ("text/vcard", "org.gnome.Contacts.desktop"),
            ("font/ttf", "org.gnome.font-viewer.desktop"),
            ("font/otf", "org.gnome.font-viewer.desktop"),
        ],
    ),
    (
        "cinnamon-mint",
        &[
            ("inode/directory", "nemo.desktop"),
            ("text/plain", "xed.desktop"),
            ("application/pdf", "xreader.desktop"),
            ("image/jpeg", "xviewer.desktop"),
            ("image/png", "xviewer.desktop"),
            ("image/gif", "xviewer.desktop"),
            ("image/webp", "xviewer.desktop"),
            ("image/svg+xml", "xviewer.desktop"),
            ("video/mp4", "xplayer.desktop"),
            ("video/x-matroska", "xplayer.desktop"),
            ("video/webm", "xplayer.desktop"),
            ("application/epub+zip", "xreader.desktop"),
            ("image/vnd.djvu", "xreader.desktop"),
        ],
    ),
    (
        "xfce",
        &[
            ("inode/directory", "thunar.desktop"),
            ("text/plain", "org.xfce.mousepad.desktop"),
            ("image/jpeg", "org.xfce.ristretto.desktop"),
            ("image/png", "org.xfce.ristretto.desktop"),
            ("image/gif", "org.xfce.ristretto.desktop"),
            ("image/webp", "org.xfce.ristretto.desktop"),
            ("image/svg+xml", "org.xfce.ristretto.desktop"),
            ("image/avif", "org.xfce.ristretto.desktop"),
            ("image/tiff", "org.xfce.ristretto.desktop"),
            ("video/mp4", "org.xfce.Parole.desktop"),
            ("video/x-matroska", "org.xfce.Parole.desktop"),
            ("video/webm", "org.xfce.Parole.desktop"),
            ("audio/mpeg", "org.xfce.Parole.desktop"),
            ("audio/flac", "org.xfce.Parole.desktop"),
            ("audio/ogg", "org.xfce.Parole.desktop"),
            ("application/json", "org.xfce.mousepad.desktop"),
            ("text/html", "org.xfce.mousepad.desktop"),
            ("text/xml", "org.xfce.mousepad.desktop"),
            ("text/markdown", "org.xfce.mousepad.desktop"),
        ],
    ),
    (
        "mate",
        &[
            ("inode/directory", "caja-folder-handler.desktop"),
            ("text/plain", "pluma.desktop"),
            ("application/pdf", "atril.desktop"),
            ("image/jpeg", "eom.desktop"),
            ("image/png", "eom.desktop"),
            ("image/gif", "eom.desktop"),
            ("image/bmp", "eom.desktop"),
            ("image/tiff", "eom.desktop"),
            ("image/svg+xml", "eom.desktop"),
            ("application/zip", "engrampa.desktop"),
            ("application/gzip", "engrampa.desktop"),
            ("application/x-7z-compressed", "engrampa.desktop"),
            ("application/x-rar", "engrampa.desktop"),
            ("application/x-tar", "engrampa.desktop"),
            ("application/x-compressed-tar", "engrampa.desktop"),
            ("application/epub+zip", "atril.desktop"),
            ("image/vnd.djvu", "atril.desktop"),
        ],
    ),
    (
        "cosmic",
        &[
            ("inode/directory", "com.system76.CosmicFiles.desktop"),
            ("text/plain", "com.system76.CosmicEdit.desktop"),
            ("video/mp4", "com.system76.CosmicPlayer.desktop"),
            ("video/x-matroska", "com.system76.CosmicPlayer.desktop"),
            ("video/webm", "com.system76.CosmicPlayer.desktop"),
            ("audio/mpeg", "com.system76.CosmicPlayer.desktop"),
            ("audio/flac", "com.system76.CosmicPlayer.desktop"),
        ],
    ),
    ("budgie", &[]),   // использует GNOME
    (
        "pantheon",
        &[
            ("inode/directory", "io.elementary.files.desktop"),
            ("text/plain", "io.elementary.code.desktop"),
            ("text/markdown", "io.elementary.code.desktop"),
            ("application/json", "io.elementary.code.desktop"),
            ("image/jpeg", "io.elementary.photos.viewer.desktop"),
            ("image/png", "io.elementary.photos.viewer.desktop"),
            ("image/gif", "io.elementary.photos.viewer.desktop"),
            ("image/webp", "io.elementary.photos.viewer.desktop"),
            ("image/tiff", "io.elementary.photos.viewer.desktop"),
            ("video/mp4", "io.elementary.videos.desktop"),
            ("video/x-matroska", "io.elementary.videos.desktop"),
            ("video/webm", "io.elementary.videos.desktop"),
            ("audio/mpeg", "io.elementary.music.desktop"),
            ("audio/flac", "io.elementary.music.desktop"),
            ("audio/ogg", "io.elementary.music.desktop"),
        ],
    ),
    // Deepin: программы группы `deepin` / `deepin-extra` (имена `.desktop`
    // сверены с их репозиториями linuxdeepin).
    (
        "deepin",
        &[
            ("inode/directory", "dde-file-manager.desktop"),
            ("text/plain", "deepin-editor.desktop"),
            ("text/markdown", "deepin-editor.desktop"),
            ("application/json", "deepin-editor.desktop"),
            ("image/jpeg", "deepin-image-viewer.desktop"),
            ("image/png", "deepin-image-viewer.desktop"),
            ("image/gif", "deepin-image-viewer.desktop"),
            ("image/webp", "deepin-image-viewer.desktop"),
            ("image/tiff", "deepin-image-viewer.desktop"),
            ("video/mp4", "deepin-movie.desktop"),
            ("video/x-matroska", "deepin-movie.desktop"),
            ("video/webm", "deepin-movie.desktop"),
            ("application/zip", "deepin-compressor.desktop"),
            ("application/x-7z-compressed", "deepin-compressor.desktop"),
        ],
    ),
    ("hyprland", &[]), // пользователь сам выбирает
];

fn native_apps_for(de_id: &str) -> Option<&'static [(&'static str, &'static str)]> {
    DE_NATIVE_APPS.iter().find(|(k, _)| *k == de_id).map(|(_, v)| *v)
}

// ============================================================================
// Rules — упорядоченный словарь bool (семантика Python dict)
// ============================================================================

/// Правила защиты: упорядоченные пары (как Python dict). `get` даёт None,
/// если ключа нет; вызывающие подставляют дефолт (`enabled`→true, прочие→false).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules(pub Vec<(String, bool)>);

impl Rules {
    /// Дефолтные правила — порт инициализации `self.rules`.
    pub fn defaults() -> Rules {
        Rules(DEFAULT_RULES.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    /// `self.rules.get(key)`.
    pub fn get(&self, key: &str) -> Option<bool> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    /// `self.rules.get("enabled", True)`.
    pub fn enabled(&self) -> bool {
        self.get("enabled").unwrap_or(true)
    }

    /// `self.rules.get(app_type, False)`.
    pub fn protect(&self, app_type: &str) -> bool {
        self.get(app_type).unwrap_or(false)
    }

    /// `self.rules.update(loaded)` — замена по позиции + дозапись новых ключей
    /// в конец (порт `_load_rules`). `loaded` — JSON-объект bool-значений
    /// (rules всегда bool).
    pub fn updated(&self, loaded: &Map<String, Value>) -> Rules {
        let mut out = self.0.clone();
        for (k, v) in loaded {
            let b = v.as_bool().unwrap_or(false);
            if let Some(e) = out.iter_mut().find(|(ek, _)| ek == k) {
                e.1 = b;
            } else {
                out.push((k.clone(), b));
            }
        }
        Rules(out)
    }

    /// `json.dumps(self.rules, indent=2, ensure_ascii=False)`.
    pub fn serialize(&self) -> String {
        let mut m = Map::new();
        for (k, v) in &self.0 {
            m.insert(k.clone(), Value::Bool(*v));
        }
        to_pretty(&Value::Object(m))
    }
}

/// `json.dumps(obj, indent=2, ensure_ascii=False)` (кириллица не экранируется,
/// отступ 2 пробела — совпадает при feature `preserve_order`).
fn to_pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("Value сериализуется")
}

// ============================================================================
// Снимок / pending — сериализация и загрузка
// ============================================================================

/// `_save_snapshot`: `{"protected_apps": {...}, "last_updated": ts}` →
/// json.dump(indent=2, ensure_ascii=False). `last_updated` (isoformat) —
/// недетерминирован, подаётся параметром.
pub fn serialize_snapshot(
    protected_apps: &[(String, Option<String>)],
    last_updated: &str,
) -> String {
    let mut inner = Map::new();
    for (k, v) in protected_apps {
        inner.insert(k.clone(), opt_to_value(v));
    }
    let mut outer = Map::new();
    outer.insert("protected_apps".to_string(), Value::Object(inner));
    outer.insert("last_updated".to_string(), Value::String(last_updated.to_string()));
    to_pretty(&Value::Object(outer))
}

/// `_save_pending`: `{app_type: {"old":.., "new":..}}` → json.dump(indent=2).
pub fn serialize_pending(pending: &[(String, (String, String))]) -> String {
    let mut m = Map::new();
    for (app_type, (old, new)) in pending {
        let mut choice = Map::new();
        choice.insert("old".to_string(), Value::String(old.clone()));
        choice.insert("new".to_string(), Value::String(new.clone()));
        m.insert(app_type.clone(), Value::Object(choice));
    }
    to_pretty(&Value::Object(m))
}

fn opt_to_value(v: &Option<String>) -> Value {
    match v {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    }
}

/// `_load_snapshot`: извлечь `protected_apps` из разобранного JSON
/// (порядок ключей как в файле). Отсутствует → пусто.
pub fn load_protected_apps(parsed: &Value) -> Vec<(String, Option<String>)> {
    parsed
        .get("protected_apps")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().map(String::from)))
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// Определение текущего терминала — порт _get_current_terminal
// ============================================================================

/// `val + ".desktop"` если ещё не оканчивается на `.desktop` (иначе как есть).
fn maybe_desktop(val: &str) -> String {
    if val.ends_with(".desktop") {
        val.to_string()
    } else {
        format!("{val}.desktop")
    }
}

/// Из какой схемы gsettings читать текущий терминал (ключ `exec`) — по
/// сырому `XDG_CURRENT_DESKTOP`; `None` — окружение держит его не в gsettings
/// (KDE, XFCE…) или своего места нет вовсе.
///
/// Одно решение на продукт (правило #12): раньше одна и та же цепочка
/// «gnome | cinnamon | budgie → схема GNOME, mate → схема MATE» жила тремя
/// копиями — здесь, в исполнителе смены стола и во вкладке «Настройки».
/// Cinnamon читается из схемы GNOME намеренно: её общий слой
/// [`set_terminal_plan`] пишет всегда.
pub fn terminal_exec_schema(desktop: &str) -> Option<&'static str> {
    let de = desktop.to_lowercase();
    if de.contains("kde") || de.contains("plasma") {
        return None;
    }
    if de.split(':').any(|t| t == "dde" || t == "deepin") {
        Some(DEEPIN_TERMINAL_SCHEMA)
    } else if ["gnome", "cinnamon", "budgie", "pantheon"].iter().any(|x| de.contains(x)) {
        Some("org.gnome.desktop.default-applications.terminal")
    } else if de.contains("mate") {
        Some("org.mate.applications-terminal")
    } else {
        None
    }
}

/// Порт `_get_current_terminal`. Все побочные источники поданы значениями:
///   * `xdg_terminals_list` — содержимое `~/.config/xdg-terminals.list`;
///   * `desktop` — сырое `XDG_CURRENT_DESKTOP` (лоуэркейс внутри);
///   * `kdeglobals` — содержимое `~/.config/kdeglobals` (только KDE-ветка);
///   * `gsettings_exec` — stdout соответствующей `gsettings get ... exec`
///     (схему выбирает [`terminal_exec_schema`]).
pub fn get_current_terminal(
    xdg_terminals_list: Option<&str>,
    desktop: &str,
    kdeglobals: Option<&str>,
    gsettings_exec: Option<&str>,
) -> Option<String> {
    // 1. xdg-terminals.list — freedesktop стандарт, независим от DE.
    if let Some(content) = xdg_terminals_list {
        let first = content.lines().next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }

    let de = desktop.to_lowercase();

    // 2. KDE: kdeglobals.
    if de.contains("kde") || de.contains("plasma") {
        if let Some(kd) = kdeglobals {
            for line in kd.lines() {
                let s = line.trim();
                if let Some(rest) = s.strip_prefix("TerminalService=") {
                    let val = rest.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
                if let Some(rest) = s.strip_prefix("TerminalApplication=") {
                    let val = rest.trim();
                    if !val.is_empty() {
                        return Some(maybe_desktop(val));
                    }
                }
            }
        }
        return Some("org.kde.konsole.desktop".to_string());
    }

    // 3-4. GNOME / Cinnamon / Budgie / Pantheon — схема GNOME, MATE — своя:
    // какую читать, решает [`terminal_exec_schema`], сюда приходит её ответ.
    if terminal_exec_schema(desktop).is_some() {
        if let Some(out) = gsettings_exec {
            let val = out.trim().trim_matches(|c| c == '\'' || c == '"');
            if !val.is_empty() {
                return Some(maybe_desktop(val));
            }
        }
    }

    // 5. XFCE.
    if de.contains("xfce") {
        return Some("xfce4-terminal.desktop".to_string());
    }

    None
}

// ============================================================================
// Свой перебор терминалов (там, где ни один источник ничего не сказал)
// ============================================================================
//
// ⚠️ Почему это вообще нужно. Общесистемный выбор терминала во freedesktop
// работает через утилиту `xdg-terminal-exec`: это её GLib зовёт первой, и
// это она читает `~/.config/xdg-terminals.list`, куда пишет Rexon. Но в
// официальных репозиториях Arch её нет ВОВСЕ (только AUR), в Fedora она
// появилась лишь в 42, а в Debian/Ubuntu/openSUSE есть, но не ставится ни
// по умолчанию, ни по зависимостям. То есть «из коробки» её нет нигде, и
// считать её наличие данностью нельзя. Ниже — свой перебор кандидатов:
// он даёт осмысленный ответ «какой терминал у человека», даже когда ни один
// источник настройки ничего не сказал.

/// Кандидаты терминалов: бинарь → возможные имена его `.desktop`.
///
/// Порядок — от «родных» терминалов крупных рабочих столов к универсальным.
/// Имён `.desktop` может быть несколько, потому что дистрибутивы называют
/// один и тот же файл по-разному (Arch — `org.gnome.Terminal.desktop`,
/// Debian — `gnome-terminal.desktop`); какое из них реально существует,
/// смотрит исполнитель и подаёт списком.
pub const TERMINAL_CANDIDATES: &[(&str, &[&str])] = &[
    ("konsole", &["org.kde.konsole.desktop", "konsole.desktop"]),
    ("kgx", &["org.gnome.Console.desktop", "kgx.desktop"]),
    (
        "gnome-terminal",
        &["org.gnome.Terminal.desktop", "gnome-terminal.desktop"],
    ),
    ("xfce4-terminal", &["xfce4-terminal.desktop"]),
    ("mate-terminal", &["mate-terminal.desktop"]),
    ("lxterminal", &["lxterminal.desktop"]),
    ("qterminal", &["qterminal.desktop"]),
    ("deepin-terminal", &["deepin-terminal.desktop"]),
    ("cosmic-term", &["com.system76.CosmicTerm.desktop"]),
    ("io.elementary.terminal", &["io.elementary.terminal.desktop"]),
    ("alacritty", &["Alacritty.desktop", "alacritty.desktop"]),
    ("kitty", &["kitty.desktop"]),
    ("foot", &["foot.desktop"]),
    ("wezterm", &["org.wezfurlong.wezterm.desktop"]),
    ("ghostty", &["com.mitchellh.ghostty.desktop"]),
    ("terminator", &["terminator.desktop"]),
    ("tilix", &["com.gexperts.Tilix.desktop"]),
    ("urxvt", &["rxvt-unicode.desktop", "urxvt.desktop"]),
    ("st", &["st.desktop"]),
    ("xterm", &["xterm.desktop", "debian-xterm.desktop"]),
];

/// Первый установленный терминал из [`TERMINAL_CANDIDATES`].
///
/// `available` — имена бинарей, найденных в PATH; `desktops_present` — имена
/// `.desktop`, реально лежащих в каталогах приложений. Если ни одно из имён
/// кандидата не нашлось, берём первое (канонический id по freedesktop) —
/// лучше записать его, чем не записать ничего.
pub fn pick_terminal(available: &[String], desktops_present: &[String]) -> Option<String> {
    for (binary, desktop_ids) in TERMINAL_CANDIDATES {
        if !available.iter().any(|b| b == binary) {
            continue;
        }
        let chosen = desktop_ids
            .iter()
            .find(|id| desktops_present.iter().any(|p| p == *id))
            .unwrap_or(&desktop_ids[0]);
        return Some((*chosen).to_string());
    }
    None
}

/// Честное предупреждение для UI, когда `xdg-terminal-exec` в системе нет.
///
/// Без него выбор терминала остаётся записанным (Rexon пишет его во все три
/// места), но часть программ, запускающих терминал через GLib, откроет свой.
/// Правило #13: говорим об этом по-человечески, а не молчим.
pub fn terminal_exec_missing_note() -> String {
    tr("Some apps will open their own terminal instead of the one you chose: a \
        system-wide choice needs the xdg-terminal-exec utility, and it isn't \
        installed. Rexon has already saved your choice — install the \
        “xdg-terminal-exec” package and it will work everywhere.")
}

/// Текст статуса терминала для UI: пусто, если предупреждать не о чем.
pub fn terminal_status_note(xdg_terminal_exec_present: bool) -> String {
    if xdg_terminal_exec_present {
        String::new()
    } else {
        terminal_exec_missing_note()
    }
}

// ============================================================================
// _set_ini_value — чистая трансформация INI (kdeglobals и подобные)
// ============================================================================

/// Порт `_set_ini_value`. `existing` — содержимое файла (None = файла нет).
/// `section` — с квадратными скобками (напр. `"[General]"`). Возвращает новое
/// содержимое файла.
///
/// Пустой `section` — файл без секций (`xfce4/helpers.rc`): ключ правится или
/// дописывается до первого заголовка `[…]`, если он вообще есть.
pub fn set_ini_value(existing: Option<&str>, section: &str, key: &str, value: &str) -> String {
    if section.is_empty() {
        return set_sectionless_value(existing, key, value);
    }
    // Python читает f.readlines() — строки С завершающим '\n' (кроме, быть
    // может, последней). split_inclusive воспроизводит это.
    let lines: Vec<&str> = match existing {
        Some(text) => text.split_inclusive('\n').collect(),
        None => Vec::new(),
    };

    let mut in_section = false;
    let mut found = false;
    let mut new_lines: Vec<String> = Vec::new();
    // `key = value` с пробелами — тот же ключ (так пишут, например,
    // `wayfire.ini`): иначе ключ дописывался бы вторым рядом с прежним.
    let is_key = |s: &str| s.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='));

    for line in &lines {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            if in_section && !found {
                new_lines.push(format!("{key}={value}\n"));
                found = true;
            }
            in_section = stripped == section;
        }
        if in_section && is_key(stripped) {
            new_lines.push(format!("{key}={value}\n"));
            found = true;
            continue;
        }
        new_lines.push((*line).to_string());
    }

    if !found {
        if !in_section {
            new_lines.push(format!("\n{section}\n"));
        }
        new_lines.push(format!("{key}={value}\n"));
    }

    new_lines.concat()
}

/// [`set_ini_value`] для ключа вне секций: правка по месту до первого
/// заголовка, иначе строка встаёт перед ним (или в конец файла без секций).
fn set_sectionless_value(existing: Option<&str>, key: &str, value: &str) -> String {
    let lines: Vec<&str> = existing.map(|t| t.split_inclusive('\n').collect()).unwrap_or_default();
    let is_key = |s: &str| s.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='));
    let new_line = format!("{key}={value}\n");
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    let mut in_head = true;
    for line in &lines {
        let stripped = line.trim();
        if in_head && stripped.starts_with('[') {
            in_head = false;
            if !done {
                out.push(new_line.clone());
                done = true;
            }
        }
        if in_head && !done && is_key(stripped) {
            out.push(new_line.clone());
            done = true;
            continue;
        }
        out.push(if line.ends_with('\n') { (*line).to_string() } else { format!("{line}\n") });
    }
    if !done {
        out.push(new_line);
    }
    out.concat()
}

// ============================================================================
// _set_terminal — план восстановления терминала
// ============================================================================

/// Шаг плана восстановления терминала (порт `_set_terminal`). Исполнение — при
/// подключении к живому Python.
/// ⚠️ Теги вариантов — ФОРМА для исполнителя плана (см. [`crate::themes::ApplyStep`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum TermStep {
    /// `open(path, "w").write(content)`.
    #[serde(rename = "write")]
    WriteFile { path: String, content: String },
    /// `os.makedirs(path, exist_ok=True)`.
    #[serde(rename = "makedirs")]
    MakeDirs { path: String },
    /// `subprocess.run(argv, check=False, capture_output=True)`.
    #[serde(rename = "run")]
    Run { argv: Vec<String> },
    /// `_set_ini_value(path, section, key, value)` (read-modify-write).
    #[serde(rename = "setini")]
    SetIni { path: String, section: String, key: String, value: String },
}

/// Где окружение держит СВОЙ терминал по умолчанию — одна таблица на весь
/// Rexon (правило #12). Её читают и вкладка «Настройки»/защита приложений
/// ([`set_terminal_plan`]), и смена рабочего стола
/// ([`crate::appdefaults::apply_de_specific`]); раньше у них были две разные
/// копии, и первая не писала Cinnamon, XFCE и LXQt вовсе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalHome {
    /// `kdeglobals [General] TerminalApplication/TerminalService`.
    Kde,
    /// `org.gnome.desktop.default-applications.terminal` (GNOME, Budgie,
    /// Pantheon — своего ключа терминала в схемах io.elementary.* нет).
    Gnome,
    /// `org.cinnamon.desktop.default-applications.terminal` — его берут Nemo
    /// «Открыть в терминале» и Ctrl+Alt+T.
    Cinnamon,
    /// `org.mate.applications-terminal`.
    Mate,
    /// `~/.config/xfce4/helpers.rc TerminalEmulator=` — `exo-open --launch
    /// TerminalEmulator`.
    Xfce,
    /// `~/.config/lxqt-qtxdg.conf TerminalEmulator=<id>.desktop` — libqtxdg
    /// `XdgDefaultApps::setTerminal`, файл `<первое из XDG_CURRENT_DESKTOP>-qtxdg`.
    Lxqt,
    /// `com.deepin.desktop.default-applications.terminal` `app-id`/`exec`/
    /// `exec-arg` — его берут Ctrl+Alt+T и «Открыть в терминале» Deepin
    /// (dde-daemon `bin/default-terminal`, `keybinding1`).
    Deepin,
    /// Своего места нет (COSMIC, тайловые): только общий слой.
    None,
}

/// Схема терминала Deepin (deepin-desktop-schemas).
pub const DEEPIN_TERMINAL_SCHEMA: &str = "com.deepin.desktop.default-applications.terminal";

/// Окружение → место терминала. Принимает и сырое `XDG_CURRENT_DESKTOP`
/// (`X-Cinnamon`, `Budgie:GNOME`, `LXQt`), и идентификатор Rexon
/// (`cinnamon-mint`, `budgie`). Своей таблицы окружений здесь нет: ответ дают
/// общие [`crate::de_customization::de_from_id`] и
/// [`crate::system::detect_current_de`].
pub fn terminal_home(desktop: &str) -> TerminalHome {
    use crate::de_customization::{de_from_id, De};
    let de = de_from_id(desktop).or_else(|| {
        crate::system::detect_current_de(desktop, "", false).and_then(|id| de_from_id(&id))
    });
    match de {
        Some(De::Kde) => TerminalHome::Kde,
        Some(De::Gnome | De::Budgie | De::Pantheon) => TerminalHome::Gnome,
        Some(De::Cinnamon) => TerminalHome::Cinnamon,
        Some(De::Mate) => TerminalHome::Mate,
        Some(De::Xfce) => TerminalHome::Xfce,
        Some(De::Lxqt) => TerminalHome::Lxqt,
        Some(De::Deepin) => TerminalHome::Deepin,
        _ => TerminalHome::None,
    }
}

/// Имя программы терминала по его `.desktop` — для `exec` в gsettings,
/// `TerminalApplication` KDE и помощника XFCE. Там, где имя бинаря не
/// совпадает с хвостом идентификатора, — явная таблица; иначе хвост после
/// последней точки в нижнем регистре (`org.kde.konsole` → `konsole`,
/// `Alacritty` → `alacritty`).
pub fn terminal_exec_name(terminal_desktop: &str) -> String {
    match terminal_desktop {
        "org.gnome.Console.desktop" => "kgx".to_string(),
        "org.gnome.Terminal.desktop" => "gnome-terminal".to_string(),
        "com.system76.CosmicTerm.desktop" => "cosmic-term".to_string(),
        // Хвост после точки дал бы «terminal» — такого бинаря нет.
        "io.elementary.terminal.desktop" => "io.elementary.terminal".to_string(),
        _ => {
            let s = terminal_desktop.replace(".desktop", "");
            s.rsplit('.').next().unwrap_or(&s).to_lowercase()
        }
    }
}

/// Ключ «выполнить команду» (`exec-arg`) для схем gsettings: у
/// gnome-terminal это `--` (`-e` у него устарел), у mate-terminal — `-x`,
/// у остальных — общепринятый `-e`.
pub fn terminal_exec_arg(exec_name: &str) -> &'static str {
    match exec_name {
        "gnome-terminal" => "--",
        "mate-terminal" => "-x",
        // `-e` у терминала elementary берёт ОДНУ строку на флаг, а
        // «остаток строки как команда» — это `-x` (`--commandline`,
        // src/Application.vala).
        "io.elementary.terminal" => "-x",
        _ => "-e",
    }
}

fn gsettings_terminal_steps(schema: &str, exec: &str) -> Vec<TermStep> {
    ["exec", "exec-arg"]
        .into_iter()
        .zip([exec, terminal_exec_arg(exec)])
        .map(|(key, val)| TermStep::Run {
            argv: vec!["gsettings".into(), "set".into(), schema.into(), key.into(), val.into()],
        })
        .collect()
}

// ============================================================================
// Помощники exo в XFCE (`helpers.rc`)
// ============================================================================
//
// `exo-open --launch TerminalEmulator` (и «Открыть в терминале» Thunar)
// берёт имя из `~/.config/xfce4/helpers.rc` и ищет ФАЙЛ помощника
// `xfce4/helpers/<имя>.desktop` в каталогах данных XDG. Помощники системы
// (пакет xfce4-settings) есть не у всех программ: у konsole, foot, kgx,
// mate-terminal, cosmic-term, wezterm, ghostty, ptyxis, Dolphin и Nemo их
// нет, и одно имя в `helpers.rc` даёт окно «выберите терминал» (Д7, проверка
// окружений 02.10.2026, снято живьём). Для таких программ пишется СВОЙ
// помощник `~/.local/share/xfce4/helpers/custom-<Категория>.desktop` — то же
// имя и тот же вид файла, что заводит сам xfce4-settings, когда человек
// выбирает «другую программу» (строки установленного `xfce4-mime-helper`).

/// Вид помощника XFCE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XfceHelper {
    Terminal,
    FileManager,
}

impl XfceHelper {
    /// Ключ `helpers.rc` — он же `X-XFCE-Category` файла помощника.
    pub fn key(self) -> &'static str {
        match self {
            XfceHelper::Terminal => "TerminalEmulator",
            XfceHelper::FileManager => "FileManager",
        }
    }

    /// Имя своего помощника (`custom-TerminalEmulator`).
    pub fn custom_id(self) -> String {
        format!("custom-{}", self.key())
    }
}

/// Каталоги помощников системы (`$XDG_DATA_DIRS` по умолчанию).
pub const XFCE_SYSTEM_HELPER_DIRS: &[&str] = &["/usr/local/share/xfce4/helpers", "/usr/share/xfce4/helpers"];

/// Есть ли у системы помощник `<name>.desktop`.
pub fn xfce_system_helper_exists(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && XFCE_SYSTEM_HELPER_DIRS
            .iter()
            .any(|d| std::path::Path::new(&format!("{d}/{name}.desktop")).is_file())
}

/// Текст своего помощника для программы `binary`.
pub fn xfce_custom_helper_text(kind: XfceHelper, binary: &str) -> String {
    let with_param = match kind {
        XfceHelper::Terminal => format!("{binary} {} %s", terminal_exec_arg(binary)),
        XfceHelper::FileManager => format!("{binary} %s"),
    };
    format!(
        "[Desktop Entry]\nNoDisplay=true\nVersion=1.0\nEncoding=UTF-8\nType=X-XFCE-Helper\n\
         X-XFCE-Category={}\nX-XFCE-Binaries={binary};\nX-XFCE-Commands={binary};\n\
         X-XFCE-CommandsWithParameter={with_param};\nIcon={binary}\nName={binary}\n",
        kind.key()
    )
}

/// Каталог данных рядом с каталогом настроек: `<дом>/.config` →
/// `<дом>/.local/share` (и `~/.config` → `~/.local/share`).
fn data_home_beside(cfg: &str) -> String {
    let cfg = cfg.trim_end_matches('/');
    match cfg.strip_suffix("/.config") {
        Some(base) => format!("{base}/.local/share"),
        None => format!("{cfg}/../.local/share"),
    }
}

/// Шаги «программа `binary` — помощник `kind` в XFCE»: имя в `helpers.rc`,
/// а если у системы помощника для неё нет (`has_system_helper == false`) —
/// сперва свой помощник `custom-…` в `~/.local/share/xfce4/helpers`.
pub fn xfce_helper_steps(kind: XfceHelper, binary: &str, cfg: &str, has_system_helper: bool) -> Vec<TermStep> {
    let mut steps = Vec::new();
    let value = if has_system_helper {
        binary.to_string()
    } else {
        let dir = format!("{}/xfce4/helpers", data_home_beside(cfg));
        steps.push(TermStep::MakeDirs { path: dir.clone() });
        steps.push(TermStep::WriteFile {
            path: format!("{dir}/{}.desktop", kind.custom_id()),
            content: xfce_custom_helper_text(kind, binary),
        });
        kind.custom_id()
    };
    steps.push(TermStep::SetIni {
        path: format!("{}/xfce4/helpers.rc", cfg.trim_end_matches('/')),
        section: String::new(),
        key: kind.key().into(),
        value,
    });
    steps
}

/// Записи терминала в РОДНОЕ место окружения (без общего слоя).
/// `cfg` — каталог настроек (`~/.config` или `$XDG_CONFIG_HOME`).
pub fn terminal_home_steps(home: TerminalHome, terminal_desktop: &str, cfg: &str) -> Vec<TermStep> {
    terminal_home_steps_with(home, terminal_desktop, cfg, &xfce_system_helper_exists)
}

/// То же, с подставным ответом «есть ли помощник системы» (для сверок).
pub fn terminal_home_steps_with(
    home: TerminalHome,
    terminal_desktop: &str,
    cfg: &str,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> Vec<TermStep> {
    let exec = terminal_exec_name(terminal_desktop);
    match home {
        TerminalHome::Kde => {
            let kd = format!("{cfg}/kdeglobals");
            let pairs = [("TerminalApplication", exec.as_str()), ("TerminalService", terminal_desktop)];
            let mut steps: Vec<TermStep> = pairs
                .iter()
                .map(|(key, val)| TermStep::SetIni {
                    path: kd.clone(),
                    section: "[General]".into(),
                    key: (*key).into(),
                    value: (*val).into(),
                })
                .collect();
            steps.extend(pairs.iter().map(|(key, val)| TermStep::Run {
                argv: vec![
                    "kwriteconfig6".into(), "--file".into(), "kdeglobals".into(),
                    "--group".into(), "General".into(), "--key".into(),
                    (*key).into(), (*val).into(),
                ],
            }));
            steps
        }
        TerminalHome::Gnome => {
            gsettings_terminal_steps("org.gnome.desktop.default-applications.terminal", &exec)
        }
        TerminalHome::Cinnamon => {
            gsettings_terminal_steps("org.cinnamon.desktop.default-applications.terminal", &exec)
        }
        TerminalHome::Mate => gsettings_terminal_steps("org.mate.applications-terminal", &exec),
        TerminalHome::Xfce => {
            let has = xfce_helper_exists(&exec);
            xfce_helper_steps(XfceHelper::Terminal, &exec, cfg, has)
        }
        TerminalHome::Lxqt => vec![TermStep::SetIni {
            path: format!("{cfg}/lxqt-qtxdg.conf"),
            section: "[General]".into(),
            key: "TerminalEmulator".into(),
            value: terminal_desktop.to_string(),
        }],
        TerminalHome::Deepin => {
            // `app-id` — имя `.desktop` без расширения: по нему служба Deepin
            // запускает терминал через менеджер приложений, `exec` — запасной.
            let app_id = terminal_desktop.trim_end_matches(".desktop");
            let mut steps = vec![TermStep::Run {
                argv: vec![
                    "gsettings".into(), "set".into(), DEEPIN_TERMINAL_SCHEMA.into(),
                    "app-id".into(), app_id.into(),
                ],
            }];
            steps.extend(gsettings_terminal_steps(DEEPIN_TERMINAL_SCHEMA, &exec));
            steps
        }
        TerminalHome::None => Vec::new(),
    }
}

/// Порт `_set_terminal`. `home` — раскрытие `~` (базовый каталог `~/.config`
/// строится как `{home}/.config`).
///
/// Общий слой пишется всегда (его видят программы вне своего окружения):
/// `xdg-terminals.list`, схема GNOME, `environment.d/rexon-terminal.conf`.
/// Затем — родное место окружения из [`terminal_home_steps`].
pub fn set_terminal_plan(terminal_desktop: &str, desktop: &str, home: &str) -> Vec<TermStep> {
    set_terminal_plan_with(terminal_desktop, desktop, home, &xfce_system_helper_exists)
}

/// То же, с подставным ответом «есть ли помощник XFCE у системы».
pub fn set_terminal_plan_with(
    terminal_desktop: &str,
    desktop: &str,
    home: &str,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> Vec<TermStep> {
    let exec = terminal_exec_name(terminal_desktop);
    let cfg = format!("{home}/.config");
    let de_home = terminal_home(desktop);

    let mut steps = Vec::new();

    // ── ОБЩИЙ СЛОЙ ──
    // 1. xdg-terminals.list.
    steps.push(TermStep::WriteFile {
        path: format!("{cfg}/xdg-terminals.list"),
        content: format!("{terminal_desktop}\n"),
    });
    // 2. Схема GNOME: её читают GLib-программы в любом окружении.
    steps.extend(terminal_home_steps(TerminalHome::Gnome, terminal_desktop, &cfg));
    // 3. environment.d/rexon-terminal.conf.
    let env_dir = format!("{cfg}/environment.d");
    steps.push(TermStep::MakeDirs { path: env_dir.clone() });
    steps.push(TermStep::WriteFile {
        path: format!("{env_dir}/rexon-terminal.conf"),
        content: format!("TERMINAL={exec}\n"),
    });

    // ── РОДНОЕ МЕСТО ОКРУЖЕНИЯ ── (GNOME/Budgie уже записаны общим слоем.)
    if de_home != TerminalHome::Gnome {
        steps.extend(terminal_home_steps_with(de_home, terminal_desktop, &cfg, xfce_helper_exists));
    }

    steps
}

// ============================================================================
// _set_default — установка defaults
// ============================================================================

/// Результат `_set_default` (порт). `Terminal` — исполнитель прогоняет
/// `set_terminal_plan`; `Xdg` — список argv `xdg-mime default …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetDefault {
    Terminal(String),
    Xdg(Vec<Vec<String>>),
}

/// Порт `_set_default`. Неизвестный тип без MIME → пустой `Xdg` (Python
/// `if not mime: return`). Браузер получает доп. `x-scheme-handler/https`.
pub fn set_default(app_type: &str, desktop: &str) -> SetDefault {
    if app_type == "terminal" {
        return SetDefault::Terminal(desktop.to_string());
    }
    let Some(mime) = mime_for(app_type) else {
        return SetDefault::Xdg(Vec::new());
    };
    let mut cmds = vec![vec![
        "xdg-mime".to_string(),
        "default".to_string(),
        desktop.to_string(),
        mime.to_string(),
    ]];
    if app_type == "browser" {
        cmds.push(vec![
            "xdg-mime".to_string(),
            "default".to_string(),
            desktop.to_string(),
            "x-scheme-handler/https".to_string(),
        ]);
    }
    SetDefault::Xdg(cmds)
}

// ============================================================================
// snapshot() — обновление protected_apps
// ============================================================================

/// Порт `snapshot`: `for key,val in current: if val and rules.get(key,False):
/// protected[key]=val`. `current` — упорядоченные пары (MIME_TYPES + terminal).
/// `existing` — уже сохранённые protected_apps (порядок сохраняется, новые
/// ключи дописываются).
pub fn compute_snapshot(
    current: &[(String, Option<String>)],
    rules: &Rules,
    existing: &[(String, Option<String>)],
) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = existing.to_vec();
    for (key, val) in current {
        let truthy = val.as_ref().is_some_and(|s| !s.is_empty());
        if truthy && rules.protect(key) {
            if let Some(e) = out.iter_mut().find(|(k, _)| k == key) {
                e.1 = val.clone();
            } else {
                out.push((key.clone(), val.clone()));
            }
        }
    }
    out
}

/// Решение «снимать ли эталон защиты при старте окна».
///
/// Раздел «Защита приложений по умолчанию» включён по умолчанию, но до
/// первого снятия эталона сверять подмену не с чем: Rexon не помнит ничего и
/// молча не защищает. Поэтому эталон снимается САМ при первом же запуске.
///
/// ⭐ Условие — по ФАКТУ, а не по отдельному ключу-флажку «первый запуск»:
/// защита включена И не запомнено ничего. Так оно само чинится, если эталон
/// удалили руками или потеряли вместе с профилем, и в правилах не заводится
/// ещё один ключ, который потом никто не сбросит. Тот же смысл «не
/// запомнено», что у строки состояния вкладки
/// (`qt_settings::protection_status_text`): пусто — сравнивать не с чем.
///
/// Уже снятый эталон НЕ переснимается: иначе второй запуск записал бы поверх
/// подменённое чужим установщиком значение как «правильное» — ровно то, от
/// чего защита и страхует. Пересъёмка остаётся за кнопкой «Запомнить
/// сейчас», то есть за осознанным действием человека.
pub fn should_snapshot_at_startup(
    rules: &Rules,
    remembered: &[(String, Option<String>)],
) -> bool {
    rules.enabled() && remembered.is_empty()
}

// ============================================================================
// restore_defaults() — решение восстановления
// ============================================================================

/// Итог `restore_defaults` (чистая часть). `results` — упорядочены по обходу
/// protected_apps; `set_calls` — какие `_set_default(app_type, desktop)`
/// исполнитель должен вызвать (в порядке); `pending` — итоговый
/// pending_choices (исходный + добавленные).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub results: Vec<(String, String)>,
    pub set_calls: Vec<(String, String)>,
    pub pending: Vec<(String, (String, String))>,
}

fn lookup<'a>(pairs: &'a [(String, Option<String>)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_deref())
}

/// Порт `restore_defaults`. `desktop_file_exists` — колбэк наличия `.desktop`.
/// `existing_pending` — текущий pending_choices (обычно пуст).
pub fn restore_defaults(
    protected_apps: &[(String, Option<String>)],
    rules: &Rules,
    current: &[(String, Option<String>)],
    de_switched: bool,
    existing_pending: &[(String, (String, String))],
    desktop_file_exists: &dyn Fn(&str) -> bool,
) -> RestoreOutcome {
    let mut outcome = RestoreOutcome {
        results: Vec::new(),
        set_calls: Vec::new(),
        pending: existing_pending.to_vec(),
    };

    if !rules.enabled() {
        // Python: возвращает {} и НЕ сохраняет pending (in-memory не тронут).
        return outcome;
    }

    for (app_type, saved) in protected_apps {
        if !rules.protect(app_type) {
            outcome.results.push((app_type.clone(), "disabled".into()));
            continue;
        }
        let saved_desktop = match saved {
            Some(s) if !s.is_empty() => s.as_str(),
            _ => {
                outcome.results.push((app_type.clone(), "absent".into()));
                continue;
            }
        };
        if !desktop_file_exists(saved_desktop) {
            outcome.results.push((app_type.clone(), "absent".into()));
            continue;
        }

        let cur = lookup(current, app_type);
        let changed = cur.is_some_and(|c| !c.is_empty() && c != saved_desktop);

        if !changed {
            outcome.results.push((app_type.clone(), "unchanged".into()));
            continue;
        }
        if de_switched {
            outcome.results.push((app_type.clone(), "skipped".into()));
            continue;
        }

        // DE НЕ переключён — восстанавливаем старое + запоминаем новое.
        outcome.set_calls.push((app_type.clone(), saved_desktop.to_string()));
        outcome.results.push((app_type.clone(), "restored".into()));

        if let Some(cur_desktop) = cur {
            if !cur_desktop.is_empty() && desktop_file_exists(cur_desktop) {
                let choice = (saved_desktop.to_string(), cur_desktop.to_string());
                if let Some(e) = outcome.pending.iter_mut().find(|(k, _)| k == app_type) {
                    e.1 = choice;
                } else {
                    outcome.pending.push((app_type.clone(), choice));
                }
            }
        }
    }

    outcome
}

// ============================================================================
// should_protect / resolve_choice
// ============================================================================

/// Порт `should_protect`.
pub fn should_protect(
    app_type: &str,
    rules: &Rules,
    protected_apps: &[(String, Option<String>)],
    desktop_file_exists: &dyn Fn(&str) -> bool,
) -> bool {
    if !rules.enabled() {
        return false;
    }
    if !rules.protect(app_type) {
        return false;
    }
    match lookup(protected_apps, app_type) {
        Some(d) if !d.is_empty() => desktop_file_exists(d),
        _ => false,
    }
}

/// Итог `resolve_choice` (чистая часть).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOutcome {
    /// `_set_default(app_type, new_desktop)` + обновление protected_apps —
    /// только при use_new и наличии choice.
    pub set_call: Option<(String, String)>,
    pub protected_apps: Vec<(String, Option<String>)>,
    pub pending: Vec<(String, (String, String))>,
}

/// Порт `resolve_choice`. `pop`-семантика: choice удаляется из pending
/// независимо от use_new; при use_new — set_default(new) + protected[app]=new.
pub fn resolve_choice(
    app_type: &str,
    use_new: bool,
    protected_apps: &[(String, Option<String>)],
    pending: &[(String, (String, String))],
) -> ResolveOutcome {
    let mut out = ResolveOutcome {
        set_call: None,
        protected_apps: protected_apps.to_vec(),
        pending: pending.to_vec(),
    };
    let Some(idx) = out.pending.iter().position(|(k, _)| k == app_type) else {
        // Нет choice — Python возвращает сразу.
        return out;
    };
    let (_, (_, new_desktop)) = out.pending.remove(idx);
    if use_new {
        out.set_call = Some((app_type.to_string(), new_desktop.clone()));
        if let Some(e) = out.protected_apps.iter_mut().find(|(k, _)| k == app_type) {
            e.1 = Some(new_desktop);
        } else {
            out.protected_apps.push((app_type.to_string(), Some(new_desktop)));
        }
    }
    out
}

// ============================================================================
// get_status_summary
// ============================================================================

/// Порт `get_status_summary`. Возвращает JSON-массив строк (как список dict
/// Python). `saved`/`current`: значение или `"—"`; `changed`: bool или null
/// (`saved and cur and saved != cur`).
pub fn get_status_summary(
    protected_apps: &[(String, Option<String>)],
    current: &[(String, Option<String>)],
    rules: &Rules,
) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let all_types: Vec<&str> = MIME_TYPES
        .iter()
        .map(|(k, _)| *k)
        .chain(std::iter::once("terminal"))
        .collect();
    for app_type in all_types {
        let saved = lookup(protected_apps, app_type);
        let cur = lookup(current, app_type);
        // changed = saved and cur and saved != cur → null если любой отсутствует
        let changed = match (saved, cur) {
            (Some(s), Some(c)) => Value::Bool(s != c),
            _ => Value::Null,
        };
        let mut row = Map::new();
        row.insert("type".into(), Value::String(app_type.to_string()));
        row.insert("saved".into(), Value::String(saved.unwrap_or("—").to_string()));
        row.insert("current".into(), Value::String(cur.unwrap_or("—").to_string()));
        row.insert("protected".into(), Value::Bool(rules.protect(app_type)));
        row.insert("changed".into(), changed);
        rows.push(Value::Object(row));
    }
    Value::Array(rows)
}

// ============================================================================
// _get_exec_from_desktop
// ============================================================================

/// Порт `_get_exec_from_desktop`: первая строка `Exec=` → первое слово после
/// `=`. `content` — содержимое `.desktop` (None = файла нет). Ищет по строкам.
pub fn get_exec_from_desktop(content: Option<&str>) -> Option<String> {
    let text = content?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Exec=") {
            // rest.strip().split()[0] — первое слово после '=' (split по
            // пробелам уже игнорирует края, отдельный strip/trim не нужен).
            return rest.split_whitespace().next().map(String::from);
        }
    }
    None
}

/// Пути поиска `.desktop` — порт `_desktop_file_exists`. `home` раскрывает `~`.
pub fn desktop_search_paths(desktop: &str, home: &str) -> [String; 2] {
    [
        format!("/usr/share/applications/{desktop}"),
        format!("{home}/.local/share/applications/{desktop}"),
    ]
}

// ============================================================================
// mimeapps.list — разбор / генерация / протекция per-DE
// ============================================================================

/// Порт `_parse_mimeapps_list`: секция `[Default Applications]` → {mime: desktop}
/// (desktop — до первой `;`). Возвращает упорядоченные пары (порядок файла).
pub fn parse_mimeapps_list(content: &str) -> Vec<(String, String)> {
    let mut result: Vec<(String, String)> = Vec::new();
    let mut in_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line == "[Default Applications]" {
            in_section = true;
            continue;
        }
        if line.starts_with('[') && in_section {
            break; // другая секция
        }
        if in_section {
            if let Some(eq) = line.find('=') {
                let mime = line[..eq].trim().to_string();
                let desktop =
                    line[eq + 1..].trim().split(';').next().unwrap_or("").to_string();
                // dict[mime] = desktop (перезапись сохраняет позицию).
                if let Some(e) = result.iter_mut().find(|(m, _)| *m == mime) {
                    e.1 = desktop;
                } else {
                    result.push((mime, desktop));
                }
            }
        }
    }
    result
}

/// Порт `_write_mimeapps_list`: заголовок + `[Default Applications]` + записи,
/// ОТСОРТИРОВАННЫЕ по MIME. `timestamp` = `datetime.now().isoformat(
/// timespec='seconds')` — подаётся параметром.
pub fn serialize_mimeapps_list(entries: &[(String, String)], timestamp: &str) -> String {
    let mut out = String::new();
    out.push_str("# Автоматически создан Rexon для изоляции DE\n");
    out.push_str(&format!("# Обновлён: {timestamp}\n"));
    out.push('\n');
    out.push_str("[Default Applications]\n");
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (mime, desktop) in sorted {
        out.push_str(&format!("{mime}={desktop}\n"));
    }
    out
}

/// Результат `protect_de_mimeapps` (чистая часть).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectDeOutcome {
    /// Упорядочены по обходу installed_des: (de_id, "created"/"updated"/
    /// "skipped"/"empty").
    pub results: Vec<(String, String)>,
    /// Файлы к записи: (абсолютный путь, содержимое) — в порядке обработки.
    pub writes: Vec<(String, String)>,
    /// Параллельно `writes`: сколько РОДНЫХ записей DE реально установлено
    /// (`len(valid_entries)` в логе оригинала — merge с пользовательскими
    /// записями это число не увеличивает).
    pub valid_counts: Vec<usize>,
}

/// Порт `protect_de_mimeapps` (чистая часть). Побочные эффекты — колбэками:
///   * `desktop_file_exists(desktop)` — установлен ли `.desktop`;
///   * `read_existing(path)` — содержимое существующего `*-mimeapps.list`
///     (None = файла нет).
///
/// `config_dir` — раскрытый `~/.config`; `timestamp` — для заголовка.
/// `current_de` в оригинале не используется (сохранён в сигнатуре для паритета).
pub fn protect_de_mimeapps(
    _current_de: &str,
    installed_des: &[String],
    config_dir: &str,
    timestamp: &str,
    desktop_file_exists: &dyn Fn(&str) -> bool,
    read_existing: &dyn Fn(&str) -> Option<String>,
) -> ProtectDeOutcome {
    let mut outcome = ProtectDeOutcome {
        results: Vec::new(),
        writes: Vec::new(),
        valid_counts: Vec::new(),
    };

    for de_id in installed_des {
        let native = match native_apps_for(de_id) {
            Some(n) if !n.is_empty() => n,
            _ => {
                outcome.results.push((de_id.clone(), "empty".into()));
                continue;
            }
        };

        let desktop_name = desktop_name_for(de_id);
        let path = format!("{config_dir}/{desktop_name}-mimeapps.list");

        // Только реально установленные .desktop (порядок native).
        let valid: Vec<(String, String)> = native
            .iter()
            .filter(|(_, d)| desktop_file_exists(d))
            .map(|(m, d)| (m.to_string(), d.to_string()))
            .collect();

        if valid.is_empty() {
            outcome.results.push((de_id.clone(), "skipped".into()));
            continue;
        }

        // Существующий файл → разбор (для merge и для action).
        let existing: Vec<(String, String)> = match read_existing(&path) {
            Some(content) => parse_mimeapps_list(&content),
            None => Vec::new(),
        };

        // merged = dict(existing); merged.update(valid) — union ключей
        // (порядок на выходе не важен: serialize сортирует по MIME).
        let mut merged = existing.clone();
        for (m, d) in &valid {
            if let Some(e) = merged.iter_mut().find(|(mm, _)| mm == m) {
                e.1 = d.clone();
            } else {
                merged.push((m.clone(), d.clone()));
            }
        }

        let content = serialize_mimeapps_list(&merged, timestamp);
        outcome.writes.push((path, content));
        outcome.valid_counts.push(valid.len());

        // Квирк: "updated" если РАЗОБРАННЫЙ словарь непуст (не факт наличия файла).
        let action = if existing.is_empty() { "created" } else { "updated" };
        outcome.results.push((de_id.clone(), action.to_string()));
    }

    outcome
}
