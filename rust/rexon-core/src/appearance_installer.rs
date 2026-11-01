//! Установщик/применятор тем каталога оформления — порт
//! `rexon/appearance/installer.py` (чистая часть).
//!
//! Поток установки оригинала:
//!   скачать архив → распаковать в temp → определить реальный тип →
//!   скопировать в целевую папку (по cat_id) → apply через ThemeManager →
//!   gtk-update-icon-cache → gtk_sanitizer → журнал.
//!
//! Портируется ДЕТЕРМИНИРОВАННАЯ ЛОГИКА без исполнения:
//!   * определение реального типа распакованной KDE-темы
//!     ([`detect_actual_kde_type`], порт `ocs_store._detect_actual_kde_type`,
//!     который зовёт установщик) — по снимку дерева;
//!   * разбор корня контента / имени темы / имени Plymouth-темы
//!     ([`find_content_root`], [`theme_id_from`], [`plymouth_theme_name`]);
//!   * детект товара, отданного одним голым файлом ([`single_file_accepted`],
//!     правила видов — [`kind_single_file_rule`]) и проверка безопасности
//!     записей архива ([`safe_extract_entry`], anti-traversal);
//!   * маппинги `actual → целевой каталог` / `actual → apply_kind` /
//!     `apply_kind → rexon_type` ([`actual_type_dir`], [`actual_to_kind`],
//!     [`kind_to_rexon_type`]) — привязка к apply_kind, а не к OCS cat_id;
//!   * ПЛАН установки как чистая последовательность шагов общего плана
//!     ([`ExtStep`], тот же, что у `.rxn`):
//!     выбор целевой папки с переклассификацией по реальному типу, копии
//!     `.colors`/каталога, `gtk-update-icon-cache` argv, запись журнала —
//!     тексты сообщений байт-в-байт с Python ([`plan_install`]);
//!   * товар одним файлом ([`plan_single_file`]): вычисление имени файла
//!     (basename → url → запасное имя, гарантия ожидаемого категорией
//!     расширения) и место укладки (каталог вида либо своя подпапка);
//!   * системная тема SDDM/Plymouth ([`plan_root_install`]): валидационные
//!     гейты (PLM / нет pkexec / нет helper / SteamOS), имя темы, `pkexec`
//!     argv, тексты сообщений/кодов возврата;
//!   * применение ([`apply_action_for`] + планы записи конфигов
//!     [`widget_style_plan`]/[`lnf_component_plan`]/[`lock_wallpaper_plan`]/
//!     [`aurorae_plan`], детект устаревшей Global Theme
//!     [`global_theme_missing_metadata_json`], тексты сообщений по apply_kind);
//!   * выбор картинки из пачки обоев ([`pack_images`], [`pick_pack_image`]) —
//!     детерминированное правило под разрешение экрана; звуковая тема
//!     применяется общим движком звука ([`crate::qt_settings::sound_theme_plan`]);
//!   * какие виды вообще имеют «применена сейчас» ([`kind_has_active_state`])
//!     и какие принимают одиночный файл вместо архива
//!     ([`kind_single_file_rule`]);
//!   * удаление ([`plan_uninstall`]): план снятия путей из журнала.
//!
//! ФАЙЛОВАЯ СИСТЕМА — за интерфейсом [`FsSnapshot`] (как `installed_scan.rs`):
//! вход — снимок распакованного дерева в порядке `os.scandir` эталона.
//!
//! НЕ портировано (исполнение — остаётся Python): скачивание, реальная
//! распаковка/копирование (`shutil`), `subprocess`/`pkexec`, реальный apply
//! через `ThemeManager` (планы записей отдаются вызывающему), `gtk_sanitizer`,
//! персистентный журнал (`record_installed`/`load_installed_log` — I/O), сам
//! `tempfile.mkdtemp` (пути даёт исполнитель). Найденные баги оригинала —
//! исправлены прямо здесь (помечено «починен баг оригинала»).

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::categories::category_by_kind;
use crate::ext_manager::ExtStep;
use crate::i18n::{tr, trc, trf};
use crate::installed_scan::FsSnapshot;
use crate::models::{ThemeCategory, ThemeItem};
use crate::privilege::op::{PkgSource, PrivOp};

// ============================================================================
// Константы: расширения и magic-сигнатуры картинок
// ============================================================================

/// Расширения одиночных файлов-обоев — порт `_IMAGE_EXTS`.
pub const IMAGE_EXTS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tif", ".tiff", ".avif",
    ".jxl", ".apng", ".svg",
];

/// Magic-сигнатуры распространённых форматов картинок — порт `_IMAGE_MAGIC`.
pub const IMAGE_MAGIC: &[&[u8]] = &[
    b"\x89PNG\r\n\x1a\n", // PNG
    b"\xff\xd8\xff",      // JPEG
    b"GIF87a",
    b"GIF89a",
    b"BM",       // BMP
    b"II*\x00",  // TIFF LE
    b"MM\x00*",  // TIFF BE
];

/// Расширения цветовой схемы KDE: схема — это ОДИН файл, и Магазин так её и
/// раздаёт (архив вокруг единственного `.colors` кладут не всегда).
pub const COLOR_SCHEME_EXTS: &[&str] = &[".colors"];

/// Расширения шрифтовых файлов: шрифт тоже самодостаточен одним файлом.
pub const FONT_EXTS: &[&str] = &[
    ".ttf", ".otf", ".ttc", ".otc", ".pfb", ".pfa", ".pcf", ".bdf", ".woff",
    ".woff2",
];

/// Расширения темы Kvantum: `.kvconfig` — описание темы целиком.
pub const KVANTUM_EXTS: &[&str] = &[".kvconfig"];

// ============================================================================
// Path-хелперы (семантика os.path)
// ============================================================================

fn join(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

/// `os.path.join(a, b)`: абсолютный `b` (ведущий `/`) заменяет `a`; иначе
/// склейка через `/` (без дублирования, если `a` уже кончается `/`).
fn py_join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.basename` (последний компонент после `/`).
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `str.rstrip("/")`.
fn rstrip_slashes(s: &str) -> &str {
    s.trim_end_matches('/')
}

/// `os.path.splitext` — (root, ext). Ведущие точки basename не считаются
/// разделителем расширения (как в CPython): `.bashrc` → (".bashrc", "").
fn py_splitext(name: &str) -> (String, String) {
    let base = basename(name);
    let prefix_len = name.len() - base.len();
    // индекс последней точки в base
    let Some(dot) = base.rfind('.') else {
        return (name.to_string(), String::new());
    };
    // все символы до точки — точки → нет расширения (leading dots)
    if base[..dot].chars().all(|c| c == '.') {
        return (name.to_string(), String::new());
    }
    let split_at = prefix_len + dot;
    (name[..split_at].to_string(), name[split_at..].to_string())
}

/// Нижний регистр расширения из имени/URL (без query/fragment) — порт
/// `_file_ext`.
pub fn file_ext(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let base = name.split('?').next().unwrap_or(name);
    let base = base.split('#').next().unwrap_or(base);
    py_splitext(base).1.to_lowercase()
}

// ============================================================================
// Маппинги apply_kind / actual-type
// ============================================================================

/// apply_kind → rexon_type для журнала.
///
/// У каждого вида оформления тип СВОЙ. Двух свалок, которые тут были, больше
/// нет: `wallpaper-pack` собирал обои, звуки, шрифты и виджеты в одну кучу
/// (звуковая тема из Магазина ложилась в журнал как пачка обоев), а `plugin`
/// — заставку, экран блокировки, GTK-тему и стиль приложений. По типу из
/// журнала вкладка узнаёт, с какой строкой карты активных тем сверять имя, и
/// на свалке этот вопрос ответа не имел. Wehrwolfmann 2026-08-22.
///
/// `plugin` остаётся дефолтом — но теперь только для того, что и правда вне
/// таблицы видов.
pub fn kind_to_rexon_type(apply_kind: &str) -> &'static str {
    match apply_kind {
        "global" => "theme-kde-global",
        "plasma" => "theme-kde-plasma",
        "colors" => "theme-kde-colors",
        "aurorae" => "theme-kde-aurorae",
        "icons" => "theme-icons",
        "cursor" => "theme-cursors",
        "kvantum" => "theme-kvantum",
        // Обои: и пачка, и одиночная картинка, и фон экрана блокировки —
        // один тип контента, различает их apply_kind записи.
        "wallpapers" | "lockwallpaper" => "wallpaper-pack",
        "wallpaper_plugins" => "kde-wallpaper-plugin",
        "sounds" => "theme-sounds",
        "fonts" => "font-pack",
        "plasmoids" => "theme-plasmoid",
        "gtk" => "theme-gtk",
        "widgetstyle" => "theme-widget-style",
        "splash" => "theme-kde-splash",
        "lockscreen" => "theme-kde-lockscreen",
        "sddm" => "theme-sddm",
        "plymouth" => "theme-plymouth",
        // Разделы окружений (скрипты и эффекты KWin, переключатель окон из
        // магазина): тип — тот же, что у раздела в матрице, второго списка нет.
        k if crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == k) => {
            crate::categories::category_kind(k).unwrap_or("plugin")
        }
        _ => "plugin",
    }
}

/// Виды, у которых есть понятие «применена ПРЯМО СЕЙЧАС», — то есть те, что
/// встречаются ключом в карте активных тем
/// ([`crate::installed_scan::active_map_from_sources`]).
///
/// Вкладка спрашивает это перед тем, как читать конфиги: у шрифтов, виджетов
/// и плагинов обоев активной темы не бывает, и дёргать ради них kreadconfig6
/// незачем. Список живёт рядом с картой, а не списком-двойником в интерфейсе
/// (правило #12): добавили вид в карту — добавили сюда.
pub fn kind_has_active_state(apply_kind: &str) -> bool {
    matches!(
        apply_kind,
        "global"
            | "plasma"
            | "colors"
            | "icons"
            | "cursor"
            | "gtk"
            | "widgetstyle"
            | "splash"
            | "lockscreen"
            | "aurorae"
            | "kvantum"
            | "sddm"
            | "plymouth"
            | "sounds"
            | "wallpapers"
    ) || de_section_has_active_theme(apply_kind)
}

/// Раздел окружения с темой (рамки xfwm4 и Marco, оболочки GNOME, Cinnamon,
/// LXQt, раскладка Alt+Tab): одно значение ключа окружения — «применена
/// сейчас» у него есть, и карта активных тем держит его под ключом раздела
/// (`installed_scan::LiveScan::de_active`). Без этого товар такого раздела,
/// поставленный и применённый из магазина, оставался «✅ Установлено».
/// Дополнения и Conky сюда не входят: у них не одно имя, а список
/// включённого.
fn de_section_has_active_theme(apply_kind: &str) -> bool {
    crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == apply_kind)
        && crate::categories::category_kind(apply_kind)
            .and_then(crate::de_apply::mode)
            .is_some_and(|m| matches!(m, crate::de_apply::Mode::Theme(_)))
}

/// Правило «Магазин отдал не архив, а один голый файл» для вида оформления.
///
/// Обои были не исключением, а первым замеченным случаем: цветовая схема,
/// шрифт и тема Kvantum тоже самодостаточны одним файлом, и часть товаров
/// выложена именно так — без обёртки в zip/tar. Раньше на них срабатывал
/// распаковщик и пользователь получал «Не удалось распаковать архив» на
/// исправном товаре. Wehrwolfmann 2026-08-29.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleFileRule {
    /// Расширения, которые вид согласен принять голым файлом. Первое —
    /// то, чем достраивается имя, если расширения у скачанного нет вовсе.
    pub exts: &'static [&'static str],
    /// Есть ли у содержимого этого вида надёжная подпись в первых байтах:
    /// имя из Магазина иногда пустое, и тогда решает только она. Такая
    /// подпись известна нам лишь у картинок.
    pub has_magic: bool,
    /// Класть файл в подпапку с именем-основой (Kvantum ищет тему как
    /// `<каталог>/<имя>/<имя>.kvconfig`), а не прямо в каталог вида.
    pub own_dir: bool,
    /// Имя темы — основа имени файла. У обоев наоборот: обоя адресуется
    /// файлом целиком, и отрезанное расширение сломало бы путь применения.
    pub name_is_stem: bool,
    /// Из чего собрать имя, когда Магазин не дал ни имени, ни ссылки.
    pub fallback_stem: &'static str,
}

/// Правило одиночного файла для вида — `None`, если вид бывает только архивом
/// (иконки, курсоры, Global Theme и прочее, что физически является деревом).
pub fn kind_single_file_rule(apply_kind: &str) -> Option<SingleFileRule> {
    match apply_kind {
        "wallpapers" | "wallpaper_plugins" | "lockwallpaper" => Some(SingleFileRule {
            exts: IMAGE_EXTS,
            has_magic: true,
            own_dir: false,
            name_is_stem: false,
            fallback_stem: "wallpaper",
        }),
        "colors" => Some(SingleFileRule {
            exts: COLOR_SCHEME_EXTS,
            has_magic: false,
            own_dir: false,
            name_is_stem: true,
            fallback_stem: "color-scheme",
        }),
        "fonts" => Some(SingleFileRule {
            exts: FONT_EXTS,
            has_magic: false,
            own_dir: false,
            name_is_stem: true,
            fallback_stem: "font",
        }),
        "kvantum" => Some(SingleFileRule {
            exts: KVANTUM_EXTS,
            has_magic: false,
            own_dir: true,
            name_is_stem: true,
            fallback_stem: "kvantum",
        }),
        _ => None,
    }
}

/// Реальный тип → apply_kind — порт `_actual_to_kind` (дефолт `"none"`).
pub fn actual_to_kind(actual: &str) -> &'static str {
    match actual {
        "look-and-feel" => "global",
        "plasma-theme" => "plasma",
        "color-scheme" => "colors",
        "aurorae" => "aurorae",
        "kvantum" => "kvantum",
        "icons" => "icons",
        _ => "none",
    }
}

/// Реальный тип → целевой каталог установки — порт `_ACTUAL_TYPE_DIR`
/// (`_dir_for(kind) = category_by_kind(kind).install_dir`). `None`, если тип
/// вне таблицы (не участвует в переклассификации).
pub fn actual_type_dir(home: &Path, actual: &str) -> Option<String> {
    let kind = match actual {
        "look-and-feel" => "global",
        "plasma-theme" => "plasma",
        "color-scheme" => "colors",
        "aurorae" => "aurorae",
        "kvantum" => "kvantum",
        "icons" => "icons",
        _ => return None,
    };
    Some(
        category_by_kind(home, kind)
            .map(|c| c.install_dir)
            .unwrap_or_default(),
    )
}

// ============================================================================
// Определение реального типа KDE-темы — порт _detect_actual_kde_type
// ============================================================================

/// Реальный тип распакованной KDE-темы по содержимому каталога — порт
/// `ocs_store._detect_actual_kde_type` (его зовёт установщик). `None` —
/// определить не удалось.
pub fn detect_actual_kde_type(fs: &FsSnapshot, content_root: &str) -> Option<String> {
    if !fs.is_dir(content_root) {
        return None;
    }
    let files: &[String] = fs.scandir(content_root);
    let is_dir_child = |name: &str| fs.is_dir(&join(content_root, name));

    // Bundle: ≥2 KDE-компонента как топ-уровневые ПАПКИ.
    const BUNDLE_MARKERS: &[&str] = &[
        "aurorae", "color-schemes", "kvantum", "Kvantum", "plasma", "icons",
        "wallpaper", "wallpapers", "sddm",
    ];
    let subdir_marker_hits = files
        .iter()
        .filter(|f| is_dir_child(f) && BUNDLE_MARKERS.contains(&f.as_str()))
        .count();
    if subdir_marker_hits >= 2 {
        return Some("bundle".to_string());
    }

    // Color scheme — .colors в корне.
    if files.iter().any(|f| f.ends_with(".colors")) {
        return Some("color-scheme".to_string());
    }

    // Aurorae — характерные SVG окон в корне.
    const AURORAE_MARKERS: &[&str] = &[
        "decoration.svg", "decoration.svgz", "close.svg", "minimize.svg",
        "maximize.svg",
    ];
    if files.iter().any(|f| AURORAE_MARKERS.contains(&f.as_str())) {
        return Some("aurorae".to_string());
    }

    // Kvantum — .kvconfig.
    if files.iter().any(|f| f.ends_with(".kvconfig")) {
        return Some("kvantum".to_string());
    }

    // Icon theme — index.theme + типичная подпапка иконок.
    if files.iter().any(|f| f == "index.theme") {
        const ICON_SUBDIRS: &[&str] = &[
            "apps", "places", "mimetypes", "actions", "categories", "devices",
            "status",
        ];
        if files
            .iter()
            .any(|f| is_dir_child(f) && ICON_SUBDIRS.contains(&f.as_str()))
        {
            return Some("icons".to_string());
        }
    }

    // Plasma look-and-feel: contents/defaults.
    let contents_dir = join(content_root, "contents");
    if fs.is_dir(&contents_dir) {
        if fs.is_file(&join(&contents_dir, "defaults")) {
            return Some("look-and-feel".to_string());
        }
        const PLASMA_STYLE_SUBDIRS: &[&str] =
            &["widgets", "dialogs", "frames", "icons", "translucent"];
        if fs
            .scandir(&contents_dir)
            .iter()
            .any(|f| PLASMA_STYLE_SUBDIRS.contains(&f.as_str()))
        {
            return Some("plasma-theme".to_string());
        }
    }

    // metadata.desktop с явным ServiceType.
    let metadata = join(content_root, "metadata.desktop");
    if fs.is_file(&metadata) {
        if let Some(content) = fs.read_text(&metadata) {
            if content.contains("Plasma/LookAndFeel") {
                return Some("look-and-feel".to_string());
            }
            if content.contains("Plasma/Theme") {
                return Some("plasma-theme".to_string());
            }
        }
    }

    None
}

// ============================================================================
// Разбор распакованного дерева
// ============================================================================

/// Спуск в единственную верхнюю папку — порт `_find_content_root`.
/// Скрытые (`.`-префикс) элементы игнорируются при подсчёте.
pub fn find_content_root(fs: &FsSnapshot, extract_dir: &str) -> String {
    let entries: Vec<&String> = fs
        .scandir(extract_dir)
        .iter()
        .filter(|e| !e.starts_with('.'))
        .collect();
    if entries.len() == 1 {
        let single = join(extract_dir, entries[0]);
        if fs.is_dir(&single) {
            return single;
        }
    }
    extract_dir.to_string()
}

// ============================================================================
// Корни тем внутри распакованного архива
// ============================================================================
//
// ⛔ Дефект, пойманный живьём 03.10.2026: архив ставился «как есть» — обёртка
// целиком уходила в каталог вида. Пример из Магазина: тема звуков легла как
// `~/.local/share/sounds/zorinos2/sounds/{Zorin,Zorin2}/index.theme`, рядом —
// обои, xml и ReadMe. Тема глубже верхнего уровня не видна ни Rexon, ни KDE,
// ни libcanberra: все они ищут `<каталог вида>/<Имя>/index.theme`. Тот же
// класс у значков, курсоров, тем GTK и Aurorae — архивы авторов часто несут
// несколько вариантов под общей папкой (`Orchis/Orchis-Dark/gtk-3.0`).
//
// Решение одно на все эти виды: найти в распакованном КАЖДЫЙ каталог, который
// сам является темой своего вида, и поставить каждый на верхний уровень
// каталога вида под его собственным именем. Всё остальное из архива не
// копируется.

/// Виды, у которых установка ищет корни тем ([`find_theme_roots`]).
pub fn kind_uses_theme_roots(apply_kind: &str) -> bool {
    matches!(apply_kind, "sounds" | "icons" | "cursor" | "gtk" | "aurorae")
        || de_theme_marker(apply_kind).is_some()
}

/// По чему узнаётся корень темы раздела окружения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeThemeMarker {
    /// Подкаталог в корне темы (`xfwm4/`, `metacity-1/`, `gnome-shell/`,
    /// `cinnamon/`) — строка матрицы с [`crate::de_customization::Payload::ThemeSubdir`].
    Subdir(&'static str),
    /// Файл в корне темы: тема LXQt без `lxqt-panel.qss` панель не меняет
    /// (примечание строки матрицы `theme-lxqt`).
    File(&'static str),
}

/// Файл, без которого каталог темы LXQt — не тема LXQt.
const LXQT_THEME_FILE: &str = "lxqt-panel.qss";

/// Признак корня темы для раздела окружения `apply_kind` (`xfwm4`, `marco`,
/// `gnomeshell`, `cinnamontheme`, `lxqttheme`); `None` — раздел не тема.
///
/// Подкаталог берётся из строки матрицы, второго списка нет (правило #12):
/// у новой строки `ThemeSubdir` раскладка архивов заработает сама.
///
/// ⛔ Дефект, пойманный стендом на настоящих архивах pling 17.10.2026:
/// разделы окружений ставились ветвью «копируем каталог как есть». Архив с
/// несколькими вариантами (`Win8-Metacity`: 16 тем в корне; `ChrButtons/
/// Chr*/xfwm4`; `Marble-shell`; `QTStep`) ложился одной папкой `extracted`
/// или обёрткой, и ни Rexon, ни само окружение темы в ней не видели.
pub fn de_theme_marker(apply_kind: &str) -> Option<DeThemeMarker> {
    if !crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == apply_kind) {
        return None;
    }
    let kind = crate::categories::category_kind(apply_kind)?;
    let row = crate::de_customization::row(kind)?;
    match row.payload {
        crate::de_customization::Payload::ThemeSubdir { sub } => Some(DeThemeMarker::Subdir(sub)),
        _ if kind == "theme-lxqt" => Some(DeThemeMarker::File(LXQT_THEME_FILE)),
        _ => None,
    }
}

/// Каталоги, по которым узнаётся тема GTK (лежат прямо в её корне).
const GTK_THEME_MARKERS: &[&str] = &[
    "gtk-2.0", "gtk-3.0", "gtk-4.0", "gnome-shell", "xfwm4", "metacity-1",
    "cinnamon",
];

/// Есть ли в `index.theme` каталога `dir` раздел `section` (`[Sound Theme]`,
/// `[Icon Theme]`). Сравнение без учёта пробелов вокруг строки.
fn index_theme_has(fs: &FsSnapshot, dir: &str, section: &str) -> bool {
    fs.read_text(&join(dir, "index.theme"))
        .map(|t| t.lines().any(|l| l.trim() == section))
        .unwrap_or(false)
}

/// Является ли каталог `dir` САМ темой вида `apply_kind`:
/// * звуки — `index.theme` с разделом `[Sound Theme]` (спецификация
///   freedesktop sound-theme);
/// * значки и курсоры — `index.theme` с `[Icon Theme]` либо подкаталог
///   `cursors/` (у курсоров `index.theme` бывает не всегда). Оба вида ставятся
///   в один каталог `icons`, поэтому признаки общие: архив «значки + курсоры»
///   встаёт целиком;
/// * GTK — в корне есть `gtk-3.0/`, `gtk-2.0/`, `gnome-shell/` и т.п.;
/// * Aurorae — `decoration.svg(z)` в корне;
/// * разделы окружений — по [`de_theme_marker`]: подкаталог рамок или
///   оболочки, у LXQt — `lxqt-panel.qss`.
pub fn is_theme_root(fs: &FsSnapshot, dir: &str, apply_kind: &str) -> bool {
    let has_child_dir = |name: &str| fs.is_dir(&join(dir, name));
    match apply_kind {
        "sounds" => index_theme_has(fs, dir, "[Sound Theme]"),
        "icons" | "cursor" => index_theme_has(fs, dir, "[Icon Theme]") || has_child_dir("cursors"),
        "gtk" => GTK_THEME_MARKERS.iter().any(|m| has_child_dir(m)),
        "aurorae" => {
            fs.is_file(&join(dir, "decoration.svg")) || fs.is_file(&join(dir, "decoration.svgz"))
        }
        other => match de_theme_marker(other) {
            Some(DeThemeMarker::Subdir(sub)) => has_child_dir(sub),
            Some(DeThemeMarker::File(name)) => fs.is_file(&join(dir, name)),
            None => false,
        },
    }
}

/// Все корни тем вида `apply_kind` под `root` (включая сам `root`), в порядке
/// обхода снимка. В найденный корень обход не спускается: внутри темы другой
/// темы не бывает, а `stereo/` или `cursors/` — её же части. Скрытые
/// каталоги (`.git` и т.п.) пропускаются.
pub fn find_theme_roots(fs: &FsSnapshot, root: &str, apply_kind: &str) -> Vec<String> {
    fn walk(fs: &FsSnapshot, dir: &str, kind: &str, depth: usize, out: &mut Vec<String>) {
        if is_theme_root(fs, dir, kind) {
            out.push(dir.to_string());
            return;
        }
        // Предохранитель от безумной вложенности (и петель в снимке).
        if depth >= 12 {
            return;
        }
        for name in fs.scandir(dir) {
            if name.starts_with('.') {
                continue;
            }
            let child = join(dir, name);
            if fs.is_dir(&child) {
                walk(fs, &child, kind, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if fs.is_dir(root) {
        walk(fs, root, apply_kind, 0, &mut out);
    }
    out
}

/// Все `*.colors` под `root` в порядке обхода снимка: `(путь, имя файла)`.
fn colors_files(fs: &FsSnapshot, root: &str) -> Vec<(String, String)> {
    fn walk(fs: &FsSnapshot, dir: &str, depth: usize, out: &mut Vec<(String, String)>) {
        for name in fs.scandir(dir) {
            let path = join(dir, name);
            if fs.is_dir(&path) {
                if depth < 12 && !name.starts_with('.') {
                    walk(fs, &path, depth + 1, out);
                }
            } else if name.ends_with(".colors") {
                out.push((path, name.clone()));
            }
        }
    }
    let mut out = Vec::new();
    walk(fs, root, 0, &mut out);
    out
}

/// Занят ли путь одиночного файла `dest` чужим: что-то на месте есть, эта
/// покупка его не ставила (`replaceable`), и это не тот же файл байт в
/// байт (тексты `.colors` снимок читает — [`install_tree_snapshot`]).
fn file_taken(fs: &FsSnapshot, replaceable: Option<&[String]>, src: &str, dest: &str) -> bool {
    if !(fs.is_file(dest) || fs.is_dir(dest)) {
        return false;
    }
    if replaceable.is_some_and(|own| own.iter().any(|p| rstrip_slashes(p) == dest)) {
        return false;
    }
    let same = !fs.is_dir(dest)
        && fs.read_text(src).is_some_and(|t| !t.is_empty() && fs.read_text(dest) == Some(t));
    !same
}

/// Почему тема из архива не поставлена (см. [`plan_theme_roots`]).
fn skipped_note(name: &str, reason: SkipReason) -> String {
    match reason {
        SkipReason::DuplicateInArchive => trf(
            "“%1” skipped: the archive contains two themes with this name",
            &[&name],
        ),
        SkipReason::TakenByOther => trf(
            "“%1” skipped: a different theme with this name is already installed",
            &[&name],
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    DuplicateInArchive,
    TakenByOther,
}

/// Сообщение «в архиве нет темы этого вида» — только для звуков: у остальных
/// видов установка без найденных корней идёт прежним путём.
pub fn no_sound_theme_message() -> String {
    tr("The archive contains no sound theme: no index.theme with a [Sound Theme] section was found.")
}

/// Ветка [`plan_install_with`] для видов с корнями тем: каждый корень — на
/// верхний уровень `dest_base` под своим именем.
///
/// `replaceable` — пути, которые эта же покупка ставила раньше (из журнала):
/// их можно снять и поставить заново. `None` — прежнее поведение «занятое
/// имя перезаписывается». С `Some` каталог, занятый ЧУЖОЙ темой (поставленной
/// руками или другой покупкой), не трогается — тема пропускается с честной
/// строкой в сообщении, а не затирает чужое молча.
/// Годится ли строка в имя одного каталога: не пустая, не `.`/`..`, без
/// разделителя пути и нуля.
fn is_single_path_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

fn plan_theme_roots(
    fs: &FsSnapshot,
    roots: &[(String, String)],
    dest_base: &str,
    apply_kind: &str,
    replaceable: Option<&[String]>,
    reclassified: Option<String>,
) -> InstallOutcome {
    let mut steps = vec![ExtStep::MakeDirs { path: dest_base.to_string() }];
    let mut names: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (root, name) in roots {
        let name = name.clone();
        // Имя каталога темы — один компонент пути, иначе `dest` уходит
        // выше каталога вида (`..`) или в него самого (`.`).
        if !is_single_path_component(&name) {
            notes.push(skipped_note(&name, SkipReason::TakenByOther));
            continue;
        }
        if names.contains(&name) {
            notes.push(skipped_note(&name, SkipReason::DuplicateInArchive));
            continue;
        }
        let dest = join(dest_base, &name);
        // Рамка окон (оболочка) с именем уже стоящей темы GTK: ложится
        // подпапкой внутрь неё, как у Mint и Xubuntu (`Arc/xfwm4`).
        if let Some(nest) = plan_nested_part(fs, root, &name, &dest, dest_base, apply_kind, replaceable) {
            match nest {
                Ok((part_steps, part)) => {
                    steps.extend(part_steps);
                    names.push(name);
                    paths.push(part);
                }
                Err(reason) => notes.push(skipped_note(&name, reason)),
            }
            continue;
        }
        let exists = fs.is_dir(&dest) || fs.is_file(&dest);
        if exists {
            if let Some(own) = replaceable {
                if !own.iter().any(|p| rstrip_slashes(p) == dest) {
                    notes.push(skipped_note(&name, SkipReason::TakenByOther));
                    continue;
                }
            }
            steps.push(ExtStep::RemoveTree { path: dest.clone(), safe: false, ignore_errors: true });
        }
        steps.push(ExtStep::CopyTree { src: root.clone(), dest: dest.clone() });
        if apply_kind == "icons" || apply_kind == "cursor" {
            steps.push(ExtStep::Run {
                argv: vec![
                    "gtk-update-icon-cache".into(),
                    "-f".into(),
                    "-t".into(),
                    dest.clone(),
                ],
                timeout_secs: Some(60),
            });
        }
        names.push(name);
        paths.push(dest);
    }
    if names.is_empty() {
        return InstallOutcome::err(notes.join("\n"));
    }
    let mut message = trf("Installed: %1", &[&names.join(", ")]);
    if !notes.is_empty() {
        message = format!("{message}\n{}", notes.join("\n"));
    }
    InstallOutcome {
        ok: true,
        message,
        names,
        steps,
        reclassified,
        record: Some(RecordInfo {
            rexon_type: kind_to_rexon_type(apply_kind).to_string(),
            apply_kind: apply_kind.to_string(),
            paths,
        }),
    }
}

/// Папка темы с именем `name`, в которую ляжет часть раздела окружения
/// (`xfwm4/`, `metacity-1/`, `gnome-shell/`, `cinnamon/`): настоящая папка
/// темы GTK в `~/.local/share/themes` (на неё ведёт наша ссылка в
/// `~/.themes`), иначе сама `~/.themes/<name>`. `None` — папки нет.
fn nest_host(fs: &FsSnapshot, name: &str, dest: &str, dest_base: &str) -> Option<String> {
    if let Some(home) = rstrip_slashes(dest_base).strip_suffix("/.themes") {
        let xdg = join(&crate::installed_scan::xdg_themes_dir(home), name);
        if fs.is_dir(&xdg) {
            return Some(xdg);
        }
    }
    fs.is_dir(dest).then(|| dest.to_string())
}

/// Решение 3в Wehrwolfmann 20.10.2026: рамка окон (Marco — `metacity-1/`,
/// xfwm4 — `xfwm4/`; так же оболочки GNOME и Cinnamon) с тем же именем, что
/// уже стоящая тема GTK, ложится ВНУТРЬ её папки своей подпапкой —
/// стандартное устройство тем freedesktop (Mint-Y, Greybird). Отдельной
/// папкой она лечь не может: имя занято, а `~/.themes/<имя>` — наша ссылка
/// на тему GTK.
///
/// `None` — вкладывать некуда (папки с этим именем нет, раздел не такой или
/// это прежняя отдельная папка этой же покупки): обычный путь. Иначе —
/// шаги и путь части для журнала, либо причина пропуска: подпапка уже есть
/// и она не этой покупки.
fn plan_nested_part(
    fs: &FsSnapshot,
    root: &str,
    name: &str,
    dest: &str,
    dest_base: &str,
    apply_kind: &str,
    replaceable: Option<&[String]>,
) -> Option<Result<(Vec<ExtStep>, String), SkipReason>> {
    let Some(DeThemeMarker::Subdir(sub)) = de_theme_marker(apply_kind) else { return None };
    let own = |p: &str| replaceable.is_some_and(|own| own.iter().any(|o| rstrip_slashes(o) == p));
    if own(dest) {
        return None; // своя отдельная папка — ставится заново целиком
    }
    let host = nest_host(fs, name, dest, dest_base)?;
    let part = join(&host, sub);
    let mut steps = Vec::new();
    if fs.is_dir(&part) || fs.is_file(&part) {
        // Чужая подпапка (своя рамка темы GTK, поставленная руками) не
        // затирается; без журнала (`None`) — прежнее «занятое
        // перезаписывается», но только сама подпапка.
        if replaceable.is_some() && !own(&part) {
            return Some(Err(SkipReason::TakenByOther));
        }
        steps.push(ExtStep::RemoveTree { path: part.clone(), safe: false, ignore_errors: true });
    }
    steps.push(ExtStep::CopyTree { src: join(root, sub), dest: part.clone() });
    Some(Ok((steps, part)))
}

/// Имя темы из metadata.desktop/json, иначе имя папки — порт `_theme_id_from`.
pub fn theme_id_from(fs: &FsSnapshot, content_root: &str) -> String {
    let theme_id = basename(rstrip_slashes(content_root)).to_string();
    let md = join(content_root, "metadata.desktop");
    let mj = join(content_root, "metadata.json");
    if fs.is_file(&md) {
        if let Some(text) = fs.read_text(&md) {
            for line in text.split_inclusive('\n') {
                // Python итерирует строки файла (с '\n'); .strip() убирает его.
                if line.trim().starts_with("X-KDE-PluginInfo-Name=") {
                    // split("=", 1)[1].strip()
                    let val = line.split_once('=').map(|(_, v)| v).unwrap_or("").trim();
                    // Имя из чужого metadata — только как имя одного каталога:
                    // «../..» вывело бы установку из каталога вида.
                    return if is_single_path_component(val) { val.to_string() } else { theme_id };
                }
            }
        }
    } else if fs.is_file(&mj) {
        if let Some(text) = fs.read_text(&mj) {
            if let Ok(Value::Object(meta)) = serde_json::from_str::<Value>(text) {
                if let Some(id) = meta
                    .get("KPlugin")
                    .and_then(Value::as_object)
                    .and_then(|kp| kp.get("Id"))
                    .and_then(Value::as_str)
                    .filter(|id| is_single_path_component(id))
                {
                    return id.to_string();
                }
            }
            // не-объект / нет KPlugin.Id / ошибка разбора → имя папки.
        }
    }
    theme_id
}

/// Имя Plymouth-темы по `<name>.plymouth` в корне — порт `_plymouth_theme_name`
/// (первый по порядку `os.listdir`; `""` — файла нет).
pub fn plymouth_theme_name(fs: &FsSnapshot, content_root: &str) -> String {
    for f in fs.scandir(content_root) {
        if let Some(stem) = f.strip_suffix(".plymouth") {
            return stem.to_string();
        }
    }
    String::new()
}

// ============================================================================
// Детект товара, отданного одним файлом / безопасность распаковки
// ============================================================================

/// True, если скачанный файл — одиночная картинка (не архив).
///
/// Составная часть [`single_file_accepted`]: у картинок, в отличие от прочих
/// одиночных файлов, содержимое опознаётся подписью, и это единственный
/// способ узнать обою, когда Магазин не дал имени файла.
///
/// `is_archive` — результат `zipfile.is_zipfile(path) or tarfile.is_tarfile`
/// (исполнитель вычисляет по реальному файлу; архив имеет приоритет).
/// `head` — первые ≤16 байт файла. `hints` — download_name / download_url.
pub fn looks_like_image(is_archive: bool, head: &[u8], hints: &[&str]) -> bool {
    if is_archive {
        return false;
    }
    for h in hints {
        if IMAGE_EXTS.contains(&file_ext(h).as_str()) {
            return true;
        }
    }
    if IMAGE_MAGIC.iter().any(|sig| head.starts_with(sig)) {
        return true;
    }
    // WEBP: «RIFF»…«WEBP» (байты 0..3 = RIFF, 8..11 = WEBP).
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return true;
    }
    false
}

/// Надо ли ставить скачанное как ОДИН голый файл, а не распаковывать.
///
/// Решение по двум фактам, и оба обязательны:
///   1. распаковать нечем — `is_archive` (реальная проба zip/tar исполнителем)
///      ложно, то есть распаковщик на этом файле только и может, что отказать;
///   2. файл похож на то, чего ждёт сама категория — расширение из её правила
///      ([`kind_single_file_rule`]), а у картинок ещё и подпись содержимого,
///      потому что имя из Магазина бывает пустым.
///
/// Раньше эта развилка существовала только для обоев, и любой другой вид,
/// выложенный голым файлом, упирался в «Не удалось распаковать архив».
///
/// `head` — первые ≤16 байт файла, `hints` — download_name / download_url.
pub fn single_file_accepted(
    apply_kind: &str,
    is_archive: bool,
    head: &[u8],
    hints: &[&str],
) -> bool {
    if is_archive {
        return false;
    }
    let Some(rule) = kind_single_file_rule(apply_kind) else {
        return false;
    };
    if hints
        .iter()
        .any(|h| rule.exts.contains(&file_ext(h).as_str()))
    {
        return true;
    }
    rule.has_magic && looks_like_image(is_archive, head, hints)
}

/// Безопасна ли запись архива относительно каталога распаковки — порт
/// проверки `_safe` из `_safe_extract` (anti path-traversal).
///
/// `dest_abs` — `os.path.abspath(dest)` (каталог распаковки), `name` — имя
/// записи внутри архива. True, если цель == dest_abs либо под `dest_abs/`.
pub fn safe_extract_entry(dest_abs: &str, name: &str) -> bool {
    let target = normpath(&py_join(dest_abs, name));
    target == dest_abs || target.starts_with(&format!("{dest_abs}/"))
}

/// Лексическая нормализация абсолютного пути (аналог `os.path.abspath` без
/// доступа к ФС): схлопывает `.`/`..`/повторные `/`. Ведущий `/` сохраняется.
fn normpath(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), Some(&s) if s != "..") {
                    stack.pop();
                } else if !absolute {
                    stack.push("..");
                }
            }
            other => stack.push(other),
        }
    }
    let joined = stack.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

// ============================================================================
// План установки
// ============================================================================

/// Запись в журнал установленных (что должен зафиксировать исполнитель).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordInfo {
    pub apply_kind: String,
    pub rexon_type: String,
    pub paths: Vec<String>,
}

/// Результат планирования установки — `(ok, message, names)` оригинала плюс
/// план шагов и запись журнала.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallOutcome {
    pub ok: bool,
    pub message: String,
    pub names: Vec<String>,
    pub steps: Vec<ExtStep>,
    /// Переклассификация по реальному типу (`None`, если не было).
    pub reclassified: Option<String>,
    /// Запись журнала (только при успехе).
    pub record: Option<RecordInfo>,
}

impl InstallOutcome {
    fn err(message: impl Into<String>) -> Self {
        InstallOutcome {
            ok: false,
            message: message.into(),
            names: Vec::new(),
            steps: Vec::new(),
            reclassified: None,
            record: None,
        }
    }
}

/// apply_kind'ы, для которых категория точна и переклассификация запрещена.
/// Содержимое видов-контейнеров (обои, звуки, шрифты, виджеты, плагины обоев)
/// похоже на что угодно, и «распознавание реального типа» уводило бы их в
/// чужой каталог. Разделы окружений (скрипты и эффекты KWin, переключатель
/// окон — KPackage с `metadata.json`) тоже точны: их место знает только
/// категория, — их проверяет [`keeps_category`], а не этот список.
const NO_RECLASSIFY: &[&str] = &[
    "colors", "none", "wallpapers", "wallpaper_plugins", "fonts", "sounds",
    "plasmoids", "lockwallpaper", "splash", "lockscreen",
];

/// Ставится ли вид KPackage-ом — решает матрица (`Payload::KPackage`):
/// виджеты Plasma, плагины обоев, скрипты и эффекты KWin, переключатель окон.
pub(crate) fn is_kpackage_kind(apply_kind: &str) -> bool {
    crate::categories::category_kind(apply_kind)
        .and_then(crate::de_customization::row)
        .is_some_and(|r| matches!(r.payload, crate::de_customization::Payload::KPackage { .. }))
}

/// Отказ для товара вида KPackage, который не готовый пакет; `None` —
/// пакет готов (или вид ставится не KPackage-ом). Готов — значит описание
/// пакета и каталог `contents/` в корне: то же требует KNewStuff в Discover.
///
/// Живые замеры магазина 04.10.2026 — без проверки дерево ложилось в
/// каталог вида мусором, а «Включить»/«Добавить виджет» потом молча ничего
/// не находили:
/// * эффекты KWin (719): семь из 46 — исходники на C++ или пакеты pacman;
/// * виджеты (705): из 80 товаров три — исходники на C++ с CMake
///   (`awesome-widgets-4.1.0-src.tar.xz`, `jarvis-main.zip`) и меню
///   Dolphin со скриптом, выложенное в виджеты (`Print-Selected-image`);
/// * плагины обоев (715): из 40 один — пакет pacman
///   (`WallpaperEngine-kde6-…-x86-64.pkg.tar.zst`), ложился каталогом `usr`.
///
/// Причины две, и человеку они говорятся разные: исходники или пакет под
/// одну систему (собирать) — [`not_a_package_message`]; готового пакета нет
/// вовсе — [`no_package_inside_message`].
fn kpackage_refusal(fs: &FsSnapshot, apply_kind: &str, content_root: &str) -> Option<String> {
    if !is_kpackage_kind(apply_kind) {
        return None;
    }
    let described = fs.is_file(&join(content_root, "metadata.json"))
        || fs.is_file(&join(content_root, "metadata.desktop"));
    if described && fs.is_dir(&join(content_root, "contents")) {
        return None;
    }
    Some(if has_build_sources(fs, content_root) {
        not_a_package_message()
    } else {
        no_package_inside_message()
    })
}

/// Исходники, которые надо собирать (CMake, Meson, C++), либо пакет pacman.
///
/// Пакет pacman узнаётся по `.PKGINFO`: он лежит в корне распакованного
/// рядом с `usr/`, а корнем содержимого [`find_content_root`] берёт `usr/`
/// (служебные имена с точкой он пропускает) — поэтому смотрим и на уровень
/// выше. Глубина обхода ограничена: исходники лежат у самого верха архива.
fn has_build_sources(fs: &FsSnapshot, content_root: &str) -> bool {
    let parent = rstrip_slashes(content_root).rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let pacman = fs.is_file(&join(content_root, ".PKGINFO"))
        || (!parent.is_empty() && fs.is_file(&join(parent, ".PKGINFO")));
    if pacman {
        return true;
    }
    fn walk(fs: &FsSnapshot, dir: &str, depth: usize) -> bool {
        fs.scandir(dir).iter().any(|name| {
            let path = join(dir, name);
            if fs.is_dir(&path) {
                depth < 3 && walk(fs, &path, depth + 1)
            } else {
                name == "CMakeLists.txt"
                    || name == "meson.build"
                    || [".cpp", ".cc", ".cxx"].iter().any(|e| name.ends_with(e))
            }
        })
    }
    walk(fs, content_root, 0)
}

/// Подсказка в конце отказа: что делать дальше. Кнопка страницы товара
/// уже есть в карточке ([`crate::appearance_page::detail_open_site_btn`]),
/// а там автор обычно и пишет, как ставить свою работу.
fn author_page_hint() -> String {
    trf(
        "How to install it is usually written on the author's page — the “%1” button in the item's card.",
        &[&crate::appearance_page::detail_open_site_btn()],
    )
}

/// Отказ установки для товара, который не готовый пакет: исходники или
/// пакет под одну систему.
pub fn not_a_package_message() -> String {
    format!(
        "{} {}",
        tr("This isn't a ready-to-use package: the author published source code (or a \
            package built for one particular system), and it has to be built first. \
            Rexon can't install it as is."),
        author_page_hint()
    )
}

/// Отказ установки для товара, в котором готового пакета этого вида нет
/// вовсе (в разделе виджетов выложено что-то другое).
pub fn no_package_inside_message() -> String {
    format!(
        "{} {}",
        tr("There's no ready-to-install package of this kind in this download, so Rexon \
            can't tell what to install from it."),
        author_page_hint()
    )
}

/// Запрещена ли виду переклассификация по содержимому.
fn keeps_category(apply_kind: &str) -> bool {
    NO_RECLASSIFY.contains(&apply_kind)
        || crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == apply_kind)
}

/// Порт «архивной» ветки `ThemeInstaller.install` (после распаковки): выбор
/// целевой папки с переклассификацией, копии, `gtk-update-icon-cache`, запись.
///
/// `home` раскрывает `~` при переклассификации; `cat` — категория по
/// `item.category_id`; `content_root` — корень распакованного (его снимок в
/// `fs`). Гейт requires_root/одиночный файл — снаружи (см.
/// [`plan_root_install`] / [`plan_single_file`]).
pub fn plan_install(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
) -> InstallOutcome {
    plan_install_with(home, fs, cat, content_root, None)
}

/// [`plan_install`] с проверкой занятых имён: `replaceable` — пути, которые
/// ставила эта же покупка (из журнала). Каталог темы, занятый другим, не
/// перезаписывается (см. [`plan_theme_roots`]). Живой установщик зовёт
/// именно эту форму.
pub fn plan_install_with(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
    replaceable: Option<&[String]>,
) -> InstallOutcome {
    plan_install_inner(home, fs, cat, content_root, replaceable, None)
}

/// Откуда взялось распакованное: каталог распаковки и имя темы на случай,
/// когда у темы своей папки в архиве нет.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveOrigin<'a> {
    /// Каталог, куда распакован архив (его имя служебное, `extracted`).
    pub extract_dir: &'a str,
    /// Имя темы, если её корень — сам каталог распаковки
    /// ([`archive_theme_name`]).
    pub fallback_name: &'a str,
}

/// Имя темы для архива без своей папки: имя файла из магазина без
/// расширения архива и без номера-приставки старого gnome-look
/// (`99823-Miu.tar.gz` → `Miu`), иначе название товара.
///
/// ⛔ Дефект стенда 17.10.2026: архив `Rosepine-B-GS-Dark.tar.xz` несёт в
/// корне сразу `gnome-shell/`, а `Marble-shell.zip` — варианты без обёртки.
/// Каталог распаковки называется `extracted`, и тема ложилась как
/// `~/.themes/extracted` (или `~/.themes/gnome-shell`).
pub fn archive_theme_name(download_name: &str, item_name: &str) -> String {
    let file = basename(rstrip_slashes(download_name));
    let lower = file.to_lowercase();
    let mut stem = crate::ocs::ARCHIVE_EXTS
        .iter()
        .find(|e| lower.ends_with(**e))
        .map(|e| &file[..file.len() - e.len()])
        .unwrap_or(file);
    if let Some((num, rest)) = stem.split_once('-') {
        if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) && !rest.trim().is_empty() {
            stem = rest;
        }
    }
    // Проверяется уже обрезанное имя: « ..» после обрезки — «..», и
    // `~/.themes/..` — это сам HOME.
    let ok = |n: &str| {
        let n = n.trim();
        !n.is_empty() && !n.starts_with('.') && !n.contains('/') && !n.contains('\0')
    };
    if ok(stem) {
        return stem.trim().to_string();
    }
    let name = item_name.trim().replace('/', "-");
    if ok(&name) {
        return name;
    }
    "theme".to_string()
}

/// [`plan_install_with`] для архива, распакованного в `extract_dir`: корни
/// тем ищутся от самого каталога распаковки (тема без своей папки — тоже
/// тема), а безымянный корень получает `fallback_name`. Живой установщик
/// зовёт именно эту форму.
pub fn plan_install_archive(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    origin: ArchiveOrigin,
    replaceable: Option<&[String]>,
) -> InstallOutcome {
    let content_root = find_content_root(fs, origin.extract_dir);
    plan_install_inner(home, fs, cat, &content_root, replaceable, Some(origin))
}

fn plan_install_inner(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
    replaceable: Option<&[String]>,
    origin: Option<ArchiveOrigin>,
) -> InstallOutcome {
    // Имя корня `dir`: у каталога распаковки своего имени нет.
    let name_of = |dir: &str, own: String| match origin {
        Some(o) if rstrip_slashes(dir) == rstrip_slashes(o.extract_dir) => o.fallback_name.to_string(),
        _ => own,
    };
    let actual = detect_actual_kde_type(fs, content_root);

    let mut dest_base = cat.install_dir.clone();
    let mut apply_kind = cat.apply_kind.clone();
    let mut reclassified: Option<String> = None;

    if let Some(actual) = actual.as_deref() {
        if !keeps_category(&cat.apply_kind) {
            if let Some(actual_dir) = actual_type_dir(home, actual) {
                // `actual in _ACTUAL_TYPE_DIR` истинно ровно когда
                // actual_type_dir вернул Some.
                if actual_dir != dest_base {
                    dest_base = actual_dir;
                    apply_kind = actual_to_kind(actual).to_string();
                    reclassified = Some(actual.to_string());
                }
            }
        }
    }

    if let Some(refusal) = kpackage_refusal(fs, &apply_kind, content_root) {
        return InstallOutcome::err(refusal);
    }

    let mut steps = vec![ExtStep::MakeDirs { path: dest_base.clone() }];

    // color-scheme: копируем .colors файлы — с ЛЮБОЙ глубины архива (тот же
    // дефект вложенности, что у корней тем: схема в `Pack/colors/X.colors`
    // раньше давала «файл .colors не найден»). Одноимённый второй — пропуск.
    if apply_kind == "colors" {
        let mut names: Vec<String> = Vec::new();
        let mut paths = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (src, f) in colors_files(fs, content_root) {
            if let Some(stem) = f.strip_suffix(".colors") {
                if seen.iter().any(|n| n == stem) {
                    continue;
                }
                seen.push(stem.to_string());
                let dst = join(&dest_base, &f);
                // Правило владельца для файлов (как у `.rxn`): на месте схема,
                // которую эта покупка не ставила, с другим содержимым — схема
                // человека или другой покупки: не перезаписывается.
                if replaceable.is_some() && file_taken(fs, replaceable, &src, &dst) {
                    notes.push(skipped_note(stem, SkipReason::TakenByOther));
                    continue;
                }
                steps.push(ExtStep::CopyFile { src, dest: dst.clone() });
                names.push(stem.to_string());
                paths.push(dst);
            }
        }
        if names.is_empty() {
            if !notes.is_empty() {
                return InstallOutcome::err(notes.join("\n"));
            }
            return InstallOutcome::err(tr("No .colors file found"));
        }
        let mut message = trf("Installed: %1", &[&names.join(", ")]);
        if !notes.is_empty() {
            message = format!("{message}\n{}", notes.join("\n"));
        }
        return InstallOutcome {
            ok: true,
            message,
            names,
            steps,
            reclassified,
            record: Some(RecordInfo {
                rexon_type: kind_to_rexon_type(&apply_kind).to_string(),
                apply_kind,
                paths,
            }),
        };
    }

    // Звуки, значки, курсоры, GTK, Aurorae: каждый корень темы — на верхний
    // уровень каталога вида, обёртка архива и всё лишнее остаются за бортом.
    if kind_uses_theme_roots(&apply_kind) {
        // От каталога распаковки, а не от единственной папки в нём: архив
        // `gnome-shell/…` без обёртки — тоже тема оболочки.
        let search_from = origin.map(|o| o.extract_dir).unwrap_or(content_root);
        let roots = find_theme_roots(fs, search_from, &apply_kind);
        let single_top = roots.len() == 1 && roots[0] == content_root;
        if roots.is_empty() && apply_kind == "sounds" {
            return InstallOutcome::err(no_sound_theme_message());
        }
        // Раздел окружения без единой темы своего вида в архиве (шрифты и
        // обои под видом темы Cinnamon, тема Openbox в разделе LXQt): ставить
        // нечего — каталог-мусор окружение всё равно не покажет.
        if roots.is_empty() && de_theme_marker(&apply_kind).is_some() {
            return InstallOutcome::err(no_package_inside_message());
        }
        if !roots.is_empty() && (!single_top || replaceable.is_some()) {
            let pairs: Vec<(String, String)> = if single_top {
                // Тема в самом корне: имя прежнее (metadata либо папка).
                vec![(content_root.to_string(), name_of(content_root, theme_id_from(fs, content_root)))]
            } else {
                roots
                    .iter()
                    .map(|r| (r.clone(), name_of(r, basename(rstrip_slashes(r)).to_string())))
                    .collect()
            };
            return plan_theme_roots(fs, &pairs, &dest_base, &apply_kind, replaceable, reclassified);
        }
        // Корней не нашлось (значки без index.theme и т.п.) — прежний путь.
    }

    // Пакет тем Conky (`.cmtp.7z` и архивы, разложенные так же): виджеты
    // лежат папками в `.conky/` (`.Conky/`, `.config/conky/`), шрифты — в
    // `.fonts/`. Каждый виджет встаёт своей плиткой.
    if apply_kind == "conky" {
        let search_from = origin.map(|o| o.extract_dir).unwrap_or(content_root);
        let parts = conky_pack_parts(fs, search_from);
        if !parts.is_empty() {
            let pack = origin.map(|o| o.fallback_name).unwrap_or_else(|| basename(rstrip_slashes(content_root)));
            return plan_conky_pack(home, fs, search_from, &parts, &dest_base, pack, replaceable);
        }
    }

    // обои/шрифты и всё прочее — копируем директорию.
    let own_id = theme_id_from(fs, content_root);
    let theme_id = if own_id == basename(rstrip_slashes(content_root)) {
        name_of(content_root, own_id)
    } else {
        own_id
    };
    let dest = join(&dest_base, &theme_id);
    // Правило владельца (как у корней тем и у `.rxn`): снимается только то,
    // что общий журнал записал за этой покупкой. Каталог (файл, ссылка) с
    // тем же именем, поставленный человеком или другой покупкой, не
    // трогается; ставить больше нечего — отказ с причиной, профиль цел.
    // Метку `.rexon-owner` магазин не кладёт: его записи своего — журнал,
    // а файл-метка лёг бы внутрь обоев, шрифтов и чужих для Rexon тем.
    // `None` — прежнее поведение «занятое перезаписывается» (эталон Python).
    let own = |p: &str| replaceable.is_none_or(|own| own.iter().any(|o| rstrip_slashes(o) == p));
    let mut notes: Vec<String> = Vec::new();
    if fs.is_dir(&dest) || fs.is_file(&dest) {
        if !own(&dest) {
            return InstallOutcome::err(skipped_note(&theme_id, SkipReason::TakenByOther));
        }
        steps.push(ExtStep::RemoveTree { path: dest.clone(), safe: false, ignore_errors: true });
    }
    steps.push(ExtStep::CopyTree {
        src: content_root.to_string(),
        dest: dest.clone(),
    });
    let mut paths = vec![dest.clone()];

    // Виджет Conky рисует шрифтами, которые автор положил в архив (`fonts/`,
    // `.fonts/` пакетов менеджера Conky) и просил поставить руками. Они
    // ставятся в СВОЮ папку виджета внутри каталога шрифтов: одноимённый
    // шрифт человека или другого виджета не перезаписывается, а «Удалить»
    // снимает ровно эти файлы (папка записана в журнал рядом с виджетом).
    if apply_kind == "conky" {
        let fonts = conky_archive_fonts(fs, content_root);
        let font_dir = format!("{}/.local/share/fonts/rexon-conky-{theme_id}", home.to_string_lossy());
        // Папка шрифтов с тем же именем от другой покупки (или человека) не
        // снимается и не пополняется: виджет ставится без шрифтов, с
        // заметкой, а чужая папка в журнал этой покупки не попадает.
        let font_dir_taken = (fs.is_dir(&font_dir) || fs.is_file(&font_dir)) && !own(&font_dir);
        if !fonts.is_empty() && font_dir_taken {
            notes.push(skipped_note(basename(&font_dir), SkipReason::TakenByOther));
        } else if !fonts.is_empty() {
            if fs.is_dir(&font_dir) {
                steps.push(ExtStep::RemoveTree { path: font_dir.clone(), safe: false, ignore_errors: true });
            }
            steps.push(ExtStep::MakeDirs { path: font_dir.clone() });
            let mut taken: Vec<String> = Vec::new();
            for src in fonts {
                let name = basename(&src).to_string();
                if taken.contains(&name) {
                    continue;
                }
                steps.push(ExtStep::CopyFile { src, dest: join(&font_dir, &name) });
                taken.push(name);
            }
            steps.push(ExtStep::Run {
                argv: vec!["fc-cache".into(), "-f".into(), font_dir.clone()],
                timeout_secs: Some(60),
            });
            paths.push(font_dir);
        }
    }

    if apply_kind == "icons" || apply_kind == "cursor" {
        steps.push(ExtStep::Run {
            argv: vec![
                "gtk-update-icon-cache".into(),
                "-f".into(),
                "-t".into(),
                dest.clone(),
            ],
            timeout_secs: Some(60),
        });
    }

    let note = match &reclassified {
        Some(r) => format!(" ({})", trf("recognized as %1", &[r])),
        None => String::new(),
    };
    let mut message = trf("Installed: %1", &[&format!("{theme_id}{note}")]);
    if !notes.is_empty() {
        message = format!("{message}\n{}", notes.join("\n"));
    }
    InstallOutcome {
        ok: true,
        message,
        names: vec![theme_id],
        steps,
        reclassified,
        record: Some(RecordInfo {
            rexon_type: kind_to_rexon_type(&apply_kind).to_string(),
            apply_kind,
            paths,
        }),
    }
}

/// Каталоги пакета тем Conky, в которых лежат виджеты: так раскладывает
/// архив программа-менеджер conky (относительно домашней папки), и так же
/// собраны некоторые архивы магазина (`.Conky/conkyx` + `.fonts/`).
pub const CONKY_PACK_DIRS: [&str; 4] = [".conky", ".Conky", ".config/conky", ".config/Conky"];

/// Одна часть пакета тем Conky: папка виджета или голый файл настроек.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConkyPackPart {
    /// Путь в распакованном.
    pub src: String,
    /// Имя виджета (папки в `~/.config/conky/`).
    pub name: String,
    /// Голый файл — ляжет внутрь папки с тем же именем.
    pub loose_file: bool,
}

/// Документ, а не виджет: `README`, `LICENSE`, `*.txt`…
fn is_pack_document(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    ["readme", "license", "licence", "copying", "changelog", "credits", "source"].iter().any(|p| l.starts_with(p))
        || l.ends_with(".txt")
        || l.ends_with(".nfo")
}

/// Части пакета тем Conky в распакованном `root` (сам `root` и одна папка-
/// обёртка в нём). Пусто — это не пакет, а один виджет.
///
/// Папка шрифтов (`fonts`, только шрифты внутри) виджетом не считается: её
/// содержимое ставится шрифтами. Голый файл — виджетом, если по имени он не
/// сценарий, картинка, шрифт или документ (содержимое проверит раздел при
/// показе: снимок установки текстов файлов не держит).
pub fn conky_pack_parts(fs: &FsSnapshot, root: &str) -> Vec<ConkyPackPart> {
    let root = rstrip_slashes(root);
    let mut bases = vec![root.to_string()];
    let wrapper = find_content_root(fs, root);
    if wrapper != root {
        bases.push(wrapper);
    }
    let mut out: Vec<ConkyPackPart> = Vec::new();
    for base in &bases {
        for container in CONKY_PACK_DIRS {
            let dir = join(base, container);
            if !fs.is_dir(&dir) {
                continue;
            }
            let mut names: Vec<&String> = fs.scandir(&dir).iter().collect();
            names.sort();
            for name in names {
                if name.starts_with('.') {
                    continue;
                }
                let src = join(&dir, name);
                if fs.is_dir(&src) {
                    let only_fonts = {
                        let fonts = conky_archive_fonts(fs, &src);
                        !fonts.is_empty() && name.eq_ignore_ascii_case("fonts")
                    };
                    if only_fonts {
                        continue;
                    }
                    out.push(ConkyPackPart { src, name: name.clone(), loose_file: false });
                } else if fs.is_file(&src)
                    && !crate::conky_scan::skipped_name(name)
                    && !crate::constants::is_supported_font(name)
                    && !is_pack_document(name)
                {
                    out.push(ConkyPackPart { src, name: name.clone(), loose_file: true });
                }
            }
        }
    }
    out
}

/// План установки пакета тем Conky: каждая часть — своя папка в
/// `dest_base`, все шрифты пакета — в одну папку
/// `~/.local/share/fonts/rexon-conky-<пакет>/` (её снимет «Удалить» товара).
/// Занятое чужим виджетом имя не затирается — часть пропускается с
/// пояснением, как у тем с несколькими корнями.
fn plan_conky_pack(
    home: &Path,
    fs: &FsSnapshot,
    root: &str,
    parts: &[ConkyPackPart],
    dest_base: &str,
    pack: &str,
    replaceable: Option<&[String]>,
) -> InstallOutcome {
    let mut steps = vec![ExtStep::MakeDirs { path: dest_base.to_string() }];
    let mut names: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for part in parts {
        if !is_single_path_component(&part.name) {
            notes.push(skipped_note(&part.name, SkipReason::TakenByOther));
            continue;
        }
        if names.contains(&part.name) {
            notes.push(skipped_note(&part.name, SkipReason::DuplicateInArchive));
            continue;
        }
        let dest = join(dest_base, &part.name);
        if fs.is_dir(&dest) || fs.is_file(&dest) {
            let own = replaceable.is_none_or(|own| own.iter().any(|p| rstrip_slashes(p) == dest));
            if !own {
                notes.push(skipped_note(&part.name, SkipReason::TakenByOther));
                continue;
            }
            steps.push(ExtStep::RemoveTree { path: dest.clone(), safe: false, ignore_errors: true });
        }
        if part.loose_file {
            steps.push(ExtStep::MakeDirs { path: dest.clone() });
            steps.push(ExtStep::CopyFile { src: part.src.clone(), dest: join(&dest, &part.name) });
        } else {
            steps.push(ExtStep::CopyTree { src: part.src.clone(), dest: dest.clone() });
        }
        names.push(part.name.clone());
        paths.push(dest);
    }
    if names.is_empty() {
        return InstallOutcome::err(notes.join("\n"));
    }
    let fonts = conky_archive_fonts(fs, root);
    let pack = pack.strip_suffix(".cmtp").unwrap_or(pack);
    let pack = if is_single_path_component(pack) { pack } else { names[0].as_str() };
    let font_dir = format!("{}/.local/share/fonts/rexon-conky-{pack}", home.to_string_lossy());
    let font_dir_present = fs.is_dir(&font_dir) || fs.is_file(&font_dir);
    let font_dir_own = replaceable.is_none_or(|own| own.iter().any(|p| rstrip_slashes(p) == font_dir));
    if !fonts.is_empty() && font_dir_present && !font_dir_own {
        // Чужая папка шрифтов с тем же именем (другая покупка, человек) не
        // пополняется и в журнал этой покупки не пишется — прежде шрифты
        // ложились в неё, и «Удалить» этой покупки снимало её целиком.
        notes.push(skipped_note(basename(&font_dir), SkipReason::TakenByOther));
    } else if !fonts.is_empty() {
        if fs.is_dir(&font_dir) {
            steps.push(ExtStep::RemoveTree { path: font_dir.clone(), safe: false, ignore_errors: true });
        }
        steps.push(ExtStep::MakeDirs { path: font_dir.clone() });
        let mut taken: Vec<String> = Vec::new();
        for src in fonts {
            let name = basename(&src).to_string();
            if taken.contains(&name) {
                continue;
            }
            steps.push(ExtStep::CopyFile { src, dest: join(&font_dir, &name) });
            taken.push(name);
        }
        steps.push(ExtStep::Run {
            argv: vec!["fc-cache".into(), "-f".into(), font_dir.clone()],
            timeout_secs: Some(60),
        });
        paths.push(font_dir);
    }
    let mut message = trf("Installed: %1", &[&names.join(", ")]);
    if !notes.is_empty() {
        message = format!("{message}\n{}", notes.join("\n"));
    }
    InstallOutcome {
        ok: true,
        message,
        names,
        steps,
        reclassified: None,
        record: Some(RecordInfo {
            rexon_type: kind_to_rexon_type("conky").to_string(),
            apply_kind: "conky".to_string(),
            paths,
        }),
    }
}

/// Файлы шрифтов в распакованном виджете Conky (до 5 уровней вглубь, по
/// порядку обхода).
fn conky_archive_fonts(fs: &FsSnapshot, root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_string(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let mut names: Vec<&String> = fs.scandir(&dir).iter().collect();
        names.sort();
        for n in names.into_iter().rev() {
            let p = join(&dir, n);
            if fs.is_dir(&p) {
                if depth < 5 {
                    stack.push((p, depth + 1));
                }
            } else if fs.is_file(&p) && crate::constants::is_supported_font(n) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Установка товара, который Магазин отдал ОДНИМ голым файлом (не архивом).
///
/// Распаковывать нечего — файл кладётся в каталог вида (а для Kvantum, где
/// тема ищется как `<имя>/<имя>.kvconfig`, — в подпапку с именем темы).
/// Гейт «а этот ли случай» — [`single_file_accepted`].
///
/// `archive_path` — скачанный файл: он и есть источник копии (явно в шаге,
/// а не «пустой источник = то, что скачано», как было до слияния
/// установщиков).
pub fn plan_single_file(item: &ThemeItem, cat: &ThemeCategory, archive_path: &str) -> InstallOutcome {
    plan_single_file_with(item, cat, archive_path, &|_| false)
}

/// [`plan_single_file`] с правилом владельца: `taken(путь)` — на месте
/// записываемого пути (файла или папки темы) лежит чужое: эта покупка его
/// не ставила, и это не тот же файл байт в байт. Тогда ставить нечего —
/// отказ с причиной, на диске ничего не тронуто. Живой установщик зовёт
/// именно эту форму.
pub fn plan_single_file_with(
    item: &ThemeItem,
    cat: &ThemeCategory,
    archive_path: &str,
    taken: &dyn Fn(&str) -> bool,
) -> InstallOutcome {
    let Some(rule) = kind_single_file_rule(&cat.apply_kind) else {
        return InstallOutcome::err(trf(
            "The “%1” type can't be installed from a single file",
            &[&cat.apply_kind],
        ));
    };
    let fname = single_file_fname(item, cat);
    let stem = basename(&py_splitext(&fname).0).to_string();

    // Каталог, который придётся создать, и путь файла в нём.
    let dest_base = if rule.own_dir {
        join(&cat.install_dir, &stem)
    } else {
        cat.install_dir.clone()
    };
    let dest = join(&dest_base, &fname);
    // В журнал уходит то, что снимет удаление: у собственной папки — папка
    // целиком, иначе сам файл. Оставь тут файл при own_dir — и после
    // удаления в каталоге вида болталась бы пустая папка темы.
    let recorded = if rule.own_dir { dest_base.clone() } else { dest.clone() };
    let name = if rule.name_is_stem { stem } else { fname };
    if taken(&recorded) {
        return InstallOutcome::err(skipped_note(&name, SkipReason::TakenByOther));
    }

    let steps = vec![
        ExtStep::MakeDirs { path: dest_base },
        ExtStep::CopyFile { src: archive_path.to_string(), dest },
    ];
    InstallOutcome {
        ok: true,
        message: trf("Installed: %1", &[&name]),
        names: vec![name],
        steps,
        reclassified: None,
        record: Some(RecordInfo {
            // Починен баг оригинала: вид писался жёстким "none" независимо от
            // категории, и одиночная картинка для фона блокировки ложилась в
            // журнал тем же видом, что обычная обоя.
            rexon_type: kind_to_rexon_type(&cat.apply_kind).to_string(),
            apply_kind: cat.apply_kind.clone(),
            paths: vec![recorded],
        }),
    }
}

/// Имя файла для одиночного товара: basename download_name → basename url →
/// запасное имя вида; расширение достраивается тем, которое ждёт категория.
///
/// Расширение важно не косметически: по нему KDE вообще находит содержимое
/// (`plasma-apply-colorscheme` ищет `.colors`, fontconfig — шрифтовые), и
/// файл без него молча не подхватится.
pub fn single_file_fname(item: &ThemeItem, cat: &ThemeCategory) -> String {
    let rule = kind_single_file_rule(&cat.apply_kind).unwrap_or(SingleFileRule {
        exts: IMAGE_EXTS,
        has_magic: true,
        own_dir: false,
        name_is_stem: false,
        fallback_stem: "wallpaper",
    });
    let mut fname = item.download_name.trim().to_string();
    if fname.is_empty() {
        let tail = item
            .download_url
            .split('?')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        fname = basename(tail).trim().to_string();
    }
    fname = basename(&fname).to_string();
    if !rule.exts.contains(&file_ext(&fname).as_str()) {
        let id_part = if item.id.is_empty() { "kde" } else { &item.id };
        let fallback = format!("{}-{}", rule.fallback_stem, id_part);
        let stem_src = if fname.is_empty() { fallback.clone() } else { fname };
        let stem = py_splitext(&stem_src).0;
        let stem = if stem.is_empty() { fallback } else { stem };
        fname = format!("{stem}{}", rule.exts[0]);
    }
    fname
}

// ============================================================================
// Системная установка (SDDM / Plymouth) — план и гейты
// ============================================================================

/// Окружение системной установки — все внешние probes инъекцией.
pub struct RootEnv<'a> {
    /// Активен ли Plasma Login Manager (plasmalogin) — блокирует SDDM-темы.
    pub is_plasmalogin: bool,
    /// `shutil.which("pkexec")` != None.
    pub has_pkexec: bool,
    /// Путь SYSTEM_HELPER (для текста ошибки).
    pub system_helper_path: &'a str,
    /// Помощник на месте (исполняемый файл по [`crate::installer::system_helper_path`]).
    pub system_helper_exists: bool,
    /// SteamOS / immutable (/usr read-only).
    pub is_steamos_or_immutable: bool,
    /// Именно SteamOS (Steam Deck и Steam Machine) — у неё своя причина
    /// отказа для человека (`distro_compat::SteamOsBlock`).
    pub is_steamos: bool,
    /// Семейство пакетов Debian (→ argv "debian"), иначе "arch".
    pub family_is_debian: bool,
}

/// Системные темы ставит системная часть Rexon, а её нет — отказ до распаковки.
pub fn root_system_part_missing_message() -> String {
    // ⭐ 28.09.2026: совет «переустановите в систему» и обещание «один пароль
    // за сеанс» сняты — системная часть едет и в пакете, а пароль держится
    // минуты, а не сеанс.
    tr("Login screen (SDDM) and boot splash (Plymouth) themes are installed by Rexon's \
        system component, and it isn't installed. Nothing was changed.\nUpdate or \
        reinstall Rexon — the system component comes with it.")
}

/// Гейты `_install_root` ДО распаковки. `Some(outcome)` — ранний отказ с
/// точным текстом; `None` — можно продолжать к [`plan_root_install`].
pub fn root_preflight(env: &RootEnv, cat: &ThemeCategory) -> Option<InstallOutcome> {
    let kind = cat.apply_kind.as_str();
    // ⭐ 28.09.2026: система только для чтения проверяется ПЕРВОЙ. Прежде
    // она шла после «нет системной части Rexon», и на SteamOS, где в версии
    // для Steam этой части нет и быть не может, человек получал совет
    // «переустановите Rexon в систему» — невыполнимый.
    if env.is_steamos {
        let block = if kind == "plymouth" {
            crate::distro_compat::SteamOsBlock::BootSplash
        } else {
            crate::distro_compat::SteamOsBlock::LoginScreen
        };
        return Some(InstallOutcome::err(block.reason()));
    }
    if env.is_steamos_or_immutable {
        return Some(InstallOutcome::err(trf(
            "“%1”: system themes can't be installed on SteamOS or an immutable \
             system (/usr is read-only).",
            &[&cat.name],
        )));
    }
    if kind == "sddm" && env.is_plasmalogin {
        return Some(InstallOutcome::err(tr(
            "Plasma Login Manager (Plasma 6.6+) is active, and it doesn't support \
             SDDM QML themes. Change the login screen look in “System Settings → \
             Login Screen”.",
        )));
    }
    // ⭐ Починка 15.09.2026: прежде «pkexec не найден — … Установите пакет
    // polkit.» — служебное слово и совет, который не срабатывал в Debian и
    // Ubuntu. Текст общий с восстановлением форков, Plymouth и SDDM.
    if !env.has_pkexec {
        return Some(InstallOutcome::err(crate::system_plan::pkexec_missing_message()));
    }
    // ⭐ Починка 15.09.2026: прежде «Системный helper Rexon не установлен
    // (<путь>)…» — служебное слово и путь к файлу в тексте для покупателя.
    // Путь по-прежнему нужен, но только в argv плана ниже.
    if !env.system_helper_exists {
        return Some(InstallOutcome::err(root_system_part_missing_message()));
    }
    None
}

/// Успешный план системной установки (после распаковки и гейтов).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootPlan {
    /// `pkexec <helper> <kind> <theme_id> <content_root> <family>`.
    pub argv: Vec<String>,
    pub timeout_secs: u64,
    pub theme_id: String,
    pub dest: String,
    /// Сообщение при успехе (returncode == 0).
    pub success_message: String,
    pub rexon_type: String,
}

/// Порт `_install_root` после распаковки: имя темы + `pkexec` argv.
/// `Err(outcome)` — Plymouth без `<name>.plymouth`. `content_root` — снимок в `fs`.
/// (`InstallOutcome` в боксе — clippy::result_large_err.)
pub fn plan_root_install(
    env: &RootEnv,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
) -> Result<RootPlan, Box<InstallOutcome>> {
    plan_root_install_with(env, fs, cat, content_root, &|_| false)
}

/// [`plan_root_install`] с правилом владельца: `taken(путь)` — на месте
/// системного каталога темы (`/usr/share/<вид>/themes/<имя>`) лежит чужое:
/// эта покупка его не ставила (тема пакета системы, человека, другой
/// покупки). Помощник копирует поверх (`cp --remove-destination`), поэтому
/// решение принимается здесь, до окна пароля: отказ с причиной, ничего не
/// тронуто. Живой установщик зовёт именно эту форму.
pub fn plan_root_install_with(
    env: &RootEnv,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
    taken: &dyn Fn(&str) -> bool,
) -> Result<RootPlan, Box<InstallOutcome>> {
    let kind = cat.apply_kind.as_str();
    let theme_id = if kind == "plymouth" {
        let id = plymouth_theme_name(fs, content_root);
        if id.is_empty() {
            return Err(Box::new(InstallOutcome::err(tr(
                "No <name>.plymouth file in the archive — this isn't a Plymouth theme.",
            ))));
        }
        id
    } else {
        // SDDM: sanity metadata.desktop/Main.qml — только предупреждение,
        // на план не влияет.
        theme_id_from(fs, content_root)
    };

    let dest = join(&cat.install_dir, &theme_id);
    if taken(&dest) {
        return Err(Box::new(InstallOutcome::err(skipped_note(&theme_id, SkipReason::TakenByOther))));
    }
    let family_arg = if env.family_is_debian { "debian" } else { "arch" };
    let applied_note = if kind == "sddm" {
        tr("Takes effect the next time you log in (SDDM screen).")
    } else {
        tr("Takes effect after a REBOOT (initramfs was rebuilt).")
    };

    Ok(RootPlan {
        argv: [
            vec!["pkexec".to_string(), env.system_helper_path.to_string()],
            // Язык сообщений помощника (решение 30.09.2026), как в ext_manager.
            crate::system_l10n::lang_args(env.system_helper_path),
            vec![
                kind.to_string(),
                theme_id.clone(),
                content_root.to_string(),
                family_arg.to_string(),
            ],
        ]
        .concat(),
        timeout_secs: 900,
        success_message: trf("✅ Installed: %1. %2", &[&theme_id, &applied_note]),
        rexon_type: kind_to_rexon_type(kind).to_string(),
        theme_id,
        dest,
    })
}

/// Сообщение отмены polkit (returncode 126/127) — порт строки `_install_root`.
///
/// ⭐ Починка 15.09.2026: прежде было «Установка отменена (pkexec/polkit)» —
/// служебные слова в скобках. Причина теперь названа теми же словами, что у
/// прочих отказов пароля (`system_plan::pkexec_denied_message`).
pub fn root_cancelled_message() -> String {
    tr("Installation canceled: the administrator password was not confirmed.")
}

/// Сообщение ошибки установки (returncode != 0) — порт строки `_install_root`:
/// последние ≤3 строки stderr через « / », иначе «см. журнал».
pub fn root_error_message(returncode: i32, stderr: &str) -> String {
    let err = stderr.trim();
    let tail: Vec<&str> = if err.is_empty() {
        Vec::new()
    } else {
        let lines: Vec<&str> = err.lines().collect();
        lines[lines.len().saturating_sub(3)..].to_vec()
    };
    let detail = if tail.is_empty() {
        tr("see the log")
    } else {
        tail.join(" / ")
    };
    trf("Installation error (code %1): %2", &[&returncode, &detail])
}

/// «Применить» системной темы — решения и строка вызова; исполнитель и
/// проба каталога — снаружи (сверки подставляют свой «pkexec»).
///
/// `theme_dir_exists` — есть ли каталог `/usr/share/<вид>/themes/<тема>`:
/// выбрать можно только то, что уже стоит, и человек узнаёт об этом до окна
/// пароля. `run` — запуск с таймаутом (у Plymouth пересборка образа ядра
/// идёт минутами).
pub fn system_theme_apply_with(
    env: &RootEnv,
    cat: &ThemeCategory,
    theme: &str,
    lang_args: &[String],
    theme_dir_exists: &dyn Fn(&str) -> bool,
    run: &dyn Fn(&[String], u64) -> crate::utils::RunReport,
) -> (bool, String) {
    if let Some(pre) = root_preflight(env, cat) {
        return (false, pre.message);
    }
    let kind = cat.apply_kind.as_str();
    if !crate::ext_manager::is_safe_sys_name(theme) || !theme_dir_exists(&join(&cat.install_dir, theme)) {
        return (
            false,
            trf("“%1” isn't installed in the system — install it first.", &[&theme]),
        );
    }
    let family = if env.family_is_debian { "debian" } else { "arch" };
    let argv = crate::de_apply::system_apply_argv(env.system_helper_path, lang_args, kind, theme, family);
    let report = run(&argv, 900);
    if report.outcome == RunOutcome::Timeout {
        return (false, tr("Applying timed out (rebuilding initramfs?)"));
    }
    let Some(rc) = report.returncode else {
        return (false, trf("Couldn't apply: %1", &[&report.error]));
    };
    if rc == 126 || rc == 127 {
        return (false, tr("Not applied: the administrator password was not confirmed."));
    }
    if rc != 0 {
        let err = crate::utils::decode_text(&report.stderr);
        let err = err.trim();
        let tail: Vec<&str> = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        let detail = if tail.is_empty() { tr("see the log") } else { tail.join(" / ") };
        return (false, trf("Couldn't apply (code %1): %2", &[&rc, &detail]));
    }
    let note = if kind == "sddm" {
        tr("Takes effect the next time you log in (SDDM screen).")
    } else {
        tr("Takes effect after a REBOOT (initramfs was rebuilt).")
    };
    (true, format!("{}. {note}", trf("✅ Applied: %1", &[&theme])))
}

/// Итог «Применить» у темы SDDM, когда экран входа сейчас показывает другой
/// менеджер (LightDM, GDM, greetd…): «при следующем входе» было бы неправдой —
/// тема выбрана для SDDM и подействует, только если экраном входа станет он.
/// `None` — показывает SDDM или менеджер неизвестен (прежний текст).
/// Plasma Login Manager сюда не доходит: у него свой раздел и отказ заранее
/// ([`root_preflight`]).
pub fn sddm_elsewhere_note(login_unit: &str) -> Option<String> {
    let unit = login_unit.trim();
    let label = crate::categories::login_manager_label(unit);
    (unit != "sddm" && !label.is_empty()).then(|| {
        trf(
            "Right now it's shown by %1, and the themes here are for SDDM: \
             they only take effect if SDDM becomes the login screen.",
            &[&label],
        )
    })
}

/// «Удалить» системной темы — решения и строка вызова; исполнитель, список
/// выбранных сейчас тем (`active`) и запуск — снаружи.
///
/// Выбранную сейчас тему не снимаем: экран входа остался бы без темы, а
/// заставка — без файлов при следующей сборке образа ядра. Человек узнаёт
/// это до окна пароля; помощник от root проверяет то же ещё раз и вдобавок
/// не трогает каталоги, принадлежащие пакетам системы.
pub fn system_theme_remove_with(
    env: &RootEnv,
    cat: &ThemeCategory,
    theme: &str,
    lang_args: &[String],
    active: &[String],
    run: &dyn Fn(&[String], u64) -> crate::utils::RunReport,
) -> (bool, String) {
    let kind = cat.apply_kind.as_str();
    if !crate::ext_manager::is_safe_sys_name(theme) {
        return (false, trf("Couldn't remove “%1”: unexpected theme name.", &[&theme]));
    }
    if active.iter().any(|a| a == theme) {
        let text = if kind == "plymouth" {
            trf(
                "“%1” is the current boot splash theme. Apply another one first, then \
                 remove this one.",
                &[&theme],
            )
        } else {
            trf(
                "“%1” is the current login screen theme. Apply another one first, then \
                 remove this one.",
                &[&theme],
            )
        };
        return (false, text);
    }
    if let Some(pre) = root_preflight(env, cat) {
        return (false, pre.message);
    }
    let family = if env.family_is_debian { "debian" } else { "arch" };
    let argv = crate::de_apply::system_remove_argv(env.system_helper_path, lang_args, kind, theme, family);
    let report = run(&argv, crate::installer::SYSTEM_INSTALL_TIMEOUT_SECS);
    if report.outcome == RunOutcome::Timeout {
        return (false, trf("Couldn't remove “%1”: it took too long.", &[&theme]));
    }
    let Some(rc) = report.returncode else {
        return (false, trf("Couldn't remove: %1", &[&report.error]));
    };
    if rc == 126 || rc == 127 {
        return (false, tr("Not removed: the administrator password was not confirmed."));
    }
    if rc != 0 {
        let err = crate::utils::decode_text(&report.stderr);
        let err = err.trim();
        let tail: Vec<&str> = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        let detail = if tail.is_empty() { tr("see the log") } else { tail.join(" / ") };
        return (false, trf("Couldn't remove (code %1): %2", &[&rc, &detail]));
    }
    (true, String::new())
}

// ============================================================================
// Применение (apply)
// ============================================================================

/// Действие применения для apply_kind категории — порт диспетчера `apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyAction {
    /// Делегирование в ThemeManager (themes.rs планы) по apply_kind.
    KdeGlobal,
    KdePlasma,
    KdeColors,
    Cursor,
    Icons,
    Kvantum,
    Aurorae,
    Gtk,
    WidgetStyle,
    Splash,
    Lockscreen,
    LockWallpaper,
    /// Одиночная картинка → фон рабочего стола (реальные обои).
    WallpaperBackground,
    /// Пачка обоев (каталог с картинками) → фон рабочего стола: картинка
    /// выбирается из пачки детерминированно ([`pick_pack_image`]).
    WallpaperPack,
    /// Звуковая тема → имя темы в настройку звука текущего окружения.
    SoundTheme,
    /// Раздел окружения (рамки xfwm4/Marco, темы оболочек, переключатель
    /// окон, дополнения, Conky): «Применить» / «Включить» / «Запустить» —
    /// [`crate::de_apply::press`] по матрице кастомизации.
    DeSection,
    /// Экран входа (SDDM) или заставка загрузки (Plymouth): выбор уже
    /// установленной темы системным помощником (режим `apply-<вид>`).
    SystemTheme,
    /// apply не требуется (шрифты/виджеты/плагины обоев/архивные плагины).
    Noop,
}

/// Выбор действия применения — порт цепочки `if kind == …` в `apply`.
/// `cat` — категория по `item.category_id` (None → apply_kind "none").
pub fn apply_action_for(cat: Option<&ThemeCategory>, theme_name: &str) -> ApplyAction {
    let kind = cat.map(|c| c.apply_kind.as_str()).unwrap_or("none");
    match kind {
        "global" => ApplyAction::KdeGlobal,
        "plasma" => ApplyAction::KdePlasma,
        "colors" => ApplyAction::KdeColors,
        "cursor" => ApplyAction::Cursor,
        "icons" => ApplyAction::Icons,
        "kvantum" => ApplyAction::Kvantum,
        "aurorae" => ApplyAction::Aurorae,
        "gtk" => ApplyAction::Gtk,
        "widgetstyle" => ApplyAction::WidgetStyle,
        "splash" => ApplyAction::Splash,
        "lockscreen" => ApplyAction::Lockscreen,
        "lockwallpaper" => ApplyAction::LockWallpaper,
        "sounds" => ApplyAction::SoundTheme,
        // Обои: одиночная картинка — прямо фоном, каталог — пачка (картинку
        // из неё выбираем сами). Плагины обоев одиночной картинкой тоже
        // приходят, но каталог у них — kpackage, применять нечего.
        _ if is_wallpaper_image(cat, theme_name) => ApplyAction::WallpaperBackground,
        "wallpapers" => ApplyAction::WallpaperPack,
        // Экран входа и заставка загрузки: установка теперь только кладёт
        // тему в системный каталог, выбирает её «Применить» (04.10.2026).
        "sddm" | "plymouth" => ApplyAction::SystemTheme,
        // Разделы окружений — по матрице: у раздела есть кнопка, если его тип
        // включается ключом окружения, списком или запуском (`de_apply::mode`).
        k if crate::categories::category_kind(k)
            .and_then(crate::de_apply::mode)
            .is_some() =>
        {
            ApplyAction::DeSection
        }
        _ => ApplyAction::Noop,
    }
}

/// True, если apply должен поставить картинку фоном рабочего стола — порт
/// `_is_wallpaper_image`.
pub fn is_wallpaper_image(cat: Option<&ThemeCategory>, theme_name: &str) -> bool {
    let Some(cat) = cat else { return false };
    // Раздел обоев (и обычный, и плагины): там же, где Магазин раздаёт
    // одиночные картинки. Фон блокировки сюда не входит — у него своё
    // действие (`LockWallpaper`).
    if !matches!(cat.apply_kind.as_str(), "wallpapers" | "wallpaper_plugins") {
        return false;
    }
    IMAGE_EXTS.contains(&file_ext(theme_name).as_str())
}

/// Включает ли тип `.rxn` кнопка «Применить» вкладки «Оформление» — то есть
/// есть ли у него раздел, чьё [`apply_action_for`] что-то делает.
///
/// ⛔ Такой тип установка ТОЛЬКО кладёт на диск, а выбор делает человек
/// кнопкой (дефект 03.10.2026: установка темы курсоров из `.rxn` молча
/// писала `~/.icons/default`, и курсор у X11-программ менялся до
/// «Применить»; так же молча применялись цвета, стиль Plasma, глобальная
/// тема, Kvantum, рамки окон, заставка и экран блокировки). Ответ выводится
/// из тех же разделов и того же диспетчера, что у самой кнопки, — второго
/// списка нет (правило #12): появился у раздела «Применить» — установка его
/// типа сама перестаёт включать тему.
pub fn applied_by_button(rxn_kind: &str) -> bool {
    crate::categories::categories(Path::new("/")).iter().any(|cat| {
        crate::categories::category_kind(&cat.key) == Some(rxn_kind)
            && apply_action_for(Some(cat), "") != ApplyAction::Noop
    })
}

/// Сообщение делегирующего apply_kind по результату `ok` — порт строк `apply`
/// (для KdeGlobal/KdePlasma/KdeColors/Cursor/Icons/Kvantum/Aurorae/Gtk/
/// Splash/Lockscreen/Noop). LockWallpaper/WallpaperBackground имеют свои
/// сообщения (см. соответствующие функции), здесь не покрыты.
pub fn apply_message(action: &ApplyAction, ok: bool) -> String {
    match action {
        ApplyAction::KdeGlobal => {
            if ok { tr("Global Theme applied") } else { tr("Couldn't apply the Global Theme") }
        }
        ApplyAction::KdePlasma => {
            if ok { tr("Plasma Style applied") } else { tr("Couldn't apply the Plasma Style") }
        }
        ApplyAction::KdeColors => {
            if ok { tr("Color scheme applied") } else { tr("Couldn't apply the scheme") }
        }
        ApplyAction::Cursor => {
            if ok {
                tr("Cursor applied")
            } else {
                tr("Cursor not applied (the theme may be unsafe)")
            }
        }
        ApplyAction::Icons => {
            if ok { tr("Icons applied") } else { tr("Icons not applied (invalid theme)") }
        }
        ApplyAction::Kvantum => {
            if ok { tr("Kvantum applied") } else { tr("Couldn't apply Kvantum") }
        }
        ApplyAction::Aurorae => {
            if ok { tr("Window decoration applied") } else { tr("Couldn't apply Aurorae") }
        }
        ApplyAction::Gtk => {
            if ok { tr("GTK theme applied") } else { tr("Couldn't apply the GTK theme") }
        }
        ApplyAction::WidgetStyle => {
            if ok {
                tr("Application style applied")
            } else {
                tr("Couldn't apply the application style")
            }
        }
        ApplyAction::Splash => {
            if ok {
                tr("Splash screen selected — takes effect the next time you log in")
            } else {
                tr("Couldn't apply the splash screen")
            }
        }
        ApplyAction::Lockscreen => {
            if ok {
                tr("Lock screen selected — takes effect the next time the screen locks")
            } else {
                tr("Couldn't apply the lock screen")
            }
        }
        ApplyAction::SoundTheme => {
            if ok {
                tr("Sound theme applied")
            } else {
                tr("Couldn't apply the sound theme")
            }
        }
        ApplyAction::Noop => tr("Installed (this type doesn't need to be applied)"),
        ApplyAction::LockWallpaper
        | ApplyAction::WallpaperBackground
        | ApplyAction::WallpaperPack
        | ApplyAction::DeSection
        | ApplyAction::SystemTheme => String::new(),
    }
}

/// Сообщение о работе gtk-санитайзера после apply — порт строки
/// `f"🛡 GTK-санитайзер: исправлено {n} ссылок"` (n = число правок, >0).
pub fn gtk_sanitizer_message(changes: usize) -> String {
    trf("🛡️ GTK sanitizer: fixed %1 links", &[&changes])
}

// ── apply: выбор картинки из пачки обоев ────────────────────────────────────

/// Штатная подпапка картинок пачки обоев KDE (KPackage).
const PACK_IMAGES_SUBDIR: &str = "contents/images";

/// Все картинки пачки обоев в порядке снимка.
///
/// Если у пачки есть штатная подпапка `contents/images` (раскладка KPackage,
/// которой пользуются обои KDE Store) — берём только её: рядом лежат
/// логотипы, превью и картинки-заглушки, и подмешивать их к выбору нельзя.
/// Иначе обходим всё дерево пачки. Своё превью, которое Rexon кладёт рядом с
/// темой при установке из Магазина, из кандидатов исключается — иначе фоном
/// поехал бы скриншот карточки.
pub fn pack_images(fs: &FsSnapshot, pack_dir: &str) -> Vec<String> {
    let pack_dir = rstrip_slashes(pack_dir);
    let standard = join(pack_dir, PACK_IMAGES_SUBDIR);
    let root = if fs.is_dir(&standard) { standard } else { pack_dir.to_string() };
    let mut out = Vec::new();
    collect_images(fs, &root, &mut out);
    out
}

/// Рекурсивный сбор картинок (в порядке снимка каталога, вглубь после файлов
/// того же уровня — порядок обхода на выбор не влияет, он полностью
/// определяется ключом сортировки [`pick_pack_image`]).
fn collect_images(fs: &FsSnapshot, dir: &str, out: &mut Vec<String>) {
    let mut subdirs = Vec::new();
    for name in fs.scandir(dir) {
        let path = join(dir, name);
        if fs.is_dir(&path) {
            subdirs.push(path);
            continue;
        }
        if name == crate::installed_scan::REXON_PREVIEW_NAME {
            continue;
        }
        if IMAGE_EXTS.contains(&file_ext(name).as_str()) {
            out.push(path);
        }
    }
    for d in subdirs {
        collect_images(fs, &d, out);
    }
}

/// Разрешение из имени файла вида `1920x1080.png` (штатное имя картинок в
/// пачках KDE). `None` — в имени разрешения нет.
fn size_from_name(path: &str) -> Option<(u64, u64)> {
    let stem = py_splitext(basename(path)).0;
    let stem = basename(&stem).to_string();
    let (w, h) = stem.split_once(['x', 'X'])?;
    if w.is_empty() || h.is_empty() {
        return None;
    }
    let w: u64 = w.parse().ok()?;
    let h: u64 = h.parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// Ключ «естественной» сортировки имени: числа сравниваются как числа
/// (`10.png` после `9.png`, а не перед ним).
fn natural_key(name: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut chars = name.chars().peekable();
    let mut text = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut num = String::new();
            while let Some(&d) = chars.peek() {
                if !d.is_ascii_digit() {
                    break;
                }
                num.push(d);
                chars.next();
            }
            // Числа длиннее u64 берём хвостом — на именах картинок не бывает,
            // а падать из-за такого имени нельзя.
            out.push((num.parse::<u64>().unwrap_or(u64::MAX), std::mem::take(&mut text)));
        } else {
            text.push(c);
            chars.next();
        }
    }
    if !text.is_empty() {
        out.push((0, text));
    }
    out
}

/// Какую картинку пачки ставить фоном — ДЕТЕРМИНИРОВАННОЕ правило.
///
/// Порядок ключей (первый решающий):
///   1. **есть ли разрешение в имени** — файлы `ШxВ.ext` (штатная раскладка
///      пачек KDE) идут раньше безымянных по размеру: про них мы точно знаем,
///      подойдут ли они экрану;
///   2. **разница пропорций с экраном** — картинка не того соотношения сторон
///      либо обрежется, либо ляжет с полями, и это видно сразу;
///   3. **разница площади с экраном** — из двух одинаковых по пропорции берём
///      ту, что ближе к разрешению экрана: масштабирование в любую сторону
///      портит картинку, а «ближайшая» портится меньше всех;
///   4. **естественная сортировка имени** — снимает остаток неоднозначности,
///      чтобы одна и та же пачка на одной и той же машине всегда давала одну
///      и ту же картинку.
///
/// Экран нулевого размера (спросить не у кого) — пропорция и площадь
/// сравниваться не могут, остаётся сортировка имени.
pub fn pick_pack_image(files: &[String], screen_w: u64, screen_h: u64) -> Option<String> {
    let known_screen = screen_w > 0 && screen_h > 0;
    let screen_ar = if known_screen {
        screen_w as f64 / screen_h as f64
    } else {
        0.0
    };
    let screen_area = (screen_w * screen_h) as f64;
    let mut best: Option<(PickKey, &String)> = None;
    for f in files {
        let size = size_from_name(f);
        let (ar_diff, area_diff) = match (size, known_screen) {
            (Some((w, h)), true) => (
                (w as f64 / h as f64 - screen_ar).abs(),
                ((w * h) as f64 - screen_area).abs(),
            ),
            _ => (0.0, 0.0),
        };
        let key = PickKey {
            no_size: size.is_none(),
            ar_diff,
            area_diff,
            name: natural_key(basename(f)),
        };
        if best.as_ref().is_none_or(|(b, _)| key.better_than(b)) {
            best = Some((key, f));
        }
    }
    best.map(|(_, f)| f.clone())
}

/// Ключ выбора картинки пачки: чем меньше по порядку полей, тем лучше.
struct PickKey {
    no_size: bool,
    ar_diff: f64,
    area_diff: f64,
    name: Vec<(u64, String)>,
}

impl PickKey {
    fn better_than(&self, other: &PickKey) -> bool {
        let a = (self.no_size, self.ar_diff, self.area_diff);
        let b = (other.no_size, other.ar_diff, other.area_diff);
        a < b || (a == b && self.name < other.name)
    }
}

/// Сообщение, когда в пачке нет ни одной картинки: ставить нечего, но
/// установка состоялась — карточка честно остаётся на «✓ Установлено».
pub fn pack_no_images_message() -> String {
    tr("Installed. This pack has no images — nothing to apply.")
}

// ── apply: планы записи конфигов (kwriteconfig6 / dbus) ─────────────────────

/// Одна команда плана применения (argv + опциональный таймаут).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CfgCmd {
    pub argv: Vec<String>,
}

fn cfg(parts: &[&str]) -> CfgCmd {
    CfgCmd { argv: parts.iter().map(|s| (*s).to_string()).collect() }
}

/// Порт `_apply_widget_style` — форма плана для KDE (`kwriteconfig6` +
/// dbus-уведомление при успехе). Осталась для моста и эталона Python;
/// исполнитель идёт через [`crate::themes::apply_qt_style_plan`], который
/// выбирает запись по окружению, а для KDE даёт ровно эти же две команды.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WidgetStylePlan {
    pub primary: CfgCmd,
    pub notify_if_ok: CfgCmd,
}

pub fn widget_style_plan(name: &str) -> WidgetStylePlan {
    let plan = crate::themes::apply_qt_style_plan(
        name,
        &crate::themes::KvantumEnv { de_id: "kde", home: "", xdg_config_home: "", platform_theme: "", icon_theme: String::new() },
    );
    let argv = |i: usize| match &plan.steps[i] {
        ApplyStep::Primary(c) | ApplyStep::IfOk(c) => CfgCmd { argv: c.argv.clone() },
        other => unreachable!("KDE-путь стиля Qt — только команды, а не {other:?}"),
    };
    WidgetStylePlan { primary: argv(0), notify_if_ok: argv(1) }
}

/// Порт `_apply_lnf_component`: запись имени темы в ksplashrc[KSplash]Theme
/// (`component == "splash"`) или kscreenlockerrc[Greeter]Theme (иначе).
pub fn lnf_component_plan(component: &str, name: &str) -> CfgCmd {
    if component == "splash" {
        cfg(&[
            "kwriteconfig6", "--file", "ksplashrc", "--group", "KSplash",
            "--key", "Theme", name,
        ])
    } else {
        cfg(&[
            "kwriteconfig6", "--file", "kscreenlockerrc", "--group", "Greeter",
            "--key", "Theme", name,
        ])
    }
}

// ── Заставка KDE: «Нет» и «Продемонстрировать» ───────────────────────────
//
// Сверено с исходниками Plasma 6.8 (plasma-desktop `kcms/ksplash/kcm.cpp`,
// plasma-workspace `startkde/startplasma.cpp`, `ksplash/ksplashqml`):
//   * плитка «Нет» у KDE — тема с именем `None`; при сохранении модуль пишет
//     ОБА ключа: `[KSplash] Theme=None` и `Engine=none` (у любой другой темы —
//     `Engine=KSplashQML`). При входе startplasma запускает ksplashqml только
//     при `Engine == KSplashQML` (нет ключа — тоже KSplashQML);
//   * «Продемонстрировать» — `ksplashqml <тема> --test`: заставка во весь
//     экран проходит этапы и закрывается сама, по щелчку или Esc (флага
//     `--window` модуль не передаёт).

/// Имя «темы» плитки «Нет» — то же, что пишет модуль KDE.
pub const SPLASH_NONE: &str = "None";
/// `Engine` выключенной заставки — как у модуля KDE.
pub const SPLASH_ENGINE_NONE: &str = "none";

/// Значение `[KSplash] Engine` для выбранной темы.
pub fn splash_engine_for(theme: &str) -> &'static str {
    if theme == SPLASH_NONE {
        SPLASH_ENGINE_NONE
    } else {
        crate::session_splash::KSPLASH_ENGINE
    }
}

/// Запись `[KSplash] Engine` к выбранной теме. Без неё после «Нет» никакая
/// другая заставка не вернулась бы: startplasma смотрит на `Engine`.
pub fn splash_engine_plan(theme: &str) -> CfgCmd {
    cfg(&[
        "kwriteconfig6", "--file", "ksplashrc", "--group", "KSplash",
        "--key", "Engine", splash_engine_for(theme),
    ])
}

/// Какую плитку раздела «Заставка» считать текущей по `ksplashrc`.
///
/// `Engine` задан и не `KSplashQML` — заставка при входе не запускается
/// вовсе (так решает startplasma), текущая — «Нет», что бы ни стояло в
/// `Theme`. Иначе — значение `Theme` как есть.
pub fn splash_active_value(theme: Option<String>, engine: Option<&str>) -> Option<String> {
    match engine.map(str::trim) {
        Some(e) if !e.is_empty() && e != crate::session_splash::KSPLASH_ENGINE => {
            Some(SPLASH_NONE.to_string())
        }
        _ => theme,
    }
}

/// Команда «Продемонстрировать заставку» — та же, что у модуля KDE.
/// `None` — показывать нечего (плитка «Нет», пустое или странное имя).
pub fn splash_demo_argv(theme: &str) -> Option<Vec<String>> {
    let theme = theme.trim();
    if theme.is_empty() || theme == SPLASH_NONE || theme.starts_with('-') || theme.contains('/') {
        return None;
    }
    Some(vec![
        crate::session_splash::KSPLASH_PROCESS.to_string(),
        theme.to_string(),
        "--test".to_string(),
    ])
}

/// Подпись кнопки показа заставки на плитке.
pub fn splash_demo_label() -> String {
    // TRANSLATORS: button on a KDE splash screen tile: shows the splash screen full screen for a few seconds.
    tr("Show Splash Screen")
}

/// Подсказка кнопки показа заставки.
pub fn splash_demo_tooltip() -> String {
    tr("Shows the splash screen full screen for a few seconds. Click or press Esc to close it.")
}

/// Показ не удался; `detail` — что сказала программа (может быть пусто).
pub fn splash_demo_failed(detail: &str) -> String {
    let head = tr("Failed to show the splash screen preview.");
    // Программа бывает многословна: человеку — последние строки, где причина.
    let lines: Vec<&str> = detail.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        head
    } else {
        format!("{head}\n{}", lines[lines.len().saturating_sub(5)..].join("\n"))
    }
}

/// Нечем показать: нет `ksplashqml` (заставка KDE стоит вместе с Plasma).
pub fn splash_demo_missing() -> String {
    tr("Can't show the splash screen: ksplashqml (part of Plasma) isn't installed.")
}

/// Название плитки «Нет».
pub fn splash_none_name() -> String {
    // TRANSLATORS: tile in the KDE splash screen section that turns the splash screen off.
    trc("splash screen", "None")
}

/// Описание плитки «Нет» — как у модуля KDE.
pub fn splash_none_description() -> String {
    tr("No splash screen will be shown")
}

/// Итог применения плитки «Нет».
pub fn splash_off_message() -> String {
    tr("Splash screen turned off — takes effect the next time you log in")
}

/// Порт `_apply_aurorae`: primary kwriteconfig6 theme + (только при ok) две
/// попытки reconfigure KWin (qdbus / qdbus6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuroraePlan {
    pub primary: CfgCmd,
    pub reconfigure_if_ok: Vec<CfgCmd>,
}

pub fn aurorae_plan(name: &str) -> AuroraePlan {
    let theme_val = format!("__aurorae__svg__{name}");
    AuroraePlan {
        primary: CfgCmd {
            argv: vec![
                "kwriteconfig6".into(), "--file".into(), "kwinrc".into(),
                "--group".into(), "org.kde.kdecoration2".into(), "--key".into(),
                "theme".into(), theme_val,
            ],
        },
        reconfigure_if_ok: vec![
            cfg(&["qdbus", "org.kde.KWin", "/KWin", "reconfigure"]),
            cfg(&["qdbus6", "org.kde.KWin", "/KWin", "reconfigure"]),
        ],
    }
}

/// Порт `_apply_lock_wallpaper`: обои экрана блокировки.
/// `install_dir` = `cat.install_dir`. `Err(msg)` — файла нет (одиночная
/// картинка обязательна). `Ok(plan)` — запись картинки и сопутствующие.
///
/// KDE — `kscreenlockerrc` (Image, затем PreviewImage и WallpaperPlugin);
/// Deepin — только в сеансе Treeland, через Центр управления
/// ([`lock_wallpaper_plan_in`]). Где ещё работает этот режим —
/// [`crate::categories::lock_wallpaper_supported`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LockWallpaperPlan {
    /// file://-URI с percent-кодированием не-ASCII (`Path.as_uri`).
    pub uri: String,
    /// Запись картинки (её returncode определяет ok/сообщение).
    pub image: CfgCmd,
    /// Best-effort записи после неё (KDE: PreviewImage и
    /// WallpaperPlugin=org.kde.image; Deepin — нет).
    pub extra: Vec<CfgCmd>,
    pub success_message: String,
    pub failure_message: String,
}

/// `path_is_file` — существует ли файл `install_dir/theme_name` (probe снаружи).
/// `de` — идентификатор окружения (`deepin`, `kde`…); всё, кроме Deepin, —
/// путь KDE, как раньше.
pub fn lock_wallpaper_plan(
    de: &str,
    install_dir: &str,
    theme_name: &str,
    path_is_file: bool,
) -> Result<LockWallpaperPlan, String> {
    lock_wallpaper_plan_in(de, false, install_dir, theme_name, path_is_file)
}

/// То же с учётом сеанса: `treeland` — идёт ли сеанс Treeland. В нём экран
/// блокировки Deepin рисует Treeland, а обои ему отдаёт Центр управления
/// ([`crate::dde::control_center_wallpaper_argv`]); другие сеансы Deepin
/// Rexon не поддерживает — отказ ([`crate::dde::not_treeland_message`]).
pub fn lock_wallpaper_plan_in(
    de: &str,
    treeland: bool,
    install_dir: &str,
    theme_name: &str,
    path_is_file: bool,
) -> Result<LockWallpaperPlan, String> {
    let path = join(install_dir, theme_name);
    if !path_is_file {
        return Err(trf(
            "Wallpaper file not found: %1. The lock screen background supports a \
             single picture only.",
            &[&path],
        ));
    }
    let uri = path_as_uri(&path);
    let success_message =
        tr("Lock screen wallpaper applied — you'll see it the next time the screen locks.");
    let failure_message = tr("Couldn't write the lock screen wallpaper");
    if crate::de_customization::de_from_id(de) == Some(crate::de_customization::De::Deepin) && treeland {
        return Ok(LockWallpaperPlan {
            image: CfgCmd { argv: crate::dde::control_center_wallpaper_argv(&path, true) },
            extra: Vec::new(),
            success_message: tr("Lock screen wallpaper handed to Deepin's Control Center — it sets it on the screen selected there."),
            failure_message: crate::dde::control_center_failed_message(),
            uri,
        });
    }
    if crate::de_customization::de_from_id(de) == Some(crate::de_customization::De::Deepin) {
        return Err(crate::dde::not_treeland_message());
    }
    let w = |key: &str, value: &str| CfgCmd {
        argv: vec![
            "kwriteconfig6".into(), "--file".into(), "kscreenlockerrc".into(),
            "--group".into(), "Greeter".into(), "--group".into(),
            "Wallpaper".into(), "--group".into(), "org.kde.image".into(),
            "--group".into(), "General".into(), "--key".into(), key.into(),
            value.into(),
        ],
    };
    Ok(LockWallpaperPlan {
        image: w("Image", &uri),
        extra: vec![
            w("PreviewImage", &uri),
            cfg(&[
                "kwriteconfig6", "--file", "kscreenlockerrc", "--group", "Greeter",
                "--key", "WallpaperPlugin", "org.kde.image",
            ]),
        ],
        success_message,
        failure_message,
        uri,
    })
}

/// `pathlib.Path(abs).as_uri()`: `file://` + percent-кодирование
/// (unreserved `A-Za-z0-9_.-~` и `/` не кодируются).
pub fn path_as_uri(abs_path: &str) -> String {
    let mut out = String::from("file://");
    for &b in abs_path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// Устарела ли установленная Global Theme (Plasma 5 без metadata.json) — порт
/// `_global_theme_missing_metadata_json`. Проверяет системный и
/// пользовательский каталоги look-and-feel по снимку.
pub fn global_theme_missing_metadata_json(
    fs: &FsSnapshot,
    home: &str,
    theme_name: &str,
) -> bool {
    let roots = [
        "/usr/share/plasma/look-and-feel".to_string(),
        format!("{home}/.local/share/plasma/look-and-feel"),
    ];
    for root in roots {
        let d = join(&root, theme_name);
        if fs.is_dir(&d) {
            return !fs.is_file(&join(&d, "metadata.json"));
        }
    }
    false
}

// ============================================================================
// Удаление
// ============================================================================

/// План удаления по записи журнала — порт `uninstall`. `paths` — из
/// `rec["paths"]`; `is_dir`/`is_file` — probes существования (снаружи).
/// Каталоги → RemoveTree, файлы → RemoveFile; несуществующее — пропуск.
pub fn plan_uninstall(
    paths: &[String],
    is_dir: &dyn Fn(&str) -> bool,
    is_file: &dyn Fn(&str) -> bool,
) -> Vec<ExtStep> {
    let mut steps = Vec::new();
    for p in paths {
        if is_dir(p) {
            steps.push(ExtStep::RemoveTree { path: p.clone(), safe: false, ignore_errors: true });
        } else if is_file(p) {
            steps.push(ExtStep::RemoveFile { path: p.clone() });
        }
    }
    steps
}

/// Сообщение об удалении — порт строки `uninstall`
/// (`f"Удалено ({removed} путей): {name}"`). `removed` = число снятых путей.
pub fn uninstall_message(removed: usize, name: &str) -> String {
    trf("Removed (%1 paths): %2", &[&removed, &name])
}

// ============================================================================
// Куда положить скачанный из Магазина скриншот — порт _save_ocs_preview
// ============================================================================

/// Путь, куда копируется скачанный OCS-скриншот, чтобы локальный скан показывал
/// его вместо плейсхолдера («Вариант 1») — порт РЕШЕНИЯ `_save_ocs_preview`.
///
/// * тип с каталогом → `<theme_dir>/.rexon-preview.png`;
/// * цвет-схема (`apply_kind == "colors"`, каталога нет) либо путь-файл →
///   сайдкар-кеш `~/.cache/rexon/local_previews/<kind>__<имя>.png`;
/// * `None` — имени нет вообще (копировать некуда, оригинал тихо выходит).
///
/// Само копирование (`shutil.copyfile`), `makedirs` и поиск источника
/// (`_ocs_preview_source`: `preview_local` либо файл из кеша превью) — side
/// effect, остаются на вызывающем. `theme_dir_is_dir` — наблюдение
/// `os.path.isdir(theme_dir)`.
pub fn ocs_preview_dest(
    home: &str,
    apply_kind: &str,
    theme_dir: &str,
    theme_dir_is_dir: bool,
    item_name: &str,
) -> Option<String> {
    if apply_kind != "colors" && !theme_dir.is_empty() && theme_dir_is_dir {
        // os.path.join не удваивает разделитель, если левая часть уже на «/».
        let sep = if theme_dir.ends_with('/') { "" } else { "/" };
        return Some(format!(
            "{theme_dir}{sep}{}",
            crate::installed_scan::REXON_PREVIEW_NAME
        ));
    }
    // Имя темы: своё либо последний сегмент пути (без замыкающих слэшей) —
    // порт `item.name or os.path.basename(theme_dir.rstrip("/"))`.
    let name = if item_name.is_empty() {
        let trimmed = theme_dir.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or("").to_string()
    } else {
        item_name.to_string()
    };
    if name.is_empty() {
        return None;
    }
    Some(crate::installed_scan::sidecar_preview_path(home, apply_kind, &name))
}

// ============================================================================
// ЖИВОЙ ИСПОЛНИТЕЛЬ: установка, применение и удаление тем оформления
// ============================================================================
//
// ЗАЧЕМ. Всё, что выше, — РЕШЕНИЯ: куда положить тему, чем её применить, что
// стереть. ДЕЛАТЬ ядро до сих пор не умело ничего: исполнителем этих планов
// был Python (`rexon/appearance/installer.py`, класс `ThemeInstaller`), и
// поэтому вкладка «Оформление» в процессе без Python не могла ни поставить
// тему, ни применить её, ни снять — кнопки были, а за ними пусто.
//
// Здесь дописан исполнитель: те же побочные вызовы, что делал Python
// (`shutil`/`os`/`subprocess`/`pkexec`), но на Rust. Разделение прежнее и оно
// принципиальное — **планировщик решает, исполнитель делает**:
//   * что копировать и что запускать — [`plan_install`] / [`plan_single_file`] /
//     [`plan_root_install`] / [`plan_uninstall`];
//   * чем применять — [`apply_action_for`] плюс планы [`crate::themes`];
//   * снимок ФС для планировщика — [`install_tree_snapshot`];
//   * прогон шагов — [`exec_install_steps`] и [`exec_apply_plan`];
//   * конвейеры целиком — [`LiveInstaller`].
//
// ⛔ ВТОРОЙ КОПИИ ПРАВИЛ ЗДЕСЬ НЕТ. Ни одного решения «какой каталог», «какое
// имя», «какой ключ конфига» этот раздел не принимает: все они уже приняты
// выше или у соседей. Установщик — самое опасное место для тихого расхождения
// (две копии решают, КУДА копировать и ЧТО удалять), и ровно поэтому граница
// проведена так строго.
//
// Живую WE-обою перед статичной снимает общий исполнитель обоев
// ([`crate::wallpaper_exec::WallpaperExec::apply`] → крюк процесса
// `wallpaper_exec::set_we_stopper`, его ставит окно на Qt): здесь второй
// копии снятия нет.
//
// ⚠️ ЧЕГО ЖИВЬЁМ НЕ ВЫХОДИТ (названо, а не замолчано):
//   * шаги плана обоев, поднимающие ДЕРЖАЩИЙ процесс (`swww`, `swaybg`, `wbg`,
//     `hyprpaper`): такой процесс обязан жить дольше вызова, а учёт таких
//     процессов ведёт движок обоев, живого исполнителя которого в ядре нет.
//     Такой шаг отвечает честным отказом со словами, а не молчанием.

use std::path::PathBuf;

use crate::themes::ApplyStep;
use crate::utils::{RunOutcome, RunOutput, RunReport, RunSpec};

/// Реальные типы распаковки, у которых есть свой каталог установки, — ключи
/// [`actual_type_dir`] (порт `_ACTUAL_TYPE_DIR`).
///
/// Список нужен исполнителю ровно для одного: подать планировщику снимок ВСЕХ
/// каталогов, куда установка может уехать после переклассификации, — иначе
/// [`plan_install`] не увидит уже стоящую тему и не снимет её перед копией.
/// Сторож расхождения с таблицей — сверка `actual_types_have_dirs`.
const ACTUAL_TYPES_WITH_DIR: &[&str] = &[
    "look-and-feel", "plasma-theme", "color-scheme", "aurorae", "kvantum",
    "icons",
];

/// Имена файлов, чей текст попадает в снимок дерева — порт `_META_NAMES`.
///
/// `index.theme` добавлен 03.10.2026: по его разделу (`[Sound Theme]`,
/// `[Icon Theme]`) [`find_theme_roots`] узнаёт корни тем в архиве.
const SNAPSHOT_META_NAMES: &[&str] = &["metadata.desktop", "metadata.json", "index.theme"];

// ── мелочи окружения ────────────────────────────────────────────────────────

/// `$HOME` — тот же, что подставлял Python (`os.path.expanduser("~")`).
fn live_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// Значение переменной окружения либо пустая строка.
fn env_str(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// `$PATH` для поиска бинарей (`None` — переменной нет вовсе).
fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// Все пути журнала установленных (что и куда ставил сам Rexon) — по ним
/// скан диска узнаёт системные темы, поставленные Rexon
/// ([`crate::installed_scan::scan_rexon_system`]).
pub fn installed_log_paths() -> Vec<String> {
    let log = crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &env_str("HOME"));
    // Читается через общий журнал установки (шаг 3 слияния установщиков).
    crate::install_journal::store_log_view(&log)
        .values()
        .filter_map(|e| e.get("paths")?.as_array().cloned())
        .flatten()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect()
}

/// `$XDG_CONFIG_HOME`, если он задан.
fn xdg_config_home() -> Option<String> {
    let v = env_str("XDG_CONFIG_HOME");
    if v.is_empty() { None } else { Some(v) }
}

/// `$XDG_DATA_HOME`, если он задан.
fn xdg_data_home() -> Option<String> {
    let v = env_str("XDG_DATA_HOME");
    if v.is_empty() { None } else { Some(v) }
}

/// Метка времени журнала установленных — порт
/// `datetime.datetime.now().isoformat(timespec="seconds")`: МЕСТНОЕ время без
/// зоны. UTC здесь не годится: журнал показывается человеку в карточке темы.
fn now_local_iso() -> String {
    // Местные часы в продукте одни на всех (правило #12): своего
    // `localtime_r` здесь больше нет, зовётся общий `Timestamp::now_local`.
    // `microsecond: 0` — это `timespec="seconds"` оригинала: `isoformat`
    // печатает дробную часть только когда она есть.
    match crate::qt_app::Timestamp::now_local() {
        Some(now) => crate::qt_app::Timestamp { microsecond: 0, ..now }.isoformat(),
        None => {
            // Зона не читается — лучше UTC с меткой, чем пустая строка.
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            crate::engine_update::now_iso(secs)
        }
    }
}

/// Запуск внешней команды с забором вывода — через ЕДИНСТВЕННУЮ точку запуска
/// продукта ([`crate::utils::run`]): она же подменяет синонимы бинарей
/// (`kwriteconfig6` → `kwriteconfig5` на Plasma 5).
///
/// Команда помощника системных тем (установка, «Применить», «Удалить»)
/// сперва идёт сеансу прав окна ([`crate::ext_exec::run_system_argv_session`]:
/// один пароль); его нет — она же прежним путём. Отказ помощника прежним
/// путём не повторяется (З5).
fn run_capture(argv: &[String], timeout_secs: Option<u64>) -> RunReport {
    if let Some(report) = crate::ext_exec::run_system_argv_session(argv, timeout_secs) {
        return report;
    }
    crate::utils::run(
        &RunSpec {
            argv: argv.to_vec(),
            output: RunOutput::Capture,
            timeout_secs,
            ..RunSpec::default()
        },
        path_env().as_deref(),
    )
}

/// `safe_run(argv).returncode == 0` — успех команды.
fn run_ok(argv: &[String], timeout_secs: Option<u64>) -> bool {
    run_capture(argv, timeout_secs).returncode == Some(0)
}

/// Тёмная ли тема GTK `theme` ([`crate::themes::gtk_theme_is_dark`]): таблица
/// стилей GTK 3 (иначе GTK 4) из первого каталога тем, где тема есть.
fn gtk_theme_dark_live(theme: &str, home: &str) -> bool {
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    let css = crate::gtk_sanitizer::theme_dirs("gtk-theme", home, &data)
        .into_iter()
        .map(|base| format!("{base}/{theme}"))
        .find(|dir| Path::new(dir).is_dir())
        .and_then(|dir| {
            ["gtk-3.0", "gtk-4.0"].iter().find_map(|v| read_text_lossy(&format!("{dir}/{v}/gtk.css")))
        });
    crate::themes::gtk_theme_is_dark(theme, css.as_deref())
}

/// Стандартная тема роли `role` (`icons`, `cursor`) окружения `de` — ответ
/// машины на [`crate::themes::standard_theme_read_cmd`]. `None` — у окружения
/// нет схемы для роли или gsettings не ответил.
pub fn live_standard_theme(de: &str, role: &str) -> Option<String> {
    let cmd = crate::themes::standard_theme_read_cmd(de, role)?;
    let report = run_capture(&cmd.argv, cmd.timeout);
    if report.returncode != Some(0) {
        return None;
    }
    let name = crate::themes::parse_native_role_output(de, &crate::utils::decode_text(&report.stdout));
    (!name.is_empty()).then_some(name)
}

/// Семейство пакетов для установки недостающих иконок и курсора темы. На
/// SteamOS и неизменяемых системах системный пакет не встанет — пустая
/// строка: Rexon честно скажет, что набора нет, вместо окна пароля, которое
/// ничего не даст.
pub(crate) fn live_deps_family() -> String {
    let detector = crate::system::DistroDetector::detect_live();
    if detector.is_steamos() || detector.is_immutable() {
        return String::new();
    }
    detector.family_live().as_str().to_string()
}

/// Установленный набор под просьбу темы — на живом диске
/// ([`crate::themes::find_installed_dep`]).
pub(crate) fn live_find_dep(role: crate::themes::DepRole, wanted: &str, roots: &[String]) -> Option<String> {
    let is_dir = |p: &str| Path::new(p).is_dir();
    let list = |p: &str| -> Vec<String> {
        std::fs::read_dir(p)
            .map(|rd| rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
            .unwrap_or_default()
    };
    let read = |p: &str| read_text_lossy(p);
    crate::themes::find_installed_dep(role, wanted, roots, &is_dir, &list, &read)
}

/// Поставить пакеты с иконками и курсором темы тем же путём, что вся
/// установка пакетов Rexon: репозиторий — [`crate::system_plan::install_argv`]
/// (помощник `org.rexon.pkg` либо `pkexec`), на Arch пакет не из репозитория
/// — помощником AUR. `Err` — причина для человека.
///
/// Что встало на самом деле, решает не код возврата, а диск: вызывающий
/// ищет набор заново.
pub(crate) fn live_install_dep_packages(packages: &[String]) -> Result<(), String> {
    live_install_packages(packages, true, PkgSource::ThemeSets)
}

/// То же, что [`live_install_dep_packages`], но только из официальных
/// репозиториев: пакет, которого там нет, из AUR НЕ собирается (сборку из
/// AUR без вопроса Rexon не запускает), а называется в `Err` — после
/// установки остальных.
///
/// `source` — таблица ядра, из которой имена ([`PkgSource`]): по ней их
/// проверяет помощник сеанса прав окна.
pub(crate) fn live_install_repo_packages(packages: &[String], source: PkgSource) -> Result<(), String> {
    live_install_packages(packages, false, source)
}

/// Операция сеанса прав для пакетов из репозитория — те же имена и то же
/// семейство, что у прежней команды [`crate::system_plan::install_argv`].
fn repo_install_op(family: crate::system::PkgFamily, repo: &[String], source: PkgSource) -> PrivOp {
    PrivOp::PkgInstall { family, packages: repo.to_vec(), source }
}

fn live_install_packages(packages: &[String], allow_aur: bool, source: PkgSource) -> Result<(), String> {
    use crate::system::PkgFamily;
    use crate::system_plan as sp;
    let family = crate::system::DistroDetector::detect_live().family_live();
    let mut waiter = sp::LiveLockWait::new(|_: &str, _: &str| {});
    let (free, why) = sp::wait_for_pkg_lock(&mut waiter, &sp::lock_paths_live(family), sp::PKG_LOCK_TIMEOUT_SECS);
    if !free {
        return Err(why);
    }
    // В CachyOS `mint-y-icons` лежит в репозитории, на чистом Arch — только
    // в AUR: путь выбирается по факту (`pacman -Si`), а не по таблице.
    let (repo, aur): (Vec<String>, Vec<String>) = if family == PkgFamily::Arch {
        packages
            .iter()
            .cloned()
            .partition(|p| run_ok(&["pacman".to_string(), "-Si".to_string(), p.clone()], Some(30)))
    } else {
        (packages.to_vec(), Vec::new())
    };
    let helper_ok = have("pkexec")
        && sp::is_exec_file(sp::pkg_helper_path())
        && Path::new(sp::PKG_POLICY_PATH).exists();
    let mut commands: Vec<Vec<String>> = Vec::new();
    if !repo.is_empty() {
        let Some(argv) = sp::install_argv(family, &repo, helper_ok) else {
            return Err(tr("Rexon doesn't know the package manager of this system"));
        };
        // Сеанс прав окна (один пароль); его нет — прежняя команда. Отказ
        // помощника прежней командой не повторяется (З5).
        // Без `pkexec` (NixOS — профиль пользователя) прав не нужно вовсе.
        let op = repo_install_op(family, &repo, source);
        let session = if argv.first().map(String::as_str) == Some("pkexec") {
            crate::privilege::run::run_live(&op, &[], None, &mut |_| {})
        } else {
            None
        };
        match session {
            Some(out) => match out.returncode {
                Some(0) => {}
                Some(126) | Some(127) => return Err(password_not_confirmed()),
                _ => return Err(tr("the package manager reported an error")),
            },
            None => commands.push(argv),
        }
    }
    if !aur.is_empty() && allow_aur {
        let Some(helper) = crate::compliance::AUR_HELPERS.iter().find(|h| have(h)) else {
            return Err(crate::appearance_page::aur_no_builder());
        };
        commands.push(crate::aur_install::install_argv(helper, &aur, false));
    }
    for argv in commands {
        let report = crate::utils::run(
            &RunSpec {
                argv,
                output: RunOutput::Capture,
                timeout_secs: Some(3600),
                lang_c: true,
                stderr_to_stdout: true,
                ..RunSpec::default()
            },
            path_env().as_deref(),
        );
        match report.returncode {
            Some(0) => {}
            // pkexec: окно закрыто (126) или пароль не принят (127).
            Some(126) | Some(127) => return Err(password_not_confirmed()),
            _ => return Err(tr("the package manager reported an error")),
        }
    }
    if !aur.is_empty() && !allow_aur {
        // TRANSLATORS: %1 is a list of packages.
        return Err(trf("not in the official repositories: %1", &[&aur.join(", ")]));
    }
    Ok(())
}

/// Причина отказа установки пакетов, когда окно пароля polkit закрыто или
/// пароль не принят: по ней вызывающий отличает «человек передумал» от
/// ошибки менеджера пакетов.
pub fn password_not_confirmed() -> String {
    tr("the administrator password wasn't confirmed")
}

/// Есть ли программа в системе (`shutil.which`).
fn have(binary: &str) -> bool {
    crate::utils::which(binary, path_env().as_deref()).is_some()
}

/// Уникальный временный каталог (`tempfile.mkdtemp(prefix=…)`).
fn mkdtemp_in(base: &Path, prefix: &str) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..64u64 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            base.join(format!("{prefix}{}_{stamp}_{}", std::process::id(), n + attempt));
        match std::fs::create_dir_all(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(trf("couldn't create a temporary folder: %1", &[&e])),
        }
    }
    Err(tr("couldn't create a temporary folder: all names are taken"))
}

/// Текст файла с заменой негодных байт (`errors="replace"`); `None` — не
/// прочли (аналог `except OSError`).
fn read_text_lossy(path: &str) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

// ── исполнитель шагов установки ─────────────────────────────────────────────

/// Исполнить план шагов установки — порт `_rs_exec_steps`.
///
/// ⭐ Починка при переносе: у оригинала шаг `Run` (`gtk-update-icon-cache`)
/// ронял ВСЮ установку, если программы в системе нет — `subprocess.run`
/// поднимал `FileNotFoundError`, и `install` возвращал «Ошибка установки»
/// уже ПОСЛЕ копирования темы, не дойдя до записи в журнал. Тема оказывалась
/// на диске, но не в списке установленных, и снять её было нечем. Здесь
/// неудача внешней команды — не ошибка плана: файлы уже на месте, кеш иконок
/// пересоберётся при следующем входе.
pub fn exec_install_steps(steps: &[ExtStep]) -> Result<(), String> {
    use crate::install_plan::{exec, Ctx, Failure};
    let plan: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    let ctx = Ctx::live(true, false, None, false);
    exec(&plan, &mut Vec::new(), &ctx).map(|_| ()).map_err(|f| match f {
        Failure::MakeDirs { path, err } | Failure::Remove { path, err } => format!("{path}: {err}"),
        Failure::Copy { src, dest, err } => format!("{src} → {dest}: {err}"),
        // Других шагов в плане магазина нет; текст — как у прочих.
        Failure::Text(text) => text,
        Failure::Symlink { link: path, err }
        | Failure::Write { path, err }
        | Failure::Unpack { archive: path, err }
        | Failure::Prepare { source: path, err } => format!("{path}: {err}"),
    })
}

// ── снимок распакованного дерева ────────────────────────────────────────────

/// Снимок дерева для планировщика — порт `_rs_tree_snapshot`.
///
/// Рекурсивно `root` (текст читается ТОЛЬКО у `metadata.json`/`metadata.desktop`
/// — остальные файлы попадают в снимок пустышками, planner'у нужен лишь факт
/// существования), плюс НЕрекурсивно дети каталогов `dest_dirs`: по ним
/// [`plan_install`] узнаёт, что тема с таким именем уже стоит, и снимает её.
///
/// Имена внутри каталога отсортированы. У оригинала здесь был порядок
/// `os.scandir`, то есть порядок файловой системы: от него зависят и выбор
/// «первого <name>.plymouth», и порядок имён в сообщении об установке
/// цветовых схем — от машины к машине он разный. Сортировка делает ответ
/// одинаковым везде (тот же довод, что у `ext_exec`).
pub fn install_tree_snapshot(root: &str, dest_dirs: &[String]) -> FsSnapshot {
    let mut dirs = serde_json::Map::new();
    let mut files = serde_json::Map::new();

    fn names_of(dir: &str) -> Option<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Some(names)
    }

    fn rec(
        dir: &str,
        dirs: &mut serde_json::Map<String, Value>,
        files: &mut serde_json::Map<String, Value>,
    ) {
        let Some(names) = names_of(dir) else { return };
        dirs.insert(dir.to_string(), Value::from(names.clone()));
        for name in names {
            let path = join(dir, &name);
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.is_dir() {
                rec(&path, dirs, files);
            } else {
                let text = if SNAPSHOT_META_NAMES.contains(&name.as_str()) || name.ends_with(".colors") {
                    read_text_lossy(&path).unwrap_or_default()
                } else {
                    String::new()
                };
                files.insert(path, Value::from(text));
            }
        }
    }

    rec(root, &mut dirs, &mut files);

    for base in dest_dirs {
        if base.is_empty() {
            continue;
        }
        let Some(names) = names_of(base) else { continue };
        dirs.entry(base.clone())
            .or_insert_with(|| Value::from(names.clone()));
        for name in names {
            let path = join(base, &name);
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            // Папка (и ссылка на папку — наши ссылки `~/.themes/<имя>`) —
            // с именами первого уровня: по ним рамка окон решает, ложиться ли
            // подпапкой в тему GTK ([`plan_nested_part`]).
            let dir_like = md.is_dir() || (md.file_type().is_symlink() && Path::new(&path).is_dir());
            if dir_like {
                let inner = names_of(&path).unwrap_or_default();
                for child in &inner {
                    let child_path = join(&path, child);
                    if Path::new(&child_path).is_dir() {
                        dirs.entry(child_path).or_insert_with(|| Value::Array(Vec::new()));
                    } else {
                        files.entry(child_path).or_insert_with(|| Value::from(""));
                    }
                }
                dirs.entry(path).or_insert_with(|| Value::from(inner));
            } else {
                // Схема цветов на месте — с текстом: одинаковая байт в байт
                // считается своей ([`file_taken`]); ссылка — пустышкой.
                let text = if md.is_file() && name.ends_with(".colors") {
                    read_text_lossy(&path).unwrap_or_default()
                } else {
                    String::new()
                };
                files.entry(path).or_insert_with(|| Value::from(text));
            }
        }
    }

    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

// ── подъём вложенных тем: починка уже испорченных установок ─────────────────
//
// Установки до 03.10.2026 клали архив «как есть» (см. [`find_theme_roots`]):
// на диске владельца лежат `~/.local/share/sounds/zorinos2/sounds/Zorin/…` и
// `~/.local/share/sounds/Sounds/Zorin/…`. Миграция находит такие обёртки
// (только СВОИ: с превью Rexon `.rexon-preview.png` внутри либо записанные в
// журнал установленного), поднимает каждую тему на верхний уровень каталога
// вида переименованием (атомарно в пределах одной ФС) и уносит остаток
// обёртки (обои, xml, ReadMe) в `<data>/rexon/theme-leftovers/<вид>/` —
// ничего не удаляется. Каждый шаг пишется в журнал миграции, запись журнала
// установленного переписывается на новые пути, чтобы «Удалить» сняло всё.
//
// Занятое имя наверху — честный пропуск: тема остаётся в обёртке, обёртка
// остаётся на месте, в журнале строка о конфликте.

/// Перенос одного корня темы.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NestedMove {
    pub from: String,
    pub to: String,
}

/// План подъёма тем из одной обёртки.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NestedWrapperPlan {
    pub kind: String,
    pub wrapper: String,
    pub moves: Vec<NestedMove>,
    /// Корни, которые поднять нельзя: имя наверху занято (или повторяется в
    /// той же обёртке). Остаются на месте.
    pub conflicts: Vec<NestedMove>,
    /// Куда унести остаток обёртки; `None` — обёртка остаётся (есть конфликт).
    pub leftovers_to: Option<String>,
}

/// Чистое решение по одной обёртке `<base>/<wrapper_name>`.
///
/// `None` — делать нечего: каталог сам тема (стоит правильно) либо тем
/// внутри нет. `exists` — наблюдение «путь наверху занят».
pub fn plan_nested_wrapper(
    fs: &FsSnapshot,
    base: &str,
    wrapper_name: &str,
    kind: &str,
    exists: &dyn Fn(&str) -> bool,
    leftovers_dest: &str,
) -> Option<NestedWrapperPlan> {
    let wrapper = join(base, wrapper_name);
    if is_theme_root(fs, &wrapper, kind) {
        return None;
    }
    let roots = find_theme_roots(fs, &wrapper, kind);
    if roots.is_empty() {
        return None;
    }
    let mut plan = NestedWrapperPlan {
        kind: kind.to_string(),
        wrapper: wrapper.clone(),
        ..Default::default()
    };
    let mut taken: Vec<String> = Vec::new();
    for root in roots {
        let name = basename(rstrip_slashes(&root)).to_string();
        let to = join(base, &name);
        // Имя самой обёртки занято ею же: подъём `Sounds/Sounds` поверх
        // `Sounds` переименованием не сделать — честный конфликт.
        if taken.contains(&to) || exists(&to) || to == wrapper {
            plan.conflicts.push(NestedMove { from: root, to });
            continue;
        }
        taken.push(to.clone());
        plan.moves.push(NestedMove { from: root, to });
    }
    if plan.conflicts.is_empty() {
        plan.leftovers_to = Some(leftovers_dest.to_string());
    }
    Some(plan)
}

/// Журнал миграции: `<data>/rexon/theme-migration.log`.
pub fn nested_migration_log_path(data_home: &str) -> String {
    format!("{}/rexon/theme-migration.log", data_home.trim_end_matches('/'))
}

/// Куда уходят остатки обёрток: `<data>/rexon/theme-leftovers`.
pub fn nested_leftovers_dir(data_home: &str) -> String {
    format!("{}/rexon/theme-leftovers", data_home.trim_end_matches('/'))
}

/// Подъём вложенных тем под `home` — исполнение.
///
/// Касается ТОЛЬКО каталогов видов из [`kind_uses_theme_roots`] и только
/// обёрток с маркером Rexon либо из журнала установленного. Возвращает строки
/// журнала (пусто — делать было нечего); они же дописываются в
/// [`nested_migration_log_path`].
pub fn live_migrate_nested_themes(home: &str) -> Vec<String> {
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    let log_path =
        crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), home);
    migrate_nested_themes_at(home, &data, &log_path)
}

/// Тело [`live_migrate_nested_themes`] с явными каталогами: `data` — каталог
/// данных (журнал миграции, остатки), `log_path` — журнал установленного.
/// Сверки зовут его на подставном HOME, не трогая окружение процесса.
pub fn migrate_nested_themes_at(home: &str, data: &str, log_path: &str) -> Vec<String> {
    let home_p = Path::new(home);
    let data = data.to_string();
    let log_path = log_path.to_string();
    let mut journal = crate::install_journal::store_log_view(&log_path);
    let recorded: Vec<String> = journal
        .values()
        .filter_map(|e| e.get("paths").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .map(|p| rstrip_slashes(p).to_string())
        .collect();

    let mut lines: Vec<String> = Vec::new();
    let mut journal_changed = false;
    let mut seen_bases: Vec<String> = Vec::new();
    for kind in ["sounds", "icons", "gtk", "aurorae"] {
        let Some(cat) = category_by_kind(home_p, kind) else { continue };
        let base = rstrip_slashes(&cat.install_dir).to_string();
        if base.is_empty() || seen_bases.contains(&base) {
            continue;
        }
        seen_bases.push(base.clone());
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();
        for name in names {
            let wrapper = join(&base, &name);
            let marker = join(&wrapper, crate::installed_scan::REXON_PREVIEW_NAME);
            let ours = Path::new(&marker).is_file() || recorded.contains(&wrapper);
            if !ours {
                continue;
            }
            let snap = install_tree_snapshot(&wrapper, &[]);
            let leftovers = unique_path(&format!("{}/{kind}/{name}", nested_leftovers_dir(&data)));
            let exists = |p: &str| Path::new(p).exists();
            let Some(mut plan) = plan_nested_wrapper(&snap, &base, &name, kind, &exists, &leftovers)
            else {
                continue;
            };
            let mut moved: Vec<String> = Vec::new();
            for mv in std::mem::take(&mut plan.moves) {
                match std::fs::rename(&mv.from, &mv.to) {
                    Ok(()) => {
                        lines.push(format!("перенесено: {} → {}", mv.from, mv.to));
                        if Path::new(&marker).is_file() {
                            let dst = join(&mv.to, crate::installed_scan::REXON_PREVIEW_NAME);
                            if !Path::new(&dst).exists() {
                                let _ = std::fs::copy(&marker, &dst);
                            }
                        }
                        moved.push(mv.to);
                    }
                    Err(e) => {
                        lines.push(format!("НЕ перенесено ({e}): {} → {}", mv.from, mv.to));
                        plan.leftovers_to = None;
                    }
                }
            }
            for c in &plan.conflicts {
                lines.push(format!("конфликт, оставлено на месте: {} (занято {})", c.from, c.to));
            }
            let mut wrapper_now = wrapper.clone();
            if let Some(dest) = &plan.leftovers_to {
                let parent = Path::new(dest).parent().map(Path::to_path_buf);
                let ok = parent.map(|p| std::fs::create_dir_all(p).is_ok()).unwrap_or(false)
                    && std::fs::rename(&wrapper, dest).is_ok();
                if ok {
                    lines.push(format!("остаток обёртки: {wrapper} → {dest}"));
                    wrapper_now = dest.clone();
                } else {
                    lines.push(format!("остаток обёртки оставлен на месте: {wrapper}"));
                }
            }
            // Журнал установленного: обёртка → поднятые темы (+ где остаток).
            for entry in journal.values_mut() {
                let Some(paths) = entry.get_mut("paths").and_then(Value::as_array_mut) else {
                    continue;
                };
                if !paths.iter().any(|p| p.as_str().map(rstrip_slashes) == Some(wrapper.as_str())) {
                    continue;
                }
                let mut next: Vec<Value> = paths
                    .iter()
                    .filter(|p| p.as_str().map(rstrip_slashes) != Some(wrapper.as_str()))
                    .cloned()
                    .collect();
                next.extend(moved.iter().map(|m| Value::from(m.clone())));
                next.push(Value::from(wrapper_now.clone()));
                *paths = next;
                journal_changed = true;
            }
        }
    }
    if journal_changed {
        if let Err(e) = crate::install_journal::put_store_log(&log_path, &journal) {
            lines.push(format!("журнал установленного не переписан: {e}"));
        }
    }
    if !lines.is_empty() {
        let log = nested_migration_log_path(&data);
        if let Some(dir) = Path::new(&log).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let stamp = now_local_iso();
        let mut text = read_text_lossy(&log).unwrap_or_default();
        for l in &lines {
            text.push_str(&format!("{stamp} {l}\n"));
        }
        let _ = crate::utils::atomic_write(&log, &text);
    }
    lines
}

/// Свободное имя: `path`, иначе `path-2`, `path-3`, …
fn unique_path(path: &str) -> String {
    if !Path::new(path).exists() {
        return path.to_string();
    }
    (2..).map(|n| format!("{path}-{n}")).find(|p| !Path::new(p).exists()).unwrap()
}

/// [`live_migrate_nested_themes`] не чаще раза на `home` за процесс — для
/// вызова из скана диска.
pub fn migrate_nested_themes_once(home: &str) -> Vec<String> {
    use std::sync::Mutex;
    static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());
    {
        let Ok(mut done) = DONE.lock() else { return Vec::new() };
        if done.iter().any(|h| h == home) {
            return Vec::new();
        }
        done.push(home.to_string());
    }
    live_migrate_nested_themes(home)
}

/// Мини-снимок каталогов look-and-feel для
/// [`global_theme_missing_metadata_json`] — порт `_rs_lnf_snapshot`.
fn lnf_snapshot(home: &str, theme_name: &str) -> FsSnapshot {
    let mut dirs = serde_json::Map::new();
    let mut files = serde_json::Map::new();
    let roots = [
        "/usr/share/plasma/look-and-feel".to_string(),
        format!("{home}/.local/share/plasma/look-and-feel"),
    ];
    for root in roots {
        let d = join(&root, theme_name);
        if Path::new(&d).is_dir() {
            dirs.insert(d.clone(), Value::Array(Vec::new()));
            let mj = join(&d, "metadata.json");
            if Path::new(&mj).is_file() {
                files.insert(mj, Value::from(""));
            }
        }
    }
    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

/// Стоит ли Global Theme в устаревшем формате Plasma 5 (без `metadata.json`) —
/// живой порт `ThemeInstaller._global_theme_missing_metadata_json`.
///
/// Plasma 6 (KPackage v2) требует `metadata.json` с
/// `KPackageStructure="Plasma/LookAndFeel"`; тема с одним `metadata.desktop`
/// не регистрируется и молча не применяется. Вкладке «Оформление» ответ нужен
/// синхронно — отсюда отдельная функция, а не только ветка внутри применения.
pub fn live_global_theme_missing_metadata_json(home: &str, theme_name: &str) -> bool {
    global_theme_missing_metadata_json(&lnf_snapshot(home, theme_name), home, theme_name)
}

// ── распаковка ──────────────────────────────────────────────────────────────

/// Абсолютный нормализованный путь каталога распаковки (`os.path.abspath`).
fn abs_dest(dest: &str) -> String {
    if dest.starts_with('/') {
        normpath(dest)
    } else {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string());
        normpath(&py_join(&cwd, dest))
    }
}

/// Имена записей ZIP-архива; `None` — это не ZIP.
fn zip_entry_names(archive: &str) -> Option<Vec<String>> {
    let file = std::fs::File::open(archive).ok()?;
    let mut zf = zip::ZipArchive::new(file).ok()?;
    let mut names = Vec::with_capacity(zf.len());
    for i in 0..zf.len() {
        names.push(zf.by_index(i).ok()?.name().to_string());
    }
    Some(names)
}

/// Имена записей TAR-архива через системный `tar -tf`; `None` — не TAR либо
/// `tar` в системе нет (различает эти случаи вызывающий по [`have`]).
fn tar_entry_names(archive: &str) -> Option<Vec<String>> {
    if !have("tar") {
        return None;
    }
    let argv = vec!["tar".to_string(), "-tf".to_string(), archive.to_string()];
    let report = run_capture(&argv, Some(120));
    if report.returncode != Some(0) {
        return None;
    }
    Some(
        crate::utils::decode_text(&report.stdout)
            .lines()
            .map(str::to_string)
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// Подпись архива 7z в первых байтах файла.
const SEVEN_ZIP_MAGIC: &[u8] = b"7z\xBC\xAF\x27\x1C";

/// Чем распаковать архив 7z: `bsdtar` (libarchive; на Arch и SteamOS стоит
/// всегда — от него зависит pacman) либо 7-Zip под любым из его имён.
fn seven_zip_tool() -> Option<&'static str> {
    ["bsdtar", "7z", "7za", "7zz"].into_iter().find(|b| have(b))
}

/// Архив ли 7z (по подписи в начале файла, а не по имени).
fn is_seven_zip(archive: &str) -> bool {
    read_head(archive, SEVEN_ZIP_MAGIC.len()).starts_with(SEVEN_ZIP_MAGIC)
}

/// Чем распаковать 7z нечем — отказ для человека с пакетами его семейства
/// дистрибутива (`family` — `system::PkgFamily`; незнакомое — имена,
/// как в большинстве дистрибутивов).
pub fn seven_zip_missing(family: &str) -> String {
    trf(
        // TRANSLATORS: %1 and %2 are package names, e.g. “libarchive-tools” and “7zip”.
        "To unpack .7z archives you need the bsdtar program (package %1) or 7-Zip (package %2)",
        &[
            &crate::utils::tool_package_name("bsdtar", family),
            &crate::utils::tool_package_name("7z", family),
        ],
    )
}

/// Пакет распаковщика 7z (bsdtar), который Rexon может поставить на системе
/// семейства `family`; `None` — не может (семейство без записи в таблице
/// пакетов).
pub fn seven_zip_package(family: &str) -> Option<&'static str> {
    crate::utils::TOOL_PACKAGES
        .iter()
        .find(|(bin, _)| *bin == "bsdtar")?
        .1
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, pkg)| *pkg)
}

/// [`seven_zip_package`] на ЭТОЙ машине; на SteamOS и неизменяемых системах
/// системный пакет не встанет — `None`.
pub fn live_seven_zip_package() -> Option<&'static str> {
    let detector = crate::system::DistroDetector::detect_live();
    if detector.is_steamos() || detector.is_immutable() {
        return None;
    }
    seven_zip_package(detector.family_live().as_str())
}

/// Есть ли на машине чем распаковать 7z.
pub fn have_seven_zip_tool() -> bool {
    seven_zip_tool().is_some()
}

/// Пакет распаковщика 7z, который Rexon ставит перед установкой товара
/// САМ, без вопроса «Да/Нет» (решение владельца 24.10; защита — окно пароля
/// polkit). `None` — ставить нечего: файл не `.7z`, распаковщик есть либо
/// поставить его Rexon здесь не может (тогда установка откажет словами
/// [`seven_zip_missing`]).
pub fn seven_zip_install_package(file: &str, have_tool: bool, package: Option<&str>) -> Option<String> {
    if have_tool || !file.to_lowercase().ends_with(".7z") {
        return None;
    }
    package.map(str::to_string)
}

/// Поставить bsdtar тем же путём, что вся установка пакетов Rexon, только
/// из официальных репозиториев ([`live_install_repo_packages`]), и
/// проверить по факту. `Err` — причина для человека.
pub fn install_seven_zip_tool_live() -> Result<(), String> {
    let Some(package) = live_seven_zip_package() else {
        return Err(tr("Rexon can't install programs on this system by itself."));
    };
    live_install_repo_packages(&[package.to_string()], PkgSource::Tools)?;
    if !have_seven_zip_tool() {
        return Err(trf("the %1 package was installed, but the bsdtar program didn't appear", &[&package]));
    }
    Ok(())
}

/// Итог, когда bsdtar не встал: товар не ставится вовсе (распаковать его
/// всё равно нечем), и сказано почему и что сделать. Окно пароля закрыто —
/// «передумал»: совет повторить установку и ввести пароль.
pub fn seven_zip_install_failed(why: &str) -> String {
    if why == password_not_confirmed() {
        return tr(
            "Nothing was installed: the file is a .7z archive, and unpacking it needs the bsdtar \
             program. Install again and enter the password — Rexon will install bsdtar first.",
        );
    }
    trf("Nothing was installed: couldn't install bsdtar to unpack the .7z archive: %1.", &[&why])
}

/// Записи архива 7z.
///
/// ⛔ Дефект стенда 17.10.2026: `.7z` стоит в [`crate::ocs::ARCHIVE_EXTS`]
/// (магазин выбирает такой файл как архив), а распаковщик знал только zip и
/// tar — «Numix Holo» (xfwm4) и «BlueSky» (Cinnamon) отвечали «неизвестный
/// формат архива».
///
/// `(имя, символическая ссылка?)`; `None` — не 7z либо распаковать нечем.
fn seven_zip_entries(archive: &str) -> Option<Vec<(String, bool)>> {
    if !is_seven_zip(archive) {
        return None;
    }
    seven_zip_entries_with(seven_zip_tool()?, archive)
}

/// [`seven_zip_entries`] программой `tool` (`bsdtar`, `7z`, `7za`, `7zz`).
fn seven_zip_entries_with(tool: &str, archive: &str) -> Option<Vec<(String, bool)>> {
    let listing = |argv: Vec<String>| {
        let report = run_capture(&argv, Some(120));
        (report.returncode == Some(0)).then(|| crate::utils::decode_text(&report.stdout))
    };
    if tool == "bsdtar" {
        // `-tf` — имена, `-tvf` — те же записи в том же порядке с правами
        // (`l…` у ссылки).
        let names = listing(vec![tool.into(), "-tf".into(), archive.into()])?;
        let long = listing(vec![tool.into(), "-tvf".into(), archive.into()])?;
        let names: Vec<&str> = names.lines().filter(|l| !l.is_empty()).collect();
        let modes: Vec<bool> = long.lines().filter(|l| !l.is_empty()).map(|l| l.starts_with('l')).collect();
        return Some(names.iter().enumerate().map(|(i, n)| (n.to_string(), modes.get(i) == Some(&true))).collect());
    }
    let text = listing(vec![tool.into(), "l".into(), "-ba".into(), "-slt".into(), "--".into(), archive.into()])?;
    Some(parse_seven_zip_slt(&text, archive))
}

/// Разбор `7z l -ba -slt` (p7zip и 7-Zip): записи по `Path = `, ссылка —
/// права Unix в `Attributes = ` начинаются с `l` (`A lrwxrwxrwx`).
fn parse_seven_zip_slt(text: &str, archive: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("Path = ") {
            if !name.is_empty() && name != archive {
                out.push((name.to_string(), false));
            }
        } else if let Some(attrs) = line.strip_prefix("Attributes = ") {
            let link = attrs.split_whitespace().any(|w| w.len() == 10 && w.starts_with('l'));
            if let Some(last) = out.last_mut() {
                last.1 |= link;
            }
        }
    }
    out
}

/// Запись архива, которая ляжет СКВОЗЬ ссылку из того же архива
/// (`d → /куда-то`, затем `d/файл`): распаковщик записал бы файл за
/// пределы каталога распаковки. `None` — таких нет.
pub fn entry_under_archive_link(entries: &[(String, bool)]) -> Option<String> {
    let links: Vec<String> =
        entries.iter().filter(|(_, l)| *l).map(|(n, _)| format!("{}/", n.trim_end_matches('/'))).collect();
    entries.iter().map(|(n, _)| n).find(|n| links.iter().any(|l| n.starts_with(l.as_str()))).cloned()
}

/// Выводит ли ссылка за пределы каталога распаковки `dest_abs`: `link` —
/// путь ссылки (под `dest_abs`), `target` — куда она ведёт. Абсолютная цель
/// — всегда наружу (тема не знает, где будет стоять); относительная — по
/// каталогу ссылки.
pub fn link_escapes(dest_abs: &str, link: &str, target: &str) -> bool {
    if target.starts_with('/') {
        return true;
    }
    let dir = link.rsplit_once('/').map(|(d, _)| d).unwrap_or(dest_abs);
    let resolved = normpath(&py_join(dir, target));
    !(resolved == dest_abs || resolved.starts_with(&format!("{dest_abs}/")))
}

/// После распаковки: ссылка, которая ведёт наружу ([`link_escapes`]) —
/// `(путь, цель)`. bsdtar кладёт такие ссылки как есть, 7-Zip абсолютную
/// переписывает внутрь, p7zip — как есть; тема со ссылкой на `/etc` или в
/// HOME уехала бы в `~/.themes` вместе с ней.
fn escaping_link_in(dest_abs: &str) -> Option<(String, String)> {
    fn walk(dest_abs: &str, dir: &Path, depth: usize) -> Option<(String, String)> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.file_type().is_symlink() {
                let target = std::fs::read_link(&path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
                let p = path.to_string_lossy().into_owned();
                if link_escapes(dest_abs, &p, &target) {
                    return Some((p, target));
                }
            } else if md.is_dir() && depth < 64 {
                if let Some(found) = walk(dest_abs, &path, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(dest_abs, Path::new(dest_abs), 0)
}

/// Распаковывается ли файл вообще — порт пробы
/// `zipfile.is_zipfile(path) or tarfile.is_tarfile(path)` (плюс 7z — по
/// подписи: и тогда, когда распаковать его нечем, это архив, а не «голый
/// файл темы», и распаковка скажет, чего не хватает).
fn is_archive_file(archive: &str) -> bool {
    zip_entry_names(archive).is_some() || tar_entry_names(archive).is_some() || is_seven_zip(archive)
}

/// Безопасно распаковать zip/tar.* в `dest` — живой порт `_safe_extract`.
///
/// Каждое имя записи проходит через [`safe_extract_entry`] ДО извлечения:
/// защита от «../» — не то место, где допустимы две слегка разные проверки.
/// ZIP извлекается общим [`crate::ext_utils::safe_zip_extract`] (у него своя,
/// вторая проверка ZipSlip — так и задумано), TAR — системным `tar` с
/// `--no-same-owner`.
///
/// ⭐ Починка при переносе: оригинал на ЛЮБУЮ неудачу отдавал наверх одно
/// «Не удалось распаковать архив», а настоящую причину клал в журнал, куда
/// покупатель не смотрит. «Опасная запись в архиве», «нет программы tar» и
/// «файл повреждён» — это три разных ответа, и теперь они разные.
pub fn live_safe_extract(archive: &str, dest: &str) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{dest}: {e}"))?;
    let dest_abs = abs_dest(dest);

    if let Some(names) = zip_entry_names(archive) {
        for name in &names {
            if !safe_extract_entry(&dest_abs, name) {
                return Err(trf("unsafe entry in the archive: %1", &[name]));
            }
        }
        return crate::ext_utils::safe_zip_extract(Path::new(archive), Path::new(dest))
            .map_err(|e| e.to_string());
    }

    if let Some(entries) = seven_zip_entries(archive) {
        return seven_zip_extract(seven_zip_tool().unwrap_or("bsdtar"), &entries, archive, dest, &dest_abs);
    }

    // 7z, а распаковать нечем: сказать, ЧТО поставить, а не «неизвестный
    // формат» (стенд 17.10.2026 без bsdtar и 7-Zip).
    if is_seven_zip(archive) && !have_seven_zip_tool() {
        let family = crate::system::DistroDetector::detect_live().family_live();
        return Err(seven_zip_missing(family.as_str()));
    }
    live_safe_extract_tar(archive, dest, &dest_abs)
}

/// Распаковать 7z программой `tool`: имена (обход пути), записи сквозь
/// ссылку архива, после распаковки — ссылки наружу.
fn seven_zip_extract(tool: &str, entries: &[(String, bool)], archive: &str, dest: &str, dest_abs: &str) -> Result<(), String> {
    for (name, _) in entries {
        if !safe_extract_entry(dest_abs, name) {
            return Err(trf("unsafe entry in the archive: %1", &[name]));
        }
    }
    // Запись сквозь ссылку того же архива — тот же обход пути.
    if let Some(name) = entry_under_archive_link(entries) {
        return Err(trf("unsafe entry in the archive: %1", &[&name]));
    }
    let argv: Vec<String> = if tool == "bsdtar" {
        vec![tool.into(), "-xf".into(), archive.into(), "-C".into(), dest.into(), "--no-same-owner".into()]
    } else {
        vec![tool.into(), "x".into(), "-y".into(), format!("-o{dest}"), "--".into(), archive.into()]
    };
    let report = run_capture(&argv, Some(300));
    // Ссылка наружу — отказ, как обход пути: 7-Zip на такой ссылке ещё
    // и кончает с кодом 2 («Dangerous link path was ignored»), и
    // человеку важна причина, а не код.
    if let Some((link, target)) = escaping_link_in(dest_abs) {
        let rel = link.strip_prefix(&format!("{dest_abs}/")).unwrap_or(&link).to_string();
        return Err(trf("unsafe entry in the archive: %1", &[&format!("{rel} → {target}")]));
    }
    if report.returncode == Some(0) {
        return Ok(());
    }
    let out = crate::utils::decode_text(&report.stderr);
    let out = if out.trim().is_empty() { crate::utils::decode_text(&report.stdout) } else { out };
    let why = out.lines().filter(|l| l.contains("ERROR")).collect::<Vec<_>>().join("; ");
    Err(format!("{tool}: {}", if why.is_empty() { out.trim().to_string() } else { why }))
}

/// tar-ветка [`live_safe_extract`].
fn live_safe_extract_tar(archive: &str, dest: &str, dest_abs: &str) -> Result<(), String> {
    if !have("tar") {
        return Err(tr(
            "the tar program was not found, so there's nothing to unpack this archive \
             with. Install the tar package.",
        ));
    }
    let Some(names) = tar_entry_names(archive) else {
        return Err(trf("unknown archive format: %1", &[&archive]));
    };
    for name in &names {
        if !safe_extract_entry(dest_abs, name) {
            return Err(trf("unsafe entry in the archive: %1", &[name]));
        }
    }
    let argv = vec![
        "tar".to_string(),
        "-xf".to_string(),
        archive.to_string(),
        "-C".to_string(),
        dest.to_string(),
        "--no-same-owner".to_string(),
    ];
    let report = run_capture(&argv, Some(300));
    if report.returncode == Some(0) {
        return Ok(());
    }
    let err = crate::utils::decode_text(&report.stderr);
    let detail = err.trim();
    Err(if detail.is_empty() {
        tr("tar couldn't unpack the archive")
    } else {
        format!("tar: {detail}")
    })
}

// ── исполнитель планов применения (`crate::themes::ApplyStep`) ──────────────

/// Живая файловая система для санитайзера GTK/KDE.
struct LiveGtkFs;

impl crate::gtk_sanitizer::GtkFs for LiveGtkFs {
    fn is_dir(&mut self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        read_text_lossy(path)
    }
}

/// КРИТИЧНО после ЛЮБОГО применения: починить битые ссылки в
/// `gtk-{3,4}.0/settings.ini` и в KDE-конфигах — порт `sanitize_all`.
///
/// Без этого GTK4-приложения падают в петлю поиска иконок у темы, которой на
/// диске уже нет. Решения (что битое и чем заменить) целиком за
/// [`crate::gtk_sanitizer`]; здесь только чтение, запись и `kwriteconfig6`.
/// Возвращает список правок — их число уходит в [`gtk_sanitizer_message`].
pub fn live_gtk_sanitize(home: &str) -> Vec<String> {
    use crate::gtk_sanitizer as gsan;

    let cfg = crate::utils::config_home(xdg_config_home().as_deref(), home);
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    let mut fs = LiveGtkFs;
    let mut changes = gtk_ini_sanitize_at(home, &cfg, &data);

    let kde = gsan::read_kde_ini(read_text_lossy(&format!("{cfg}/kdeglobals")).as_deref());
    let kcm = gsan::read_kcminput_cursor(read_text_lossy(&format!("{cfg}/kcminputrc")).as_deref());
    let edits = gsan::plan_kde_edits(&mut fs, &kde, &kcm, home, &data);
    if !edits.is_empty() {
        let Some(tool) = crate::utils::resolve_tool("kwriteconfig6", &[], path_env().as_deref())
        else {
            return changes;
        };
        for edit in edits {
            if run_ok(&gsan::kwriteconfig_argv(&tool, &edit), None) {
                changes.push(edit.change);
            }
        }
    }
    changes
}

/// Починка `gtk-{3,4}.0/settings.ini` с явными каталогами — половина
/// [`live_gtk_sanitize`], которая не запускает чужих программ; сверки зовут
/// её на подставном HOME.
pub fn gtk_ini_sanitize_at(home: &str, cfg: &str, data: &str) -> Vec<String> {
    use crate::gtk_sanitizer as gsan;
    let mut fs = LiveGtkFs;
    let mut changes: Vec<String> = Vec::new();
    for ver in gsan::GTK_VERSIONS {
        let ini = gsan::gtk_ini_path(cfg, ver);
        if !Path::new(&ini).is_file() {
            continue;
        }
        let Some(text) = read_text_lossy(&ini) else {
            // Файл есть, но не читается — переписывать НЕЛЬЗЯ: пересборка
            // текста без исходника снесла бы все остальные ключи.
            continue;
        };
        let settings = gsan::read_ini_settings(Some(&text));
        for edit in gsan::plan_gtk_ini_edits(&mut fs, &ini, &settings, home, data) {
            // Файл перечитывается на каждый ключ — как в оригинале.
            let existing = read_text_lossy(&ini);
            let content = gsan::write_ini_setting(existing.as_deref(), &edit.key, &edit.value);
            if std::fs::write(&ini, content).is_ok() {
                changes.push(edit.change);
            }
        }
    }
    changes
}

/// Замечания санитайзера о ссылках, которые он НЕ правит: звуковая тема из
/// `gtk-{3,4}.0/settings.ini`, которой нет на диске
/// ([`crate::gtk_sanitizer::plan_gtk_ini_notes`]). Только чтение — ни один
/// файл не меняется; строки идут в журнал при старте окна.
pub fn live_gtk_notes(home: &str) -> Vec<String> {
    let cfg = crate::utils::config_home(xdg_config_home().as_deref(), home);
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    gtk_notes_at(home, &cfg, &data)
}

/// Тело [`live_gtk_notes`] с явными каталогами — сверки зовут его на
/// подставном HOME, не трогая окружение процесса.
pub fn gtk_notes_at(home: &str, cfg: &str, data: &str) -> Vec<String> {
    use crate::gtk_sanitizer as gsan;
    let mut fs = LiveGtkFs;
    let mut notes = Vec::new();
    for ver in gsan::GTK_VERSIONS {
        let ini = gsan::gtk_ini_path(cfg, ver);
        let Some(text) = read_text_lossy(&ini) else { continue };
        let settings = gsan::read_ini_settings(Some(&text));
        notes.extend(gsan::plan_gtk_ini_notes(&mut fs, &ini, &settings, home, data));
    }
    notes
}

/// Прочитать размер курсора (`gsettings get …cursor-size`), дефолт «24».
fn cursor_size(run: &dyn Fn(&[String], Option<u64>) -> RunReport) -> String {
    let cmd = crate::themes::cursor_size_get_cmd();
    let report = run(&cmd.argv, cmd.timeout);
    if report.returncode == Some(0) {
        let raw = crate::utils::decode_text(&report.stdout);
        let value = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "24".to_string()
}

/// Окружение для выбора стиля Qt (Kvantum и «Стиль приложений» — одно на
/// двоих): работающий переключатель стиля Qt ищется в `PATH`, пусто — идём
/// запасным путём `QT_STYLE_OVERRIDE`.
fn qt_style_env<'a>(
    de: &'a str,
    home: &'a str,
    xdg: Option<&'a str>,
) -> crate::themes::KvantumEnv<'a> {
    let platform_theme = ["qt6ct", "qt5ct"].into_iter().find(|t| have(t)).unwrap_or("");
    // Нынешние значки окружения — для нового конфига qt6ct (Д2). Читаются,
    // только когда этот конфиг и правда понадобится.
    let icon_theme = if !platform_theme.is_empty()
        && crate::themes::qt_style_way(de) == crate::themes::QtStyleWay::PlatformTheme
    {
        current_icon_theme(de, home, xdg)
    } else {
        String::new()
    };
    crate::themes::KvantumEnv {
        de_id: de,
        home,
        xdg_config_home: xdg.unwrap_or(""),
        platform_theme,
        icon_theme,
    }
}

/// Тема значков, которую окружение отдаёт программам сейчас (пусто —
/// прочитать не вышло). Источник решает ядро.
fn current_icon_theme(de: &str, home: &str, xdg: Option<&str>) -> String {
    use crate::themes::IconThemeSource as S;
    match crate::themes::current_icon_theme_source(de, home, xdg) {
        S::Cmd(c) => {
            if !have(&c.argv[0]) {
                return String::new();
            }
            let r = run_capture(&c.argv, c.timeout);
            if r.returncode != Some(0) {
                return String::new();
            }
            crate::themes::parse_icon_theme_output(&crate::utils::decode_text(&r.stdout))
        }
        S::GtkIni(path) => crate::gtk_sanitizer::read_ini_settings(read_text_lossy(&path).as_deref())
            .get("gtk-icon-theme-name")
            .cloned()
            .unwrap_or_default(),
    }
}

/// Исполнить план применения темы — точный порт `ThemeManager._exec_apply_plan`.
///
/// Флаг `ok` ведётся строго по семантике тегов ядра: его задаёт `Primary`,
/// поднимает удавшийся `IfNotOk`, удавшаяся запись `WriteSetsOk` и
/// безусловный `ForceOk`; `BestEffort` его не трогает. Порядок и argv — из
/// плана, своих решений здесь нет.
pub fn exec_apply_plan(steps: &[ApplyStep]) -> bool {
    exec_apply_plan_report(steps).ok
}

/// То же, что [`exec_apply_plan`], но с подробностями для честного
/// сообщения о курсоре: сменилось ли сразу и что стало с конфигом
/// композитора.
pub fn exec_apply_plan_report(steps: &[ApplyStep]) -> crate::themes::CursorReport {
    exec_apply_plan_report_in(steps, crate::dde::treeland_session_live())
}

/// То же с явным признаком сеанса Treeland (причина стопа
/// [`ApplyStep::DdeTreelandGate`]).
fn exec_apply_plan_report_in(steps: &[ApplyStep], treeland: bool) -> crate::themes::CursorReport {
    let plan: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    let ctx = crate::install_plan::Ctx::live(true, false, None, treeland);
    match crate::install_plan::exec(&plan, &mut Vec::new(), &ctx) {
        Ok(report) => report.apply,
        // Не удалась запись: всё, что нажатие успело записать, исполнитель
        // уже вернул — человеку причина.
        Err(f) => press_failed_report(&f),
    }
}

/// Итог нажатия «Применить», которое оборвал сбой файловой записи: не
/// применено, причина — какой файл не записался (записи нажатия уже
/// возвращены общим исполнителем).
pub(crate) fn press_failed_report(f: &crate::install_plan::Failure) -> crate::themes::CursorReport {
    crate::themes::CursorReport {
        ok: false,
        reason: Some(trf(
            "Not applied: %1. The files this action had changed are back as they were.",
            &[&crate::install_plan::failure_text(f)],
        )),
        ..Default::default()
    }
}

/// Прогон слоя включения общего плана ([`crate::install_plan::exec`]):
/// флаг `ok` ведётся строго по семантике тегов ядра — его задаёт `Primary`,
/// поднимает удавшийся `IfNotOk`, удавшаяся запись `WriteSetsOk` и
/// безусловный `ForceOk`; `BestEffort` его не трогает. Порядок и argv — из
/// плана, своих решений здесь нет.
pub(crate) struct ApplyRun<'r> {
    report: crate::themes::CursorReport,
    ok: bool,
    /// Записано ли что-то в `environment.d` — тогда переменные надо
    /// отзеркалить в сеансы, которые этот каталог не читают (Д1).
    env_written: bool,
    /// Идёт ли сеанс Treeland (причина стопа [`ApplyStep::DdeTreelandGate`]).
    treeland: bool,
    /// Запуск команд и поиск программ — общие для всего плана
    /// ([`crate::install_plan::Ctx::run`], [`crate::install_plan::Ctx::have`]).
    run: &'r dyn Fn(&[String], Option<u64>) -> RunReport,
    have: &'r dyn Fn(&str) -> bool,
    /// Отзеркалить `environment.d` ([`crate::install_plan::Ctx::sync_env`]).
    sync_env: &'r dyn Fn(&mut crate::install_plan::Undo),
    /// Файлы под [`ApplyStep::Guard`], которых до команд не было: после
    /// прогона заведённое командами отмечается в откате ([`Self::settle`]).
    guarded: Vec<(PathBuf, PathBuf)>,
}

impl<'r> ApplyRun<'r> {
    pub(crate) fn new(
        treeland: bool,
        run: &'r dyn Fn(&[String], Option<u64>) -> RunReport,
        have: &'r dyn Fn(&str) -> bool,
        sync_env: &'r dyn Fn(&mut crate::install_plan::Undo),
    ) -> Self {
        ApplyRun { report: Default::default(), ok: false, env_written: false, treeland, run, have, sync_env, guarded: Vec::new() }
    }

    fn ok_run(&self, argv: &[String], timeout_secs: Option<u64>) -> bool {
        (self.run)(argv, timeout_secs).returncode == Some(0)
    }

    /// Исполнить шаг; `Ok(false)` — план остановлен (Treeland).
    ///
    /// Файловые записи (ini, наши файлы, конфиг композитора) идут под
    /// откатом `undo` (шаг 5а слияния установщиков): не удалась запись —
    /// `Err`, и общий исполнитель возвращает всё, что это нажатие успело
    /// записать («всё или ничего» внутри одного нажатия). Прежде ошибка
    /// записи молча пропускалась, и нажатие оставляло конфиги наполовину.
    pub(crate) fn step(
        &mut self,
        step: &ApplyStep,
        undo: &mut crate::install_plan::Undo,
    ) -> Result<bool, crate::install_plan::Failure> {
        use crate::install_plan::Failure;
        fn write(path: &str) -> impl FnOnce(std::io::Error) -> Failure + '_ {
            move |e| Failure::Write { path: path.to_string(), err: e.to_string() }
        }
        fn mkdir(path: &str) -> impl FnOnce(std::io::Error) -> Failure + '_ {
            move |e| Failure::MakeDirs { path: path.to_string(), err: e.to_string() }
        }
        match step {
            ApplyStep::DdeTreelandGate => {
                self.report.reason = Some(crate::dde::refusal(self.treeland));
                self.ok = false;
                return Ok(false);
            }
            // Команды, которые меняют значения стола (`gsettings`, `dconf`,
            // `xfconf-query`), идут через откат: прежнее значение снимается до
            // них ([`crate::install_plan::Undo::run_kept`]).
            ApplyStep::Primary(c) => self.ok = undo.run_kept(self.run, &c.argv, c.timeout).returncode == Some(0),
            ApplyStep::BestEffort(c) => {
                let _ = undo.run_kept(self.run, &c.argv, c.timeout);
            }
            ApplyStep::IfOk(c) => {
                if self.ok {
                    let _ = undo.run_kept(self.run, &c.argv, c.timeout);
                }
            }
            ApplyStep::IfNotOk(c) => {
                if !self.ok && undo.run_kept(self.run, &c.argv, c.timeout).returncode == Some(0) {
                    self.ok = true;
                }
            }
            ApplyStep::GtkIni { path, key, value } => {
                let existing = if Path::new(path).is_file() { read_text_lossy(path) } else { None };
                let content = crate::themes::update_gtk_settings_ini(existing.as_deref(), key, value);
                undo.write_text(Path::new(path), &content, false).map_err(write(path))?;
            }
            ApplyStep::HyprctlCursor { theme, size } => {
                if (self.have)("hyprctl") {
                    let size = size.clone().unwrap_or_else(|| cursor_size(self.run));
                    let [set, env] = crate::themes::hyprctl_cursor_cmds(theme, &size);
                    let live = self.ok_run(&set.argv, set.timeout);
                    let _ = self.ok_run(&env.argv, env.timeout);
                    let size_env = vec![
                        "hyprctl".to_string(),
                        "keyword".to_string(),
                        "env".to_string(),
                        format!("XCURSOR_SIZE,{size}"),
                    ];
                    let _ = self.ok_run(&size_env, None);
                    self.report.live = Some(self.report.live.unwrap_or(false) || live);
                } else {
                    self.report.live.get_or_insert(false);
                }
            }
            ApplyStep::Live(c) => {
                let live = (self.have)(&c.argv[0]) && self.ok_run(&c.argv, c.timeout);
                self.report.live = Some(self.report.live.unwrap_or(false) || live);
            }
            ApplyStep::ForceOk => self.ok = true,
            ApplyStep::WriteIfOk { dir, path, content } => {
                if self.ok {
                    undo.make_dirs(Path::new(dir)).map_err(mkdir(dir))?;
                    undo.write_text(Path::new(path), content, false).map_err(write(path))?;
                    self.env_written |= crate::compositor_autostart::is_environment_d_path(path);
                }
            }
            ApplyStep::WriteSetsOk { dir, path, content } => {
                undo.make_dirs(Path::new(dir)).map_err(mkdir(dir))?;
                undo.write_text(Path::new(path), content, true).map_err(write(path))?;
                self.ok = true;
                self.env_written |= crate::compositor_autostart::is_environment_d_path(path);
            }
            ApplyStep::IniSetIfOk { path, section, key, value } => {
                if self.ok {
                    // Файл ведём не мы — правка одного ключа, не переписывание.
                    let existing = if Path::new(path).exists() { read_text_lossy(path) } else { None };
                    let content = crate::defaults::set_ini_value(existing.as_deref(), section, key, value);
                    undo.write_text(Path::new(path), &content, false).map_err(write(path))?;
                }
            }
            ApplyStep::IniSetGuarded { path, section, key, value, only_if_file, keep_existing } => {
                if self.ok {
                    let existing = if Path::new(path).exists() { read_text_lossy(path) } else { None };
                    if let Some(text) = crate::themes::ini_guarded_text(
                        existing.as_deref(),
                        section,
                        key,
                        value,
                        *only_if_file,
                        *keep_existing,
                    ) {
                        undo.write_text(Path::new(path), &text, false).map_err(write(path))?;
                    }
                }
            }
            ApplyStep::WmConfig(plan) => {
                let (outcome, path) = exec_wm_config(plan, self.run, self.have, undo);
                self.report.wm = Some((plan.wm, outcome, path));
            }
            ApplyStep::GtkIniUnsetIfOk { path, key } => {
                if self.ok {
                    if let Some(text) = read_text_lossy(path)
                        .and_then(|t| crate::themes::remove_gtk_settings_key(&t, key))
                    {
                        undo.write_text(Path::new(path), &text, true).map_err(write(path))?;
                    }
                }
            }
            ApplyStep::RemoveIfOk { path } => {
                if self.ok {
                    undo.remove_file(Path::new(path))
                        .map_err(|e| Failure::Remove { path: path.clone(), err: e.to_string() })?;
                }
            }
            ApplyStep::Guard { paths } => {
                let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
                let missing = undo.guard_external(&paths);
                self.guarded.extend(missing);
            }
            ApplyStep::ReadBackSetsOk { read, expect } => {
                let report = (self.run)(&read.argv, read.timeout);
                let raw = if report.returncode == Some(0) {
                    crate::utils::decode_text(&report.stdout)
                } else {
                    String::new()
                };
                self.ok = report.returncode == Some(0) && crate::qt_settings::parse_sound_theme(&raw) == *expect;
            }
        }
        Ok(true)
    }

    /// Отметить в откате файлы, заведённые командами после
    /// [`ApplyStep::Guard`] (зовётся и при сбое посреди плана — тогда откат
    /// снимет и их).
    pub(crate) fn settle(&mut self, undo: &mut crate::install_plan::Undo) {
        undo.after_external(std::mem::take(&mut self.guarded));
    }

    pub(crate) fn finish(mut self, undo: &mut crate::install_plan::Undo) -> crate::themes::CursorReport {
        self.settle(undo);
        if self.env_written {
            (self.sync_env)(undo);
        }
        self.report.ok = self.ok;
        self.report
    }
}

/// Исполнить [`crate::themes::WmConfigPlan`]: наш файл + управляемый блок в
/// конфиге композитора, с копией до первой правки и откатом, если проверка
/// самого композитора отвергла результат. Отдаёт итог и путь конфига.
///
/// Записи — под откатом `undo` нажатия: сбой ДАЛЬШЕ в том же нажатии
/// возвращает и их. Сам `Failed` нажатие не роняет (итог сообщается
/// человеку отдельно, [`crate::themes::CursorReport::wm`]).
fn exec_wm_config(
    plan: &crate::themes::WmConfigPlan,
    run: &dyn Fn(&[String], Option<u64>) -> RunReport,
    have: &dyn Fn(&str) -> bool,
    undo: &mut crate::install_plan::Undo,
) -> (crate::themes::WmConfigOutcome, String) {
    use crate::themes::WmConfigOutcome as O;
    let first_path = plan.candidates.first().map(|c| c.path.clone()).unwrap_or_default();
    // 1. Версия: старый композитор не прочтёт строку подключения.
    if let Some(gate) = &plan.min_version {
        if have(&gate.argv[0]) {
            let out = run(&gate.argv, Some(10));
            let text = crate::utils::decode_text(&out.stdout);
            if let Some(v) = crate::themes::parse_wm_version(&text) {
                if v < gate.min {
                    return (O::TooOld, first_path);
                }
            }
        }
    }
    // 2. Какой конфиг композитор прочтёт.
    let mut chosen = plan.candidates.iter().find(|c| Path::new(&c.path).is_file());
    if chosen.is_none() {
        if let Some(f) = plan.foreign.iter().find(|f| Path::new(f.as_str()).is_file()) {
            return (O::Foreign, f.clone());
        }
        match &plan.seed {
            Some(seed) if Path::new(&seed.from).is_file() => {
                let Some(target) = plan.candidates.get(seed.candidate) else {
                    return (O::NoConfig, first_path);
                };
                let Some(text) = read_text_lossy(&seed.from) else {
                    return (O::Failed, target.path.clone());
                };
                if undo.write_text(Path::new(&target.path), &text, true).is_err() {
                    return (O::Failed, target.path.clone());
                }
                chosen = Some(target);
            }
            _ => return (O::NoConfig, first_path),
        }
    }
    let Some(file) = chosen else {
        return (O::NoConfig, first_path);
    };
    let path = file.path.clone();
    // 3. Наш файл, затем строка подключения.
    if undo.write_text(Path::new(&file.own_path), &file.own_content, true).is_err() {
        return (O::Failed, path);
    }
    let Some(before) = read_text_lossy(&path) else {
        return (O::Failed, path);
    };
    let after = crate::themes::upsert_managed_block(
        &before,
        file.syntax.comment(),
        crate::themes::CURSOR_BLOCK_TAG,
        &file.include_line,
    );
    if after == before {
        return (O::Written, path);
    }
    let backup = format!("{path}.rexon-bak");
    if !Path::new(&backup).exists() && undo.write_text(Path::new(&backup), &before, true).is_err() {
        return (O::Failed, path);
    }
    // 4. Проверка самим композитором — до и после.
    let validator = plan.validate.first().filter(|bin| have(bin)).is_some();
    let valid = |p: &str| -> bool {
        let mut argv = plan.validate.clone();
        argv.push(p.to_string());
        run(&argv, Some(30)).returncode == Some(0)
    };
    let was_valid = validator && valid(&path);
    if undo.write_text(Path::new(&path), &after, true).is_err() {
        return (O::Failed, path);
    }
    if was_valid && !valid(&path) {
        // Копия прежнего уже снята откатом — здесь простая запись.
        let _ = crate::utils::atomic_write(&path, &before);
        return (O::RolledBack, path);
    }
    (O::Written, path)
}

/// Текущая тема (`size == false`) или размер курсора в этом окружении —
/// первый ответ по списку источников ядра ([`crate::themes::cursor_queries`]).
pub(crate) fn live_cursor_value(de: &str, home: &str, xdg: Option<&str>, size: bool) -> Option<String> {
    use crate::themes::CursorQuery as Q;
    for q in crate::themes::cursor_queries(de, home, xdg, size) {
        let value = match q {
            Q::Cmd(c) => {
                if !have(&c.argv[0]) {
                    continue;
                }
                let out = run_capture(&c.argv, c.timeout);
                if out.returncode != Some(0) {
                    continue;
                }
                crate::themes::clean_cursor_value(&crate::utils::decode_text(&out.stdout))
            }
            Q::Ini { path, section, key } => read_text_lossy(&path)
                .and_then(|t| crate::themes::ini_key_value(&t, &section, &key)),
            Q::EnvFile { path, key } => read_text_lossy(&path)
                .and_then(|t| crate::themes::env_file_key_value(&t, &key)),
            Q::Env(name) => std::env::var(&name).ok().and_then(|v| crate::themes::clean_cursor_value(&v)),
        };
        let value = match value {
            Some(v) if size && crate::themes::parse_cursor_size(&v).is_none() => None,
            v => v,
        };
        if value.is_some() {
            return value;
        }
    }
    None
}

/// argv «KWin, перечитай конфиг» (порт `utils.kwin_reconfigure`) — первым
/// инструментом шины, который есть. `None` — ни одного нет.
///
/// Идём на сессионную шину, а не списком `qdbus`/`qdbus6` из
/// [`aurorae_plan`]: этих бинарей на большинстве систем нет вовсе, а
/// `dbus-send` есть везде, где есть сама шина. Порядок инструментов и сборка
/// argv — общие ([`crate::utils`]), своей копии здесь нет.
fn kwin_reconfigure_argv() -> Option<Vec<String>> {
    let path = path_env();
    crate::utils::DBUS_TOOL_PREFERENCE.iter().find_map(|(tool, kind)| {
        let exe = crate::utils::resolve_tool(tool, &[], path.as_deref())?;
        let (dest, obj, method) = crate::utils::KWIN_RECONFIGURE;
        Some(crate::utils::dbus_call_argv(*kind, &exe, dest, obj, method, &[]))
    })
}

// ── План нажатия «Применить» (шаг 5а) ───────────────────────────────────────

/// План одного нажатия «Применить».
#[derive(Debug, Clone, PartialEq)]
pub enum PressPlan {
    /// Шаги общего плана ([`crate::install_plan::exec`]); при успехе —
    /// запись в историю тем `(вид, стол)`, если она у действия есть.
    Steps {
        steps: Vec<crate::install_plan::Step>,
        history: Option<(&'static str, String)>,
        /// Что сказать человеку после прогона.
        say: PressSay,
    },
    /// Кнопка раздела окружения (тема, дополнение): шаги общего плана,
    /// команды — через подменяемую машину `de_apply`; итог — не коды
    /// команд, а сверка «окружение отдаёт после записи»
    /// ([`crate::de_apply::check`]).
    Desktop { steps: Vec<crate::install_plan::Step>, check: crate::de_apply::Check },
    /// Отказ до первого шага: ничего не запускается и не пишется.
    /// `None` — общее «не применено» ([`apply_message`]).
    Refused(Option<String>),
    /// Делать нечего — успех (шрифты, плазмоиды…).
    Nothing,
    /// Нажатие пока исполняется своим путём, не общим планом: цвета в
    /// Plasma (`plasma-apply-colorscheme`, затем снятие нашего ключа из
    /// `kdeglobals` — его текст известен только ПОСЛЕ команды), обои
    /// рабочего стола, Conky (запуск процессов, `de_apply`), экран входа и
    /// загрузки (помощник от root).
    Own,
}

/// Текст итога нажатия, собранного планом ([`PressPlan::Steps`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressSay {
    /// Общее «применено»/«не применено» ([`apply_message`]); при неудаче —
    /// причина, если она известна.
    Plain,
    /// Курсор: что сменилось сразу и что с конфигом композитора
    /// ([`crate::themes::cursor_apply_message`]).
    Cursor,
    /// Удача — этот текст (цвета слоями Rexon: что получили Qt-программы,
    /// [`crate::shell_palette::layer_applied_msg`]); неудача — причина.
    Done(String),
    /// Удача — `done`; неудача — причина из прогона (сбой записи), а без
    /// неё — `failed` (обои блокировки, звук без команды проверки).
    Pair { done: String, failed: String },
}

/// Живые сведения для звуковой темы ([`PressFacts::sound`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundFacts {
    /// Тема с таким именем есть в каталогах звуковых тем.
    pub exists: bool,
    /// Найденные `kwriteconfig6`/`kreadconfig6` (пусто — нет).
    pub kwrite: String,
    pub kread: String,
    pub has_gsettings: bool,
}

/// Живые сведения для обоев экрана блокировки ([`PressFacts::lock`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockFacts {
    /// Каталог раздела (там лежит картинка).
    pub install_dir: String,
    /// Картинка `install_dir/<тема>` — файл.
    pub is_file: bool,
    /// Сеанс Treeland (Deepin: обои отдаёт Центр управления; вне Treeland
    /// у Deepin обоев блокировки нет).
    pub treeland: bool,
}

/// Файлы, которые перепишут команды `kwriteconfig` из `cmds`
/// (`--file имя` → `<config>/имя`), по разу, в порядке команд. Их «что
/// было» снимает [`crate::themes::ApplyStep::Guard`]: сами команды мимо
/// отката.
pub fn kconfig_files<'a>(cmds: impl IntoIterator<Item = &'a [String]>, config: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for argv in cmds {
        let is_kwrite = argv.first().is_some_and(|a| basename(a).starts_with("kwriteconfig"));
        let file = argv.iter().position(|a| a == "--file").and_then(|i| argv.get(i + 1));
        if let (true, Some(file)) = (is_kwrite, file) {
            let path = if file.starts_with('/') { file.clone() } else { format!("{}/{file}", config.trim_end_matches('/')) };
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// Живые сведения для [`press_plan_with`] — всё, что требует опроса машины,
/// снято заранее ([`LiveInstaller::press_plan`]); в проверках — подставное.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PressFacts {
    pub de: String,
    pub home: String,
    pub xdg_config_home: Option<String>,
    /// У глобальной темы есть `metadata.json` (Plasma 6 её прочтёт).
    pub global_theme_ok: bool,
    /// Тема значков валидна ([`crate::themes::is_valid_icon_theme`]).
    pub icons_valid: bool,
    /// Тема GTK тёмная ([`crate::themes::gtk_theme_is_dark`]).
    pub gtk_dark: bool,
    /// Тема GTK в программах GTK 4/libadwaita ([`crate::gtk4_theme`]);
    /// `None` — не собрано, план прежний.
    pub gtk4: Option<crate::gtk4_theme::Facts>,
    /// Переключатель стиля Qt в системе (`qt6ct`/`qt5ct`, пусто — нет).
    pub qt_platform_theme: String,
    /// Тема значков окружения сейчас — для нового конфига qt6ct.
    pub qt_icon_theme: String,
    /// argv «KWin, перечитай конфиг»; `None` — звать нечем.
    pub kwin_reconfigure: Option<Vec<String>>,
    /// `library` темы Aurorae SVG на этой машине
    /// ([`crate::kdecoration::svg_library`]); пусто — прежний движок.
    pub aurorae_library: String,
    /// Тема курсоров безопасна (`Inherits` не ведёт в никуда, порт
    /// `_cursor_preflight`).
    pub cursor_safe: bool,
    /// Тема курсоров — курсор окружения по умолчанию (Adwaita в GNOME и
    /// Cinnamon): небезопасная такая — не отказ, а сброс родного ключа.
    pub cursor_standard: bool,
    /// Размер курсора: при выборе темы — тот, что сейчас (он нужен целиком:
    /// переменные окружения, конфиг композитора), при смене размера — новый.
    pub cursor_size: Option<u32>,
    /// Цвета вне Plasma: план слоёв Rexon
    /// ([`crate::shell_palette::scheme_layer_press`]); `Err` — отказ (схемы
    /// нет), `None` — не собран.
    pub colors: Option<Result<crate::shell_palette::SchemePress, String>>,
    /// Звуковая тема: есть ли она и чем писать; `None` — не собрано.
    pub sound: Option<SoundFacts>,
    /// Обои экрана блокировки: где картинка; `None` — не собрано.
    pub lock: Option<LockFacts>,
    /// Раздел окружения: план кнопки ([`crate::de_apply::press_plan`]);
    /// `None` — не собран (своим путём).
    pub desktop: Option<crate::de_apply::Plan>,
    /// Глобальная тема: что отметил человек в окне «Применить «X»?» и что
    /// несёт тема; `None` — окна не было, применяется оформление
    /// (`plasma-apply-lookandfeel -a`, как прежде).
    pub global: Option<crate::themes::GlobalPress>,
}

/// План нажатия «Применить» действия `action` для темы `theme` — чистая
/// функция: построители те же, что звало прежнее `apply`, порядок и argv
/// не менялись (сверка — `appearance_apply_plans_parity.rs`).
pub fn press_plan_with(action: &ApplyAction, theme: &str, facts: &PressFacts) -> PressPlan {
    use crate::install_plan::Step;
    use crate::themes::Cmd;
    let de = facts.de.as_str();
    let home = facts.home.as_str();
    let xdg = facts.xdg_config_home.as_deref();
    let qt_env = || crate::themes::KvantumEnv {
        de_id: de,
        home,
        xdg_config_home: xdg.unwrap_or(""),
        platform_theme: &facts.qt_platform_theme,
        icon_theme: facts.qt_icon_theme.clone(),
    };
    let steps = |plan: Vec<crate::themes::ApplyStep>| plan.into_iter().map(Step::from).collect::<Vec<_>>();
    let with_history = |plan: crate::themes::Plan, de: &str| PressPlan::Steps {
        steps: steps(plan.steps),
        history: Some((plan.history_kind, de.to_string())),
        say: PressSay::Plain,
    };
    let primary = |argv: Vec<String>| Step::from(crate::themes::ApplyStep::Primary(Cmd { argv, timeout: None }));
    match action {
        ApplyAction::KdeGlobal => {
            if !facts.global_theme_ok {
                return PressPlan::Refused(Some(trf(
                    "The theme “%1” uses the old Plasma 5 format (metadata.desktop \
                     only, no metadata.json) — Plasma 6 can't apply it. You need a \
                     version made for Plasma 6.",
                    &[&theme],
                )));
            }
            let Some(press) = &facts.global else {
                return with_history(crate::themes::apply_kde_global_plan(theme), "kde");
            };
            // Части, отмеченные в окне «Применить «X»?» (вне Plasma — те, что
            // переносятся на этот стол).
            let Some(got) = crate::themes::apply_global_choice_plan(theme, press, de) else {
                return PressPlan::Refused(Some(tr("Nothing is selected to apply.")));
            };
            let skipped = got.skipped.join("; ");
            if got.plan.steps.is_empty() {
                return PressPlan::Refused(Some(if skipped.is_empty() {
                    tr("Nothing is selected to apply.")
                } else {
                    trf("Nothing from the Global Theme was applied: %1.", &[&skipped])
                }));
            }
            match with_history(got.plan, de) {
                PressPlan::Steps { steps, history, .. } if !skipped.is_empty() => PressPlan::Steps {
                    steps,
                    history,
                    say: PressSay::Pair {
                        done: trf("Global Theme applied, except: %1.", &[&skipped]),
                        failed: apply_message(action, false),
                    },
                },
                other => other,
            }
        }
        ApplyAction::KdePlasma => with_history(crate::themes::apply_kde_plasma_plan(theme), "kde"),
        ApplyAction::Icons => {
            match crate::themes::apply_icon_plan_xdg(theme, de, home, xdg, facts.icons_valid) {
                Some(plan) => with_history(plan, de),
                None => PressPlan::Refused(None),
            }
        }
        ApplyAction::Kvantum => with_history(crate::themes::apply_kvantum_plan(theme, &qt_env()), "kde"),
        ApplyAction::Gtk => {
            let plan = with_history(crate::themes::apply_gtk_plan_scheme(theme, de, home, xdg, Some(facts.gtk_dark)), de);
            match (plan, &facts.gtk4) {
                (PressPlan::Steps { mut steps, history, say }, Some(gtk4)) => {
                    crate::gtk4_theme::extend_press(&mut steps, theme, de, gtk4);
                    PressPlan::Steps { steps, history, say }
                }
                (plan, _) => plan,
            }
        }
        // Модуль рамки (`library`), затем тема, затем (только при успехе)
        // KWin перечитывает конфиг. Без `library` тема Aurorae под Breeze не
        // включалась: KWin рисует модуль из `library`, а `theme` читает уже
        // он. Плитка встроенной рамки (Breeze, Oxygen) — `library/theme`
        // ([`crate::kdecoration::choice`]). Истории у рамок, стиля Qt и
        // заставок, как и прежде, нет.
        ApplyAction::Aurorae => {
            let library = if facts.aurorae_library.is_empty() {
                crate::kdecoration::AURORAE_V1
            } else {
                facts.aurorae_library.as_str()
            };
            let [write_library, write_theme] = crate::kdecoration::write_argv(theme, library);
            let mut plan = vec![
                primary(write_library),
                Step::from(crate::themes::ApplyStep::IfOk(Cmd { argv: write_theme, timeout: None })),
            ];
            if let Some(argv) = facts.kwin_reconfigure.clone() {
                plan.push(Step::from(crate::themes::ApplyStep::IfOk(Cmd { argv, timeout: Some(10) })));
            }
            PressPlan::Steps { steps: plan, history: None, say: PressSay::Plain }
        }
        ApplyAction::WidgetStyle => PressPlan::Steps {
            steps: steps(crate::themes::apply_qt_style_plan(theme, &qt_env()).steps),
            history: None,
            say: PressSay::Plain,
        },
        // Заставка — как модуль KDE: тема и сразу `Engine` (у «Нет» — `none`,
        // иначе `KSplashQML`, чтобы после «Нет» заставка вернулась).
        ApplyAction::Splash => PressPlan::Steps {
            steps: vec![
                primary(lnf_component_plan("splash", theme).argv),
                Step::from(crate::themes::ApplyStep::IfOk(Cmd { argv: splash_engine_plan(theme).argv, timeout: None })),
            ],
            history: None,
            say: if theme == SPLASH_NONE { PressSay::Done(splash_off_message()) } else { PressSay::Plain },
        },
        ApplyAction::Lockscreen => PressPlan::Steps {
            steps: vec![primary(lnf_component_plan("lockscreen", theme).argv)],
            history: None,
            say: PressSay::Plain,
        },
        ApplyAction::Noop => PressPlan::Nothing,
        // Небезопасная тема, которая и есть курсор окружения по умолчанию:
        // сброс родного ключа (имя темы нигде явно не пишется). Иначе
        // небезопасная — план не собирается: отказ без единой записи.
        ApplyAction::Cursor => {
            let size = facts.cursor_size;
            let standard = (!facts.cursor_safe && facts.cursor_standard)
                .then(|| crate::themes::standard_cursor_plan(de, size, home, xdg))
                .flatten();
            match standard
                .or_else(|| crate::themes::apply_cursor_plan_sized(theme, size, de, home, xdg, facts.cursor_safe))
            {
                Some(plan) => PressPlan::Steps {
                    steps: steps(plan.steps),
                    history: Some((plan.history_kind, de.to_string())),
                    say: PressSay::Cursor,
                },
                None => PressPlan::Refused(None),
            }
        }
        // Вне Plasma — слои Rexon общим планом; в Plasma — своим путём.
        ApplyAction::KdeColors => match crate::shell_palette::scheme_way(de) {
            crate::shell_palette::SchemeWay::Layer { .. } => match &facts.colors {
                Some(Ok(press)) => PressPlan::Steps {
                    steps: steps(press.steps.clone()),
                    history: Some(("kde-colors", de.to_string())),
                    say: PressSay::Done(press.applied_msg()),
                },
                Some(Err(m)) => PressPlan::Refused(Some(m.clone())),
                None => PressPlan::Refused(None),
            },
            _ => PressPlan::Own,
        },
        // Звуковая тема — общий движок звука (тот же, что у вкладки
        // «Настройки»): записи командами, затем прочитанное обратно имя.
        // Несуществующее имя не пишем вовсе: система молча играла бы
        // `freedesktop`, а вкладка говорила бы «применено» (живой случай
        // 03.10 — в kdeglobals стояла тема «Sounds», которой нет).
        ApplyAction::SoundTheme => {
            let Some(sf) = &facts.sound else { return PressPlan::Refused(None) };
            if !sf.exists {
                return PressPlan::Refused(Some(crate::qt_settings::sound_theme_missing_message(theme)));
            }
            let plan = crate::qt_settings::sound_theme_plan(de, theme, &sf.kwrite, &sf.kread, sf.has_gsettings);
            if plan.write.is_empty() {
                return PressPlan::Refused(Some(tr(
                    "Nothing to write the sound theme with: neither kwriteconfig6 nor \
                     gsettings is available.",
                )));
            }
            let config = crate::utils::config_home(xdg, home);
            let guard = kconfig_files(plan.write.iter().map(Vec::as_slice), &config);
            let mut v: Vec<Step> = Vec::new();
            if !guard.is_empty() {
                v.push(Step::from(crate::themes::ApplyStep::Guard { paths: guard }));
            }
            v.extend(plan.write.iter().map(|argv| {
                Step::from(crate::themes::ApplyStep::BestEffort(Cmd { argv: argv.clone(), timeout: None }))
            }));
            let say = match plan.verify {
                Some(read) => {
                    v.push(Step::from(crate::themes::ApplyStep::ReadBackSetsOk {
                        read: Cmd { argv: read, timeout: None },
                        expect: theme.to_string(),
                    }));
                    PressSay::Plain
                }
                None => PressSay::Pair {
                    done: apply_message(action, true),
                    failed: tr(
                        "Sound theme written, but there's nothing to check it with (no \
                         kreadconfig6).",
                    ),
                },
            };
            PressPlan::Steps { steps: v, history: None, say }
        }
        // Обои блокировки: картинка (по ней `ok`), затем вспомогательные
        // ключи — они идут и при неудаче, как у прежнего своего пути.
        // `kscreenlockerrc` перепишет `kwriteconfig6` мимо отката — его «что
        // было» берёт `Guard`.
        ApplyAction::LockWallpaper => {
            let Some(lf) = &facts.lock else { return PressPlan::Refused(None) };
            match lock_wallpaper_plan_in(de, lf.treeland, &lf.install_dir, theme, lf.is_file) {
                Err(e) => PressPlan::Refused(Some(e)),
                Ok(plan) => {
                    let config = crate::utils::config_home(xdg, home);
                    let cmds = std::iter::once(&plan.image).chain(&plan.extra);
                    let guard = kconfig_files(cmds.map(|c| c.argv.as_slice()), &config);
                    let mut v: Vec<Step> = Vec::new();
                    if !guard.is_empty() {
                        v.push(Step::from(crate::themes::ApplyStep::Guard { paths: guard }));
                    }
                    v.push(primary(plan.image.argv.clone()));
                    v.extend(plan.extra.iter().map(|c| {
                        Step::from(crate::themes::ApplyStep::BestEffort(Cmd { argv: c.argv.clone(), timeout: None }))
                    }));
                    PressPlan::Steps {
                        steps: v,
                        history: None,
                        say: PressSay::Pair { done: plan.success_message, failed: plan.failure_message },
                    }
                }
            }
        }
        // Раздел окружения: тема и дополнение — шаги общего плана со
        // сверкой окружения; Conky (процессы) — своим путём.
        ApplyAction::DeSection => match facts.desktop.clone() {
            Some(crate::de_apply::Plan::Refused(m)) => PressPlan::Refused(Some(m)),
            Some(crate::de_apply::Plan::Steps { steps, check }) => PressPlan::Desktop { steps, check },
            Some(crate::de_apply::Plan::Own) | None => PressPlan::Own,
        },
        ApplyAction::WallpaperBackground
        | ApplyAction::WallpaperPack
        | ApplyAction::SystemTheme => PressPlan::Own,
    }
}


// ── детекторы среды для системной установки ─────────────────────────────────

/// `(realpath симлинка display-manager.service, жива ли цель)` — порт
/// `_dm_symlink_target`. Висячий симлинк после `pacman -R sddm` даёт
/// `(цель, false)`, и решение по нему принимает ядро.
fn dm_symlink_target() -> (Option<String>, bool) {
    let link = Path::new(crate::system_plan::DM_SYMLINK);
    if link.symlink_metadata().is_err() && !link.exists() {
        return (None, false);
    }
    match std::fs::canonicalize(link) {
        Ok(target) => {
            let s = target.to_string_lossy().into_owned();
            let alive = target.exists();
            (Some(s), alive)
        }
        // Цель не разрешается — симлинк висячий: путь всё равно называем.
        Err(_) => (
            std::fs::read_link(link)
                .ok()
                .map(|t| t.to_string_lossy().into_owned()),
            false,
        ),
    }
}

/// Имя активного юнита экрана входа — порт `system.active_login_manager`.
///
/// Нужен для одного вопроса — рисует ли экран входа Plasma Login Manager
/// (Plasma 6.6+), который QML-темы SDDM НЕ грузит: установку такой темы надо
/// остановить до pkexec, раздел «Оформления» — показать другим, а тему SDDM
/// не отмечать активной.
///
/// ⚠️ Может опрашивать `systemctl` (до пяти юнитов по 5 с): звать в рабочем
/// потоке, НЕ в потоке интерфейса. Быстрый путь — живой симлинк
/// `display-manager.service`, тогда процессов нет вовсе.
pub fn active_login_manager() -> String {
    let (target, alive) = dm_symlink_target();
    let has_systemctl = have("systemctl");
    let is_enabled = |unit: &str| -> Option<String> {
        let argv = vec![
            "systemctl".to_string(),
            "is-enabled".to_string(),
            format!("{unit}.service"),
        ];
        let report = run_capture(&argv, Some(5));
        if matches!(report.outcome, RunOutcome::Error | RunOutcome::Timeout) {
            return None;
        }
        Some(crate::utils::decode_text(&report.stdout).trim().to_string())
    };
    crate::system_plan::active_login_manager_from(
        target.as_deref(),
        alive,
        has_systemctl,
        &is_enabled,
    )
}

// ============================================================================
// LiveInstaller — конвейеры вкладки «Оформление»
// ============================================================================

/// Живой установщик тем — исполнитель планов этого модуля.
pub struct LiveInstaller {
    /// Рабочий стол, под который применяем (пусто — определить самому).
    de: String,
    home: String,
    log: Box<dyn FnMut(&str) + Send>,
    /// Можно ли при «Применить» ставить недостающие иконки и курсор темы
    /// ([`LiveInstaller::allow_deps_install`]).
    deps_install: bool,
    /// Включать ли значки, на которые рассчитана тема
    /// ([`LiveInstaller::set_icons_choice`]); `None` — не решено, значки не
    /// трогаются.
    icons_choice: Option<bool>,
    /// Выбор окна «Применить «X»?» глобальной темы
    /// ([`LiveInstaller::set_global_choice`]); `None` — окна не было.
    global_choice: Option<crate::themes::GlobalChoice>,
}

impl LiveInstaller {
    /// `log` — строка журнала в интерфейс (у Python это `on_log`).
    ///
    /// Пустой `de` СПРАШИВАЕТ систему, а не считает, что это KDE: прежний
    /// жёсткий фолбэк уводил применение GTK-темы, иконок и курсоров на
    /// KDE-путь у всех, кто зовёт установщик без аргумента, — под GNOME это
    /// значило запись в `kdeglobals` вместо `gsettings`.
    pub fn new(de: &str, log: Box<dyn FnMut(&str) + Send>) -> Self {
        let de = if de.is_empty() {
            crate::system::detect_current_de(
                &env_str("XDG_CURRENT_DESKTOP"),
                &env_str("DESKTOP_SESSION"),
                !env_str("WAYLAND_DISPLAY").is_empty(),
            )
            .unwrap_or_else(|| "kde".to_string())
        } else {
            de.to_string()
        };
        LiveInstaller { de, home: live_home(), log, deps_install: false, icons_choice: None, global_choice: None }
    }

    /// Строка в журнал интерфейса.
    fn say(&mut self, message: &str) {
        (self.log)(message);
    }

    fn home_path(&self) -> PathBuf {
        PathBuf::from(&self.home)
    }

    // ── УСТАНОВКА ───────────────────────────────────────────────────────────

    /// Порт `ThemeInstaller.install` — `(получилось, сообщение, имена тем)`.
    ///
    /// НЕ применяет: применение — отдельный шаг (см. [`Self::install_and_apply`]).
    pub fn install(
        &mut self,
        item: &crate::models::ThemeItem,
        archive_path: &str,
    ) -> (bool, String, Vec<String>) {
        let home = self.home_path();
        let Some(cat) = crate::categories::category_by_id(&home, item.category_id) else {
            return (
                false,
                trf("Unknown category: %1", &[&item.category_id]),
                Vec::new(),
            );
        };
        if cat.requires_root {
            // SDDM/Plymouth: системные пути и активация требуют root —
            // отдельный путь через polkit-helper.
            return self.install_root(item, &cat, archive_path);
        }

        // Товар отдан одним голым файлом (не архивом): цветовая схема, шрифт,
        // тема Kvantum и обои самодостаточны, обёртки в zip/tar у части
        // товаров просто нет. Распаковывать нечего.
        let is_archive = is_archive_file(archive_path);
        let head = read_head(archive_path, 16);
        if single_file_accepted(
            &cat.apply_kind,
            is_archive,
            &head,
            &[&item.download_name, &item.download_url],
        ) {
            return self.install_single_file(item, &cat, archive_path);
        }

        let work = match mkdtemp_in(&std::env::temp_dir(), "rexon-ocs-") {
            Ok(p) => p,
            Err(e) => return (false, trf("Installation error: %1", &[&e]), Vec::new()),
        };
        let extract_dir = work.join("extracted");
        let extract_dir_s = extract_dir.to_string_lossy().into_owned();
        let result = match live_safe_extract(archive_path, &extract_dir_s) {
            Ok(()) => self.install_archive(item, &cat, &extract_dir_s),
            Err(e) => (false, trf("Couldn't unpack the archive: %1", &[&e]), Vec::new()),
        };
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    /// Установить уже распакованное (`extract_dir`) — та же «архивная»
    /// ветка, что у архива из магазина, без повторной распаковки. Нужна
    /// источникам, которые сами отбирают, что ставить («Установить из
    /// файла…», `local_install`).
    pub fn install_extracted(
        &mut self,
        item: &crate::models::ThemeItem,
        extract_dir: &str,
    ) -> (bool, String, Vec<String>) {
        let home = self.home_path();
        let Some(cat) = crate::categories::category_by_id(&home, item.category_id) else {
            return (
                false,
                trf("Unknown category: %1", &[&item.category_id]),
                Vec::new(),
            );
        };
        // Системный вид (SDDM, Plymouth) — тем же помощником, что у архива
        // из магазина, с теми же проверками до окна пароля. Помощник берёт
        // только каталог под `/tmp`: так и распаковывает «Установить из
        // файла…» (`crate::local_install`).
        if cat.requires_root {
            let env = Self::live_root_env(&cat.apply_kind);
            if let Some(pre) = root_preflight(&env, &cat) {
                return (false, pre.message, Vec::new());
            }
            return self.install_root_staged(item, &cat, &env, extract_dir);
        }
        self.install_archive(item, &cat, extract_dir)
    }

    /// «Архивная» ветка установки: план из [`plan_install`], исполнение здесь.
    fn install_archive(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        extract_dir: &str,
    ) -> (bool, String, Vec<String>) {
        let home = self.home_path();
        // Снимок обязан покрыть и каталог категории, и ВСЕ каталоги, куда
        // установка может уехать после переклассификации, — иначе уже стоящая
        // тема не будет замечена и не будет снята перед копией.
        let mut dest_dirs = vec![cat.install_dir.clone()];
        // Рамки окон и оболочки ложатся и внутрь темы GTK с тем же именем
        // (`plan_nested_part`): её каталог — тоже в снимок.
        if matches!(de_theme_marker(&cat.apply_kind), Some(DeThemeMarker::Subdir(_))) {
            dest_dirs.push(crate::installed_scan::xdg_themes_dir(&self.home));
        }
        for actual in ACTUAL_TYPES_WITH_DIR {
            if let Some(dir) = actual_type_dir(&home, actual) {
                if !dest_dirs.contains(&dir) {
                    dest_dirs.push(dir);
                }
            }
        }
        // Виджет Conky кладёт шрифты в `~/.local/share/fonts/rexon-conky-<имя>`:
        // занята ли эта папка чужим, план узнаёт по снимку.
        if cat.apply_kind == "conky" {
            let fonts = format!("{}/.local/share/fonts", self.home);
            if !dest_dirs.contains(&fonts) {
                dest_dirs.push(fonts);
            }
        }
        let snap = install_tree_snapshot(extract_dir, &dest_dirs);
        // Что эта же покупка ставила раньше: только это можно снять и
        // поставить заново, занятое чужой темой — нет.
        let own = self.recorded_paths(&item.id);
        let fallback_name = archive_theme_name(&item.download_name, &item.name);
        let origin = ArchiveOrigin { extract_dir, fallback_name: &fallback_name };
        let out = plan_install_archive(&home, &snap, cat, origin, Some(&own));
        if !out.ok {
            return (false, out.message, Vec::new());
        }
        let Some(record) = out.record.clone() else {
            return (false, out.message, Vec::new());
        };
        if let Some(actual) = &out.reclassified {
            let dir = record
                .paths
                .first()
                .map(|p| rstrip_slashes(p).rsplit_once('/').map(|(d, _)| d.to_string()))
                .unwrap_or_default()
                .unwrap_or_default();
            let line = trf("Store category %1, actually %2 → %3", &[&cat.key, &actual, &dir]);
            self.say(&line);
        }
        if let Err(e) = exec_install_steps(&out.steps) {
            return (false, trf("Installation error: %1", &[&e]), Vec::new());
        }
        // Виджет Conky, написанный под `~/.conky/<Имя>/`: пути — на папку,
        // куда он встал (`de_apply::conky_repair_paths`).
        if record.apply_kind == "conky" {
            for p in &record.paths {
                crate::de_apply::conky_repair_paths(&self.home, p);
            }
        }
        self.record(item, &record.apply_kind, &record.paths);
        if record.apply_kind == "colors" {
            let line = trf("Color scheme installed: %1", &[&out.names.join(", ")]);
            self.say(&line);
        } else {
            let line = out.message.clone();
            self.say(&line);
        }
        (true, out.message, out.names)
    }

    /// Товар одним голым файлом: план из [`plan_single_file`], копия здесь.
    fn install_single_file(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        archive_path: &str,
    ) -> (bool, String, Vec<String>) {
        // Что эта же покупка ставила раньше — его можно заменить; чужой
        // файл (или папку темы) с тем же именем — нет, если это не тот же
        // файл байт в байт.
        let own = self.recorded_paths(&item.id);
        let taken = |p: &str| -> bool {
            let Ok(md) = std::fs::symlink_metadata(p) else { return false };
            if own.iter().any(|o| rstrip_slashes(o) == p) {
                return false;
            }
            let same = md.file_type().is_file()
                && std::fs::read(p).ok().is_some_and(|b| std::fs::read(archive_path).ok() == Some(b));
            !same
        };
        let plan = plan_single_file_with(item, cat, archive_path, &taken);
        if !plan.ok {
            return (false, plan.message, Vec::new());
        }
        let Some(record) = plan.record.clone() else {
            return (false, plan.message, Vec::new());
        };
        if let Err(e) = exec_install_steps(&plan.steps) {
            return (false, trf("File installation error: %1", &[&e]), Vec::new());
        }
        self.record(item, &record.apply_kind, &record.paths);
        let line = trf(
            "Installed as a single file: %1",
            &[&plan.names.first().map(String::as_str).unwrap_or("")],
        );
        self.say(&line);
        (true, plan.message, plan.names)
    }

    /// Системная тема (SDDM/Plymouth) через polkit-helper Rexon.
    ///
    /// Пароль спрашивается реже, чем на каждую тему: polkit-action
    /// `org.rexon.system-install` объявлен с `auth_admin_keep` (минуты для
    /// окна Rexon) и покрыт «зонтиком» `org.rexon.pkg`
    /// ([`crate::system_plan::PKG_IMPLIES`]). Гейты, имя темы и argv — ядро;
    /// наблюдения среды и сам `pkexec` — здесь.
    fn install_root(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        archive_path: &str,
    ) -> (bool, String, Vec<String>) {
        let env = Self::live_root_env(&cat.apply_kind);
        if let Some(pre) = root_preflight(&env, cat) {
            return (false, pre.message, Vec::new());
        }

        // helper принимает staged-путь только под /tmp|/var/tmp — временный
        // каталог обязан лежать там, а не в $TMPDIR пользователя.
        let work = match mkdtemp_in(Path::new("/tmp"), "rexon-ocs-root-") {
            Ok(p) => p,
            Err(e) => return (false, trf("Installation error: %1", &[&e]), Vec::new()),
        };
        let extract_dir = work.join("extracted");
        let extract_dir_s = extract_dir.to_string_lossy().into_owned();
        let result = match live_safe_extract(archive_path, &extract_dir_s) {
            Ok(()) => self.install_root_staged(item, cat, &env, &extract_dir_s),
            Err(e) => (false, trf("Couldn't unpack the archive: %1", &[&e]), Vec::new()),
        };
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    fn install_root_staged(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        env: &RootEnv,
        extract_dir: &str,
    ) -> (bool, String, Vec<String>) {
        let snap = install_tree_snapshot(extract_dir, &[]);
        let content_root = find_content_root(&snap, extract_dir);
        // Системный каталог темы с тем же именем, который эта покупка не
        // ставила (тема пакета, человека, другой покупки), не перезаписывается.
        let own = self.recorded_paths(&item.id);
        let taken = |p: &str| -> bool {
            std::fs::symlink_metadata(p).is_ok() && !own.iter().any(|o| rstrip_slashes(o) == p)
        };
        let plan = match plan_root_install_with(env, &snap, cat, &content_root, &taken) {
            Ok(p) => p,
            Err(outcome) => return (false, outcome.message.clone(), Vec::new()),
        };

        // Та же проверка, что сделает помощник от root, — до окна пароля:
        // ссылка в теме, `.so` или чужой модуль Plymouth (код от root при
        // загрузке) отвергаются сразу и с причиной.
        if let Some(problem) = crate::installer::system_theme_problem(
            &cat.apply_kind,
            Path::new(&content_root),
            Some(&plan.theme_id),
        ) {
            return (false, problem.message(), Vec::new());
        }

        if cat.apply_kind != "plymouth" {
            let has_md = Path::new(&join(&content_root, "metadata.desktop")).is_file();
            let has_qml = Path::new(&join(&content_root, "Main.qml")).is_file();
            if !has_md && !has_qml {
                // ⭐ Починка при переносе: оригинал писал это в файл журнала,
                // куда покупатель не смотрит, — а речь о том, что тема,
                // скорее всего, не заработает. Говорим ему прямо.
                let line = trf(
                    "⚠️ SDDM theme without metadata.desktop or Main.qml: %1 — the \
                     login screen may not pick it up.",
                    &[&plan.theme_id],
                );
                self.say(&line);
            }
        }

        let line = trf(
            "🔐 Installing “%1”: %2 (an administrator password may be requested)…",
            &[&cat.name, &plan.theme_id],
        );
        self.say(&line);

        let report = run_capture(&plan.argv, Some(plan.timeout_secs));
        if report.outcome == RunOutcome::Timeout {
            return (
                false,
                tr("Installation timed out (rebuilding initramfs?)"),
                Vec::new(),
            );
        }
        let Some(rc) = report.returncode else {
            // pkexec не запустился вовсе — текст ошибки запуска полезнее
            // молчания (оригинал ронял тут исключение в журнал).
            return (
                false,
                trf("Installation error: %1", &[&report.error]),
                Vec::new(),
            );
        };
        // 126 — polkit-авторизация не получена/отменена; 127 — не запущен.
        if rc == 126 || rc == 127 {
            return (false, root_cancelled_message().to_string(), Vec::new());
        }
        if rc != 0 {
            let err = crate::utils::decode_text(&report.stderr);
            return (false, root_error_message(rc, &err), Vec::new());
        }

        self.record(item, &cat.apply_kind, &[plan.dest.clone()]);
        let line = plan.success_message.clone();
        self.say(&line);
        (true, plan.success_message, vec![plan.theme_id])
    }

    // ── журнал установленного ───────────────────────────────────────────────

    /// Пути, записанные в журнал для покупки `content_id` (пусто — не было).
    fn recorded_paths(&self, content_id: &str) -> Vec<String> {
        if content_id.is_empty() {
            return Vec::new();
        }
        let log_path =
            crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &self.home);
        let data = crate::install_journal::store_log_view(&log_path);
        data.get(content_id)
            .and_then(|e| e.get("paths"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Запись в журнал установленных — порт `_record`.
    ///
    /// ⭐ Починка при переносе: оригинал при неудачной записи журнала только
    /// шептал в файл журнала. Тема оставалась на диске, но списка
    /// установленных не видела: карточка показывала «не установлено», а снять
    /// тему было нечем. Теперь неудача видна человеку.
    fn record(
        &mut self,
        item: &crate::models::ThemeItem,
        apply_kind: &str,
        paths: &[String],
    ) {
        // Тема легла в `~/.local/share/themes` — сразу и ссылка в `~/.themes`
        // для тех, кто читает только его (окно «Внешний вид» MATE).
        let xdg_themes = crate::installed_scan::xdg_themes_dir(&self.home);
        if paths.iter().any(|p| p.rsplit_once('/').is_some_and(|(d, _)| d == xdg_themes)) {
            for line in crate::installed_scan::sync_theme_links(&self.home) {
                crate::log_info!("theme", "{line}");
            }
        }
        if item.id.is_empty() {
            return; // без content_id журналу нечего запоминать
        }
        let entry = crate::ocs_store::InstalledItem {
            content_id: item.id.clone(),
            name: item.name.clone(),
            rexon_type: kind_to_rexon_type(apply_kind).to_string(),
            author: item.author.clone(),
            description: item.description.clone(),
            summary: String::new(),
            version: item.version.clone(),
            license: item.license.clone(),
            preview_url: item.preview_url.clone(),
            score: item.rating,
            downloads: item.downloads,
            tags: Vec::new(),
            web_url: String::new(),
            provider_id: "kde-look".to_string(),
            category_id: item.category_id,
        };
        let log_path =
            crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &self.home);
        // Пишется общий журнал установки, `ocs_installed.json` — его зеркало
        // (шаг 5б слияния установщиков).
        let mut data = crate::install_journal::store_log_view(&log_path);
        crate::ocs_store::record_installed(&mut data, &entry, paths, &now_local_iso());
        if let Err(e) = crate::install_journal::put_store_log(&log_path, &data) {
            let line = trf(
                "⚠️ The installed-themes log wasn't written (%1) — the theme is on \
                 disk, but it won't be in the list of installed themes.",
                &[&e],
            );
            self.say(&line);
        }
        // Скриншот Магазина рядом с темой — best-effort: локальный скан не
        // находит превью у GTK/Plasma/иконок/курсоров (в файлах темы его нет),
        // а мы его уже скачали.
        self.save_ocs_preview(item, apply_kind, paths);
    }

    /// Положить скачанный из Магазина скриншот туда, где его ищет скан
    /// установленного — порт `_save_ocs_preview` (адрес решает
    /// [`ocs_preview_dest`]).
    fn save_ocs_preview(
        &mut self,
        item: &crate::models::ThemeItem,
        apply_kind: &str,
        paths: &[String],
    ) {
        let src = self.ocs_preview_source(item);
        if src.is_empty() {
            return; // ставилось не через Магазин либо превью не скачалось
        }
        // Покупка из нескольких тем (архив с двумя темами звуков, вариантами
        // GTK) — превью у каждой: скан показывает их отдельными карточками.
        let dirs: Vec<&String> = paths.iter().filter(|p| Path::new(p).is_dir()).collect();
        let targets: Vec<String> = if apply_kind != "colors" && dirs.len() > 1 {
            dirs.into_iter().cloned().collect()
        } else {
            vec![paths.first().cloned().unwrap_or_default()]
        };
        for theme_dir in targets {
            let is_dir = !theme_dir.is_empty() && Path::new(&theme_dir).is_dir();
            let Some(dst) =
                ocs_preview_dest(&self.home, apply_kind, &theme_dir, is_dir, &item.name)
            else {
                continue; // имени нет — копировать некуда
            };
            // Каталог сайдкар-кеша создаёт вызывающий; его адрес берём у самого
            // скана, чтобы второй копии пути в проекте не было.
            let probe = crate::installed_scan::sidecar_preview_path(&self.home, "k", "n");
            if let Some((sidecar_dir, _)) = probe.rsplit_once('/') {
                if dst.starts_with(sidecar_dir) {
                    let _ = std::fs::create_dir_all(sidecar_dir);
                }
            }
            let _ = std::fs::copy(&src, &dst);
        }
    }

    /// Путь скачанного OCS-скриншота: `preview_local`, иначе файл из кеша
    /// превью по `preview_url`; `""` — ничего нет.
    fn ocs_preview_source(&self, item: &crate::models::ThemeItem) -> String {
        let local = item.preview_local.trim();
        if !local.is_empty() && Path::new(local).is_file() {
            return local.to_string();
        }
        let url = item.preview_url.trim();
        if url.is_empty() {
            return String::new();
        }
        crate::appearance_cache::LiveCache::open(&self.home)
            .get_preview(url)
            .unwrap_or_default()
    }

    /// Записать применение в историю тем — порт `ThemeManager._add_history`
    /// (`~/.config/rexon/theme_history.json`, вставка в начало, обрезка до 50).
    fn add_history(&mut self, kind: &str, theme: &str, de: &str) {
        let cfg = crate::utils::config_home(xdg_config_home().as_deref(), &self.home);
        let path = format!("{cfg}/rexon/theme_history.json");
        let existing: Vec<Value> = read_text_lossy(&path)
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let entry = crate::themes::history_entry(kind, theme, de, &now_local_iso());
        let updated = crate::themes::push_history(&existing, entry);
        if let Some(dir) = Path::new(&path).parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        let _ = crate::utils::atomic_write(&path, &crate::themes::serialize_history(&updated));
    }

    /// Валидна ли тема иконок — порт `_is_valid_icon_theme` (снимок
    /// `index.theme`, решение за [`crate::themes::is_valid_icon_theme`]).
    fn icon_theme_valid(&self, theme: &str) -> bool {
        let dirs = crate::themes::icon_dirs(&self.home);
        let mut files = serde_json::Map::new();
        for d in &dirs {
            let idx = format!("{d}/{theme}/index.theme");
            if Path::new(&idx).is_file() {
                files.insert(idx.clone(), Value::from(read_text_lossy(&idx).unwrap_or_default()));
            }
        }
        let fs = FsSnapshot::from_json(&serde_json::json!({ "dirs": {}, "files": files }));
        crate::themes::is_valid_icon_theme(&fs, theme, &dirs)
    }

    /// Безопасно ли применять тему курсоров — порт `_cursor_preflight`
    /// (GTK4-краш-гард: цепочка `Inherits` не должна вести в никуда).
    fn cursor_safe(&self, theme: &str) -> bool {
        let bases = crate::themes::icon_dirs(&self.home);
        let mut dirs = serde_json::Map::new();
        let mut files = serde_json::Map::new();
        for base in &bases {
            let d = format!("{base}/{theme}");
            if !Path::new(&d).is_dir() {
                continue;
            }
            dirs.insert(d.clone(), Value::Array(Vec::new()));
            let cur = format!("{d}/cursors");
            if Path::new(&cur).is_dir() {
                dirs.insert(cur, Value::Array(Vec::new()));
            }
            let ct = format!("{d}/cursor.theme");
            if Path::new(&ct).is_file() {
                files.insert(ct, Value::from(""));
            }
            let idx = format!("{d}/index.theme");
            if Path::new(&idx).is_file() {
                files.insert(idx.clone(), Value::from(read_text_lossy(&idx).unwrap_or_default()));
            }
        }
        let fs = FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }));
        let parent_has_cursors = |parent: &str| {
            bases
                .iter()
                .any(|base| Path::new(&format!("{base}/{parent}/cursors")).is_dir())
        };
        crate::themes::cursor_preflight_ok(&fs, theme, &self.home, parent_has_cursors)
    }

    /// Исполнить план [`crate::themes`] и записать историю при успехе.
    /// Применить курсор: тема и размер во всех окружениях — `(получилось,
    /// сообщение)`. Небезопасная тема — отказ без единой записи.
    ///
    /// Тот же план нажатия, что у «Применить» в разделе курсоров
    /// ([`press_plan_with`], размер — `size`), тот же исполнитель с откатом;
    /// удача оставляет снимок «что было» раздела `cursor` в общем журнале.
    fn apply_cursor(&mut self, theme: &str, size: Option<u32>) -> (bool, String) {
        let action = ApplyAction::Cursor;
        let mut facts = PressFacts {
            de: self.de.clone(),
            home: self.home.clone(),
            xdg_config_home: xdg_config_home(),
            cursor_size: size,
            ..PressFacts::default()
        };
        self.cursor_facts(theme, &mut facts);
        let plan = press_plan_with(&action, theme, &facts);
        let cat = crate::categories::category_by_kind(&self.home_path(), "cursor");
        let (ok, msg) = self.run_press("", cat.as_ref(), theme, &action, plan);
        (ok, msg.unwrap_or_else(|| apply_message(&action, ok).to_string()))
    }

    /// Сменить только размер курсора — тема остаётся текущей. Тот же путь,
    /// что у выбора темы, поэтому размер доходит туда же: родной ключ
    /// окружения, GTK, переменные окружения, конфиг композитора.
    pub fn apply_cursor_size(&mut self, size: u32) -> (bool, String) {
        let xdg = xdg_config_home();
        let Some(theme) = live_cursor_value(&self.de, &self.home, xdg.as_deref(), false) else {
            let text = tr("Couldn't find out which cursor theme is in use — pick a cursor theme first.");
            self.say(&text);
            return (false, text);
        };
        let (ok, msg) = self.apply_cursor(&theme, Some(size));
        let changes = live_gtk_sanitize(&self.home);
        if !changes.is_empty() {
            let line = gtk_sanitizer_message(changes.len());
            self.say(&line);
        }
        self.say(&msg);
        (ok, msg)
    }

    /// Текущий размер курсора в этом окружении (`None` — никто не задавал).
    pub fn cursor_size(&self) -> Option<u32> {
        let xdg = xdg_config_home();
        live_cursor_value(&self.de, &self.home, xdg.as_deref(), true)
            .and_then(|v| crate::themes::parse_cursor_size(&v))
    }

    // ── ЧТО ТЕМА ПРОСИТ В ПРИДАЧУ (иконки и курсор) ────────────────────────

    /// Разрешить ставить недостающие иконки и курсор темы при «Применить»
    /// (после [`Self::prepare_theme_deps`] — ставить там уже нечего; сторож
    /// сеанса — по кнопке «Установить» уведомления).
    pub fn allow_deps_install(&mut self, yes: bool) {
        self.deps_install = yes;
    }

    /// Что применить из глобальной темы — ответ окна «Применить «X»?».
    pub fn set_global_choice(&mut self, choice: Option<crate::themes::GlobalChoice>) {
        self.global_choice = choice;
    }

    /// Ответ на вопрос о значках темы ([`Self::theme_icons_question`]):
    /// `Some(true)` — скачать (без отдельного согласия на пакет) и включить,
    /// `Some(false)` — оставить свои, `None` — не решено (значки не
    /// трогаются; «Применить» берёт запомненный ответ темы).
    pub fn set_icons_choice(&mut self, yes: Option<bool>) {
        self.icons_choice = yes;
    }

    /// Вопрос «Тема рассчитана на значки «X». Скачать и включить?» перед
    /// «Применить» у GTK-темы `item` — в потоке интерфейса ДО фоновой
    /// работы. `None` — спрашивать не о чем ([`crate::themes::icons_question`]:
    /// значков тема не просит, они уже стоят, ответ для темы запомнен).
    pub fn theme_icons_question(&self, item: &crate::models::ThemeItem, theme_name: &str) -> Option<crate::themes::IconsQuestion> {
        let cat = crate::categories::category_by_id(&self.home_path(), item.category_id);
        if apply_action_for(cat.as_ref(), theme_name) != ApplyAction::Gtk {
            return None;
        }
        let plan = self.theme_deps_plan(theme_name);
        if !plan.iter().any(|i| i.role == crate::themes::DepRole::Icons) {
            return None;
        }
        let current = crate::theme_watch::live_looks(&self.de).icons;
        crate::themes::icons_question(theme_name, &plan, &current, crate::theme_watch::live_icons_choice(theme_name))
    }

    /// Просьбы GTK-темы `theme` к живой системе: что стоит, что можно
    /// поставить, чего нет нигде. Пусто — у темы нет метатемы или она
    /// ничего не просит.
    pub fn theme_deps_plan(&self, theme: &str) -> Vec<crate::themes::DepItem> {
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let dirs = env_str("XDG_DATA_DIRS");
        let index = crate::themes::gtk_theme_search_roots(&self.home, &data, &dirs)
            .into_iter()
            .map(|root| format!("{root}/{theme}/index.theme"))
            .find_map(|p| read_text_lossy(&p));
        let Some(index) = index else { return Vec::new() };
        let deps = crate::themes::theme_deps(&index);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &dirs);
        crate::themes::plan_theme_deps(&deps, &live_deps_family(), &|role, wanted| {
            live_find_dep(role, wanted, &roots)
        })
    }

    /// Поставить перед «Применить» у GTK-темы `item` то, с чем она идёт и
    /// чего нет (курсор; значки — если выбрано «Скачать и включить»), — САМО,
    /// без вопроса «Да/Нет» (защита — окно пароля polkit). Сначала — в
    /// домашний каталог без пароля, где можно; остальное — пакетами только
    /// из официальных репозиториев (сборки из AUR без вопроса нет).
    ///
    /// «Всё или ничего»: не встало — `Err` с причиной и советом, и тема НЕ
    /// применяется. Что встало, решает диск, а не код возврата.
    pub fn prepare_theme_deps(&mut self, item: &crate::models::ThemeItem, theme: &str) -> Result<(), String> {
        use crate::themes::{DepState, deps_to_prepare};
        let cat = crate::categories::category_by_id(&self.home_path(), item.category_id);
        if apply_action_for(cat.as_ref(), theme) != ApplyAction::Gtk {
            return Ok(());
        }
        if self.icons_choice.is_none() {
            self.icons_choice = crate::theme_watch::live_icons_choice(theme);
        }
        let wanted = deps_to_prepare(&self.theme_deps_plan(theme), self.icons_choice == Some(true));
        if wanted.is_empty() {
            return Ok(());
        }
        let mut why: Option<String> = None;
        let mut packages: Vec<String> = Vec::new();
        for i in &wanted {
            if crate::themes::dep_home_source(i.role, &i.wanted).is_some() {
                match self.install_home_set(i.role, &i.wanted) {
                    Ok(_) => continue,
                    Err(e) => why = Some(e),
                }
            }
            if let DepState::Installable(p) = &i.state {
                if !packages.contains(p) {
                    packages.push(p.clone());
                }
            }
        }
        if !packages.is_empty() {
            let line = trf(
                "📥 Installing what the theme comes with: %1 (an administrator password may be requested)…",
                &[&packages.join(", ")],
            );
            self.say(&line);
            if let Err(e) = live_install_repo_packages(&packages, PkgSource::ThemeSets) {
                why = Some(e);
            }
        }
        // Всё, что ставили, на месте — дальше «Применить» берёт готовое.
        let now = self.theme_deps_plan(theme);
        let missing: Vec<crate::themes::DepItem> = wanted
            .into_iter()
            .filter(|w| !now.iter().any(|n| n.role == w.role && matches!(n.state, DepState::Installed(_))))
            .collect();
        self.deps_install = true;
        if missing.is_empty() {
            return Ok(());
        }
        let why = why.unwrap_or_else(|| tr("the package didn't bring it"));
        Err(crate::themes::deps_refusal(theme, &missing, &why))
    }

    /// Применить иконки и курсор, с которыми идёт GTK-тема `theme`
    /// (поставив недостающее, если человек согласился). Строки итога — для
    /// сообщения «Применить».
    ///
    /// ⛔ Имя, которого на диске нет, в настройки не пишется: остаются
    /// прежние иконки и курсор, и человеку сказано почему.
    fn apply_theme_deps(&mut self, theme: &str) -> Vec<String> {
        // Вопрос о недостающем задан здесь, в Rexon: сторож сеанса
        // (`theme_watch`) о той же теме второй раз не спрашивает.
        crate::theme_watch::note_rexon_applied(self, theme);
        if self.icons_choice.is_none() {
            self.icons_choice = crate::theme_watch::live_icons_choice(theme);
        }
        let items = self.theme_deps_plan(theme);
        self.apply_dep_items(&items)
    }

    /// Окружение, под которое этот установщик применяет темы.
    pub fn de_id(&self) -> &str {
        &self.de
    }

    /// Поставить (если человек согласился — [`Self::allow_deps_install`]) и
    /// применить наборы `items` — общий путь «Применить» у GTK-темы и
    /// кнопки «Установить» в уведомлении сторожа сеанса (`theme_watch`).
    /// Строки итога — для человека.
    ///
    /// ⛔ Имя, которого на диске нет, в настройки не пишется.
    pub fn apply_dep_items(&mut self, items: &[crate::themes::DepItem]) -> Vec<String> {
        if items.is_empty() {
            return Vec::new();
        }
        // Сначала — то, что кладётся в домашний каталог без пароля и без
        // сборки (решение Wehrwolfmann 17.10.2026): молча, если это не
        // выключено в настройках, или с согласия человека. Не вышло — дальше
        // обычный путь пакета.
        let mut items: Vec<crate::themes::DepItem> = items.to_vec();
        // Значки темы — только по ответу человека (решение 3а): «Оставить
        // мои» или не решено — набор не качается и не включается.
        let mut lines = Vec::new();
        let icons_ok = self.icons_choice == Some(true);
        if !icons_ok {
            if self.icons_choice == Some(false) {
                for i in items.iter().filter(|i| i.role == crate::themes::DepRole::Icons) {
                    lines.push(crate::themes::dep_icons_kept_line(&i.wanted));
                }
            }
            items.retain(|i| i.role != crate::themes::DepRole::Icons);
        }
        // Согласие на установку: значки — ответом на свой вопрос, прочее —
        // общим вопросом о пакетах.
        let deps_install = self.deps_install;
        let consent = move |role: crate::themes::DepRole| deps_install || (icons_ok && role == crate::themes::DepRole::Icons);
        let auto_home = crate::theme_watch::auto_home_enabled();
        {
            for item in items.iter_mut() {
                if matches!(item.state, crate::themes::DepState::Installed(_))
                    || crate::themes::dep_home_source(item.role, &item.wanted).is_none()
                    || !(consent(item.role) || auto_home)
                {
                    continue;
                }
                match self.install_home_set(item.role, &item.wanted) {
                    Ok(name) => item.state = crate::themes::DepState::Installed(name),
                    Err(e) => {
                        let line = trf("⚠️ Couldn't download %1: %2", &[&crate::themes::dep_label(item.role, &item.wanted), &e]);
                        self.say(&line);
                    }
                }
            }
        }
        let mut install_error: Option<String> = None;
        let packages: Vec<String> = items
            .iter()
            .filter(|i| consent(i.role))
            .filter_map(|i| match &i.state {
                crate::themes::DepState::Installable(p) => Some(p.clone()),
                _ => None,
            })
            .fold(Vec::new(), |mut acc, p| {
                if !acc.contains(&p) {
                    acc.push(p);
                }
                acc
            });
        if !packages.is_empty() {
            let line = trf(
                "📥 Installing what the theme comes with: %1 (an administrator password may be requested)…",
                &[&packages.join(", ")],
            );
            self.say(&line);
            if let Err(e) = live_install_dep_packages(&packages) {
                install_error = Some(e);
            }
        }
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &env_str("XDG_DATA_DIRS"));
        let didnt_bring = tr("the package didn't bring it");
        for item in &items {
            use crate::themes::DepState;
            let name = match &item.state {
                DepState::Installed(name) => Some(name.clone()),
                // Правда — диск после установки, а не код возврата помощника.
                DepState::Installable(_) if consent(item.role) => live_find_dep(item.role, &item.wanted, &roots),
                _ => None,
            };
            let Some(name) = name else {
                lines.push(match &item.state {
                    DepState::Missing => crate::themes::dep_missing_line(item.role, &item.wanted),
                    _ => crate::themes::dep_not_installed_line(
                        item.role,
                        &item.wanted,
                        consent(item.role).then_some(install_error.as_deref().unwrap_or(&didnt_bring)),
                    ),
                });
                continue;
            };
            let ok = match item.role {
                crate::themes::DepRole::Icons => self.apply_icons_named(&name, &roots),
                crate::themes::DepRole::Cursor => {
                    let xdg = xdg_config_home();
                    let size = live_cursor_value(&self.de, &self.home, xdg.as_deref(), true)
                        .and_then(|v| crate::themes::parse_cursor_size(&v));
                    self.apply_cursor(&name, size).0
                }
            };
            lines.push(if ok {
                crate::themes::dep_applied_line(item.role, &name, &item.wanted)
            } else {
                crate::themes::dep_apply_failed_line(item.role, &name)
            });
        }
        lines
    }

    /// Применить набор иконок, уже найденный на диске в одном из `roots`
    /// (там же, где его ищет GTK, — включая `~/.icons`).
    fn apply_icons_named(&mut self, name: &str, roots: &[String]) -> bool {
        let valid = self.icon_theme_valid(name) || {
            let read = |p: &str| read_text_lossy(p);
            let is_dir = |p: &str| Path::new(p).is_dir();
            crate::themes::find_installed_dep(crate::themes::DepRole::Icons, name, roots, &is_dir, &|_| Vec::new(), &read)
                .as_deref()
                == Some(name)
        };
        let de = self.de.clone();
        let home = self.home.clone();
        match crate::themes::apply_icon_plan_xdg(name, &de, &home, xdg_config_home().as_deref(), valid) {
            Some(plan) => self.run_theme_plan(&plan, name, &de),
            None => false,
        }
    }

    /// Положить набор `wanted` в домашний каталог из его домашнего источника
    /// ([`crate::themes::dep_home_source`]) — только скачать и распаковать,
    /// чужих сценариев установки не запускать — и найти его на диске.
    /// `Ok(имя набора на диске)`; `Err` — причина для человека.
    ///
    /// Ставит тот же установщик, что темы магазина, — с записью в
    /// журнал установленного, так что «Удалить» в «Оформлении» его снимет.
    pub fn install_home_set(&mut self, role: crate::themes::DepRole, wanted: &str) -> Result<String, String> {
        use crate::themes::HomeSource;
        let Some(source) = crate::themes::dep_home_source(role, wanted) else {
            return Err(tr("Rexon doesn't know where to download it"));
        };
        let label = crate::themes::dep_label(role, wanted);
        self.say(&trf("📥 Downloading %1 into your home folder…", &[&label]));
        let failed = || tr("the download failed");
        match source {
            HomeSource::None => return Err(tr("Rexon doesn't know where to download it")),
            HomeSource::Ocs { content_id, file, .. } => {
                let category = match role {
                    crate::themes::DepRole::Icons => 132,
                    crate::themes::DepRole::Cursor => 107,
                };
                let io = crate::ocs::LiveOcsIo::new(&self.home);
                let preview_dir = io.preview_dir();
                let mut client = crate::ocs::OcsClient::new(io, &preview_dir);
                let work = mkdtemp_in(&std::env::temp_dir(), "rexon-deps-").map_err(|e| e.to_string())?;
                let dest = work.join(file).to_string_lossy().into_owned();
                let link_of = |item: &crate::models::ThemeItem| {
                    item.download_links
                        .iter()
                        .position(|l| l.get("name").and_then(serde_json::Value::as_str) == Some(file))
                };
                // Детали — из кеша магазина (сутки), к сайту бережно. Ссылки на
                // файлы у магазина подписаны и со временем протухают: не
                // скачалось по кешу — один раз со свежими деталями.
                let mut downloaded = None;
                for fresh in [false, true] {
                    let content = if fresh {
                        client.get_item(content_id)
                    } else {
                        client.get_item_cached(content_id, crate::ocs::DETAIL_TTL_SECS)
                    };
                    let Some(mut item) = content.map(|c| c.item) else { continue };
                    item.category_id = category;
                    let Some(idx) = link_of(&item) else { continue };
                    if client.download(&item, &dest, idx as isize, false) {
                        downloaded = Some(item);
                        break;
                    }
                }
                let result = match downloaded {
                    None => Err(failed()),
                    Some(item) => {
                        let (ok, msg, _names) = self.install(&item, &dest);
                        if ok { Ok(()) } else { Err(msg) }
                    }
                };
                let _ = std::fs::remove_dir_all(&work);
                result?;
            }
        }
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &env_str("XDG_DATA_DIRS"));
        // Правда — диск после распаковки.
        live_find_dep(role, wanted, &roots).ok_or_else(|| tr("the download didn't bring it"))
    }

    /// Применить уже стоящий набор `name` роли `role` (значки или курсор) —
    /// тем же путём, что наборы GTK-темы в [`Self::apply_dep_items`]. Набора
    /// на диске нет — `false` без единой записи.
    pub fn apply_installed_set(&mut self, role: crate::themes::DepRole, name: &str) -> bool {
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &env_str("XDG_DATA_DIRS"));
        let Some(found) = live_find_dep(role, name, &roots) else {
            return false;
        };
        match role {
            crate::themes::DepRole::Icons => self.apply_icons_named(&found, &roots),
            crate::themes::DepRole::Cursor => {
                let xdg = xdg_config_home();
                let size = live_cursor_value(&self.de, &self.home, xdg.as_deref(), true)
                    .and_then(|v| crate::themes::parse_cursor_size(&v));
                self.apply_cursor(&found, size).0
            }
        }
    }

    fn run_theme_plan(&mut self, plan: &crate::themes::Plan, theme: &str, de: &str) -> bool {
        self.run_theme_plan_report(plan, theme, de).ok
    }

    /// То же, с причиной отказа, если она известна точно (Deepin).
    fn run_theme_plan_report(
        &mut self,
        plan: &crate::themes::Plan,
        theme: &str,
        de: &str,
    ) -> crate::themes::CursorReport {
        let report = exec_apply_plan_report(&plan.steps);
        if report.ok {
            self.add_history(plan.history_kind, theme, de);
        }
        report
    }

    // ── ПРИМЕНЕНИЕ ──────────────────────────────────────────────────────────

    /// Порт `ThemeInstaller.apply` — `(получилось, сообщение)`.
    ///
    /// Диспетчер — [`apply_action_for`]. Шаг 5а слияния установщиков:
    /// нажатие сначала СОБИРАЕТ план ([`Self::press_plan`], без единого
    /// запуска и записи), затем его исполняет общий
    /// [`crate::install_plan::exec`] — файловые записи нажатия под общим
    /// откатом, команды через `Ctx::run`. Действия, которые пока исполняются
    /// своим путём ([`PressPlan::Own`]: цвета в Plasma, обои стола, Conky,
    /// экран входа), — в [`Self::apply_own`]. После ЛЮБОГО
    /// применения обязательно идёт GTK-санитайзер: он чинит битые ссылки
    /// `gtk-{3,4}.0`, которые иначе роняют GTK4-приложения.
    pub fn apply(
        &mut self,
        item: &crate::models::ThemeItem,
        theme_name: &str,
    ) -> (bool, String) {
        let home = self.home_path();
        let cat = crate::categories::category_by_id(&home, item.category_id);
        let action = apply_action_for(cat.as_ref(), theme_name);
        let home_s = self.home.clone();

        let plan = self.press_plan(cat.as_ref(), &action, theme_name);
        let (ok, msg) = self.run_press(&item.id, cat.as_ref(), theme_name, &action, plan);

        let message = msg.unwrap_or_else(|| apply_message(&action, ok).to_string());

        // КРИТИЧНО: чиним gtk-конфиги после ЛЮБОГО применения.
        let changes = live_gtk_sanitize(&home_s);
        if !changes.is_empty() {
            let line = gtk_sanitizer_message(changes.len());
            self.say(&line);
        }
        self.say(&message);

        // Включённое дополнение или запущенный виджет — не смена темы.
        if ok && action != ApplyAction::DeSection {
            self.notify_theme_changed(theme_name);
        }
        (ok, message)
    }

    /// План нажатия «Применить» — без единого запуска команды и записи
    /// (шаг 5а). Живые пробы (есть ли `metadata.json`, валидна ли тема
    /// значков, тёмная ли тема GTK, переключатель стиля Qt, чем звать
    /// KWin) снимаются здесь и только для своего действия; план из них
    /// собирает [`press_plan_with`].
    pub fn press_plan(&self, cat: Option<&ThemeCategory>, action: &ApplyAction, theme_name: &str) -> PressPlan {
        let de = self.de.as_str();
        let home = self.home.as_str();
        let xdg = xdg_config_home();
        let mut facts = PressFacts {
            de: de.to_string(),
            home: home.to_string(),
            xdg_config_home: xdg.clone(),
            ..PressFacts::default()
        };
        match action {
            ApplyAction::KdeGlobal => {
                facts.global_theme_ok = !live_global_theme_missing_metadata_json(home, theme_name);
                // Вне Plasma `plasma-apply-lookandfeel` звать нельзя: без окна
                // (нажатие не из «Оформления») — оформление, что переносится.
                let choice = self.global_choice.or_else(|| {
                    (de != "kde").then_some(crate::themes::GlobalChoice { appearance: true, layout: false, parts: None })
                });
                facts.global = choice.map(|choice| {
                    let io = crate::animations::SystemIo;
                    let contents = crate::themes::lnf_contents_live(&io, home, theme_name).unwrap_or_default();
                    let mut press = crate::themes::GlobalPress { choice, contents, ..Default::default() };
                    press.sections = self.global_sections(&press);
                    press
                });
            }
            ApplyAction::Icons => facts.icons_valid = self.icon_theme_valid(theme_name),
            ApplyAction::Gtk => {
                facts.gtk_dark = gtk_theme_dark_live(theme_name, home);
                facts.gtk4 = Some(crate::gtk4_theme::facts_live(theme_name, home, xdg.as_deref()));
            }
            ApplyAction::Kvantum | ApplyAction::WidgetStyle => {
                let env = qt_style_env(de, home, xdg.as_deref());
                facts.qt_platform_theme = env.platform_theme.to_string();
                facts.qt_icon_theme = env.icon_theme;
            }
            ApplyAction::Aurorae => {
                facts.kwin_reconfigure = kwin_reconfigure_argv();
                facts.aurorae_library = crate::kdecoration::live_svg_library().to_string();
            }
            ApplyAction::Cursor => {
                self.cursor_facts(theme_name, &mut facts);
                // Размер не меняем — берём тот, что сейчас.
                facts.cursor_size = live_cursor_value(de, home, xdg.as_deref(), true)
                    .and_then(|v| crate::themes::parse_cursor_size(&v));
            }
            ApplyAction::DeSection => {
                let kind = cat.and_then(|c| crate::categories::category_kind(&c.key)).unwrap_or("");
                facts.desktop = Some(crate::de_apply::press_plan(
                    &crate::animations::SystemIo,
                    &crate::de_apply::Ctx::live(),
                    kind,
                    theme_name,
                ));
            }
            ApplyAction::LockWallpaper => {
                let install_dir = cat.map(|c| c.install_dir.clone()).unwrap_or_default();
                let is_file = Path::new(&join(&install_dir, theme_name)).is_file();
                facts.lock = Some(LockFacts { install_dir, is_file, treeland: crate::dde::treeland_session_live() });
            }
            ApplyAction::SoundTheme => {
                let dirs = crate::qt_settings::live_xdg_sound_theme_dirs();
                // ⭐ Починка при переносе: оригинал искал строго
                // `kwriteconfig6` и на Plasma 5 отказывался писать вовсе, хотя
                // там есть `kwriteconfig5`. Ищем общим резолвером синонимов.
                let path = path_env();
                let tool = |t: &str| crate::utils::resolve_tool(t, &[], path.as_deref()).unwrap_or_default();
                facts.sound = Some(SoundFacts {
                    exists: crate::qt_settings::sound_theme_exists(&dirs, theme_name),
                    kwrite: tool("kwriteconfig6"),
                    kread: tool("kreadconfig6"),
                    has_gsettings: have("gsettings"),
                });
            }
            ApplyAction::KdeColors => {
                if let crate::shell_palette::SchemeWay::Layer { lxqt } = crate::shell_palette::scheme_way(de) {
                    let ctx = crate::shell_palette::SchemeCtx::live(de);
                    facts.colors = Some(crate::shell_palette::scheme_layer_press(
                        &crate::animations::SystemIo,
                        &ctx,
                        lxqt,
                        theme_name,
                    ));
                }
            }
            _ => {}
        }
        press_plan_with(action, theme_name, &facts)
    }

    /// Планы разделов для частей глобальной темы, которые отметил человек
    /// ([`crate::themes::lnf_section_parts`]): та же кнопка «Применить»
    /// раздела «Значки», «Курсоры», «Цветовые схемы», «Стиль приложений» с
    /// её пробами, кнопки заголовка — план раздела «Кнопки заголовка».
    fn global_sections(
        &self,
        press: &crate::themes::GlobalPress,
    ) -> std::collections::BTreeMap<crate::themes::LnfPart, Result<Vec<crate::themes::ApplyStep>, String>> {
        use crate::themes::LnfPart;
        let de = self.de.as_str();
        let sel = crate::themes::global_selection(press, de);
        let mut out = std::collections::BTreeMap::new();
        for &part in crate::themes::lnf_section_parts(de) {
            if sel & part.bit() == 0 {
                continue;
            }
            let got = if part == LnfPart::TitlebarLayout {
                let io = crate::animations::SystemIo;
                let ctx = crate::animations::LookCtx::from_env();
                crate::themes::lnf_buttons_steps(&io, &ctx, &press.contents)
            } else {
                let action = match part {
                    LnfPart::Colors => ApplyAction::KdeColors,
                    LnfPart::WidgetStyle => ApplyAction::WidgetStyle,
                    LnfPart::Icons => ApplyAction::Icons,
                    _ => ApplyAction::Cursor,
                };
                let name = crate::themes::lnf_section_value(&press.contents, part).unwrap_or_default();
                match self.press_plan(None, &action, &name) {
                    PressPlan::Steps { steps, .. } => Ok(steps
                        .into_iter()
                        .filter_map(|s| match s {
                            crate::install_plan::Step::Apply(a) => Some(a),
                            crate::install_plan::Step::File(_) => None,
                        })
                        .collect()),
                    PressPlan::Refused(why) => Err(why.unwrap_or_else(|| apply_message(&action, false))),
                    _ => Err(apply_message(&action, false)),
                }
            };
            out.insert(part, got);
        }
        out
    }

    /// Пробы курсора для плана: безопасна ли тема и не курсор ли она
    /// окружения по умолчанию (спрашивается, только если небезопасна).
    fn cursor_facts(&self, theme: &str, facts: &mut PressFacts) {
        facts.cursor_safe = self.cursor_safe(theme);
        facts.cursor_standard =
            !facts.cursor_safe && live_standard_theme(&self.de, "cursor").as_deref() == Some(theme);
    }

    /// Исполнить план нажатия — `(получилось, сообщение)`; `None` — общее
    /// [`apply_message`]. Общий путь «Применить» и кнопок курсора (тема
    /// набора GTK-темы, смена размера).
    fn run_press(
        &mut self,
        item_id: &str,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
        action: &ApplyAction,
        plan: PressPlan,
    ) -> (bool, Option<String>) {
        match plan {
            PressPlan::Steps { steps, history, say } => {
                let report = self.exec_press(item_id, cat, theme_name, &steps);
                let ok = report.ok;
                let mut msg = match say {
                    PressSay::Plain => report.reason.clone().filter(|_| !ok),
                    PressSay::Cursor => Some(crate::themes::cursor_apply_message(&self.de, &report)),
                    PressSay::Done(text) if ok => Some(text),
                    PressSay::Done(_) => report.reason.clone(),
                    PressSay::Pair { done, .. } if ok => Some(done),
                    PressSay::Pair { failed, .. } => Some(report.reason.clone().unwrap_or(failed)),
                };
                if ok {
                    if let Some((kind, de)) = history {
                        self.add_history(kind, theme_name, &de);
                    }
                }
                // Иконки и курсор, с которыми тема GTK задумана, — тем же
                // путём, что кнопки разделов, во всех окружениях.
                if ok && *action == ApplyAction::Gtk {
                    let lines = self.apply_theme_deps(theme_name);
                    if !lines.is_empty() {
                        msg = Some(
                            std::iter::once(apply_message(action, true))
                                .chain(lines)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                }
                (ok, msg)
            }
            PressPlan::Desktop { steps, check } => {
                let (ok, msg) = self.exec_desktop(item_id, cat, theme_name, &steps, &check);
                (ok, Some(msg))
            }
            PressPlan::Refused(m) => (false, m),
            PressPlan::Nothing => (true, None),
            PressPlan::Own => self.apply_own(action, cat, theme_name),
        }
    }

    /// Кнопка раздела окружения общим исполнителем под откатом: команды —
    /// живой машиной `de_apply` (`SystemIo`), итог — сверка окружения. Сбой
    /// файлового шага — записанное возвращено; удача — снимок «что было» и
    /// запись раздела в общий журнал ([`Self::record_press`]); не принято
    /// окружением — записанное остаётся (как прежде), копии стираются.
    fn exec_desktop(
        &mut self,
        item_id: &str,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
        steps: &[crate::install_plan::Step],
        check: &crate::de_apply::Check,
    ) -> (bool, String) {
        use crate::install_plan::Undo;
        let io = crate::animations::SystemIo;
        let ctx = crate::de_apply::Ctx::live();
        let mut undo = Undo::new().keeping_values();
        match crate::de_apply::exec_plan(&io, steps, &mut undo) {
            Err(f) => {
                undo.finish(false);
                (false, press_failed_report(&f).reason.unwrap_or_default())
            }
            Ok(err) => {
                let (ok, msg) = crate::de_apply::check(&io, &ctx, check, err);
                match cat {
                    Some(cat) if ok => self.record_press(item_id, cat, theme_name, undo),
                    _ => undo.finish(true),
                }
                (ok, msg)
            }
        }
    }

    /// Исполнить план нажатия общим исполнителем на живой машине.
    ///
    /// Сбой записи — всё, что нажатие записало, возвращено (общий откат).
    /// Успех — отложенные откатом копии не стираются, а ложатся снимком «что
    /// было», и в общий журнал идёт запись раздела ([`Self::record_press`]).
    /// Не применилось (`ok = false`) — как прежде: записанное остаётся,
    /// копии стираются.
    fn exec_press(
        &mut self,
        item_id: &str,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
        steps: &[crate::install_plan::Step],
    ) -> crate::themes::CursorReport {
        use crate::install_plan::{exec_undoable, Ctx, Undo};
        let ctx = Ctx::live(true, false, None, crate::dde::treeland_session_live());
        let mut undo = Undo::new().keeping_values();
        match exec_undoable(steps, &mut Vec::new(), &ctx, &mut undo) {
            Err(f) => {
                undo.finish(false);
                press_failed_report(&f)
            }
            Ok(report) => {
                match cat {
                    Some(cat) if report.apply.ok => self.record_press(item_id, cat, theme_name, undo),
                    _ => undo.finish(true),
                }
                report.apply
            }
        }
    }

    /// Снимок «что было» удачного нажатия и запись раздела в общий журнал
    /// ([`crate::install_journal::Applied`]): файлы и прежние значения стола
    /// (`gsettings`/`dconf`, `xfconf`). Нажатие не тронуло ни того, ни
    /// другого — записывать нечего.
    fn record_press(&mut self, item_id: &str, cat: &ThemeCategory, theme: &str, mut undo: crate::install_plan::Undo) {
        use crate::install_journal as journal;
        let rexon = PathBuf::from(crate::utils::config_home(xdg_config_home().as_deref(), &self.home)).join("rexon");
        let dir = journal::applied_snapshot_dir(&rexon, &cat.key);
        let first = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<usize>().ok()))
                    .max()
                    .map_or(1, |n| n + 1)
            })
            .unwrap_or(1);
        let values = undo.take_values();
        let touched = undo.keep_into(&dir, first);
        if touched.is_empty() && values.is_empty() {
            return;
        }
        let key = journal::key_of(journal::Source::Store, item_id);
        let item = (!item_id.is_empty() && journal::view_at(&rexon).items.contains_key(&key)).then_some(key);
        let text = |p: &Path| p.to_string_lossy().into_owned();
        let applied = journal::Applied {
            section: cat.key.clone(),
            theme: theme.to_string(),
            item,
            applied_at: Some(now_local_iso()),
            snapshot: text(&dir),
            files: touched
                .iter()
                .map(|t| journal::AppliedFile { path: text(&t.path), before: t.before.as_deref().map(text), dir: t.dir })
                .collect(),
        };
        let values = values.into_iter().map(journal::AppliedValue::from).collect();
        if let Err(e) = journal::record_applied_values_at(&rexon, applied, values) {
            crate::log_warn!("install", "«Применить» не записано в общий журнал ({}) — {e}", rexon.display());
        }
    }

    /// Действия «Применить», которые пока исполняются своим путём
    /// ([`PressPlan::Own`]) — `(получилось, сообщение)`.
    fn apply_own(
        &mut self,
        action: &ApplyAction,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
    ) -> (bool, Option<String>) {
        let de = self.de.clone();
        let mut ok = false;
        let msg: Option<String>;
        match action {
            ApplyAction::KdeColors => {
                // Сюда — только Plasma (`plasma-apply-colorscheme`); вне
                // Plasma слои Rexon собирает план нажатия.
                let (o, m) = crate::shell_palette::apply_scheme_live(&de, theme_name);
                ok = o;
                if ok {
                    self.add_history("kde-colors", theme_name, &de);
                }
                msg = Some(m);
            }
            ApplyAction::WallpaperBackground => {
                let install_dir = cat.map(|c| c.install_dir.clone()).unwrap_or_default();
                let (o, m) = self.apply_wallpaper_background(&join(&install_dir, theme_name));
                ok = o;
                msg = Some(m);
            }
            ApplyAction::WallpaperPack => {
                let install_dir = cat.map(|c| c.install_dir.clone()).unwrap_or_default();
                let (o, m) = self.apply_wallpaper_pack(&install_dir, theme_name);
                ok = o;
                msg = Some(m);
            }
            // Сюда — только Conky (запуск и остановка процессов); тема и
            // дополнение собираются планом нажатия.
            ApplyAction::DeSection => {
                let kind = cat
                    .as_ref()
                    .and_then(|c| crate::categories::category_kind(&c.key))
                    .unwrap_or("");
                let (o, m) = crate::de_apply::press(
                    &crate::animations::SystemIo,
                    &crate::de_apply::Ctx::live(),
                    kind,
                    theme_name,
                );
                ok = o;
                msg = Some(m);
            }
            ApplyAction::SystemTheme => {
                let (o, m) = match cat {
                    Some(c) => self.apply_system_theme(c, theme_name),
                    None => (false, apply_message(&ApplyAction::Noop, false)),
                };
                ok = o;
                msg = Some(m);
            }
            // Остальное собирается планом (`press_plan`), сюда не приходит.
            _ => msg = Some(apply_message(action, false)),
        }
        (ok, msg)
    }

    /// Окружение системной части для экрана входа и заставки загрузки —
    /// одно на установку и на «Применить».
    fn live_root_env(kind: &str) -> RootEnv<'static> {
        let is_plasmalogin =
            kind == "sddm" && crate::system_plan::is_plasma_login(&active_login_manager());
        let detector = crate::system::DistroDetector::detect_live();
        RootEnv {
            is_plasmalogin,
            has_pkexec: have("pkexec"),
            system_helper_path: crate::installer::system_helper_path(),
            system_helper_exists: crate::system_plan::is_exec_file(
                crate::installer::system_helper_path(),
            ),
            is_steamos_or_immutable: detector.is_steamos() || detector.is_immutable(),
            is_steamos: crate::distro_compat::is_steamos_id(&detector.distro_id()),
            family_is_debian: detector.family_live() == crate::system::PkgFamily::Debian,
        }
    }

    /// «Применить» у темы экрана входа (SDDM) или заставки загрузки
    /// (Plymouth): помощник от root выбирает тему, уже лежащую в системном
    /// каталоге. Пароль — тем же действием polkit, что у установки
    /// (`auth_admin_keep`): «Установить и применить» спрашивает его один раз.
    fn apply_system_theme(&mut self, cat: &ThemeCategory, theme: &str) -> (bool, String) {
        let env = Self::live_root_env(&cat.apply_kind);
        let line = trf(
            "🔐 Applying “%1” (an administrator password may be requested)…",
            &[&theme],
        );
        self.say(&line);
        let (ok, msg) = system_theme_apply_with(
            &env,
            cat,
            theme,
            &crate::system_l10n::lang_args(env.system_helper_path),
            &|p| Path::new(p).is_dir(),
            &|argv, timeout| run_capture(argv, Some(timeout)),
        );
        if ok && cat.apply_kind == "sddm" {
            if let Some(note) = sddm_elsewhere_note(&active_login_manager()) {
                return (true, format!("{}. {note}", trf("✅ Applied: %1", &[&theme])));
            }
        }
        (ok, msg)
    }

    /// Сказать мосту, что тема сменилась (`bridge.json`, поле `current_theme`).
    fn notify_theme_changed(&mut self, theme_name: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        let _ = bridge.ensure_dirs();
        let _ = bridge.notify_theme_changed(
            theme_name,
            &crate::engine_update::now_iso(now.as_secs() as i64),
            now.as_millis() as u64,
        );
    }

    /// Поставить картинку фоном рабочего стола — ОБЩИМ исполнителем обоев
    /// процесса ([`crate::wallpaper_exec::live_slot`]), тем же, что у вкладок
    /// «Обои», «Настройки» и «Steam»; второго в проекте нет.
    ///
    /// ⭐ Починка (правило #12): прежде здесь жил свой урезанный исполнитель
    /// плана. Он не умел поднимать демон swww и держать swaybg/wbg и отвечал
    /// «поставьте во вкладке Wallpaper Engine», поэтому на Hyprland (без
    /// запущенного hyprpaper), niri, river и Wayfire обои из магазина не
    /// ставились никогда; не проверял он и запись pcmanfm-qt (LXQt, Д12).
    /// Общий исполнитель заодно снимает наши живые обои (видео, веб) перед
    /// картинкой — раньше статичная ложилась под их поверхность.
    fn apply_wallpaper_background(&mut self, path: &str) -> (bool, String) {
        if !Path::new(path).is_file() {
            return (false, trf("Wallpaper file not found: %1", &[&path]));
        }
        let slot = crate::wallpaper_exec::live_slot();
        let (ok, detail) = apply_static_with_slot(&slot, path);
        if ok {
            return (true, tr("✅ Wallpaper set as the desktop background."));
        }
        // Мягкая деградация: установка-то состоялась, фон не встал.
        (
            true,
            trf(
                "Installed to the wallpaper folder. ⚠️ Couldn't set it as the \
                 background automatically (%1).",
                &[&detail],
            ),
        )
    }

    /// Поставить фоном картинку ИЗ ПАЧКИ обоев — какую именно, решает ядро
    /// ([`pick_pack_image`]); установка фона — тот же общий движок.
    ///
    /// Размер экрана берётся у общего опроса выходов ([`screen_size`]) — по
    /// самому большому монитору, потому что из пачки на весь стол уходит
    /// одна картинка. «Не знаю» ядро понимает — выбор тогда идёт по имени
    /// файла.
    fn apply_wallpaper_pack(&mut self, install_dir: &str, theme_name: &str) -> (bool, String) {
        let pack_dir = join(install_dir, theme_name);
        if !Path::new(&pack_dir).is_dir() {
            return (false, trf("Wallpaper pack folder not found: %1", &[&pack_dir]));
        }
        let snap = install_tree_snapshot(&pack_dir, &[]);
        let images = pack_images(&snap, &pack_dir);
        let (w, h) = screen_size();
        let Some(chosen) = pick_pack_image(&images, w, h) else {
            return (true, pack_no_images_message().to_string());
        };
        let line = trf("🖼️ Picture chosen from the pack: %1", &[&basename(&chosen)]);
        self.say(&line);
        self.apply_wallpaper_background(&chosen)
    }

    /// Порт `ThemeInstaller.install_and_apply` (поставить и включить
    /// одним действием).
    pub fn install_and_apply(
        &mut self,
        item: &crate::models::ThemeItem,
        archive_path: &str,
    ) -> (bool, String) {
        let (ok, msg, names) = self.install(item, archive_path);
        if !ok {
            return (false, msg);
        }
        let Some(first) = names.first().cloned() else {
            return (true, msg);
        };
        let (applied_ok, apply_msg) = self.apply(item, &first);
        if applied_ok {
            (true, format!("✅ {msg}. {apply_msg}"))
        } else {
            (true, format!("{msg}. ⚠️ {apply_msg}"))
        }
    }

    // ── УДАЛЕНИЕ ────────────────────────────────────────────────────────────

    /// Порт `ThemeInstaller.uninstall`: снять файлы по журналу и забыть запись.
    ///
    /// ⭐ Починка при переносе: оригинал отвечал «Удалено» даже когда журнал
    /// после удаления записать не удалось — тема исчезала с диска, но
    /// оставалась в списке установленных навсегда. Теперь расхождение с диском
    /// названо прямо в ответе.
    pub fn uninstall(&mut self, content_id: &str) -> (bool, String) {
        let log_path =
            crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &self.home);
        let cats = crate::categories::categories(Path::new(&self.home));
        let home = self.home.clone();
        uninstall_from_log(&home, &log_path, content_id, &cats, &mut |cat, theme| {
            self.remove_system_theme(cat, theme)
        })
    }

    /// «Удалить» у системной темы без пакета-владельца и без записи журнала
    /// (положена руками или чужим установщиком) — по пути из скана. Тот же
    /// помощник от root, что у темы из магазина: он ещё раз проверяет, что
    /// каталог не принадлежит пакету и тема не выбрана сейчас.
    pub fn remove_loose_system_theme(&mut self, path: &str) -> (bool, String) {
        let cats = crate::categories::categories(Path::new(&self.home));
        let Some((cat, theme)) = system_theme_of_path(&cats, path) else {
            return (false, trf("Couldn't remove “%1”: unexpected theme name.", &[&path]));
        };
        let (ok, why) = self.remove_system_theme(&cat, &theme);
        if !ok {
            return (false, why);
        }
        if path_present(path) {
            return (false, trf("Couldn't remove %1.", &[&path]));
        }
        (true, uninstall_message(1, &theme))
    }

    /// «Удалить» у темы экрана входа (SDDM) или заставки загрузки (Plymouth):
    /// каталог в `/usr/share/<вид>/themes` снимает помощник от root, тем же
    /// действием polkit, что у установки и «Применить».
    fn remove_system_theme(&mut self, cat: &ThemeCategory, theme: &str) -> (bool, String) {
        let mut env = Self::live_root_env(&cat.apply_kind);
        // Plasma Login Manager мешает ПРИМЕНИТЬ тему SDDM, но не удалить её.
        env.is_plasmalogin = false;
        let active = crate::installed_scan::live_system_theme_active(&cat.apply_kind);
        if !active.iter().any(|a| a == theme) {
            let line = trf(
                "🔐 Removing “%1” (an administrator password may be requested)…",
                &[&theme],
            );
            self.say(&line);
        }
        system_theme_remove_with(
            &env,
            cat,
            theme,
            &crate::system_l10n::lang_args(env.system_helper_path),
            &active,
            &|argv, timeout| run_capture(argv, Some(timeout)),
        )
    }
}

/// Системная тема по пути из журнала установленных: раздел, которому нужен
/// root (`requires_root`), и имя темы. Только прямой потомок каталога тем
/// раздела с безопасным именем; всё прочее — не системная тема.
pub fn system_theme_of_path(cats: &[ThemeCategory], path: &str) -> Option<(ThemeCategory, String)> {
    let p = rstrip_slashes(path);
    let (parent, name) = p.rsplit_once('/')?;
    if !crate::ext_manager::is_safe_sys_name(name) {
        return None;
    }
    cats.iter()
        .find(|c| c.requires_root && !c.install_dir.is_empty() && rstrip_slashes(&c.install_dir) == parent)
        .map(|c| (c.clone(), name.to_string()))
}

/// Есть ли что-то по пути (ссылку не разыменовываем).
fn path_present(p: &str) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// Домашняя корзина этой машины (`$XDG_DATA_HOME/Trash`).
fn home_trash_live() -> String {
    let xdg_data = std::env::var("XDG_DATA_HOME").ok();
    crate::utils::home_trash_dir(xdg_data.as_deref(), &std::env::var("HOME").unwrap_or_default())
}

/// Снять покупку `content_id` по журналу `log_path` — порт
/// `ThemeInstaller.uninstall`, решения и местное удаление.
///
/// Пути системных разделов ([`system_theme_of_path`]) уходят в
/// `remove_system` (помощник от root); остальные снимаются здесь.
///
/// ⛔ Починка 09.10.2026 (дефект «Удалено (1 путей)»): прежде ошибка
/// удаления глоталась, путь считался снятым, а запись журнала стиралась
/// всегда. У темы Plymouth/SDDM из магазина это давало «Удалено», пустой
/// журнал и тему, оставшуюся в `/usr/share`. Теперь снятым считается только
/// то, чего на диске больше нет; если хоть что-то осталось — запись журнала
/// не трогается («Удалить» можно нажать ещё раз), а человек видит причину.
pub fn uninstall_from_log(
    home: &str,
    log_path: &str,
    content_id: &str,
    cats: &[ThemeCategory],
    remove_system: &mut dyn FnMut(&ThemeCategory, &str) -> (bool, String),
) -> (bool, String) {
    let mut data = crate::install_journal::store_log_view(log_path);
    let Some(rec) = data.get(content_id) else {
        return (false, tr("Theme not found in the installed-themes log"));
    };
    let paths: Vec<String> = rec["paths"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let name = rec["name"].as_str().unwrap_or(content_id).to_string();

    let mut removed = 0usize;
    let mut problems: Vec<String> = Vec::new();
    let mut local: Vec<String> = Vec::new();
    for p in &paths {
        // Виджет Conky: сначала погасить и снять автозапуск, потом удалять.
        crate::de_apply::conky_forget_widget(
            &crate::animations::SystemIo,
            &crate::de_apply::Ctx::live(),
            p,
        );
        match system_theme_of_path(cats, p) {
            Some((cat, theme)) => {
                if !path_present(p) {
                    continue; // уже снята — снимать нечего
                }
                let (ok, msg) = remove_system(&cat, &theme);
                if ok && !path_present(p) {
                    removed += 1;
                } else if !ok {
                    problems.push(msg);
                }
            }
            None => local.push(p.clone()),
        }
    }
    // Что снимать здесь — по общему журналу установки (шаг 3г слияния
    // установщиков): записанное покупкой и только своё, вместе со ссылкой
    // `~/.themes` на тему GTK. Системные темы уже снял помощник выше — их
    // шаги не повторяются. Записи нет (общий журнал не прочитан) — прежним
    // путём, по диску.
    let journal = crate::install_journal::store_view(log_path);
    let entry = journal.items.get(&crate::install_journal::key_of(crate::install_journal::Source::Store, content_id));
    let by_journal = entry.is_some();
    let steps: Vec<ExtStep> = match entry {
        Some(e) => crate::install_journal::removal_plan(e, &crate::install_journal::probe_live)
            .steps
            .into_iter()
            .filter(|st| {
                crate::install_journal::removal_path(st).is_some_and(|p| system_theme_of_path(cats, &p).is_none())
            })
            .collect(),
        None => plan_uninstall(&local, &|p| Path::new(p).is_dir(), &|p| Path::new(p).is_file()),
    };
    // Снятые пути покупки считаются в «Удалено (N путей)» — служебная
    // ссылка `~/.themes` нет (её и раньше снимала уборка, не считая).
    let counted = |path: &str| {
        let path = rstrip_slashes(path);
        paths.iter().any(|p| rstrip_slashes(p) == path)
    };
    // Виджет Conky уходит в корзину, а не стирается — как у «Удалить» его
    // плитки (`theme_removal::commit`): у виджета бывают правки человека.
    let conky_root = format!("{}/", crate::de_apply::conky_root(&crate::de_apply::Ctx::live()));
    let trash = home_trash_live();
    let date = crate::utils::trash_deletion_date_now();
    for step in &steps {
        let path = match step {
            ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => path,
            // Плана удаления из других шагов не бывает — молча пропускаем.
            _ => continue,
        };
        if path.starts_with(&conky_root) {
            match crate::utils::move_to_trash(Path::new(path), Path::new(&trash), &date) {
                Ok(_) => removed += usize::from(counted(path)),
                // TRANSLATORS: %1 is a Conky widget name, %2 the system error.
                Err(e) => problems.push(trf(
                    "Couldn't move “%1” to the trash: %2. The widget stays where it was.",
                    &[&name, &e],
                )),
            }
            continue;
        }
        let res = match step {
            ExtStep::RemoveTree { .. } => std::fs::remove_dir_all(path),
            _ => std::fs::remove_file(path),
        };
        match res {
            Ok(()) => {
                removed += usize::from(counted(path));
                if by_journal {
                    // Ссылку ~/.themes снял журнал; остатки спайса (переводы,
                    // настройки) в журнал не пишутся — их уборка остаётся.
                    crate::cinnamon_spices::cleanup_after_removal(
                        &crate::cinnamon_spices::SpiceEnv::for_home(home),
                        path,
                    );
                } else {
                    // Та же уборка, что у «Удалить» на плитке (ссылка
                    // ~/.themes, остатки спайса), — одно поведение у двух
                    // удалений.
                    crate::theme_removal::tidy_after_removal(home, path);
                }
            }
            // TRANSLATORS: %1 is a file or folder path, %2 is the system error.
            Err(e) => problems.push(trf("Couldn't remove %1: %2.", &[path, &e.to_string()])),
        }
    }

    let left: Vec<&String> = paths.iter().filter(|p| path_present(p)).collect();
    if !problems.is_empty() || !left.is_empty() {
        let mut message = if problems.is_empty() {
            trf("Couldn't remove %1.", &[&left.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")])
        } else {
            problems.join(" ")
        };
        message.push(' ');
        message.push_str(&trf(
            "“%1” stays in the list of installed themes.",
            &[&name],
        ));
        return (false, message);
    }

    crate::ocs_store::forget_installed(&mut data, content_id);
    // Части, лёгшие подпапками в снятую папку (рамка окон в теме GTK,
    // `plan_nested_part`), ушли вместе с ней — их записи тоже забываются.
    let gone_inside: Vec<String> = data
        .iter()
        .filter(|(_, rec)| {
            let inner: Vec<&str> = rec["paths"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            !inner.is_empty()
                && inner.iter().any(|q| paths.iter().any(|p| q.starts_with(&format!("{}/", rstrip_slashes(p)))))
                && !inner.iter().any(|q| path_present(q))
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in gone_inside {
        crate::ocs_store::forget_installed(&mut data, &id);
    }
    let mut message = uninstall_message(removed, &name);
    if let Err(e) = crate::install_journal::put_store_log(log_path, &data) {
        message.push(' ');
        message.push_str(&trf(
            "⚠️ The installed-themes log wasn't updated (%1) — the theme will stay \
             in the list until the next successful write.",
            &[&e],
        ));
    }
    (true, message)
}

/// Первые `n` байт файла (для распознавания картинки без имени).
fn read_head(path: &str, n: usize) -> Vec<u8> {
    use std::io::Read as _;
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; n];
    match f.read(&mut buf) {
        Ok(read) => {
            buf.truncate(read);
            buf
        }
        Err(_) => Vec::new(),
    }
}

/// Картинка фоном рабочего стола исполнителем слота: `(встала ли, текст
/// исполнителя)`. Название обоины для моста не выдумываем (пусто — «не
/// знаю», см. [`crate::wallpaper_exec::WallpaperExec::apply`]).
fn apply_static_with_slot<S: crate::wallpaper_exec::WpSys + Default>(
    slot: &crate::wallpaper_exec::ExecSlot<S>,
    path: &str,
) -> (bool, String) {
    crate::wallpaper_exec::with_slot(slot, |exec| {
        let res = exec.apply(path, "static", "");
        (res.ok, res.message)
    })
}

/// Размер экрана, по которому меряют картинку из пачки; `(0, 0)` — спросить
/// не у кого.
///
/// Оригинал спрашивал Qt (вкладка «Оформление» работает внутри приложения);
/// из ядра ответ даёт ОБЩИЙ опрос выходов
/// ([`crate::wallpaper_exec::live_screen_outputs`]) — тот же, которым живут
/// метки в командах чужих движков и раскладка обоев по мониторам. Своего
/// разбора `xrandr` здесь больше нет: он брал «первое разрешение со
/// звёздочкой», и на машине с разными мониторами пачку выбирал первый
/// активный режим — на большой монитор ехала картинка, растянутая с
/// меньшего.
///
/// ⚠️ Меркой служит САМЫЙ БОЛЬШОЙ выход. Из пачки на весь стол уходит ОДНА
/// картинка (её ставит общий движок статичных обоев, а он кроет стол
/// целиком — по картинке на монитор этот путь не умеет), и из двух зол
/// «большой монитор показывает свой размер, маленький ужимает» заметно
/// лучше, чем растянутое с маленького на большой.
///
/// ⚠️ Защита от дурака: инструмента опроса нет, опрос не удался, выходов
/// ноль, разрешение не разобралось — ответ `(0, 0)`, и ядро понимает его
/// как «не знаю»: выбор картинки уходит на сортировку по имени, ровно как
/// до всякого перечисления выходов.
fn screen_size() -> (u64, u64) {
    let session = crate::wallpaper_engine::detect_session_type(
        &env_str("XDG_SESSION_TYPE"),
        &env_str("WAYLAND_DISPLAY"),
        &env_str("DISPLAY"),
    );
    let outputs = crate::wallpaper_exec::live_screen_outputs(&session);
    crate::wallpaper_exec::largest_output(&outputs)
        .and_then(|o| o.size())
        .unwrap_or((0, 0))
}

// ============================================================================
// Сверки живого исполнителя
// ============================================================================
//
// Проверяется РАБОТА на настоящей файловой системе, а не тексты планов
// (тексты сверяет `tests/appearance_installer_parity.rs`): создался ли
// каталог, лёг ли файл, подставился ли путь скачанного вместо пустого `src`,
// переживает ли журнал перезапись, отказывает ли распаковка на побеге из
// каталога. Всё временное — под `std::env::temp_dir()` и убирается за собой.
//
// ⛔ Переменные окружения сверки НЕ трогают: `HOME` и `XDG_CONFIG_HOME` читают
// соседние модули (и их собственные сверки), а подмена на живой машине уводит
// запись в НАСТОЯЩИЙ профиль пользователя. Поэтому путь журнала в сверке
// задаётся явно, а не через `LiveInstaller::record`.
#[cfg(test)]
mod tests {
    use super::*;

    /// «Применить» у темы SDDM при другом экране входа говорит, что тема
    /// подействует только с SDDM; при SDDM и неизвестном — прежний итог.
    #[test]
    fn sddm_theme_note_names_other_login_manager() {
        let n = sddm_elsewhere_note("lightdm").unwrap();
        assert!(n.contains("LightDM") && n.contains("SDDM"), "{n}");
        assert!(sddm_elsewhere_note("gdm3").unwrap().contains("GDM"));
        for none in ["sddm", " sddm\n", "", "что-то-своё"] {
            assert_eq!(sddm_elsewhere_note(none), None, "{none:?}");
        }
    }

    /// Подменная машина исполнителя обоев: есть только названные программы,
    /// команды и запуски записываются, ничего не исполняется.
    #[derive(Default)]
    struct WpFake {
        tools: Vec<&'static str>,
        files: Vec<String>,
        log: Vec<String>,
        clock: f64,
    }

    impl crate::wallpaper_exec::WpSys for WpFake {
        fn run(&mut self, argv: &[String], _t: f64) -> Option<crate::session_splash::RunOut> {
            self.log.push(format!("run {}", argv.join(" ")));
            Some(crate::session_splash::RunOut { rc: 0, stdout: String::new(), stderr: String::new() })
        }
        fn spawn(&mut self, argv: &[String], _new_session: bool) -> Result<i64, String> {
            self.log.push(format!("spawn {}", argv.join(" ")));
            Ok(self.log.len() as i64)
        }
        fn poll(&mut self, _p: i64) -> Option<i64> {
            None
        }
        fn pid(&mut self, p: i64) -> Option<i64> {
            Some(1000 + p)
        }
        fn kill_process(&mut self, p: i64) {
            self.log.push(format!("kill {p}"));
        }
        fn has_tool(&mut self, tool: &str) -> bool {
            self.tools.contains(&tool)
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
        fn is_dir(&mut self, _path: &str) -> bool {
            false
        }
        fn listdir(&mut self, _path: &str) -> Vec<String> {
            Vec::new()
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
        fn write_text(&mut self, _path: &str, _content: &str) {}
        fn remove_file(&mut self, _path: &str) {}
        fn env(&mut self, name: &str) -> String {
            if name == "HOME" { "/fake-home".into() } else { String::new() }
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 1.0;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, _live: bool, _name: &str, _announce: bool) {}
    }

    fn wp_slot(de: &str, tools: &[&'static str], path: &str) -> crate::wallpaper_exec::ExecSlot<WpFake> {
        let sys = WpFake { tools: tools.to_vec(), files: vec![path.to_string()], ..WpFake::default() };
        std::sync::Arc::new(std::sync::Mutex::new(Some(crate::wallpaper_exec::WallpaperExec::with_env(sys, de, "wayland"))))
    }

    fn wp_log(slot: &crate::wallpaper_exec::ExecSlot<WpFake>) -> Vec<String> {
        crate::wallpaper_exec::with_slot(slot, |e| e.sys_mut().log.clone())
    }

    /// Обои из магазина на тайловых ставит ОБЩИЙ исполнитель процесса: демон
    /// swww поднимается, swaybg держит фон. Прежний свой исполнитель
    /// «Оформления» эти шаги отвергал, и на niri/river/Wayfire/Hyprland без
    /// hyprpaper картинка не вставала никогда.
    #[test]
    fn store_wallpaper_on_tiling_goes_through_shared_executor() {
        let pic = "/h/.local/share/wallpapers/лес.jpg";
        for de in ["niri", "river", "wayfire", "hyprland"] {
            let slot = wp_slot(de, &["swaybg"], pic);
            let (ok, _) = apply_static_with_slot(&slot, pic);
            assert!(ok, "{de}");
            let log = wp_log(&slot);
            assert!(log.iter().any(|l| l == &format!("spawn swaybg -i {pic} -m fill")), "{de}: {log:?}");
            // Вторая картинка снимает прежний держатель фона, а не кладёт
            // второй swaybg поверх.
            let (ok, _) = apply_static_with_slot(&slot, pic);
            assert!(ok);
            assert!(wp_log(&slot).iter().any(|l| l.starts_with("kill ")), "{de}: держатель не снят");

            let slot = wp_slot(de, &["swww"], pic);
            assert!(apply_static_with_slot(&slot, pic).0, "{de}");
            let log = wp_log(&slot);
            assert!(log.iter().any(|l| l == "run swww query"), "{de}: демон swww не проверен: {log:?}");
            assert!(log.iter().any(|l| l == &format!("run swww img {pic}")), "{de}: {log:?}");
        }
        // Ставить нечем — честный отказ с подсказкой, что поставить.
        let slot = wp_slot("niri", &[], pic);
        let (ok, msg) = apply_static_with_slot(&slot, pic);
        assert!(!ok);
        assert!(msg.contains("swww"), "{msg}");
        // Полные окружения — тем же исполнителем, родным ключом.
        let slot = wp_slot("gnome", &[], pic);
        assert!(apply_static_with_slot(&slot, pic).0);
        assert!(wp_log(&slot).iter().any(|l| l.starts_with("run gsettings set org.gnome.desktop.background picture-uri")));
    }

    /// Темы Deepin: стоп до записи в любом сеансе — `ok = false`, ни одной
    /// записи; причина — по сеансу: в Treeland «только Центр управления»,
    /// вне его — «Rexon поддерживает Deepin только на Treeland».
    #[test]
    fn deepin_theme_plan_stops_in_every_session() {
        let dir = tmp_dir("treeland");
        let path = dir.join("written").to_string_lossy().into_owned();
        let steps = vec![
            ApplyStep::DdeTreelandGate,
            ApplyStep::WriteSetsOk { dir: dir.to_string_lossy().into_owned(), path: path.clone(), content: "x".into() },
        ];
        for treeland in [true, false] {
            let r = exec_apply_plan_report_in(&steps, treeland);
            assert!(!r.ok);
            assert_eq!(r.reason, Some(crate::dde::refusal(treeland)));
            assert!(!Path::new(&path).exists(), "в Deepin ничего не пишем");
            assert_eq!(crate::themes::cursor_apply_message("deepin", &r), crate::dde::refusal(treeland));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Обои блокировки в Deepin на Treeland — через Центр управления
    /// (`type=lock`); вне Treeland — отказ.
    #[test]
    fn deepin_lock_wallpaper_in_treeland_goes_to_control_center() {
        let wl = lock_wallpaper_plan_in("deepin", true, "/h/Обои", "a b.png", true).unwrap();
        assert_eq!(wl.image.argv, crate::dde::control_center_wallpaper_argv("/h/Обои/a b.png", true));
        let page = wl.image.argv.last().unwrap();
        assert!(page.ends_with("&type=lock"), "{page}");
        assert!(page.contains("url=file:///h/%D0%9E%D0%B1%D0%BE%D0%B8/a%20b.png"), "{page}");
        assert_eq!(lock_wallpaper_plan_in("deepin", false, "/h", "a.png", true).err(), Some(crate::dde::not_treeland_message()));
        assert_eq!(lock_wallpaper_plan_in("kde", true, "/h", "a.png", true).unwrap().image.argv[0], "kwriteconfig6");
    }

    /// Свой каталог под сверку в `std::env::temp_dir()`.
    fn tmp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rexon_ainst_{tag}_{}_{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("временный каталог создан");
        path
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    // ── «Удалить» по журналу установленных (дефект 09.10.2026) ──────────

    /// Разделы с каталогами системных тем в песочнице `root`.
    fn sandbox_cats(root: &Path) -> Vec<ThemeCategory> {
        crate::categories::categories(root)
            .into_iter()
            .map(|mut c| {
                if c.requires_root {
                    c.install_dir = s(&root.join(format!("usr/share/{}/themes", c.apply_kind)));
                }
                c
            })
            .collect()
    }

    fn write_log(root: &Path, paths: &[&Path]) -> String {
        let log = s(&root.join("installed_themes.json"));
        let list: Vec<String> = paths.iter().map(|p| format!("\"{}\"", s(p))).collect();
        std::fs::write(&log, format!("{{\"777\": {{\"name\": \"Neon\", \"paths\": [{}]}}}}", list.join(","))).unwrap();
        log
    }

    fn log_has_entry(log: &str) -> bool {
        crate::ocs_store::parse_installed_log(read_text_lossy(log).as_deref()).contains_key("777")
    }

    /// Прежде: «Удалено (1 путей)», запись стёрта, каталог на месте.
    #[test]
    fn uninstall_keeps_log_when_folder_stays() {
        use std::os::unix::fs::PermissionsExt;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_stuck");
        let parent = root.join("locked");
        let theme = parent.join("neon");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("a"), "x").unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(&theme, std::fs::Permissions::from_mode(0o555)).unwrap();
        let log = write_log(&root, &[&theme]);
        let cats = sandbox_cats(&root);
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |_, _| panic!("не системная тема"));
        let stays = theme.is_dir();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&theme, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Под root права не мешают — тогда проверять нечего.
        if stays {
            assert!(!ok, "{msg}");
            assert!(log_has_entry(&log), "запись журнала стёрта при оставшейся теме");
            assert!(msg.contains("stays in the list"), "{msg}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Д4: удаление по журналу снимает и нашу ссылку `~/.themes/<тема>` —
    /// как «Удалить» на плитке; чужую ссылку с тем же именем не трогает.
    #[test]
    fn uninstall_from_log_drops_theme_link() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_link");
        let home = s(&root);
        let theme = root.join(".local/share/themes/Neon");
        std::fs::create_dir_all(theme.join("gtk-3.0")).unwrap();
        std::fs::create_dir_all(root.join(".themes")).unwrap();
        let link = root.join(".themes/Neon");
        std::os::unix::fs::symlink("../.local/share/themes/Neon", &link).unwrap();
        // Чужая ссылка под другим именем — ведёт не на нашу тему.
        let other = root.join(".themes/Other");
        std::os::unix::fs::symlink("/usr/share/themes/Other", &other).unwrap();
        let log = write_log(&root, &[&theme]);
        let (ok, msg) = uninstall_from_log(&home, &log, "777", &sandbox_cats(&root), &mut |_, _| panic!());
        assert!(ok, "{msg}");
        assert!(!theme.exists(), "тема осталась");
        assert!(std::fs::symlink_metadata(&link).is_err(), "висячая ссылка ~/.themes/Neon осталась");
        assert!(std::fs::symlink_metadata(&other).is_ok(), "снята чужая ссылка");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_local_theme_forgets_entry() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_local");
        let theme = root.join("icons/neon");
        let file = root.join("colors/Neon.colors");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x").unwrap();
        let gone = root.join("icons/gone");
        let log = write_log(&root, &[&theme, &file, &gone]);
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &sandbox_cats(&root), &mut |_, _| panic!());
        assert!(ok, "{msg}");
        assert_eq!(msg, uninstall_message(2, "Neon"));
        assert!(!theme.exists() && !file.exists());
        assert!(!log_has_entry(&log));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_system_theme_goes_through_helper() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_sys");
        let theme = root.join("usr/share/plymouth/themes/neon");
        std::fs::create_dir_all(&theme).unwrap();
        let log = write_log(&root, &[&theme]);
        let cats = sandbox_cats(&root);
        // Помощник отказал — тема на месте, запись цела, причина видна.
        let mut calls = Vec::new();
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |c, n| {
            calls.push((c.apply_kind.clone(), n.to_string()));
            (false, "REFUSED".to_string())
        });
        assert!(!ok && msg.contains("REFUSED") && log_has_entry(&log), "{msg}");
        assert_eq!(calls, vec![("plymouth".to_string(), "neon".to_string())]);
        // Помощник ответил «готово», но каталог остался — это не успех.
        let (ok, _) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |_, _| (true, String::new()));
        assert!(!ok && log_has_entry(&log));
        // Помощник снял каталог — запись забыта.
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |_, _| {
            std::fs::remove_dir_all(&theme).unwrap();
            (true, String::new())
        });
        assert!(ok, "{msg}");
        assert_eq!(msg, uninstall_message(1, "Neon"));
        assert!(!log_has_entry(&log));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn system_theme_path_is_direct_child_only() {
        let cats = crate::categories::categories(Path::new("/home/t"));
        let (c, n) = system_theme_of_path(&cats, "/usr/share/sddm/themes/sugar/").unwrap();
        assert_eq!((c.apply_kind.as_str(), n.as_str()), ("sddm", "sugar"));
        let (c, _) = system_theme_of_path(&cats, "/usr/share/plymouth/themes/neon").unwrap();
        assert_eq!(c.apply_kind, "plymouth");
        for p in [
            "/usr/share/plymouth/themes",
            "/usr/share/plymouth/themes/neon/sub",
            "/usr/share/plymouth/themes/..",
            "/usr/share/plymouth/themes/-R",
            "/home/t/.local/share/icons/neon",
            "neon",
        ] {
            assert!(system_theme_of_path(&cats, p).is_none(), "{p}");
        }
    }

    #[test]
    fn system_theme_remove_decisions() {
        use std::cell::RefCell;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cats = crate::categories::categories(Path::new("/home/t"));
        let ply = cats.iter().find(|c| c.apply_kind == "plymouth").unwrap();
        let sddm = cats.iter().find(|c| c.apply_kind == "sddm").unwrap();
        let env = RootEnv {
            is_plasmalogin: false,
            has_pkexec: true,
            system_helper_path: "/usr/lib/rexon-system-helper",
            system_helper_exists: true,
            is_steamos_or_immutable: false,
            is_steamos: false,
            family_is_debian: false,
        };
        let seen: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let reply = |rc: i32, err: &str| crate::utils::RunReport {
            outcome: RunOutcome::Ok,
            returncode: Some(rc),
            stderr: err.as_bytes().to_vec(),
            ..Default::default()
        };
        // Активная — отказ до окна пароля, помощник не зовётся.
        let (ok, msg) = system_theme_remove_with(&env, sddm, "sugar", &[], &["sugar".into()], &|a, _| {
            seen.borrow_mut().push(a.to_vec());
            reply(0, "")
        });
        assert!(!ok && msg.contains("current login screen theme"), "{msg}");
        let (ok, msg) = system_theme_remove_with(&env, ply, "neon", &[], &["neon".into()], &|_, _| reply(0, ""));
        assert!(!ok && msg.contains("current boot splash theme"), "{msg}");
        assert!(seen.borrow().is_empty());
        // Обычный путь — строка вызова помощника.
        let (ok, _) = system_theme_remove_with(&env, ply, "neon", &["--lang=ru".into()], &["other".into()], &|a, _| {
            seen.borrow_mut().push(a.to_vec());
            reply(0, "")
        });
        assert!(ok);
        assert_eq!(
            seen.borrow()[0],
            vec!["pkexec", "/usr/lib/rexon-system-helper", "--lang=ru", "remove-plymouth", "neon", "-", "arch"]
        );
        // Пароль не подтверждён; помощник отказал с причиной.
        let (ok, msg) = system_theme_remove_with(&env, ply, "neon", &[], &[], &|_, _| reply(126, ""));
        assert!(!ok && msg.contains("password"), "{msg}");
        let (ok, msg) = system_theme_remove_with(&env, ply, "neon", &[], &[], &|_, _| {
            reply(2, "rexon-helper: the theme neon belongs to a system package")
        });
        assert!(!ok && msg.contains("system package"), "{msg}");
        // Нет помощника; недопустимое имя.
        let no_helper = RootEnv { system_helper_exists: false, ..env };
        let (ok, _) = system_theme_remove_with(&no_helper, ply, "neon", &[], &[], &|_, _| panic!());
        assert!(!ok);
        let (ok, _) = system_theme_remove_with(&env, ply, "../x", &[], &[], &|_, _| panic!());
        assert!(!ok);
    }

    /// Каталог создаётся, скачанный файл копируется по явному источнику,
    /// ссылки дерева остаются ссылками.
    #[test]
    fn exec_steps_creates_dirs_copies_and_keeps_links() {
        let base = tmp_dir("exec");
        let archive = base.join("downloaded.png");
        std::fs::write(&archive, b"\x89PNG\r\n\x1a\n").expect("файл записан");
        let src_tree = base.join("tree");
        std::fs::create_dir_all(src_tree.join("inner")).expect("дерево создано");
        std::fs::write(src_tree.join("inner/a.txt"), "a").expect("файл записан");
        std::os::unix::fs::symlink("a.txt", src_tree.join("inner/link.txt"))
            .expect("ссылка создана");

        let dest_dir = base.join("dest");
        let steps = vec![
            ExtStep::MakeDirs { path: s(&dest_dir) },
            ExtStep::CopyFile {
                src: s(&archive),
                dest: s(&dest_dir.join("wallpaper.png")),
            },
            ExtStep::CopyTree {
                src: s(&src_tree),
                dest: s(&dest_dir.join("theme")),
            },
        ];
        exec_install_steps(&steps).expect("план исполнен");

        assert!(dest_dir.is_dir(), "каталог назначения создан");
        assert_eq!(
            std::fs::read(dest_dir.join("wallpaper.png")).expect("копия читается"),
            b"\x89PNG\r\n\x1a\n",
            "скачанный файл скопирован"
        );
        assert!(dest_dir.join("theme/inner/a.txt").is_file(), "дерево скопировано");
        let link = dest_dir.join("theme/inner/link.txt");
        assert!(
            link.symlink_metadata().expect("ссылка на месте").file_type().is_symlink(),
            "символьная ссылка осталась ссылкой, а не копией"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `RemoveTree` несуществующего пути не роняет план: у оригинала это
    /// `shutil.rmtree(..., ignore_errors=True)`, и следующие шаги обязаны
    /// выполниться.
    #[test]
    fn remove_tree_of_missing_path_does_not_stop_the_plan() {
        let base = tmp_dir("rmtree");
        let marker = base.join("after");
        let steps = vec![
            ExtStep::RemoveTree { path: s(&base.join("нет-такого")), safe: false, ignore_errors: true },
            ExtStep::MakeDirs { path: s(&marker) },
        ];
        exec_install_steps(&steps).expect("план исполнен целиком");
        assert!(marker.is_dir(), "шаг после снятия отсутствующего пути выполнен");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Журнал установленных переживает запись и перечитывание: это то, по
    /// чему вкладка узнаёт, что тема стоит, и по чему её потом снимают.
    #[test]
    fn install_log_survives_write_and_read() {
        let base = tmp_dir("log");
        let log_path = s(&base.join("rexon/ocs_installed.json"));

        let entry = crate::ocs_store::InstalledItem {
            content_id: "12345".to_string(),
            name: "Тема «Пример»".to_string(),
            rexon_type: kind_to_rexon_type("icons").to_string(),
            provider_id: "kde-look".to_string(),
            category_id: 132,
            ..crate::ocs_store::InstalledItem::default()
        };
        let paths = vec![s(&base.join("icons/Пример"))];
        let mut data = crate::ocs_store::parse_installed_log(None);
        crate::ocs_store::record_installed(&mut data, &entry, &paths, "2026-09-08T12:00:00");
        crate::install_journal::put_store_log(&log_path, &data).expect("журнал записан");

        let mut names: Vec<String> = std::fs::read_dir(base.join("rexon"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["install_journal.json", "ocs_installed.json"],
            "временный файл переименован, а не оставлен рядом"
        );
        // Зеркало — тот же текст, что писала прежняя запись журнала магазина.
        assert_eq!(
            read_text_lossy(&log_path).as_deref(),
            Some(crate::ocs_store::serialize_installed_log(&data).as_str())
        );
        let reread = crate::install_journal::store_log_view(&log_path);
        let rec = reread.get("12345").expect("запись найдена после перечитывания");
        assert_eq!(rec["name"].as_str(), Some("Тема «Пример»"));
        assert_eq!(rec["rexon_type"].as_str(), Some(kind_to_rexon_type("icons")));
        assert_eq!(rec["paths"][0].as_str(), Some(paths[0].as_str()));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// ZIP с побегом из каталога распаковки ОТКАЗЫВАЕТ и не пишет наружу
    /// ничего: это единственная защита от подменённого архива из Магазина.
    #[test]
    fn zip_with_escaping_entry_is_refused_and_writes_nothing() {
        use std::io::Write as _;

        let base = tmp_dir("zipslip");
        let archive = base.join("evil.zip");
        {
            let file = std::fs::File::create(&archive).expect("архив создан");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../побег.txt", options).expect("запись начата");
            zip.write_all("побег".as_bytes()).expect("запись удалась");
            zip.finish().expect("архив закрыт");
        }

        let dest = base.join("extracted");
        let err = live_safe_extract(&s(&archive), &s(&dest)).expect_err("побег отвергнут");
        // Сверка по исходной строке каталога, а не по тексту: язык процесса
        // сверок зависит от машины, а подпись переводится.
        let unsafe_entry = crate::i18n::trf("unsafe entry in the archive: %1", &[&"../побег.txt"]);
        assert_eq!(
            err, unsafe_entry,
            "причина отказа названа словами, а не общим «не удалось»: {err}"
        );
        assert!(
            !base.join("побег.txt").exists(),
            "наружу каталога распаковки не записано ничего"
        );
        assert_eq!(
            std::fs::read_dir(&dest).expect("каталог распаковки создан").count(),
            0,
            "внутри каталога распаковки тоже пусто"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Сторож расхождения: у КАЖДОГО типа из [`ACTUAL_TYPES_WITH_DIR`] есть и
    /// каталог установки, и вид применения. Разойдись список с таблицами — и
    /// снимок не покрыл бы каталог, куда уезжает переклассифицированная тема:
    /// прежняя копия темы осталась бы на диске рядом с новой.
    #[test]
    fn actual_types_have_dirs_and_kinds() {
        let home = Path::new("/home/тест");
        for actual in ACTUAL_TYPES_WITH_DIR {
            let dir = actual_type_dir(home, actual)
                .unwrap_or_else(|| panic!("{actual}: нет каталога установки"));
            assert!(!dir.is_empty(), "{actual}: каталог установки пуст");
            assert_ne!(
                actual_to_kind(actual),
                "none",
                "{actual}: нет вида применения"
            );
        }
    }

    /// Снимок распакованного дерева: тексты читаются только у метаданных, а
    /// дети целевых каталогов добавляются НЕрекурсивно — по ним планировщик
    /// узнаёт уже стоящую тему.
    #[test]
    fn tree_snapshot_reads_metadata_and_lists_dest_dirs() {
        let base = tmp_dir("snap");
        let root = base.join("extracted/Тема");
        std::fs::create_dir_all(root.join("contents")).expect("дерево создано");
        std::fs::write(root.join("metadata.json"), r#"{"KPlugin":{"Id":"Тема"}}"#)
            .expect("метаданные записаны");
        std::fs::write(root.join("contents/big.bin"), vec![0u8; 64]).expect("файл записан");

        let dest = base.join("installed");
        std::fs::create_dir_all(dest.join("Старая/глубоко/ещё")).expect("каталог создан");

        let snap = install_tree_snapshot(&s(&base.join("extracted")), &[s(&dest)]);
        assert_eq!(
            snap.read_text(&s(&root.join("metadata.json"))),
            Some(r#"{"KPlugin":{"Id":"Тема"}}"#),
            "текст метаданных прочитан"
        );
        assert_eq!(
            snap.read_text(&s(&root.join("contents/big.bin"))),
            Some(""),
            "содержимое обычного файла в снимок не тянется"
        );
        assert!(snap.is_dir(&s(&dest.join("Старая"))), "уже стоящая тема видна");
        // Первый уровень внутри темы виден (подпапка рамки в теме GTK,
        // `plan_nested_part`), глубже — нет.
        assert!(snap.is_dir(&s(&dest.join("Старая/глубоко"))));
        assert!(
            !snap.is_dir(&s(&dest.join("Старая/глубоко/ещё"))),
            "в целевой каталог снимок не спускается глубже детей темы"
        );
        assert_eq!(
            find_content_root(&snap, &s(&base.join("extracted"))),
            s(&root),
            "спуск в единственную верхнюю папку архива"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Живая распаковка TAR: имена берутся у системного `tar -tf`, извлекает
    /// он же. Нужен настоящий `tar` — на машине без него сверка не о чем.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный tar")]
    fn tar_archive_extracts_through_system_tar() {
        let base = tmp_dir("tar");
        let src = base.join("src");
        std::fs::create_dir_all(&src).expect("каталог создан");
        std::fs::write(src.join("тема.txt"), "содержимое").expect("файл записан");
        let archive = base.join("theme.tar.gz");
        let argv = vec![
            "tar".to_string(),
            "-czf".to_string(),
            s(&archive),
            "-C".to_string(),
            s(&src),
            "тема.txt".to_string(),
        ];
        assert!(run_ok(&argv, Some(60)), "архив собран системным tar");

        let dest = base.join("out");
        live_safe_extract(&s(&archive), &s(&dest)).expect("архив распакован");
        assert_eq!(
            std::fs::read_to_string(dest.join("тема.txt")).expect("файл на месте"),
            "содержимое"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Ссылки в 7z: разбор списка 7-Zip/p7zip, запись сквозь ссылку архива,
    /// куда ведёт ссылка.
    #[test]
    fn seven_zip_links_are_judged() {
        let slt = "Path = T\nAttributes = D drwxr-xr-x\n\nPath = T/a\nAttributes = A lrwxrwxrwx\n\n\
                   Path = T/f\nAttributes = A -rw-r--r--\n\nPath = T/w\nAttributes = A\n";
        let e = parse_seven_zip_slt(slt, "x.7z");
        assert_eq!(e, [("T".into(), false), ("T/a".into(), true), ("T/f".into(), false), ("T/w".into(), false)]);
        assert_eq!(entry_under_archive_link(&e), None);
        let bad = vec![("T/d".to_string(), true), ("T/d/pwned".to_string(), false)];
        assert_eq!(entry_under_archive_link(&bad).as_deref(), Some("T/d/pwned"));
        let d = "/tmp/x";
        assert!(!link_escapes(d, "/tmp/x/T/gtk-3.0/link.css", "gtk.css"));
        assert!(!link_escapes(d, "/tmp/x/T-dark/gtk-2.0", "../T/gtk-2.0"));
        assert!(link_escapes(d, "/tmp/x/T/up", "../../../etc"));
        assert!(link_escapes(d, "/tmp/x/T/abs", "/etc/passwd"));
        assert!(link_escapes(d, "/tmp/x/T/abs", "/tmp/x/T/f"), "абсолютная — всегда наружу");
    }

    /// Живая распаковка 7z со ссылками — каждой стоящей программой (bsdtar,
    /// 7-Zip `7z`/`7zz`, p7zip `7za`): ссылка внутри темы сохраняется,
    /// ссылка наружу, абсолютная и запись сквозь ссылку — отказ.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bsdtar или 7z")]
    fn seven_zip_links_extract_safely_with_every_tool() {
        let base = tmp_dir("7z-links");
        let bsdtar_ok = have("bsdtar");
        if !bsdtar_ok {
            return; // собирать архив нечем
        }
        // Архив: из каталога (ссылки как есть) либо из tar-потока со своими
        // записями — `bsdtar --format 7zip @tar`.
        let make = |name: &str, setup: &dyn Fn(&Path)| -> PathBuf {
            let src = base.join(format!("src-{name}"));
            std::fs::create_dir_all(&src).unwrap();
            setup(&src);
            let archive = base.join(format!("{name}.7z"));
            let argv: Vec<String> =
                vec!["bsdtar".into(), "--format".into(), "7zip".into(), "-cf".into(), s(&archive), "-C".into(), s(&src), "T".into()];
            assert!(run_ok(&argv, Some(60)), "архив {name} собран");
            archive
        };
        let inside = make("inside", &|src| {
            put(&src.join("T/gtk-3.0/gtk.css"), "css");
            std::os::unix::fs::symlink("gtk.css", src.join("T/gtk-3.0/link.css")).unwrap();
        });
        let up = make("up", &|src| {
            put(&src.join("T/gtk-3.0/gtk.css"), "css");
            std::os::unix::fs::symlink("../../../../../../etc", src.join("T/up")).unwrap();
        });
        let abs = make("abs", &|src| {
            put(&src.join("T/gtk-3.0/gtk.css"), "css");
            std::os::unix::fs::symlink("/etc/passwd", src.join("T/abs")).unwrap();
        });
        // Запись сквозь ссылку: `T/d → /куда-то`, потом `T/d/pwned`.
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let through = {
            let tarball = base.join("through.tar");
            let src = base.join("src-through");
            std::fs::create_dir_all(src.join("T")).unwrap();
            std::os::unix::fs::symlink(&outside, src.join("T/d")).unwrap();
            let src2 = base.join("src-through2");
            put(&src2.join("T/d/pwned"), "x");
            assert!(run_ok(&["bsdtar".into(), "-cf".into(), s(&tarball), "-C".into(), s(&src), "T".into()], Some(60)));
            assert!(run_ok(&["bsdtar".into(), "-rf".into(), s(&tarball), "-C".into(), s(&src2), "T/d/pwned".into()], Some(60)));
            let archive = base.join("through.7z");
            assert!(run_ok(
                &["bsdtar".into(), "--format".into(), "7zip".into(), "-cf".into(), s(&archive), format!("@{}", s(&tarball))],
                Some(60)
            ));
            archive
        };
        let tools: Vec<&str> = ["bsdtar", "7z", "7za", "7zz"].into_iter().filter(|t| have(t)).collect();
        for tool in &tools {
            let run = |archive: &Path, tag: &str| {
                let dest = base.join(format!("out-{tool}-{tag}"));
                std::fs::create_dir_all(&dest).unwrap();
                let entries = seven_zip_entries_with(tool, &s(archive)).expect("список записей");
                let dest_abs = abs_dest(&s(&dest));
                (seven_zip_extract(tool, &entries, &s(archive), &s(&dest), &dest_abs), dest)
            };
            let (res, dest) = run(&inside, "inside");
            res.unwrap_or_else(|e| panic!("{tool}: ссылка внутри темы — не отказ: {e}"));
            let link = dest.join("T/gtk-3.0/link.css");
            assert!(link.symlink_metadata().unwrap().file_type().is_symlink(), "{tool}: ссылка сохранилась");
            assert_eq!(std::fs::read_to_string(&link).unwrap(), "css");
            for (archive, tag) in [(&up, "up"), (&abs, "abs"), (&through, "through")] {
                let (res, _) = run(archive, tag);
                assert!(res.as_ref().is_err_and(|e| e.contains("T/")), "{tool} {tag}: {res:?}");
            }
            assert!(!outside.join("pwned").exists(), "{tool}: запись сквозь ссылку ушла наружу");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Живая распаковка 7z: `bsdtar` или 7-Zip (стенд 17.10.2026 — «Numix
    /// Holo» и «BlueSky» из магазина приходят именно так). Архив собирает
    /// тот же инструмент.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bsdtar или 7z")]
    fn seven_zip_archive_extracts_through_system_tool() {
        let base = tmp_dir("7z");
        let src = base.join("src");
        put(&src.join("NumixHolo/xfwm4/themerc"), "button_offset=0");
        let archive = base.join("159304-NumixHolo.7z");
        let tool = seven_zip_tool().expect("на машине есть bsdtar или 7z");
        let argv: Vec<String> = if tool == "bsdtar" {
            vec![tool.into(), "--format".into(), "7zip".into(), "-cf".into(), s(&archive), "-C".into(), s(&src), "NumixHolo".into()]
        } else {
            vec![tool.into(), "a".into(), s(&archive), s(&src.join("NumixHolo"))]
        };
        assert!(run_ok(&argv, Some(60)), "архив собран: {tool}");
        assert!(is_archive_file(&s(&archive)), "7z опознан архивом");
        let dest = base.join("out");
        live_safe_extract(&s(&archive), &s(&dest)).expect("архив распакован");
        assert!(dest.join("NumixHolo/xfwm4/themerc").is_file());
        // Не 7z, а текст: подпись не совпала — архивом не считается.
        let text = base.join("scheme.colors");
        std::fs::write(&text, "[General]\n").unwrap();
        assert!(!is_archive_file(&s(&text)));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Распаковать 7z нечем: отказ называет программу и пакеты семейства, а
    /// перед установкой товара с файлом `.7z` ставится bsdtar (пакеты
    /// сверены по репозиториям Arch, Debian, Fedora, openSUSE 19.10.2026).
    #[test]
    fn seven_zip_without_tool_says_what_to_install() {
        let debian = seven_zip_missing("debian");
        assert!(debian.contains("bsdtar") && debian.contains("libarchive-tools") && debian.contains("7zip"), "{debian}");
        assert!(seven_zip_missing("arch").contains("libarchive"));
        assert_eq!(seven_zip_package("arch"), Some("libarchive"));
        assert_eq!(seven_zip_package("debian"), Some("libarchive-tools"));
        assert_eq!(seven_zip_package("fedora"), Some("bsdtar"));
        assert_eq!(seven_zip_package("suse"), Some("bsdtar"));
        assert_eq!(seven_zip_package("gentoo"), None, "незнакомое семейство — установку не предлагаем");

        // Распаковщика нет — ставится сам, без вопроса «Да/Нет».
        assert_eq!(seven_zip_install_package("159304-NumixHolo.7z", false, Some("bsdtar")).as_deref(), Some("bsdtar"));
        assert!(seven_zip_install_package("A.7Z", false, Some("bsdtar")).is_some(), "расширение без регистра");
        assert!(seven_zip_install_package("a.7z", true, Some("bsdtar")).is_none(), "распаковщик есть");
        assert!(seven_zip_install_package("a.tar.xz", false, Some("bsdtar")).is_none());
        assert!(seven_zip_install_package("a.7z", false, None).is_none(), "поставить нечем");
        // Не встал — товар не ставится; пароль закрыт — совет повторить.
        let canceled = seven_zip_install_failed(&password_not_confirmed());
        assert!(canceled.contains("bsdtar") && !canceled.contains(&password_not_confirmed()), "{canceled}");
        let failed = seven_zip_install_failed("pacman: error");
        assert!(failed.contains("bsdtar") && failed.contains("pacman: error"), "{failed}");

        // 7z по подписи — архив, даже когда распаковать его нечем (не «голый
        // файл темы»).
        let base = tmp_dir("7z-magic");
        let archive = base.join("t.7z");
        let mut bytes = SEVEN_ZIP_MAGIC.to_vec();
        bytes.extend_from_slice(&[0; 26]);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(&archive, &bytes).unwrap();
        assert!(is_archive_file(&s(&archive)));
        let _ = std::fs::remove_dir_all(&base);
    }

    // ── Разделы окружений: настоящие раскладки архивов pling ─────────────
    //
    // Фикстуры повторяют строение архивов, снятых стендом 17.10.2026
    // (ChrButtons, Win8-Metacity, Rosepine, Breeze-Lxqt, Semabe, Extra) —
    // в миниатюре, сами архивы в дерево не кладутся.

    /// План + исполнение архивной установки так, как её зовёт живой
    /// установщик: от каталога распаковки, с именем из магазина.
    fn install_archive_tree(home: &Path, ex: &Path, kind: &str, download_name: &str) -> InstallOutcome {
        install_archive_tree_own(home, ex, kind, download_name, &[])
    }

    /// [`install_archive_tree`] с путями, которые эта покупка ставила раньше.
    fn install_archive_tree_own(home: &Path, ex: &Path, kind: &str, download_name: &str, own: &[String]) -> InstallOutcome {
        let cat = category_by_kind(home, kind).expect("раздел окружения");
        let mut dest_dirs = vec![cat.install_dir.clone()];
        if matches!(de_theme_marker(kind), Some(DeThemeMarker::Subdir(_))) {
            dest_dirs.push(crate::installed_scan::xdg_themes_dir(&s(home)));
        }
        let snap = install_tree_snapshot(&s(ex), &dest_dirs);
        let fallback = archive_theme_name(download_name, "Товар");
        let origin = ArchiveOrigin { extract_dir: &s(ex), fallback_name: &fallback };
        let out = plan_install_archive(home, &snap, &cat, origin, Some(own));
        if out.ok {
            exec_install_steps(&out.steps).expect("шаги исполнены");
        }
        out
    }

    /// Виджет Conky из магазина: шрифты архива (любая глубина, в том числе
    /// `.fonts/` пакетов менеджера Conky) — в свою папку
    /// `~/.local/share/fonts/rexon-conky-<имя>/`; она записана в журнал
    /// рядом с виджетом (её снимет «Удалить»), кэш шрифтов обновляется.
    #[test]
    fn conky_store_widget_installs_its_fonts() {
        let base = tmp_dir("conky_fonts");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("victorConky/ArchLarge"), "TEXT\n${font Zekton:size=13}x\n");
        put(&ex.join("fonts/Zekton.ttf"), "ttf");
        put(&ex.join("fonts/readme.txt"), "r");
        put(&ex.join(".fonts/Ubuntu.otf"), "otf");
        put(&ex.join("install.sh"), "cp -r fonts ~/.fonts\n");
        let cat = category_by_kind(&home, "conky").expect("раздел Conky");
        let plan = |ex: &Path, name: &str| {
            let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone()]);
            let origin = ArchiveOrigin { extract_dir: &s(ex), fallback_name: name };
            plan_install_archive(&home, &snap, &cat, origin, Some(&[]))
        };
        let out = plan(&ex, "victorConky1.1");
        assert!(out.ok, "{}", out.message);
        let fonts = s(&home.join(".local/share/fonts/rexon-conky-victorConky1.1"));
        let widget = s(&home.join(".config/conky/victorConky1.1"));
        assert_eq!(out.record.as_ref().expect("запись").paths, [widget, fonts.clone()]);
        for f in ["fonts/Zekton.ttf", ".fonts/Ubuntu.otf"] {
            let name = f.rsplit('/').next().unwrap();
            let want = ExtStep::CopyFile { src: s(&ex.join(f)), dest: format!("{fonts}/{name}") };
            assert!(out.steps.contains(&want), "{f}: {:#?}", out.steps);
        }
        assert!(!out.steps.iter().any(|st| matches!(st, ExtStep::CopyFile { dest, .. } if dest.ends_with("readme.txt"))));
        assert!(out.steps.iter().any(|st| matches!(st, ExtStep::Run { argv, .. } if argv[0] == "fc-cache")));

        // Без шрифтов — ни папки шрифтов в журнале, ни fc-cache.
        let bare = base.join("bare");
        put(&bare.join("Gotham/conky.conf"), "conky.text = [[x]]\n");
        let out = plan(&bare, "Gotham");
        assert_eq!(out.record.expect("запись").paths, [s(&home.join(".config/conky/Gotham"))]);
        assert!(!out.steps.iter().any(|st| matches!(st, ExtStep::Run { .. })));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Пакет тем Conky (`.cmtp.7z` и архивы магазина с той же раскладкой):
    /// каждый виджет из `.conky/` — своя папка, голый файл из `.Conky/` —
    /// папка с ним же, шрифты `.fonts/` и `.conky/fonts/` — в одну папку
    /// пакета; занятое чужим виджетом имя не затирается.
    #[test]
    fn conky_theme_pack_installs_each_widget() {
        let base = tmp_dir("conky_pack");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join(".conky/Gotham/Gotham"), "conky.text = [[x]]\n");
        put(&ex.join(".conky/Conky Seamod/conky_seamod"), "TEXT\nx\n");
        put(&ex.join(".conky/Conky Seamod/rings.lua"), "-- lua\n");
        put(&ex.join(".conky/Green Apple Desktop/Green Apple Desktop.cmtheme"), "~/.conky/Green Apple Desktop/Gotham\n");
        put(&ex.join(".conky/fonts/Ubuntu.ttf"), "ttf");
        put(&ex.join(".conky/README"), "r");
        put(&ex.join(".Conky/conkyx"), "TEXT\ny\n");
        put(&ex.join(".fonts/symbols.ttf"), "ttf");
        put(&home.join(".config/conky/Gotham/mine.conf"), "conky.text = [[mine]]\n");
        let cat = category_by_kind(&home, "conky").expect("раздел Conky");
        let snap = install_tree_snapshot(&s(&ex), &[cat.install_dir.clone()]);
        let origin = ArchiveOrigin { extract_dir: &s(&ex), fallback_name: "default-themes-2.1.cmtp" };
        let out = plan_install_archive(&home, &snap, &cat, origin, Some(&[]));
        assert!(out.ok, "{}", out.message);
        let conky = s(&home.join(".config/conky"));
        let fonts = s(&home.join(".local/share/fonts/rexon-conky-default-themes-2.1"));
        assert_eq!(out.names, ["Conky Seamod", "Green Apple Desktop", "conkyx"]);
        assert_eq!(
            out.record.as_ref().expect("запись").paths,
            [
                format!("{conky}/Conky Seamod"),
                format!("{conky}/Green Apple Desktop"),
                format!("{conky}/conkyx"),
                fonts.clone(),
            ]
        );
        // Чужой «Gotham» человека не тронут — и сказано об этом.
        assert!(out.message.contains("Gotham"), "{}", out.message);
        assert!(!out.steps.iter().any(|st| matches!(st, ExtStep::RemoveTree { path, .. } if path.ends_with("/Gotham"))));
        let loose = ExtStep::CopyFile { src: s(&ex.join(".Conky/conkyx")), dest: format!("{conky}/conkyx/conkyx") };
        assert!(out.steps.contains(&loose), "{:#?}", out.steps);
        for f in [".conky/fonts/Ubuntu.ttf", ".fonts/symbols.ttf"] {
            let name = f.rsplit('/').next().unwrap();
            let want = ExtStep::CopyFile { src: s(&ex.join(f)), dest: format!("{fonts}/{name}") };
            assert!(out.steps.contains(&want), "{f}: {:#?}", out.steps);
        }
        exec_install_steps(&out.steps).expect("шаги исполнены");
        assert!(home.join(".config/conky/Conky Seamod/rings.lua").is_file());
        assert!(!home.join(".config/conky/fonts").exists() && !home.join(".config/conky/README").exists());

        // Архив без папок пакета — по-прежнему один виджет.
        let one = base.join("one");
        put(&one.join("Mimosa/Mimosa.conf"), "conky.text = [[x]]\n");
        let snap = install_tree_snapshot(&s(&one), &[cat.install_dir.clone()]);
        let origin = ArchiveOrigin { extract_dir: &s(&one), fallback_name: "Mimosa" };
        let out = plan_install_archive(&home, &snap, &cat, origin, Some(&[]));
        assert_eq!(out.names, ["Mimosa"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Запасная ветка «обои / шрифты и прочее» по правилу владельца: каталог
    /// с тем же именем, который эта покупка не ставила (человек, другая
    /// покупка, ссылка), не снимается — отказ с причиной, ни одного шага;
    /// записанный журналом этой покупки — снимается и ставится заново.
    #[test]
    fn fallback_branch_keeps_foreign_dir() {
        let base = tmp_dir("fallback_owner");
        let home = base.join("home");
        for (kind, dir) in [("wallpapers", ".local/share/wallpapers"), ("fonts", ".local/share/fonts")] {
            let ex = base.join(format!("ex-{kind}"));
            put(&ex.join("Gold/contents/a.jpg"), "v2");
            let dest = home.join(dir).join("Gold");
            put(&dest.join("mine.txt"), "моё");
            let out = install_tree(&home, &ex, kind, Some(&[]));
            assert!(!out.ok, "{kind}: чужой каталог перезаписан: {}", out.message);
            assert_eq!(out.message, skipped_note("Gold", SkipReason::TakenByOther), "{kind}");
            assert!(out.steps.is_empty() && out.record.is_none(), "{kind}: {:#?}", out.steps);
            assert_eq!(std::fs::read_to_string(dest.join("mine.txt")).unwrap(), "моё", "{kind}");

            // Ссылка на месте — тоже чужое.
            std::fs::remove_dir_all(&dest).unwrap();
            std::os::unix::fs::symlink(base.join("outside"), &dest).unwrap();
            let out = install_tree(&home, &ex, kind, Some(&[]));
            assert!(!out.ok, "{kind}: ссылка заменена: {}", out.message);
            std::fs::remove_file(&dest).unwrap();

            // Своя прежняя покупка — снимается и ставится заново.
            put(&dest.join("old.jpg"), "v1");
            let own = vec![s(&dest)];
            let out = install_tree(&home, &ex, kind, Some(&own));
            assert!(out.ok, "{kind}: своя прежняя не заменена: {}", out.message);
            assert_eq!(out.record.expect("запись").paths, own, "{kind}");
            assert!(!dest.join("old.jpg").exists(), "{kind}: прежняя копия не снята");
            assert_eq!(std::fs::read_to_string(dest.join("contents/a.jpg")).unwrap(), "v2", "{kind}");

            // Без журнала (`None`, эталон Python) — прежнее «перезаписывается».
            let out = install_tree(&home, &ex, kind, None);
            assert!(out.ok, "{kind}: {}", out.message);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Папка шрифтов виджета Conky (одиночного и пакета) с тем же именем от
    /// другой покупки не пополняется и в журнал этой покупки не пишется:
    /// виджет ставится, шрифты пропускаются с причиной. Своя — заменяется.
    #[test]
    fn conky_font_dir_of_other_purchase_is_kept() {
        let base = tmp_dir("conky_font_owner");
        let home = base.join("home");
        let cat = category_by_kind(&home, "conky").expect("раздел Conky");
        let fonts_base = s(&home.join(".local/share/fonts"));
        let plan = |ex: &Path, name: &str, own: &[String]| {
            let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone(), fonts_base.clone()]);
            let origin = ArchiveOrigin { extract_dir: &s(ex), fallback_name: name };
            plan_install_archive(&home, &snap, &cat, origin, Some(own))
        };
        let one = base.join("one");
        put(&one.join("victor/conky.conf"), "conky.text = [[x]]\n");
        put(&one.join("fonts/Zekton.ttf"), "ttf");
        let pack = base.join("pack");
        put(&pack.join(".conky/Gotham/Gotham"), "conky.text = [[x]]\n");
        put(&pack.join(".fonts/symbols.ttf"), "ttf");
        for (ex, name, font_name) in [(&one, "victor", "rexon-conky-victor"), (&pack, "themes.cmtp", "rexon-conky-themes")] {
            let font_dir = home.join(".local/share/fonts").join(font_name);
            put(&font_dir.join("other.ttf"), "чужой");
            let out = plan(ex, name, &[]);
            assert!(out.ok, "{name}: {}", out.message);
            assert!(out.message.contains(&skipped_note(font_name, SkipReason::TakenByOther)), "{name}: {}", out.message);
            let paths = out.record.expect("запись").paths;
            assert!(!paths.contains(&s(&font_dir)), "{name}: чужая папка шрифтов в журнале: {paths:?}");
            assert!(
                !out.steps.iter().any(|st| match st {
                    ExtStep::RemoveTree { path, .. } | ExtStep::MakeDirs { path } => path == &s(&font_dir),
                    ExtStep::CopyFile { dest, .. } => dest.starts_with(&s(&font_dir)),
                    _ => false,
                }),
                "{name}: шаги трогают чужую папку шрифтов: {:#?}",
                out.steps
            );
            // Своя (записана журналом) — снимается и ставится заново.
            let out = plan(ex, name, &[s(&font_dir)]);
            assert!(out.ok, "{name}: {}", out.message);
            assert!(out.record.expect("запись").paths.contains(&s(&font_dir)), "{name}");
            assert!(out.steps.contains(&ExtStep::RemoveTree { path: s(&font_dir), safe: false, ignore_errors: true }), "{name}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Системная тема (SDDM, Plymouth): каталог темы с тем же именем, который
    /// эта покупка не ставила (тема пакета, человека), — отказ до окна
    /// пароля: помощник копирует поверх, поэтому решение — здесь.
    #[test]
    fn root_install_refuses_foreign_system_theme() {
        let base = tmp_dir("root_owner");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("MyTheme/metadata.desktop"), "[SddmGreeterTheme]\nName=My\n");
        put(&ex.join("MyTheme/Main.qml"), "Item {}\n");
        let cat = category_by_kind(&home, "sddm").expect("раздел SDDM");
        let env = RootEnv {
            is_steamos: false,
            is_steamos_or_immutable: false,
            is_plasmalogin: false,
            has_pkexec: true,
            system_helper_exists: true,
            system_helper_path: "/usr/lib/rexon/helper",
            family_is_debian: false,
        };
        let snap = install_tree_snapshot(&s(&ex), &[]);
        let root = s(&ex.join("MyTheme"));
        let dest = join(&cat.install_dir, "MyTheme");
        let err = plan_root_install_with(&env, &snap, &cat, &root, &|p| p == dest).unwrap_err();
        assert_eq!(err.message, skipped_note("MyTheme", SkipReason::TakenByOther));
        let ok = plan_root_install_with(&env, &snap, &cat, &root, &|_| false).expect("свободно — план");
        assert_eq!(ok.dest, dest);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn de_theme_markers_come_from_matrix() {
        assert_eq!(de_theme_marker("xfwm4"), Some(DeThemeMarker::Subdir("xfwm4")));
        assert_eq!(de_theme_marker("marco"), Some(DeThemeMarker::Subdir("metacity-1")));
        assert_eq!(de_theme_marker("gnomeshell"), Some(DeThemeMarker::Subdir("gnome-shell")));
        assert_eq!(de_theme_marker("cinnamontheme"), Some(DeThemeMarker::Subdir("cinnamon")));
        assert_eq!(de_theme_marker("lxqttheme"), Some(DeThemeMarker::File("lxqt-panel.qss")));
        for other in ["gtk", "gnomeext", "cinnamonapplets", "nemoactions", "conky", "tabbox"] {
            assert_eq!(de_theme_marker(other), None, "{other}");
        }
    }

    #[test]
    fn archive_theme_name_strips_extension_and_legacy_number() {
        assert_eq!(archive_theme_name("Rosepine-B-GS-Dark.tar.xz", "x"), "Rosepine-B-GS-Dark");
        assert_eq!(archive_theme_name("99823-Miu with xfwm.tar.gz", "x"), "Miu with xfwm");
        assert_eq!(archive_theme_name("Marble-shell.zip", "x"), "Marble-shell");
        assert_eq!(archive_theme_name("159304-NumixHolo.7z", "x"), "NumixHolo");
        assert_eq!(archive_theme_name("", "Win 8 / Metacity"), "Win 8 - Metacity");
        assert_eq!(archive_theme_name(".tar.gz", ""), "theme");
        // Имя файла задаёт автор товара: пробел перед точками не должен
        // превращать имя темы в «..» или «.» — иначе `~/.themes/..` = `~`.
        for dl in [" ...zip", " ..zip", "1- ...tar.gz", " .hidden.zip"] {
            let n = archive_theme_name(dl, "Item");
            assert_eq!(n, "Item", "{dl:?} → {n:?}");
        }
    }

    /// Безымянный корень архива с опасным именем файла не ложится в HOME.
    #[test]
    fn archive_without_folder_never_lands_above_the_theme_dir() {
        let base = tmp_dir("dotdot_name");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("gnome-shell/gnome-shell.css"), "x");
        put(&ex.join(".evilrc"), "x");
        let _ = install_archive_tree(&home, &ex, "gnomeshell", " ...tar.xz");
        assert!(!home.join(".evilrc").exists(), "файл архива лёг прямо в HOME");
    }

    /// Имя пакета из metadata — тоже чужие данные: «../..» в `KPlugin.Id` не
    /// выводит установку из каталога вида.
    #[test]
    fn metadata_id_cannot_climb_out_of_the_kind_dir() {
        let snap = |file: &str, text: &str| {
            FsSnapshot::from_json(&serde_json::json!({
                "dirs": {"/x/Pkg": [file]},
                "files": {format!("/x/Pkg/{file}"): text},
            }))
        };
        let fs = snap("metadata.json", r#"{"KPlugin": {"Id": "../../../evil"}}"#);
        assert_eq!(theme_id_from(&fs, "/x/Pkg"), "Pkg");
        let fs = snap("metadata.desktop", "X-KDE-PluginInfo-Name=..\n");
        assert_eq!(theme_id_from(&fs, "/x/Pkg"), "Pkg");
        // Обычный Id остаётся.
        let fs = snap("metadata.json", r#"{"KPlugin": {"Id": "org.kde.good"}}"#);
        assert_eq!(theme_id_from(&fs, "/x/Pkg"), "org.kde.good");
    }

    /// Рамка окон с именем стоящей темы GTK ложится подпапкой в её папку
    /// (через нашу ссылку `~/.themes/<имя>` — в `~/.local/share/themes`),
    /// ссылка остаётся ссылкой; повторная установка той же покупки заменяет
    /// только подпапку; чужая подпапка не затирается; имени нет — отдельная
    /// папка, как раньше.
    #[test]
    fn frame_with_gtk_theme_name_goes_inside_the_gtk_folder() {
        let base = tmp_dir("frame_nest");
        let home = base.join("home");
        let xdg = home.join(".local/share/themes");
        let legacy = home.join(".themes");
        put(&xdg.join("Arc/gtk-3.0/gtk.css"), "gtk");
        std::fs::create_dir_all(&legacy).unwrap();
        std::os::unix::fs::symlink("../.local/share/themes/Arc", legacy.join("Arc")).unwrap();
        put(&legacy.join("Plain/gtk-2.0/gtkrc"), "gtk2");

        let ex = base.join("marco");
        put(&ex.join("Arc/metacity-1/metacity-theme-3.xml"), "<metacity_theme/>");
        put(&ex.join("Arc/README"), "r");
        let out = install_archive_tree(&home, &ex, "marco", "Arc-metacity.tar.gz");
        assert!(out.ok, "{}", out.message);
        let part = s(&xdg.join("Arc/metacity-1"));
        assert_eq!(out.record.as_ref().expect("запись").paths, [part.clone()]);
        assert!(xdg.join("Arc/metacity-1/metacity-theme-3.xml").is_file());
        assert!(xdg.join("Arc/gtk-3.0/gtk.css").is_file(), "тема GTK цела");
        assert!(!xdg.join("Arc/README").exists(), "из архива — только подпапка рамки");
        assert!(std::fs::symlink_metadata(legacy.join("Arc")).unwrap().file_type().is_symlink());

        // Та же покупка ещё раз — заменяется подпапка, тема GTK цела.
        put(&ex.join("Arc/metacity-1/metacity-theme-3.xml"), "<metacity_theme v2/>");
        let again = install_archive_tree_own(&home, &ex, "marco", "Arc-metacity.tar.gz", &[part.clone()]);
        assert!(again.ok, "{}", again.message);
        assert_eq!(std::fs::read_to_string(xdg.join("Arc/metacity-1/metacity-theme-3.xml")).unwrap(), "<metacity_theme v2/>");
        assert!(xdg.join("Arc/gtk-3.0/gtk.css").is_file());
        // Другая покупка с тем же именем — чужую подпапку не трогает.
        let other = install_archive_tree(&home, &ex, "marco", "Arc-metacity.tar.gz");
        assert!(!other.ok, "{}", other.message);

        // xfwm4 в настоящую папку `~/.themes/Plain` (не ссылку).
        let ex = base.join("xfwm");
        put(&ex.join("Plain/xfwm4/themerc"), "button_offset=0");
        let out = install_archive_tree(&home, &ex, "xfwm4", "Plain.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.record.expect("запись").paths, [s(&legacy.join("Plain/xfwm4"))]);
        assert!(legacy.join("Plain/gtk-2.0/gtkrc").is_file());

        // Имени нет — отдельная папка в `~/.themes`.
        let ex = base.join("solo");
        put(&ex.join("Solo/xfwm4/themerc"), "x");
        let out = install_archive_tree(&home, &ex, "xfwm4", "Solo.tar.gz");
        assert_eq!(out.record.expect("запись").paths, [s(&legacy.join("Solo"))]);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Несколько вариантов под общей обёрткой — каждый на верхний уровень
    /// `~/.themes` (xfwm4: ChrButtons) и без обёртки вовсе (Marco: Win8).
    #[test]
    fn de_frame_variants_land_on_top_level() {
        let base = tmp_dir("de_variants");
        let home = base.join("home");
        let themes = home.join(".themes");

        let ex = base.join("xfwm");
        put(&ex.join("ChrButtons/README"), "r");
        for v in ["ChrBlueButtons", "ChrDarkButtons"] {
            put(&ex.join(format!("ChrButtons/{v}/xfwm4/themerc")), "button_offset=2");
        }
        let out = install_archive_tree(&home, &ex, "xfwm4", "ChrButtons.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["ChrBlueButtons", "ChrDarkButtons"]);
        assert!(themes.join("ChrDarkButtons/xfwm4/themerc").is_file());
        assert!(!themes.join("ChrButtons").exists(), "обёртка не ставится");
        assert_eq!(out.record.expect("запись").rexon_type, "theme-xfwm4");

        let ex = base.join("marco");
        for v in ["Win8-05-Saffron", "Win8-16-Alabaster"] {
            put(&ex.join(format!("{v}/metacity-1/metacity-theme-3.xml")), "<metacity_theme/>");
        }
        let out = install_archive_tree(&home, &ex, "marco", "Win8-Metacity-v1.4.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Win8-05-Saffron", "Win8-16-Alabaster"]);
        assert!(themes.join("Win8-16-Alabaster/metacity-1/metacity-theme-3.xml").is_file());
        assert!(!themes.join("extracted").exists() && !themes.join("marco").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Архив, где в корне сразу `gnome-shell/` (Rosepine): имя — из файла
    /// магазина, тема — `~/.themes/<имя>/gnome-shell`.
    #[test]
    fn shell_theme_without_own_folder_takes_archive_name() {
        let base = tmp_dir("de_noname");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("gnome-shell/gnome-shell.css"), "stage {}");
        put(&ex.join("gnome-shell/assets/a.svg"), "<svg/>");
        let out = install_archive_tree(&home, &ex, "gnomeshell", "Rosepine-B-GS-Dark.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Rosepine-B-GS-Dark"]);
        let theme = home.join(".themes/Rosepine-B-GS-Dark");
        assert!(theme.join("gnome-shell/gnome-shell.css").is_file());
        assert!(theme.join("gnome-shell/assets/a.svg").is_file());
        assert!(!home.join(".themes/gnome-shell").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Темы LXQt узнаются по `lxqt-panel.qss` на любой глубине (Breeze-Lxqt:
    /// `Breeze-Lxqt/themes/<имя>/`); соседние темы Openbox не ставятся.
    /// Cinnamon: варианты на три уровня вглубь (Semabe), комбинированная
    /// GTK-тема ставится целиком.
    #[test]
    fn lxqt_and_cinnamon_themes_found_deep_inside() {
        let base = tmp_dir("de_deep");
        let home = base.join("home");

        let ex = base.join("lxqt");
        let w = ex.join("Breeze-Dark-Light-Lxqt-Ob");
        put(&w.join("Breeze-Dark-Ob/openbox-3/themerc"), "x");
        put(&w.join("Wallpaper-Dark.jpg"), "jpg");
        for v in ["Breeze Dark", "Breeze Light"] {
            put(&w.join(format!("Breeze-Lxqt/themes/{v}/lxqt-panel.qss")), "LXQtPanel {}");
        }
        let out = install_archive_tree(&home, &ex, "lxqttheme", "Breeze-Dark-Light-Lxqt-Ob.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Breeze Dark", "Breeze Light"]);
        let lxqt = home.join(".local/share/lxqt/themes");
        assert_eq!(listing(&lxqt), ["Breeze Dark", "Breeze Light"]);
        assert!(lxqt.join("Breeze Light/lxqt-panel.qss").is_file());

        let ex = base.join("cinnamon");
        put(&ex.join("LED/gnome/large/Semabe Azure (LED)LG/cinnamon/cinnamon.css"), "");
        put(&ex.join("LED/left/small/Semabe Grey (LED)SL/cinnamon/cinnamon.css"), "");
        put(&ex.join("LED/left/small/Semabe Grey (LED)SL/gtk-3.0/gtk.css"), "");
        let out = install_archive_tree(&home, &ex, "cinnamontheme", "semabe-LED-3.6.2.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Semabe Azure (LED)LG", "Semabe Grey (LED)SL"]);
        let grey = home.join(".themes/Semabe Grey (LED)SL");
        assert!(grey.join("cinnamon/cinnamon.css").is_file() && grey.join("gtk-3.0/gtk.css").is_file());
        assert!(!home.join(".themes/LED").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// В архиве нет темы своего вида (шрифты и обои под видом темы
    /// Cinnamon; тема Openbox в разделе LXQt) — отказ, на диске ничего.
    #[test]
    fn de_archive_without_theme_of_its_kind_is_refused() {
        let base = tmp_dir("de_none");
        let home = base.join("home");
        let ex = base.join("cin");
        put(&ex.join("Extra 1.4/Roboto-Regular.ttf"), "ttf");
        put(&ex.join("Extra 1.4/Desert-wallpaper/1.jpg"), "jpg");
        let out = install_archive_tree(&home, &ex, "cinnamontheme", "Extra 1.4.tar.xz");
        assert!(!out.ok);
        assert_eq!(out.message, no_package_inside_message());
        let ex = base.join("lx");
        put(&ex.join("Lubuntu Big Sur/openbox-3/themerc"), "x");
        let out = install_archive_tree(&home, &ex, "lxqttheme", "Lubuntu Big Sur.zip");
        assert!(!out.ok);
        assert!(!home.join(".themes").exists() && !home.join(".local/share/lxqt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Одна тема с обёрткой ставится, как и раньше, под своим именем; имя
    /// из магазина не подменяет имя папки темы.
    #[test]
    fn single_de_theme_keeps_folder_name() {
        let base = tmp_dir("de_single");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("Miu/xfwm4/themerc"), "x");
        put(&ex.join("Miu/metacity-1/metacity-theme-1.xml"), "<m/>");
        let out = install_archive_tree(&home, &ex, "xfwm4", "99823-Miu with xfwm.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Miu"]);
        assert!(home.join(".themes/Miu/metacity-1/metacity-theme-1.xml").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Тот же дефект безымянного корня у GTK: архив `gtk-3.0/` без обёртки.
    #[test]
    fn gtk_theme_without_own_folder_takes_archive_name() {
        let base = tmp_dir("gtk_noname");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("gtk-3.0/gtk.css"), "");
        put(&ex.join("gtk-2.0/gtkrc"), "");
        let out = install_archive_tree(&home, &ex, "gtk", "Nordic-darker.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Nordic-darker"]);
        assert!(home.join(".local/share/themes/Nordic-darker/gtk-3.0/gtk.css").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Картинка из пачки выбирается по САМОМУ БОЛЬШОМУ монитору.
    ///
    /// На стол из пачки уходит ОДНА картинка, поэтому мерка — большой
    /// монитор: маленький её ужмёт, и это лучше растянутой с меньшего.
    /// Прежний разбор `xrandr` брал «первое разрешение со звёздочкой» — на
    /// такой паре мониторов он выдал бы картинку 1600x900.
    #[test]
    fn the_pack_image_is_chosen_by_the_biggest_monitor() {
        use crate::wallpaper_exec::{largest_output, ScreenOutput};
        let out = |name: &str, res: &str| ScreenOutput {
            name: name.to_string(),
            resolution: res.to_string(),
            position: None,
        };
        let files: Vec<String> = ["1600x900.png", "1920x1080.png", "3840x2160.png"]
            .iter()
            .map(|n| format!("/сб/пачка/contents/images/{n}"))
            .collect();
        let measured = |outputs: &[ScreenOutput]| {
            let (w, h) = largest_output(outputs).and_then(|o| o.size()).unwrap_or((0, 0));
            pick_pack_image(&files, w, h).map(|f| basename(&f).to_string())
        };

        // Один выход — как было.
        assert_eq!(measured(&[out("eDP-1", "1920x1080")]).as_deref(), Some("1920x1080.png"));
        // Два разного размера, маленький первым в списке — берём большой.
        assert_eq!(
            measured(&[out("HDMI-1", "1600x900"), out("DP-1", "3840x2160")]).as_deref(),
            Some("3840x2160.png")
        );
        // ⚠️ Опрос не удался (выходов нет) — выбор идёт по имени, как до
        // всякого перечисления выходов.
        assert_eq!(measured(&[]).as_deref(), Some("1600x900.png"));
    }

    // ── корни тем в архиве и подъём вложенных (03.10.2026) ──────────────────

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("есть родитель")).expect("каталог");
        std::fs::write(path, text).expect("файл записан");
    }

    const SOUND_INDEX: &str = "[Sound Theme]\nName=Zorin\nDirectories=stereo\n\n[stereo]\nOutputProfile=stereo\n";

    /// Архив как у живого товара «zorinos2»: две темы звуков на два уровня
    /// глубже, рядом обои, xml и ReadMe.
    fn zorin_archive(ex: &Path) {
        let w = ex.join("zorinos2");
        put(&w.join("ReadMe."), "читай меня");
        put(&w.join("backgrounds/Waves.jpg"), "jpg");
        put(&w.join("gnome-background-properties/cosmos.xml"), "<xml/>");
        for t in ["Zorin", "Zorin2"] {
            put(&w.join(format!("sounds/{t}/index.theme")), SOUND_INDEX);
            put(&w.join(format!("sounds/{t}/stereo/bell.oga")), "oga");
        }
    }

    /// План + исполнение установки распакованного `ex` в категорию `kind`.
    fn install_tree(home: &Path, ex: &Path, kind: &str, own: Option<&[String]>) -> InstallOutcome {
        let cat = category_by_kind(home, kind).expect("категория есть");
        let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone()]);
        let root = find_content_root(&snap, &s(ex));
        let out = plan_install_with(home, &snap, &cat, &root, own);
        if out.ok {
            exec_install_steps(&out.steps).expect("шаги исполнены");
        }
        out
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn nested_sound_themes_land_on_top_level_without_extras() {
        let base = tmp_dir("snd_nested");
        let (home, ex) = (base.join("home"), base.join("ex"));
        zorin_archive(&ex);
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Zorin", "Zorin2"]);
        let sounds = home.join(".local/share/sounds");
        assert_eq!(listing(&sounds), ["Zorin", "Zorin2"], "ни обёртки, ни обоев в sounds/");
        assert!(sounds.join("Zorin/index.theme").is_file());
        assert!(sounds.join("Zorin2/stereo/bell.oga").is_file());
        let rec = out.record.expect("запись журнала");
        assert_eq!(rec.paths, [s(&sounds.join("Zorin")), s(&sounds.join("Zorin2"))]);
    }

    /// Товар раздела KWin из магазина (номер товара = номер раздела) ложится
    /// KPackage-ом: каталог — `KPlugin.Id`, переклассификации нет, тип журнала —
    /// тип раздела. Исходники на C++ не ставятся — отказ с причиной.
    #[test]
    fn kwin_store_items_install_as_kpackage() {
        let base = tmp_dir("kwin_store");
        let home = base.join("home");
        for (cat_id, dir, rexon_type) in [
            (720, ".local/share/kwin/scripts", "kde-kwin-script"),
            (719, ".local/share/kwin/effects", "kde-kwin-effect"),
            (721, ".local/share/kwin/tabbox", "kde-window-switcher"),
        ] {
            let ex = base.join(format!("ex{cat_id}"));
            // Без обёртки: метаданные и contents/ в корне архива (.kwinscript).
            put(&ex.join("metadata.json"), r#"{"KPlugin":{"Id":"pkg-id","Name":"Pkg"}}"#);
            put(&ex.join("contents/code/main.js"), "//");
            let cat = crate::categories::category_by_id(&home, cat_id).expect("раздел");
            let snap = install_tree_snapshot(&s(&ex), &[cat.install_dir.clone()]);
            let root = find_content_root(&snap, &s(&ex));
            let out = plan_install_with(&home, &snap, &cat, &root, Some(&[]));
            assert!(out.ok, "{cat_id}: {}", out.message);
            assert_eq!(out.names, ["pkg-id"]);
            assert!(out.reclassified.is_none());
            let rec = out.record.expect("запись");
            assert_eq!(rec.paths, [s(&home.join(dir).join("pkg-id"))]);
            assert_eq!(rec.rexon_type, rexon_type);
            assert_eq!(rec.apply_kind, cat.key);

            // Исходники эффекта на C++: метаданные есть только глубже.
            let src = base.join(format!("src{cat_id}"));
            put(&src.join("kwin-fx-main/CMakeLists.txt"), "project(x)");
            put(&src.join("kwin-fx-main/src/metadata.json"), "{}");
            let snap = install_tree_snapshot(&s(&src), &[cat.install_dir.clone()]);
            let root = find_content_root(&snap, &s(&src));
            let out = plan_install_with(&home, &snap, &cat, &root, Some(&[]));
            assert!(!out.ok, "{cat_id}: исходники не ставятся");
            assert_eq!(out.message, not_a_package_message());
            assert!(out.steps.is_empty());
        }
    }

    /// Виджеты и плагины обоев — тоже KPackage: исходники на C++, пакет
    /// pacman и не-пакет (меню Dolphin в разделе виджетов) не ставятся, а
    /// отказ говорит причину и куда смотреть дальше. Раскладки — с живых
    /// товаров магазина 04.10.2026.
    #[test]
    fn widget_and_wallpaper_plugin_need_ready_kpackage() {
        let base = tmp_dir("kpkg_ready");
        let home = base.join("home");
        let plan = |ex: &std::path::Path, cat_id: i64| {
            let cat = crate::categories::category_by_id(&home, cat_id).expect("раздел");
            let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone()]);
            let root = find_content_root(&snap, &s(ex));
            plan_install_with(&home, &snap, &cat, &root, Some(&[]))
        };

        // Исходники виджета: CMake наверху, пакеты QML — в package/ глубже.
        let src = base.join("awesome");
        put(&src.join("awesome-widgets/CMakeLists.txt"), "project(aw)");
        put(&src.join("awesome-widgets/awesomewidgets/plugin.cpp"), "//");
        put(&src.join("awesome-widgets/awesome-widget/package/metadata.json"), "{}");
        put(&src.join("awesome-widgets/awesome-widget/package/contents/ui/main.qml"), "Item {}");
        let out = plan(&src, 706);
        assert!(!out.ok);
        assert_eq!(out.message, not_a_package_message());
        assert!(out.steps.is_empty() && out.record.is_none());

        // Пакет pacman в плагинах обоев: корень содержимого — usr/.
        let pkg = base.join("pacman");
        put(&pkg.join(".PKGINFO"), "pkgname = x");
        put(&pkg.join(".MTREE"), "");
        put(&pkg.join("usr/share/plasma/wallpapers/com.example.we/metadata.json"), "{}");
        put(&pkg.join("usr/share/plasma/wallpapers/com.example.we/contents/ui/main.qml"), "Item {}");
        let out = plan(&pkg, 715);
        assert!(!out.ok);
        assert_eq!(out.message, not_a_package_message());

        // Меню Dolphin со скриптом в разделе виджетов: пакета нет вовсе.
        let menu = base.join("menu");
        put(&menu.join("Print_Selected_image/bin/print-image.sh"), "#!/bin/sh");
        put(&menu.join("Print_Selected_image/.local/share/kio/servicemenus/PrintImage.desktop"), "");
        put(&menu.join("Print_Selected_image/Readme.txt"), "");
        let out = plan(&menu, 709);
        assert!(!out.ok);
        assert_eq!(out.message, no_package_inside_message());
        assert_ne!(no_package_inside_message(), not_a_package_message());

        // Готовый плагин обоев — ставится по KPlugin.Id; CMake рядом с
        // готовым пакетом не мешает (так выложен com.t145.matrixrain).
        let ready = base.join("ready");
        put(&ready.join("rain/metadata.json"), r#"{"KPlugin":{"Id":"org.example.rain"}}"#);
        put(&ready.join("rain/contents/ui/main.qml"), "Item {}");
        put(&ready.join("rain/CMakeLists.txt"), "project(rain)");
        let out = plan(&ready, 715);
        assert!(out.ok, "{}", out.message);
        assert_eq!(
            out.record.expect("запись").paths,
            [s(&home.join(".local/share/plasma/wallpapers/org.example.rain"))]
        );
    }

    /// Виджет из подкатегории (часы — 708) — это раздел «Виджеты»: плазмоид.
    #[test]
    fn widget_from_subcategory_installs_as_plasmoid() {
        let base = tmp_dir("plasmoid_708");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("clock/metadata.json"), r#"{"KPlugin":{"Id":"org.example.clock"}}"#);
        put(&ex.join("clock/contents/ui/main.qml"), "Item {}");
        let cat = crate::categories::category_by_id(&home, 708).expect("раздел");
        assert_eq!(cat.apply_kind, "plasmoids");
        let snap = install_tree_snapshot(&s(&ex), &[cat.install_dir.clone()]);
        let root = find_content_root(&snap, &s(&ex));
        let out = plan_install_with(&home, &snap, &cat, &root, Some(&[]));
        assert!(out.ok, "{}", out.message);
        let rec = out.record.expect("запись");
        assert_eq!(
            rec.paths,
            [s(&home.join(".local/share/plasma/plasmoids/org.example.clock"))]
        );
        assert_eq!(rec.rexon_type, "theme-plasmoid");
    }

    #[test]
    fn archive_without_sound_theme_is_refused() {
        let base = tmp_dir("snd_none");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("pack/bell.wav"), "wav");
        put(&ex.join("pack/ReadMe"), "r");
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(!out.ok);
        assert_eq!(out.message, no_sound_theme_message());
        assert!(!home.join(".local/share/sounds/pack").exists());
    }

    /// Занятое чужой темой имя не затирается; своё прежнее — заменяется.
    #[test]
    fn taken_name_is_skipped_honestly_but_own_reinstall_replaces() {
        let base = tmp_dir("snd_conflict");
        let (home, ex) = (base.join("home"), base.join("ex"));
        zorin_archive(&ex);
        let foreign = home.join(".local/share/sounds/Zorin");
        put(&foreign.join("index.theme"), "[Sound Theme]\nName=Чужая\n");
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok);
        assert_eq!(out.names, ["Zorin2"]);
        assert!(out.message.contains("Zorin"), "{}", out.message);
        assert_eq!(
            std::fs::read_to_string(foreign.join("index.theme")).unwrap(),
            "[Sound Theme]\nName=Чужая\n",
            "чужая тема не тронута"
        );
        // Та же покупка ставится снова: её пути из журнала заменяемы.
        let own = vec![s(&foreign), s(&home.join(".local/share/sounds/Zorin2"))];
        let again = install_tree(&home, &ex, "sounds", Some(&own));
        assert_eq!(again.names, ["Zorin", "Zorin2"]);
        assert_eq!(std::fs::read_to_string(foreign.join("index.theme")).unwrap(), SOUND_INDEX);
    }

    #[test]
    fn duplicate_names_inside_archive_are_reported() {
        let base = tmp_dir("snd_dup");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("w/a/Zorin/index.theme"), SOUND_INDEX);
        put(&ex.join("w/b/Zorin/index.theme"), SOUND_INDEX);
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok);
        assert_eq!(out.names, ["Zorin"]);
        assert_eq!(out.message.lines().count(), 2, "{}", out.message);
    }

    /// Тема в самом корне архива ставится как раньше: имя — по каталогу.
    #[test]
    fn top_level_sound_theme_keeps_old_path() {
        let base = tmp_dir("snd_top");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("Ocean/index.theme"), SOUND_INDEX);
        let out = install_tree(&home, &ex, "sounds", None);
        assert!(out.ok);
        assert_eq!(out.names, ["Ocean"]);
        assert!(home.join(".local/share/sounds/Ocean/index.theme").is_file());
    }

    #[test]
    fn gtk_variants_icons_cursors_aurorae_colors_from_nested_archives() {
        let base = tmp_dir("roots_kinds");
        let home = base.join("home");

        let ex = base.join("gtk");
        put(&ex.join("Orchis/README.md"), "r");
        put(&ex.join("Orchis/Orchis-Dark/gtk-3.0/gtk.css"), "");
        put(&ex.join("Orchis/Orchis-Light/gtk-3.0/gtk.css"), "");
        let out = install_tree(&home, &ex, "gtk", Some(&[]));
        assert_eq!(out.names, ["Orchis-Dark", "Orchis-Light"]);
        assert_eq!(listing(&home.join(".local/share/themes")), ["Orchis-Dark", "Orchis-Light"]);

        let ex = base.join("icons");
        put(&ex.join("Pack/MyIcons/index.theme"), "[Icon Theme]\nName=My\nDirectories=apps\n");
        put(&ex.join("Pack/MyIcons/apps/a.svg"), "");
        put(&ex.join("Pack/MyCursor/cursors/left_ptr"), "");
        put(&ex.join("Pack/wallpaper.png"), "");
        let out = install_tree(&home, &ex, "icons", Some(&[]));
        assert_eq!(out.names, ["MyCursor", "MyIcons"]);
        assert_eq!(listing(&home.join(".local/share/icons")), ["MyCursor", "MyIcons"]);

        let ex = base.join("aurorae");
        put(&ex.join("Pack/Deco/decoration.svg"), "");
        put(&ex.join("Pack/Deco/Decorc"), "");
        let out = install_tree(&home, &ex, "aurorae", Some(&[]));
        assert_eq!(out.names, ["Deco"]);
        assert!(home.join(".local/share/aurorae/themes/Deco/decoration.svg").is_file());

        let ex = base.join("colors");
        put(&ex.join("Pack/colors/Nord.colors"), "[General]\n");
        put(&ex.join("Pack/README"), "");
        let out = install_tree(&home, &ex, "colors", Some(&[]));
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Nord"]);
        assert!(home.join(".local/share/color-schemes/Nord.colors").is_file());
    }

    /// Откуда у владельца взялось имя «Sounds»: архив товара «ZorinOS 6 Sound
    /// Theme» — верхняя папка `Sounds` с ReadMe и темой `Zorin` внутри.
    /// Прежняя установка клала верхнюю папку как тему и применяла её имя.
    /// Теперь темой встаёт каталог с `index.theme`, а имя обёртки темой не
    /// считается — применить его нельзя (`sound_theme_exists`).
    #[test]
    fn archive_wrapper_name_never_becomes_sound_theme_name() {
        let base = tmp_dir("snd_wrapper_name");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("Sounds/ReadMe"), "r");
        put(&ex.join("Sounds/Zorin/index.theme"), SOUND_INDEX);
        put(&ex.join("Sounds/Zorin/stereo/bell.oga"), "oga");
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Zorin"]);
        let sounds = home.join(".local/share/sounds");
        assert_eq!(listing(&sounds), ["Zorin"]);
        let dirs = vec![s(&sounds)];
        assert!(crate::qt_settings::sound_theme_exists(&dirs, "Zorin"));
        assert!(!crate::qt_settings::sound_theme_exists(&dirs, "Sounds"));
    }

    /// Живой случай 13.10.2026 на подставном HOME: в файлах GTK стоит
    /// звуковая тема «Sounds», которой нет (наверху лежит Zorin). Санитайзер
    /// файл НЕ переписывает и говорит об этом замечанием; битая тема значков
    /// в том же файле по-прежнему чинится, а звук при этом остаётся прежним.
    #[test]
    fn gtk_sanitizer_keeps_missing_sound_theme_and_reports_it() {
        let base = tmp_dir("gtk-sound-kept");
        let home = base.join("home");
        let cfg = home.join(".config");
        let data = home.join(".local/share");
        put(&data.join("sounds/Zorin/index.theme"), SOUND_INDEX);
        let ini3 = cfg.join("gtk-3.0/settings.ini");
        let ini4 = cfg.join("gtk-4.0/settings.ini");
        let text = "[Settings]\ngtk-sound-theme-name=Sounds\ngtk-enable-event-sounds=1\n";
        put(&ini3, text);
        put(&ini4, text);

        assert!(gtk_ini_sanitize_at(&s(&home), &s(&cfg), &s(&data)).is_empty());
        assert_eq!(std::fs::read_to_string(&ini3).unwrap(), text);
        assert_eq!(std::fs::read_to_string(&ini4).unwrap(), text);
        let notes = gtk_notes_at(&s(&home), &s(&cfg), &s(&data));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("gtk-3.0/settings.ini: gtk-sound-theme-name"), "{}", notes[0]);
        assert!(notes[1].contains("gtk-4.0/settings.ini: gtk-sound-theme-name"), "{}", notes[1]);

        // Существующая тема — без замечаний.
        put(&ini3, "[Settings]\ngtk-sound-theme-name=Zorin\n");
        put(&ini4, "[Settings]\ngtk-sound-theme-name=Zorin\n");
        assert!(gtk_notes_at(&s(&home), &s(&cfg), &s(&data)).is_empty());

        // Битые значки чинятся, звук рядом не трогается.
        put(&ini3, "[Settings]\ngtk-icon-theme-name=NoSuchIcons-rexon-test\ngtk-sound-theme-name=Sounds\n");
        let changes = gtk_ini_sanitize_at(&s(&home), &s(&cfg), &s(&data));
        assert_eq!(changes.len(), 1, "{changes:?}");
        let after = std::fs::read_to_string(&ini3).unwrap();
        assert!(after.contains("gtk-sound-theme-name=Sounds"), "{after}");
        assert!(!after.contains("NoSuchIcons-rexon-test"), "{after}");
    }

    /// Миграция на подставном HOME: обёртки Rexon разбираются, чужое не трогается.
    #[test]
    fn migration_lifts_nested_themes_and_rewrites_journal() {
        let base = tmp_dir("migrate");
        let home = base.join("home");
        let sounds = home.join(".local/share/sounds");
        let data = home.join(".local/share");
        let log = home.join(".config/rexon/ocs_installed.json");

        // Как у владельца 03.10: zorinos2 (две темы глубоко + обои) и Sounds/Zorin.
        let z = sounds.join("zorinos2");
        put(&z.join(crate::installed_scan::REXON_PREVIEW_NAME), "png");
        put(&z.join("backgrounds/Waves.jpg"), "jpg");
        for t in ["Zorin", "Zorin2"] {
            put(&z.join(format!("sounds/{t}/index.theme")), SOUND_INDEX);
        }
        let w = sounds.join("Sounds");
        put(&w.join("ReadMe"), "r");
        put(&w.join("Zorin/index.theme"), SOUND_INDEX);
        // Обёртку Sounds знает только журнал (маркера нет).
        // Чужая (без маркера и не в журнале) — не трогать.
        let f = sounds.join("Foreign");
        put(&f.join("Inner/index.theme"), SOUND_INDEX);
        // Правильная тема наверху — не трогать.
        put(&sounds.join("Ocean/index.theme"), SOUND_INDEX);
        put(
            &log,
            &format!(
                "{{\"1\": {{\"name\": \"a\", \"paths\": [\"{}\"]}}, \"2\": {{\"name\": \"b\", \"paths\": [\"{}\"]}}}}",
                s(&w),
                s(&z)
            ),
        );

        let lines = migrate_nested_themes_at(&s(&home), &s(&data), &s(&log));
        assert!(!lines.is_empty());
        // «Sounds» идёт раньше «zorinos2»: его Zorin занимает имя первым.
        assert_eq!(listing(&sounds), ["Foreign", "Ocean", "Zorin", "Zorin2", "zorinos2"]);
        assert!(sounds.join("Zorin2/index.theme").is_file());
        assert!(
            sounds.join("Zorin2").join(crate::installed_scan::REXON_PREVIEW_NAME).is_file(),
            "превью Rexon переехало к поднятой теме"
        );
        // Конфликт: вторая Zorin осталась в обёртке, обёртка на месте.
        assert!(z.join("sounds/Zorin/index.theme").is_file());
        assert!(f.join("Inner/index.theme").is_file(), "чужое не тронуто");
        // Остаток обёртки Sounds — в theme-leftovers, ничего не потеряно.
        let left = data.join("rexon/theme-leftovers/sounds/Sounds");
        assert!(left.join("ReadMe").is_file());
        // Журнал установленного указывает на новые места.
        let j: Value = serde_json::from_str(&std::fs::read_to_string(&log).unwrap()).unwrap();
        let p1: Vec<&str> = j["1"]["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(p1, [s(&sounds.join("Zorin")), s(&left)]);
        let p2: Vec<&str> = j["2"]["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(p2, [s(&sounds.join("Zorin2")), s(&z)]);
        // Журнал миграции записан.
        let mlog = std::fs::read_to_string(nested_migration_log_path(&s(&data))).unwrap();
        assert!(mlog.contains("конфликт"), "{mlog}");
    }

    /// Стенд «тема с иконками и курсором» — НЕ для обычного прогона: живые
    /// команды окружения (gsettings и т.п.). Запускать только под
    /// `dbus-run-session` с `GSETTINGS_BACKEND=keyfile` и временными
    /// `HOME`/`XDG_*` (в пути `HOME` обязано быть `deps-stand`), без
    /// `DISPLAY`/`WAYLAND_DISPLAY`:
    /// `REXON_DEPS_STAND_THEME=<тема> REXON_DEPS_STAND_DE=mate cargo test -p
    /// rexon-core --lib theme_deps_stand -- --ignored --nocapture`.
    /// `REXON_DEPS_STAND_INSTALL=1` — согласие на установку недостающего
    /// (команды установки на стенде должны быть подменены в `PATH`).
    #[test]
    #[ignore]
    fn theme_deps_stand() {
        let home = std::env::var("HOME").unwrap_or_default();
        let Ok(theme) = std::env::var("REXON_DEPS_STAND_THEME") else { return };
        assert!(home.contains("deps-stand"), "стенд только во временном HOME, а не в {home}");
        let de = std::env::var("REXON_DEPS_STAND_DE").unwrap_or_default();
        let mut installer = LiveInstaller::new(&de, Box::new(|line: &str| println!("журнал: {line}")));
        installer.allow_deps_install(std::env::var("REXON_DEPS_STAND_INSTALL").is_ok());
        let item = ThemeItem { category_id: 135, ..ThemeItem::default() };
        for i in installer.theme_deps_plan(&theme) {
            println!("просьба: {i:?}");
        }
        if std::env::var("REXON_DEPS_STAND_INSTALL").is_ok() {
            if let Err(why) = installer.prepare_theme_deps(&item, &theme) {
                println!("отказ: {why}");
                return;
            }
        }
        let (ok, msg) = installer.apply(&item, &theme);
        println!("итог ({ok}):\n{msg}");
    }

    /// Б9: без помощника сеанса — ровно прежняя команда установки
    /// пакетов оформления (тот же argv у обоих видов прежнего пути).
    #[test]
    fn repo_install_op_legacy_argv_is_todays() {
        use crate::privilege::op::Helpers;
        use crate::system::PkgFamily;
        let repo = vec!["conky".to_string(), "mint-y-icons".to_string()];
        for fam in [PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse] {
            for pkg in [true, false] {
                let op = repo_install_op(fam, &repo, PkgSource::ThemeSets);
                let helpers = Helpers { pkg, ..Helpers::default() };
                assert_eq!(op.pkexec_argv(&helpers), crate::system_plan::install_argv(fam, &repo, pkg), "{fam:?}/{pkg}");
            }
        }
    }
}
