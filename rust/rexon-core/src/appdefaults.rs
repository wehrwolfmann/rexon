//! Порт `rexon/appdefaults.py` — автопереключение приложений по умолчанию
//! при смене DE (класс `DEAppSwitcher`).
//!
//! Здесь — ЧИСТАЯ логика: таблицы `DE → категория → .desktop`, построение
//! текста per-DE `mimeapps.list`, определение DE по окружению, построение
//! argv DE-специфичных команд (kdeglobals/gsettings) и текста XFCE
//! `helpers.rc`, тексты autostart-`.desktop` и apply-скрипта. Ничего не
//! исполняется: проверки существования `.desktop`-файлов и наличия бинарей
//! приходят замыканиями (`exists` / `which` — как `qg`/`qi` в
//! `system_plan::remove_plan`), а результат выражается структурами
//! (`ProfileOutcome`, `ApplyDeSpecific` — argv команд + содержимое файла).
//! Живой subprocess / запись файлов остаются на исполнителе (пока Python
//! через мост, позже rexon-gui).
//!
//! Портировано:
//!   * таблицы `DE_NATIVE_APPS` (8 DE) + generic-набор тайловых/wlroots WM
//!     (`wm_app_set`), `MIME_GROUPS`, `_FALLBACKS`, имена per-DE
//!     `mimeapps.list`, `_XDG_DESKTOP_TO_DE`, `_FM_DESKTOP_CANDIDATES`;
//!   * `resolve_de_apps` / `resolve_mimeapps_filename` / `is_tiling_wm` /
//!     `detect_current_de` / `fm_short_name` / `default_file_manager_for_de`;
//!   * `resolve_desktop` (с fallback-цепочкой), `generate_profile` (текст
//!     mimeapps.list байт-в-байт), `apply_de_specific` (+ приватные
//!     `_apply_kde/_gnome/_mate/_cinnamon/_xfce/_generic_wm`),
//!     `get_installed_des`, `get_de_apps_info`;
//!   * текст autostart-`.desktop` (`autostart_desktop_content`); прежний
//!     bash apply-скрипт на python3 снят 02.10.2026 — автозапуск зовёт бинарь
//!     с `--apply-defaults`;
//!   * чистая часть `set_file_manager_default` (выбор `.desktop`-цели +
//!     argv `xdg-mime default/query`) и regex-хелперы override'а
//!     `org.freedesktop.FileManager1` (`fm1_exec_basename` / `fm1_matches` /
//!     `busctl_pid`, argv busctl).
//!
//! НЕ портировано (исполнение — при подключении к живому Python/мосту):
//!   реальный `glob` каталогов `.desktop`, запуск `xdg-mime`/`busctl`,
//!   запись `mimeapps.list` / `helpers.rc` / autostart, чтение
//!   `/proc/<pid>/comm` и `os.kill` в `_override_filemanager1`,
//!   оркестрация `apply_for_current_de` / `on_switch_de` /
//!   `generate_all_profiles` / `install_autostart` (склейка вокруг чистых
//!   функций + файловые проверки).

use crate::defaults::{terminal_exec_name, TerminalHome};
use crate::i18n::{tr, trf};
use serde::Serialize;

// ============================================================================
// Таблицы DE → категория → .desktop
// ============================================================================

type AppTable = &'static [(&'static str, &'static str)];

const KDE_APPS: AppTable = &[
    ("file_manager", "org.kde.dolphin.desktop"),
    ("terminal", "org.kde.konsole.desktop"),
    ("text_editor", "org.kde.kate.desktop"),
    ("image_viewer", "org.kde.gwenview.desktop"),
    ("video_player", "org.kde.haruna.desktop"),
    ("pdf_viewer", "org.kde.okular.desktop"),
    ("archive_manager", "org.kde.ark.desktop"),
    ("music_player", "org.kde.elisa.desktop"),
    ("screenshot", "org.kde.spectacle.desktop"),
];

const GNOME_APPS: AppTable = &[
    ("file_manager", "org.gnome.Nautilus.desktop"),
    ("terminal", "org.gnome.Console.desktop"),
    ("text_editor", "org.gnome.TextEditor.desktop"),
    ("image_viewer", "org.gnome.Loupe.desktop"),
    ("video_player", "org.gnome.Showtime.desktop"),
    ("pdf_viewer", "org.gnome.Papers.desktop"),
    ("archive_manager", "org.gnome.FileRoller.desktop"),
];

const MATE_APPS: AppTable = &[
    ("file_manager", "caja.desktop"),
    ("terminal", "mate-terminal.desktop"),
    ("text_editor", "pluma.desktop"),
    ("image_viewer", "eom.desktop"),
    ("pdf_viewer", "atril.desktop"),
    ("archive_manager", "engrampa.desktop"),
];

const COSMIC_APPS: AppTable = &[
    ("file_manager", "com.system76.CosmicFiles.desktop"),
    ("terminal", "com.system76.CosmicTerm.desktop"),
    ("text_editor", "com.system76.CosmicEdit.desktop"),
    ("video_player", "com.system76.CosmicPlayer.desktop"),
];

const CINNAMON_APPS: AppTable = &[
    ("file_manager", "nemo.desktop"),
    ("terminal", "org.gnome.Terminal.desktop"),
    ("text_editor", "xed.desktop"),
    ("image_viewer", "xviewer.desktop"),
    ("video_player", "xplayer.desktop"),
    ("pdf_viewer", "xreader.desktop"),
];

const XFCE_APPS: AppTable = &[
    ("file_manager", "thunar.desktop"),
    ("terminal", "xfce4-terminal.desktop"),
    ("text_editor", "mousepad.desktop"),
    ("image_viewer", "ristretto.desktop"),
    ("video_player", "parole.desktop"),
    ("archive_manager", "xarchiver.desktop"),
];

// Budgie использует GNOME-приложения (тот же набор, что и gnome).
const BUDGIE_APPS: AppTable = &[
    ("file_manager", "org.gnome.Nautilus.desktop"),
    ("terminal", "org.gnome.Console.desktop"),
    ("text_editor", "org.gnome.TextEditor.desktop"),
    ("image_viewer", "org.gnome.Loupe.desktop"),
    ("video_player", "org.gnome.Showtime.desktop"),
    ("pdf_viewer", "org.gnome.Papers.desktop"),
    ("archive_manager", "org.gnome.FileRoller.desktop"),
];

// Pantheon: родные программы elementary из группы `pantheon` (идентификаторы
// `.desktop` — `meson.project_name()` их репозиториев; просмотр картинок —
// отдельный `.viewer` у Photos). Своих PDF-читалки и архиватора у Pantheon
// нет — их и не записываем.
const PANTHEON_APPS: AppTable = &[
    ("file_manager", "io.elementary.files.desktop"),
    ("terminal", "io.elementary.terminal.desktop"),
    ("text_editor", "io.elementary.code.desktop"),
    ("image_viewer", "io.elementary.photos.viewer.desktop"),
    ("video_player", "io.elementary.videos.desktop"),
];

