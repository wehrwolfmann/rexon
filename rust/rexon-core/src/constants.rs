//! Константы системы расширений — порт `rexon/extensions/constants.py`.
//!
//! Чистые данные: типы расширений, пути установки, метки, иконки, градиенты,
//! форматы обоев, поддерживаемые форматы файлов, рекомендуемые расширения.
//! Python-множества (`set`) неупорядочены, поэтому порядок массивов здесь
//! несущественен; байтовая сверка нужна только для `RECOMMENDED_EXTENSIONS`
//! (сериализуемые данные — golden-тест `constants_parity.rs`).

use std::path::PathBuf;

use serde::Serialize;

use crate::i18n::{n_, tr};

// ── Версии ───────────────────────────────────────────────────────────

pub const REXON_VERSION: (u32, u32, u32) = (0, 1, 0);
pub const FORMAT_VERSION: u32 = 3;

// ── Типы расширений ──────────────────────────────────────────────────
//
// ⚠️ Списка типов здесь БОЛЬШЕ НЕТ. Единственный источник правды —
// матрица `de_customization::ROWS`: она одна знает и куда класть файл, и в
// каких окружениях тип имеет смысл. Раньше «безопасное оформление» жило
// вторым списком прямо тут, и две таблицы про одно и то же неизбежно
// расходились (правило #12). Новый тип заводится СТРОКОЙ В МАТРИЦЕ —
// `VALID_TYPES`, категория «Оформление» и путь установки подхватываются
// сами; здесь остаются только подпись, значок и градиент карточки, то есть
// чистое отображение (нужное и скриптовым типам, которых в матрице нет).

/// Категория «Оформление»: всё, что Rexon ставит сам, — из матрицы.
pub fn category_appearance() -> Vec<&'static str> {
    crate::de_customization::kinds()
}

/// Все допустимые типы расширений (`VALID_TYPES`) — оформление из матрицы
/// плюс скриптовые расширения.
pub fn valid_types() -> Vec<&'static str> {
    let mut v = category_appearance();
    v.extend_from_slice(CATEGORY_EXTENSIONS);
    v
}

/// Категория «Расширения» (со скриптами).
pub const CATEGORY_EXTENSIONS: &[&str] = &["desktop-tool", "plugin"];

/// Типы, требующие install.sh/uninstall.sh (запускают код).
pub const SCRIPTED_TYPES: &[&str] = &["desktop-tool", "plugin"];

/// Тип допустим?
pub fn is_valid_type(t: &str) -> bool {
    is_appearance(t) || is_extension(t)
}

/// Тип относится к категории «Оформление»?
pub fn is_appearance(t: &str) -> bool {
    crate::de_customization::row(t).is_some()
}

/// Тип относится к категории «Расширения»?
pub fn is_extension(t: &str) -> bool {
    CATEGORY_EXTENSIONS.contains(&t)
}

/// Тип запускает скрипты (install.sh/uninstall.sh)?
pub fn is_scripted(t: &str) -> bool {
    SCRIPTED_TYPES.contains(&t)
}

// ── Пути установки (безопасные типы) ─────────────────────────────────

/// Суффикс пути установки относительно `$HOME` — прямо из матрицы (её же
/// читает планировщик `ext_manager`, чтобы не держать вторую копию).
///
/// `None` — типа в матрице нет (скриптовые расширения) либо каталога
/// установки у него нет вовсе: набор обоев применяется прямо из пакета, как
/// и было в `INSTALL_PATHS` оригинала.
pub fn install_path_suffix(t: &str) -> Option<&'static str> {
    crate::de_customization::dest_suffix(t)
}