// Deepin: программы групп `deepin` и `deepin-extra` (имена `.desktop`
// сверены с репозиториями linuxdeepin). Своего PDF-читателя в группах нет.
const DEEPIN_APPS: AppTable = &[
    ("file_manager", "dde-file-manager.desktop"),
    ("terminal", "deepin-terminal.desktop"),
    ("text_editor", "deepin-editor.desktop"),
    ("image_viewer", "deepin-image-viewer.desktop"),
    ("video_player", "deepin-movie.desktop"),
    ("archive_manager", "deepin-compressor.desktop"),
];

const LXQT_APPS: AppTable = &[
    ("file_manager", "pcmanfm-qt.desktop"),
    ("terminal", "qterminal.desktop"),
    ("text_editor", "featherpad.desktop"),
    ("image_viewer", "lximage-qt.desktop"),
    ("archive_manager", "lxqt-archiver.desktop"),
];

/// Родные приложения известного DE. None → тайловый/wlroots WM (generic).
/// Порядок в таблице = порядок словаря Python (важен для итераций).
fn native_apps(de_id: &str) -> Option<AppTable> {
    match de_id {
        "kde" => Some(KDE_APPS),
        "gnome" => Some(GNOME_APPS),
        "mate" => Some(MATE_APPS),
        "cosmic" => Some(COSMIC_APPS),
        "cinnamon-mint" => Some(CINNAMON_APPS),
        "xfce" => Some(XFCE_APPS),
        "budgie" => Some(BUDGIE_APPS),
        "pantheon" => Some(PANTHEON_APPS),
        "deepin" => Some(DEEPIN_APPS),
        "lxqt" => Some(LXQT_APPS),
        _ => None,
    }
}

/// Терминал generic-набора по WM — порт `_WM_TERMINALS` (пакеты из system.py).
fn wm_terminal(wm_id: &str) -> &'static str {
    match wm_id {
        "hyprland" => "kitty.desktop",
        "niri" => "alacritty.desktop",
        "sway" => "foot.desktop",
        // Оба ставятся с foot (`system::DE_DATABASE`); его же зовёт Super+Enter
        // стартового init River.
        "river" => "foot.desktop",
        "wayfire" => "foot.desktop",
        _ => "xterm.desktop",
    }
}

/// Generic-набор дефолтов для тайлового / wlroots WM (любого) —
/// порт `_wm_app_set`. Один общий набор, терминал по WM (rule #12).
fn wm_app_set(wm_id: &str) -> Vec<(&'static str, String)> {
    vec![
        ("file_manager", "thunar.desktop".to_string()),
        ("terminal", wm_terminal(wm_id).to_string()),
        ("text_editor", "mousepad.desktop".to_string()),
        ("image_viewer", "org.gnome.Loupe.desktop".to_string()),
        ("pdf_viewer", "org.gnome.Papers.desktop".to_string()),
        ("archive_manager", "xarchiver.desktop".to_string()),
    ]
}

/// Legacy / fallback имена `.desktop` — порт `_FALLBACKS` (порядок важен).
fn fallbacks(desktop_id: &str) -> &'static [&'static str] {
    match desktop_id {
        "org.gnome.Console.desktop" => {
            &["org.gnome.Terminal.desktop", "gnome-terminal.desktop"]
        }
        "org.gnome.TextEditor.desktop" => &["org.gnome.gedit.desktop", "gedit.desktop"],
        "org.gnome.Loupe.desktop" => &["org.gnome.eog.desktop", "eog.desktop"],
        "org.gnome.Showtime.desktop" => &["org.gnome.Totem.desktop", "totem.desktop"],
        "org.gnome.Papers.desktop" => &["org.gnome.Evince.desktop", "evince.desktop"],
        "org.kde.haruna.desktop" => &["org.kde.dragonplayer.desktop"],
        "kitty.desktop" => &["foot.desktop", "alacritty.desktop"],
        _ => &[],
    }
}

// ============================================================================
// Категория → MIME-типы (порт MIME_GROUPS, порядок как в Python)
// ============================================================================

fn mime_group(category: &str) -> &'static [&'static str] {
    match category {
        "file_manager" => &["inode/directory"],
        "text_editor" => &[
            "text/plain", "text/x-csrc", "text/x-python", "text/x-shellscript",
            "text/markdown", "text/css", "text/csv", "text/xml",
            "application/json", "application/xml", "application/x-yaml",
            "application/toml", "application/javascript",
        ],
        "image_viewer" => &[
            "image/jpeg", "image/png", "image/gif", "image/bmp", "image/svg+xml",
            "image/tiff", "image/webp", "image/avif", "image/heif", "image/jxl",
            "image/x-icon", "image/vnd.microsoft.icon",
        ],
        "video_player" => &[
            "video/mp4", "video/x-matroska", "video/webm", "video/x-msvideo",
            "video/mpeg", "video/quicktime", "video/ogg", "video/x-flv",
            "video/3gpp", "video/mp2t", "video/x-ms-wmv",
        ],
        "pdf_viewer" => &["application/pdf", "application/x-pdf"],
        "archive_manager" => &[
            "application/zip", "application/gzip", "application/x-tar",
            "application/x-compressed-tar", "application/x-bzip-compressed-tar",
            "application/x-xz-compressed-tar", "application/x-zstd-compressed-tar",
            "application/x-7z-compressed", "application/vnd.rar",
        ],
        "music_player" => &[
            "audio/mpeg", "audio/ogg", "audio/flac", "audio/x-wav", "audio/mp4",
            "audio/aac", "audio/opus", "audio/x-vorbis+ogg",
        ],
        "browser" => &[
            "x-scheme-handler/http", "x-scheme-handler/https", "text/html",
            "application/xhtml+xml", "x-scheme-handler/about",
            "x-scheme-handler/unknown",
        ],
        _ => &[],
    }
}

// ============================================================================
// Имена per-DE mimeapps.list, XDG-детект, порядок DE
// ============================================================================

/// Порядок DE как в `DE_NATIVE_APPS` после регистрации WM (8 базовых +
/// hyprland/niri/sway через `setdefault`; River и Wayfire — с 24.10). Важен
/// для `get_installed_des` и подстрочного поиска в `detect_current_de`.
const DE_ORDER: &[&str] = &[
    "kde", "gnome", "mate", "cosmic", "cinnamon-mint", "xfce", "budgie",
    "pantheon", "deepin", "lxqt", "hyprland", "niri", "sway", "river", "wayfire",
];

/// Множество «известных» (не-generic) DE — порт `_KNOWN_DE_IDS`.
fn is_known_de(de_id: &str) -> bool {
    DE_ORDER.contains(&de_id)
}

/// Отвечает на вопрос «нет своего набора родных приложений» — порт
/// `_is_tiling_wm`.
///
/// ⚠️ Имя уже́ смысла. Условие «или вообще неизвестный DE» стоит здесь не
/// случайно: для такого окружения у нас всё равно нет своего списка программ,
/// и годится тот же общий набор, что для тайловых. Поэтому предикат отвечает
/// «использовать generic-набор и `<id>-mimeapps.list`», а НЕ «это тайловый
/// композитор».
///
/// Вопрос «тайловый ли композитор» решает другой источник —
/// [`crate::de_customization::de_from_id`] → `De::Wlroots`. Не путать: у них
/// разный состав (здесь любое неизвестное окружение даст `true`).
///
/// ⚠️ Wayfire родных программ тоже не имеет, но он НЕ тайловый и стоит в
/// своей секции «Композиторы с эффектами» (`qt_desktops::TILING_IDS`
/// сверяется с этим предикатом),
/// поэтому здесь его нет. Общий набор программ он получает по другому
/// признаку — «своего набора нет» (`native_apps`), см. `apply_de_specific`.
pub fn is_tiling_wm(de_id: &str) -> bool {
    matches!(de_id, "hyprland" | "niri" | "sway" | "river") || !is_known_de(de_id)
}

/// Родные приложения DE; для тайлового/wlroots WM — общий набор.
/// Порт `_resolve_de_apps`.
pub fn resolve_de_apps(de_id: &str) -> Vec<(&'static str, String)> {
    match native_apps(de_id) {
        Some(t) => t.iter().map(|(k, v)| (*k, v.to_string())).collect(),
        None => wm_app_set(de_id),
    }
}

/// Значение категории в наборе (аналог `apps.get(category)`).
fn app_get<'a>(apps: &'a [(&'static str, String)], category: &str) -> Option<&'a str> {
    apps.iter().find(|(k, _)| *k == category).map(|(_, v)| v.as_str())
}

/// Имя per-DE mimeapps.list — порт `_resolve_mimeapps_filename`.
pub fn resolve_mimeapps_filename(de_id: &str) -> String {
    let known = match de_id {
        "kde" => Some("kde-mimeapps.list"),
        "gnome" => Some("gnome-mimeapps.list"),
        "mate" => Some("mate-mimeapps.list"),
        "cosmic" => Some("cosmic-mimeapps.list"),
        "cinnamon-mint" => Some("x-cinnamon-mimeapps.list"),
        "xfce" => Some("xfce-mimeapps.list"),
        "budgie" => Some("budgie-mimeapps.list"),
        "pantheon" => Some("pantheon-mimeapps.list"),
        // `XDG_CURRENT_DESKTOP=DDE` — файл называется по нему, не по «deepin».
        "deepin" => Some("dde-mimeapps.list"),
        "hyprland" => Some("hyprland-mimeapps.list"),
        "niri" => Some("niri-mimeapps.list"),
        "sway" => Some("sway-mimeapps.list"),
        // `XDG_CURRENT_DESKTOP=river` (стартовый init Rexon) и `wayfire`
        // (Wayfire ставит сам).
        "river" => Some("river-mimeapps.list"),
        "wayfire" => Some("wayfire-mimeapps.list"),
        _ => None,
    };
    // lxqt и неизвестный WM: `{de_id}-mimeapps.list` (dict.get(...) or default).
    known.map(str::to_string).unwrap_or_else(|| format!("{de_id}-mimeapps.list"))
}

/// Определить текущий DE — порт `_detect_current_de`. Вход — значения
/// окружения (`XDG_CURRENT_DESKTOP`, `DESKTOP_SESSION`, наличие
/// `WAYLAND_DISPLAY`) вместо чтения `os.environ` внутри.
///
/// Своей таблицы окружений здесь больше нет — она одна на весь продукт
/// ([`crate::system::detect_current_de`]). Попутно починены два дефекта
/// оригинала: токены сравнивались С УЧЁТОМ РЕГИСТРА (`kde` вместо `KDE` не
/// опознавался), а имя сеанса не знало ни `plasma`, ни `cinnamon` — самые
/// ходовые значения `DESKTOP_SESSION` на Plasma и на Mint давали «не знаю».
pub fn detect_current_de(
    xdg_current_desktop: &str,
    desktop_session: &str,
    has_wayland: bool,
) -> Option<String> {
    crate::system::detect_current_de(xdg_current_desktop, desktop_session, has_wayland)
}

// ============================================================================
// resolve_desktop + короткие имена файловых менеджеров
// ============================================================================

/// Вернуть `desktop_id`, если он существует, иначе первый существующий
/// fallback, иначе None — порт `_resolve_desktop`.
pub fn resolve_desktop(desktop_id: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if exists(desktop_id) {
        return Some(desktop_id.to_string());
    }
    for fb in fallbacks(desktop_id) {
        if exists(fb) {
            return Some((*fb).to_string());
        }
    }
    None
}

/// Короткие имена файловых менеджеров, которые знает Rexon — В ПОРЯДКЕ
/// предпочтения. Один список на программу: из него собирается и разбор
/// `.desktop`-имени, и набор пунктов во вкладке «Настройки» (правило #12 —
/// новый файловый менеджер добавляется в одном месте).
pub const FM_SHORT_NAMES: &[&str] = &[
    "dolphin",
    "nautilus",
    "nemo",
    "thunar",
    "caja",
    "pcmanfm",
    "pcmanfm-qt",
    "cosmic-files",
    "io.elementary.files",
    "dde-file-manager",
];

/// Короткое имя FM (как хранит SettingsManager) → кандидаты `.desktop`.
/// Первый существующий выигрывает. Порт `_FM_DESKTOP_CANDIDATES` (порядок!).
fn fm_desktop_candidates(short: &str) -> Option<&'static [&'static str]> {
    match short {
        "dolphin" => Some(&["org.kde.dolphin.desktop", "dolphin.desktop"]),
        "nautilus" => Some(&["org.gnome.Nautilus.desktop", "nautilus.desktop"]),
        "nemo" => Some(&["nemo.desktop"]),
        "thunar" => Some(&["thunar.desktop", "Thunar.desktop"]),
        "caja" => Some(&["caja.desktop", "caja-browser.desktop"]),
        "pcmanfm" => Some(&["pcmanfm.desktop"]),
        "pcmanfm-qt" => Some(&["pcmanfm-qt.desktop"]),
        "cosmic-files" => {
            Some(&["com.system76.CosmicFiles.desktop", "cosmic-files.desktop"])
        }
        "io.elementary.files" => Some(&["io.elementary.files.desktop"]),
        "dde-file-manager" => Some(&["dde-file-manager.desktop"]),
        _ => None,
    }
}

/// «org.kde.dolphin.desktop» → «dolphin» — порт `_fm_short_name`.
pub fn fm_short_name(fm_id: &str, desktop: &str) -> String {
    if !fm_id.ends_with(".desktop") {
        return fm_id.to_lowercase();
    }
    // Порядок словаря _FM_DESKTOP_CANDIDATES.
    for short in FM_SHORT_NAMES {
        if let Some(cands) = fm_desktop_candidates(short) {
            if cands.contains(&desktop) {
                return short.to_string();
            }
        }
    }
    // os.path.splitext(desktop)[0].split(".")[-1].lower()
    let stem = desktop.strip_suffix(".desktop").unwrap_or(desktop);
    stem.rsplit('.').next().unwrap_or(stem).to_lowercase()
}

/// Режим «auto»: DE → короткое имя родного файлового менеджера —
/// порт `default_file_manager_for_de`. `which` — `shutil.which` (наличие
/// бинаря); ветка перебора фактически недостижима (resolve_de_apps всегда
/// даёт file_manager), сохранена для паритета.
pub fn default_file_manager_for_de(de: Option<&str>, which: &dyn Fn(&str) -> bool) -> String {
    let de_l = de.unwrap_or("").to_lowercase();
    let apps = resolve_de_apps(&de_l);
    if let Some(desktop) = app_get(&apps, "file_manager") {
        return fm_short_name(desktop, desktop);
    }
    for cand in ["dolphin", "thunar", "nautilus", "nemo", "caja", "pcmanfm-qt", "pcmanfm"] {
        if which(cand) {
            return cand.to_string();
        }
    }
    "dolphin".to_string()
}

// ============================================================================
// generate_profile — текст per-DE mimeapps.list
// ============================================================================

/// Результат `generate_profile`. Исполнитель дописывает `filepath =
/// ~/.config/{filename}`: при `Write` пишет `content` и возвращает
/// `(true, filepath)`; `Skip`/`NoApps` — готовые `(true|false, message)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProfileOutcome {
    /// DE не установлен (file_manager не найден) → `(true, message)`.
    Skip { message: String },
    /// Нет доступных приложений → `(false, message)`. Практически недостижимо
    /// (file_manager даёт хотя бы одну ассоциацию), сохранено для паритета.
    NoApps { message: String },
    /// Записать `content` в `~/.config/{filename}`; `count` ассоциаций.
    Write { filename: String, content: String, count: usize },
}