/// Путь установки для типа (раскрытый от дома пользователя), либо `None`,
/// если тип не в `INSTALL_PATHS`.
///
/// ⚠️ Починка: раньше путь собирался из голого `$HOME`, минуя `XDG_*`.
/// Питоновская сторона строит те же пути через `expand_user`, то есть
/// XDG-осведомлённо, — и при заданном `XDG_DATA_HOME` таблицы расходились
/// в 27 ключах из 30. Цена была не в красном тесте: самопроверка
/// `rexon/extensions/constants.py` сверяет КАЖДУЮ таблицу с ядром и при
/// расхождении молча откатывает ВСЕ 21 таблицу на питоновские литералы —
/// мост констант отключался целиком, а установка оформления двух десятков
/// типов уезжала мимо каталога, который читает система. Путь теперь
/// считается каноническим [`crate::utils::expand_user_xdg`], как и все
/// остальные пути пользователя.
pub fn install_path(t: &str) -> Option<PathBuf> {
    install_path_suffix(t).map(|suffix| {
        PathBuf::from(crate::utils::expand_user_xdg(
            &format!("~/{suffix}"),
            &home_dir().to_string_lossy(),
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
            std::env::var("XDG_DATA_HOME").ok().as_deref(),
        ))
    })
}

// ── Команды применения KDE-тем ───────────────────────────────────────

const KDE_APPLY_COMMANDS: &[(&str, &str)] = &[
    ("theme-kde-global", "plasma-apply-lookandfeel"),
    ("theme-kde-colors", "plasma-apply-colorscheme"),
    ("theme-kde-plasma", "plasma-apply-desktoptheme"),
    ("theme-kvantum", "kvantummanager"),
];

/// Команда применения KDE-темы для типа, либо `None`.
pub fn kde_apply_command(t: &str) -> Option<&'static str> {
    lookup(KDE_APPLY_COMMANDS, t)
}

// ── Отображение ──────────────────────────────────────────────────────
//
// Подпись, значок и градиент карточки — про ПОКАЗ, а не про механику
// установки, и нужны в том числе скриптовым типам (`desktop-tool`,
// `plugin`), которых в матрице нет и не будет. Поэтому они остаются здесь, а
// не переезжают в строку матрицы: иначе таблица типов снова разъехалась бы
// на две части. От расхождения защищает тест
// `every_matrix_kind_is_registered_everywhere`: у каждого типа матрицы
// обязаны быть все три значения.
//
// Подписи — английские `msgid` под маркером `n_` (русский — в `po/ru.po`);
// переводит их место показа: [`type_label`] сразу, а владельцы
// [`type_labels_ordered`] — через `i18n::tr`.

const TYPE_LABELS: &[(&str, &str)] = &[
    ("theme-gtk", n_("GTK Theme")),
    ("theme-icons", n_("Icons")),
    ("theme-cursors", n_("Cursors")),
    ("theme-sounds", n_("Sounds")),
    ("theme-kde-global", "KDE Global Theme"),
    ("theme-kde-colors", n_("KDE Colors")),
    ("theme-kde-plasma", "KDE Plasma Style"),
    ("theme-kvantum", "Kvantum"),
    ("theme-window-decoration", n_("Window Decorations")),
    ("theme-login-screen", n_("Login Screen")),
    ("theme-plymouth", n_("Boot Splash")),
    ("theme-conky", n_("Conky Widget")),
    ("theme-bundle", n_("Full Theme")),
    ("theme-plasmoid", n_("Plasma Widget")),
    ("wallpaper-pack", n_("Wallpapers")),
    ("wallpaper-video-adaptive", n_("Adaptive Wallpapers (v4)")),
    ("font-pack", n_("Fonts")),
    ("kde-kwin-script", n_("KWin Script")),
    ("kde-kwin-effect", n_("KWin Effect")),
    ("kde-window-switcher", n_("Window Switcher")),
    ("kde-wallpaper-plugin", n_("Plasma Wallpaper Plugin")),
    ("theme-kde-splash", n_("KDE Splash Screen")),
    ("theme-kde-lockscreen", n_("Lock Screen")),
    ("gnome-shell-extension", n_("GNOME Extension")),
    ("theme-gnome-shell", n_("GNOME Shell Theme")),
    ("cinnamon-applet", n_("Cinnamon Applet")),
    ("cinnamon-desklet", n_("Cinnamon Desklet")),
    ("cinnamon-extension", n_("Cinnamon Extension")),
    ("theme-cinnamon", n_("Cinnamon Theme")),
    ("nemo-action", n_("Nemo Action")),
    ("theme-xfwm4", n_("XFCE Windows (xfwm4)")),
    ("theme-marco", n_("MATE Windows (Marco)")),
    ("theme-lxqt", n_("LXQt Theme")),
    ("theme-openbox", n_("Openbox and labwc Windows")),
    ("desktop-tool", n_("Utility")),
    ("plugin", n_("Plugin")),
];

/// Человекочитаемая метка типа на языке интерфейса, либо `None`.
pub fn type_label(t: &str) -> Option<String> {
    lookup(TYPE_LABELS, t).map(tr)
}

/// Таблица подписей ЦЕЛИКОМ, в порядке объявления.
///
/// Порядок тут — продуктовое решение: в этом же порядке вкладка «Расширения»
/// расставляет кнопки фильтров и группы режима «список» (там это была
/// итерация по `TYPE_LABELS.keys()` словаря Python). `valid_types()` для
/// этого не годится — она собрана из матрицы кастомизации и идёт другим
/// порядком, а два разных порядка на одной витрине покупатель заметит.
///
/// ⚠️ Подписи здесь — НЕПЕРЕВЕДЁННЫЕ `msgid`: показывать их можно только
/// через `i18n::tr`.
pub fn type_labels_ordered() -> &'static [(&'static str, &'static str)] {
    TYPE_LABELS
}

const TYPE_ICONS: &[(&str, &str)] = &[
    ("theme-gtk", "🎨"),
    ("theme-icons", "📁"),
    ("theme-cursors", "🖱️"),
    ("theme-sounds", "🔔"),
    ("theme-kde-global", "💎"),
    ("theme-kde-colors", "🌈"),
    ("theme-kde-plasma", "🖥️"),
    ("theme-kvantum", "🔲"),
    ("theme-window-decoration", "🪟"),
    ("theme-login-screen", "🔐"),
    ("theme-plymouth", "⚡"),
    ("theme-conky", "📊"),
    ("theme-bundle", "🎁"),
    ("theme-plasmoid", "🧊"),
    ("wallpaper-pack", "🖼️"),
    ("wallpaper-video-adaptive", "🎞️"),
    ("font-pack", "🔤"),
    ("kde-kwin-script", "📜"),
    ("kde-kwin-effect", "✨"),
    ("kde-window-switcher", "🔀"),
    ("kde-wallpaper-plugin", "🌄"),
    ("theme-kde-splash", "🌅"),
    ("theme-kde-lockscreen", "🔒"),
    ("gnome-shell-extension", "👣"),
    ("theme-gnome-shell", "🎩"),
    ("cinnamon-applet", "🍥"),
    ("cinnamon-desklet", "📌"),
    ("cinnamon-extension", "🧬"),
    ("theme-cinnamon", "🍂"),
    ("nemo-action", "📂"),
    ("theme-xfwm4", "🐭"),
    ("theme-marco", "🍃"),
    ("theme-lxqt", "🪶"),
    ("theme-openbox", "🪟"),
    ("desktop-tool", "🔧"),
    ("plugin", "🧩"),
];

/// Emoji-иконка типа, либо `None`.
pub fn type_icon(t: &str) -> Option<&'static str> {
    lookup(TYPE_ICONS, t)
}