/// Порт `DEAppSwitcher.generate_profile`. `exists` — `_desktop_exists`.
pub fn generate_profile(de_id: &str, exists: &dyn Fn(&str) -> bool) -> ProfileOutcome {
    let apps = resolve_de_apps(de_id);
    let filename = resolve_mimeapps_filename(de_id);

    // Проверка «установлен ли DE»: file_manager DE не найден → пропуск молча.
    if let Some(fm) = app_get(&apps, "file_manager") {
        if !exists(fm) {
            return ProfileOutcome::Skip {
                // TRANSLATORS: %1 is a desktop environment id (kde, gnome…).
                message: trf("%1: not installed, skipping", &[&de_id]),
            };
        }
    }

    // MIME → .desktop (dict Python; итог сортируется по MIME).
    let mut associations: Vec<(&'static str, String)> = Vec::new();
    for (category, desktop_id) in &apps {
        if *category == "terminal" || *category == "screenshot" {
            continue; // терминал/скриншот — не через MIME
        }
        let Some(resolved) = resolve_desktop(desktop_id, exists) else {
            continue;
        };
        for mime in mime_group(category) {
            // dict[mime] = resolved (перезапись при повторе; MIME-группы
            // не пересекаются, так что перезаписей нет).
            if let Some(e) = associations.iter_mut().find(|(m, _)| m == mime) {
                e.1 = resolved.clone();
            } else {
                associations.push((mime, resolved.clone()));
            }
        }
    }
    // Браузер не перезаписываем (Firefox/Chrome кроссплатформенные).

    if associations.is_empty() {
        return ProfileOutcome::NoApps {
            // Имя стола для человека, а не идентификатор («для unknown»).
            message: crate::screenshots::place_de_name(
                // TRANSLATORS: %1 is the name of a desktop environment.
                &tr("No apps available for %1"),
                "%1",
                &crate::screenshots::display_name(de_id),
            ),
        };
    }

    let count = associations.len();
    associations.sort_by(|a, b| a.0.cmp(b.0));
    let mut lines = vec!["[Default Applications]".to_string()];
    for (mime, desktop) in &associations {
        lines.push(format!("{mime}={desktop}"));
    }
    let content = lines.join("\n") + "\n";
    ProfileOutcome::Write { filename, content, count }
}

// ============================================================================
// apply_de_specific — DE-специфичные настройки терминала
// ============================================================================

/// DE-специфичный план: argv команд `safe_run` в порядке вызова, содержимое
/// XFCE `helpers.rc` (если DE = xfce), логи. Порт `apply_de_specific`.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ApplyDeSpecific {
    /// argv каждого `safe_run(..., capture_output=True)`.
    pub commands: Vec<Vec<String>>,
    /// Новое содержимое `~/.config/xfce4/helpers.rc` (пишется исполнителем).
    pub helpers_rc: Option<String>,
    /// Правки чужих конфигов по месту (LXQt: `lxqt-qtxdg.conf`). Пути
    /// начинаются с `~/.config/` — исполнитель раскрывает их тем же
    /// `expand`, что и `helpers.rc` (с учётом `$XDG_CONFIG_HOME`).
    pub file_steps: Vec<crate::defaults::TermStep>,
    pub logs: Vec<String>,
}

/// Команды (`Run`) из общей таблицы терминала [`crate::defaults::terminal_home_steps`].
fn terminal_commands(home: TerminalHome, term: &str) -> Vec<Vec<String>> {
    crate::defaults::terminal_home_steps(home, term, "~/.config")
        .into_iter()
        .filter_map(|s| match s {
            crate::defaults::TermStep::Run { argv } => Some(argv),
            _ => None,
        })
        .collect()
}