/// Градиент карточки типа: две RGB-тройки (0.0-1.0), либо `None`.
const TYPE_GRADIENT_COLORS: &[(&str, [[f64; 3]; 2])] = &[
    ("theme-gtk", [[0.55, 0.24, 0.78], [0.28, 0.10, 0.50]]),
    ("theme-icons", [[0.50, 0.30, 0.70], [0.25, 0.15, 0.40]]),
    ("theme-cursors", [[0.18, 0.60, 0.85], [0.08, 0.30, 0.55]]),
    ("theme-sounds", [[0.80, 0.45, 0.20], [0.50, 0.25, 0.10]]),
    ("theme-kde-global", [[0.12, 0.56, 0.80], [0.06, 0.30, 0.50]]),
    ("theme-kde-colors", [[0.40, 0.20, 0.75], [0.20, 0.10, 0.45]]),
    ("theme-kde-plasma", [[0.15, 0.50, 0.70], [0.08, 0.28, 0.42]]),
    ("theme-kvantum", [[0.35, 0.35, 0.55], [0.18, 0.18, 0.32]]),
    ("theme-window-decoration", [[0.60, 0.40, 0.15], [0.35, 0.20, 0.08]]),
    ("theme-login-screen", [[0.15, 0.15, 0.35], [0.05, 0.05, 0.18]]),
    ("theme-plymouth", [[0.08, 0.08, 0.20], [0.02, 0.02, 0.08]]),
    ("theme-conky", [[0.10, 0.45, 0.25], [0.04, 0.22, 0.12]]),
    ("theme-bundle", [[0.75, 0.55, 0.15], [0.45, 0.28, 0.05]]),
    ("theme-plasmoid", [[0.20, 0.50, 0.80], [0.10, 0.28, 0.50]]),
    ("wallpaper-pack", [[0.18, 0.60, 0.85], [0.08, 0.30, 0.55]]),
    ("wallpaper-video-adaptive", [[0.20, 0.65, 0.90], [0.05, 0.25, 0.45]]),
    ("font-pack", [[0.70, 0.70, 0.75], [0.35, 0.35, 0.42]]),
    ("kde-kwin-script", [[0.14, 0.52, 0.74], [0.07, 0.26, 0.44]]),
    ("kde-kwin-effect", [[0.30, 0.60, 0.85], [0.12, 0.30, 0.52]]),
    ("kde-window-switcher", [[0.22, 0.45, 0.72], [0.10, 0.22, 0.40]]),
    ("kde-wallpaper-plugin", [[0.16, 0.58, 0.78], [0.06, 0.28, 0.46]]),
    ("theme-kde-splash", [[0.90, 0.55, 0.25], [0.55, 0.28, 0.10]]),
    ("theme-kde-lockscreen", [[0.20, 0.22, 0.42], [0.08, 0.09, 0.22]]),
    ("gnome-shell-extension", [[0.45, 0.35, 0.72], [0.22, 0.16, 0.42]]),
    ("theme-gnome-shell", [[0.35, 0.30, 0.62], [0.16, 0.13, 0.34]]),
    ("cinnamon-applet", [[0.72, 0.42, 0.22], [0.42, 0.22, 0.10]]),
    ("cinnamon-desklet", [[0.65, 0.38, 0.28], [0.36, 0.19, 0.13]]),
    ("cinnamon-extension", [[0.58, 0.34, 0.20], [0.32, 0.17, 0.09]]),
    ("theme-cinnamon", [[0.78, 0.48, 0.20], [0.46, 0.25, 0.08]]),
    ("nemo-action", [[0.40, 0.45, 0.30], [0.20, 0.23, 0.14]]),
    ("theme-xfwm4", [[0.20, 0.55, 0.62], [0.09, 0.27, 0.32]]),
    ("theme-marco", [[0.30, 0.58, 0.35], [0.14, 0.30, 0.17]]),
    ("theme-lxqt", [[0.25, 0.42, 0.60], [0.11, 0.20, 0.32]]),
    ("theme-openbox", [[0.42, 0.45, 0.50], [0.20, 0.22, 0.26]]),
    ("desktop-tool", [[0.85, 0.55, 0.10], [0.55, 0.30, 0.05]]),
    ("plugin", [[0.20, 0.70, 0.40], [0.10, 0.40, 0.22]]),
];

/// Градиент карточки для типа, либо `None`.
pub fn type_gradient(t: &str) -> Option<[[f64; 3]; 2]> {
    TYPE_GRADIENT_COLORS
        .iter()
        .find(|(k, _)| *k == t)
        .map(|(_, v)| *v)
}

// ── Форматы обоев (wallpaper-pack) ───────────────────────────────────

pub const WALLPAPER_FORMATS: &[&str] = &[
    "static",
    "animated",
    "video",
    "web",
    "scene",
    "swf",
    "application",
];