/// Порт `apply_de_specific`. `exists`/`which` — `_desktop_exists`/
/// `shutil.which`; `xfce_helpers_existing` — прочитанный существующий
/// `helpers.rc` (None, если файла нет; учитывается только для xfce).
pub fn apply_de_specific(
    de_id: &str,
    exists: &dyn Fn(&str) -> bool,
    which: &dyn Fn(&str) -> bool,
    xfce_helpers_existing: Option<&str>,
) -> ApplyDeSpecific {
    let apps = resolve_de_apps(de_id);
    let terminal = app_get(&apps, "terminal").and_then(|t| resolve_desktop(t, exists));
    let terminal = terminal.as_deref();

    match de_id {
        "kde" => apply_kde(terminal, which),
        "gnome" => apply_gnome(terminal, which),
        "mate" => apply_mate(terminal, which),
        "cinnamon-mint" => apply_cinnamon(terminal, which),
        "xfce" => apply_xfce(&apps, xfce_helpers_existing),
        "lxqt" => apply_lxqt(terminal),
        "budgie" => apply_gnome(terminal, which), // Budgie использует gsettings
        // Pantheon: своего ключа терминала в схемах io.elementary.* нет —
        // родное место то же, что у GNOME.
        "pantheon" => apply_gnome(terminal, which),
        "deepin" => apply_deepin(terminal, which),
        "cosmic" => ApplyDeSpecific {
            logs: vec![tr("ℹ️ COSMIC: default apps via XDG MIME (no gsettings)")],
            ..Default::default()
        },
        // Своего набора программ нет — тайловые, Wayfire и незнакомые.
        _ if native_apps(de_id).is_none() => apply_generic_wm(de_id, terminal),
        _ => ApplyDeSpecific::default(),
    }
}