const WALLPAPER_FORMAT_LABELS: &[(&str, &str)] = &[
    ("static", n_("Static")),
    ("animated", n_("Animated (GIF)")),
    ("video", n_("Video Wallpapers")),
    ("web", n_("Web (HTML/JS)")),
    ("scene", n_("Scene (Parallax/Particles)")),
    ("swf", "Flash (SWF)"),
    ("application", n_("Application")),
];

/// Метка формата обоев на языке интерфейса, либо `None`.
pub fn wallpaper_format_label(f: &str) -> Option<String> {
    lookup(WALLPAPER_FORMAT_LABELS, f).map(tr)
}

const WALLPAPER_FORMAT_ICONS: &[(&str, &str)] = &[
    ("static", "🏞️"),
    ("animated", "✨"),
    ("video", "🎬"),
    ("web", "🌐"),
    ("scene", "🎭"),
    ("swf", "⚡"),
    ("application", "⚙️"),
];

/// Иконка формата обоев, либо `None`.
pub fn wallpaper_format_icon(f: &str) -> Option<&'static str> {
    lookup(WALLPAPER_FORMAT_ICONS, f)
}

// ── Поддерживаемые форматы файлов (расширения в нижнем регистре) ──────

pub const SUPPORTED_IMAGE_FORMATS: &[&str] =
    &[".png", ".jpg", ".jpeg", ".webp", ".bmp", ".tiff"];
pub const SUPPORTED_ANIMATED_FORMATS: &[&str] = &[".gif", ".apng", ".webp"];
pub const SUPPORTED_VIDEO_FORMATS: &[&str] = &[
    ".mp4", ".webm", ".avi", ".m4v", ".mov", ".mkv", ".flv", ".bik", ".bk2",
    ".wmv", ".mpg", ".mpeg",
];
pub const SUPPORTED_FLASH_FORMATS: &[&str] = &[".swf"];
pub const SUPPORTED_AUDIO_FORMATS: &[&str] =
    &[".mp3", ".ogg", ".wav", ".flac", ".opus", ".m4a"];
pub const SUPPORTED_FONT_FORMATS: &[&str] = &[
    ".ttf", ".otf", ".woff", ".woff2", // современные
    ".ttc", // TrueType Collection (CJK, Noto)
    ".pfb", ".pfm", ".pfa", // PostScript Type 1 (Adobe legacy)
    ".bdf", ".pcf", ".pcf.gz", // Bitmap (Terminus, ретро-терминалы)
    ".fon", ".fnt", // Windows bitmap (Win 3.1/95/XP legacy)
];

/// Совпадает ли имя файла с одним из `SUPPORTED_FONT_FORMATS` по суффиксу
/// ЦЕЛИКОМ. Корректно матчит СОСТАВНЫЕ суффиксы (`.pcf.gz`) и односоставные
/// (`.ttf`), без ложных совпадений (`.woff2` не считается `.woff`).
///
/// Починен баг оригинала: сравнивался только последний dotted-компонент имени
/// (`os.path.splitext(f)[1]` → для `x.pcf.gz` даёт `.gz`), из-за чего запись
/// `.pcf.gz` в наборе была мёртвой и gzip-PCF шрифты (Terminus и пр.) молча не
/// устанавливались, занижая `font_count`.
pub fn is_supported_font(name: &str) -> bool {
    let lower = name.to_lowercase();
    SUPPORTED_FONT_FORMATS.iter().any(|fmt| lower.ends_with(fmt))
}

// ── Рекомендуемые расширения (каталог-заглушка) ──────────────────────

/// Одна запись каталога-заглушки `RECOMMENDED_EXTENSIONS`. Порядок полей и
/// пропуск отсутствующего `github_repo` (`skip_serializing_if`) повторяют
/// Python-dict — сериализация сверяется побайтово.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecommendedExtension {
    pub id: &'static str,
    pub name: &'static str,
    pub author: &'static str,
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub description: &'static str,
    pub tags: &'static [&'static str],
    pub download_url: &'static str,
    pub ocs_provider: &'static str,
    pub ocs_content_id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_repo: Option<&'static str>,
}