fn apply_kde(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("kwriteconfig6") {
        out.logs.push(tr("⚠️ kwriteconfig6 not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Kde, term);
        out.logs.push(trf("✅ KDE terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

fn apply_gnome(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Gnome, term);
        out.logs.push(trf("✅ GNOME terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

/// Deepin: терминал — своя схема `com.deepin.desktop.default-applications.terminal`.
fn apply_deepin(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Deepin, term);
        out.logs.push(trf("✅ Deepin terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

fn apply_mate(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Mate, term);
        out.logs.push(trf("✅ MATE terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

fn apply_cinnamon(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Cinnamon, term);
        out.logs.push(trf("✅ Cinnamon terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

/// LXQt: терминал libqtxdg (`lxqt-qtxdg.conf`) — его берут панель, lxqt-runner
/// и «Открыть в терминале» программ на libqtxdg.
fn apply_lxqt(terminal: Option<&str>) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if let Some(term) = terminal {
        out.file_steps = crate::defaults::terminal_home_steps(TerminalHome::Lxqt, term, "~/.config");
        out.logs.push(trf("✅ LXQt terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

/// Имя XFCE-помощника файлового менеджера по `.desktop` — короткое имя
/// программы ([`fm_short_name`]), под которым лежат и помощники системы
/// (`/usr/share/xfce4/helpers/thunar.desktop`).
///
/// Починка 02.10.2026: порт `desktop_to_helper` писал `Thunar` с заглавной,
/// а файл помощника — `thunar.desktop`; на файловой системе с учётом регистра
/// exo его не находил, и вместо Thunar открывалось окно выбора программы.
/// Умолчание самого XFCE (`/etc/xdg/xfce4/helpers.rc`) — `FileManager=thunar`.
fn xfce_helper_name(desktop_id: &str) -> String {
    fm_short_name(desktop_id, desktop_id)
}

/// Шаги «файловый менеджер по умолчанию» в родном месте окружения сверх
/// `xdg-mime` — общие для вкладки «Настройки» и стенда. Сейчас это XFCE:
/// `helpers.rc FileManager=` (его читают `exo-open --launch FileManager`,
/// панель и рабочий стол XFCE), при отсутствии помощника системы — свой
/// `custom-FileManager` (см. [`crate::defaults::xfce_helper_steps`]).
/// `desktop` — `XDG_CURRENT_DESKTOP` или идентификатор Rexon; `cfg` —
/// каталог настроек.
pub fn file_manager_home_steps(
    fm_desktop: &str,
    desktop: &str,
    cfg: &str,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> Vec<crate::defaults::TermStep> {
    if crate::defaults::terminal_home(desktop) != TerminalHome::Xfce {
        return Vec::new();
    }
    let name = xfce_helper_name(fm_desktop);
    let has = xfce_helper_exists(&name);
    crate::defaults::xfce_helper_steps(crate::defaults::XfceHelper::FileManager, &name, cfg, has)
}

fn apply_xfce(apps: &[(&'static str, String)], existing: Option<&str>) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    // Существующие пары k=v (dict — перезапись; итог сортируется).
    let mut lines: Vec<(String, String)> = Vec::new();
    if let Some(text) = existing {
        for line in text.lines() {
            let line = line.trim();
            if line.contains('=') && !line.starts_with('#') {
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim().to_string();
                    let v = v.trim().to_string();
                    if let Some(e) = lines.iter_mut().find(|(ek, _)| *ek == k) {
                        e.1 = v;
                    } else {
                        lines.push((k, v));
                    }
                }
            }
        }
    }
    // Категория → ключ helper'а (порядок Python dict).
    for (cat, helper_key) in [("terminal", "TerminalEmulator"), ("file_manager", "FileManager")] {
        if let Some(desktop_id) = app_get(apps, cat) {
            // Имя помощника терминала — из общей таблицы (то же, что пишет
            // вкладка «Настройки»): у помощников exo имена в нижнем регистре.
            let helper_name = if cat == "terminal" {
                terminal_exec_name(desktop_id)
            } else {
                xfce_helper_name(desktop_id)
            };
            if let Some(e) = lines.iter_mut().find(|(k, _)| k == helper_key) {
                e.1 = helper_name;
            } else {
                lines.push((helper_key.to_string(), helper_name));
            }
        }
    }
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let content: String = lines.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    out.helpers_rc = Some(content);
    out.logs.push(tr("✅ XFCE helpers.rc updated"));
    out
}

fn apply_generic_wm(de_id: &str, terminal: Option<&str>) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if let Some(term) = terminal {
        let term_bin = terminal_exec_name(term);
        out.logs.push(trf(
            // TRANSLATORS: %1 is a window manager id (sway, hyprland…), %2 a terminal program.
            "ℹ️ %1: default terminal is %2 (set it in the window manager's config)",
            &[&de_id, &term_bin],
        ));
    }
    // TRANSLATORS: %1 is a window manager id (sway, hyprland…).
    out.logs.push(trf("ℹ️ %1: default apps via XDG MIME", &[&de_id]));
    out
}

// ============================================================================
// Информация / список установленных DE
// ============================================================================

/// Список DE, для которых найден хотя бы file_manager — порт
/// `get_installed_des` (итерация в порядке `DE_ORDER`).
pub fn installed_des(exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for de_id in DE_ORDER {
        let apps = resolve_de_apps(de_id);
        if let Some(fm) = app_get(&apps, "file_manager") {
            if exists(fm) {
                out.push((*de_id).to_string());
            }
        }
    }
    out
}

/// Таблица `категория → resolved_desktop_id | None` — порт `get_de_apps_info`.
pub fn de_apps_info(
    de_id: &str,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<(String, Option<String>)> {
    let apps = resolve_de_apps(de_id);
    apps.iter()
        .map(|(cat, desktop)| ((*cat).to_string(), resolve_desktop(desktop, exists)))
        .collect()
}

// ============================================================================
// Autostart / apply-скрипт (тексты)
// ============================================================================

/// Программа для `Exec=` автозапуска дефолтов (флаг `--apply-defaults`
/// добавляет [`autostart_desktop_content`]). `None` — запускать нечем.
///
/// ⚠️ Вход выбирает ТА ЖЕ функция, что у ярлыка и лотка
/// ([`crate::installer::launch_entry_live`]), — сюда он приходит готовым.
/// Rust-вход — бинарь окна: он отрабатывает `--apply-defaults` без окна
/// (`cli::run_headless`), и интерпретатор при входе в сеанс не поднимается.
/// Python-вход (откат) — как прежде: `rexon` из `PATH`, иначе сам
/// работающий бинарь.
///
/// ⭐ Починка при переносе: путь уходил в `Exec=` без кавычек, и каталог с
/// пробелом разрывал команду на два аргумента. Теперь любой вход — по
/// правилам Desktop Entry ([`crate::installer::desktop_exec_arg`]).
pub fn autostart_exec_for(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    self_exe: Option<&str>,
) -> Option<String> {
    use crate::installer::desktop_exec_arg;
    // Общее правило — [`crate::app::launch_program`], ОДНО на лоток,
    // заставку входа и эту запись. Своё здесь только последнее звено:
    // сам работающий бинарь Rexon, который и обрабатывает
    // `--apply-defaults`, — посредник для этого маршрута не нужен.
    crate::app::launch_program(entry, rexon_path).or_else(|| {
        self_exe
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(desktop_exec_arg)
    })
}

// TRANSLATORS: Description of the autostart entry that applies Rexon's default applications at login (shown in the desktop's autostart settings).
const AUTOSTART_COMMENT: &str = crate::i18n::n_("Switches default applications for the current desktop environment");

/// Шаблон autostart-`.desktop` — порт `_AUTOSTART_DESKTOP`.
///
/// `Comment` — на всех языках каталога ([`crate::system_l10n::desktop_key`]):
/// прежде он был только русским, без ключа языка, и английский покупатель
/// видел русский.
pub fn autostart_desktop_content(exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rexon App Defaults\n\
         {comment}\
         Exec={exec_path} --apply-defaults\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-Phase=Initialization\n",
        comment = crate::system_l10n::desktop_key("Comment", AUTOSTART_COMMENT),
    )
}

// ============================================================================
// set_file_manager_default (чистая часть) + FileManager1 regex-хелперы
// ============================================================================

pub const FM1_NAME: &str = "org.freedesktop.FileManager1";

/// Выбор целевого `.desktop` для FM по умолчанию — чистая часть
/// `set_file_manager_default` (до subprocess). None → кандидат не найден.
pub fn fm_default_target(fm_id: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let candidates: Vec<String> = if fm_id.ends_with(".desktop") {
        vec![fm_id.to_string()]
    } else {
        match fm_desktop_candidates(&fm_id.to_lowercase()) {
            Some(c) => c.iter().map(|s| s.to_string()).collect(),
            None => vec![format!("{fm_id}.desktop")],
        }
    };
    candidates.into_iter().find(|c| exists(c))
}

/// argv `xdg-mime default <desktop> inode/directory`.
pub fn xdg_mime_default_cmd(desktop: &str) -> Vec<String> {
    vec![
        "xdg-mime".into(), "default".into(), desktop.into(),
        "inode/directory".into(),
    ]
}

/// argv `xdg-mime query default inode/directory`.
pub fn xdg_mime_query_cmd() -> Vec<String> {
    vec![
        "xdg-mime".into(), "query".into(), "default".into(),
        "inode/directory".into(),
    ]
}

/// `re.search(r"^Exec=(\S+)", txt, re.M)` → basename(group(1)). None — нет.
pub fn fm1_exec_basename(txt: &str) -> Option<String> {
    for line in txt.split('\n') {
        if let Some(rest) = line.strip_prefix("Exec=") {
            // \S+ — до первого пробельного символа.
            let val: &str = rest.split_whitespace().next().unwrap_or("");
            if val.is_empty() {
                return None; // \S+ требует хотя бы один символ
            }
            let base = val.rsplit('/').next().unwrap_or(val);
            return Some(base.to_string());
        }
    }
    None
}

/// Условие установки override'а FileManager1: `Name=<FM1>` в тексте И
/// basename(Exec) == binary — порт условия в `_override_filemanager1`.
pub fn fm1_matches(txt: &str, binary: &str) -> bool {
    txt.contains(&format!("Name={FM1_NAME}"))
        && fm1_exec_basename(txt).as_deref() == Some(binary)
}

/// `re.search(r"^PID=(\d+)", stdout, re.M)` → int. Порт разбора busctl.
pub fn busctl_pid(stdout: &str) -> Option<i64> {
    for line in stdout.split('\n') {
        if let Some(rest) = line.strip_prefix("PID=") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
        }
    }
    None
}

/// argv `busctl --user --no-pager status <name>`.
pub fn busctl_status_cmd(name: &str) -> Vec<String> {
    vec![
        "busctl".into(), "--user".into(), "--no-pager".into(), "status".into(),
        name.into(),
    ]
}

/// argv перезагрузки конфигурации шины (`busctl ... ReloadConfig`).
pub fn busctl_reload_cmd() -> Vec<String> {
    vec![
        "busctl".into(), "--user".into(), "call".into(),
        "org.freedesktop.DBus".into(), "/org/freedesktop/DBus".into(),
        "org.freedesktop.DBus".into(), "ReloadConfig".into(),
    ]
}

// ============================================================================
// Слот `org.freedesktop.FileManager1`: кто его держит и как вернуть
// ============================================================================
//
// ЗАЧЕМ ЭТО ЗДЕСЬ. Пункт «Открыть содержащую папку» идёт МИМО mimeapps: у
// приложений это вызов метода `ShowItems` по имени `org.freedesktop.FileManager1`
// на пользовательской шине. Пока имя держит ЖИВОЙ процесс, D-Bus не смотрит на
// `.service`-файлы вовсе — ни на вендорные, ни на наш override в
// `~/.local/share/dbus-1/services`. Поэтому верных mimeapps НЕ ДОСТАТОЧНО:
// правильно настроенная машина всё равно открывает чужой файловый менеджер,
// если тот успел заявить имя первым (нам такой сеанс восстанавливал KDE из
// сохранённой сессии — `Thunar --daemon` из `ksmserverrc`).
//
// Имя заявляют СРАЗУ НЕСКОЛЬКО пакетов (у нас на машине четыре: nautilus,
// dolphin, nemo, Thunar), так что «кто первый — того и папки». Здесь чистая
// часть лечения: опознать владельца, решить, свой он или чужой, и можно ли его
// гасить. Действия — в [`crate::appdefaults_exec`].

/// Как файловый менеджер ведёт себя вокруг имени FileManager1: чем гасится
/// штатно и по каким признакам узнаётся режим службы (без окон).
///
/// Одна таблица на продукт (правило #12): новый менеджер добавляется сюда, а
/// не веткой в потоке исполнителя. Полноту стережёт сверка — каждое имя из
/// [`FM_SHORT_NAMES`] обязано иметь здесь запись.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmDaemonInfo {
    /// Короткое имя менеджера — ключ, общий с [`FM_SHORT_NAMES`].
    pub short: &'static str,
    /// Флаг штатного выхода (`thunar --quit`). Пусто — своего выхода нет,
    /// остаётся мягкий сигнал ровно тому PID, что держит имя.
    pub quit_flag: &'static str,
    /// Признаки режима службы в командной строке процесса.
    pub daemon_flags: &'static [&'static str],
}

/// Таблица «менеджер → как гасится штатно → чем выдаёт режим службы».
pub const FM_DAEMONS: &[FmDaemonInfo] = &[
    FmDaemonInfo { short: "dolphin", quit_flag: "", daemon_flags: &["--daemon"] },
    FmDaemonInfo {
        short: "nautilus",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service"],
    },
    FmDaemonInfo {
        short: "nemo",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service", "--no-default-window"],
    },
    FmDaemonInfo {
        short: "thunar",
        quit_flag: "--quit",
        daemon_flags: &["--daemon", "--gapplication-service"],
    },
    FmDaemonInfo {
        short: "caja",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service", "--no-default-window"],
    },
    FmDaemonInfo { short: "pcmanfm", quit_flag: "--quit", daemon_flags: &["--daemon-mode"] },
    FmDaemonInfo {
        short: "pcmanfm-qt",
        quit_flag: "--quit",
        daemon_flags: &["--daemon-mode"],
    },
    FmDaemonInfo { short: "cosmic-files", quit_flag: "", daemon_flags: &["--daemon"] },
    // Pantheon Files: короткое имя = имя бинаря (по нему процесс и узнаётся);
    // `--quit` — свой ключ программы (src/Application.vala), службой её
    // поднимает D-Bus с `--gapplication-service`.
    FmDaemonInfo {
        short: "io.elementary.files",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service"],
    },
    // Deepin File Manager: своего ключа выхода нет; служба без окон — `-d`
    // (`backendOption`, src/apps/dde-file-manager/commandparser.cpp).
    FmDaemonInfo { short: "dde-file-manager", quit_flag: "", daemon_flags: &["-d"] },
];

/// Запись таблицы по короткому имени.
pub fn fm_daemon_info(short: &str) -> Option<&'static FmDaemonInfo> {
    let lower = short.to_lowercase();
    FM_DAEMONS.iter().find(|d| d.short == lower)
}

/// Последний сегмент пути (`/usr/bin/Thunar` → `Thunar`).
fn path_basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

/// Опознать файловый менеджер по тому, как назвался процесс: `Comm` шины
/// (обрезан ядром до 15 символов) и полный путь `Exe`.
///
/// ⚠️ Регистр здесь принципиален: `/proc/<pid>/comm` у Thunar — `Thunar` с
/// большой буквы, а короткие имена в [`FM_SHORT_NAMES`] строчные. Прежняя
/// сверка владельца сравнивала их как есть и на Thunar не срабатывала
/// никогда — ровно тот дефект, из-за которого чужой демон и жил.
pub fn fm_short_from_process(comm: &str, exe: &str) -> Option<String> {
    for candidate in [path_basename(exe), comm] {
        let name = candidate.trim().to_lowercase();
        if name.is_empty() {
            continue;
        }
        if let Some(hit) = FM_SHORT_NAMES.iter().find(|s| **s == name) {
            return Some((*hit).to_string());
        }
    }
    None
}

/// Что шина рассказала о владельце имени — разбор `busctl status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fm1OwnerFacts {
    pub pid: i64,
    /// `Comm=` (обрезано ядром до 15 символов).
    pub comm: String,
    /// `Exe=` — полный путь бинаря.
    pub exe: String,
    /// `CommandLine=` целиком, как её отдала шина.
    pub cmdline: String,
}

impl Fm1OwnerFacts {
    /// Как назвать владельца человеку.
    pub fn display(&self) -> String {
        if !self.exe.is_empty() {
            return path_basename(&self.exe).to_string();
        }
        if !self.comm.is_empty() {
            return self.comm.clone();
        }
        format!("pid {}", self.pid)
    }
}

/// Разбор вывода `busctl --user status <имя>`: `PID`, `Comm`, `Exe`,
/// `CommandLine`. `None` — владельца в выводе нет (имя никто не держит).
///
/// Заменяет прежний [`busctl_pid`]: одного PID мало — по нему нельзя ни
/// опознать менеджер, ни отличить режим службы от открытых окон.
pub fn busctl_owner(stdout: &str) -> Option<Fm1OwnerFacts> {
    let mut facts = Fm1OwnerFacts::default();
    for line in stdout.split('\n') {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "PID" => {
                let digits: String =
                    value.chars().take_while(|c| c.is_ascii_digit()).collect();
                facts.pid = digits.parse().unwrap_or(0);
            }
            "Comm" => facts.comm = value.to_string(),
            "Exe" => facts.exe = value.to_string(),
            "CommandLine" => facts.cmdline = value.to_string(),
            _ => {}
        }
    }
    (facts.pid > 0).then_some(facts)
}

/// Владелец имени вместе с наблюдением об окнах.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fm1Owner {
    pub facts: Fm1OwnerFacts,
    /// Есть ли у процесса окна на экране. `None` — узнать НЕ УДАЛОСЬ
    /// (сеанс Wayland: списка окон с PID оттуда не добыть). Тогда решение
    /// принимается по командной строке, и по умолчанию — в пользу человека.
    pub has_windows: Option<bool>,
}

/// Приговор слоту `org.freedesktop.FileManager1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fm1Verdict {
    /// Имя никто не держит — его отдаст D-Bus-активация, то есть наш override.
    Free,
    /// Имя держит тот самый менеджер, что назначен умолчанием. Делать нечего.
    Owned { short: String, pid: i64 },
    /// Имя держит не файловый менеджер (или незнакомый нам) — не наше дело.
    Unknown { process: String, pid: i64 },
    /// Чужой менеджер, но с ОТКРЫТЫМИ ОКНАМИ: у человека там работа, гасить
    /// нельзя (правило #13 — защита от дурака работает в обе стороны).
    Busy { short: String, pid: i64 },
    /// Чужой менеджер в режиме службы — слот возвращаем.
    Evict {
        short: String,
        pid: i64,
        /// Чем гасить штатно; пусто — своего выхода у менеджера нет.
        quit: Vec<String>,
    },
}

/// Есть ли в командной строке признак режима службы.
fn cmdline_is_daemon(cmdline: &str, short: &str) -> bool {
    let Some(info) = fm_daemon_info(short) else {
        return false;
    };
    cmdline
        .split_whitespace()
        .any(|token| info.daemon_flags.contains(&token))
}

/// argv штатного выхода: бинарь владельца (а не одноимённый из `PATH`) плюс
/// флаг из таблицы. Пусто — гасить нечем, остаётся сигнал по PID.
pub fn fm_quit_argv(short: &str, exe: &str) -> Vec<String> {
    let Some(info) = fm_daemon_info(short) else {
        return Vec::new();
    };
    if info.quit_flag.is_empty() {
        return Vec::new();
    }
    let binary = if exe.is_empty() { short } else { exe };
    vec![binary.to_string(), info.quit_flag.to_string()]
}

/// Решение по слоту: сравнить владельца имени с назначенным умолчанием.
///
/// `desired_short` — короткое имя менеджера, который Rexon считает
/// умолчанием для `inode/directory`.
///
/// ⛔ Гасим ТОЛЬКО режим службы. Порядок доводов: наблюдение об окнах
/// сильнее командной строки, а когда наблюдения нет вовсе — решает командная
/// строка, и отсутствие флага службы читается как «у человека открыто окно».
pub fn fm1_verdict(owner: Option<&Fm1Owner>, desired_short: &str) -> Fm1Verdict {
    let Some(owner) = owner else {
        return Fm1Verdict::Free;
    };
    let pid = owner.facts.pid;
    let Some(short) = fm_short_from_process(&owner.facts.comm, &owner.facts.exe) else {
        return Fm1Verdict::Unknown { process: owner.facts.display(), pid };
    };
    if short == desired_short.trim().to_lowercase() {
        return Fm1Verdict::Owned { short, pid };
    }
    let daemon = match owner.has_windows {
        Some(true) => false,
        Some(false) => true,
        None => cmdline_is_daemon(&owner.facts.cmdline, &short),
    };
    if !daemon {
        return Fm1Verdict::Busy { short, pid };
    }
    let quit = fm_quit_argv(&short, &owner.facts.exe);
    Fm1Verdict::Evict { short, pid, quit }
}

// ── Сохранённая сессия KDE ───────────────────────────────────────────────

/// Путь файла сохранённой сессии KDE относительно `$XDG_CONFIG_HOME`.
pub const KSMSERVERRC_NAME: &str = "ksmserverrc";

/// Ключ и его порядковый номер: `restartCommand12=…` → `("restartCommand", "12")`.
fn ksmserver_key_index(line: &str) -> Option<(&str, &str)> {
    let key = line.split_once('=')?.0;
    let digits_at = key.len() - key.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits_at == key.len() || digits_at == 0 {
        return None;
    }
    Some((&key[..digits_at], &key[digits_at..]))
}

/// Имя группы из строки `[Имя]`.
fn ini_group(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']')
}

/// Группа сохранённой сессии (`[Session: …]`, `[LegacySession: …]`).
fn is_session_group(group: &str) -> bool {
    group.starts_with("Session:") || group.starts_with("LegacySession:")
}

/// Убрать менеджер из сохранённой сессии KDE: стереть его запись из
/// `[Session: …]` и внести имя в `excludeApps` группы `[General]`.
///
/// ЗАЧЕМ ОБА ДЕЙСТВИЯ. Погашенного демона KDE поднимет обратно при следующем
/// входе — он записан в сохранённой сессии вместе со своей командой запуска
/// (`Thunar,--sm-client-id,…,--daemon`). Стереть запись мало: сессия
/// пересохраняется при выходе, и демон вернётся. `excludeApps` — тот самый
/// штатный список KDE, по которому менеджер сессий не восстанавливает и не
/// сохраняет приложение.
///
/// Номер записи НЕ перенумеровывается, а `count` остаётся прежним: KDE
/// пропускает запись с пустым `program`, а перенумерация сдвинула бы чужие
/// строки — в чужом файле это лишний риск.
///
/// `None` — менять нечего (записи нет и имя уже в исключениях).
pub fn ksmserver_forget(text: &str, short: &str) -> Option<String> {
    let short_l = short.trim().to_lowercase();
    if short_l.is_empty() {
        return None;
    }

    // 1. Найти номера записей этого менеджера в группах сохранённой сессии.
    let mut doomed: Vec<(String, String)> = Vec::new();
    let mut group = String::new();
    let mut has_general = false;
    let mut has_exclude = false;
    for line in text.lines() {
        if let Some(g) = ini_group(line) {
            group = g.to_string();
            has_general |= group == "General";
            continue;
        }
        if group == "General" && line.trim_start().starts_with("excludeApps=") {
            has_exclude = true;
        }
        if !is_session_group(&group) {
            continue;
        }
        let Some((key, index)) = ksmserver_key_index(line) else {
            continue;
        };
        if key != "program" {
            continue;
        }
        let value = line.split_once('=').map(|(_, v)| v).unwrap_or("");
        if path_basename(value.trim()).to_lowercase() == short_l {
            doomed.push((group.clone(), index.to_string()));
        }
    }

    // 2. Переписать: выкинуть строки обречённых записей, дополнить excludeApps.
    let mut out: Vec<String> = Vec::new();
    let mut group = String::new();
    let mut changed = false;
    for line in text.lines() {
        if let Some(g) = ini_group(line) {
            group = g.to_string();
            out.push(line.to_string());
            if group == "General" && !has_exclude {
                out.push(format!("excludeApps={short_l}"));
                changed = true;
            }
            continue;
        }
        if group == "General" {
            if let Some(rest) = line.trim_start().strip_prefix("excludeApps=") {
                let mut apps: Vec<String> = rest
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !apps.contains(&short_l) {
                    apps.push(short_l.clone());
                    changed = true;
                }
                out.push(format!("excludeApps={}", apps.join(",")));
                continue;
            }
        }
        if is_session_group(&group) {
            if let Some((_, index)) = ksmserver_key_index(line) {
                if doomed.iter().any(|(g, i)| *g == group && i == index) {
                    changed = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }

    if !has_general {
        let mut head = vec!["[General]".to_string(), format!("excludeApps={short_l}"), String::new()];
        head.extend(out);
        out = head;
        changed = true;
    }

    if !changed {
        return None;
    }
    let mut text_out = out.join("\n");
    text_out.push('\n');
    Some(text_out)
}