/// Рекомендуемые расширения (каталог-заглушка) — как `RECOMMENDED_EXTENSIONS`.
pub fn recommended_extensions() -> Vec<RecommendedExtension> {
    vec![
        RecommendedExtension {
            id: "dracula-gtk",
            name: "Dracula GTK",
            author: "Dracula Team",
            type_: "theme-gtk",
            description: "Тёмная тема Dracula для GTK3/GTK4",
            tags: &["dark", "purple", "gothic"],
            download_url: "",
            ocs_provider: "gnome-look",
            ocs_content_id: "1687249",
            github_repo: None,
        },
        RecommendedExtension {
            id: "catppuccin-mocha-gtk",
            name: "Catppuccin Mocha GTK",
            author: "Catppuccin",
            type_: "theme-gtk",
            description: "Пастельная тёмная тема (вариант Mocha)",
            tags: &["dark", "pastel"],
            download_url: "",
            ocs_provider: "gnome-look",
            ocs_content_id: "1996672",
            github_repo: None,
        },
        RecommendedExtension {
            id: "papirus-icons",
            name: "Papirus Icons",
            author: "PapirusDevelopmentTeam",
            type_: "theme-icons",
            description: "Популярные SVG-иконки Papirus для Linux",
            tags: &["icons", "svg", "modern"],
            download_url: "",
            ocs_provider: "gnome-look",
            ocs_content_id: "1166289",
            github_repo: None,
        },
        RecommendedExtension {
            id: "bibata-cursors",
            name: "Bibata Modern Ice",
            author: "Kaiz Khatri",
            type_: "theme-cursors",
            description: "Современные курсоры Bibata (вариант Ice)",
            tags: &["cursors", "modern"],
            download_url: "",
            ocs_provider: "gnome-look",
            ocs_content_id: "1197198",
            github_repo: None,
        },
        RecommendedExtension {
            id: "jetbrains-mono-fonts",
            name: "JetBrains Mono",
            author: "JetBrains",
            type_: "font-pack",
            description: "Моноширинный шрифт для терминала и IDE",
            tags: &["mono", "coding", "terminal"],
            download_url: "",
            ocs_provider: "pling",
            ocs_content_id: "1390418",
            github_repo: None,
        },
        RecommendedExtension {
            id: "sweet-kde-global",
            name: "Sweet KDE",
            author: "EliverLara",
            type_: "theme-kde-global",
            description: "Тёмная тема Sweet для KDE Plasma",
            tags: &["kde", "dark", "sweet"],
            download_url: "",
            ocs_provider: "kde-look",
            ocs_content_id: "1294174",
            github_repo: None,
        },
        RecommendedExtension {
            id: "catppuccin-kde-colors",
            name: "Catppuccin Mocha Colors",
            author: "Catppuccin",
            type_: "theme-kde-colors",
            description: "Цветовая схема Catppuccin Mocha для KDE",
            tags: &["kde", "dark", "pastel", "catppuccin"],
            download_url: "",
            ocs_provider: "kde-look",
            ocs_content_id: "1921998",
            github_repo: None,
        },
        RecommendedExtension {
            id: "lightly-kde-plasma",
            name: "Lightly Plasma Style",
            author: "Luwx",
            type_: "theme-kde-plasma",
            description: "Современный минималистичный Plasma style",
            tags: &["kde", "light", "modern", "minimal"],
            download_url: "",
            ocs_provider: "kde-look",
            ocs_content_id: "1879921",
            github_repo: None,
        },
        RecommendedExtension {
            id: "kvantum-catppuccin",
            name: "Catppuccin Kvantum",
            author: "Catppuccin",
            type_: "theme-kvantum",
            description: "Kvantum-тема для Qt-приложений в стиле Catppuccin",
            tags: &["kde", "kvantum", "qt", "catppuccin"],
            download_url: "",
            ocs_provider: "",
            ocs_content_id: "",
            github_repo: Some("catppuccin/kvantum"),
        },
    ]
}

// ── Внутренние хелперы ───────────────────────────────────────────────

/// Поиск значения по ключу в массиве пар (аналог dict.get → Option).
fn lookup(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Домашний каталог из `$HOME` (fallback — текущий каталог).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
