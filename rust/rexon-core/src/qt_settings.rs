//! Порт `rexon/qt_settings/window.py` — логика вкладки «Настройки».
//!
//! Вкладка большая и разнородная (звуки событий, шрифты, обои, защита
//! приложений, панель Plasma), но всё, что в ней СЧИТАЕТСЯ и РЕШАЕТСЯ, не
//! требует ни виджета, ни цикла событий: где лежат звуковые темы, какой
//! командой читается шрифт в текущем окружении, во что превращается
//! «20260620_011005» на глазах у покупателя. Ровно это здесь и живёт, а
//! Qt-сторона остаётся рисованием.
//!
//! Границы те же, что у соседних модулей ядра: команды ядро ТОЛЬКО собирает
//! (`*_argv`) и разбирает их вывод, запускает исполнитель на Python. Файловую
//! систему ядро читает само — каталоги звуковых тем обходятся здесь.
//!
//! ⚠️ Починки оригинала (правило #16 — баги при порте чиним, не консервируем),
//! каждая помечена `Починка:` по месту:
//!   1. каталог пользовательских звуковых тем строился от `~/.local/share`
//!      мимо `XDG_DATA_HOME`;
//!   2. `_patch_gtk_ini` дописывал ключ по индексу «строка после первой
//!      `[Settings]`», а при отсутствии секции в начале файла попадал в чужую
//!      секцию; теперь текст собирает общий движок
//!      [`crate::gtk_sanitizer::write_ini_setting`];
//!   3. русское склонение «файл/файла/файлов» было четвёртой копией — теперь
//!      формы по числу даёт каталог перевода ([`crate::i18n::ntr`]).
//!
//! Сверка — `rust/rexon-core/tests/qt_settings_parity.rs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::i18n::{n_, ntr, tr, trc, trf};

// ============================================================================
// Обои: формат файла по расширению
// ============================================================================

/// Расширения, которые вкладка считает статичной картинкой.
pub const STATIC_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif"];

/// Расширения, которые проигрываются видео-путём (GIF в том числе).
pub const VIDEO_EXTS: &[&str] =
    &["mp4", "webm", "mkv", "avi", "mov", "m4v", "wmv", "mpg", "mpeg", "gif"];

/// Расширения Flash-обоев (движок проигрывает их через Ruffle).
///
/// Список ведётся здесь по правилу #12 — рядом с [`STATIC_EXTS`] и
/// [`VIDEO_EXTS`], из которых собираются и разбор формата, и фильтр окна
/// выбора. Второй копии (`SUPPORTED_FLASH_FORMATS` в `constants.rs`) он не
/// противоречит: там расширения с точкой, для разбора файлов расширения.
pub const FLASH_EXTS: &[&str] = &["swf"];

/// Расширение файла в нижнем регистре, без точки.
fn ext_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// Формат обоев по расширению пути: `"static"`, `"video"` либо `"swf"`.
///
/// Неизвестное расширение — `"static"`: картинку показать безопаснее, чем
/// пытаться запустить проигрыватель на непонятном файле.
///
/// `.swf` сюда добавлен потому, что до этого формат `swf` был движку
/// известен (`WallpaperApplier.apply` разбирает `case "swf"`), а вкладке —
/// нет: разбор возвращал `"static"`, и выбранный человеком флеш-ролик
/// молча уезжал в показ статичной картинки.
pub fn detect_wallpaper_format(path: &str) -> &'static str {
    let ext = ext_of(path);
    if VIDEO_EXTS.contains(&ext.as_str()) {
        "video"
    } else if FLASH_EXTS.contains(&ext.as_str()) {
        "swf"
    } else {
        "static"
    }
}

/// Фильтр окна выбора файла по тому же списку расширений, что и разбор
/// формата: «Изображения (*.jpg *.jpeg …)».
///
/// Правило #12: списки расширений в проекте одни. Раньше окно выбора знало
/// свой набор, а `_detect_wallpaper_format` — свой, и `.tif` в фильтре не
/// было: файл, который движок принимал, покупателю просто не показывался.
pub fn wallpaper_dialog_filter(kind: &str) -> String {
    // Окно открывается на «родной» группе нажатой кнопки — что видел
    // покупатель раньше, то видит и теперь. Дальше идут «Все обои» и
    // остальные группы: без них Flash-ролик был недостижим вовсе. Кнопки
    // во вкладке две («Картинка…», «Видео…»), третьей для .swf нет, а
    // заводить её — значит требовать от человека заранее отнести свой файл
    // к одному из наших разрядов. Формат всё равно считается по расширению
    // выбранного файла ([`detect_wallpaper_format`]), поэтому переключение
    // группы в окне ничего не ломает: .swf, выбранный из-под кнопки
    // «Картинка…», применится как Flash, а не как картинка.
    // Группы сравниваются ЦЕЛЫМИ строками одного и того же перевода, так что
    // язык интерфейса на выбор «родной» группы не влияет.
    let all = || filter_group(&tr("All wallpapers"), &all_wallpaper_exts());
    let images = || filter_group(&tr("Images"), STATIC_EXTS);
    let video = || filter_group(&tr("Video"), VIDEO_EXTS);
    let flash = || filter_group("Flash", FLASH_EXTS);
    let own = match kind {
        "video" => video(),
        "swf" => flash(),
        "static" => images(),
        // `auto` — окно «выбрать обои»: все применимые файлы одним списком.
        _ => all(),
    };
    let mut out = vec![own.clone()];
    for g in [all(), images(), video(), flash()] {
        if g != own {
            out.push(g);
        }
    }
    out.join(";;")
}

/// Одна группа фильтра Qt: `Видео (*.mp4 *.webm)`.
fn filter_group(title: &str, exts: &[&str]) -> String {
    let masks: Vec<String> = exts.iter().map(|e| format!("*.{e}")).collect();
    format!("{title} ({})", masks.join(" "))
}

/// Все расширения, которые вкладка умеет применить как обои.
///
/// Порядок — картинки, видео, Flash: тот же, в каком группы идут в фильтре.
pub fn all_wallpaper_exts() -> Vec<&'static str> {
    STATIC_EXTS
        .iter()
        .chain(VIDEO_EXTS)
        .chain(FLASH_EXTS)
        .copied()
        .collect()
}

// ============================================================================
// Файловый менеджер по умолчанию
// ============================================================================

/// Значок и подпись файлового менеджера для списка выбора.
///
/// Сам НАБОР менеджеров держит [`crate::appdefaults::FM_SHORT_NAMES`] — тут
/// только как их назвать человеку. Проверка «подпись есть у каждого» — в
/// `qt_settings_parity.rs`: новый менеджер, добавленный в ядро, не появится
/// в списке безымянным.
///
/// Подпись — `msgid` (имена программ переводить нечего, «Автоматически» —
/// есть); переводит её место показа, [`fm_choices`].
pub fn fm_label(short: &str) -> (&'static str, &'static str) {
    match short {
        "auto" => ("⚙️", n_("Automatic (follows the desktop)")),
        "nemo" => ("🍃", "Nemo (Cinnamon)"),
        // Budgie своего файлового менеджера не имеет — берёт Nautilus.
        "nautilus" => ("🦶", "Nautilus (GNOME, Budgie)"),
        "thunar" => ("🐭", "Thunar (XFCE)"),
        "caja" => ("🦣", "Caja (MATE)"),
        "dolphin" => ("💎", "Dolphin (KDE)"),
        "pcmanfm" => ("📂", "PCManFM (LXDE)"),
        "pcmanfm-qt" => ("🗂️", "PCManFM-Qt (LXQt)"),
        "cosmic-files" => ("🪐", "COSMIC Files (COSMIC)"),
        "io.elementary.files" => ("🏛️", "Pantheon Files (Pantheon)"),
        "dde-file-manager" => ("🌊", "Deepin File Manager (Deepin)"),
        _ => ("📁", ""),
    }
}

/// Порядок пунктов в списке — ПОКАЗА, а не предпочтения.
///
/// Он не совпадает с [`crate::appdefaults::FM_SHORT_NAMES`] (там порядок
/// автовыбора) и сохранён ровно таким, каким его видел покупатель. Набор
/// же обязан совпадать: проверка «в списке ровно те менеджеры, что знает
/// ядро» — в `qt_settings_parity.rs`, чтобы добавленный в ядро менеджер не
/// потерялся по дороге в окно.
pub const FM_DISPLAY_ORDER: &[&str] = &[
    "nemo",
    "nautilus",
    "thunar",
    "caja",
    "dolphin",
    "pcmanfm",
    "pcmanfm-qt",
    "cosmic-files",
    "io.elementary.files",
    "dde-file-manager",
];

/// Пункты списка «Файловый менеджер»: (короткое имя, готовая подпись).
/// Первым — «Автоматически», дальше [`FM_DISPLAY_ORDER`].
pub fn fm_choices() -> Vec<(String, String)> {
    let row = |short: &str| {
        let (icon, label) = fm_label(short);
        let label = if label.is_empty() {
            short.to_string()
        } else {
            tr(label)
        };
        (short.to_string(), format!("{icon}  {label}"))
    };
    let mut out = vec![row("auto")];
    out.extend(FM_DISPLAY_ORDER.iter().map(|s| row(s)));
    out
}

// ============================================================================
// Звуковые темы: где лежат, что в них есть
// ============================================================================

/// Каталоги, которые НЕ являются звуковыми темами, хотя лежат рядом с ними.
pub const NOT_SOUND_THEMES: &[&str] = &["alsa", "speech-dispatcher", "sox"];

/// Расширения звуковых файлов темы.
pub const SOUND_EXTS: &[&str] = &["oga", "ogg", "wav"];

/// Тема, которую подставляем, когда своей нет либо прочесть не вышло.
pub const DEFAULT_SOUND_THEME: &str = "freedesktop";

/// Каталоги звуковых тем: системный и пользовательский.
///
/// Починка: пользовательский путь строился как `~/.local/share/sounds` мимо
/// `XDG_DATA_HOME` — на системах, где каталог данных переопределён, свои темы
/// в списке не появлялись. `data_home` берём у [`crate::utils::data_home`].
pub fn sound_theme_dirs(data_home: &str) -> Vec<String> {
    vec![
        "/usr/share/sounds".to_string(),
        format!("{}/sounds", data_home.trim_end_matches('/')),
    ]
}

/// Каталоги поиска звуковых тем по спецификации XDG (freedesktop
/// sound-theme): `$XDG_DATA_HOME/sounds`, затем `<каждый из
/// $XDG_DATA_DIRS>/sounds`; пустой `XDG_DATA_DIRS` — `/usr/local/share:/usr/share`.
/// Повторы убираются, порядок сохраняется (первый выигрывает, как у libcanberra).
pub fn xdg_sound_theme_dirs(data_home: &str, data_dirs: Option<&str>) -> Vec<String> {
    let dirs = match data_dirs {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => "/usr/local/share:/usr/share".to_string(),
    };
    let mut out: Vec<String> = Vec::new();
    let first = format!("{}/sounds", data_home.trim_end_matches('/'));
    for d in std::iter::once(first).chain(
        dirs.split(':')
            .filter(|d| !d.is_empty())
            .map(|d| format!("{}/sounds", d.trim_end_matches('/'))),
    ) {
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// [`xdg_sound_theme_dirs`] по окружению процесса.
pub fn live_xdg_sound_theme_dirs() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home);
    xdg_sound_theme_dirs(&data_home, std::env::var("XDG_DATA_DIRS").ok().as_deref())
}

/// Каталог — звуковая тема по спецификации: в нём `index.theme` с разделом
/// `[Sound Theme]`. Ровно так тему ищут KDE (kcm «Системные звуки») и
/// libcanberra; каталог без него для них не тема.
pub fn is_sound_theme_dir(dir: &Path) -> bool {
    fs::read_to_string(dir.join("index.theme"))
        .map(|t| t.lines().any(|l| l.trim() == "[Sound Theme]"))
        .unwrap_or(false)
}

/// Есть ли тема `name` на ВЕРХНЕМ уровне хотя бы одного из `dirs`.
/// Имя с `/` или пустое — не имя темы.
pub fn sound_theme_exists(dirs: &[String], name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return false;
    }
    dirs.iter().any(|d| is_sound_theme_dir(&Path::new(d).join(name)))
}

/// Отказ применить тему, которой нет на диске (вместо «тема применена»:
/// записанное несуществующее имя система молча подменяет `freedesktop`).
pub fn sound_theme_missing_message(name: &str) -> String {
    // TRANSLATORS: %1 is a sound theme name.
    trf(
        "Sound theme “%1” not found: there is no folder with its index.theme. Nothing was changed.",
        &[&name],
    )
}

/// Установленные звуковые темы — только настоящие темы ([`is_sound_theme_dir`]:
/// `index.theme` с разделом `[Sound Theme]`), ровно как их видят KDE и
/// libcanberra.
///
/// ⚠️ До 03.10.2026 темой считался и любой каталог со звуковыми файлами
/// внутри. В список попадали наборы GNOME, MATE и Budgie без `index.theme`
/// (`/usr/share/sounds/gnome`, `…/mate`, `…/budgie`) и обёртки архивов:
/// выбрать их можно было, а система такое имя молча подменяет на
/// `freedesktop` — «тема применена», а звучит другое.
///
/// Пустой результат заменяется на [`DEFAULT_SOUND_THEME`] — выпадающий список
/// без единого пункта покупателю показывать нечего.
pub fn list_sound_themes(dirs: &[String]) -> Vec<String> {
    let mut themes: BTreeSet<String> = BTreeSet::new();
    for base in dirs {
        let Ok(entries) = fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if NOT_SOUND_THEMES.contains(&name.as_str()) {
                continue;
            }
            if is_sound_theme_dir(&path) {
                themes.insert(name);
            }
        }
    }
    if themes.is_empty() {
        return vec![DEFAULT_SOUND_THEME.to_string()];
    }
    themes.into_iter().collect()
}

/// Обход дерева темы: зовёт `visit` на каждый звуковой файл (имя без
/// расширения, полный путь). `visit` возвращает `false`, чтобы остановиться.
fn walk_sounds(dir: &Path, visit: &mut dyn FnMut(&str, &str) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return true;
    };
    let mut subdirs = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((stem, ext)) = name.rsplit_once('.') else {
            continue;
        };
        if !SOUND_EXTS.contains(&ext.to_lowercase().as_str()) {
            continue;
        }
        files.push((stem.to_string(), path.to_string_lossy().to_string()));
    }
    // Порядок внутри каталога у `read_dir` не определён — сортируем, иначе
    // «какой файл нашёлся первым» зависело бы от файловой системы.
    files.sort();
    for (stem, path) in files {
        if !visit(&stem, &path) {
            return false;
        }
    }
    subdirs.sort();
    for sub in subdirs {
        if !walk_sounds(&sub, visit) {
            return false;
        }
    }
    true
}

/// Путь к файлу события в теме (рекурсивно), либо `None`.
///
/// Порядок поиска тот же, что был у оригинала: сначала каталог за каталогом,
/// внутри каталога — расширение за расширением ([`SOUND_EXTS`]), и только
/// потом вглубь. То есть `bell.oga` побеждает `bell.wav`, где бы тот ни лежал
/// в этой же теме.
pub fn find_sound_file(dirs: &[String], theme: &str, event_id: &str) -> Option<String> {
    // Пустое имя темы дало бы КАТАЛОГ ТЕМ ЦЕЛИКОМ, и поиск вытащил бы звук
    // произвольной чужой темы. Тема не задана — значит freedesktop.
    let theme = effective_sound_theme(theme);
    for base in dirs {
        let theme_dir = Path::new(base).join(theme);
        if !theme_dir.is_dir() {
            continue;
        }
        for want_ext in SOUND_EXTS {
            let mut found: Option<String> = None;
            walk_sounds(&theme_dir, &mut |stem, path| {
                if stem == event_id && ext_of(path) == *want_ext {
                    found = Some(path.to_string());
                    return false;
                }
                true
            });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

/// Все события темы (имена файлов без расширения), по алфавиту.
///
/// Тема не задана — берутся события `freedesktop`: кнопки «послушать»
/// обязаны играть то же, что играет система (см. [`effective_sound_theme`]),
/// а не собирать в кучу события всех установленных тем сразу.
pub fn list_theme_events(dirs: &[String], theme: &str) -> Vec<String> {
    let theme = effective_sound_theme(theme);
    let mut events: BTreeSet<String> = BTreeSet::new();
    for base in dirs {
        let theme_dir = Path::new(base).join(theme);
        if !theme_dir.is_dir() {
            continue;
        }
        walk_sounds(&theme_dir, &mut |stem, _path| {
            events.insert(stem.to_string());
            true
        });
    }
    events.into_iter().collect()
}

/// События, которые показываем кнопками «послушать» — в первую очередь
/// узнаваемые, остальные добираются по алфавиту до `limit`.
pub const SOUND_PREFERRED_EVENTS: &[&str] = &[
    "bell",
    "complete",
    "dialog-error",
    "dialog-warning",
    "dialog-information",
    "message-new-instant",
    "desktop-login",
    "desktop-logout",
    "audio-volume-change",
    "device-added",
];

/// Значок и человеческая подпись события: (id, значок, подпись).
///
/// Подпись — `msgid`; переводит её [`event_meta`].
pub const SOUND_EVENT_META: &[(&str, &str, &str)] = &[
    ("bell", "🔔", n_("Bell")),
    ("bell-window-system", "🔔", n_("System bell")),
    ("complete", "✅", n_("Done")),
    ("dialog-error", "🔴", n_("Error")),
    ("dialog-warning", "⚠️", n_("Warning")),
    ("dialog-information", "ℹ️", n_("Notification")),
    ("alarm-clock-elapsed", "⏰", n_("Alarm clock")),
    ("battery-low", "🔋", n_("Battery low")),
    ("battery-caution", "🪫", n_("Battery critical")),
    ("message-new-instant", "💬", n_("New message")),
    ("message", "💬", n_("Message")),
    ("service-login", "🔓", n_("Service login")),
    ("desktop-login", "✅", n_("Desktop login")),
    ("desktop-logout", "🔒", n_("Desktop logout")),
    ("audio-volume-change", "🔊", n_("Volume")),
    ("device-added", "🔌", n_("Device connected")),
    ("device-removed", "❌", n_("Device disconnected")),
    ("trash-empty", "🗑️", n_("Emptying the trash")),
    ("screen-capture", "📸", n_("Screenshot")),
];

/// Какие события показать кнопками: сначала узнаваемые из
/// [`SOUND_PREFERRED_EVENTS`], потом остальные — до `limit` штук.
pub fn showcase_events(events: &[String], limit: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for want in SOUND_PREFERRED_EVENTS {
        if out.len() >= limit {
            return out;
        }
        if events.iter().any(|e| e == want) {
            out.push((*want).to_string());
        }
    }
    for ev in events {
        if out.len() >= limit {
            break;
        }
        if !out.contains(ev) {
            out.push(ev.clone());
        }
    }
    out
}

/// Значок и подпись события. Незнакомое событие подписывается собственным
/// id по-человечески: `message-new-instant` → «Message new instant».
pub fn event_meta(event_id: &str) -> (String, String) {
    for (id, emoji, label) in SOUND_EVENT_META {
        if *id == event_id {
            return ((*emoji).to_string(), tr(label));
        }
    }
    let words: String = event_id.replace(['-', '_'], " ");
    let mut chars = words.chars();
    let label = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    ("▶️".to_string(), label)
}

// ============================================================================
// Звуковые темы: чем читать и писать в текущем окружении
// ============================================================================

/// Что настраиваем: саму тему или сам факт «звуки событий включены».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundSetting {
    Theme,
    EventSounds,
}

/// Куда идёт настройка в этом окружении.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundBackend {
    /// `gsettings get|set <schema> <key>`
    GSettings { schema: String, key: String },
    /// `xfconf-query -c <channel> -p <prop> [-s <value>]`
    Xfconf { channel: String, prop: String },
    /// `kreadconfig6|kwriteconfig6 --file <file> --group <group> --key <key>`
    KConfig {
        file: String,
        group: String,
        key: String,
    },
    /// Deepin ([`crate::dde`]): запись — готовая команда службы DDE, значение
    /// дописывается последним аргументом; чтение — `dde-dconfig get` ключа,
    /// в который служба кладёт то же значение.
    Dde {
        write: Vec<String>,
        read: crate::dde::DConfigKey,
    },
}

/// Команда службы DDE без последнего аргумента (значения) — заготовка
/// записи [`SoundBackend::Dde`].
fn dde_write_prefix(mut argv: Vec<String>) -> Vec<String> {
    argv.pop();
    argv
}

/// Где живёт настройка звука для окружения `de`.
///
/// Тайловые и wlroots-сеансы (Hyprland, niri, sway, «unknown») идут по общей
/// ветке `org.gnome.desktop.sound`: libcanberra-приложения читают её и без
/// GNOME. Одна общая ветка вместо списка по WM — правило #12.
pub fn sound_backend(de: &str, setting: SoundSetting) -> SoundBackend {
    let key = match setting {
        SoundSetting::Theme => "theme-name",
        SoundSetting::EventSounds => "event-sounds",
    };
    let gsettings = |schema: &str| SoundBackend::GSettings {
        schema: schema.to_string(),
        key: key.to_string(),
    };
    if de == "deepin" {
        // Тему звуков служба оформления DDE через `Set` не принимает: её
        // держит ключ DConfig `Sound_Theme`, за которым следит служба звуков
        // (dde-daemon `soundeffect1`). Общий выключатель — свойство `Enabled`
        // службы звуков: его обработчик ещё и передаёт значение проигрывателю.
        return match setting {
            SoundSetting::Theme => SoundBackend::Dde {
                write: dde_write_prefix(crate::dde::dconfig_set_argv(crate::dde::SOUND_THEME, "")),
                read: crate::dde::SOUND_THEME,
            },
            SoundSetting::EventSounds => SoundBackend::Dde {
                write: dde_write_prefix(crate::dde::event_sounds_argv(true)),
                read: crate::dde::EVENT_SOUNDS,
            },
        };
    }
    match de {
        // Pantheon: pantheon-default-settings ставит `theme-name='elementary'`
        // именно в org.gnome.desktop.sound (переопределение `:Pantheon`).
        "gnome" | "budgie" | "pantheon" | "cosmic" => gsettings("org.gnome.desktop.sound"),
        "cinnamon" | "cinnamon-mint" => gsettings("org.cinnamon.desktop.sound"),
        "mate" => gsettings("org.mate.sound"),
        "xfce" => SoundBackend::Xfconf {
            channel: "xsettings".to_string(),
            prop: match setting {
                SoundSetting::Theme => "/Net/SoundThemeName".to_string(),
                SoundSetting::EventSounds => "/Net/EnableEventSounds".to_string(),
            },
        },
        "kde" => SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: "Sounds".to_string(),
            key: match setting {
                SoundSetting::Theme => "Theme".to_string(),
                // KF6 Notifications читает `[Sounds] Enable` (bool, по
                // умолчанию true): knotifications `src/notifybyaudio.cpp:42,48`
                // `group.readEntry("Enable", true)`. Прежний
                // `EnableNotificationSounds` не читает никто — переключатель
                // в KDE не действовал вовсе.
                SoundSetting::EventSounds => "Enable".to_string(),
            },
        },
        _ => gsettings("org.gnome.desktop.sound"),
    }
}

/// argv чтения настройки. `kde_tool` — найденный `kreadconfig6` (пусто —
/// утилиты нет, читать нечем: `None`).
pub fn sound_read_argv(backend: &SoundBackend, kde_tool: &str) -> Option<Vec<String>> {
    Some(match backend {
        SoundBackend::GSettings { schema, key } => {
            vec![
                "gsettings".to_string(),
                "get".to_string(),
                schema.clone(),
                key.clone(),
            ]
        }
        SoundBackend::Xfconf { channel, prop } => vec![
            "xfconf-query".to_string(),
            "-c".to_string(),
            channel.clone(),
            "-p".to_string(),
            prop.clone(),
        ],
        SoundBackend::KConfig { file, group, key } => {
            if kde_tool.is_empty() {
                return None;
            }
            vec![
                kde_tool.to_string(),
                "--file".to_string(),
                file.clone(),
                "--group".to_string(),
                group.clone(),
                "--key".to_string(),
                key.clone(),
                "--default".to_string(),
                String::new(),
            ]
        }
        SoundBackend::Dde { read, .. } => crate::dde::dconfig_get_argv(*read),
    })
}

/// Тип свойства xfconf, которое пишет вкладка: переключатель звуков —
/// `bool`; сглаживание, хинтинг и DPI шрифтов — `int` (так их заводит сам
/// `xfce4-appearance-settings`: `-1` у них значит «не задано»); остальное
/// (тема звуков, шрифты, `/Xft/HintStyle`, `/Xft/RGBA`) — строка.
fn xfconf_value_type(prop: &str) -> &'static str {
    match prop {
        "/Net/EnableEventSounds" => "bool",
        "/Xft/Antialias" | "/Xft/Hinting" | "/Xft/DPI" => "int",
        _ => "string",
    }
}

/// argv записи настройки. `kde_tool` — найденный `kwriteconfig6`.
pub fn sound_write_argv(
    backend: &SoundBackend,
    value: &str,
    kde_tool: &str,
) -> Option<Vec<String>> {
    Some(match backend {
        SoundBackend::GSettings { schema, key } => vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.clone(),
            key.clone(),
            value.to_string(),
        ],
        // `-n -t <тип>`: на профиле, где настройку ни разу не меняли, свойства
        // в xfconf нет (умолчание xfwm4 лежит в /usr/share/xfwm4/defaults, а
        // не в канале), и голый `-s` отказывает кодом 1 — шрифт заголовков
        // в XFCE не применялся вовсе (Д6, проверка окружений 02.10.2026).
        // Тип обязателен при создании; у существующего свойства того же
        // типа он ничего не меняет.
        SoundBackend::Xfconf { channel, prop } => vec![
            "xfconf-query".to_string(),
            "-c".to_string(),
            channel.clone(),
            "-p".to_string(),
            prop.clone(),
            "-n".to_string(),
            "-t".to_string(),
            xfconf_value_type(prop).to_string(),
            "-s".to_string(),
            value.to_string(),
        ],
        SoundBackend::KConfig { file, group, key } => {
            if kde_tool.is_empty() {
                return None;
            }
            // `--notify`: запущенные программы KDE следят за kdeglobals через
            // KConfigWatcher (так KF6 Notifications узнаёт о `[Sounds]`,
            // `notifybyaudio.cpp:32-43`), а сигнал об изменении уходит только
            // при записи с уведомлением. Без него настройка доходила бы лишь
            // до вновь запущенных программ.
            vec![
                kde_tool.to_string(),
                "--file".to_string(),
                file.clone(),
                "--group".to_string(),
                group.clone(),
                "--key".to_string(),
                key.clone(),
                "--notify".to_string(),
                value.to_string(),
            ]
        }
        // Записи нет (шрифты Deepin) — писать нечем.
        SoundBackend::Dde { write, .. } if write.is_empty() => return None,
        SoundBackend::Dde { write, .. } => {
            let mut argv = write.clone();
            argv.push(value.to_string());
            argv
        }
    })
}

/// argv чтения настройки звука сразу по имени окружения — чтобы исполнителю
/// не приходилось собирать [`SoundBackend`] руками.
///
/// У KDE ключа `[Sounds] Enable` на чистом профиле нет, а KF6 Notifications
/// считает отсутствующий ключ включённым (`readEntry("Enable", true)`), —
/// поэтому и читаем с умолчанием `true`, иначе вкладка показывала бы
/// «выключено» при звучащих звуках.
pub fn sound_read_argv_for(de: &str, setting: SoundSetting, kde_tool: &str) -> Option<Vec<String>> {
    let backend = sound_backend(de, setting);
    let mut argv = sound_read_argv(&backend, kde_tool)?;
    if setting == SoundSetting::EventSounds && matches!(backend, SoundBackend::KConfig { .. }) {
        if let Some(last) = argv.last_mut() {
            *last = event_sounds_value(true).to_string();
        }
    }
    Some(argv)
}

/// argv записи настройки звука сразу по имени окружения.
pub fn sound_write_argv_for(
    de: &str,
    setting: SoundSetting,
    value: &str,
    kde_tool: &str,
) -> Option<Vec<String>> {
    sound_write_argv(&sound_backend(de, setting), value, kde_tool)
}

/// Полный план смены звуковой настройки (темы или «звуки событий вкл/выкл»):
/// чем писать и чем проверить.
///
/// Основной механизм — тот, что у окружения ([`sound_backend`]). К нему
/// добавляется общий слой, чтобы настройка дошла до программ ЧУЖОГО
/// инструментария в любом окружении:
/// * `org.gnome.desktop.sound` (`theme-name` / `event-sounds`) — его читает
///   libcanberra, то есть GTK-программы; без него в KDE звуки менялись бы
///   только у самого KDE;
/// * `kdeglobals [Sounds] Theme / Enable` — его читает KF6 Notifications, то
///   есть KDE/Qt-программы под GNOME, XFCE и прочими (пишется, только если
///   есть `kwriteconfig6`).
///
/// Дубль не добавляется — если основной механизм и есть эта ветка. В KDE при
/// записи «звуков событий» снимается мусорный `EnableNotificationSounds`,
/// который прежние версии Rexon писали вместо `Enable`.
///
/// Проверка обязательна: команда может отработать с кодом 0 и не записать
/// ничего (нет kwriteconfig6, нет схемы gsettings). Применение считается
/// удавшимся, только если ОСНОВНОЙ механизм отдал обратно записанное.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundThemePlan {
    /// Команды записи: `[0]` — основная (по ней решается успех), дальше —
    /// вспомогательные (best-effort).
    pub write: Vec<Vec<String>>,
    /// Команда чтения основного механизма (None — читать нечем).
    pub verify: Option<Vec<String>>,
}

/// Собрать [`SoundThemePlan`] для настройки `setting` со значением `value`
/// (имя темы или [`event_sounds_value`]). `kwrite`/`kread` — найденные
/// kwriteconfig6 / kreadconfig6 (пусто — утилиты нет); `has_gsettings` —
/// нашёлся ли `gsettings` (нет — общую ветку GNOME не предлагаем).
pub fn sound_plan(
    de: &str,
    setting: SoundSetting,
    value: &str,
    kwrite: &str,
    kread: &str,
    has_gsettings: bool,
) -> SoundThemePlan {
    let backend = sound_backend(de, setting);
    let mut write = Vec::new();
    if let Some(argv) = sound_write_argv(&backend, value, kwrite) {
        write.push(argv);
    }
    let gnome = sound_backend("gnome", setting);
    if has_gsettings && backend != gnome {
        if let Some(argv) = sound_write_argv(&gnome, value, "") {
            write.push(argv);
        }
    }
    let kde = sound_backend("kde", setting);
    if backend != kde {
        if let Some(argv) = sound_write_argv(&kde, value, kwrite) {
            write.push(argv);
        }
    }
    // Deepin: GTK-программам тему звуков раздаёт служба XSettings из своего
    // ключа — он отдельный от `Sound_Theme` службы оформления.
    if de == "deepin" && setting == SoundSetting::Theme {
        write.push(crate::dde::dconfig_set_argv(crate::dde::XS_SOUND_THEME, value));
    }
    if setting == SoundSetting::EventSounds && !kwrite.is_empty() {
        write.push(vec![
            kwrite.to_string(),
            "--file".to_string(),
            "kdeglobals".to_string(),
            "--group".to_string(),
            "Sounds".to_string(),
            "--key".to_string(),
            "EnableNotificationSounds".to_string(),
            "--delete".to_string(),
        ]);
    }
    SoundThemePlan {
        verify: sound_read_argv(&backend, kread),
        write,
    }
}

/// План смены звуковой темы — [`sound_plan`] для [`SoundSetting::Theme`].
pub fn sound_theme_plan(
    de: &str,
    name: &str,
    kwrite: &str,
    kread: &str,
    has_gsettings: bool,
) -> SoundThemePlan {
    sound_plan(de, SoundSetting::Theme, name, kwrite, kread, has_gsettings)
}

/// Имя темы из вывода команды: `'freedesktop'\n` → `freedesktop`.
///
/// Пустой ответ означает ровно одно — тема НЕ ЗАДАНА, и отдаётся он пустой
/// строкой. Прежде на это место подставлялось [`DEFAULT_SOUND_THEME`], и на
/// чистом профиле вкладка «Настройки» уверяла человека, что у него стоит
/// `freedesktop`, хотя не стояло ничего: строка «— не задана —»
/// (подпись `snd.theme_unset`) была недостижима НИКОГДА.
///
/// Умолчание нужно не подписи, а звуку — система без заданной темы играет
/// `freedesktop`. Этот второй путь развёден отдельно:
/// [`effective_sound_theme`], и живёт он там, где звук ИЩУТ на диске, а не
/// там, где тему показывают человеку.
pub fn parse_sound_theme(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .trim()
        .to_string()
}

/// Тема, звуки которой система возьмёт НА САМОМ ДЕЛЕ: заданная, а когда
/// своей нет — [`DEFAULT_SOUND_THEME`] (так же поступает libcanberra).
///
/// Для подписи не годится: про незаданную тему человеку надо сказать, что
/// она не задана. Годится там, где звук ищут и проигрывают.
pub fn effective_sound_theme(theme: &str) -> &str {
    let name = theme.trim();
    if name.is_empty() {
        DEFAULT_SOUND_THEME
    } else {
        name
    }
}

/// Включены ли звуки событий по ответу команды.
pub fn parse_event_sounds(raw: &str) -> bool {
    let value = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    matches!(value.to_lowercase().as_str(), "true" | "1" | "yes")
}

/// Значение «включено/выключено» в том виде, в каком его понимают все три
/// хранилища настроек.
pub fn event_sounds_value(enabled: bool) -> &'static str {
    if enabled {
        "true"
    } else {
        "false"
    }
}

/// Ответ на «в теме нет такого звука» — готовая фраза для человека.
pub fn msg_no_event_in_theme(theme: &str) -> String {
    // TRANSLATORS: %1 is the name of a sound theme.
    trf("The “%1” theme has no sound for this event", &[&theme])
}

// ============================================================================
// Системные шрифты
// ============================================================================

/// Что настраиваем — в порядке страницы «Шрифты» Plasma 6: шрифт
/// интерфейса, моноширинный, маленький, панели инструментов, меню,
/// заголовков окон.
pub const FONT_TARGETS: &[&str] = &["system", "mono", "small", "toolbar", "menu", "title"];

/// Шрифты, у которых родной ключ есть только в KDE (`kdeglobals [General]
/// smallestReadableFont`, `toolBarFont`, `menuFont`). Ни GNOME, ни XFCE,
/// Cinnamon, MATE, LXQt, COSMIC отдельных шрифтов мелкого текста, панелей
/// инструментов и меню не держат (их схемы и страницы настроек сверены
/// 04.10.2026). Там их несёт общий слой Rexon: свой блок в
/// `gtk-{3,4}.0/gtk.css` для GTK-программ и своя таблица стилей qt6ct/qt5ct
/// для Qt-программ ([`font_plan`], [`ExtraFontDialect`]).
pub const EXTRA_FONT_TARGETS: &[&str] = &["small", "toolbar", "menu"];

/// Куда пишется шрифт в этом окружении командой (gsettings / xfconf /
/// kwriteconfig6): тот же разбор, что у звуков, но со своими ключами.
///
/// Ключи сверены по первоисточникам (30.09.2026):
/// * Cinnamon — свой модуль настроек `cinnamon-settings/modules/cs_fonts.py`
///   пишет `org.cinnamon.desktop.interface font-name` (стр. 32),
///   `org.gnome.desktop.interface monospace-font-name` (стр. 41: своего
///   моноширинного ключа у Cinnamon нет — в схеме
///   `org.cinnamon.desktop.interface` его действительно нет) и
///   `org.cinnamon.desktop.wm.preferences titlebar-font` (стр. 44).
/// * MATE — `mate-control-center/capplets/appearance/appearance.h`:
///   `INTERFACE_SCHEMA "org.mate.interface"`, `GTK_FONT_KEY "font-name"`,
///   `MONOSPACE_FONT_KEY "monospace-font-name"`,
///   `MARCO_SCHEMA "org.mate.Marco.general"`,
///   `WINDOW_TITLE_FONT_KEY "titlebar-font"` (стр. 57–75).
/// * Budgie читает общие ключи GNOME: мост `labwc_bridge.py` берёт
///   `org.gnome.desktop.wm.preferences titlebar-font` (стр. 434, 1309),
///   панель — `org.gnome.desktop.interface`.
/// * Pantheon — ключи GNOME: pantheon-default-settings задаёт для `:Pantheon`
///   `org.gnome.desktop.interface font-name`/`monospace-font-name`, заголовки
///   окон без CSD рисует Gala (Mutter) по `org.gnome.desktop.wm.preferences
///   titlebar-font`.
/// * Тайловые сеансы и неопознанное окружение — тоже ключи GNOME: их читает
///   сам GTK.
///
/// COSMIC и LXQt хранят шрифт не командой, а файлом — их родное хранилище
/// описывает [`font_plan`]; здесь для них остаются ключи GNOME, которые
/// читают GTK-программы этих окружений.
pub fn font_backend(de: &str, target: &str) -> SoundBackend {
    // Deepin: только чтение — нынешний шрифт из DConfig службы оформления.
    // Записи нет: в сеансе Treeland шрифты задаёт только Центр управления, а
    // другие сеансы Deepin Rexon не поддерживает (окно выбора не открывается,
    // причина — [`crate::dde::refusal_live`]). Шрифта заголовков у Deepin нет.
    if de == "deepin" && matches!(target, "system" | "mono") {
        let read = if target == "mono" { crate::dde::FONT_MONOSPACE } else { crate::dde::FONT_STANDARD };
        return SoundBackend::Dde { write: Vec::new(), read };
    }
    match de {
        "kde" => {
            let (group, key) = match target {
                "mono" => ("General", "fixed"),
                "small" => ("General", "smallestReadableFont"),
                "toolbar" => ("General", "toolBarFont"),
                "menu" => ("General", "menuFont"),
                "title" => ("WM", "activeFont"),
                _ => ("General", "font"),
            };
            SoundBackend::KConfig {
                file: "kdeglobals".to_string(),
                group: group.to_string(),
                key: key.to_string(),
            }
        }
        "xfce" => {
            let (channel, prop) = match target {
                "mono" => ("xsettings", "/Gtk/MonospaceFontName"),
                "title" => ("xfwm4", "/general/title_font"),
                _ => ("xsettings", "/Gtk/FontName"),
            };
            SoundBackend::Xfconf {
                channel: channel.to_string(),
                prop: prop.to_string(),
            }
        }
        _ => {
            let (schema, key) = font_gsettings_key(de, target);
            SoundBackend::GSettings {
                schema: schema.to_string(),
                key: key.to_string(),
            }
        }
    }
}

/// Ключ gsettings шрифта для окружений, которые держат его в dconf.
fn font_gsettings_key(de: &str, target: &str) -> (&'static str, &'static str) {
    match (de, target) {
        ("cinnamon" | "cinnamon-mint", "mono") => ("org.gnome.desktop.interface", "monospace-font-name"),
        ("cinnamon" | "cinnamon-mint", "title") => ("org.cinnamon.desktop.wm.preferences", "titlebar-font"),
        ("cinnamon" | "cinnamon-mint", _) => ("org.cinnamon.desktop.interface", "font-name"),
        ("mate", "mono") => ("org.mate.interface", "monospace-font-name"),
        ("mate", "title") => ("org.mate.Marco.general", "titlebar-font"),
        ("mate", _) => ("org.mate.interface", "font-name"),
        (_, "mono") => ("org.gnome.desktop.interface", "monospace-font-name"),
        (_, "title") => ("org.gnome.desktop.wm.preferences", "titlebar-font"),
        _ => ("org.gnome.desktop.interface", "font-name"),
    }
}

/// Файл, в котором окружение держит шрифт (путь — от `$XDG_CONFIG_HOME`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontFile {
    /// Ключ `libcosmic` в `cosmic-config`: файл
    /// `cosmic/com.system76.CosmicTk/v1/<key>` с RON-структурой `FontConfig`
    /// (libcosmic `src/config/mod.rs`: `ID = "com.system76.CosmicTk"`,
    /// `#[version = 1]`, поля `interface_font` / `monospace_font`, у
    /// `FontConfig` — `family`, `weight`, `stretch`, `style`; пишет его
    /// `cosmic-config` через `ron::ser::to_string_pretty`).
    CosmicTk { key: String },
    /// Ключ ini-файла QSettings: строка QFont в кавычках (без кавычек
    /// QSettings прочтёт запятые как список, и `toString()` отдаст пусто).
    /// `create = false` — файл не заводить, если его нет: это чужая
    /// программа, которой человек может не пользоваться (qt6ct).
    QtIni {
        path: String,
        group: String,
        key: String,
        create: bool,
    },
    /// Наш блок шрифтов в `gtk-<version>/gtk.css` (общий слой для GTK-программ
    /// там, где у окружения нет своего ключа): `target` — `small`/`toolbar`/
    /// `menu`, см. [`extra_fonts_block`].
    GtkCss { version: String, target: String },
    /// Наша таблица стилей `<tool>/qss/rexon-fonts.qss` и её строка в
    /// `<tool>/<tool>.conf [Interface] stylesheets` (`tool` — `qt6ct` или
    /// `qt5ct`). Пишется, только если человек пользуется этой программой —
    /// её `.conf` уже есть.
    QtCtQss { tool: String, target: String },
    /// Шрифт заголовков окон в `rc.xml` Openbox или labwc: элементы
    /// `<theme><font place="ActiveWindow">` и `place="InactiveWindow"`
    /// ([`rc_xml_with_title_font`]).
    RcXml { wm: TitleWm },
    /// Шрифт заголовков sway: наш файл `sway/rexon-title-font.conf` со
    /// строкой `font pango:…`, подключённый управляемым блоком в конфиг
    /// sway. Пишет его не [`FontFile::file_writes`], а
    /// [`sway_title_font_apply`]: подключение, засев личного конфига из
    /// системного и проверка `sway --validate` — общий механизм композиторов
    /// ([`crate::animations::wm_include`]), тот же, что у акцента.
    SwayConf,
}

/// Одна запись файла по плану шрифта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFileWrite {
    /// Путь от `$XDG_CONFIG_HOME`.
    pub rel_path: String,
    pub text: String,
    /// На месте файла лежит символьная ссылка (тема GTK, подключённая
    /// ссылкой): её снять и положить обычный файл — писать СКВОЗЬ неё
    /// нельзя, это правка чужой темы.
    pub replace_link: bool,
}

/// Чем план шрифта читает файлы: текст по пути от `$XDG_CONFIG_HOME` и цель
/// символьной ссылки (абсолютный путь), если на этом месте ссылка.
pub trait FontFileIo {
    fn read(&self, rel_path: &str) -> Option<String>;
    fn link_target(&self, rel_path: &str) -> Option<String>;
    /// Тот же путь в системных каталогах настроек (`$XDG_CONFIG_DIRS`,
    /// иначе `/etc/xdg`) — образец, с которого заводится свой файл.
    fn read_system(&self, _rel_path: &str) -> Option<String> {
        None
    }
}

impl FontFile {
    /// Путь файла от `$XDG_CONFIG_HOME`.
    pub fn rel_path(&self) -> String {
        match self {
            FontFile::CosmicTk { key } => format!("cosmic/com.system76.CosmicTk/v1/{key}"),
            FontFile::QtIni { path, .. } => path.clone(),
            FontFile::GtkCss { version, .. } => format!("gtk-{version}/gtk.css"),
            FontFile::QtCtQss { tool, .. } => format!("{tool}/qss/{QTCT_QSS_NAME}"),
            FontFile::RcXml { wm } => wm.rc_xml_path().unwrap_or_default().to_string(),
            FontFile::SwayConf => format!("sway/{SWAY_FONT_STEM}.conf"),
        }
    }

    /// Можно ли завести файл, если его ещё нет.
    pub fn may_create(&self) -> bool {
        match self {
            FontFile::CosmicTk { .. } | FontFile::GtkCss { .. } | FontFile::QtCtQss { .. } | FontFile::SwayConf => true,
            FontFile::QtIni { create, .. } => *create,
            // Свой rc.xml Openbox читает ВМЕСТО системного: заведённый с
            // одним шрифтом, он оставил бы окна без привязок клавиш и мыши.
            // Нет своего — берётся копия системного ([`FontFileIo::read_system`]).
            FontFile::RcXml { wm } => *wm == TitleWm::Labwc,
        }
    }

    /// Строка шрифта из текста файла в виде, понятном [`parse_font_spec_face`]
    /// (пусто — не задан).
    pub fn read_spec(&self, text: &str) -> String {
        match self {
            FontFile::CosmicTk { .. } => cosmic_font_family(text),
            FontFile::QtIni { group, key, .. } => ini_value(text, group, key)
                .map(|v| v.trim_matches('"').to_string())
                .unwrap_or_default(),
            FontFile::GtkCss { target, .. } | FontFile::QtCtQss { target, .. } => extra_fonts_parse(text)
                .into_iter()
                .find(|f| f.target == *target)
                .map(|f| gnome_font_spec(&f.family, f.size))
                .unwrap_or_default(),
            // Начертание — словами Pango («Sans Bold 10»): строку читает
            // [`parse_font_spec_face`].
            FontFile::RcXml { .. } => rc_xml_title_font_face(text)
                .map(|(family, size, face)| font_face_spec(&family, size, face))
                .unwrap_or_default(),
            FontFile::SwayConf => sway_font_value(text)
                .map(|desc| {
                    let (family, size, face) = parse_pango_font_desc(&desc);
                    font_face_spec(&family, size, face)
                })
                .unwrap_or_default(),
        }
    }

    /// Новый текст файла с этим шрифтом. `existing = None` — файла нет.
    /// Если файл тронуть нельзя (наш блок в `gtk.css` разорван руками),
    /// возвращается прежний текст как есть.
    pub fn new_text(&self, existing: Option<&str>, family: &str, size: i32) -> String {
        self.try_new_text(existing, family, size)
            .unwrap_or_else(|| existing.unwrap_or_default().to_string())
    }

    /// Новый текст файла; `None` — файл трогать нельзя.
    pub fn try_new_text(&self, existing: Option<&str>, family: &str, size: i32) -> Option<String> {
        self.try_new_text_face(existing, family, size, FontFace::default())
    }

    /// То же с начертанием из окна выбора (жирность, наклон): его держат
    /// `FontConfig` COSMIC, запись QFont (LXQt, qt6ct), `rc.xml` Openbox и
    /// labwc (`<weight>`, `<slant>`) и строка `font` sway; блоки `gtk.css` и
    /// `.qss` — только семейство и кегль.
    pub fn try_new_text_face(&self, existing: Option<&str>, family: &str, size: i32, face: FontFace) -> Option<String> {
        match self {
            FontFile::CosmicTk { .. } => Some(cosmic_font_ron_face(family, face)),
            FontFile::QtIni { group, key, .. } => Some(crate::gtk_sanitizer::write_ini_setting_in(
                existing,
                group,
                key,
                &format!("\"{}\"", qt_font_spec_face(family, size, face)),
            )),
            FontFile::GtkCss { version, target } => {
                let dialect = if version == "4.0" { ExtraFontDialect::Gtk4 } else { ExtraFontDialect::Gtk3 };
                let text = existing.unwrap_or_default();
                let fonts = with_extra_font(extra_fonts_parse(text), target, family, size);
                text_with_extra_fonts_block(existing, &extra_fonts_block(dialect, &fonts))
            }
            FontFile::QtCtQss { target, .. } => {
                // Файл целиком наш: в нём только блок.
                let fonts = with_extra_font(extra_fonts_parse(existing.unwrap_or_default()), target, family, size);
                Some(extra_fonts_block(ExtraFontDialect::Qss, &fonts))
            }
            FontFile::RcXml { wm } => rc_xml_with_title_font_face(existing, *wm, family, size, face),
            FontFile::SwayConf => Some(sway_title_font_content(family, size, face)),
        }
    }

    /// Все записи файлов, которых требует запись шрифта в это хранилище.
    /// `None` — писать нельзя: файла нет, а заводить его нельзя (qt6ct не
    /// настроен — его `.conf` нет), либо наш блок разорван руками.
    ///
    /// * `gtk.css`-ссылка на чужую тему не правится сквозь ссылку: на её
    ///   месте встаёт обычный файл, который подключает ту же тему
    ///   (`@import url("file://…")`), и уже за ним — наш блок.
    /// * qt6ct/qt5ct: рядом с самим `.qss` его путь вписывается в
    ///   `[Interface] stylesheets` (так его подключает их же страница «Style
    ///   Sheets»); чужие таблицы из списка остаются.
    pub fn file_writes(
        &self,
        config_home: &str,
        io: &dyn FontFileIo,
        family: &str,
        size: i32,
    ) -> Option<Vec<FontFileWrite>> {
        self.file_writes_face(config_home, io, family, size, FontFace::default())
    }

    /// [`FontFile::file_writes`] с начертанием из окна выбора.
    pub fn file_writes_face(
        &self,
        config_home: &str,
        io: &dyn FontFileIo,
        family: &str,
        size: i32,
        face: FontFace,
    ) -> Option<Vec<FontFileWrite>> {
        let rel = self.rel_path();
        let mut out = Vec::new();
        match self {
            // Одного файла мало: нужен ещё блок в конфиге sway с проверкой
            // самим sway — это делает [`sway_title_font_apply`].
            FontFile::SwayConf => return None,
            FontFile::QtCtQss { tool, .. } => {
                let conf_rel = format!("{tool}/{tool}.conf");
                let conf = io.read(&conf_rel)?;
                let existing = io.read(&rel);
                out.push(FontFileWrite {
                    text: self.try_new_text_face(existing.as_deref(), family, size, face)?,
                    rel_path: rel.clone(),
                    replace_link: false,
                });
                let qss_abs = format!("{}/{rel}", config_home.trim_end_matches('/'));
                if let Some(text) = qtct_conf_with_stylesheet(&conf, &qss_abs) {
                    out.push(FontFileWrite { rel_path: conf_rel, text, replace_link: false });
                }
            }
            FontFile::GtkCss { .. } => {
                let (existing, replace_link) = match io.link_target(&rel) {
                    Some(target) => (Some(format!("@import url(\"{}\");\n", css_file_url(&target))), true),
                    None => (io.read(&rel), false),
                };
                out.push(FontFileWrite {
                    text: self.try_new_text_face(existing.as_deref(), family, size, face)?,
                    rel_path: rel,
                    replace_link,
                });
            }
            _ => {
                let existing = io.read(&rel).or_else(|| match self {
                    FontFile::RcXml { .. } => io.read_system(&rel),
                    _ => None,
                });
                if existing.is_none() && !self.may_create() {
                    return None;
                }
                out.push(FontFileWrite {
                    text: self.try_new_text_face(existing.as_deref(), family, size, face)?,
                    rel_path: rel,
                    replace_link: false,
                });
            }
        }
        Some(out)
    }
}

// ── Общий слой шрифтов «маленький / панели инструментов / меню» ───────────
//
// Родной ключ этих шрифтов есть только у KDE. Чтобы они действовали в
// любом окружении (правило «настройка одного окружения — во всех»), Rexon
// кладёт их сам туда, где их берут программы:
//
// * GTK 3 и GTK 4 — пользовательская таблица стилей `gtk-<версия>/gtk.css`:
//   GTK подключает её с приоритетом пользователя (выше темы), и свойства
//   шрифта в ней наследуются вниз по дереву узлов. Узлы и классы — по
//   документации GTK (имена CSS-узлов у `GtkMenu`/`GtkMenuItem`/`GtkMenuBar`/
//   `GtkToolbar`/`GtkStatusbar`/`GtkPopoverMenu` в GTK 3; `menubar > item`,
//   `popover.menu`, `modelbutton` и классы `.toolbar`, `.caption` в GTK 4).
//   Пишутся только `font-family` и `font-size` — не сокращение `font`, чтобы
//   не сбить толщину, которую тема задаёт, например, `.caption-heading`.
// * Qt-программы на виджетах вне KDE — таблица стилей qt6ct/qt5ct
//   (`[Interface] stylesheets`): её платформенная тема qt6ct читает при
//   запуске программы и ставит перед таблицей самой программы
//   (`Qt6CTPlatformTheme::applySettings`). Классы — те же, по которым Qt
//   раздаёт шрифты платформенной темы: `QMenu`/`QMenuBar` (MenuFont),
//   `QToolButton` (ToolButtonFont); мелкому шрифту в Qt отдельного класса
//   нет, его несут строки состояния `QStatusBar`.
// * KDE-программы, запущенные с платформенной темой Plasma и вне Plasma, —
//   `kdeglobals [General]` (его читает `KDEPlasmaPlatformTheme6`).

/// Имя нашей таблицы стилей в каталоге `qss` у qt6ct/qt5ct.
pub const QTCT_QSS_NAME: &str = "rexon-fonts.qss";

/// Программы-настройщики Qt вне KDE, у которых есть свои таблицы стилей.
pub const QTCT_TOOLS: &[&str] = &["qt6ct", "qt5ct"];

const EXTRA_BLOCK_BEGIN: &str = "/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */";
const EXTRA_BLOCK_BEGIN_PREFIX: &str = "/* >>> Rexon: fonts";
const EXTRA_BLOCK_END: &str = "/* <<< Rexon: fonts */";
const EXTRA_ENTRY_PREFIX: &str = "/* rexon:font ";

/// Для какой таблицы стилей собирается блок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraFontDialect {
    Gtk3,
    Gtk4,
    Qss,
}

/// Селекторы шрифта `target` в таблице стилей `dialect`.
pub fn extra_font_selectors(dialect: ExtraFontDialect, target: &str) -> &'static str {
    match (dialect, target) {
        (ExtraFontDialect::Gtk3, "small") => "statusbar, .caption, .caption-heading",
        (ExtraFontDialect::Gtk3, "toolbar") => "toolbar, toolbar button",
        (ExtraFontDialect::Gtk3, _) => {
            "menubar, menubar > menuitem, menu, menuitem, popover.menu, popover.menu modelbutton"
        }
        (ExtraFontDialect::Gtk4, "small") => ".caption, .caption-heading",
        (ExtraFontDialect::Gtk4, "toolbar") => ".toolbar, .toolbar button",
        (ExtraFontDialect::Gtk4, _) => "menubar, menubar > item, popover.menu, popover.menu modelbutton",
        (ExtraFontDialect::Qss, "small") => "QStatusBar, QStatusBar QLabel",
        (ExtraFontDialect::Qss, "toolbar") => "QToolButton",
        (ExtraFontDialect::Qss, _) => "QMenuBar, QMenu",
    }
}

/// Один шрифт нашего блока.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraFont {
    pub target: String,
    pub family: String,
    pub size: i32,
}

/// Строка CSS: кавычки и обратная косая черта экранируются, переводов строк
/// в имени шрифта не бывает.
fn css_string(value: &str) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).collect();
    format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `file://`-адрес файла для `@import url(...)`.
fn css_file_url(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Наш блок шрифтов: на каждый шрифт — строка-метка (по ней блок читается
/// обратно) и правило.
pub fn extra_fonts_block(dialect: ExtraFontDialect, fonts: &[ExtraFont]) -> String {
    let mut out = format!("{EXTRA_BLOCK_BEGIN}\n");
    for target in EXTRA_FONT_TARGETS {
        let Some(f) = fonts.iter().find(|f| f.target == *target) else { continue };
        let label: String = f.family.chars().filter(|c| !c.is_control()).collect::<String>().replace("*/", "* /");
        out.push_str(&format!("{EXTRA_ENTRY_PREFIX}{target} {} {label} */\n", f.size));
        out.push_str(&format!(
            "{} {{\n  font-family: {};\n  font-size: {}pt;\n}}\n",
            extra_font_selectors(dialect, target),
            css_string(&f.family),
            f.size
        ));
    }
    out.push_str(EXTRA_BLOCK_END);
    out.push('\n');
    out
}

/// Шрифты нашего блока в тексте таблицы стилей (вне блока ничего не
/// читается: чужие правила не наши).
pub fn extra_fonts_parse(text: &str) -> Vec<ExtraFont> {
    let Some(begin) = text.find(EXTRA_BLOCK_BEGIN_PREFIX) else { return Vec::new() };
    let rest = &text[begin..];
    let block = match rest.find(EXTRA_BLOCK_END) {
        Some(end) => &rest[..end],
        None => rest,
    };
    let mut out: Vec<ExtraFont> = Vec::new();
    for line in block.lines() {
        let Some(entry) = line.trim().strip_prefix(EXTRA_ENTRY_PREFIX) else { continue };
        let entry = entry.trim_end_matches("*/").trim();
        let mut parts = entry.splitn(3, ' ');
        let (Some(target), Some(size), Some(family)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (Some(target), Ok(size)) = (EXTRA_FONT_TARGETS.iter().find(|t| **t == target), size.parse::<i32>())
        else {
            continue;
        };
        out.retain(|f| f.target != *target);
        out.push(ExtraFont { target: target.to_string(), family: family.trim().to_string(), size });
    }
    out
}

fn with_extra_font(mut fonts: Vec<ExtraFont>, target: &str, family: &str, size: i32) -> Vec<ExtraFont> {
    fonts.retain(|f| f.target != target);
    fonts.push(ExtraFont { target: target.to_string(), family: family.to_string(), size });
    fonts
}

/// Текст таблицы стилей с нашим блоком: прежний наш блок снимается, новый
/// встаёт в конец (позже — значит главнее при равном весе); чужие правила и
/// `@import` остаются на своих местах. `None` — начало блока есть, а конца
/// нет (правили руками): угадывать, где кончается наше, нельзя.
pub fn text_with_extra_fonts_block(existing: Option<&str>, block: &str) -> Option<String> {
    let mut body = existing.unwrap_or_default().to_string();
    if let Some(begin) = body.find(EXTRA_BLOCK_BEGIN_PREFIX) {
        let end_rel = body[begin..].find(EXTRA_BLOCK_END)?;
        let mut end = begin + end_rel + EXTRA_BLOCK_END.len();
        if body[end..].starts_with('\n') {
            end += 1;
        }
        body.replace_range(begin..end, "");
    }
    let body = body.trim_end();
    Some(if body.is_empty() { block.to_string() } else { format!("{body}\n\n{block}") })
}

/// Строки списка qt6ct `stylesheets=` (запись QSettings: через запятую, в
/// кавычках — то, где есть запятая; пустой список — `@Invalid()`).
fn qsettings_list(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "@Invalid()" {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ',' if !quoted => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out.retain(|s| !s.is_empty());
    out
}

fn qsettings_list_value(items: &[String]) -> String {
    items
        .iter()
        .map(|s| {
            if s.contains(',') || s.contains('"') {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Текст `qt6ct.conf` с нашей таблицей в `[Interface] stylesheets`; `None`
/// — она там уже есть. Чужие таблицы из списка остаются, наша — последней
/// (позже — главнее).
pub fn qtct_conf_with_stylesheet(conf: &str, qss_path: &str) -> Option<String> {
    let mut list = qsettings_list(&ini_value(conf, "Interface", "stylesheets").unwrap_or_default());
    if list.iter().any(|p| p == qss_path) {
        return None;
    }
    list.push(qss_path.to_string());
    Some(crate::gtk_sanitizer::write_ini_setting_in(
        Some(conf),
        "Interface",
        "stylesheets",
        &qsettings_list_value(&list),
    ))
}

/// Пути таблиц стилей из `qt6ct.conf` — для проверки.
pub fn qtct_conf_stylesheets(conf: &str) -> Vec<String> {
    qsettings_list(&ini_value(conf, "Interface", "stylesheets").unwrap_or_default())
}

/// На что действуют «маленький», «панели инструментов» и «меню» в
/// окружении `de` — одна строка под кнопками шрифтов.
pub fn extra_fonts_scope_note(de: &str) -> String {
    let mut text = match de {
        "kde" => tr("Small, toolbar and menu fonts also reach GTK apps: Rexon writes them to its own block \
                     in gtk.css."),
        "lxqt" => tr("LXQt has no small, toolbar and menu fonts of its own, so Rexon applies them to GTK apps \
                      (its own block in gtk.css). LXQt's Qt apps don't take them: LXQt passes them only the \
                      interface and monospace fonts."),
        "cosmic" => tr("COSMIC has no small, toolbar and menu fonts of its own, so Rexon applies them to GTK \
                        apps (its own block in gtk.css) and to Qt apps styled by qt6ct or qt5ct. In COSMIC apps \
                        only the interface and monospace fonts change, without the size: text size there \
                        follows the interface density."),
        _ => tr("This desktop has no small, toolbar and menu fonts of its own, so Rexon applies them to GTK \
                 apps (its own block in gtk.css) and to Qt apps styled by qt6ct or qt5ct."),
    };
    match de {
        "gnome" => {
            text.push(' ');
            text.push_str(&tr("The panel and menus of GNOME Shell itself don't take them."));
        }
        "cinnamon" | "cinnamon-mint" => {
            text.push(' ');
            text.push_str(&tr("The panel and menus of Cinnamon itself don't take them."));
        }
        _ => {}
    }
    text.push(' ');
    text.push_str(&tr("Electron apps and browsers don't take them either; open apps pick the change up after a \
                       restart."));
    text
}

/// Одно хранилище шрифта: команда или файл.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontStore {
    Cmd(SoundBackend),
    File(FontFile),
}

/// Полный план смены шрифта в окружении.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontPlan {
    /// Родное хранилище окружения: из него читается текущий шрифт, по записи
    /// в него решается успех.
    pub primary: FontStore,
    /// Вспомогательные хранилища того же шрифта (best-effort): чтобы его
    /// увидели GTK- и Qt-программы, которые родной ключ окружения не читают.
    pub also: Vec<FontStore>,
    /// Готовые команды-спутники (best-effort), например «заголовки окон —
    /// не системным шрифтом»: без неё выбранный шрифт заголовка молча не
    /// действует.
    pub extra: Vec<Vec<String>>,
    /// Команды ПОСЛЕ записи (best-effort): оконному менеджеру перечитать
    /// настройки (`openbox --reconfigure`, `labwc --reconfigure`).
    pub notify: Vec<Vec<String>>,
    /// Переносить ли шрифт в `gtk-{3,4}.0/settings.ini`.
    pub gtk_ini: bool,
}

/// Окружения, чей оконный менеджер рисует заголовки шрифтом из настройки,
/// которую пишет [`font_backend`]. В остальных (COSMIC — заголовки рисуют
/// сами программы шрифтом интерфейса; Hyprland/niri — заголовков нет) шрифт
/// заголовка записать некуда, и кнопку не показываем, а не делаем вид, что
/// применили ([`title_font_note`] говорит почему). В LXQt, labwc и sway
/// заголовки рисует менеджер сеанса — [`title_wm_for`], [`font_plan_wm`].
const TITLE_FONT_DES: &[&str] =
    &["kde", "xfce", "gnome", "budgie", "pantheon", "cinnamon", "cinnamon-mint", "mate"];

/// Оконный менеджер, который рисует заголовки окон там, где у окружения
/// своего менеджера нет: в LXQt (менеджер выбирается в «Настройках сеанса»)
/// и в голых сеансах labwc и sway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleWm {
    Openbox,
    Labwc,
    Xfwm4,
    Marco,
    KWin,
    /// sway: строка `font` в нашем файле, подключённом управляемым блоком
    /// в его конфиг ([`sway_title_font_steps`]).
    Sway,
}

impl TitleWm {
    /// Имя программы (для команды `--reconfigure`).
    pub fn program(self) -> &'static str {
        match self {
            TitleWm::Openbox => "openbox",
            TitleWm::Labwc => "labwc",
            TitleWm::Xfwm4 => "xfwm4",
            TitleWm::Marco => "marco",
            TitleWm::KWin => "kwin",
            TitleWm::Sway => "sway",
        }
    }

    /// `rc.xml` от `$XDG_CONFIG_HOME` — у Openbox и labwc.
    ///
    /// * Openbox в LXQt с 0.17 читает обычный `openbox/rc.xml`:
    ///   `lxqt-session` запускает менеджер из `window_manager` без
    ///   `--config-file` (`LXQtModuleManager::startWm`), отдельный
    ///   `lxqt-rc.xml` остался в прошлом.
    /// * labwc в LXQt Wayland — `labwc/rc.xml`: `startlxqtwayland` кладёт
    ///   туда свой образец, если каталога нет, и запускает labwc с
    ///   умолчаниями (labwc-config(5)).
    pub fn rc_xml_path(self) -> Option<&'static str> {
        match self {
            TitleWm::Openbox => Some("openbox/rc.xml"),
            TitleWm::Labwc => Some("labwc/rc.xml"),
            _ => None,
        }
    }

    /// Корневой элемент `rc.xml`.
    fn rc_xml_root(self) -> &'static str {
        if self == TitleWm::Labwc { "labwc_config" } else { "openbox_config" }
    }
}

/// Кто рисует заголовки в этом сеансе — для окружений без своего
/// менеджера окон. `lxqt_session_confs` — тексты `lxqt/session.conf` по
/// старшинству (свой, затем системные из `$XDG_CONFIG_DIRS`): действует
/// первый, где задан `[General] window_manager` на X11 или `compositor` на
/// Wayland (их пишет `lxqt-config-session`; значение бывает полным путём
/// или с ключами — берётся имя программы). Не задан или незнакомый
/// менеджер — `None`: шрифт заголовка не предлагается.
pub fn title_wm_for(de: &str, lxqt_session_confs: &[&str], wayland: bool) -> Option<TitleWm> {
    match de {
        "labwc" => Some(TitleWm::Labwc),
        "sway" => Some(TitleWm::Sway),
        "lxqt" => {
            let key = if wayland { "compositor" } else { "window_manager" };
            let value = lxqt_session_confs.iter().find_map(|t| ini_value(t, "General", key))?;
            let program = value.split_whitespace().next()?.rsplit('/').next()?.to_ascii_lowercase();
            match program.as_str() {
                "openbox" if !wayland => Some(TitleWm::Openbox),
                "labwc" if wayland => Some(TitleWm::Labwc),
                "xfwm4" if !wayland => Some(TitleWm::Xfwm4),
                "marco" if !wayland => Some(TitleWm::Marco),
                "kwin_x11" | "kwin" if !wayland => Some(TitleWm::KWin),
                "kwin_wayland" if wayland => Some(TitleWm::KWin),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Почему кнопки шрифта заголовка в этом окружении нет — одна фраза к
/// пояснению под кнопками шрифтов; `None` — кнопка есть.
pub fn title_font_note(de: &str, wm: Option<TitleWm>) -> Option<String> {
    if font_target_available_wm(de, "title", wm) {
        return None;
    }
    Some(match de {
        "lxqt" => tr("The title bar font in LXQt belongs to its window manager: Rexon changes it for Openbox, \
                      labwc, xfwm4, Marco and KWin, and this session uses another one (or none is chosen in \
                      Session Settings)."),
        "cosmic" => tr("COSMIC has no separate title bar font: title bars use the interface font."),
        "hyprland" | "niri" | "river" => tr("This window manager draws no title bars of its own (only with \
                                              plugins, which Rexon doesn't configure)."),
        "deepin" => tr("Deepin has no title bar font setting."),
        _ => tr("Rexon doesn't know where this desktop keeps its title bar font."),
    })
}

// ── Шрифт заголовков sway ─────────────────────────────────────────────────
//
// sway рисует заголовки шрифтом из строки `font [pango:]<описание Pango>`
// своего конфига (sway(5)); побеждает последняя строка верхнего уровня
// (`font` внутри `bar { … }` — шрифт панели, не заголовков). Rexon кладёт
// строку в свой файл и подключает его управляемым блоком в КОНЕЦ конфига —
// тем же механизмом, что рамку цвета акцента (`crate::accent`).

/// Имя управляемого блока шрифта заголовков в конфиге sway.
pub const SWAY_FONT_TAG: &str = "title bar font";
/// Наш файл шрифта заголовков рядом с конфигом sway (без `.conf`).
pub const SWAY_FONT_STEM: &str = "rexon-title-font";
/// Шрифт заголовков sway, если в конфиге его нет (sway(5), `font`).
pub const SWAY_DEFAULT_FONT: &str = "monospace 10";

/// Имя жирности в описании Pango (ступени 100–900); обычная — без слова.
fn pango_weight_name(weight: i32) -> Option<&'static str> {
    if weight <= 0 {
        return None;
    }
    match ((weight as f64 / 100.0).round() as i32).clamp(1, 9) {
        1 => Some("Thin"),
        2 => Some("Ultra-Light"),
        3 => Some("Light"),
        5 => Some("Medium"),
        6 => Some("Semi-Bold"),
        7 => Some("Bold"),
        8 => Some("Ultra-Bold"),
        9 => Some("Heavy"),
        _ => None,
    }
}

/// Описание шрифта Pango «Семейство [жирность] [Italic] кегль».
///
/// Запятую-ограничитель списка семейств не ставим: в командах sway запятая
/// разделяет команды, и `swaymsg font …` отрезал бы кегль.
pub fn pango_font_desc(family: &str, size: i32, face: FontFace) -> String {
    let mut parts = vec![family.trim().to_string()];
    if let Some(weight) = pango_weight_name(face.weight) {
        parts.push(weight.to_string());
    }
    if face.italic {
        parts.push("Italic".to_string());
    }
    parts.push(size.max(1).to_string());
    parts.join(" ")
}

/// Строка шрифта для чтения обратно ([`parse_font_spec_face`]) и для показа
/// человеку: описание Pango «Семейство [Начертание] Кегль»
/// ([`pango_font_desc`]), а без кегля (`0` — не задан) — без числа, чтобы
/// не выдумать кегль 1.
pub fn font_face_spec(family: &str, size: i32, face: FontFace) -> String {
    if size > 0 {
        return pango_font_desc(family, size, face);
    }
    let desc = pango_font_desc(family, 1, face);
    desc.strip_suffix(" 1").unwrap_or(&desc).to_string()
}

/// Жирность по слову начертания (Pango, имя стиля Qt): ступень 100–900,
/// `Some(0)` — обычная, `None` — слово не о жирности. Слово — в нижнем
/// регистре и без дефисов.
fn face_weight_word(word: &str) -> Option<i32> {
    match word {
        "thin" | "hairline" => Some(100),
        "ultralight" | "extralight" => Some(200),
        "light" | "semilight" | "demilight" => Some(300),
        "book" | "regular" | "normal" => Some(0),
        "medium" => Some(500),
        "semibold" | "demibold" => Some(600),
        "bold" => Some(700),
        "ultrabold" | "extrabold" => Some(800),
        "heavy" | "black" | "ultrablack" | "ultraheavy" => Some(900),
        _ => None,
    }
}

/// Описание Pango (с `pango:` или без, в кавычках или без) → семейство,
/// кегль и начертание. Слова начертания в конце семейства снимаются; из
/// списка семейств берётся первое. Кегля нет — 10.
pub fn parse_pango_font_desc(desc: &str) -> (String, i32, FontFace) {
    let desc = desc.trim().trim_matches(['"', '\'']).trim();
    let desc = desc.strip_prefix("pango:").unwrap_or(desc);
    let mut words: Vec<&str> = desc.split_whitespace().collect();
    let mut size = 10;
    if let Some(last) = words.last() {
        if let Ok(v) = last.trim_end_matches("px").parse::<f64>() {
            size = (v.round() as i32).max(1);
            words.pop();
        }
    }
    let mut face = FontFace::default();
    while words.len() > 1 {
        let word = words[words.len() - 1].to_ascii_lowercase().replace('-', "");
        if let Some(w) = face_weight_word(&word) {
            if face.weight == 0 {
                face.weight = w;
            }
        } else if matches!(word.as_str(), "italic" | "oblique") {
            face.italic = true;
        } else if !matches!(
            word.as_str(),
            "roman" | "smallcaps" | "ultracondensed" | "extracondensed" | "condensed" | "semicondensed"
                | "semiexpanded" | "expanded" | "extraexpanded" | "ultraexpanded"
        ) {
            break;
        }
        words.pop();
    }
    let joined = words.join(" ");
    let family = joined.split(',').map(str::trim).find(|f| !f.is_empty()).unwrap_or("").to_string();
    (family, size, face)
}

/// Последнее значение `font` верхнего уровня в тексте конфига sway (без
/// `pango:` не трогаем — его снимает [`parse_pango_font_desc`]). Строки
/// внутри блоков `{ … }` (`bar`, `mode`, `output`) и комментарии
/// пропускаются.
pub fn sway_font_value(text: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if depth == 0 {
            let mut words = line.split_whitespace();
            if words.next() == Some("font") {
                let rest: Vec<&str> = words.filter(|w| !w.starts_with("--")).collect();
                let value = rest.join(" ").trim_matches(['"', '\'']).trim().to_string();
                if !value.is_empty() {
                    found = Some(value);
                }
            }
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        depth = depth.max(0);
    }
    found
}

/// Наш файл шрифта заголовков sway.
pub fn sway_title_font_content(family: &str, size: i32, face: FontFace) -> String {
    format!(
        "# Rexon: title bar font. This file is rewritten by Rexon; change the title bar font in Rexon instead.\n\
         font pango:{}\n",
        pango_font_desc(family, size, face)
    )
}

/// Шаги записи шрифта заголовков sway: наш файл и блок с `include` в
/// конфиге ([`crate::animations::wm_include`]), затем `swaymsg font …`.
///
/// Не `swaymsg reload`: перечитав конфиг, sway перезапускает свой swaybg
/// поверх обоев Rexon (так же рассуждает акцент). `font` — команда и
/// конфига, и живого sway (sway(5)); не запущен — уведомлять некого, это
/// не ошибка.
pub fn sway_title_font_steps(
    ctx: &crate::animations::LookCtx,
    family: &str,
    size: i32,
    face: FontFace,
) -> Vec<crate::animations::LookStep> {
    use crate::animations::{argv, wm_include, LookStep};
    let content = sway_title_font_content(family, size, face);
    let desc = pango_font_desc(family, size, face);
    vec![
        LookStep::Wm(wm_include(ctx, "sway", SWAY_FONT_TAG, SWAY_FONT_STEM, &move |_| content.clone())),
        LookStep::Notify(vec![argv(&["swaymsg", &format!("font pango:{desc}")])]),
    ]
}

/// Записать шрифт заголовков sway; `Err` — причина для человека.
pub fn sway_title_font_apply(
    io: &dyn crate::animations::LookIo,
    ctx: &crate::animations::LookCtx,
    family: &str,
    size: i32,
    face: FontFace,
) -> Result<(), String> {
    crate::animations::run_steps(io, ctx, &sway_title_font_steps(ctx, family, size, face))
}

/// Шрифт заголовков, которым sway рисует сейчас, описанием Pango: конфиг
/// ищется в порядке самого sway (sway(5): `~/.sway/config`,
/// `$XDG_CONFIG_HOME/sway/config`, затем конфиги i3 и системный). Если в
/// нём стоит наш блок — действует наш файл (блок в конце), иначе последняя
/// строка `font`; нет ни того, ни другого — [`SWAY_DEFAULT_FONT`].
pub fn sway_title_font_current(io: &dyn crate::animations::LookIo, ctx: &crate::animations::LookCtx) -> String {
    let candidates = [
        format!("{}/.sway/config", ctx.home),
        ctx.cfg("sway/config"),
        format!("{}/.i3/config", ctx.home),
        ctx.cfg("i3/config"),
        "/etc/sway/config".to_string(),
        "/etc/i3/config".to_string(),
    ];
    let Some((path, text)) = candidates.iter().find_map(|p| io.read(p).map(|t| (p.clone(), t))) else {
        return SWAY_DEFAULT_FONT.to_string();
    };
    let targets = crate::animations::wm_targets(ctx, "sway", SWAY_FONT_STEM, &|_| String::new());
    let ours = targets
        .iter()
        .find(|t| t.config == path && text.lines().any(|l| l.trim() == t.include_line))
        .and_then(|t| io.read(&t.own_path))
        .and_then(|own| sway_font_value(&own));
    ours.or_else(|| sway_font_value(&text)).unwrap_or_else(|| SWAY_DEFAULT_FONT.to_string())
}

/// [`sway_title_font_current`] строкой «Семейство [Начертание] кегль» (как
/// у GNOME — её понимает [`parse_font_spec_face`]).
pub fn sway_title_font_spec(io: &dyn crate::animations::LookIo, ctx: &crate::animations::LookCtx) -> String {
    let (family, size, face) = parse_pango_font_desc(&sway_title_font_current(io, ctx));
    font_face_spec(&family, size, face)
}

/// [`sway_title_font_spec`] на живой машине.
pub fn sway_title_font_spec_live() -> String {
    sway_title_font_spec(&crate::animations::SystemIo, &crate::animations::LookCtx::from_env())
}

/// [`sway_title_font_apply`] на живой машине → получилось ли.
pub fn sway_title_font_apply_live(family: &str, size: i32, face: FontFace) -> bool {
    sway_title_font_apply(&crate::animations::SystemIo, &crate::animations::LookCtx::from_env(), family, size, face)
        .is_ok()
}

/// Поиск тегов не должен находить их в комментариях `<!-- … -->`: те же
/// байты, но содержимое комментариев (с рамками) — пробелы.
fn xml_mask_comments(text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    let mut from = 0;
    while let Some(start) = text[from..].find("<!--").map(|i| i + from) {
        let end = text[start + 4..].find("-->").map(|i| start + 4 + i + 3).unwrap_or(text.len());
        bytes[start..end].fill(b' ');
        from = end;
    }
    // Заменялись только целые комментарии, их рамки — ASCII: UTF-8 цел.
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_string())
}

/// Начало тега `<name` (за именем — пробел, `>` или `/`) в `masked[from..to]`.
fn xml_find_tag(masked: &str, from: usize, to: usize, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let mut at = from;
    while let Some(i) = masked.get(at..to)?.find(&open) {
        let start = at + i;
        match masked.as_bytes().get(start + open.len()) {
            Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') => return Some(start),
            _ => at = start + open.len(),
        }
    }
    None
}

/// Значение атрибута `name` в открывающем теге `tag` (`<font place="…">`).
fn xml_attr(tag: &str, name: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    while let Some(i) = tag[at..].find(name) {
        let start = at + i;
        let before_ok = start > 0 && tag.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = tag[start + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let after_eq = rest[1..].trim_start();
            let quote = after_eq.chars().next()?;
            if quote == '"' || quote == '\'' {
                let value_start = tag.len() - after_eq.len() + 1;
                let value_end = value_start + tag[value_start..].find(quote)?;
                return Some((value_start, value_end));
            }
        }
        at = start + name.len();
    }
    None
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn xml_escape_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Элемент `<font>` в `<theme>`: границы элемента, открывающего тега и
/// значение `place` (пусто — без `place`, у labwc это «все места»).
struct RcXmlFont {
    start: usize,
    tag_end: usize,
    end: usize,
    self_closing: bool,
    place: String,
}

/// Границы `<theme>…</theme>` (после открывающего тега и начало
/// закрывающего) и все `<font>` в нём.
fn rc_xml_theme_fonts(text: &str) -> Option<((usize, usize), Vec<RcXmlFont>)> {
    let masked = xml_mask_comments(text);
    let theme = xml_find_tag(&masked, 0, masked.len(), "theme")?;
    let open_end = theme + masked[theme..].find('>')? + 1;
    if masked.as_bytes()[open_end - 2] == b'/' {
        return None;
    }
    let close = open_end + masked[open_end..].find("</theme>")?;
    let mut fonts = Vec::new();
    let mut at = open_end;
    while let Some(start) = xml_find_tag(&masked, at, close, "font") {
        let tag_end = start + masked[start..].find('>')? + 1;
        let self_closing = masked.as_bytes()[tag_end - 2] == b'/';
        let end = if self_closing { tag_end } else { tag_end + masked[tag_end..close].find("</font>")? + "</font>".len() };
        let tag = &text[start..tag_end];
        let place = xml_attr(tag, "place").map(|(a, b)| tag[a..b].trim().to_string()).unwrap_or_default();
        fonts.push(RcXmlFont { start, tag_end, end, self_closing, place });
        at = end;
    }
    Some(((open_end, close), fonts))
}

/// Значение поля `field` элемента `<font>`: дочерний `<field>…</field>`
/// (Openbox, labwc) или атрибут (labwc: атрибуты и элементы у него
/// равнозначны, labwc-config(5)).
fn rc_xml_font_field(text: &str, font: &RcXmlFont, field: &str) -> Option<String> {
    let tag = &text[font.start..font.tag_end];
    if let Some((a, b)) = xml_attr(tag, field) {
        return Some(xml_unescape(tag[a..b].trim()));
    }
    if font.self_closing {
        return None;
    }
    let masked = xml_mask_comments(text);
    let open = xml_find_tag(&masked, font.tag_end, font.end, field)?;
    let value_start = open + masked[open..].find('>')? + 1;
    let value_end = value_start + masked[value_start..font.end].find(&format!("</{field}>"))?;
    Some(xml_unescape(text[value_start..value_end].trim()))
}

/// Шрифт заголовка активного окна из `rc.xml`: `place="ActiveWindow"`,
/// иначе `<font>` без `place` (у labwc — для всех мест). Кегль `0` — не
/// задан.
pub fn rc_xml_title_font(text: &str) -> Option<(String, i32)> {
    rc_xml_title_font_face(text).map(|(family, size, _)| (family, size))
}

/// [`rc_xml_title_font`] с начертанием: `<weight>` (Openbox — `Bold` или
/// `Normal`, labwc — слова Pango: `semibold`, `bold`, …) и `<slant>`
/// (`italic`, `oblique`). Обычная жирность и неизвестное слово — `0`, как у
/// [`parse_pango_font_desc`].
pub fn rc_xml_title_font_face(text: &str) -> Option<(String, i32, FontFace)> {
    let (_, fonts) = rc_xml_theme_fonts(text)?;
    let font = fonts
        .iter()
        .find(|f| f.place.eq_ignore_ascii_case("ActiveWindow"))
        .or_else(|| fonts.iter().find(|f| f.place.is_empty()))?;
    let family = rc_xml_font_field(text, font, "name")?;
    if family.is_empty() {
        return None;
    }
    let size = rc_xml_font_field(text, font, "size").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let word = |field| rc_xml_font_field(text, font, field).unwrap_or_default().to_ascii_lowercase().replace('-', "");
    let face = FontFace {
        weight: face_weight_word(&word("weight")).unwrap_or(0),
        italic: matches!(word("slant").as_str(), "italic" | "oblique"),
    };
    Some((family, size, face))
}

/// Слова `<weight>` и `<slant>` для `rc.xml`. Openbox различает только
/// `Bold` (остальное для него обычная жирность, `parse_theme` в его
/// `config.c`), поэтому полужирный и жирнее у него — `Bold`, тоньше —
/// `Normal`. labwc понимает ступени Pango (labwc-config(5): normal, thin,
/// ultralight, light, semilight, book, medium, semibold, bold, ultrabold,
/// heavy, ultraheavy).
fn rc_xml_face_words(wm: TitleWm, face: FontFace) -> (&'static str, &'static str) {
    if wm == TitleWm::Labwc {
        let weight = match ((face.weight.max(1) as f64 / 100.0).round() as i32).clamp(1, 9) {
            1 => "thin",
            2 => "ultralight",
            3 => "light",
            5 => "medium",
            6 => "semibold",
            7 => "bold",
            8 => "ultrabold",
            9 => "heavy",
            _ => "normal",
        };
        let weight = if face.weight <= 0 { "normal" } else { weight };
        (weight, if face.italic { "italic" } else { "normal" })
    } else {
        (if face.weight >= 600 { "Bold" } else { "Normal" }, if face.italic { "Italic" } else { "Normal" })
    }
}

/// Места заголовков, которые получают выбранный шрифт.
const RC_XML_TITLE_PLACES: [&str; 2] = ["ActiveWindow", "InactiveWindow"];

/// Новый `rc.xml` со шрифтом заголовков `family`/`size` у активного и
/// неактивного окна. Остальные поля элемента (`weight`, `slant`) и всё
/// прочее в файле остаются как были; нет элемента — он дописывается в
/// конец `<theme>`, нет `<theme>` — она дописывается в корень.
///
/// `existing = None` (своего файла нет и системного образца тоже): у labwc
/// заводится файл с одним `<theme>` (остальное labwc берёт из своих
/// умолчаний), у Openbox — `None`: его `rc.xml` заменяет системный целиком,
/// и окна остались бы без привязок. `None` и тогда, когда файл не
/// разбирается (нет корня) — чужое не портим.
pub fn rc_xml_with_title_font(existing: Option<&str>, wm: TitleWm, family: &str, size: i32) -> Option<String> {
    rc_xml_with_title_font_face(existing, wm, family, size, FontFace::default())
}

/// [`rc_xml_with_title_font`] с начертанием из окна выбора: оно уходит в
/// `<weight>` и `<slant>` обоих мест ([`rc_xml_face_words`]). Начертание не
/// выбрано ([`FontFace::default`]) — эти поля остаются как были.
pub fn rc_xml_with_title_font_face(
    existing: Option<&str>,
    wm: TitleWm,
    family: &str,
    size: i32,
    face: FontFace,
) -> Option<String> {
    let root = wm.rc_xml_root();
    let mut text = match existing {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ if wm == TitleWm::Labwc => format!("<?xml version=\"1.0\"?>\n<{root}>\n</{root}>\n"),
        _ => return None,
    };
    xml_find_tag(&xml_mask_comments(&text), 0, text.len(), root)?;
    let family_xml = xml_escape_text(family);
    let mut fields = vec![("name", family_xml.clone()), ("size", size.to_string())];
    if face != FontFace::default() {
        let (weight, slant) = rc_xml_face_words(wm, face);
        fields.push(("weight", weight.to_string()));
        fields.push(("slant", slant.to_string()));
    }
    if rc_xml_theme_fonts(&text).is_none() {
        let masked = xml_mask_comments(&text);
        let close = masked.rfind(&format!("</{root}>"))?;
        text.insert_str(close, "  <theme>\n  </theme>\n");
    }
    for place in RC_XML_TITLE_PLACES {
        let ((_, close), fonts) = rc_xml_theme_fonts(&text)?;
        match fonts.iter().find(|f| f.place.eq_ignore_ascii_case(place)) {
            Some(font) => {
                let mut element = text[font.start..font.end].to_string();
                for (field, value) in &fields {
                    element = rc_xml_set_font_field(&element, font.self_closing, field, value)?;
                }
                text.replace_range(font.start..font.end, &element);
            }
            None => {
                // Перед `</theme>`, на шаг глубже его отступа.
                let line_start = text[..close].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let lead = &text[line_start..close];
                let (at, indent, prefix) =
                    if lead.trim().is_empty() { (line_start, lead.to_string(), "") } else { (close, String::new(), "\n") };
                let i = format!("{indent}  ");
                let body: String = fields.iter().map(|(f, v)| format!("{i}  <{f}>{v}</{f}>\n")).collect();
                let block = format!("{prefix}{i}<font place=\"{place}\">\n{body}{i}</font>\n");
                text.insert_str(at, &block);
            }
        }
    }
    Some(text)
}

/// Поле `field` элемента `<font>` = `value` (уже экранированное): атрибут,
/// если поле задано атрибутом или элемент самозакрытый, иначе дочерний
/// элемент (заменить значение или дописать перед `</font>`).
fn rc_xml_set_font_field(element: &str, self_closing: bool, field: &str, value: &str) -> Option<String> {
    let tag_end = element.find('>')? + 1;
    let tag = &element[..tag_end];
    if let Some((a, b)) = xml_attr(tag, field) {
        return Some(format!("{}{value}{}", &element[..a], &element[b..]));
    }
    if self_closing {
        let slash = tag.rfind('/')?;
        return Some(format!("{} {field}=\"{value}\" {}", element[..slash].trim_end(), &element[slash..]));
    }
    let masked = xml_mask_comments(element);
    if let Some(open) = xml_find_tag(&masked, tag_end, masked.len(), field) {
        let value_start = open + masked[open..].find('>')? + 1;
        if masked.as_bytes()[value_start - 2] != b'/' {
            let value_end = value_start + masked[value_start..].find(&format!("</{field}>"))?;
            return Some(format!("{}{value}{}", &element[..value_start], &element[value_end..]));
        }
        let open_end = value_start;
        return Some(format!("{}<{field}>{value}</{field}>{}", &element[..open], &element[open_end..]));
    }
    let close = masked.rfind("</font>")?;
    let line_start = element[..close].rfind('\n').map(|i| i + 1).unwrap_or(close);
    if element[line_start..close].trim().is_empty() {
        let indent = &element[line_start..close];
        Some(format!("{}{indent}  <{field}>{value}</{field}>\n{}", &element[..line_start], &element[line_start..]))
    } else {
        Some(format!("{}<{field}>{value}</{field}>{}", &element[..close], &element[close..]))
    }
}

// ── Узлы `<theme>` в rc.xml: тема рамок и раскладка кнопок ─────────────
//
// Тем же разбором, что шрифт заголовка ([`rc_xml_with_title_font`]): файл
// не пересобирается, меняется только текст нужного узла, комментарии и
// чужие узлы остаются байт в байт. Узлы ищутся среди ПРЯМЫХ потомков:
// `<name>` темы — не то же, что `<name>` внутри `<font>` той же `<theme>`.

/// Прямой потомок элемента: границы элемента, конец открывающего тега и
/// начало закрывающего (у самозакрытого оба — его конец).
struct XmlChild {
    name: String,
    start: usize,
    open_end: usize,
    close: usize,
    end: usize,
    self_closing: bool,
}

/// Прямые потомки в `masked[from..to]` (`from` — сразу за открывающим
/// тегом родителя, `to` — начало его закрывающего). `None` — теги не
/// сходятся (файл битый — чужое не трогаем).
fn xml_children(masked: &str, from: usize, to: usize) -> Option<Vec<XmlChild>> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut at = from;
    while let Some(i) = masked.get(at..to)?.find('<') {
        let lt = at + i;
        let gt = lt + masked.get(lt..to)?.find('>')?;
        let inner = &masked[lt + 1..gt];
        at = gt + 1;
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let (open, start, open_end) = stack.pop()?;
            if open != name.trim() {
                return None;
            }
            if stack.is_empty() {
                out.push(XmlChild { name: open, start, open_end, close: lt, end: at, self_closing: false });
            }
            continue;
        }
        let name: String = inner.chars().take_while(|c| !c.is_whitespace() && *c != '/').collect();
        if inner.trim_end().ends_with('/') {
            if stack.is_empty() {
                out.push(XmlChild { name, start: lt, open_end: at, close: at, end: at, self_closing: true });
            }
        } else {
            stack.push((name, lt, at));
        }
    }
    stack.is_empty().then_some(out)
}

/// Корневой элемент файла (первый элемент верхнего уровня).
fn xml_root(masked: &str) -> Option<XmlChild> {
    xml_children(masked, 0, masked.len())?.into_iter().find(|c| !c.self_closing)
}

/// Текст узла `<theme>/<path…>` из `rc.xml`; `None` — узла нет. Пустой
/// узел — пустая строка.
pub fn rc_xml_theme_value(text: &str, path: &[&str]) -> Option<String> {
    let masked = xml_mask_comments(text);
    let mut node = xml_root(&masked)?;
    for comp in std::iter::once("theme").chain(path.iter().copied()) {
        if node.self_closing {
            return None;
        }
        node = xml_children(&masked, node.open_end, node.close)?.into_iter().find(|c| c.name == comp)?;
    }
    Some(if node.self_closing { String::new() } else { xml_unescape(text[node.open_end..node.close].trim()) })
}

/// Отступ строки, на которой начинается `at`.
fn xml_line_indent(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..at].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// Вложенные элементы `comps` со значением `value` в самом глубоком; каждый
/// уровень глубже — на два пробела от `indent`.
fn xml_nested(comps: &[&str], value: &str, indent: &str) -> String {
    match comps {
        [] => value.to_string(),
        [one] => format!("<{one}>{value}</{one}>"),
        [first, rest @ ..] => {
            let inner = format!("{indent}  ");
            format!("<{first}>\n{inner}{}\n{indent}</{first}>", xml_nested(rest, value, &inner))
        }
    }
}

/// Новый `rc.xml`, где узел `<theme>/<path…>` = `value`. Есть узел — меняется
/// только его текст; нет — недостающие элементы встают первыми в
/// ближайшего существующего родителя (нет и `<theme>` — она встаёт в корень).
/// Всё прочее в файле остаётся как было.
///
/// `existing = None` (ни своего файла, ни системного образца): у labwc
/// заводится файл с одной `<theme>`, у Openbox — `None`, как у шрифта
/// ([`rc_xml_with_title_font`]): его `rc.xml` заменяет системный целиком.
/// `None` и у файла, который не разбирается, — чужое не портим.
pub fn rc_xml_with_theme_value(existing: Option<&str>, wm: TitleWm, path: &[&str], value: &str) -> Option<String> {
    let root = wm.rc_xml_root();
    let mut text = match existing {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ if wm == TitleWm::Labwc => format!("<?xml version=\"1.0\"?>\n<{root}>\n</{root}>\n"),
        _ => return None,
    };
    let value = xml_escape_text(value);
    let full: Vec<&str> = std::iter::once("theme").chain(path.iter().copied()).collect();
    let masked = xml_mask_comments(&text);
    let mut node = xml_root(&masked)?;
    if node.self_closing {
        return None;
    }
    for (i, comp) in full.iter().enumerate() {
        let found = xml_children(&masked, node.open_end, node.close)?.into_iter().find(|c| c.name == *comp);
        match found {
            Some(child) if child.self_closing => {
                let indent = xml_line_indent(&text, child.start);
                text.replace_range(child.start..child.end, &xml_nested(&full[i..], &value, &indent));
                return Some(text);
            }
            Some(child) if i + 1 == full.len() => {
                text.replace_range(child.open_end..child.close, &value);
                return Some(text);
            }
            Some(child) => node = child,
            None => {
                let indent = format!("{}  ", xml_line_indent(&text, node.start));
                let block = format!("\n{indent}{}", xml_nested(&full[i..], &value, &indent));
                // Родитель в одну строку (`<theme></theme>`) — закрывающий
                // тег переносится на свою строку.
                let tail = if text[node.open_end..node.close].contains('\n') {
                    String::new()
                } else {
                    format!("\n{}", xml_line_indent(&text, node.start))
                };
                text.insert_str(node.open_end, &format!("{block}{tail}"));
                return Some(text);
            }
        }
    }
    None
}

/// Пути системных `rc.xml` менеджера (`$XDG_CONFIG_DIRS`, затем
/// `/etc/xdg`) — образец, с которого заводится свой файл (Openbox и labwc
/// читают ПЕРВЫЙ найденный `rc.xml`, а не сливают их).
pub fn rc_xml_system_paths(wm: TitleWm, xdg_config_dirs: &str) -> Vec<String> {
    let Some(rel) = wm.rc_xml_path() else { return Vec::new() };
    let mut dirs: Vec<&str> = xdg_config_dirs.split(':').filter(|d| !d.is_empty()).collect();
    dirs.push("/etc/xdg");
    let mut out: Vec<String> = Vec::new();
    for d in dirs {
        let p = format!("{}/{rel}", d.trim_end_matches('/'));
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Пути `lxqt/session.conf` по старшинству: свой, затем системные
/// (`$XDG_CONFIG_DIRS`, `/etc/xdg`, `/usr/share` — как их ищет `startlxqt`).
pub fn lxqt_session_conf_paths(config_home: &str, xdg_config_dirs: &str) -> Vec<String> {
    let mut dirs = vec![config_home.to_string()];
    dirs.extend(xdg_config_dirs.split(':').filter(|d| !d.is_empty()).map(str::to_string));
    dirs.extend(["/etc/xdg".to_string(), "/usr/share".to_string()]);
    dirs.iter().map(|d| format!("{}/lxqt/session.conf", d.trim_end_matches('/'))).collect()
}

/// [`title_wm_for`] с чтением `lxqt/session.conf` через `read` (подделка в
/// проверках, файлы на живой машине).
pub fn session_title_wm(
    de: &str,
    config_home: &str,
    xdg_config_dirs: &str,
    wayland: bool,
    read: &dyn Fn(&str) -> Option<String>,
) -> Option<TitleWm> {
    let confs: Vec<String> = if de == "lxqt" {
        lxqt_session_conf_paths(config_home, xdg_config_dirs).iter().filter_map(|p| read(p)).collect()
    } else {
        Vec::new()
    };
    let refs: Vec<&str> = confs.iter().map(String::as_str).collect();
    title_wm_for(de, &refs, wayland)
}

/// Свой `rc.xml` менеджера, а без него — первый системный: `(путь своего,
/// текст, из системного ли)`. Текста нет — нет ни того, ни другого.
pub fn rc_xml_source(
    wm: TitleWm,
    config_home: &str,
    xdg_config_dirs: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> (String, Option<String>, bool) {
    let own = format!("{}/{}", config_home.trim_end_matches('/'), wm.rc_xml_path().unwrap_or_default());
    if let Some(text) = read(&own) {
        return (own, Some(text), false);
    }
    let system = rc_xml_system_paths(wm, xdg_config_dirs).iter().find_map(|p| read(p));
    let from_system = system.is_some();
    (own, system, from_system)
}

/// Почему шрифты в этом окружении не меняются вовсе — до окна выбора:
/// Deepin (в Treeland их задаёт только Центр управления, другие сеансы
/// Deepin Rexon не поддерживает, [`crate::dde::refusal_live`]). `None` —
/// меняются.
pub fn font_refusal_live(de: &str) -> Option<String> {
    (de == "deepin").then(crate::dde::refusal_live)
}

/// Есть ли в окружении `de` куда применить шрифт `target`.
pub fn font_target_available(de: &str, target: &str) -> bool {
    font_target_available_wm(de, target, None)
}

/// То же с оконным менеджером сеанса ([`title_wm_for`]): в LXQt и labwc
/// заголовок применим, если менеджер опознан.
pub fn font_target_available_wm(de: &str, target: &str, wm: Option<TitleWm>) -> bool {
    target != "title" || TITLE_FONT_DES.contains(&de) || wm.is_some()
}

/// Цели шрифтов, которые в окружении `de` можно применить.
pub fn font_targets_for(de: &str) -> Vec<&'static str> {
    font_targets_for_wm(de, None)
}

/// [`font_targets_for`] с оконным менеджером сеанса.
pub fn font_targets_for_wm(de: &str, wm: Option<TitleWm>) -> Vec<&'static str> {
    FONT_TARGETS
        .iter()
        .copied()
        .filter(|t| font_target_available_wm(de, t, wm))
        .collect()
}

/// Цели шрифтов, которые страница показывает в окружении `de`, — с
/// пометкой, можно ли применить. Заголовок окон там, где его рисует не
/// настраиваемый нами оконный менеджер, не показывается вовсе; остальные
/// цели применимы везде (у «маленького», «панели инструментов» и «меню»
/// вне KDE — общим слоем Rexon).
pub fn font_targets_shown(de: &str) -> Vec<(&'static str, bool)> {
    font_targets_shown_wm(de, None)
}

/// [`font_targets_shown`] с оконным менеджером сеанса.
pub fn font_targets_shown_wm(de: &str, wm: Option<TitleWm>) -> Vec<(&'static str, bool)> {
    FONT_TARGETS
        .iter()
        .copied()
        .filter(|t| *t != "title" || font_target_available_wm(de, t, wm))
        .map(|t| (t, font_target_available_wm(de, t, wm)))
        .collect()
}

/// Собрать [`FontPlan`] для окружения `de` и цели `target`.
///
/// * Родной ключ окружения — [`font_backend`], у COSMIC и LXQt — файл:
///   COSMIC `com.system76.CosmicTk` `interface_font`/`monospace_font`;
///   LXQt `lxqt/lxqt.conf` `[Qt] font` (так пишет `lxqt-config-appearance`,
///   `fontsconfig.cpp`) и `[Qt] fixedFont` (его читает платформенная тема
///   `lxqt-qtplugin`, `lxqtplatformtheme.cpp:267`).
/// * Спутник для GTK — `org.gnome.desktop.interface`: его читает GTK на
///   Wayland и GNOME-программы в любом окружении (так же делает сам LXQt,
///   `configothertoolkits.cpp:85`). В COSMIC не пишем: `cosmic-settings-daemon`
///   сам переносит туда шрифт из `CosmicTk` (`src/theme.rs:298-303`) и
///   перезаписал бы наш.
/// * Спутник для Qt — `qt6ct/qt6ct.conf` `[Fonts] general|fixed`, только если
///   файл уже есть (человек пользуется qt6ct); в KDE и LXQt Qt-программы
///   берут шрифт из родного хранилища.
/// * Заголовок в GNOME/Cinnamon/MATE/Budgie: вместе с `titlebar-font`
///   снимается `titlebar-uses-system-font` — при `true` ключ шрифта
///   игнорируется (описание ключа в схемах), а в Arch у Cinnamon он включён
///   по умолчанию (`30_org.archlinux.cinnamon.gschema.override`).
pub fn font_plan(de: &str, target: &str) -> FontPlan {
    font_plan_wm(de, target, None)
}

/// [`font_plan`] с оконным менеджером сеанса ([`title_wm_for`]): шрифт
/// заголовка в LXQt, labwc и sway пишется туда, откуда его берёт этот
/// менеджер. Openbox и labwc — `rc.xml` и `--reconfigure`; xfwm4, Marco и
/// KWin под LXQt — те же ключи, что в их родных окружениях (XFCE, MATE,
/// KDE); sway — [`FontFile::SwayConf`] (сам sway узнаёт шрифт командой
/// `swaymsg font …` из [`sway_title_font_steps`]).
pub fn font_plan_wm(de: &str, target: &str, wm: Option<TitleWm>) -> FontPlan {
    if target == "title" && !TITLE_FONT_DES.contains(&de) {
        match wm {
            Some(TitleWm::Xfwm4) => return font_plan_wm("xfce", target, None),
            Some(TitleWm::Marco) => return font_plan_wm("mate", target, None),
            Some(TitleWm::KWin) => return font_plan_wm("kde", target, None),
            Some(TitleWm::Sway) => {
                return FontPlan {
                    primary: FontStore::File(FontFile::SwayConf),
                    also: Vec::new(),
                    extra: Vec::new(),
                    notify: Vec::new(),
                    gtk_ini: false,
                };
            }
            Some(wm @ (TitleWm::Openbox | TitleWm::Labwc)) => {
                return FontPlan {
                    primary: FontStore::File(FontFile::RcXml { wm }),
                    also: Vec::new(),
                    extra: Vec::new(),
                    notify: vec![vec![wm.program().to_string(), "--reconfigure".to_string()]],
                    gtk_ini: false,
                };
            }
            None => {}
        }
    }
    let native_file = match (de, target) {
        ("cosmic", "system") => Some(FontFile::CosmicTk {
            key: "interface_font".to_string(),
        }),
        ("cosmic", "mono") => Some(FontFile::CosmicTk {
            key: "monospace_font".to_string(),
        }),
        ("lxqt", "system" | "mono") => Some(FontFile::QtIni {
            path: "lxqt/lxqt.conf".to_string(),
            group: "Qt".to_string(),
            key: if target == "mono" { "fixedFont" } else { "font" }.to_string(),
            create: true,
        }),
        // Вне KDE у этих шрифтов родного ключа нет: родным служит наш блок
        // в gtk.css GTK 3 (из него шрифт и читается обратно).
        (_, "small" | "toolbar" | "menu") if de != "kde" => Some(FontFile::GtkCss {
            version: "3.0".to_string(),
            target: target.to_string(),
        }),
        _ => None,
    };
    let backend = font_backend(de, target);
    let primary = match native_file {
        Some(file) => FontStore::File(file),
        None => FontStore::Cmd(backend.clone()),
    };

    let mut also = Vec::new();
    if matches!(target, "system" | "mono") {
        let (schema, key) = font_gsettings_key("gnome", target);
        let shared = SoundBackend::GSettings {
            schema: schema.to_string(),
            key: key.to_string(),
        };
        if de != "cosmic" && primary != FontStore::Cmd(shared.clone()) {
            also.push(FontStore::Cmd(shared));
        }
        if de != "kde" && de != "lxqt" {
            also.push(FontStore::File(FontFile::QtIni {
                path: "qt6ct/qt6ct.conf".to_string(),
                group: "Fonts".to_string(),
                key: if target == "mono" { "fixed" } else { "general" }.to_string(),
                create: false,
            }));
        }
    }
    if EXTRA_FONT_TARGETS.contains(&target) {
        // Общий слой: GTK 3 (вне KDE он уже родной), GTK 4, qt6ct/qt5ct вне
        // KDE и LXQt (там Qt-программы берут шрифты у своего окружения, а
        // платформенная тема LXQt таблиц стилей не читает), kdeglobals вне
        // KDE — для KDE-программ с платформенной темой Plasma.
        let gtk3 = FontFile::GtkCss { version: "3.0".to_string(), target: target.to_string() };
        if primary != FontStore::File(gtk3.clone()) {
            also.push(FontStore::File(gtk3));
        }
        also.push(FontStore::File(FontFile::GtkCss { version: "4.0".to_string(), target: target.to_string() }));
        if de != "kde" && de != "lxqt" {
            for tool in QTCT_TOOLS {
                also.push(FontStore::File(FontFile::QtCtQss { tool: tool.to_string(), target: target.to_string() }));
            }
        }
        if de != "kde" {
            also.push(FontStore::Cmd(font_backend("kde", target)));
        }
    }

    let mut extra = Vec::new();
    if let FontStore::Cmd(SoundBackend::GSettings { schema, key }) = &primary {
        if key == "titlebar-font" {
            extra.push(vec![
                "gsettings".to_string(),
                "set".to_string(),
                schema.clone(),
                "titlebar-uses-system-font".to_string(),
                "false".to_string(),
            ]);
        }
    }

    FontPlan {
        primary,
        also,
        extra,
        notify: Vec::new(),
        gtk_ini: font_syncs_to_gtk(target),
    }
}

/// argv записи шрифта в хранилище-команду; для файла — `None`.
pub fn font_store_write_argv(
    store: &FontStore,
    family: &str,
    size: i32,
    kde_tool: &str,
) -> Option<Vec<String>> {
    font_store_write_argv_face(store, family, size, FontFace::default(), kde_tool)
}

/// [`font_store_write_argv`] с начертанием: KDE получает его полями QFont
/// ([`kde_font_spec_face`]), gsettings и xfconf — словами описания Pango
/// («Sans Bold Italic 10», [`pango_font_desc`]): так их записывают и читают
/// сами GNOME, MATE, Cinnamon, Budgie и XFCE.
pub fn font_store_write_argv_face(
    store: &FontStore,
    family: &str,
    size: i32,
    face: FontFace,
    kde_tool: &str,
) -> Option<Vec<String>> {
    let FontStore::Cmd(backend) = store else {
        return None;
    };
    let value = match backend {
        SoundBackend::KConfig { .. } => kde_font_spec_face(family, size, face),
        SoundBackend::GSettings { .. } | SoundBackend::Xfconf { .. } => pango_font_desc(family, size, face),
        SoundBackend::Dde { .. } => gnome_font_spec(family, size),
    };
    sound_write_argv(backend, &value, kde_tool)
}

/// Значение ключа `key` в секции `[group]` ini-текста.
fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_group = &line[1..line.len() - 1] == group;
            continue;
        }
        if in_group {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// RON-запись `FontConfig` COSMIC — в том виде, как её пишет `cosmic-config`
/// (`to_string_pretty` с `PrettyConfig::new()`). Кегля в COSMIC нет вовсе:
/// размер текста там задаётся плотностью интерфейса, не шрифтом.
pub fn cosmic_font_ron(family: &str) -> String {
    cosmic_font_ron_face(family, FontFace::default())
}

/// Начертание из окна выбора шрифта: жирность по шкале Qt 6 (100–900;
/// `0` — не выбрана) и наклон.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FontFace {
    pub weight: i32,
    pub italic: bool,
}

/// Начертание из ответа окна выбора «Семейство,кегль,жирность,наклон»
/// (третье и четвёртое поля; нет их — начертание не выбрано).
pub fn parse_font_face(spec: &str) -> FontFace {
    let mut parts = spec.trim().split(',').skip(2);
    let weight = parts.next().and_then(|w| w.trim().parse::<f64>().ok()).map(|w| w.round() as i32).unwrap_or(0);
    let italic = matches!(parts.next().map(str::trim), Some("1") | Some("true"));
    FontFace { weight: weight.clamp(0, 1000), italic }
}

/// Жирность в записи `iced::font::Weight` (ей пользуется `FontConfig`
/// COSMIC): ближайшая ступень шкалы 100–900.
pub fn cosmic_weight_name(weight: i32) -> &'static str {
    const NAMES: [&str; 9] =
        ["Thin", "ExtraLight", "Light", "Normal", "Medium", "Semibold", "Bold", "ExtraBold", "Black"];
    if weight <= 0 {
        return "Normal";
    }
    let step = ((weight as f64 / 100.0).round() as i32).clamp(1, 9);
    NAMES[(step - 1) as usize]
}

/// RON-запись `FontConfig` COSMIC с начертанием: `weight` — ступень
/// `iced::font::Weight`, `style` — `Italic` при наклоне; ширина
/// (`stretch`) окном выбора не задаётся и остаётся `Normal`. Первая строка
/// — метка [`COSMIC_FONT_MARK`]: по ней сброс шрифтов узнаёт свой файл.
pub fn cosmic_font_ron_face(family: &str, face: FontFace) -> String {
    let family = family.replace('\\', "\\\\").replace('"', "\\\"");
    let weight = cosmic_weight_name(face.weight);
    let style = if face.italic { "Italic" } else { "Normal" };
    format!(
        "{COSMIC_FONT_MARK}\n(\n    family: \"{family}\",\n    weight: {weight},\n    stretch: Normal,\n    style: {style},\n)"
    )
}

/// Жирность и наклон из RON-записи `FontConfig` COSMIC — для проверки
/// записанного: `(ступень weight, style)`.
pub fn cosmic_font_face_fields(text: &str) -> (String, String) {
    let field = |name: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(name))
            .map(|v| v.trim().trim_end_matches(',').trim().to_string())
            .next()
            .unwrap_or_default()
    };
    (field("weight:"), field("style:"))
}

/// Семейство из RON-записи `FontConfig` COSMIC; пусто — не нашлось.
pub fn cosmic_font_family(text: &str) -> String {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("family:") {
            let value = rest.trim().trim_end_matches(',').trim();
            if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                return inner.replace("\\\"", "\"").replace("\\\\", "\\");
            }
        }
    }
    String::new()
}

/// Семейство и кегль из строки шрифта. Понимает обе записи: GNOME
/// «Noto Sans 11» и KDE «Noto Sans,11,-1,5,Regular,0,0,0,0,0».
/// Кегль `0` — «в строке его не было».
pub fn parse_font_spec(raw: &str) -> (String, i32) {
    let raw = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if raw.is_empty() {
        return (String::new(), 0);
    }
    if let Some((family, tail)) = raw.split_once(',') {
        let size = tail
            .split(',')
            .next()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .map(|v| v as i32)
            .unwrap_or(10);
        return (family.trim().to_string(), size);
    }
    if let Some((head, tail)) = raw.rsplit_once(' ') {
        if let Ok(size) = tail.trim().parse::<f64>() {
            return (head.trim().to_string(), size as i32);
        }
    }
    (raw.to_string(), 0)
}

/// Строка шрифта в записи GNOME/XFCE: «Noto Sans 11».
pub fn gnome_font_spec(family: &str, size: i32) -> String {
    format!("{family} {size}")
}

/// Строка QFont в записи Qt 6 (`QFont::toString`, 16 полей): семейство,
/// кегль, пиксельный кегль, styleHint, вес (400 — обычный), стиль и флаги.
///
/// ⚠️ Прежде здесь стояла десятиполевая запись с «Regular» в поле веса. Qt 6
/// при десяти полях читает вес по старой шкале и «Regular» разбирает как `0`
/// → вес 100 (Thin): проверено `QFont::fromString` из PySide6 30.09.2026 —
/// `…,5,Regular,0,0,0,0,0` даёт weight 100, эта запись даёт 400. Так же
/// пишет сама Plasma 6 (`activeFont=Segoe UI,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0`).
pub fn qt_font_spec(family: &str, size: i32) -> String {
    qt_font_spec_face(family, size, FontFace::default())
}

/// [`qt_font_spec`] с начертанием — так его пишет сама Plasma: жирность —
/// пятое поле по шкале Qt 6 (400 обычная, 600 полужирная, 700 жирная;
/// не выбрана — 400), наклон — шестое (`1` — italic). Имя стиля
/// (семнадцатое поле) не пишем: тогда Qt подбирает начертание по жирности,
/// а не по имени, которое у каждого семейства своё («SemiBold»,
/// «Semibold», «DemiBold»).
pub fn qt_font_spec_face(family: &str, size: i32, face: FontFace) -> String {
    let weight = if face.weight > 0 { face.weight.clamp(1, 1000) } else { 400 };
    let style = i32::from(face.italic);
    format!("{family},{size},-1,5,{weight},{style},0,0,0,0,0,0,0,0,0,1")
}

/// Строка шрифта в записи KDE — сериализация QFont, см. [`qt_font_spec`].
pub fn kde_font_spec(family: &str, size: i32) -> String {
    qt_font_spec(family, size)
}

/// [`kde_font_spec`] с начертанием, см. [`qt_font_spec_face`].
pub fn kde_font_spec_face(family: &str, size: i32, face: FontFace) -> String {
    qt_font_spec_face(family, size, face)
}

/// Жирность Qt 5 (шкала 0–99: 50 обычная, 63 полужирная, 75 жирная) — в
/// шкалу Qt 6, ближайшей ступенью (как `qt_legacyToOpenTypeWeight` в Qt).
fn qt5_weight_to_qt6(weight: i32) -> i32 {
    const STEPS: [(i32, i32); 9] =
        [(0, 100), (12, 200), (25, 300), (50, 400), (57, 500), (63, 600), (75, 700), (81, 800), (87, 900)];
    STEPS.iter().min_by_key(|(old, _)| (old - weight).abs()).map(|(_, new)| *new).unwrap_or(400)
}

/// Сериализация QFont (`QFont::toString`: десять-одиннадцать полей Qt 5 или
/// шестнадцать и больше Qt 6) → семейство, кегль и начертание. `None` —
/// это не запись QFont. Кегль в пикселях (точек нет) — `0`.
///
/// Жирность — пятое поле: у записи Qt 6 по шкале 100–900, у старой — по
/// шкале Qt 5 (так же читает `QFont::fromString`). Слово вместо числа
/// («Regular» — так писали старые версии Rexon) и пустая жирность при
/// заданном имени стиля читаются по словам начертания. Наклон — шестое
/// поле (`1` italic, `2` oblique) или слово в имени стиля.
pub fn parse_qt_font(raw: &str) -> Option<(String, i32, FontFace)> {
    let raw = raw.trim().trim_matches(['\'', '"']).trim();
    let fields: Vec<&str> = raw.split(',').map(str::trim).collect();
    if fields.len() < 10 || fields[0].is_empty() {
        return None;
    }
    let size = fields[1].parse::<f64>().ok().map(|v| v.round() as i32).filter(|v| *v > 0).unwrap_or(0);
    let qt6 = fields.len() >= 16;
    let style_name = fields.get(if qt6 { 16 } else { 10 }).copied().unwrap_or("");
    let words: Vec<String> = style_name.split_whitespace().map(|w| w.to_ascii_lowercase().replace('-', "")).collect();
    let mut face = FontFace::default();
    face.weight = match fields[4].parse::<f64>() {
        Ok(w) if qt6 => (w.round() as i32).clamp(0, 1000),
        Ok(w) => qt5_weight_to_qt6(w.round() as i32),
        Err(_) => face_weight_word(&fields[4].to_ascii_lowercase().replace('-', "")).unwrap_or(0),
    };
    if face.weight == 0 || (face.weight == 400 && !words.is_empty()) {
        if let Some(w) = words.iter().find_map(|w| face_weight_word(w)).filter(|w| *w > 0) {
            face.weight = w;
        }
    }
    if face.weight == 0 {
        face.weight = 400;
    }
    face.italic = matches!(fields[5], "1" | "2") || words.iter().any(|w| w == "italic" || w == "oblique");
    Some((fields[0].to_string(), size, face))
}

/// Семейство, кегль и начертание из строки шрифта любой из записей:
/// сериализации QFont ([`parse_qt_font`]) или описания Pango «Семейство
/// [Начертание] Кегль» ([`parse_pango_font_desc`]) — в ней хранят шрифт
/// gsettings GNOME/MATE/Cinnamon/Budgie и xfconf. Кегль `0` — «в строке его
/// не было»; начертание не названо — [`FontFace::default`].
pub fn parse_font_spec_face(raw: &str) -> (String, i32, FontFace) {
    let raw = raw.trim().trim_matches(['\'', '"']).trim();
    if raw.is_empty() {
        return (String::new(), 0, FontFace::default());
    }
    if let Some(qt) = parse_qt_font(raw) {
        return qt;
    }
    if raw.contains(',') {
        let (family, size) = parse_font_spec(raw);
        return (family, size, FontFace::default());
    }
    let has_size = raw.split_whitespace().last().is_some_and(|w| w.trim_end_matches("px").parse::<f64>().is_ok());
    let (family, size, face) = parse_pango_font_desc(raw);
    (family, if has_size { size } else { 0 }, face)
}

/// argv чтения шрифта командой окружения. `kde_tool` — найденный
/// `kreadconfig6` (пусто — читать нечем). Родное хранилище COSMIC и LXQt —
/// файл, его читают через [`font_plan`].
pub fn font_read_argv(de: &str, target: &str, kde_tool: &str) -> Option<Vec<String>> {
    sound_read_argv(&font_backend(de, target), kde_tool)
}

/// argv записи шрифта командой окружения. Строка шрифта берётся в записи
/// того хранилища, куда пишем: KDE понимает сериализацию QFont, остальные —
/// «Семейство Кегль».
pub fn font_write_argv(
    de: &str,
    target: &str,
    family: &str,
    size: i32,
    kde_tool: &str,
) -> Option<Vec<String>> {
    font_write_argv_face(de, target, family, size, FontFace::default(), kde_tool)
}

/// [`font_write_argv`] с начертанием ([`font_store_write_argv_face`]).
pub fn font_write_argv_face(
    de: &str,
    target: &str,
    family: &str,
    size: i32,
    face: FontFace,
    kde_tool: &str,
) -> Option<Vec<String>> {
    font_store_write_argv_face(&FontStore::Cmd(font_backend(de, target)), family, size, face, kde_tool)
}

/// Ключ `settings.ini`, куда переносится шрифт интерфейса. Моноширинный и
/// заголовочный своих ключей в GTK не имеют — переносится только `system`.
pub const GTK_FONT_KEY: &str = "gtk-font-name";

/// Нужно ли переносить этот шрифт в `gtk-{3,4}.0/settings.ini`.
pub fn font_syncs_to_gtk(target: &str) -> bool {
    target == "system"
}

/// Файлы `settings.ini`, куда переносится шрифт интерфейса для GTK, — по
/// одному на каждую версию из `gtk_sanitizer::GTK_VERSIONS`.
///
/// Починка 02.10.2026: окно подавало в `gtk_ini_path` имя «gtk-3.0», а та
/// сама дописывает «gtk-», и шрифт уходил в `~/.config/gtk-gtk-3.0/` —
/// каталог, которого GTK не читает. Путь теперь собирается здесь, в одном
/// месте, из тех же версий, что у санитайзера.
pub fn gtk_font_ini_paths(config_home: &str) -> Vec<String> {
    crate::gtk_sanitizer::GTK_VERSIONS
        .iter()
        .map(|v| crate::gtk_sanitizer::gtk_ini_path(config_home, v))
        .collect()
}

/// Каталоги, которые заводила прежняя ошибочная запись шрифта
/// (`gtk-gtk-3.0`, `gtk-gtk-4.0`), — каждый с путём его `settings.ini`.
pub fn stray_gtk_font_dirs(config_home: &str) -> Vec<(String, String)> {
    let base = config_home.trim_end_matches('/');
    crate::gtk_sanitizer::GTK_VERSIONS
        .iter()
        .map(|v| {
            let dir = format!("{base}/gtk-gtk-{v}");
            let ini = format!("{dir}/settings.ini");
            (dir, ini)
        })
        .collect()
}

/// Можно ли снять мусорный каталог прежней записи: в нём ровно один файл
/// `settings.ini` (`entries` — имена в каталоге), и в файле ровно то, что
/// туда клала та запись, — секция `[Settings]` с одним ключом
/// `gtk-font-name`. Что-то ещё — каталог чей-то, его не трогаем.
pub fn stray_gtk_font_dir_removable(entries: &[String], ini_text: Option<&str>) -> bool {
    if entries.len() != 1 || entries[0] != "settings.ini" {
        return false;
    }
    let Some(text) = ini_text else { return false };
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.len() == 2
        && lines[0] == "[Settings]"
        && lines[1].starts_with(&format!("{GTK_FONT_KEY}="))
}

// ============================================================================
// Отрисовка шрифтов: сглаживание, субпиксели, хинтинг
// ============================================================================
//
// Эталон набора — страница «Шрифты» Plasma 6: сглаживание вкл/выкл с
// диапазоном кеглей «не сглаживать», субпиксельная отрисовка и степень
// хинтинга. Здесь та же настройка — для ЛЮБОГО окружения:
//
// * общий слой — свой блок в `~/.config/fontconfig/fonts.conf`: этот файл
//   читают все программы, которые подбирают шрифт через fontconfig (GTK, Qt,
//   Electron, Firefox, Chromium), в любом окружении и на X11, и на Wayland;
// * поверх — родные ключи окружения (их показывают и меняют его собственные
//   настройки, а GTK/Qt берут оттуда значения на лету): KDE — `kdeglobals
//   [General] XftAntialias/XftHintStyle/XftSubPixel`, XFCE — канал
//   `xsettings` `/Xft/*`, Cinnamon — `org.cinnamon.settings-daemon.plugins.
//   xsettings`, MATE — `org.mate.font-rendering`, остальные — ключи GNOME
//   `org.gnome.desktop.interface font-antialiasing/font-hinting/
//   font-rgba-order` (их читает сам GTK и на Wayland). Ключи сверены по
//   схемам gsettings и исходникам настроек окружений (04.10.2026).
//
// ⚠️ Блок в `fonts.conf` записан в той же форме, в какой этот файл ведёт
// страница «Шрифты» KDE: по одному `<match target="pattern">` с одним
// `<edit mode="assign">` на свойство, диапазон без сглаживания — двумя
// `<test>` по `size` и по `pixelsize`. Тогда KDE, открыв файл, узнаёт наши
// записи как свои и правит их на месте, а не заводит вторые.

/// Порядок субпикселей экрана, как его называет fontconfig (`rgba`).
pub const SUBPIXEL_ORDERS: &[&str] = &["none", "rgb", "bgr", "vrgb", "vbgr"];

/// Степени хинтинга (`hintstyle` без приставки `hint`).
pub const HINT_STYLES: &[&str] = &["none", "slight", "medium", "full"];

/// Диапазон «не сглаживать», который предлагается, пока человек его не
/// задавал (то же умолчание, что у KDE).
pub const AA_EXCLUDE_DEFAULT: (i32, i32) = (8, 15);

/// Границы кегля в диапазоне «не сглаживать».
pub const AA_EXCLUDE_LIMITS: (i32, i32) = (1, 99);

/// DPI, от которого считается кегль в пикселях, если своего человек не
/// задавал (опорное значение X11 и Wayland).
pub const FONT_BASE_DPI: i32 = 96;

/// Настройка отрисовки шрифтов целиком.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontRendering {
    pub antialias: bool,
    /// Кегли (pt) без сглаживания, включительно; `None` — исключений нет.
    pub exclude: Option<(i32, i32)>,
    /// Один из [`SUBPIXEL_ORDERS`].
    pub subpixel: String,
    /// Один из [`HINT_STYLES`].
    pub hinting: String,
}

impl Default for FontRendering {
    /// Умолчания — как у KDE: сглаживание есть, субпиксели RGB, лёгкий
    /// хинтинг.
    fn default() -> Self {
        FontRendering {
            antialias: true,
            exclude: None,
            subpixel: "rgb".to_string(),
            hinting: "slight".to_string(),
        }
    }
}

impl FontRendering {
    /// Привести к допустимому: незнакомые значения — к умолчанию, границы
    /// диапазона — по порядку и в пределах [`AA_EXCLUDE_LIMITS`]. Без
    /// сглаживания субпикселей не бывает (так же ведёт себя KDE).
    pub fn normalized(mut self) -> Self {
        let def = FontRendering::default();
        if !SUBPIXEL_ORDERS.contains(&self.subpixel.as_str()) {
            self.subpixel = def.subpixel;
        }
        if !HINT_STYLES.contains(&self.hinting.as_str()) {
            self.hinting = def.hinting;
        }
        if let Some((a, b)) = self.exclude {
            let (lo, hi) = AA_EXCLUDE_LIMITS;
            let (a, b) = (a.clamp(lo, hi), b.clamp(lo, hi));
            self.exclude = Some((a.min(b), a.max(b)));
        }
        if !self.antialias {
            self.subpixel = "none".to_string();
        }
        self
    }

    /// `font-antialiasing` GNOME (и `antialiasing` Cinnamon/MATE): одно
    /// значение на «сглаживание» и «субпиксели» вместе.
    pub fn gnome_antialiasing(&self) -> &'static str {
        if !self.antialias {
            "none"
        } else if self.subpixel == "none" {
            "grayscale"
        } else {
            "rgba"
        }
    }

    /// `font-rgba-order` GNOME: порядок без «none» (у перечисления его нет).
    pub fn gnome_rgba_order(&self) -> &str {
        if self.subpixel == "none" {
            "rgb"
        } else {
            &self.subpixel
        }
    }

    /// Степень хинтинга в записи Xft/fontconfig: `hintslight`.
    pub fn xft_hintstyle(&self) -> String {
        format!("hint{}", self.hinting)
    }
}

/// Кегль в пунктах → в пикселях при данном DPI (округление к ближайшему).
pub fn point_to_pixel(points: i32, dpi: i32) -> i32 {
    ((points as f64 * dpi as f64) / 72.0).round() as i32
}

/// Какое свойство отрисовки хранит ключ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderKey {
    Antialias,
    /// Хинтинг вкл/выкл (есть у XFCE; у KDE и GNOME выводится из степени).
    Hinting,
    HintStyle,
    SubPixel,
}

/// Запись значений в хранилище окружения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDialect {
    /// `kdeglobals`: `true/false`, `hintslight`, `rgb`.
    Kde,
    /// gsettings-перечисления GNOME, Cinnamon и MATE: `rgba/grayscale/none`,
    /// `slight`, `rgb`.
    Gnome,
    /// Канал `xsettings` XFCE: `1/0`, `hintslight`, `rgb`.
    Xfce,
}

/// Родное хранилище отрисовки шрифтов окружения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderStore {
    pub dialect: RenderDialect,
    pub keys: Vec<(RenderKey, SoundBackend)>,
}

fn render_gsettings(schema: &str, prefix: &str) -> RenderStore {
    let key = |k: RenderKey, name: &str| {
        (
            k,
            SoundBackend::GSettings {
                schema: schema.to_string(),
                key: format!("{prefix}{name}"),
            },
        )
    };
    RenderStore {
        dialect: RenderDialect::Gnome,
        keys: vec![
            key(RenderKey::Antialias, "antialiasing"),
            key(RenderKey::HintStyle, "hinting"),
            key(RenderKey::SubPixel, "rgba-order"),
        ],
    }
}

/// Ключи GNOME — общий для GTK слой в любом окружении.
fn render_gnome_store() -> RenderStore {
    render_gsettings("org.gnome.desktop.interface", "font-")
}

/// Родное хранилище окружения `de`. `None` — своей настройки у окружения
/// нет, и родным для него служит сам `fonts.conf` (LXQt пишет туда же своей
/// страницей «Шрифты»; COSMIC, Deepin, тайловые сеансы — своей страницы не
/// имеют вовсе).
pub fn render_native_store(de: &str) -> Option<RenderStore> {
    let kde = |key: &str| SoundBackend::KConfig {
        file: "kdeglobals".to_string(),
        group: "General".to_string(),
        key: key.to_string(),
    };
    let xfce = |prop: &str| SoundBackend::Xfconf {
        channel: "xsettings".to_string(),
        prop: prop.to_string(),
    };
    match de {
        "kde" => Some(RenderStore {
            dialect: RenderDialect::Kde,
            keys: vec![
                (RenderKey::Antialias, kde("XftAntialias")),
                (RenderKey::HintStyle, kde("XftHintStyle")),
                (RenderKey::SubPixel, kde("XftSubPixel")),
            ],
        }),
        "xfce" => Some(RenderStore {
            dialect: RenderDialect::Xfce,
            keys: vec![
                (RenderKey::Antialias, xfce("/Xft/Antialias")),
                (RenderKey::Hinting, xfce("/Xft/Hinting")),
                (RenderKey::HintStyle, xfce("/Xft/HintStyle")),
                (RenderKey::SubPixel, xfce("/Xft/RGBA")),
            ],
        }),
        "cinnamon" | "cinnamon-mint" => {
            Some(render_gsettings("org.cinnamon.settings-daemon.plugins.xsettings", ""))
        }
        "mate" => Some(render_gsettings("org.mate.font-rendering", "")),
        "gnome" | "budgie" | "pantheon" => Some(render_gnome_store()),
        _ => None,
    }
}

/// Значение свойства `key` в записи хранилища `dialect`.
pub fn render_value(dialect: RenderDialect, key: RenderKey, r: &FontRendering) -> String {
    match (dialect, key) {
        (RenderDialect::Kde, RenderKey::Antialias) => bool_word(r.antialias).to_string(),
        (RenderDialect::Xfce, RenderKey::Antialias) => if r.antialias { "1" } else { "0" }.to_string(),
        (RenderDialect::Gnome, RenderKey::Antialias) => r.gnome_antialiasing().to_string(),
        (_, RenderKey::Hinting) => if r.hinting == "none" { "0" } else { "1" }.to_string(),
        (RenderDialect::Gnome, RenderKey::HintStyle) => r.hinting.clone(),
        (_, RenderKey::HintStyle) => r.xft_hintstyle(),
        (RenderDialect::Gnome, RenderKey::SubPixel) => r.gnome_rgba_order().to_string(),
        (_, RenderKey::SubPixel) => r.subpixel.clone(),
    }
}

/// Части настройки, найденные в одном месте (`None` — там не задано).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderingParts {
    pub antialias: Option<bool>,
    pub subpixel: Option<String>,
    pub hinting: Option<String>,
    pub exclude: Option<(i32, i32)>,
    /// DPI из блока `fonts.conf` (`<edit name="dpi">`).
    pub dpi: Option<i32>,
}

fn clean_answer(raw: &str) -> String {
    raw.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string()
}

/// Разобрать ответы хранилища (`(ключ, вывод команды чтения)`).
pub fn parse_render_answers(dialect: RenderDialect, answers: &[(RenderKey, String)]) -> RenderingParts {
    let mut out = RenderingParts::default();
    let get = |k: RenderKey| {
        answers
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| clean_answer(v))
            .filter(|v| !v.is_empty())
    };
    let hint = get(RenderKey::HintStyle).map(|h| h.strip_prefix("hint").unwrap_or(&h).to_string());
    out.hinting = hint.filter(|h| HINT_STYLES.contains(&h.as_str()));
    if dialect == RenderDialect::Xfce && get(RenderKey::Hinting).as_deref() == Some("0") {
        out.hinting = Some("none".to_string());
    }
    let sub = get(RenderKey::SubPixel).filter(|s| SUBPIXEL_ORDERS.contains(&s.as_str()) || s == "rgba");
    match dialect {
        RenderDialect::Gnome => match get(RenderKey::Antialias).as_deref() {
            Some("none") => {
                out.antialias = Some(false);
                out.subpixel = Some("none".to_string());
            }
            Some("grayscale") => {
                out.antialias = Some(true);
                out.subpixel = Some("none".to_string());
            }
            Some("rgba") => {
                out.antialias = Some(true);
                // Ник «rgba» у перечисления порядка значит «как у экрана» —
                // самый частый порядок RGB.
                out.subpixel = Some(match sub.as_deref() {
                    Some("rgba") | Some("none") | None => "rgb".to_string(),
                    Some(s) => s.to_string(),
                });
            }
            _ => {}
        },
        RenderDialect::Kde | RenderDialect::Xfce => {
            out.antialias = match get(RenderKey::Antialias).as_deref() {
                Some("true") | Some("1") => Some(true),
                Some("false") | Some("0") => Some(false),
                _ => None,
            };
            out.subpixel = sub.filter(|s| s != "rgba");
        }
    }
    out
}

/// Итоговая настройка: родное хранилище главнее `fonts.conf` (его показывает
/// и меняет само окружение), диапазон «не сглаживать» живёт только в
/// `fonts.conf`; чего нет нигде — умолчание.
pub fn merge_rendering(native: &RenderingParts, fontconfig: &RenderingParts) -> FontRendering {
    let def = FontRendering::default();
    FontRendering {
        antialias: native.antialias.or(fontconfig.antialias).unwrap_or(def.antialias),
        exclude: fontconfig.exclude,
        subpixel: native.subpixel.clone().or_else(|| fontconfig.subpixel.clone()).unwrap_or(def.subpixel),
        hinting: native.hinting.clone().or_else(|| fontconfig.hinting.clone()).unwrap_or(def.hinting),
    }
    .normalized()
}

/// План записи отрисовки шрифтов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderingPlan {
    /// Команды родного хранилища: по ним (и по записи `fonts.conf`)
    /// решается успех.
    pub primary: Vec<Vec<String>>,
    /// Общий слой для GTK-программ (best-effort).
    pub also: Vec<Vec<String>>,
}

fn render_store_argv(store: &RenderStore, r: &FontRendering, kwrite: &str) -> Vec<Vec<String>> {
    store
        .keys
        .iter()
        .filter_map(|(key, backend)| sound_write_argv(backend, &render_value(store.dialect, *key, r), kwrite))
        .collect()
}

/// Собрать [`RenderingPlan`]. `kwrite` — найденный `kwriteconfig6` (пусто —
/// нет); `has_gsettings` — есть ли `gsettings`.
pub fn rendering_plan(de: &str, r: &FontRendering, kwrite: &str, has_gsettings: bool) -> RenderingPlan {
    let native = render_native_store(de);
    let primary = native.as_ref().map(|s| render_store_argv(s, r, kwrite)).unwrap_or_default();
    let gnome = render_gnome_store();
    let also = if has_gsettings && native.as_ref() != Some(&gnome) {
        render_store_argv(&gnome, r, "")
    } else {
        Vec::new()
    };
    RenderingPlan { primary, also }
}

/// Команды чтения родного хранилища: `(ключ, argv)`.
pub fn rendering_read_argv(de: &str, kread: &str) -> Option<(RenderDialect, Vec<(RenderKey, Vec<String>)>)> {
    let store = render_native_store(de)?;
    let reads = store
        .keys
        .iter()
        .filter_map(|(key, backend)| sound_read_argv(backend, kread).map(|argv| (*key, argv)))
        .collect();
    Some((store.dialect, reads))
}

// ── Старый GNOME: ключи службы настроек ─────────────────────────────────
//
// До переноса в `org.gnome.desktop.interface` (`font-antialiasing`,
// `font-hinting`, `font-rgba-order` — gsettings-desktop-schemas 40+) GNOME
// держал отрисовку шрифтов в схеме своей службы настроек
// `org.gnome.settings-daemon.plugins.xsettings`: `antialiasing`
// (`none|grayscale|rgba`), `hinting` (`none|slight|medium|full`),
// `rgba-order` (`rgb|bgr|vrgb|vbgr`) — те же перечисления, что у новых
// ключей. Из них служба и раздаёт Xft/* программам X11. В свежих выпусках
// ключей в схеме уже нет, поэтому пишутся они только там, где схема их ещё
// держит ([`legacy_render_probe_argv`]), — в любом окружении: кроме самой
// службы GNOME их никто не читает.

/// Схема службы настроек старого GNOME.
pub const LEGACY_XSETTINGS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.xsettings";

/// Ключи отрисовки старого GNOME.
pub fn legacy_render_store() -> RenderStore {
    render_gsettings(LEGACY_XSETTINGS_SCHEMA, "")
}

/// argv проверки, что ключи старого GNOME есть: `gsettings writable` с
/// несуществующим ключом завершается ошибкой, с существующим печатает
/// `true`/`false`.
pub fn legacy_render_probe_argv() -> Vec<String> {
    ["gsettings", "writable", LEGACY_XSETTINGS_SCHEMA, "antialiasing"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Есть ли ключи старого GNOME и можно ли их писать — по ответу
/// [`legacy_render_probe_argv`] (пусто — ключа нет).
pub fn legacy_render_available(probe_answer: &str) -> bool {
    probe_answer.trim() == "true"
}

/// Команды записи отрисовки в ключи старого GNOME.
pub fn legacy_rendering_argv(r: &FontRendering) -> Vec<Vec<String>> {
    render_store_argv(&legacy_render_store(), r, "")
}

/// Команды чтения ключей старого GNOME: `(ключ, argv)`.
pub fn legacy_rendering_read_argv() -> (RenderDialect, Vec<(RenderKey, Vec<String>)>) {
    let store = legacy_render_store();
    let reads = store
        .keys
        .iter()
        .filter_map(|(key, backend)| sound_read_argv(backend, "").map(|argv| (*key, argv)))
        .collect();
    (store.dialect, reads)
}

/// Заменяют ли ключи старого GNOME родные ключи окружения `de`: у
/// окружений на службе настроек GNOME (GNOME, Budgie, Pantheon) в старых
/// выпусках новых ключей нет вовсе, и успех записи решают старые.
pub fn legacy_render_is_native(de: &str) -> bool {
    render_native_store(de).as_ref() == Some(&render_gnome_store())
}

/// Брать ли отрисовку из ключей старого GNOME: родное хранилище окружения
/// ничего не дало (у старого GNOME новых ключей нет), а старые есть.
pub fn rendering_needs_legacy(native: &RenderingParts, legacy_available: bool) -> bool {
    legacy_available && native.antialias.is_none() && native.hinting.is_none() && native.subpixel.is_none()
}

// ── Блок в fonts.conf ───────────────────────────────────────────────────

/// Путь пользовательского файла fontconfig от `$XDG_CONFIG_HOME`.
pub const FONTCONFIG_REL_PATH: &str = "fontconfig/fonts.conf";

const FC_BLOCK_BEGIN: &str = "<!-- >>> Rexon: font rendering (managed by Rexon, edits are overwritten) -->";
const FC_BLOCK_BEGIN_PREFIX: &str = "<!-- >>> Rexon: font rendering";
const FC_BLOCK_END: &str = "<!-- <<< Rexon: font rendering -->";

/// Почему Rexon не тронул `fonts.conf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontconfigRefusal {
    /// Файл не разбирается как XML fontconfig: дописать в него — значит
    /// угадывать, а ошибка в этом файле ломает шрифты всем программам.
    Malformed,
    /// Начало нашего блока есть, а конца нет — кто-то правил файл руками.
    BrokenBlock,
}

fn fc_single(name: &str, value_tag: &str, value: &str) -> String {
    format!(
        "  <match target=\"pattern\">\n    <edit name=\"{name}\" mode=\"assign\">\n      <{value_tag}>{value}</{value_tag}>\n    </edit>\n  </match>\n"
    )
}

fn fc_range(prop: &str, from: i32, to: i32) -> String {
    format!(
        "  <match target=\"pattern\">\n    <test qual=\"any\" name=\"{prop}\" compare=\"more_eq\">\n      <double>{from}</double>\n    </test>\n    <test qual=\"any\" name=\"{prop}\" compare=\"less_eq\">\n      <double>{to}</double>\n    </test>\n    <edit name=\"antialias\" mode=\"assign\">\n      <bool>false</bool>\n    </edit>\n  </match>\n"
    )
}

/// Наш блок для `fonts.conf`. `dpi` — принудительное DPI шрифтов (`None` —
/// не задано): от него же считается диапазон «не сглаживать» в пикселях.
pub fn fontconfig_block(r: &FontRendering, dpi: Option<i32>) -> String {
    let r = r.clone().normalized();
    let mut out = String::new();
    out.push_str("  ");
    out.push_str(FC_BLOCK_BEGIN);
    out.push('\n');
    out.push_str(&fc_single("antialias", "bool", bool_word(r.antialias)));
    out.push_str(&fc_single("hinting", "bool", bool_word(r.hinting != "none")));
    out.push_str(&fc_single("hintstyle", "const", &r.xft_hintstyle()));
    out.push_str(&fc_single("rgba", "const", &r.subpixel));
    if let Some((from, to)) = r.exclude.filter(|_| r.antialias) {
        let px_dpi = dpi.unwrap_or(FONT_BASE_DPI);
        out.push_str(&fc_range("size", from, to));
        out.push_str(&fc_range("pixelsize", point_to_pixel(from, px_dpi), point_to_pixel(to, px_dpi)));
    }
    if let Some(dpi) = dpi {
        out.push_str(&fc_single("dpi", "double", &dpi.to_string()));
    }
    out.push_str("  ");
    out.push_str(FC_BLOCK_END);
    out.push('\n');
    out
}

/// Узел XML: имя, атрибуты, текст, дети.
#[derive(Debug, Default)]
struct FcNode {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<FcNode>,
}

impl FcNode {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Разобрать `fonts.conf`. `None` — файл не XML, корень не `<fontconfig>`
/// либо не закрыт.
fn fc_parse(text: &str) -> Option<FcNode> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack: Vec<FcNode> = vec![FcNode::default()];
    let open = |e: &quick_xml::events::BytesStart| -> Option<FcNode> {
        let mut node = FcNode {
            tag: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
            ..Default::default()
        };
        for attr in e.attributes() {
            let attr = attr.ok()?;
            node.attrs.push((
                String::from_utf8_lossy(attr.key.as_ref()).into_owned(),
                attr.unescape_value().ok()?.into_owned(),
            ));
        }
        Some(node)
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(open(&e)?),
            Ok(Event::Empty(e)) => {
                let node = open(&e)?;
                stack.last_mut()?.children.push(node);
            }
            Ok(Event::End(_)) => {
                if stack.len() < 2 {
                    return None;
                }
                let node = stack.pop()?;
                stack.last_mut()?.children.push(node);
            }
            Ok(Event::Text(e)) => {
                let t = e.decode().ok()?;
                stack.last_mut()?.text.push_str(&t);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    if stack.len() != 1 {
        return None;
    }
    let mut doc = stack.pop()?;
    let roots: Vec<FcNode> = doc.children.drain(..).collect();
    let mut roots = roots.into_iter();
    let root = roots.next()?;
    if root.tag != "fontconfig" || roots.next().is_some() {
        return None;
    }
    Some(root)
}

/// Значения отрисовки, заданные в `fonts.conf`: смотрим верхние `<match>`
/// той же формы, в какой их пишем мы и KDE; при повторе берётся последнее —
/// fontconfig тоже применяет их по порядку.
pub fn fontconfig_rendering(text: &str) -> RenderingParts {
    let mut out = RenderingParts::default();
    let Some(root) = fc_parse(text) else { return out };
    for m in root.children.iter().filter(|n| n.tag == "match") {
        if !matches!(m.attr("target"), Some("pattern") | Some("font") | None) {
            continue;
        }
        let tests: Vec<&FcNode> = m.children.iter().filter(|n| n.tag == "test").collect();
        let edits: Vec<&FcNode> = m.children.iter().filter(|n| n.tag == "edit").collect();
        if edits.len() != 1 || m.children.len() != tests.len() + 1 {
            continue;
        }
        let edit = edits[0];
        if !matches!(edit.attr("mode"), Some("assign") | Some("append")) {
            continue;
        }
        let Some(value) = edit.children.first().map(|v| v.text.trim().to_string()) else {
            continue;
        };
        let name = edit.attr("name").unwrap_or("");
        if tests.is_empty() {
            match name {
                "antialias" => out.antialias = Some(value != "false"),
                "rgba" if SUBPIXEL_ORDERS.contains(&value.as_str()) => out.subpixel = Some(value),
                "hintstyle" => {
                    let style = value.strip_prefix("hint").unwrap_or(&value).to_string();
                    if HINT_STYLES.contains(&style.as_str()) {
                        out.hinting = Some(style);
                    }
                }
                "dpi" => out.dpi = value.parse::<f64>().ok().map(|v| v.round() as i32).filter(|v| *v > 0),
                _ => {}
            }
        } else if tests.len() == 2 && name == "antialias" && value == "false" {
            let mut from = None;
            let mut to = None;
            for t in &tests {
                if t.attr("name") != Some("size") {
                    continue;
                }
                let v = t.children.first().and_then(|v| v.text.trim().parse::<f64>().ok());
                match t.attr("compare") {
                    Some("more_eq") | Some("more") => from = v,
                    Some("less_eq") | Some("less") => to = v,
                    _ => {}
                }
            }
            if let (Some(a), Some(b)) = (from, to) {
                let (a, b) = (a.round() as i32, b.round() as i32);
                out.exclude = Some((a.min(b), a.max(b)));
            }
        }
    }
    out
}

/// Текст `fonts.conf` с нашим блоком: чужие записи остаются, где были, наш
/// прежний блок снимается, новый встаёт перед `</fontconfig>` — последним,
/// чтобы именно он и действовал. `existing = None` — файла нет.
pub fn fontconfig_with_block(existing: Option<&str>, block: &str) -> Result<String, FontconfigRefusal> {
    fc_with_managed_block(existing, block, FC_BLOCK_BEGIN_PREFIX, FC_BLOCK_END)
}

/// Общий ход для всех наших блоков `fonts.conf` (отрисовка, выключенные
/// шрифты): блок с началом `begin_prefix` и концом `block_end` снимается и, если
/// `block` не пуст, ставится заново перед `</fontconfig>`.
fn fc_with_managed_block(
    existing: Option<&str>,
    block: &str,
    begin_prefix: &str,
    block_end: &str,
) -> Result<String, FontconfigRefusal> {
    let text = existing.unwrap_or("");
    if text.trim().is_empty() {
        return Ok(format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n{block}</fontconfig>\n"
        ));
    }
    if fc_parse(text).is_none() {
        return Err(FontconfigRefusal::Malformed);
    }
    let mut body = text.to_string();
    if let Some(begin) = body.find(begin_prefix) {
        let Some(end_rel) = body[begin..].find(block_end) else {
            return Err(FontconfigRefusal::BrokenBlock);
        };
        let line_start = body[..begin].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let start = if body[line_start..begin].trim().is_empty() { line_start } else { begin };
        let mut end = begin + end_rel + block_end.len();
        if body[end..].starts_with('\n') {
            end += 1;
        }
        body.replace_range(start..end, "");
    }
    let new_text = if let Some(close) = body.rfind("</fontconfig>") {
        let line_start = body[..close].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let (at, lead) = if body[line_start..close].trim().is_empty() {
            (line_start, "")
        } else {
            (close, "\n")
        };
        format!("{}{lead}{block}{}", &body[..at], &body[at..].trim_start_matches([' ', '\t']))
    } else if let Some(pos) = body.find("<fontconfig/>").or_else(|| body.find("<fontconfig />")) {
        let len = if body[pos..].starts_with("<fontconfig/>") { 13 } else { 14 };
        format!("{}<fontconfig>\n{block}</fontconfig>{}", &body[..pos], &body[pos + len..])
    } else {
        return Err(FontconfigRefusal::Malformed);
    };
    if fc_parse(&new_text).is_none() {
        return Err(FontconfigRefusal::Malformed);
    }
    Ok(new_text)
}

/// `true`/`false` — так булево значение пишут и `kdeglobals`, и fontconfig.
fn bool_word(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// Варианты субпиксельной отрисовки для списка: `(значение, подпись)`.
pub fn subpixel_choices() -> Vec<(&'static str, String)> {
    SUBPIXEL_ORDERS
        .iter()
        .map(|id| {
            let label = match *id {
                "none" => trc("sub-pixel rendering", "None"),
                "rgb" => tr("RGB"),
                "bgr" => tr("BGR"),
                "vrgb" => tr("Vertical RGB"),
                _ => tr("Vertical BGR"),
            };
            (*id, label)
        })
        .collect()
}

/// Варианты хинтинга для списка: `(значение, подпись)`.
pub fn hinting_choices() -> Vec<(&'static str, String)> {
    HINT_STYLES
        .iter()
        .map(|id| {
            let label = match *id {
                "none" => trc("font hinting", "None"),
                "slight" => trc("font hinting", "Slight"),
                "medium" => trc("font hinting", "Medium"),
                _ => trc("font hinting", "Full"),
            };
            (*id, label)
        })
        .collect()
}

/// Чем кончилась запись отрисовки шрифтов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderingOutcome {
    /// Записано и в `fonts.conf`, и в родные ключи окружения.
    Saved,
    /// `fonts.conf` не тронут: он не разбирается ([`FontconfigRefusal`]).
    /// В строке — путь файла.
    FontconfigRefused(String),
    /// `fonts.conf` не записался (нет прав, нет места).
    FontconfigFailed(String),
    /// Родные ключи окружения не записались.
    NativeFailed,
}

/// Итог записи отрисовки шрифтов — для человека.
pub fn rendering_status(outcome: &RenderingOutcome) -> String {
    match outcome {
        RenderingOutcome::Saved => t("font.render_saved"),
        RenderingOutcome::FontconfigRefused(path) => tf("font.fc_refused", &[path]),
        RenderingOutcome::FontconfigFailed(path) => tf("font.fc_failed", &[path]),
        RenderingOutcome::NativeFailed => t("font.render_native_failed"),
    }
}

// ============================================================================
// «Изменить все шрифты» и принудительное DPI шрифтов
// ============================================================================

/// Что меняет «Изменить все шрифты»: семейство и кегль, только семейство,
/// только кегль (у KDE то же выбирается галочками в окне выбора шрифта).
pub const CHANGE_ALL_MODES: &[&str] = &["both", "family", "size"];

/// Варианты «что менять» для списка: `(значение, подпись)`.
pub fn change_all_mode_choices() -> Vec<(&'static str, String)> {
    CHANGE_ALL_MODES
        .iter()
        .map(|id| {
            let label = match *id {
                "family" => tr("Family only"),
                "size" => tr("Size only"),
                _ => tr("Family and size"),
            };
            (*id, label)
        })
        .collect()
}

/// Подпись кнопки «Изменить все шрифты», подсказка и заголовок окна выбора.
pub fn change_all_texts() -> (String, String, String) {
    (
        tr("🔤 Change All Fonts…"),
        tr("Set one family and size for every font at once"),
        tr("All Fonts"),
    )
}

/// Один шрифт в «изменить все»: цель, нынешние семейство и кегль.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontNow {
    pub target: String,
    pub family: String,
    pub size: i32,
}

/// Что записать каждому шрифту по «Изменить все шрифты».
///
/// Правила — те же, что у страницы «Шрифты» KDE: маленький шрифт выходит на
/// 2 пт меньше выбранного (если выбранный не меньше 9 пт — иначе мелкий
/// текст стал бы нечитаемым), моноширинный не получает семейство, которое
/// само не моноширинное (`family_is_mono`), — у него меняется только кегль.
/// Второе значение — оставлен ли моноширинному прежний шрифт.
pub fn change_all_fonts(now: &[FontNow], family: &str, size: i32, mode: &str, family_is_mono: bool) -> (Vec<FontNow>, bool) {
    let set_family = mode != "size";
    let set_size = mode != "family";
    let mut mono_kept = false;
    let out = now
        .iter()
        .filter_map(|cur| {
            let mut fam = if set_family { family.to_string() } else { cur.family.clone() };
            if cur.target == "mono" && set_family && !family_is_mono {
                // Моноширинный остаётся своим; если своего нет — не трогаем
                // его вовсе, чтобы не записать туда обычный шрифт.
                if cur.family.is_empty() {
                    return None;
                }
                fam = cur.family.clone();
                mono_kept = true;
            }
            if fam.is_empty() {
                // Только кегль, а семейства у шрифта нет (не задан) — берём
                // шрифт интерфейса, как и само окружение.
                fam = now
                    .iter()
                    .find(|n| n.target == "system")
                    .map(|n| n.family.clone())
                    .filter(|f| !f.is_empty())?;
            }
            let base = if cur.size > 0 { cur.size } else { 10 };
            let sz = if !set_size {
                base
            } else if cur.target == "small" && size >= 9 {
                size - 2
            } else {
                size
            };
            Some(FontNow {
                target: cur.target.clone(),
                family: fam,
                size: sz,
            })
        })
        .collect();
    (out, mono_kept)
}

/// argv проверки «моноширинное ли семейство»: `fc-list` отдаёт `spacing`
/// каждого начертания семейства (100 — моноширинный, 90 — двойной ширины).
pub fn fc_spacing_argv(family: &str) -> Vec<String> {
    vec![
        "fc-list".to_string(),
        format!(":family={}", family.replace('\\', "\\\\").replace(':', "\\:").replace('-', "\\-")),
        "spacing".to_string(),
    ]
}

/// Моноширинное ли семейство по выводу [`fc_spacing_argv`]: все начертания —
/// `spacing=100` или `90`. Пустой ответ (семейство неизвестно) — нет.
pub fn parse_fc_spacing(out: &str) -> bool {
    let values: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_start_matches(':').trim().trim_start_matches("spacing=").trim())
        .collect();
    !values.is_empty() && values.iter().all(|v| *v == "100" || *v == "90")
}

/// Итог «Изменить все шрифты».
pub fn change_all_status(failed: &[String], family: &str, size: i32, mode: &str, mono_kept: Option<&str>) -> String {
    if !failed.is_empty() {
        // TRANSLATORS: %1 is a comma-separated list of font kinds (“Menu Font, Small Font”).
        return trf("⚠️ All fonts: could not apply %1", &[&failed.join(", ")]);
    }
    let what = match mode {
        "family" => family.to_string(),
        "size" => trf("%1 pt", &[&size]),
        _ => trf("%1 %2 pt", &[&family, &size]),
    };
    // TRANSLATORS: %1 is the chosen font, e.g. “Noto Sans 11 pt”.
    let mut text = trf("✅ All fonts: %1", &[&what]);
    if let Some(kept) = mono_kept {
        text.push_str(". ");
        // TRANSLATORS: %1 is the monospace font family that was kept.
        text.push_str(&trf("The monospace font stays %1: the chosen one isn't monospaced", &[&kept]));
    }
    text
}

/// Границы принудительного DPI и значение, с которого начинается поле.
pub const FONT_DPI_RANGE: (i32, i32) = (48, 480);

/// Где окружение держит DPI шрифтов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontDpiStore {
    /// `kcmfonts [General] forceFontDPI` (`0` — не задано). Plasma 6.7 и
    /// раньше переносит его в `Xft.dpi` при входе в сеанс X11; в Wayland
    /// DPI шрифтов у Plasma нет — шрифты следуют масштабу экрана.
    Kde,
    /// XFCE `xsettings /Xft/DPI` (`-1` — считать по экрану).
    Xfce,
    /// MATE `org.mate.font-rendering dpi` (`0` — по экрану).
    Mate,
    /// Своего DPI шрифтов у окружения нет, есть множитель размера текста
    /// (`text-scaling-factor`) — тот самый, из которого служба настроек
    /// GNOME и считает отдаваемый GTK `Xft/DPI` (96 × множитель). Значит,
    /// DPI N — это множитель N/96.
    TextScaling { schema: String },
    /// DPI в своём блоке `fonts.conf` — так его хранит страница «Шрифты»
    /// LXQt.
    Fontconfig,
    /// `Xft.dpi` в своём блоке `~/.Xresources` — сеанс X11 без службы
    /// настроек, которая раздавала бы DPI сама (i3, Openbox и прочие
    /// оконные менеджеры без окружения). Этот файл при входе вливают в
    /// базу ресурсов X сценарии сеанса (`startx`/`xinitrc`, Xsession
    /// SDDM, LightDM, GDM), а `Xft.dpi` из неё берут GTK (если нет
    /// XSETTINGS), Qt и программы на Xft.
    Xresources,
}

/// Хранилище DPI шрифтов окружения `de`. `None` — задать нечем (Plasma в
/// сеансе Wayland).
pub fn font_dpi_store(de: &str, wayland: bool) -> Option<FontDpiStore> {
    let scaling = |schema: &str| Some(FontDpiStore::TextScaling { schema: schema.to_string() });
    match de {
        "kde" if wayland => None,
        "kde" => Some(FontDpiStore::Kde),
        "xfce" => Some(FontDpiStore::Xfce),
        "mate" => Some(FontDpiStore::Mate),
        "cinnamon" | "cinnamon-mint" => scaling("org.cinnamon.desktop.interface"),
        "lxqt" => Some(FontDpiStore::Fontconfig),
        // Deepin вне Wayland — сеанс X11, его Rexon не поддерживает
        // ([`font_dpi_note_for`] так и говорит).
        "deepin" if !wayland => None,
        // Окружения со своей службой настроек (она и раздаёт Xft/DPI в
        // сеансе X11) и сеансы Wayland: множитель текста GNOME.
        "gnome" | "budgie" | "pantheon" | "deepin" | "cosmic" => scaling("org.gnome.desktop.interface"),
        _ if !wayland => Some(FontDpiStore::Xresources),
        _ => scaling("org.gnome.desktop.interface"),
    }
}

/// Дописывать ли DPI ещё и в `~/.Xresources` поверх родного хранилища:
/// LXQt в сеансе X11 держит DPI только в `fonts.conf`, а `Xft.dpi` в базу
/// ресурсов X не кладёт — Qt и Xft-программы его без этого не видят.
pub fn font_dpi_xresources_also(de: &str, wayland: bool) -> bool {
    de == "lxqt" && !wayland
}

/// Файл ресурсов X пользователя (от домашнего каталога).
pub const XRESOURCES_REL_PATH: &str = ".Xresources";

const XR_BLOCK_BEGIN: &str = "! >>> Rexon: font DPI (managed by Rexon, edits are overwritten)";
const XR_BLOCK_BEGIN_PREFIX: &str = "! >>> Rexon: font DPI";
const XR_BLOCK_END: &str = "! <<< Rexon: font DPI";

/// Строка ресурса DPI шрифтов.
pub fn xft_dpi_line(dpi: i32) -> String {
    format!("Xft.dpi: {dpi}\n")
}

/// Текст `~/.Xresources` с нашим блоком `Xft.dpi` (`dpi = None` — блок
/// снимается). Чужие строки остаются, наш блок — в конце (у xrdb из двух
/// одинаковых ресурсов действует последний). `None` — начало блока есть, а
/// конца нет (правили руками).
pub fn xresources_with_dpi(existing: Option<&str>, dpi: Option<i32>) -> Option<String> {
    let mut body = existing.unwrap_or_default().to_string();
    if let Some(begin) = body.find(XR_BLOCK_BEGIN_PREFIX) {
        let end_rel = body[begin..].find(XR_BLOCK_END)?;
        let mut end = begin + end_rel + XR_BLOCK_END.len();
        if body[end..].starts_with('\n') {
            end += 1;
        }
        body.replace_range(begin..end, "");
    }
    let body = body.trim_end();
    let Some(dpi) = dpi else {
        return Some(if body.is_empty() { String::new() } else { format!("{body}\n") });
    };
    let block = format!("{XR_BLOCK_BEGIN}\n{}{XR_BLOCK_END}\n", xft_dpi_line(dpi));
    Some(if body.is_empty() { block } else { format!("{body}\n\n{block}") })
}

/// `Xft.dpi` из текста `~/.Xresources`: последнее значение (так решает и
/// xrdb); `None` — не задано.
pub fn xresources_dpi(text: &str) -> Option<i32> {
    let mut out = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('!') || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        let name = name.trim();
        if name == "Xft.dpi" || name == "Xft*dpi" || name == "*dpi" {
            out = value.trim().parse::<f64>().ok().map(|v| v.round() as i32).filter(|v| *v > 0);
        }
    }
    out
}

/// argv вливания файла ресурсов в живую базу X (без препроцессора: в нашем
/// файле одна строка).
pub fn xrdb_merge_argv(path: &str) -> Vec<String> {
    vec!["xrdb".to_string(), "-nocpp".to_string(), "-merge".to_string(), path.to_string()]
}

fn dpi_backend(store: &FontDpiStore) -> Option<SoundBackend> {
    Some(match store {
        FontDpiStore::Kde => SoundBackend::KConfig {
            file: "kcmfonts".to_string(),
            group: "General".to_string(),
            key: "forceFontDPI".to_string(),
        },
        FontDpiStore::Xfce => SoundBackend::Xfconf {
            channel: "xsettings".to_string(),
            prop: "/Xft/DPI".to_string(),
        },
        FontDpiStore::Mate => SoundBackend::GSettings {
            schema: "org.mate.font-rendering".to_string(),
            key: "dpi".to_string(),
        },
        FontDpiStore::TextScaling { schema } => SoundBackend::GSettings {
            schema: schema.clone(),
            key: "text-scaling-factor".to_string(),
        },
        FontDpiStore::Fontconfig | FontDpiStore::Xresources => return None,
    })
}

/// Множитель размера текста для DPI `dpi` (в пределах схемы 0.5–3.0).
pub fn text_scaling_for_dpi(dpi: i32) -> f64 {
    ((dpi as f64 / FONT_BASE_DPI as f64) * 100.0).round().clamp(50.0, 300.0) / 100.0
}

/// Значение DPI в записи хранилища; `None` — снять (вернуть «по экрану»).
pub fn font_dpi_value(store: &FontDpiStore, dpi: Option<i32>) -> String {
    match (store, dpi) {
        (FontDpiStore::Kde, Some(d)) | (FontDpiStore::Xfce, Some(d)) => d.to_string(),
        (FontDpiStore::Kde, None) => "0".to_string(),
        (FontDpiStore::Xfce, None) => "-1".to_string(),
        (FontDpiStore::Mate, Some(d)) => format!("{d}.0"),
        (FontDpiStore::Mate, None) => "0.0".to_string(),
        (FontDpiStore::TextScaling { .. }, Some(d)) => format!("{:.2}", text_scaling_for_dpi(d)),
        (FontDpiStore::TextScaling { .. }, None) => "1.0".to_string(),
        (FontDpiStore::Fontconfig, d) | (FontDpiStore::Xresources, d) => {
            d.map(|d| d.to_string()).unwrap_or_default()
        }
    }
}

/// argv записи DPI; для `fonts.conf` — `None` (это файл).
pub fn font_dpi_write_argv(store: &FontDpiStore, dpi: Option<i32>, kwrite: &str) -> Option<Vec<String>> {
    sound_write_argv(&dpi_backend(store)?, &font_dpi_value(store, dpi), kwrite)
}

/// argv чтения DPI; для `fonts.conf` — `None`.
pub fn font_dpi_read_argv(store: &FontDpiStore, kread: &str) -> Option<Vec<String>> {
    sound_read_argv(&dpi_backend(store)?, kread)
}

/// Заданное DPI из ответа хранилища; `None` — не задано (по экрану).
pub fn parse_font_dpi(store: &FontDpiStore, raw: &str) -> Option<i32> {
    let v: f64 = clean_answer(raw).parse().ok()?;
    match store {
        FontDpiStore::TextScaling { .. } => {
            if (v - 1.0).abs() < 0.005 || v <= 0.0 {
                None
            } else {
                Some((v * FONT_BASE_DPI as f64).round() as i32)
            }
        }
        _ => (v > 0.0).then(|| v.round() as i32),
    }
}

/// Пояснение под полем DPI — что именно и когда подействует.
pub fn font_dpi_note(store: Option<&FontDpiStore>) -> String {
    match store {
        None => tr("In the Plasma Wayland session fonts follow the display scale: change it in System \
                    Settings → Display"),
        Some(FontDpiStore::Kde) => tr("Takes effect after you log in again"),
        Some(FontDpiStore::TextScaling { .. }) => tr("This desktop has no separate font DPI, so Rexon \
                    sets its text scaling factor instead (96 DPI = 100%)"),
        Some(FontDpiStore::Fontconfig) => tr("Written to the shared font settings file, as LXQt itself \
                    does; apps that are already open pick it up after a restart"),
        Some(FontDpiStore::Xresources) => tr("This session has no settings service of its own, so Rexon \
                    writes Xft.dpi to ~/.Xresources: apps started from now on pick it up, all of them after you \
                    log in again"),
        Some(FontDpiStore::Xfce) | Some(FontDpiStore::Mate) => String::new(),
    }
}

/// [`font_dpi_note`] с поправкой на окружение: у Deepin хранилища нет вне
/// Wayland — это сеанс X11, и подпись говорит, что Rexon его не поддерживает.
pub fn font_dpi_note_for(de: &str, store: Option<&FontDpiStore>) -> String {
    match (de, store) {
        ("deepin", None) => crate::dde::not_treeland_message(),
        _ => font_dpi_note(store),
    }
}

/// Итог смены DPI шрифтов.
pub fn font_dpi_status(ok: bool, dpi: Option<i32>) -> String {
    match (ok, dpi) {
        (false, _) => tr("⚠️ Could not change the font DPI"),
        // TRANSLATORS: %1 is a number of dots per inch, e.g. 120.
        (true, Some(d)) => trf("✅ Font DPI: %1", &[&d]),
        (true, None) => tr("✅ Font DPI follows the screen again"),
    }
}

// ============================================================================
// Снятие слоёв шрифтов Rexon (сброс шрифтов на стандартные)
// ============================================================================
//
// Кроме родных ключей окружений, страница «Шрифты» кладёт свои слои в чужие
// файлы — каждый в отмеченном блоке или в файле, который целиком наш:
//
// * `gtk-3.0/gtk.css`, `gtk-4.0/gtk.css` — блок шрифтов меню, панелей
//   инструментов и мелких подписей (на месте ссылки на тему — файл с
//   `@import` той же темы и нашим блоком);
// * `qt6ct/qss/rexon-fonts.qss`, `qt5ct/qss/rexon-fonts.qss` — наша
//   таблица стилей и её строка в `[Interface] stylesheets`;
// * `fontconfig/fonts.conf` — блок сглаживания, хинтинга и DPI;
// * `~/.Xresources` — блок `Xft.dpi`;
// * COSMIC `com.system76.CosmicTk/v1/interface_font` и `monospace_font` —
//   файл целиком, с нашей меткой первой строкой.
//
// Сброс шрифтов снимает ровно эти блоки: чужое содержимое файлов остаётся
// как было. Файл, в котором после снятия ничего не осталось, удаляется;
// `gtk.css`, в котором осталась лишь наша ссылка на тему, снова становится
// символьной ссылкой на неё. Разорванный руками блок (начало без конца) не
// трогается — угадывать, где кончается наше, нельзя.

/// Метка первой строкой в RON-файлах шрифтов COSMIC, записанных Rexon. RON
/// разрешает комментарии, `cosmic-config` их пропускает; когда файл
/// перепишут «Параметры» COSMIC, метка уйдёт вместе с нашим значением.
pub const COSMIC_FONT_MARK: &str = "// Rexon: font (managed by Rexon)";

/// Ключи шрифтов COSMIC, которые пишет Rexon.
pub const COSMIC_FONT_KEYS: &[&str] = &["interface_font", "monospace_font"];

/// Что сделать с одним файлом при сбросе шрифтов. Пути — полные.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontLayerUndo {
    /// Переписать файл этим текстом (наш блок снят, чужое осталось).
    Write { path: String, text: String },
    /// Удалить файл: в нём не осталось ничего, кроме нашего.
    Remove { path: String },
    /// Вернуть на место файла символьную ссылку на тему (`target`).
    Relink { path: String, target: String },
}

/// Чем план сброса смотрит на файлы: текст по полному пути и то, ссылка ли
/// это (сквозь ссылку на чужую тему мы не пишем — значит, и снимать в ней
/// нечего).
pub trait FontLayerFs {
    fn read(&self, path: &str) -> Option<String>;
    fn is_link(&self, path: &str) -> bool;
}

/// Текст таблицы стилей без нашего блока шрифтов. `None` — блока нет или он
/// разорван руками.
pub fn text_without_extra_fonts_block(text: &str) -> Option<String> {
    let begin = text.find(EXTRA_BLOCK_BEGIN_PREFIX)?;
    let end_rel = text[begin..].find(EXTRA_BLOCK_END)?;
    let mut end = begin + end_rel + EXTRA_BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    let head = text[..begin].trim_end();
    let tail = text[end..].trim_start_matches('\n');
    let mut out = head.to_string();
    if !out.is_empty() && !tail.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(tail);
    let out = out.trim_end();
    Some(if out.is_empty() { String::new() } else { format!("{out}\n") })
}

/// Тема, на которую указывала ссылка `gtk.css`, если от файла осталась
/// ровно наша строка `@import url("file://…");` (её Rexon ставит на место
/// ссылки, см. [`FontFile::file_writes`]).
pub fn gtk_css_import_only(text: &str) -> Option<String> {
    let url = text
        .trim()
        .strip_prefix("@import url(\"file://")?
        .strip_suffix("\");")?;
    if url.contains(['"', '\n', ')']) {
        return None;
    }
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().filter(|p| p.starts_with('/'))
}

/// Текст `qt6ct.conf` без нашей таблицы в `[Interface] stylesheets`. `None`
/// — её там нет. Узнаётся по имени и месту (`<tool>/qss/rexon-fonts.qss`),
/// а не по всему пути: каталог настроек мог переехать.
pub fn qtct_conf_without_stylesheet(conf: &str, tool: &str) -> Option<String> {
    let ours = format!("/{tool}/qss/{QTCT_QSS_NAME}");
    let list = qsettings_list(&ini_value(conf, "Interface", "stylesheets").unwrap_or_default());
    let kept: Vec<String> = list.iter().filter(|p| !p.ends_with(&ours)).cloned().collect();
    if kept.len() == list.len() {
        return None;
    }
    // Пустой список QSettings записывает так же, как сам qt6ct.
    let value = if kept.is_empty() { "@Invalid()".to_string() } else { qsettings_list_value(&kept) };
    Some(crate::gtk_sanitizer::write_ini_setting_in(Some(conf), "Interface", "stylesheets", &value))
}

/// Текст `fonts.conf` без нашего блока. `None` — блока нет или он разорван.
pub fn fontconfig_without_block(text: &str) -> Option<String> {
    let begin = text.find(FC_BLOCK_BEGIN_PREFIX)?;
    let end_rel = text[begin..].find(FC_BLOCK_END)?;
    let line_start = text[..begin].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let start = if text[line_start..begin].trim().is_empty() { line_start } else { begin };
    let mut end = begin + end_rel + FC_BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    Some(format!("{}{}", &text[..start], &text[end..]))
}

/// Пуст ли `fonts.conf`: кроме объявления XML, DOCTYPE и пустого корня
/// `<fontconfig>` в нём ничего (даже комментария) нет — такой файл заводит
/// сам Rexon, когда пишет первый блок.
pub fn fontconfig_is_bare(text: &str) -> bool {
    let mut rest = text.trim();
    if let Some(r) = rest.strip_prefix("<?xml") {
        match r.find("?>") {
            Some(i) => rest = r[i + 2..].trim_start(),
            None => return false,
        }
    }
    if let Some(r) = rest.strip_prefix("<!DOCTYPE") {
        match r.find('>') {
            Some(i) => rest = r[i + 1..].trim_start(),
            None => return false,
        }
    }
    if rest == "<fontconfig/>" || rest == "<fontconfig />" {
        return true;
    }
    rest.strip_prefix("<fontconfig>")
        .and_then(|r| r.trim().strip_suffix("</fontconfig>"))
        .is_some_and(|inner| inner.trim().is_empty())
}

/// Записан ли RON-файл шрифта COSMIC самим Rexon (по метке).
pub fn cosmic_font_is_ours(text: &str) -> bool {
    text.trim_start().starts_with(COSMIC_FONT_MARK)
}

/// План сброса шрифтов: какие файлы и как вернуть, чтобы от слоёв Rexon
/// ничего не осталось. Файлы без нашего, ссылки и разорванные руками блоки
/// в план не попадают.
pub fn font_layers_reset_plan(config_home: &str, home: &str, fs: &dyn FontLayerFs) -> Vec<FontLayerUndo> {
    let base = config_home.trim_end_matches('/');
    let mut plan = Vec::new();
    // Снять наш блок; файл без остатка — удалить.
    let mut strip = |path: String, without: &dyn Fn(&str) -> Option<String>, bare: &dyn Fn(&str) -> bool| {
        if fs.is_link(&path) {
            return;
        }
        let Some(text) = fs.read(&path) else { return };
        let Some(rest) = without(&text) else { return };
        plan.push(if bare(&rest) {
            FontLayerUndo::Remove { path }
        } else {
            FontLayerUndo::Write { path, text: rest }
        });
    };

    for version in ["3.0", "4.0"] {
        strip(format!("{base}/gtk-{version}/gtk.css"), &text_without_extra_fonts_block, &|r| r.trim().is_empty());
    }
    strip(format!("{base}/{FONTCONFIG_REL_PATH}"), &fontconfig_without_block, &fontconfig_is_bare);
    strip(
        format!("{}/{XRESOURCES_REL_PATH}", home.trim_end_matches('/')),
        &|t| t.contains(XR_BLOCK_BEGIN_PREFIX).then(|| xresources_with_dpi(Some(t), None)).flatten(),
        &|r| r.trim().is_empty(),
    );

    // gtk.css, от которого осталась лишь наша ссылка на тему, — обратно в
    // символьную ссылку (если темы уже нет — просто убрать).
    for undo in plan.iter_mut() {
        let FontLayerUndo::Write { path, text } = undo else { continue };
        if !path.ends_with("/gtk.css") {
            continue;
        }
        if let Some(target) = gtk_css_import_only(text) {
            *undo = if fs.read(&target).is_some() {
                FontLayerUndo::Relink { path: path.clone(), target }
            } else {
                FontLayerUndo::Remove { path: path.clone() }
            };
        }
    }

    for tool in QTCT_TOOLS {
        let qss = format!("{base}/{tool}/qss/{QTCT_QSS_NAME}");
        if !fs.is_link(&qss) && fs.read(&qss).is_some_and(|t| t.contains(EXTRA_BLOCK_BEGIN_PREFIX)) {
            plan.push(FontLayerUndo::Remove { path: qss });
        }
        let conf = format!("{base}/{tool}/{tool}.conf");
        if fs.is_link(&conf) {
            continue;
        }
        if let Some(text) = fs.read(&conf).and_then(|t| qtct_conf_without_stylesheet(&t, tool)) {
            plan.push(FontLayerUndo::Write { path: conf, text });
        }
    }

    for key in COSMIC_FONT_KEYS {
        let path = format!("{base}/{}", FontFile::CosmicTk { key: key.to_string() }.rel_path());
        if !fs.is_link(&path) && fs.read(&path).is_some_and(|t| cosmic_font_is_ours(&t)) {
            plan.push(FontLayerUndo::Remove { path });
        }
    }
    plan
}

// ============================================================================
// Управление шрифтами: установленные шрифты, установка из файла, удаление
// ============================================================================
//
// Эталон — страница «Управление шрифтами» KDE: все установленные шрифты по
// семействам, образец каждого начертания, поиск, установка своих и
// удаление своих; системные — только для просмотра (их ставит пакетный
// менеджер, и удалять их из окна настроек — значит ломать пакеты).
//
// Источник списка — сам fontconfig (`fc-list`): ровно те шрифты, которые
// видят программы. Свои шрифты живут в каталогах, которые fontconfig
// сканирует для пользователя: `$XDG_DATA_HOME/fonts` (туда же Rexon и
// ставит) и устаревший `~/.fonts`. После любого изменения кэш каталога
// пересобирается `fc-cache -f <каталог>`.

/// Расширения файлов шрифтов, которые предлагает окно выбора. Что из них
/// понимает данная машина, решает FreeType, с которой собран fontconfig
/// (WOFF — с zlib, WOFF2 — с brotli), поэтому перед установкой каждый файл
/// проверяется [`fc_query_families_argv`], а не верится расширению.
pub const FONT_INSTALL_EXTS: &[&str] =
    &["ttf", "otf", "ttc", "otc", "woff", "woff2", "pfb", "pfa", "pcf", "pcf.gz", "bdf"];

/// Каталог шрифтов «для всех пользователей» — тот же, куда ставит
/// «Управление шрифтами» KDE в группу «Системные». Не `/usr/share/fonts`:
/// там файлы пакетов, и менеджер пакетов о наших ничего бы не знал. Этот
/// каталог fontconfig сканирует по умолчанию (`fonts.conf` дистрибутивов).
/// Ставит и удаляет здесь только помощник от root
/// ([`crate::privilege::op::PrivOp::FontInstall`] / `FontRemove`), сюда же
/// смотрит его проверка ([`crate::privilege::validate::check_font_remove`]).
pub const SHARED_FONT_DIR: &str = "/usr/local/share/fonts";

/// Один установленный шрифт (начертание в файле).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFont {
    pub family: String,
    pub style: String,
    pub file: String,
    /// Лежит в каталоге шрифтов пользователя — можно удалить.
    pub user: bool,
    /// Лежит в [`SHARED_FONT_DIR`] (поставлен «для всех пользователей»):
    /// удаляется с паролем администратора. Шрифты пакетов — ни `user`, ни
    /// `shared`: их снимает только менеджер пакетов.
    pub shared: bool,
    /// Выключен Rexon (`<rejectfont>` в нашем блоке `fonts.conf`): файл на
    /// месте, но программы его не видят.
    pub disabled: bool,
    /// Производитель (`foundry` fontconfig) после [`font_foundry_norm`].
    pub foundry: String,
    /// Письменности начертания — значения [`FONT_SCRIPTS`] (без «other»:
    /// её даёт отсутствие остальных, см. [`font_scripts_of`]).
    pub scripts: Vec<&'static str>,
}

/// Семейство в списке «Управления шрифтами».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamilyGroup {
    pub family: String,
    pub styles: Vec<InstalledFont>,
}

impl FontFamilyGroup {
    /// Есть ли у семейства начертания, которые можно удалить.
    pub fn has_user(&self) -> bool {
        self.styles.iter().any(|f| f.user)
    }

    /// Есть ли системные начертания.
    pub fn has_system(&self) -> bool {
        self.styles.iter().any(|f| !f.user)
    }

    /// Есть ли начертания, поставленные для всех пользователей
    /// ([`SHARED_FONT_DIR`]).
    pub fn has_shared(&self) -> bool {
        self.styles.iter().any(|f| f.shared)
    }

    /// Есть ли начертания из пакетов системы (не свои и не «для всех»).
    pub fn has_packaged(&self) -> bool {
        self.styles.iter().any(|f| !f.user && !f.shared)
    }

    /// Выключены ли все начертания семейства.
    pub fn all_disabled(&self) -> bool {
        !self.styles.is_empty() && self.styles.iter().all(|f| f.disabled)
    }

    /// Выключено ли хоть одно начертание.
    pub fn any_disabled(&self) -> bool {
        self.styles.iter().any(|f| f.disabled)
    }
}

/// Каталоги шрифтов пользователя, которые сканирует fontconfig.
pub fn font_user_dirs(home: &str, data_home: &str) -> Vec<String> {
    vec![
        format!("{}/fonts", data_home.trim_end_matches('/')),
        format!("{}/.fonts", home.trim_end_matches('/')),
    ]
}

/// Каталог, куда Rexon ставит шрифты.
pub fn font_install_dir(data_home: &str) -> String {
    format!("{}/fonts", data_home.trim_end_matches('/'))
}

/// Лежит ли файл в одном из каталогов шрифтов пользователя (`..` в пути —
/// нет: такой путь мог бы выйти за каталог).
pub fn font_file_is_user(file: &str, user_dirs: &[String]) -> bool {
    if file.split('/').any(|part| part == "..") {
        return false;
    }
    user_dirs
        .iter()
        .map(|d| d.trim_end_matches('/'))
        .filter(|d| !d.is_empty())
        .any(|d| file.strip_prefix(d).is_some_and(|rest| rest.starts_with('/') && rest.len() > 1))
}

/// Лежит ли файл в каталоге шрифтов «для всех пользователей»
/// ([`SHARED_FONT_DIR`]). Окну это подсказка (кнопка «Удалить»); решает
/// проверка помощника от root по настоящему пути.
pub fn font_file_is_shared(file: &str) -> bool {
    font_file_is_user(file, &[SHARED_FONT_DIR.to_string()])
}

/// Строка формата `fc-list`/`fc-query` для списка шрифтов: семейство,
/// начертание, файл, производитель и языки (через `|`). Файл — третьим, как
/// было до фильтров: старый трёхполевой вывод разбирается так же.
pub const FC_FONT_FORMAT: &str = "%{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n";

/// argv списка установленных шрифтов ([`FC_FONT_FORMAT`]).
pub fn fc_list_fonts_argv() -> Vec<String> {
    vec!["fc-list".to_string(), "--format".to_string(), FC_FONT_FORMAT.to_string()]
}

/// argv того же списка для одного файла — выключенного шрифта, которого
/// `fc-list` уже не видит.
pub fn fc_query_fonts_argv(file: &str) -> Vec<String> {
    vec!["fc-query".to_string(), "--format".to_string(), FC_FONT_FORMAT.to_string(), file.to_string()]
}

/// Разобрать вывод [`fc_list_fonts_argv`]. Повторы (одно начертание,
/// найденное дважды) снимаются; начертание без имени — «Regular».
///
/// Переменный шрифт fontconfig перечисляет дважды: сам файл целиком (без
/// имени начертания) и каждое именованное начертание того же файла. Запись
/// без имени у такого файла пропускается — иначе у семейства появлялось
/// лишнее «Regular», которое для файла курсива рисовало прямой образец, а
/// его «✕» удаляло курсив.
pub fn parse_fc_list_fonts(out: &str, user_dirs: &[String]) -> Vec<InstalledFont> {
    let named_files: BTreeSet<&str> = out
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let (_, style, file) = (parts.next()?, parts.next()?, parts.next()?);
            (!style.trim().is_empty()).then_some(file.trim())
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut fonts = Vec::new();
    for line in out.lines() {
        let mut parts = line.split('\t');
        let (Some(family), Some(style), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let foundry = parts.next().unwrap_or("");
        let langs = parts.next().unwrap_or("");
        let (family, file) = (family.trim(), file.trim());
        if family.is_empty() || file.is_empty() {
            continue;
        }
        // Служебные растровые шрифты X11 (`cursor.pcf`, `deccurs.pcf`…) имени
        // не имеют — fontconfig отдаёт вместо него имя файла. Текста ими не
        // набрать, в списке шрифтов им не место.
        let family_lower = family.to_lowercase();
        if [".pcf", ".pcf.gz", ".bdf"].iter().any(|ext| family_lower.ends_with(ext)) {
            continue;
        }
        if style.trim().is_empty() && named_files.contains(file) {
            continue;
        }
        let style = if style.trim().is_empty() { "Regular" } else { style.trim() };
        if !seen.insert((family.to_string(), style.to_string(), file.to_string())) {
            continue;
        }
        let user = font_file_is_user(file, user_dirs);
        fonts.push(InstalledFont {
            family: family.to_string(),
            style: style.to_string(),
            file: file.to_string(),
            user,
            shared: !user && font_file_is_shared(file),
            disabled: false,
            foundry: font_foundry_norm(foundry),
            scripts: font_scripts_of(langs),
        });
    }
    fonts
}

/// Вес начертания для порядка внутри семейства: от тонкого к жирному,
/// прямое раньше наклонного.
fn style_rank(style: &str) -> (i32, bool, String) {
    let s = style.to_lowercase();
    let weight = [
        ("thin", 100),
        ("hairline", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("light", 300),
        ("book", 400),
        ("regular", 400),
        ("normal", 400),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
    ]
    .iter()
    .find(|(k, _)| s.replace([' ', '-'], "").contains(k))
    .map(|(_, w)| *w)
    .unwrap_or(400);
    (weight, s.contains("italic") || s.contains("oblique"), s)
}

/// Сгруппировать шрифты по семействам; семейства — по алфавиту без учёта
/// регистра, начертания — от тонкого к жирному.
pub fn group_font_families(fonts: &[InstalledFont]) -> Vec<FontFamilyGroup> {
    let mut groups: Vec<FontFamilyGroup> = Vec::new();
    for f in fonts {
        match groups.iter_mut().find(|g| g.family == f.family) {
            Some(g) => g.styles.push(f.clone()),
            None => groups.push(FontFamilyGroup { family: f.family.clone(), styles: vec![f.clone()] }),
        }
    }
    for g in &mut groups {
        g.styles.sort_by_key(|f| style_rank(&f.style));
    }
    groups.sort_by(|a, b| a.family.to_lowercase().cmp(&b.family.to_lowercase()).then(a.family.cmp(&b.family)));
    groups
}

/// Подходит ли семейство под строку поиска (без учёта регистра, по любому
/// слову строки).
pub fn font_family_matches(family: &str, query: &str) -> bool {
    let family = family.to_lowercase();
    query.split_whitespace().all(|w| family.contains(&w.to_lowercase()))
}

/// Подходит ли файл по расширению.
pub fn font_install_ext_ok(path: &str) -> bool {
    let lower = path.to_lowercase();
    FONT_INSTALL_EXTS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

/// Фильтр окна выбора файла шрифта.
pub fn font_install_filter() -> String {
    let mask: Vec<String> = FONT_INSTALL_EXTS.iter().map(|e| format!("*.{e}")).collect();
    // TRANSLATORS: file dialog filter; %1 is the list of file masks.
    format!("{};;{}", trf("Font files (%1)", &[&mask.join(" ")]), tr("All files (*)"))
}

/// Заголовок окна выбора файла шрифта.
pub fn font_install_dialog_title() -> String {
    tr("Install Font")
}

/// argv проверки файла шрифта: какие семейства в нём видит fontconfig.
pub fn fc_query_families_argv(path: &str) -> Vec<String> {
    vec!["fc-query".to_string(), "--format".to_string(), "%{family[0]}\n".to_string(), path.to_string()]
}

/// Семейства из вывода [`fc_query_families_argv`] (без повторов, по
/// порядку). Пусто — fontconfig этот файл шрифтом не считает.
pub fn parse_fc_query_families(out: &str) -> Vec<String> {
    let mut out_list: Vec<String> = Vec::new();
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !out_list.iter().any(|f| f == line) {
            out_list.push(line.to_string());
        }
    }
    out_list
}

/// Куда ставится файл `source`: `<каталог шрифтов>/<имя файла>`. `None` —
/// у пути нет имени.
pub fn font_install_dest(data_home: &str, source: &str) -> Option<String> {
    let name = Path::new(source).file_name()?.to_string_lossy().into_owned();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(format!("{}/{name}", font_install_dir(data_home)))
}

/// argv пересборки кэша fontconfig для каталога.
pub fn fc_cache_argv(dir: &str) -> Vec<String> {
    vec!["fc-cache".to_string(), "-f".to_string(), dir.to_string()]
}

/// Куда ставить шрифт: значения выбора «для себя / для всех».
pub const FONT_INSTALL_SCOPES: &[&str] = &["user", "all"];

/// Можно ли на этой системе ставить шрифты «для всех»: на NixOS шрифты
/// системы описывает её конфигурация, на SteamOS системный раздел только
/// для чтения — помощник от root там откажет, и окно выбор не предлагает.
pub fn font_shared_supported(family: crate::system::PkgFamily, steamos: bool) -> bool {
    family != crate::system::PkgFamily::Nix && !steamos
}

/// Варианты выбора, для кого ставится шрифт: `(значение, подпись)`. Без
/// `shared_ok` ([`font_shared_supported`]) — только «для себя».
pub fn font_install_scope_choices(shared_ok: bool) -> Vec<(&'static str, String)> {
    FONT_INSTALL_SCOPES
        .iter()
        .filter(|id| shared_ok || !font_install_scope_is_all(id))
        .map(|id| {
            let label = match *id {
                // TRANSLATORS: who a new font is installed for.
                "all" => trc("font install scope", "For all users"),
                // TRANSLATORS: who a new font is installed for.
                _ => trc("font install scope", "Just for me"),
            };
            (*id, label)
        })
        .collect()
}

/// Ставить ли для всех пользователей (неизвестное значение — для себя:
/// без пароля и без касания системы).
pub fn font_install_scope_is_all(scope: &str) -> bool {
    scope == "all"
}

/// Куда ляжет шрифт «для всех»: `<SHARED_FONT_DIR>/<имя файла>`. `None` — у
/// пути нет имени.
pub fn font_shared_dest(source: &str) -> Option<String> {
    let name = Path::new(source).file_name()?.to_string_lossy().into_owned();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(format!("{SHARED_FONT_DIR}/{name}"))
}

/// Чем кончилась установка шрифта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontInstallOutcome {
    /// Поставлен; семейства из файла.
    Installed(Vec<String>),
    /// Поставлен для всех пользователей; семейства из файла.
    InstalledForAll(Vec<String>),
    /// Не поставлен для всех: готовый текст для человека (отказ проверки
    /// помощника, пароль не введён, помощника нет).
    NotForAll(String),
    /// Файл с таким именем уже стоит (тот же самый).
    AlreadyInstalled(String),
    /// Файл с таким именем уже стоит, но другой.
    NameTaken(String),
    /// fontconfig не считает файл шрифтом (или этот вид шрифтов не собран).
    NotAFont(String),
    /// Не скопировался (нет места, нет прав).
    CopyFailed(String),
}

/// Итог установки — для человека.
pub fn font_install_status(outcome: &FontInstallOutcome) -> String {
    match outcome {
        // TRANSLATORS: %1 is a font family name or a comma-separated list of them.
        FontInstallOutcome::Installed(families) => trf("✅ Font installed: %1", &[&families.join(", ")]),
        // TRANSLATORS: %1 is a font family name or a comma-separated list of them.
        FontInstallOutcome::InstalledForAll(families) => {
            trf("✅ Font installed for all users: %1", &[&families.join(", ")])
        }
        FontInstallOutcome::NotForAll(text) => format!("⚠️ {text}"),
        // TRANSLATORS: %1 is a font file name.
        FontInstallOutcome::AlreadyInstalled(name) => trf("%1 is already installed", &[name]),
        // TRANSLATORS: %1 is a font file name.
        FontInstallOutcome::NameTaken(name) => trf(
            "⚠️ A different font file named %1 is already installed — remove it first or rename the new one",
            &[name],
        ),
        // TRANSLATORS: %1 is a file name.
        FontInstallOutcome::NotAFont(name) => {
            trf("⚠️ %1 is not a font this system can read — nothing was installed", &[name])
        }
        // TRANSLATORS: %1 is a file name.
        FontInstallOutcome::CopyFailed(name) => trf("⚠️ Could not copy %1 to your fonts folder", &[name]),
    }
}

/// Вопрос перед удалением: `(заголовок, текст)`. `families` — семейства
/// удаляемых файлов (у `.ttc` в одном файле их бывает несколько, и они
/// уйдут вместе).
pub fn font_remove_confirm(family: &str, files: usize, families: &[String]) -> (String, String) {
    let mut text = crate::i18n::fill(
        &ntr(
            // TRANSLATORS: %1 is a font family, %n the number of font files.
            "Remove the font %1 (%n file)?",
            "Remove the font %1 (%n files)?",
            files as u64,
        ),
        &[&family],
    );
    let others: Vec<&String> = families.iter().filter(|f| *f != family).collect();
    if !others.is_empty() {
        text.push(' ');
        let list: Vec<&str> = others.iter().map(|s| s.as_str()).collect();
        // TRANSLATORS: %1 is a comma-separated list of font families.
        text.push_str(&trf("The same files also hold %1 — they will be removed too.", &[&list.join(", ")]));
    }
    text.push(' ');
    text.push_str(&tr("Apps that use it will fall back to another font."));
    (tr("Remove Font"), text)
}

/// Итог удаления.
pub fn font_remove_status(family: &str, removed: usize, failed: usize) -> String {
    if failed > 0 {
        // TRANSLATORS: %1 is a font family.
        return trf("⚠️ Could not remove some files of %1", &[&family]);
    }
    if removed == 0 {
        return tr(
            "Only fonts installed from a file can be removed here; fonts that came with the system or a \
             package are removed together with that package",
        );
    }
    // TRANSLATORS: %1 is a font family.
    trf("✅ Font removed: %1", &[&family])
}

/// Добавка к вопросу перед удалением, если среди файлов есть поставленные
/// для всех пользователей.
pub fn font_remove_shared_note() -> String {
    tr("It is installed for all users, so Rexon will ask for the administrator password.")
}

/// Подписи раздела «Управление шрифтами».
pub fn font_manager_texts() -> Vec<(&'static str, String)> {
    vec![
        ("open", tr("🗂️ Manage Fonts…")),
        ("open_tip", tr("Installed fonts: preview, install from a file, remove your own")),
        ("close", tr("Hide Font List")),
        ("search", tr("Search fonts…")),
        ("install", tr("➕ Install from File…")),
        ("install_tip", tr("Copy a font file to your fonts folder so every app can use it")),
        (
            "install_all_tip",
            tr("Install the font for everyone who uses this computer; Rexon will ask for the administrator password"),
        ),
        ("scope_tip", tr("Who the new font is for")),
        ("remove", tr("Remove")),
        ("remove_tip", tr("Remove this font from your fonts folder")),
        ("system", tr("System font — view only: it came with the system or a package")),
        ("shared", tr("Installed for all users — removing it asks for the administrator password")),
        ("user", tr("Installed by you")),
        ("empty", tr("No fonts match the search")),
        ("pick", tr("Choose a font on the left to see its styles")),
        ("sample", tr("The quick brown fox jumps over the lazy dog 0123456789")),
        ("no_fclist", tr("fontconfig (fc-list) isn't available, so the font list can't be shown")),
        ("disable", trc("font action", "Disable")),
        ("disable_tip", tr("Hide this font from all apps without deleting the file; you can turn it back on any time")),
        ("enable", trc("font action", "Enable")),
        ("enable_tip", tr("Let apps use this font again")),
        // TRANSLATORS: short badge next to a disabled font in the list.
        ("off", trc("font state", "off")),
        ("disabled_note", tr("Disabled: apps don't see this font, the file stays in place")),
        // TRANSLATORS: %1 is the sample text size in points; keep %1.
        ("sample_size", tr("Sample size: %1 pt")),
        ("duplicates", tr("🔍 Find Duplicates")),
        ("duplicates_tip", tr("Show font styles that are installed in more than one file")),
        ("duplicates_close", tr("Back to Font List")),
        (
            "duplicates_note",
            tr("Apps pick any one of the copies. Your extra copies can be removed; system copies belong to the system and stay."),
        ),
        ("copy_system", tr("System copy")),
        ("copy_shared", tr("Copy for all users")),
        ("filter_foundry_tip", tr("Foundry: the company or project that made the font")),
        ("filter_type_tip", tr("Font file type")),
        ("filter_script_tip", tr("Writing system: show only fonts that can write in it")),
        ("sample_text_tip", tr("Your own text for the samples; Rexon remembers it. Leave empty for the default text.")),
        ("sample_reset", tr("Default Text")),
        // TRANSLATORS: switch in the font preview: text samples of every style.
        ("view_sample", trc("font preview", "Samples")),
        // TRANSLATORS: switch in the font preview: table of the font's characters.
        ("view_chars", trc("font preview", "Characters")),
        ("chars_style_tip", tr("Style whose characters are shown")),
        ("chars_block_tip", tr("Unicode block")),
        ("chars_none", tr("No characters to show: fontconfig couldn't read this font file")),
        ("char_pick", tr("Click a character to see its code and copy it")),
        ("groups_tip", tr("Pick a group to show its fonts. Drag a font from the list onto one of your groups to add it")),
        ("group_new", tr("➕ New Group…")),
        ("group_new_tip", tr("Your own group of fonts, shared with KDE Font Management")),
        ("group_name", tr("Group name")),
        ("group_create", tr("Create")),
        ("group_cancel", tr("Cancel")),
        ("group_rename", tr("✏️ Rename…")),
        ("group_rename_save", tr("Rename")),
        ("group_remove", tr("🗑️ Remove Group")),
        ("group_remove_tip", tr("Removes only the group — the fonts stay installed")),
        ("group_enable", tr("Enable Fonts in Group")),
        ("group_disable", tr("Disable Fonts in Group")),
        ("group_toggle_tip", tr("Turn every font of this group on or off at once, without removing anything")),
        ("group_export", tr("📦 Export…")),
        ("group_export_tip", tr("Save the font files of this group into a ZIP archive")),
        ("group_add", tr("➕ Add to Group…")),
        ("group_add_tip", tr("Add this font family to one of your groups")),
        ("group_take", tr("➖ Remove from Group")),
        ("group_take_tip", tr("Take this font family out of the group; the font stays installed")),
        (
            "group_empty",
            tr("No fonts in this group match the filters yet. Add fonts with “Add to Group…” or drag them onto the group"),
        ),
    ]
}

/// Сводка над списком: сколько семейств, из них своих.
pub fn font_manager_summary(families: usize, user_families: usize) -> String {
    crate::i18n::fill(
        &ntr(
            // TRANSLATORS: %n is the number of font families, %1 how many of them the user installed.
            "%n font family, yours: %1",
            "%n font families, yours: %1",
            families as u64,
        ),
        &[&user_families],
    )
}

// ── Фильтр «Все / Личные / Системные», поиск, кегль образца ─────────────

/// Значения фильтра списка шрифтов по месту: все, личные (каталоги шрифтов
/// пользователя), системные (всё остальное, что видит fontconfig).
pub const FONT_ORIGINS: &[&str] = &["all", "user", "system"];

/// Варианты фильтра для списка: `(значение, подпись)`.
pub fn font_origin_choices() -> Vec<(&'static str, String)> {
    FONT_ORIGINS
        .iter()
        .map(|id| {
            let label = match *id {
                "user" => trc("font origin", "Personal"),
                "system" => trc("font origin", "System"),
                _ => trc("font origin", "All"),
            };
            (*id, label)
        })
        .collect()
}

/// Кегль образца в «Управлении шрифтами»: наименьший, наибольший и
/// начальный (пункты), как у предпросмотра KDE.
pub const FONT_SAMPLE_SIZE: (i32, i32, i32) = (12, 48, 16);

/// Что показать в списке: начертания под фильтр `origin` (`FONT_ORIGINS`;
/// неизвестное — «все») и строку поиска. Слово поиска, которого нет в имени
/// семейства, ищется в имени начертания: «inter italic» оставит у Inter
/// только наклонные. Семейство без подходящих начертаний не показывается.
pub fn font_group_view(groups: &[FontFamilyGroup], origin: &str, query: &str) -> Vec<FontFamilyGroup> {
    let filter = FontListFilter { origin: origin.to_string(), query: query.to_string(), ..Default::default() };
    font_group_view_filtered(groups, &filter)
}

// ── Фильтры списка: производитель, тип файла, письменность ──────────────
//
// Набор — как у фильтра «Управления шрифтами» KDE: производитель
// (`foundry` fontconfig), тип файла (по расширению: TrueType, OpenType,
// коллекция, Type 1, PCF, BDF; у Rexon ещё WOFF — его ставит окно
// установки), письменность (языки fontconfig `lang`; где fontconfig языка
// не знает или знает не у всех шрифтов — проба одним символом через
// `fc-list :charset=…`). Фильтры складываются друг с другом, с местом
// («Все / Личные / Системные») и со строкой поиска, а не заменяют друг
// друга: «кириллица + OpenType» — обычная просьба.

/// Письменность и язык fontconfig, по которому она узнаётся: шрифт
/// подходит, если в его `lang` есть этот язык с любой территорией
/// (`zh-tw` подходит и для «упрощённого китайского» — fontconfig так же
/// считает языки равными с точностью до территории).
const FONT_SCRIPT_LANGS: &[(&str, &str)] = &[
    ("latin", "en"),
    ("greek", "el"),
    ("cyrillic", "ru"),
    ("armenian", "hy"),
    ("hebrew", "he"),
    ("arabic", "ar"),
    ("syriac", "syr"),
    ("thaana", "div"),
    ("devanagari", "hi"),
    ("bengali", "bn"),
    ("gurmukhi", "pa"),
    ("gujarati", "gu"),
    ("oriya", "or"),
    ("tamil", "ta"),
    ("telugu", "te"),
    ("kannada", "kn"),
    ("malayalam", "ml"),
    ("sinhala", "si"),
    ("thai", "th"),
    ("lao", "lo"),
    ("tibetan", "bo"),
    ("myanmar", "my"),
    ("georgian", "ka"),
    ("khmer", "km"),
    ("chinese_simplified", "zh-cn"),
    ("chinese_traditional", "zh-tw"),
    ("japanese", "ja"),
    ("korean", "ko"),
    ("vietnamese", "vi"),
];

/// Письменности, которые узнаются по одному символу (`fc-list
/// :charset=<код>`): у огамического и рунического письма языка в
/// fontconfig нет вовсе, у остальных он есть не во всех версиях.
pub const FONT_SCRIPT_PROBES: &[(&str, u32)] = &[
    ("telugu", 0x0C15),
    ("kannada", 0x0C95),
    ("malayalam", 0x0D15),
    ("sinhala", 0x0D9A),
    ("myanmar", 0x1000),
    ("ogham", 0x1681),
    ("runic", 0x16A0),
];

/// Все значения фильтра письменности, в порядке хранения у шрифта.
/// «other» — «символы и прочее»: шрифт, не подошедший ни под одну.
pub const FONT_SCRIPTS: &[&str] = &[
    "latin",
    "greek",
    "cyrillic",
    "armenian",
    "hebrew",
    "arabic",
    "syriac",
    "thaana",
    "devanagari",
    "bengali",
    "gurmukhi",
    "gujarati",
    "oriya",
    "tamil",
    "telugu",
    "kannada",
    "malayalam",
    "sinhala",
    "thai",
    "lao",
    "tibetan",
    "myanmar",
    "georgian",
    "khmer",
    "chinese_simplified",
    "chinese_traditional",
    "japanese",
    "korean",
    "vietnamese",
    "ogham",
    "runic",
    "other",
];

/// Основная часть языкового тега: `zh-tw` → `zh`.
fn lang_primary(lang: &str) -> &str {
    lang.split('-').next().unwrap_or(lang)
}

/// Письменности по полю `%{lang}` (языки через `|`).
pub fn font_scripts_of(langs: &str) -> Vec<&'static str> {
    let have: BTreeSet<String> = langs
        .split('|')
        .map(|l| lang_primary(l.trim()).to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    FONT_SCRIPT_LANGS
        .iter()
        .filter(|(_, lang)| have.contains(lang_primary(lang)))
        .map(|(id, _)| *id)
        .collect()
}

/// argv пробы письменности: файлы шрифтов, в которых есть символ `ch`.
pub fn fc_list_char_argv(ch: u32) -> Vec<String> {
    vec![
        "fc-list".to_string(),
        "--format".to_string(),
        "%{file}\n".to_string(),
        format!(":charset={ch:x}"),
    ]
}

/// Дописать письменность `script` шрифтам, чьи файлы нашла проба
/// ([`fc_list_char_argv`]). Порядок письменностей у шрифта — как в
/// [`FONT_SCRIPTS`], повторов нет.
pub fn font_add_probed_script(fonts: &mut [InstalledFont], script: &str, probe_out: &str) {
    let Some(id) = FONT_SCRIPTS.iter().find(|s| **s == script && **s != "other") else {
        return;
    };
    let files: BTreeSet<&str> = probe_out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    for f in fonts.iter_mut().filter(|f| files.contains(f.file.as_str())) {
        if !f.scripts.contains(id) {
            f.scripts.push(id);
            f.scripts.sort_by_key(|s| FONT_SCRIPTS.iter().position(|x| x == s));
        }
    }
}

/// Подпись письменности.
pub fn font_script_label(id: &str) -> String {
    match id {
        "latin" => tr("Latin"),
        "greek" => tr("Greek"),
        "cyrillic" => tr("Cyrillic"),
        "armenian" => tr("Armenian"),
        "hebrew" => tr("Hebrew"),
        "arabic" => tr("Arabic"),
        "syriac" => tr("Syriac"),
        "thaana" => tr("Thaana"),
        "devanagari" => tr("Devanagari"),
        "bengali" => tr("Bengali"),
        "gurmukhi" => tr("Gurmukhi"),
        "gujarati" => tr("Gujarati"),
        "oriya" => tr("Oriya"),
        "tamil" => tr("Tamil"),
        "telugu" => tr("Telugu"),
        "kannada" => tr("Kannada"),
        "malayalam" => tr("Malayalam"),
        "sinhala" => tr("Sinhala"),
        "thai" => tr("Thai"),
        "lao" => tr("Lao"),
        "tibetan" => tr("Tibetan"),
        "myanmar" => tr("Myanmar"),
        "georgian" => tr("Georgian"),
        "khmer" => tr("Khmer"),
        "chinese_simplified" => tr("Simplified Chinese"),
        "chinese_traditional" => tr("Traditional Chinese"),
        "japanese" => tr("Japanese"),
        "korean" => tr("Korean"),
        "vietnamese" => tr("Vietnamese"),
        "ogham" => tr("Ogham"),
        "runic" => tr("Runic"),
        _ => tr("Symbols and other"),
    }
}

/// Типы файлов фильтра и их расширения (без учёта регистра).
pub const FONT_FILE_TYPES: &[(&str, &[&str])] = &[
    ("truetype", &["ttf"]),
    ("opentype", &["otf"]),
    ("collection", &["ttc", "otc"]),
    ("type1", &["pfa", "pfb", "t1", "gsf", "pfa.gz", "pfb.gz"]),
    ("pcf", &["pcf", "pcf.gz"]),
    ("bdf", &["bdf", "bdf.gz"]),
    ("woff", &["woff", "woff2"]),
];

/// Тип файла шрифта по расширению (`None` — ни один из [`FONT_FILE_TYPES`]).
pub fn font_file_type(file: &str) -> Option<&'static str> {
    let name = file.rsplit('/').next().unwrap_or(file).to_lowercase();
    FONT_FILE_TYPES
        .iter()
        .find(|(_, exts)| exts.iter().any(|e| name.ends_with(&format!(".{e}"))))
        .map(|(id, _)| *id)
}

/// Подпись типа файла.
pub fn font_file_type_label(id: &str) -> String {
    match id {
        "truetype" => tr("TrueType"),
        "opentype" => tr("OpenType"),
        "collection" => tr("Font collection (TTC/OTC)"),
        "type1" => tr("Type 1 (PostScript)"),
        "pcf" => tr("PCF bitmap font"),
        "bdf" => tr("BDF bitmap font"),
        "woff" => tr("Web font (WOFF)"),
        _ => id.to_string(),
    }
}

/// Значение производителя «не указан».
pub const FONT_FOUNDRY_UNKNOWN: &str = "unknown";

/// Производитель так, как его показывать и сравнивать: без пробелов по
/// краям (код производителя в OS/2 — ровно 4 знака, короткие дополнены
/// пробелами); пустой и заглушки (`unknown`, `UKWN`, `NONE`) — «не указан».
pub fn font_foundry_norm(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() || ["unknown", "ukwn", "none"].contains(&t.to_ascii_lowercase().as_str()) {
        FONT_FOUNDRY_UNKNOWN.to_string()
    } else {
        t.to_string()
    }
}

/// Фильтр списка шрифтов целиком. Пустое поле — «любой».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontListFilter {
    /// [`FONT_ORIGINS`]; пустое и неизвестное — «все».
    pub origin: String,
    pub query: String,
    /// Производитель (без учёта регистра) или [`FONT_FOUNDRY_UNKNOWN`].
    pub foundry: String,
    /// Значение [`FONT_FILE_TYPES`].
    pub file_type: String,
    /// Значение [`FONT_SCRIPTS`].
    pub script: String,
}

impl FontListFilter {
    /// Из JSON страницы: `{"origin", "query", "foundry", "type", "script"}`;
    /// чего нет или что не строка — «любой».
    pub fn from_json(text: &str) -> Self {
        let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        FontListFilter {
            origin: get("origin"),
            query: get("query"),
            foundry: get("foundry"),
            file_type: get("type"),
            script: get("script"),
        }
    }

    /// Подходит ли начертание под производителя, тип и письменность.
    pub fn matches_font(&self, f: &InstalledFont) -> bool {
        let foundry_ok = self.foundry.is_empty() || f.foundry.eq_ignore_ascii_case(&self.foundry);
        let type_ok = self.file_type.is_empty() || font_file_type(&f.file) == Some(self.file_type.as_str());
        let script_ok = match self.script.as_str() {
            "" => true,
            "other" => f.scripts.is_empty(),
            s => f.scripts.contains(&s),
        };
        foundry_ok && type_ok && script_ok
    }
}

/// [`font_group_view`] с фильтрами производителя, типа и письменности.
pub fn font_group_view_filtered(groups: &[FontFamilyGroup], filter: &FontListFilter) -> Vec<FontFamilyGroup> {
    let words: Vec<String> = filter.query.split_whitespace().map(str::to_lowercase).collect();
    groups
        .iter()
        .filter_map(|g| {
            let family = g.family.to_lowercase();
            let rest: Vec<&String> = words.iter().filter(|w| !family.contains(w.as_str())).collect();
            let styles: Vec<InstalledFont> = g
                .styles
                .iter()
                .filter(|f| match filter.origin.as_str() {
                    "user" => f.user,
                    "system" => !f.user,
                    _ => true,
                })
                .filter(|f| {
                    let style = f.style.to_lowercase();
                    rest.iter().all(|w| style.contains(w.as_str()))
                })
                .filter(|f| filter.matches_font(f))
                .cloned()
                .collect();
            (!styles.is_empty()).then(|| FontFamilyGroup { family: g.family.clone(), styles })
        })
        .collect()
}

/// Варианты трёх фильтров по тому, что установлено: `(значение, подпись)`,
/// первым — «любой» с пустым значением. Пустых вариантов не бывает: тип и
/// письменность, которых нет ни у одного шрифта, не предлагаются.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFilterChoices {
    pub foundries: Vec<(String, String)>,
    pub file_types: Vec<(String, String)>,
    pub scripts: Vec<(String, String)>,
}

/// Собрать [`FontFilterChoices`]. Производители — по алфавиту без учёта
/// регистра (одинаковые с точностью до регистра — один вариант), «не
/// указан» — последним; типы — в порядке [`FONT_FILE_TYPES`];
/// письменности — по подписи, «символы и прочее» — последней.
pub fn font_filter_choices(fonts: &[InstalledFont]) -> FontFilterChoices {
    let mut foundries: Vec<String> = Vec::new();
    let mut unknown = false;
    for f in fonts {
        if f.foundry == FONT_FOUNDRY_UNKNOWN {
            unknown = true;
        } else if !foundries.iter().any(|x| x.eq_ignore_ascii_case(&f.foundry)) {
            foundries.push(f.foundry.clone());
        }
    }
    foundries.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    let mut foundry_choices = vec![(String::new(), tr("Any foundry"))];
    foundry_choices.extend(foundries.into_iter().map(|f| (f.clone(), f)));
    if unknown {
        foundry_choices.push((FONT_FOUNDRY_UNKNOWN.to_string(), tr("Foundry not specified")));
    }

    let mut type_choices = vec![(String::new(), tr("Any file type"))];
    for (id, _) in FONT_FILE_TYPES {
        if fonts.iter().any(|f| font_file_type(&f.file) == Some(*id)) {
            type_choices.push((id.to_string(), font_file_type_label(id)));
        }
    }

    let mut scripts: Vec<(String, String)> = FONT_SCRIPTS
        .iter()
        .filter(|id| **id != "other" && fonts.iter().any(|f| f.scripts.contains(id)))
        .map(|id| (id.to_string(), font_script_label(id)))
        .collect();
    scripts.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    let mut script_choices = vec![(String::new(), tr("Any writing system"))];
    script_choices.extend(scripts);
    if fonts.iter().any(|f| f.scripts.is_empty()) {
        script_choices.push(("other".to_string(), font_script_label("other")));
    }
    FontFilterChoices { foundries: foundry_choices, file_types: type_choices, scripts: script_choices }
}

// ── Таблица символов и свой текст образца ───────────────────────────────
//
// Как «Предпросмотр» KDE в режиме «Блок Юникода»: символы, которые есть в
// файле шрифта (`%{charset}` fontconfig), по блокам Юникода. Блоки — вся
// основная плоскость и те блоки дополнительных, что встречаются в обычных
// шрифтах (латиница и кириллица, эмодзи, математика, карты, частные
// области значков); остальное, что есть в шрифте, — одним блоком «Другие
// символы», ничего не теряется.

/// Блоки Юникода (Blocks.txt, Unicode 17.0): начало, конец, имя.
/// По возрастанию начала — поиск блока идёт двоичный.
pub const UNICODE_BLOCKS: &[(u32, u32, &str)] = &[
    (0x0000, 0x007F, n_("Basic Latin")),
    (0x0080, 0x00FF, n_("Latin-1 Supplement")),
    (0x0100, 0x017F, n_("Latin Extended-A")),
    (0x0180, 0x024F, n_("Latin Extended-B")),
    (0x0250, 0x02AF, n_("IPA Extensions")),
    (0x02B0, 0x02FF, n_("Spacing Modifier Letters")),
    (0x0300, 0x036F, n_("Combining Diacritical Marks")),
    (0x0370, 0x03FF, n_("Greek and Coptic")),
    (0x0400, 0x04FF, n_("Cyrillic")),
    (0x0500, 0x052F, n_("Cyrillic Supplement")),
    (0x0530, 0x058F, n_("Armenian")),
    (0x0590, 0x05FF, n_("Hebrew")),
    (0x0600, 0x06FF, n_("Arabic")),
    (0x0700, 0x074F, n_("Syriac")),
    (0x0750, 0x077F, n_("Arabic Supplement")),
    (0x0780, 0x07BF, n_("Thaana")),
    (0x07C0, 0x07FF, n_("NKo")),
    (0x0800, 0x083F, n_("Samaritan")),
    (0x0840, 0x085F, n_("Mandaic")),
    (0x0860, 0x086F, n_("Syriac Supplement")),
    (0x0870, 0x089F, n_("Arabic Extended-B")),
    (0x08A0, 0x08FF, n_("Arabic Extended-A")),
    (0x0900, 0x097F, n_("Devanagari")),
    (0x0980, 0x09FF, n_("Bengali")),
    (0x0A00, 0x0A7F, n_("Gurmukhi")),
    (0x0A80, 0x0AFF, n_("Gujarati")),
    (0x0B00, 0x0B7F, n_("Oriya")),
    (0x0B80, 0x0BFF, n_("Tamil")),
    (0x0C00, 0x0C7F, n_("Telugu")),
    (0x0C80, 0x0CFF, n_("Kannada")),
    (0x0D00, 0x0D7F, n_("Malayalam")),
    (0x0D80, 0x0DFF, n_("Sinhala")),
    (0x0E00, 0x0E7F, n_("Thai")),
    (0x0E80, 0x0EFF, n_("Lao")),
    (0x0F00, 0x0FFF, n_("Tibetan")),
    (0x1000, 0x109F, n_("Myanmar")),
    (0x10A0, 0x10FF, n_("Georgian")),
    (0x1100, 0x11FF, n_("Hangul Jamo")),
    (0x1200, 0x137F, n_("Ethiopic")),
    (0x1380, 0x139F, n_("Ethiopic Supplement")),
    (0x13A0, 0x13FF, n_("Cherokee")),
    (0x1400, 0x167F, n_("Unified Canadian Aboriginal Syllabics")),
    (0x1680, 0x169F, n_("Ogham")),
    (0x16A0, 0x16FF, n_("Runic")),
    (0x1700, 0x171F, n_("Tagalog")),
    (0x1720, 0x173F, n_("Hanunoo")),
    (0x1740, 0x175F, n_("Buhid")),
    (0x1760, 0x177F, n_("Tagbanwa")),
    (0x1780, 0x17FF, n_("Khmer")),
    (0x1800, 0x18AF, n_("Mongolian")),
    (0x18B0, 0x18FF, n_("Unified Canadian Aboriginal Syllabics Extended")),
    (0x1900, 0x194F, n_("Limbu")),
    (0x1950, 0x197F, n_("Tai Le")),
    (0x1980, 0x19DF, n_("New Tai Lue")),
    (0x19E0, 0x19FF, n_("Khmer Symbols")),
    (0x1A00, 0x1A1F, n_("Buginese")),
    (0x1A20, 0x1AAF, n_("Tai Tham")),
    (0x1AB0, 0x1AFF, n_("Combining Diacritical Marks Extended")),
    (0x1B00, 0x1B7F, n_("Balinese")),
    (0x1B80, 0x1BBF, n_("Sundanese")),
    (0x1BC0, 0x1BFF, n_("Batak")),
    (0x1C00, 0x1C4F, n_("Lepcha")),
    (0x1C50, 0x1C7F, n_("Ol Chiki")),
    (0x1C80, 0x1C8F, n_("Cyrillic Extended-C")),
    (0x1C90, 0x1CBF, n_("Georgian Extended")),
    (0x1CC0, 0x1CCF, n_("Sundanese Supplement")),
    (0x1CD0, 0x1CFF, n_("Vedic Extensions")),
    (0x1D00, 0x1D7F, n_("Phonetic Extensions")),
    (0x1D80, 0x1DBF, n_("Phonetic Extensions Supplement")),
    (0x1DC0, 0x1DFF, n_("Combining Diacritical Marks Supplement")),
    (0x1E00, 0x1EFF, n_("Latin Extended Additional")),
    (0x1F00, 0x1FFF, n_("Greek Extended")),
    (0x2000, 0x206F, n_("General Punctuation")),
    (0x2070, 0x209F, n_("Superscripts and Subscripts")),
    (0x20A0, 0x20CF, n_("Currency Symbols")),
    (0x20D0, 0x20FF, n_("Combining Diacritical Marks for Symbols")),
    (0x2100, 0x214F, n_("Letterlike Symbols")),
    (0x2150, 0x218F, n_("Number Forms")),
    (0x2190, 0x21FF, n_("Arrows")),
    (0x2200, 0x22FF, n_("Mathematical Operators")),
    (0x2300, 0x23FF, n_("Miscellaneous Technical")),
    (0x2400, 0x243F, n_("Control Pictures")),
    (0x2440, 0x245F, n_("Optical Character Recognition")),
    (0x2460, 0x24FF, n_("Enclosed Alphanumerics")),
    (0x2500, 0x257F, n_("Box Drawing")),
    (0x2580, 0x259F, n_("Block Elements")),
    (0x25A0, 0x25FF, n_("Geometric Shapes")),
    (0x2600, 0x26FF, n_("Miscellaneous Symbols")),
    (0x2700, 0x27BF, n_("Dingbats")),
    (0x27C0, 0x27EF, n_("Miscellaneous Mathematical Symbols-A")),
    (0x27F0, 0x27FF, n_("Supplemental Arrows-A")),
    (0x2800, 0x28FF, n_("Braille Patterns")),
    (0x2900, 0x297F, n_("Supplemental Arrows-B")),
    (0x2980, 0x29FF, n_("Miscellaneous Mathematical Symbols-B")),
    (0x2A00, 0x2AFF, n_("Supplemental Mathematical Operators")),
    (0x2B00, 0x2BFF, n_("Miscellaneous Symbols and Arrows")),
    (0x2C00, 0x2C5F, n_("Glagolitic")),
    (0x2C60, 0x2C7F, n_("Latin Extended-C")),
    (0x2C80, 0x2CFF, n_("Coptic")),
    (0x2D00, 0x2D2F, n_("Georgian Supplement")),
    (0x2D30, 0x2D7F, n_("Tifinagh")),
    (0x2D80, 0x2DDF, n_("Ethiopic Extended")),
    (0x2DE0, 0x2DFF, n_("Cyrillic Extended-A")),
    (0x2E00, 0x2E7F, n_("Supplemental Punctuation")),
    (0x2E80, 0x2EFF, n_("CJK Radicals Supplement")),
    (0x2F00, 0x2FDF, n_("Kangxi Radicals")),
    (0x2FF0, 0x2FFF, n_("Ideographic Description Characters")),
    (0x3000, 0x303F, n_("CJK Symbols and Punctuation")),
    (0x3040, 0x309F, n_("Hiragana")),
    (0x30A0, 0x30FF, n_("Katakana")),
    (0x3100, 0x312F, n_("Bopomofo")),
    (0x3130, 0x318F, n_("Hangul Compatibility Jamo")),
    (0x3190, 0x319F, n_("Kanbun")),
    (0x31A0, 0x31BF, n_("Bopomofo Extended")),
    (0x31C0, 0x31EF, n_("CJK Strokes")),
    (0x31F0, 0x31FF, n_("Katakana Phonetic Extensions")),
    (0x3200, 0x32FF, n_("Enclosed CJK Letters and Months")),
    (0x3300, 0x33FF, n_("CJK Compatibility")),
    (0x3400, 0x4DBF, n_("CJK Unified Ideographs Extension A")),
    (0x4DC0, 0x4DFF, n_("Yijing Hexagram Symbols")),
    (0x4E00, 0x9FFF, n_("CJK Unified Ideographs")),
    (0xA000, 0xA48F, n_("Yi Syllables")),
    (0xA490, 0xA4CF, n_("Yi Radicals")),
    (0xA4D0, 0xA4FF, n_("Lisu")),
    (0xA500, 0xA63F, n_("Vai")),
    (0xA640, 0xA69F, n_("Cyrillic Extended-B")),
    (0xA6A0, 0xA6FF, n_("Bamum")),
    (0xA700, 0xA71F, n_("Modifier Tone Letters")),
    (0xA720, 0xA7FF, n_("Latin Extended-D")),
    (0xA800, 0xA82F, n_("Syloti Nagri")),
    (0xA830, 0xA83F, n_("Common Indic Number Forms")),
    (0xA840, 0xA87F, n_("Phags-pa")),
    (0xA880, 0xA8DF, n_("Saurashtra")),
    (0xA8E0, 0xA8FF, n_("Devanagari Extended")),
    (0xA900, 0xA92F, n_("Kayah Li")),
    (0xA930, 0xA95F, n_("Rejang")),
    (0xA960, 0xA97F, n_("Hangul Jamo Extended-A")),
    (0xA980, 0xA9DF, n_("Javanese")),
    (0xA9E0, 0xA9FF, n_("Myanmar Extended-B")),
    (0xAA00, 0xAA5F, n_("Cham")),
    (0xAA60, 0xAA7F, n_("Myanmar Extended-A")),
    (0xAA80, 0xAADF, n_("Tai Viet")),
    (0xAAE0, 0xAAFF, n_("Meetei Mayek Extensions")),
    (0xAB00, 0xAB2F, n_("Ethiopic Extended-A")),
    (0xAB30, 0xAB6F, n_("Latin Extended-E")),
    (0xAB70, 0xABBF, n_("Cherokee Supplement")),
    (0xABC0, 0xABFF, n_("Meetei Mayek")),
    (0xAC00, 0xD7AF, n_("Hangul Syllables")),
    (0xD7B0, 0xD7FF, n_("Hangul Jamo Extended-B")),
    (0xE000, 0xF8FF, n_("Private Use Area")),
    (0xF900, 0xFAFF, n_("CJK Compatibility Ideographs")),
    (0xFB00, 0xFB4F, n_("Alphabetic Presentation Forms")),
    (0xFB50, 0xFDFF, n_("Arabic Presentation Forms-A")),
    (0xFE00, 0xFE0F, n_("Variation Selectors")),
    (0xFE10, 0xFE1F, n_("Vertical Forms")),
    (0xFE20, 0xFE2F, n_("Combining Half Marks")),
    (0xFE30, 0xFE4F, n_("CJK Compatibility Forms")),
    (0xFE50, 0xFE6F, n_("Small Form Variants")),
    (0xFE70, 0xFEFF, n_("Arabic Presentation Forms-B")),
    (0xFF00, 0xFFEF, n_("Halfwidth and Fullwidth Forms")),
    (0xFFF0, 0xFFFF, n_("Specials")),
    (0x10780, 0x107BF, n_("Latin Extended-F")),
    (0x1D100, 0x1D1FF, n_("Musical Symbols")),
    (0x1D400, 0x1D7FF, n_("Mathematical Alphanumeric Symbols")),
    (0x1DF00, 0x1DFFF, n_("Latin Extended-G")),
    (0x1E030, 0x1E08F, n_("Cyrillic Extended-D")),
    (0x1F000, 0x1F02F, n_("Mahjong Tiles")),
    (0x1F030, 0x1F09F, n_("Domino Tiles")),
    (0x1F0A0, 0x1F0FF, n_("Playing Cards")),
    (0x1F100, 0x1F1FF, n_("Enclosed Alphanumeric Supplement")),
    (0x1F200, 0x1F2FF, n_("Enclosed Ideographic Supplement")),
    (0x1F300, 0x1F5FF, n_("Miscellaneous Symbols and Pictographs")),
    (0x1F600, 0x1F64F, n_("Emoticons")),
    (0x1F650, 0x1F67F, n_("Ornamental Dingbats")),
    (0x1F680, 0x1F6FF, n_("Transport and Map Symbols")),
    (0x1F700, 0x1F77F, n_("Alchemical Symbols")),
    (0x1F780, 0x1F7FF, n_("Geometric Shapes Extended")),
    (0x1F800, 0x1F8FF, n_("Supplemental Arrows-C")),
    (0x1F900, 0x1F9FF, n_("Supplemental Symbols and Pictographs")),
    (0x1FA00, 0x1FA6F, n_("Chess Symbols")),
    (0x1FA70, 0x1FAFF, n_("Symbols and Pictographs Extended-A")),
    (0x1FB00, 0x1FBFF, n_("Symbols for Legacy Computing")),
    (0xF0000, 0xFFFFF, n_("Supplementary Private Use Area-A")),
    (0x100000, 0x10FFFF, n_("Supplementary Private Use Area-B")),
];

/// argv набора символов файла: начертание и `charset` каждого лица.
pub fn fc_query_charset_argv(file: &str) -> Vec<String> {
    vec![
        "fc-query".to_string(),
        "--format".to_string(),
        "%{style[0]}\t%{charset}\n".to_string(),
        file.to_string(),
    ]
}

/// Разобрать `charset` fontconfig: шестнадцатеричные коды и диапазоны
/// через пробел (`20-7e a0 401-44f`). Неразборчивое пропускается.
pub fn parse_fc_charset(text: &str) -> Vec<(u32, u32)> {
    text.split_whitespace()
        .filter_map(|tok| {
            let (a, b) = tok.split_once('-').unwrap_or((tok, tok));
            let (a, b) = (u32::from_str_radix(a, 16).ok()?, u32::from_str_radix(b, 16).ok()?);
            (a <= b && b <= 0x10FFFF).then_some((a, b))
        })
        .collect()
}

/// Набор символов нужного начертания из вывода [`fc_query_charset_argv`]:
/// лицо с тем же именем начертания (без учёта регистра), иначе первое.
pub fn fc_charset_for_style(out: &str, style: &str) -> Vec<(u32, u32)> {
    let faces: Vec<(&str, &str)> = out.lines().filter_map(|l| l.split_once('\t')).collect();
    let face = faces
        .iter()
        .find(|(s, _)| s.trim().eq_ignore_ascii_case(style.trim()))
        .or_else(|| faces.first());
    face.map(|(_, cs)| parse_fc_charset(cs)).unwrap_or_default()
}

/// Показывается ли символ в таблице: управляющие, суррогаты и
/// «несимволы» Юникода — нет (рисовать там нечего).
pub fn font_char_printable(cp: u32) -> bool {
    !(cp < 0x20
        || (0x7F..=0x9F).contains(&cp)
        || (0xD800..=0xDFFF).contains(&cp)
        || (0xFDD0..=0xFDEF).contains(&cp)
        || cp & 0xFFFE == 0xFFFE
        || cp > 0x10FFFF)
}

/// Один блок таблицы символов шрифта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharBlock {
    /// Начало блока; у «Других символов» — `None`.
    pub start: Option<u32>,
    pub name: String,
    pub chars: Vec<u32>,
}

/// Номер блока символа в [`UNICODE_BLOCKS`].
fn unicode_block_index(cp: u32) -> Option<usize> {
    let i = UNICODE_BLOCKS.partition_point(|(start, _, _)| *start <= cp);
    (i > 0 && cp <= UNICODE_BLOCKS[i - 1].1).then(|| i - 1)
}

/// Символы шрифта по блокам: только блоки, где у шрифта есть хоть один
/// показываемый символ, в порядке Юникода; «Другие символы» — последним.
pub fn font_char_blocks(ranges: &[(u32, u32)]) -> Vec<CharBlock> {
    let mut by_block: Vec<Vec<u32>> = vec![Vec::new(); UNICODE_BLOCKS.len()];
    let mut other: Vec<u32> = Vec::new();
    let mut seen = BTreeSet::new();
    for &(a, b) in ranges {
        for cp in a..=b.min(0x10FFFF) {
            if !font_char_printable(cp) || !seen.insert(cp) {
                continue;
            }
            match unicode_block_index(cp) {
                Some(i) => by_block[i].push(cp),
                None => other.push(cp),
            }
        }
    }
    let mut out: Vec<CharBlock> = by_block
        .into_iter()
        .enumerate()
        .filter(|(_, chars)| !chars.is_empty())
        .map(|(i, mut chars)| {
            chars.sort_unstable();
            CharBlock { start: Some(UNICODE_BLOCKS[i].0), name: tr(UNICODE_BLOCKS[i].2), chars }
        })
        .collect();
    if !other.is_empty() {
        other.sort_unstable();
        out.push(CharBlock { start: None, name: tr("Other characters"), chars: other });
    }
    out
}

/// Ключ своего текста образца в настройках Rexon (`settings.json`).
pub const FONT_SAMPLE_TEXT_KEY: &str = "font_sample_text";

/// Наибольшая длина своего текста образца (в символах).
pub const FONT_SAMPLE_TEXT_MAX: usize = 300;

/// Образец по умолчанию — на языке интерфейса.
pub fn font_sample_default() -> String {
    tr("The quick brown fox jumps over the lazy dog 0123456789")
}

/// Текст образца: свой из настроек или, если его нет, — по умолчанию.
pub fn font_sample_text(stored: Option<&str>) -> String {
    match stored.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.chars().take(FONT_SAMPLE_TEXT_MAX).collect(),
        None => font_sample_default(),
    }
}

/// Что записать в настройки из поля ввода: одна строка без пробелов по
/// краям, не длиннее [`FONT_SAMPLE_TEXT_MAX`]. Пусто или ровно образец по
/// умолчанию — пустая строка: тогда после смены языка интерфейса образец
/// сменится вместе с ним, а не застрянет на прежнем языке.
pub fn font_sample_text_to_store(input: &str) -> String {
    let one_line: String = input.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    let text: String = one_line.trim().chars().take(FONT_SAMPLE_TEXT_MAX).collect();
    if text == font_sample_default() {
        String::new()
    } else {
        text
    }
}

// ── Включение и выключение шрифтов: `<rejectfont>` в fonts.conf ─────────
//
// Шрифт выключается без удаления: его файл попадает в наш отдельный блок
// пользовательского `fonts.conf` (fonts-conf(5), `<selectfont>` →
// `<rejectfont>` → `<glob>` с полным путём файла). fontconfig исключает
// такие файлы из набора шрифтов, «как будто их нет в системе», — их не
// видят ни `fc-list`, ни программы; файл остаётся на месте, root не нужен,
// касается только этого пользователя. Включить — убрать путь из блока.
// Пустой блок снимается целиком; файл, в котором после этого ничего не
// осталось, удаляется — система возвращается ровно в прежний вид.
//
// Блок свой, отдельный от блока отрисовки: сглаживание переписывает свой
// блок целиком и про выключенные шрифты не знает. Сброс шрифтов
// (`font_layers_reset_plan`) этот блок не снимает: выключение — выбор
// человека, как удаление, а не настройка вида.

const FC_REJECT_BEGIN: &str = "<!-- >>> Rexon: disabled fonts (managed by Rexon, edits are overwritten) -->";
const FC_REJECT_BEGIN_PREFIX: &str = "<!-- >>> Rexon: disabled fonts";
const FC_REJECT_END: &str = "<!-- <<< Rexon: disabled fonts -->";

/// Можно ли выключить файл записью `<glob>`: путь полный, без `..` и без
/// `*`/`?` — их fontconfig читает как шаблон, и запись выключила бы лишнее.
pub fn font_glob_safe(file: &str) -> bool {
    file.starts_with('/') && !file.contains(['*', '?', '\n', '\r']) && !file.split('/').any(|part| part == "..")
}

/// Обратное к `system_l10n::xml_escape`; `&amp;` — последним.
fn fc_xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Наш блок выключенных шрифтов. Пустой список — пустая строка (блока нет).
pub fn fontconfig_reject_block(files: &[String]) -> String {
    let files: Vec<&String> = files.iter().filter(|f| font_glob_safe(f)).collect();
    if files.is_empty() {
        return String::new();
    }
    let mut out = format!("  {FC_REJECT_BEGIN}\n  <selectfont>\n    <rejectfont>\n");
    for f in files {
        out.push_str(&format!("      <glob>{}</glob>\n", crate::system_l10n::xml_escape(f)));
    }
    out.push_str(&format!("    </rejectfont>\n  </selectfont>\n  {FC_REJECT_END}\n"));
    out
}

/// Файлы, выключенные нашим блоком (чужие `<rejectfont>` не наши — их
/// здесь нет). Разорванный блок — пусто.
pub fn fontconfig_disabled_files(text: &str) -> Vec<String> {
    let Some(begin) = text.find(FC_REJECT_BEGIN_PREFIX) else { return Vec::new() };
    let Some(end_rel) = text[begin..].find(FC_REJECT_END) else { return Vec::new() };
    let mut body = &text[begin..begin + end_rel];
    let mut out: Vec<String> = Vec::new();
    while let Some(at) = body.find("<glob>") {
        let rest = &body[at + "<glob>".len()..];
        let Some(close) = rest.find("</glob>") else { break };
        let file = fc_xml_unescape(rest[..close].trim());
        if font_glob_safe(&file) && !out.contains(&file) {
            out.push(file);
        }
        body = &rest[close + "</glob>".len()..];
    }
    out
}

/// Текст `fonts.conf` с выключенными `files`. Если в итоге остался голый
/// файл ([`fontconfig_is_bare`]) — его надо удалить, а не записывать.
pub fn fontconfig_with_disabled(existing: Option<&str>, files: &[String]) -> Result<String, FontconfigRefusal> {
    fc_with_managed_block(existing, &fontconfig_reject_block(files), FC_REJECT_BEGIN_PREFIX, FC_REJECT_END)
}

/// Файлы, которые затрагивает переключение: всё семейство (`file` пуст)
/// или одно начертание. Без повторов, по алфавиту.
pub fn font_toggle_files(fonts: &[InstalledFont], family: &str, file: &str) -> Vec<String> {
    let mut files: Vec<String> = fonts
        .iter()
        .filter(|f| f.family == family && (file.is_empty() || f.file == file))
        .map(|f| f.file.clone())
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Новый список выключенных файлов: `files` добавлены (`enable = false`)
/// или убраны. По алфавиту, без повторов.
pub fn font_disabled_after(current: &[String], files: &[String], enable: bool) -> Vec<String> {
    let mut out: Vec<String> = current.iter().filter(|f| !files.contains(f)).cloned().collect();
    if !enable {
        out.extend(files.iter().cloned());
    }
    out.sort();
    out.dedup();
    out
}

/// Чем кончилось включение или выключение шрифта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontToggleOutcome {
    /// Выключен (семейство).
    Disabled(String),
    /// Включён (семейство).
    Enabled(String),
    /// Семейство выбрано шрифтом системы — выключать нельзя.
    InUse(String),
    /// Выключились бы вообще все шрифты.
    LastFonts,
    /// Имя файла читалось бы как шаблон (`*`, `?`).
    UnsafeName(String),
    /// Шрифт не нашёлся (список устарел).
    NotFound(String),
    /// `fonts.conf` не разбирается или наш блок в нём разорван руками.
    Refused(String),
    /// `fonts.conf` не записался.
    Failed(String),
}

/// Запрет на выключение, если он есть: семейство, выбранное шрифтом
/// системы (`in_use` — семейства из настроек окружения), пропало бы
/// целиком; или не осталось бы ни одного включённого шрифта. `disabled` —
/// список выключенных ПОСЛЕ переключения.
pub fn font_disable_refusal(fonts: &[InstalledFont], disabled: &[String], in_use: &[String]) -> Option<FontToggleOutcome> {
    let gone = |f: &InstalledFont| disabled.contains(&f.file);
    for name in in_use.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        let faces: Vec<&InstalledFont> = fonts.iter().filter(|f| f.family.eq_ignore_ascii_case(name)).collect();
        if !faces.is_empty() && faces.iter().all(|f| gone(f)) && faces.iter().any(|f| !f.disabled) {
            return Some(FontToggleOutcome::InUse(faces[0].family.clone()));
        }
    }
    if !fonts.is_empty() && fonts.iter().all(gone) {
        return Some(FontToggleOutcome::LastFonts);
    }
    None
}

/// Итог переключения — для человека.
pub fn font_toggle_status(outcome: &FontToggleOutcome) -> String {
    match outcome {
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::Disabled(family) => trf(
            "⏸️ Font disabled: %1. Apps stop seeing it after a restart; you can turn it back on here",
            &[family],
        ),
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::Enabled(family) => trf("✅ Font enabled: %1. Apps will see it after a restart", &[family]),
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::InUse(family) => trf(
            "⚠️ %1 is set as a system font right now — choose another one first, then disable this one",
            &[family],
        ),
        FontToggleOutcome::LastFonts => tr("⚠️ You can't disable every font — text in apps would become unreadable"),
        // TRANSLATORS: %1 is a font file name.
        FontToggleOutcome::UnsafeName(name) => trf(
            "⚠️ The file name %1 contains * or ?, which fontconfig reads as a wildcard and could disable other fonts too — nothing was changed",
            &[name],
        ),
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::NotFound(family) => trf("⚠️ Font %1 wasn't found — the list has been refreshed", &[family]),
        FontToggleOutcome::Refused(path) => tf("font.fc_refused", &[path]),
        // TRANSLATORS: %1 is the path of the fontconfig file (fonts.conf).
        FontToggleOutcome::Failed(path) => trf("⚠️ Could not write %1", &[path]),
    }
}

/// Вопрос перед выключением — только если в тех же файлах лежат и другие
/// семейства (`.ttc`): они выключатся вместе. Иначе `None` — выключение
/// обратимо, спрашивать незачем.
pub fn font_disable_confirm(family: &str, families: &[String]) -> Option<(String, String)> {
    let others: Vec<&str> = families.iter().filter(|f| *f != family).map(String::as_str).collect();
    if others.is_empty() {
        return None;
    }
    // TRANSLATORS: %1 is a font family, %2 a comma-separated list of other font families.
    let text = trf(
        "Disable the font %1? The same files also hold %2 — they will be disabled too.",
        &[&family, &others.join(", ")],
    );
    Some((tr("Disable Font"), text))
}

// ── Свои группы шрифтов: `fontgroups.xml`, общий с KDE ──────────────────
//
// Группы хранятся там же и в том же виде, что у «Управления шрифтами» KDE
// Plasma: `$XDG_CONFIG_HOME/fontgroups.xml`, корень `<groups>`, в нём
// `<group name="…">` со списком `<family>…</family>`. Группа — это только
// имена семейств: файлы шрифтов не трогаются, и группа, созданная в Rexon,
// видна в KDE и наоборот. Семейство, которого сейчас нет в системе, из
// группы не выбрасывается — его могли удалить на время или поставить позже.
//
// Встроенные группы («Все», «Личные», «Системные», «Неклассифицированные»)
// в файл не пишутся: это фильтры списка. «Неклассифицированные» — семейства,
// не вошедшие ни в одну свою группу; как и в KDE, они предлагаются, только
// когда своя группа есть хоть одна.
//
// Файл перечитывается перед каждой правкой: группы могли поменять в KDE, и
// запись по устаревшему списку стёрла бы чужое. Файл, который не
// разбирается, не переписывается вовсе.

/// Файл групп относительно `$XDG_CONFIG_HOME`.
pub const FONT_GROUPS_REL_PATH: &str = "fontgroups.xml";

/// Приставка значения фильтра «место» для своей группы: `group:Имя`.
pub const FONT_GROUP_ORIGIN_PREFIX: &str = "group:";

/// Самое длинное имя группы (символов).
pub const FONT_GROUP_NAME_MAX: usize = 100;

/// Своя группа шрифтов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontGroup {
    pub name: String,
    /// Семейства по алфавиту, без повторов.
    pub families: BTreeSet<String>,
}

/// Разобрать `fontgroups.xml`. `Err` — файл не XML: такой файл не
/// переписывается, чтобы не стереть группы, которые в нём были. Группа без
/// имени пропускается, одноимённые сливаются (как при чтении в KDE).
pub fn parse_font_groups(text: &str) -> Result<Vec<FontGroup>, ()> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut groups: Vec<FontGroup> = Vec::new();
    // Глубина: 1 — корень, 2 — `<group>`, 3 — `<family>`.
    let mut depth = 0usize;
    let mut group: Option<usize> = None;
    let mut family: Option<String> = None;
    let mut seen_root = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                if depth == 1 {
                    seen_root = true;
                }
                let name = e.name();
                if depth == 2 && name.as_ref() == b"group" {
                    group = font_group_attr_name(&e)?.map(|n| font_group_slot(&mut groups, n));
                } else if depth == 3 && group.is_some() && name.as_ref() == b"family" {
                    family = Some(String::new());
                }
            }
            Ok(Event::Empty(e)) => {
                if depth == 0 {
                    seen_root = true;
                } else if depth == 1 && e.name().as_ref() == b"group" {
                    if let Some(n) = font_group_attr_name(&e)? {
                        font_group_slot(&mut groups, n);
                    }
                }
            }
            Ok(Event::End(_)) => {
                if depth == 3 {
                    if let (Some(g), Some(f)) = (group, family.take()) {
                        let f = f.trim();
                        if !f.is_empty() {
                            groups[g].families.insert(f.to_string());
                        }
                    }
                } else if depth == 2 {
                    group = None;
                }
                depth = depth.checked_sub(1).ok_or(())?;
            }
            Ok(Event::Text(e)) => {
                if let Some(f) = family.as_mut() {
                    f.push_str(&e.decode().map_err(|_| ())?);
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(f) = family.as_mut() {
                    f.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(f) = family.as_mut() {
                    let ch = match e.resolve_char_ref().map_err(|_| ())? {
                        Some(c) => c,
                        None => match e.decode().map_err(|_| ())?.as_ref() {
                            "lt" => '<',
                            "gt" => '>',
                            "amp" => '&',
                            "quot" => '"',
                            "apos" => '\'',
                            _ => return Err(()),
                        },
                    };
                    f.push(ch);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
    if depth != 0 || (!seen_root && !text.trim().is_empty()) {
        return Err(());
    }
    Ok(groups)
}

/// Атрибут `name` у `<group>` (пустое имя — нет имени).
fn font_group_attr_name(e: &quick_xml::events::BytesStart) -> Result<Option<String>, ()> {
    for attr in e.attributes() {
        let attr = attr.map_err(|_| ())?;
        if attr.key.as_ref() == b"name" {
            let v = attr.unescape_value().map_err(|_| ())?;
            return Ok((!v.is_empty()).then(|| v.into_owned()));
        }
    }
    Ok(None)
}

/// Номер группы `name` в списке; нет — заводится.
fn font_group_slot(groups: &mut Vec<FontGroup>, name: String) -> usize {
    match groups.iter().position(|g| g.name == name) {
        Some(i) => i,
        None => {
            groups.push(FontGroup { name, families: BTreeSet::new() });
            groups.len() - 1
        }
    }
}

/// Текст `fontgroups.xml` — в том виде, в каком его пишет KDE.
pub fn font_groups_xml(groups: &[FontGroup]) -> String {
    let esc = crate::system_l10n::xml_escape;
    let mut out = String::from("<groups>\n");
    for g in groups {
        out.push_str(&format!(" <group name=\"{}\">\n", esc(&g.name)));
        for f in &g.families {
            out.push_str(&format!("  <family>{}</family>\n", esc(f)));
        }
        out.push_str(" </group>\n");
    }
    out.push_str("</groups>\n");
    out
}

/// Имя группы, как его ввёл человек, — в виде для файла: переводы строк и
/// табуляции становятся пробелами, пробелы по краям и повторные снимаются,
/// длина — не больше [`FONT_GROUP_NAME_MAX`].
pub fn font_group_name_clean(raw: &str) -> String {
    let joined = raw.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    joined.chars().take(FONT_GROUP_NAME_MAX).collect()
}

/// Имена встроенных групп: занять их своей группой нельзя (в KDE тоже —
/// там встроенные группы стоят в одном списке со своими).
fn font_group_reserved(name: &str) -> bool {
    let mut names: Vec<String> = ["All", "Personal", "System", "Unclassified"].iter().map(|s| s.to_string()).collect();
    names.extend(font_origin_choices().into_iter().map(|(_, label)| label));
    names.push(trc("font origin", "Unclassified"));
    names.iter().any(|n| n == name)
}

/// Правка своих групп.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontGroupEdit {
    Create(String),
    Rename { from: String, to: String },
    Remove(String),
    /// Добавить семейства в группу.
    Add { group: String, families: Vec<String> },
    /// Убрать семейства из группы (сами шрифты остаются).
    Take { group: String, families: Vec<String> },
}

/// Чем кончилась правка групп.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontGroupOutcome {
    Created(String),
    Renamed { from: String, to: String },
    Removed(String),
    /// Добавлено столько-то семейств.
    Added { group: String, count: usize },
    /// Убрано столько-то семейств.
    Taken { group: String, count: usize },
    /// Все семейства уже были в группе.
    AlreadyIn(String),
    /// Ни одного из семейств в группе не было.
    NotIn(String),
    EmptyName,
    Exists(String),
    Reserved(String),
    NotFound(String),
    /// `fontgroups.xml` не разбирается — не тронут.
    Refused(String),
    /// `fontgroups.xml` не записался.
    Failed(String),
}

impl FontGroupOutcome {
    /// Поменяла ли правка список групп (его надо записать).
    pub fn changed(&self) -> bool {
        matches!(
            self,
            FontGroupOutcome::Created(_)
                | FontGroupOutcome::Renamed { .. }
                | FontGroupOutcome::Removed(_)
                | FontGroupOutcome::Added { .. }
                | FontGroupOutcome::Taken { .. }
        )
    }
}

/// Применить правку: новый список групп и итог. При отказе список тот же.
/// Новая группа встаёт в конец, переименованная — на своё место.
pub fn font_groups_apply(groups: &[FontGroup], edit: &FontGroupEdit) -> (Vec<FontGroup>, FontGroupOutcome) {
    let mut out = groups.to_vec();
    let find = |out: &Vec<FontGroup>, name: &str| out.iter().position(|g| g.name == name);
    let outcome = match edit {
        FontGroupEdit::Create(raw) => {
            let name = font_group_name_clean(raw);
            if name.is_empty() {
                FontGroupOutcome::EmptyName
            } else if font_group_reserved(&name) {
                FontGroupOutcome::Reserved(name)
            } else if find(&out, &name).is_some() {
                FontGroupOutcome::Exists(name)
            } else {
                out.push(FontGroup { name: name.clone(), families: BTreeSet::new() });
                FontGroupOutcome::Created(name)
            }
        }
        FontGroupEdit::Rename { from, to } => {
            let to = font_group_name_clean(to);
            match find(&out, from) {
                None => FontGroupOutcome::NotFound(from.clone()),
                Some(_) if to.is_empty() => FontGroupOutcome::EmptyName,
                Some(_) if to == *from => FontGroupOutcome::Renamed { from: from.clone(), to },
                Some(_) if font_group_reserved(&to) => FontGroupOutcome::Reserved(to),
                Some(_) if find(&out, &to).is_some() => FontGroupOutcome::Exists(to),
                Some(i) => {
                    out[i].name = to.clone();
                    FontGroupOutcome::Renamed { from: from.clone(), to }
                }
            }
        }
        FontGroupEdit::Remove(name) => match find(&out, name) {
            None => FontGroupOutcome::NotFound(name.clone()),
            Some(i) => {
                out.remove(i);
                FontGroupOutcome::Removed(name.clone())
            }
        },
        FontGroupEdit::Add { group, families } => match find(&out, group) {
            None => FontGroupOutcome::NotFound(group.clone()),
            Some(i) => {
                let count = families
                    .iter()
                    .map(|f| f.trim())
                    .filter(|f| !f.is_empty())
                    .filter(|f| out[i].families.insert(f.to_string()))
                    .count();
                if count == 0 {
                    FontGroupOutcome::AlreadyIn(group.clone())
                } else {
                    FontGroupOutcome::Added { group: group.clone(), count }
                }
            }
        },
        FontGroupEdit::Take { group, families } => match find(&out, group) {
            None => FontGroupOutcome::NotFound(group.clone()),
            Some(i) => {
                let count = families.iter().filter(|f| out[i].families.remove(f.trim())).count();
                if count == 0 {
                    FontGroupOutcome::NotIn(group.clone())
                } else {
                    FontGroupOutcome::Taken { group: group.clone(), count }
                }
            }
        },
    };
    if outcome.changed() {
        (out, outcome)
    } else {
        (groups.to_vec(), outcome)
    }
}

/// Итог правки групп — для человека.
pub fn font_group_status(outcome: &FontGroupOutcome) -> String {
    match outcome {
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::Created(name) => trf("✅ Group %1 created — drag fonts onto it or use “Add to Group”", &[name]),
        // TRANSLATORS: %1 is the old name of a font group, %2 the new one.
        FontGroupOutcome::Renamed { from, to } => trf("✅ Group %1 renamed to %2", &[from, to]),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::Removed(name) => trf("🗑️ Group %1 removed; its fonts stay installed", &[name]),
        FontGroupOutcome::Added { group, count } => crate::i18n::fill(
            // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
            &ntr("✅ %n font family added to %1", "✅ %n font families added to %1", *count as u64),
            &[group],
        ),
        FontGroupOutcome::Taken { group, count } => crate::i18n::fill(
            // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
            &ntr(
                "✅ %n font family removed from %1; the font stays installed",
                "✅ %n font families removed from %1; the fonts stay installed",
                *count as u64,
            ),
            &[group],
        ),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::AlreadyIn(name) => trf("ℹ️ Already in group %1", &[name]),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::NotIn(name) => trf("ℹ️ Not in group %1", &[name]),
        FontGroupOutcome::EmptyName => tr("⚠️ Type a name for the group"),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::Exists(name) => trf("⚠️ A group named %1 already exists", &[name]),
        // TRANSLATORS: %1 is a name the user typed for a new font group.
        FontGroupOutcome::Reserved(name) => trf("⚠️ %1 is a built-in group — choose another name", &[name]),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::NotFound(name) => trf("⚠️ Group %1 wasn't found — the list has been refreshed", &[name]),
        // TRANSLATORS: %1 is the path of the font groups file.
        FontGroupOutcome::Refused(path) => trf(
            "⚠️ %1 can't be read (it may have been edited by hand) — groups were left untouched",
            &[path],
        ),
        // TRANSLATORS: %1 is the path of the font groups file.
        FontGroupOutcome::Failed(path) => trf("⚠️ Could not write %1", &[path]),
    }
}

/// Вопрос перед удалением группы: `(заголовок, текст)`.
pub fn font_group_remove_confirm(name: &str) -> (String, String) {
    (
        tr("Remove Group"),
        // TRANSLATORS: %1 is the name of a font group.
        trf("Remove the group %1? Only the group goes away — its fonts stay installed.", &[&name]),
    )
}

/// Варианты фильтра «место» вместе со своими группами: `(значение,
/// подпись, своя ли группа)`. Встроенные — первыми; «Неклассифицированные»
/// и свои группы (по алфавиту без учёта регистра) — только если своя группа
/// есть хоть одна.
pub fn font_origin_choices_with_groups(groups: &[FontGroup]) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> =
        font_origin_choices().into_iter().map(|(id, label)| (id.to_string(), label, false)).collect();
    if groups.is_empty() {
        return out;
    }
    out.push(("unclassified".to_string(), trc("font origin", "Unclassified"), false));
    let mut names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    out.extend(names.into_iter().map(|n| (format!("{FONT_GROUP_ORIGIN_PREFIX}{n}"), n.to_string(), true)));
    out
}

/// Своя группа из значения фильтра «место» (`group:Имя`).
pub fn font_group_of_origin(origin: &str) -> Option<&str> {
    origin.strip_prefix(FONT_GROUP_ORIGIN_PREFIX)
}

/// Свои группы, в которые входит семейство (по алфавиту).
pub fn font_groups_of_family(groups: &[FontGroup], family: &str) -> Vec<String> {
    let mut out: Vec<String> = groups.iter().filter(|g| g.families.contains(family)).map(|g| g.name.clone()).collect();
    out.sort_by_key(|n| n.to_lowercase());
    out
}

/// [`font_group_view_filtered`] со своими группами: «место» бывает ещё
/// `unclassified` (семейства вне всех своих групп) и `group:Имя` (семейства
/// этой группы, со всеми начертаниями). Неизвестная группа — пустой список.
pub fn font_group_view_in(groups: &[FontFamilyGroup], filter: &FontListFilter, custom: &[FontGroup]) -> Vec<FontFamilyGroup> {
    let keep = |family: &str| -> bool {
        if filter.origin == "unclassified" {
            return !custom.iter().any(|g| g.families.contains(family));
        }
        match font_group_of_origin(&filter.origin) {
            Some(name) => custom.iter().any(|g| g.name == name && g.families.contains(family)),
            None => true,
        }
    };
    let custom_origin = filter.origin == "unclassified" || font_group_of_origin(&filter.origin).is_some();
    let inner = if custom_origin { FontListFilter { origin: "all".to_string(), ..filter.clone() } } else { filter.clone() };
    let picked: Vec<FontFamilyGroup> = groups.iter().filter(|g| keep(&g.family)).cloned().collect();
    font_group_view_filtered(&picked, &inner)
}

/// Файлы шрифтов группы — для включения, выключения и экспорта. По
/// алфавиту, без повторов.
pub fn font_group_files(fonts: &[InstalledFont], group: &FontGroup) -> Vec<String> {
    let mut files: Vec<String> =
        fonts.iter().filter(|f| group.families.contains(&f.family)).map(|f| f.file.clone()).collect();
    files.sort();
    files.dedup();
    files
}

/// Включение или выключение шрифтов группы целиком — для человека.
/// `families` — сколько установленных семейств группы затронуто.
pub fn font_group_toggle_status(group: &str, families: usize, enabled: bool) -> String {
    let template = if enabled {
        // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
        ntr(
            "✅ Group %1: %n font family enabled. Apps will see it after a restart",
            "✅ Group %1: %n font families enabled. Apps will see them after a restart",
            families as u64,
        )
    } else {
        // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
        ntr(
            "⏸️ Group %1: %n font family disabled. Apps stop seeing it after a restart; you can turn it back on here",
            "⏸️ Group %1: %n font families disabled. Apps stop seeing them after a restart; you can turn them back on here",
            families as u64,
        )
    };
    crate::i18n::fill(&template, &[&group])
}

/// В группе нет ни одного установленного шрифта.
pub fn font_group_empty_status(group: &str) -> String {
    // TRANSLATORS: %1 is the name of a font group.
    trf("ℹ️ Group %1 has no installed fonts yet", &[&group])
}

/// Вопрос перед выключением группы — только если в тех же файлах лежат
/// семейства не из группы (`.ttc`): они выключатся вместе.
pub fn font_group_disable_confirm(group: &str, others: &[String]) -> Option<(String, String)> {
    if others.is_empty() {
        return None;
    }
    // TRANSLATORS: %1 is the name of a font group, %2 a comma-separated list of font families.
    let text = trf(
        "Disable the fonts of group %1? The same files also hold %2 — they will be disabled too.",
        &[&group, &others.join(", ")],
    );
    Some((tr("Disable Fonts in Group"), text))
}

/// Что положить в архив группы: `(имя в архиве, файл)`. Шрифты Type 1
/// берут с собой файлы метрик рядом (`.afm`, `.pfm` — то же имя), если они
/// есть (`exists`). Одинаковые имена из разных каталогов не затирают друг
/// друга: второй файл получает приставку `-2`, третий `-3`…
pub fn font_group_export_entries(files: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut paths: Vec<String> = Vec::new();
    for file in files {
        if !paths.contains(file) {
            paths.push(file.clone());
        }
        let lower = file.to_lowercase();
        if lower.ends_with(".pfa") || lower.ends_with(".pfb") {
            let stem = &file[..file.len() - 4];
            for ext in ["afm", "AFM", "pfm", "PFM"] {
                let side = format!("{stem}.{ext}");
                if !paths.contains(&side) && exists(&side) {
                    paths.push(side);
                }
            }
        }
    }
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for path in paths {
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let (stem, ext) = match name.find('.') {
            Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
            _ => (name.clone(), String::new()),
        };
        let mut entry = name.clone();
        let mut n = 2;
        while taken.contains(&entry.to_lowercase()) {
            entry = format!("{stem}-{n}{ext}");
            n += 1;
        }
        taken.insert(entry.to_lowercase());
        out.push((entry, path));
    }
    out
}

/// Имя архива группы по умолчанию: имя группы без `/` и управляющих
/// символов, `.zip`.
pub fn font_group_export_name(group: &str) -> String {
    let clean: String = group.chars().map(|c| if c == '/' || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.');
    format!("{}.zip", if clean.is_empty() { "fonts" } else { clean })
}

/// Путь, выбранный в окне сохранения, — с `.zip` на конце.
pub fn font_group_export_path(chosen: &str) -> String {
    if chosen.to_lowercase().ends_with(".zip") {
        chosen.to_string()
    } else {
        format!("{chosen}.zip")
    }
}

/// Заголовок и фильтр окна сохранения архива группы.
pub fn font_group_export_dialog() -> (String, String) {
    (tr("Export Group"), format!("{} (*.zip)", tr("ZIP archive")))
}

/// Записать архив группы: сначала во временный файл рядом, затем
/// переименованием — недописанный архив под нужным именем не остаётся.
/// Вернёт, сколько файлов легло в архив.
pub fn write_font_group_zip(entries: &[(String, String)], dest: &Path) -> std::io::Result<usize> {
    use std::io::Write as _;
    let part = dest.with_extension("zip.part");
    let result = (|| -> std::io::Result<usize> {
        let mut zip = zip::ZipWriter::new(fs::File::create(&part)?);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, path) in entries {
            let data = fs::read(path)?;
            // Дата файла в архиве — его время изменения (без неё было бы
            // 1 января 1980-го).
            let mtime = fs::metadata(path).and_then(|m| m.modified()).ok().and_then(zip_date_of);
            let options = match mtime {
                Some(t) => options.last_modified_time(t),
                None => options,
            };
            zip.start_file(name.as_str(), options).map_err(std::io::Error::other)?;
            zip.write_all(&data)?;
        }
        zip.finish().map_err(std::io::Error::other)?;
        Ok(entries.len())
    })();
    match result {
        Ok(n) => {
            fs::rename(&part, dest)?;
            Ok(n)
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Время файла для записи ZIP — местное, как у архиваторов (ZIP хранит
/// дату без пояса). Вне 1980–2107 — `None`.
fn zip_date_of(t: std::time::SystemTime) -> Option<zip::DateTime> {
    let secs = libc::time_t::try_from(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
    // SAFETY: `localtime_r` пишет только в переданную структуру.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return None;
        }
        tm
    };
    zip::DateTime::from_date_and_time(
        u16::try_from(tm.tm_year + 1900).ok()?,
        u8::try_from(tm.tm_mon + 1).ok()?,
        u8::try_from(tm.tm_mday).ok()?,
        u8::try_from(tm.tm_hour).ok()?,
        u8::try_from(tm.tm_min).ok()?,
        u8::try_from(tm.tm_sec).ok()?,
    )
    .ok()
}

/// Итог экспорта группы — для человека. `Ok(n)` — сколько файлов в архиве.
pub fn font_group_export_status(group: &str, dest: &str, result: &Result<usize, String>) -> String {
    match result {
        Ok(n) => crate::i18n::fill(
            // TRANSLATORS: %n is the number of font files, %1 the name of a font group, %2 the archive path.
            &ntr("📦 Group %1 exported: %n font file → %2", "📦 Group %1 exported: %n font files → %2", *n as u64),
            &[&group, &dest],
        ),
        // TRANSLATORS: %1 is the archive path, %2 the error from the system.
        Err(e) => trf("⚠️ Could not write %1: %2", &[&dest, e]),
    }
}

// ── Дубликаты ───────────────────────────────────────────────────────────

/// argv списка для поиска дубликатов: только масштабируемые шрифты.
/// Растровые семейства намеренно лежат многими файлами (кегль, кодировка —
/// `misc/7x13.pcf.gz` и `misc/8x13.pcf.gz` у «Misc Fixed Regular»), это не
/// копии.
pub fn fc_list_scalable_argv() -> Vec<String> {
    let mut argv = fc_list_fonts_argv();
    argv.insert(1, ":scalable=true".to_string());
    argv
}

/// Одно начертание, найденное в нескольких файлах.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontDuplicate {
    pub family: String,
    pub style: String,
    /// Копии: сперва системные, потом свои; внутри — по пути.
    pub copies: Vec<InstalledFont>,
}

/// Найти дубликаты: одно семейство и начертание в нескольких РАЗНЫХ файлах
/// (`real_path` разворачивает ссылки — два пути к одному файлу не копии).
/// Выключенные не считаются: программы их и так не видят.
pub fn find_font_duplicates(fonts: &[InstalledFont], real_path: &dyn Fn(&str) -> String) -> Vec<FontDuplicate> {
    let mut out: Vec<FontDuplicate> = Vec::new();
    for f in fonts.iter().filter(|f| !f.disabled) {
        match out.iter_mut().find(|d| d.family == f.family && d.style == f.style) {
            Some(d) => d.copies.push(f.clone()),
            None => out.push(FontDuplicate { family: f.family.clone(), style: f.style.clone(), copies: vec![f.clone()] }),
        }
    }
    for d in &mut out {
        let mut seen = BTreeSet::new();
        d.copies.retain(|c| seen.insert(real_path(&c.file)));
        d.copies.sort_by(|a, b| a.user.cmp(&b.user).then(a.file.cmp(&b.file)));
    }
    out.retain(|d| d.copies.len() > 1);
    out.sort_by(|a, b| {
        a.family
            .to_lowercase()
            .cmp(&b.family.to_lowercase())
            .then(style_rank(&a.style).cmp(&style_rank(&b.style)))
    });
    out
}

/// Сводка над списком дубликатов.
pub fn font_duplicates_summary(found: usize) -> String {
    if found == 0 {
        return tr("No duplicates: every font style is installed once");
    }
    ntr(
        // TRANSLATORS: %n is the number of font styles found in more than one file.
        "%n style is installed more than once",
        "%n styles are installed more than once",
        found as u64,
    )
}

// ============================================================================
// Защита приложений по умолчанию и копии панели Plasma
// ============================================================================

/// Читаемое имя из id ярлыка: `org.kde.dolphin.desktop` → `Dolphin`.
/// Пустая строка — «нечего показывать».
pub fn pretty_desktop_id(app_id: &str) -> String {
    let app_id = app_id.trim();
    if app_id.is_empty() || app_id == "—" || app_id == "-" {
        return String::new();
    }
    let stem = app_id.split(".desktop").next().unwrap_or(app_id);
    let name = stem.rsplit('.').next().unwrap_or(stem);
    if name.is_empty() {
        return app_id.to_string();
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => app_id.to_string(),
    }
}

/// Метка времени копии по-человечески: `20260620_011005` →
/// «20.06.2026 в 01:10» (по-английски «20.06.2026 at 01:10»; порядок частей
/// даты решает перевод). Не разобралось — отдаём как есть.
pub fn pretty_backup_ts(ts: &str) -> String {
    let bytes = ts.as_bytes();
    if bytes.len() != 15 || bytes[8] != b'_' {
        return ts.to_string();
    }
    let digits = |from: usize, to: usize| -> Option<&str> {
        let part = ts.get(from..to)?;
        if part.bytes().all(|b| b.is_ascii_digit()) {
            Some(part)
        } else {
            None
        }
    };
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (
        digits(0, 4),
        digits(4, 6),
        digits(6, 8),
        digits(9, 11),
        digits(11, 13),
        digits(13, 15),
    ) else {
        return ts.to_string();
    };
    // Разбор строгий, как у `datetime.strptime`: несуществующая дата остаётся
    // как есть, а не превращается в «32.13.2026». Длину месяца тоже
    // проверяем — 30 февраля датой не является.
    let year = y.parse::<u32>().unwrap_or(0);
    let month = mo.parse::<u32>().unwrap_or(0);
    let day = d.parse::<u32>().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    let ok = days_in_month > 0
        && (1..=days_in_month).contains(&day)
        && h.parse::<u32>().unwrap_or(99) <= 23
        && mi.parse::<u32>().unwrap_or(99) <= 59
        && s.parse::<u32>().unwrap_or(99) <= 60;
    if !ok {
        return ts.to_string();
    }
    // TRANSLATORS: date and time of a saved copy: %1 day, %2 month, %3 year,
    // %4 hours, %5 minutes (all with leading zeros). Reorder them if needed.
    trf("%1.%2.%3 at %4:%5", &[&d, &mo, &y, &h, &mi])
}

/// Подписи причин, по которым сделана копия панели Plasma.
pub fn backup_reason_label(reason: &str) -> String {
    match reason {
        // TRANSLATORS: why a copy of the Plasma panel was made.
        "before_install" => tr("automatically, before installing"),
        // TRANSLATORS: why a copy of the Plasma panel was made.
        "manual" => tr("manually"),
        other => other.to_string(),
    }
}

/// Строка одной копии панели: «📁 20.06.2026 в 01:10 · вручную · 3 файла».
pub fn backup_row_label(ts: &str, reason: &str, files: i64) -> String {
    format!(
        "📁 {}  ·  {}  ·  {}",
        pretty_backup_ts(ts),
        backup_reason_label(reason),
        // TRANSLATORS: number of files in a saved copy of the Plasma panel.
        ntr("%n file", "%n files", files.max(0) as u64)
    )
}

// ============================================================================
// Комплектность рабочего стола: итог проверки словами
// ============================================================================

/// Итог проверки комплектности одной строкой для человека.
///
/// `results` — пары «проверка → статус» (`fixed` / `failed` / `skipped` /
/// `needs_action` / что угодно ещё). Пустой набор значит «проверок нет».
pub fn compliance_summary(results: &[(String, String)]) -> String {
    if results.is_empty() {
        return tr("No checks for this desktop yet");
    }
    let count = |what: &str| results.iter().filter(|(_, v)| v == what).count();
    let fixed = count("fixed");
    let failed = count("failed");
    let skipped = count("skipped");
    let needs = count("needs_action");

    let mut parts: Vec<String> = Vec::new();
    if fixed > 0 {
        // TRANSLATORS: part of a list joined with commas; the first letter is
        // capitalized by the program. %1 is a number.
        parts.push(trf("installed: %1", &[&fixed]));
    }
    if failed > 0 {
        // TRANSLATORS: part of a list joined with commas. %1 is a number.
        parts.push(trf("failed: %1", &[&failed]));
    }
    if needs > 0 {
        // TRANSLATORS: part of a list joined with commas. %1 is a number.
        parts.push(trf("still to install: %1", &[&needs]));
    }
    if skipped > 0 {
        // TRANSLATORS: part of a list joined with commas.
        parts.push(tr("the package check is only available on Arch/CachyOS"));
    }
    if parts.is_empty() {
        return trf("✅ Everything is in place (items checked: %1)", &[&results.len()]);
    }
    let joined = parts.join(", ");
    // Первая буква заглавная — фраза начинается с неё.
    let mut chars = joined.chars();
    let joined = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => joined,
    };
    if failed > 0 || needs > 0 {
        format!("⚠️ {joined}")
    } else {
        format!("✅ {joined}")
    }
}

// ============================================================================
// Обои с YouTube: значения выпадающих списков
// ============================================================================

/// Пункты списка качества: (подпись, значение для движка).
///
/// Подпись — `msgid` (числа переводить нечего); переводит её место показа —
/// [`yt_qualities`] либо строка хода [`youtube_starting_status`].
pub const YT_QUALITIES: &[(&str, &str)] = &[
    ("1080p", "1080p"),
    ("720p", "720p"),
    ("1440p", "1440p"),
    ("4K", "4k"),
    // TRANSLATORS: YouTube video quality: the best one available.
    (n_("Best available"), "best"),
];

/// Пункты списка режима: (подпись, значение для движка). Подпись — `msgid`.
pub const YT_MODES: &[(&str, &str)] = &[
    // TRANSLATORS: YouTube wallpaper mode: play the video online.
    (n_("Stream"), "stream"),
    // TRANSLATORS: YouTube wallpaper mode: download the video first.
    (n_("Download"), "download"),
];

/// Список качества для показа: (переведённая подпись, значение для движка).
pub fn yt_qualities() -> Vec<(String, &'static str)> {
    YT_QUALITIES.iter().map(|(l, v)| (tr(l), *v)).collect()
}

/// Список режима для показа: (переведённая подпись, значение для движка).
pub fn yt_modes() -> Vec<(String, &'static str)> {
    YT_MODES.iter().map(|(l, v)| (tr(l), *v)).collect()
}

// ============================================================================
// Имя текущего окружения для подписей
// ============================================================================

/// Как назвать окружение в подписи секции.
///
/// `system.py` не опознаёт niri/sway и отдаёт «unknown» — тогда берём токен
/// из `XDG_CURRENT_DESKTOP`/`DESKTOP_SESSION`, чтобы покупатель не читал
/// «UNKNOWN» (правило #13). Не нашлось и там — «ВАШ РАБОЧИЙ СТОЛ»:
/// сокращение «WM» покупателю ничего не говорит (правило #13).
pub fn de_display(de: &str, xdg_current_desktop: &str, desktop_session: &str) -> String {
    if !de.is_empty() && de != "unknown" {
        // Имя стола — то же, что на вкладке «Рабочие столы» и в остальных
        // секциях («KDE Plasma», а не «KDE»): одно окружение под двумя
        // именами в одном окне читается как два разных.
        let known = crate::qt_desktops::display_name(de);
        return if known == de { de.to_uppercase() } else { known.to_string() };
    }
    let raw = if !xdg_current_desktop.is_empty() {
        xdg_current_desktop
    } else {
        desktop_session
    };
    let token = raw.split(':').next().unwrap_or("").trim();
    if token.is_empty() {
        // Имя одно на продукт; в фразу оно попадает только через
        // `screenshots::place_de_name`, иначе падеж ломается.
        crate::screenshots::unknown_de_name()
    } else {
        token.to_uppercase()
    }
}

// ============================================================================
// Тексты вкладки
// ============================================================================

/// Подпись кнопки и заголовок окна выбора шрифта: (значок+подпись кнопки,
/// подсказка, заголовок окна).
pub fn font_target_texts(target: &str) -> (String, String, String) {
    match target {
        "mono" => (
            tr("🔠 Monospace…"),
            tr("Font for the terminal and code editors"),
            tr("Monospace Font"),
        ),
        "small" => (
            tr("🔡 Small…"),
            tr("Font for the smallest text: captions, hints, status bars"),
            tr("Small Font"),
        ),
        "toolbar" => (
            tr("🧰 Toolbar…"),
            tr("Font of toolbar button labels"),
            tr("Toolbar Font"),
        ),
        "menu" => (
            tr("📋 Menu…"),
            tr("Font of menu bars and menus"),
            tr("Menu Font"),
        ),
        "title" => (
            tr("🪟 Window Titles…"),
            tr("Font in window title bars"),
            tr("Window Title Font"),
        ),
        _ => (
            tr("🔤 System…"),
            tr("Font for buttons, menus and text in apps"),
            tr("System Font"),
        ),
    }
}

/// Строка «что сейчас на рабочем столе» для секции обоев.
///
/// `running` — играют ли живые обои; `format` и `path` — что именно.
/// `None` значит «движок про это вообще не сказал» (ключа в снимке нет) —
/// только тогда в строке появляется «?».
///
/// Починка (правило #13): формат показывался служебным словом движка —
/// покупатель читал «Активны живые обои: video». Имя для человека даёт общий
/// [`crate::wallpaper_engine::format_label`], которым подписаны и остальные
/// сообщения об обоях, — второй таблицы имён у вкладки нет.
pub fn wallpaper_status_text(running: bool, format: Option<&str>, path: Option<&str>) -> String {
    if !running {
        return tr("<b>Current wallpaper:</b> static (or system)");
    }
    let name = path
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();
    let kind = match format {
        Some(fmt) => crate::wallpaper_engine::format_label(fmt),
        None => "?".to_string(),
    };
    // TRANSLATORS: %1 is the wallpaper kind (video, Flash…), %2 is the file name.
    trf("<b>▶️ Live wallpaper is on:</b> %1 — %2", &[&kind, &name])
}

/// Итог восстановления родных приложений окружения.
///
/// `restored` — сколько вернули; `missing` — подписи категорий, для которых
/// в системе не нашлось ни одного варианта. Молчать про них нельзя: раньше
/// человек видел «✅ Восстановлено 2 приложений» и не догадывался, что
/// терминал не восстановился вовсе (правило #13).
pub fn restore_native_message(restored: usize, missing: &[String], de_display: &str) -> String {
    // Починка (правило #13): счёт стоял с одним окончанием на все числа —
    // покупатель читал «Восстановлено 1 приложений». Форму по числу выбирает
    // каталог перевода ([`crate::i18n::ntr`]).
    let msg = crate::screenshots::place_de_name(
        // TRANSLATORS: %n is how many default apps were restored, %1 is the
        // name of the current desktop environment.
        &ntr("Restored %n app for %1", "Restored %n apps for %1", restored as u64),
        "%1",
        de_display,
    );
    if missing.is_empty() {
        return msg;
    }
    // TRANSLATORS: %1 is the sentence about restored apps, %2 is a
    // comma-separated list of app types that were not found.
    trf("%1, not found on the system: %2", &[&msg, &missing.join(", ")])
}

/// Строка состояния защиты приложений по умолчанию.
///
/// `remembered` — сколько приложений уже запомнено (эталон, с которым идёт
/// сверка); `changed` — тройки (подпись категории, что запомнено, что сейчас).
///
/// ⚠️ Починка (правило #13): прежде строка считала успехом ПУСТОЙ список
/// расхождений и на чистом профиле объявляла «✅ Всё на месте», хотя
/// запомнено не было ничего и сверять было не с чем. Покупатель читал
/// зелёную галку как «защита работает», а защищён не был никак. Теперь
/// зелёный ответ даётся ТОЛЬКО когда эталон снят; без эталона говорится
/// прямо, что подмену заметить не по чему, и называется кнопка, которая это
/// исправляет. Подпись кнопки берётся из каталога вкладки — второй копии
/// букв тут быть не должно, иначе совет разойдётся с окном.
pub fn protection_status_text(
    remembered: usize,
    changed: &[(String, String, String)],
) -> String {
    if remembered == 0 {
        let button = t("prot.snapshot");
        // TRANSLATORS: %1 is the label of the “Remember Now” button.
        return trf("<b>⚠️ Nothing to compare with yet</b><br>Rexon hasn't yet remembered \
                    which apps you use by default, so it won't notice if they get replaced. \
                    Click “%1”.", &[&button]);
    }
    if changed.is_empty() {
        return tr("<b>✅ Everything is in place</b><br>Your default apps match the remembered ones");
    }
    let names: Vec<String> = changed
        .iter()
        .map(|(label, saved, current)| format!("{label}: {saved} → {current}"))
        .collect();
    // TRANSLATORS: %1 is a list like “Browser: firefox → chromium”.
    trf("<b>⚠️ Default apps have changed:</b><br>%1", &[&names.join(";  ")])
}

/// Подпись строки активного фонового звука и его id по пути файла.
pub fn ambient_sound_id(path: &str) -> String {
    format!("manual_{}", path.rsplit('/').next().unwrap_or(path))
}

/// Фильтр окна выбора аудиофайла для фоновых звуков.
pub fn ambient_file_filter() -> String {
    format!(
        "{} (*.ogg *.oga *.mp3 *.wav *.flac *.opus *.m4a);;{} (*)",
        // TRANSLATORS: file type group in the “Choose an audio file” window.
        tr("Audio files"),
        // TRANSLATORS: file type group in a file chooser window.
        tr("All files"),
    )
}

// ============================================================================
// Секции вкладки: порядок показа, заголовки, описания
// ============================================================================

/// Все секции вкладки в порядке показа: (имя рамки, заголовок, описание).
///
/// Порядок — решение о том, что покупатель видит первым, и живёт здесь, а не
/// в цепочке вызовов Qt: раньше он читался только из тела `__init__`, и
/// переставить секцию можно было лишь правкой окна.
///
/// `%1` в описании — имя текущего окружения (см. [`section_texts`]).
/// Заголовки и описания — английские `msgid`; переводит их [`section_texts`].
/// Заголовок секции обучения берётся у самого обучения
/// ([`crate::onboarding::settings_label`]), поэтому в таблице он пустой.
pub const SECTIONS: &[(&str, &str, &str)] = &[
    (
        "langGroup",
        n_("🌐 Language"),
        n_("The language of Rexon's windows, menus and messages"),
    ),
    (
        "skinGroup",
        n_("🎨 Interface Skin"),
        n_("The look of the Rexon app itself (window colors). This is NOT the KDE system \
            theme — only Rexon's own look changes."),
    ),
    (
        "obGroup",
        "",
        n_("Short tips on top of the window: where things are and where to start"),
    ),
    (
        "trayGroup",
        n_("🔔 System Tray Icon"),
        n_("Rexon keeps an icon next to the clock: wallpapers, sections and quitting — \
            without opening the window. While the icon is there, the window's close \
            button hides Rexon to the tray instead of quitting it."),
    ),
    (
        "fmGroup",
        n_("📁 File Manager"),
        n_("The program that opens folders — both in Rexon itself and across the whole \
            system"),
    ),
    (
        "quickFix",
        n_("🔧 Quick App Restore"),
        n_("After you install another desktop environment, your default apps (terminal, \
            file manager, text editor) may be switched to its own. This button brings \
            back the native apps of the current desktop."),
    ),
    (
        "completeness",
        n_("🧩 Desktop Completeness"),
        n_("Rexon checks that everything the current desktop needs is in place: the right \
            themes, fonts and add-ons. At login the check only looks and installs \
            nothing, so it doesn't ask for a password at startup. Here you can install \
            what's missing (the system will ask for a password)."),
    ),
    (
        "wpGroup",
        n_("🎬 Wallpaper (image / video / YouTube / web)"),
        n_("Set a still image, a local video, YouTube/Vimeo or a web page as your desktop \
            wallpaper"),
    ),
    (
        "libraryGroup",
        n_("📚 Wallpaper Library"),
        n_("Wallpaper packs installed in Rexon (.rxn files). “Apply” puts the wallpaper \
            on your desktop; the packs themselves come from the OCS Theme Store or the \
            “Extensions” tab"),
    ),
    (
        "importGroup",
        n_("🪟 Import Windows Themes"),
        n_("Bring over the look of your old Windows PC: wallpapers, icons, cursors and \
            visual styles. Rexon converts them to its own format and installs them."),
    ),
    (
        "blurGroup",
        n_("🌫️ Wallpaper Blur"),
        n_("Blur an image wallpaper so icons and windows on top of it are easier to read. \
            The original image is kept — the blur comes off with one click"),
    ),
    (
        "ambientGroup",
        n_("🔊 Ambient Sounds"),
        n_("Sounds that play on a loop: rain, wind, music. Change the volume here or in \
            the system mixer"),
    ),
    (
        "soundThemeGroup",
        n_("🔔 Event Sounds"),
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("System sound theme for %1: clicks, notifications, errors. Rexon applies it \
            with the desktop's own tools"),
    ),
    (
        "mateIconsGroup",
        n_("🌽 MATE: Desktop Icons"),
        n_("Manage icons on the MATE desktop (Caja). Handy when Rexon wallpaper is on and \
            the icons get in the way"),
    ),
    (
        "fontsGroup",
        n_("🔤 System Fonts"),
        n_("Fonts of the interface, the terminal, small captions, toolbars, menus and window \
            titles, and how text is smoothed. Applies to the current desktop and to apps of \
            other toolkits as well"),
    ),
    (
        "protGroup",
        n_("🔒 Default App Protection"),
        n_("Makes sure your usual programs (browser, player, etc.) stay your default apps \
            when a new desktop is installed"),
    ),
    (
        "plasmaGroup",
        n_("🛡️ KDE Plasma Panel Protection"),
        n_("Saves the panel layout, widgets and keyboard shortcuts — and brings them back \
            if something breaks after installing another desktop"),
    ),
    (
        "aeroGroup",
        n_("🪟 Desktop Appearance"),
        n_("The desktop appearance is built for the system version that was installed at \
            the time. After a system update it has to be rebuilt — otherwise the panel, \
            menu and effects disappear without warning"),
    ),
];

/// Имена рамок секций в порядке показа.
pub fn section_order() -> Vec<&'static str> {
    SECTIONS.iter().map(|(id, _, _)| *id).collect()
}

/// Заголовок и описание секции на языке интерфейса. `de_display`
/// подставляется вместо `%1`.
/// Незнакомое имя рамки — `None` (зовущий обязан сказать об этом вслух, а не
/// рисовать пустую рамку).
pub fn section_texts(id: &str, de_display: &str) -> Option<(String, String)> {
    let (_, title, desc) = SECTIONS.iter().find(|(name, _, _)| *name == id)?;
    let title = if title.is_empty() {
        // Секция обучения подписана самим обучением — двух формулировок
        // «что это» у одной функции быть не должно.
        format!("🎓 {}", crate::onboarding::settings_label())
    } else {
        tr(title)
    };
    // ⭐ Починка: «Системная звуковая тема для ВАШ РАБОЧИЙ СТОЛ» — прямая
    // подстановка не склоняла имя неопознанного стола.
    Some((title, crate::screenshots::place_de_name(&tr(desc), "%1", de_display)))
}

// ============================================================================
// Каталог подписей и сообщений вкладки
// ============================================================================

/// Все постоянные строки вкладки: подписи, подсказки, сообщения о состоянии.
///
/// Один каталог вместо восьмидесяти одноразовых функций (правило #12): у всех
/// этих строк одна судьба — их читает Qt и показывает как есть, а решают
/// «какую именно» отдельные функции ниже. `%1`, `%2` — места подстановки,
/// подставляет [`text`].
///
/// ПЕРЕВОД. Шаблоны — английские `msgid` под маркером `n_`: таблицу читает
/// `xgettext`, а переводит её [`text_template`] в момент показа. Русский
/// живёт в `po/ru.po` дословно прежним текстом. Строки без слов (значки,
/// голое `%1`, имя значка темы) маркера не несут: переводить в них нечего, и
/// переводчик их не видит. Ключ — признак для кода, подпись — только
/// для человека: код сверяет КЛЮЧИ, а не текст.
pub const TEXTS: &[(&str, &str)] = &[
    // окно и общая часть
    ("window.title", n_("Rexon — Settings")),
    // «⚠️ Ошибка: …» — одна формулировка на всю вкладку: и на неудачу
    // движка обоев, и на исключение при записи настройки.
    ("status.error", n_("⚠️ Error: %1")),
    // Голое предупреждение: сообщение движка уже объясняет, что не вышло, и
    // слово «Ошибка» перед ним было бы вторым.
    ("status.warn", "⚠️ %1"),
    ("page.title", n_("⚙️ Settings")),
    // Ответы окна подтверждения. Виджетная вкладка брала их у Qt
    // (`QMessageBox::Yes/No`), но QML-страница показывает подтверждение
    // ВНУТРИ себя — с затемнением фона и цепочкой фокуса, как требует канон
    // Big Picture (диалоги операционной системы там неудобны). Значит и
    // подписи ответов живут там же, где остальные подписи вкладки.
    ("dialog.yes", n_("Yes")),
    ("dialog.no", n_("No")),
    ("polkit.banner", "⚠️ %1"),
    // Право изменять систему (Б13): те же подписи, что в меню значка
    // (`tray`, ключи `admin.*`) — одно и то же состояние.
    // Значок «🔓» ставит страница: тот же, что у пункта меню значка.
    (
        "admin.info",
        // TRANSLATORS: shown while Rexon may change the system without asking for the password again.
        n_("Rexon can change the system until the window is closed"),
    ),
    // TRANSLATORS: button/menu item: stop letting Rexon change the system; next time it asks for the password.
    ("admin.revoke", n_("Revoke Now")),
    // язык интерфейса
    ("lang.row", n_("<b>Language:</b>")),
    (
        "lang.system",
        // TRANSLATORS: first entry of the language list; %1 is the name of the
        // language the system is set to, in that language (“Deutsch”).
        n_("Same as system (%1)"),
    ),
    ("lang.restart_hint", n_("The language will change after Rexon restarts.")),
    ("lang.restart", n_("Restart Rexon")),
    (
        "lang.saved",
        // TRANSLATORS: %1 is the chosen language, in that language (“English”).
        n_("🌐 Language saved: %1"),
    ),
    ("lang.restart_failed", n_("⚠️ Could not restart Rexon: %1")),
    // скин интерфейса
    ("skin.row", n_("<b>Skin:</b>")),
    (
        "skin.hint",
        n_("💡 “System” uses your system's look as is. The other skins set their own \
            colors regardless of the system theme."),
    ),
    (
        "skin.applied",
        // TRANSLATORS: %1 is the skin name.
        n_("🎨 Interface skin applied: %1"),
    ),
    // обучение
    ("ob.check", n_("Show the tour on first launch")),
    ("ob.on", n_("🎓 Tour turned on")),
    ("ob.off", n_("🎓 Tour turned off")),
    (
        "ob.no_main_window",
        n_("🎓 The tour is only available in the main Rexon window — open the full window, \
            not a single tab"),
    ),
    ("ob.started", n_("🎓 Tour started")),
    // файловый менеджер
    ("fm.installed", n_("✅ installed")),
    ("fm.missing", n_("❌ not installed")),
    ("fm.auto_hint", n_("← follows the desktop")),
    (
        "fm.wm_note",
        n_("Hyprland and other window managers have no file manager of their own — any \
            one from the list will do (people usually install Dolphin or Thunar)."),
    ),
    ("fm.apply", n_("Apply File Manager Settings")),
    ("fm.current", n_("Current file manager: <b>%1</b>")),
    ("fm.unknown", "?"),
    ("fm.applied", n_("✅ File manager applied")),
    ("fm.not_applied", n_("⚠️ Could not apply")),
    // быстрое восстановление родных приложений
    (
        "quickfix.current",
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("Current desktop: <b>%1</b>"),
    ),
    (
        "quickfix.button",
        // TRANSLATORS: %1 is the name of the current desktop environment, e.g. KDE.
        n_("Restore %1 Apps"),
    ),
    (
        "quickfix.unknown_de",
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("The native app set for “%1” is unknown. Choose apps manually in the “Default \
            App Protection” section below."),
    ),
    ("quickfix.notify_title", "Rexon"),
    ("quickfix.notify_icon", "preferences-desktop-default-applications"),
    // комплектность рабочего стола
    ("compl.idle", n_("The check hasn't run yet")),
    ("compl.button", n_("Check and Install Missing")),
    ("compl.running", n_("Checking…")),
    ("compl.busy", n_("⏳ A check is already running")),
    ("compl.failed_log", n_("Check failed: %1")),
    // обои
    ("wp.no_engine", n_("⚠️ The wallpaper engine didn't load — this section is unavailable")),
    ("wp.stop", n_("⏹️ Stop Live Wallpaper")),
    ("wp.from_file", n_("<b>From a file</b>")),
    ("wp.file_hint", n_("Image: JPG, PNG, WebP.  Video: MP4, WebM, MKV, AVI, WMV, MPEG, GIF.  Flash: SWF.")),
    // ⚠️ Не «видео-обоев»: с 16.09.2026 эта галочка и ползунок рядом с ней
    // распоряжаются звуком и видео-обоев, и Flash-обоев — ключи настроек у
    // них одни. Подпись про видео означала бы, что человек выключает звук
    // Flash кнопкой, которая про Flash ничего не говорит.
    ("wp.video_audio", n_("🔊 Wallpaper sound (video and Flash)")),
    ("wp.pick_image", n_("🖼️ Image…")),
    ("wp.pick_video", n_("🎬 Video…")),
    ("wp.busy", n_("⚠️ Wallpaper is already being applied")),
    (
        "wp.applying",
        // TRANSLATORS: %1 is a file name, %2 is the wallpaper kind.
        n_("⏳ Applying: %1 (%2)…"),
    ),
    ("wp.applied", n_("✅ Wallpaper applied: %1")),
    ("wp.stopped", n_("⏹️ Live wallpaper stopped")),
    (
        "wp.audio_next",
        // TRANSLATORS: %1 is “on” or “off”.
        n_("🔊 Video sound: %1 (takes effect with the next video)"),
    ),
    (
        "wp.audio_live",
        // TRANSLATORS: %1 is “on” or “off”.
        n_("🔊 Wallpaper sound: %1"),
    ),
    (
        "wp.audio_volume",
        // TRANSLATORS: %1 is the volume in percent.
        n_("🔉 Wallpaper volume: %1%"),
    ),
    ("wp.audio_mute_other", n_("Mute when another app is playing sound")),
    ("wp.audio_mute_covered", n_("Pause the wallpaper when windows hide it")),
    (
        "wp.audio_hint",
        n_("Wallpaper sound changes on the fly — no need to reapply the wallpaper. Video \
            and Flash wallpapers share one volume."),
    ),
    (
        "wp.audio_reapply",
        // TRANSLATORS: %1 is “Turning on” or “Turning off”.
        n_("⏳ %1 video wallpaper sound…"),
    ),
    (
        "wp.audio_on",
        // TRANSLATORS: Sound state, shown as “Wallpaper sound: on”.
        n_("on"),
    ),
    (
        "wp.audio_off",
        // TRANSLATORS: Sound state, shown as “Wallpaper sound: off”.
        n_("off"),
    ),
    ("wp.audio_enabling", n_("Turning on")),
    ("wp.audio_disabling", n_("Turning off")),
    ("wp.working", "⏳…"),
    // YouTube / Vimeo
    ("yt.header", "<b>▶️ YouTube / Vimeo</b>"),
    ("yt.url_label", n_("Link:")),
    ("yt.url_hint", n_("https://youtube.com/watch?v=…  or a Vimeo link")),
    ("yt.quality_label", n_("Quality:")),
    ("yt.mode_label", n_("Mode:")),
    ("yt.apply", n_("▶️ Apply")),
    ("yt.audio", n_("🔊 YouTube video sound")),
    (
        "yt.audio_next",
        // TRANSLATORS: %1 is “on” or “off”.
        n_("🔊 YouTube sound: %1 (takes effect the next time a video starts)"),
    ),
    ("yt.no_url", n_("⚠️ Paste a video link")),
    ("yt.no_ytdlp", n_("⚠️ yt-dlp is not installed — click “Install yt-dlp”")),
    (
        "yt.starting",
        // TRANSLATORS: %1 is the video quality, %2 is the mode, %3 is “on” or “off” for sound.
        n_("⏳ YouTube (%1, %2, sound %3) — this may take a while…"),
    ),
    ("yt.applied", n_("✅ YouTube wallpaper: %1")),
    // yt-dlp
    ("ytdlp.have", "yt-dlp: ✅ <tt>v%1</tt>"),
    ("ytdlp.missing", n_("⚠️ yt-dlp is not installed — it's needed for YouTube wallpapers")),
    ("ytdlp.install", n_("📦 Install yt-dlp")),
    ("ytdlp.update", n_("🔄 Update yt-dlp")),
    ("ytdlp.installing", n_("Installing")),
    ("ytdlp.updating", n_("Updating")),
    ("ytdlp.working", "⏳ %1…"),
    (
        "ytdlp.password",
        // TRANSLATORS: %1 is “Installing” or “Updating”.
        n_("⏳ %1 yt-dlp (you'll be asked for a password)…"),
    ),
    ("ytdlp.done", "✅ yt-dlp: %1"),
    // веб-обои
    ("web.header", n_("<b>🌐 Web Wallpaper</b>")),
    ("web.hint", n_("A link to a web page or the path to an HTML file on this computer.")),
    // Починка (правило #13): образцом пути стояло английское «/path/to/…» —
    // покупатель читает в поле ввода чужой язык и служебное слово вместо
    // понятного примера.
    ("web.url_hint", n_("https://example.com  or  /home/name/page.html")),
    ("web.apply", n_("🌐 Apply")),
    ("web.no_url", n_("⚠️ Paste a link or the path to an HTML file")),
    ("web.loading", n_("⏳ Loading web wallpaper: %1…")),
    ("web.applied", n_("✅ Web wallpaper: %1")),
    // библиотека обоев
    ("lib.no_engine", n_("The wallpaper engine is unavailable")),
    (
        "lib.empty",
        n_("Nothing here yet. Wallpaper packs are installed from the OCS Theme Store, and \
            you can drop your own .rxn file into the “Extensions” tab."),
    ),
    ("lib.unnamed", n_("Untitled")),
    ("lib.apply", n_("Apply")),
    ("lib.preview", "👁️"),
    ("lib.preview_tip", n_("Open the file in the system viewer")),
    ("lib.no_file", n_("⚠️ The wallpaper file is unavailable")),
    (
        "lib.applying",
        // TRANSLATORS: %1 is a wallpaper name, %2 is the wallpaper kind.
        n_("⏳ Applying “%1” (%2)…"),
    ),
    // импорт тем Windows
    (
        "imp.formats",
        n_("Supported: themes (.themepack, .theme), visual styles (.msstyles), icons \
            (.ico, .icl), cursors (.cur, .ani), DreamScene video wallpapers (.dream, \
            .wmv)."),
    ),
    ("imp.pick", n_("📥 Choose Theme File…")),
    ("imp.dialog_title", n_("Choose a Windows theme file")),
    (
        "imp.unavailable_note",
        n_("⚠️ Theme import is currently unavailable in this Rexon build."),
    ),
    ("imp.unavailable", n_("⚠️ Theme import is currently unavailable in this build")),
    ("imp.busy", n_("⚠️ An import is already running — please wait")),
    ("imp.working", n_("⏳ Importing the theme…")),
    ("imp.working_file", n_("⏳ Importing the theme: %1…")),
    ("imp.working_status", n_("⏳ Importing the theme — this may take a while…")),
    (
        "imp.default_name",
        // TRANSLATORS: Used in place of a theme name when the theme has none.
        n_("theme"),
    ),
    ("imp.installed", n_("✅ “%1” is installed — you'll find it in the “Extensions” tab.")),
    ("imp.installed_status", n_("✅ Theme installed: %1")),
    ("imp.not_found", n_("file not found")),
    ("imp.broken", n_("the file is damaged or its format is not supported")),
    ("imp.failed", n_("⚠️ Could not import: %1")),
    // размытие обоев
    (
        "blur.no_pillow",
        n_("⚠️ Blur is unavailable: the Pillow library is missing (package \
            <tt>python-pillow</tt>)"),
    ),
    ("blur.strength", n_("Strength:")),
    // Значение ползунка силы. Починка (правило #13): было «20px» — про
    // пиксели покупателю знать незачем, радиус размытия ему ни о чём не
    // говорит. Осталось само деление шкалы рядом с подписью «Сила:», как у
    // любого ползунка. Ключ оставлен прежним: его читают окно и страница, а
    // человеку он не виден.
    ("blur.px", "%1"),
    ("blur.apply", n_("🌫️ Blur Wallpaper")),
    ("blur.remove", n_("🔆 Remove Blur")),
    ("blur.busy", n_("⚠️ A blur operation is already running")),
    (
        "blur.applying",
        // TRANSLATORS: %1 is the blur strength (a number).
        n_("⏳ Blurring the wallpaper (strength %1)…"),
    ),
    ("blur.removing", n_("⏳ Removing the blur and restoring the previous wallpaper…")),
    ("blur.done", n_("The current wallpaper is blurred")),
    (
        "blur.failed",
        n_("Could not blur: there's no image wallpaper right now, or it couldn't be read"),
    ),
    ("blur.removed", n_("Blur removed, wallpaper restored")),
    ("blur.nothing_to_restore", n_("No saved wallpaper to restore")),
    // фоновые звуки
    (
        "amb.no_manager",
        n_("⚠️ Ambient sounds are unavailable: the wallpaper engine didn't load"),
    ),
    (
        "amb.no_mpv",
        n_("⚠️ Ambient sounds are unavailable: the <tt>mpv</tt> package is not installed"),
    ),
    ("amb.add", n_("➕ Add File…")),
    ("amb.add_tip", n_("Play the chosen audio file on a loop in the background")),
    ("amb.dialog_title", n_("Choose an audio file")),
    ("amb.volume", n_("🔊 Volume:")),
    ("amb.percent", "%1%"),
    ("amb.stop_all", n_("⏹️ Stop All")),
    (
        "amb.volume_hint",
        n_("The slider sets the volume of a <b>new</b> sound. To change the volume of one \
            that's already playing, click 🔊 in its row."),
    ),
    ("amb.empty", n_("No ambient sounds are playing. Click “Add File…”.")),
    ("amb.row_name", "▶️ <b>%1</b>"),
    ("amb.row_volume", "🔊 %1%"),
    ("amb.row_unknown", "?"),
    ("amb.setvol", "🔊"),
    ("amb.setvol_tip", n_("Set this sound's volume from the slider")),
    ("amb.stop", "⏹️"),
    ("amb.stop_tip", n_("Stop this sound")),
    (
        "amb.started",
        // TRANSLATORS: %1 is a file name, %2 is the volume in percent.
        n_("✅ Started: %1 (%2%)"),
    ),
    ("amb.start_failed", n_("⚠️ Could not start (mpv is required)")),
    (
        "amb.start_unplayable",
        n_("⚠️ The sound couldn't be played: the file can't be read or there's no audio \
            output device"),
    ),
    ("amb.start_error", n_("⚠️ Error starting the sound: %1")),
    (
        "amb.volume_set",
        // TRANSLATORS: %1 is a sound name, %2 is the volume in percent.
        n_("🔊 Volume of “%1” → %2%"),
    ),
    ("amb.not_playing", n_("⚠️ This sound is no longer playing")),
    ("amb.stopped", n_("⏹️ Stopped: %1")),
    ("amb.stopped_all", n_("⏹️ All ambient sounds stopped")),
    // звуки событий
    ("snd.theme_label", n_("<b>Sound theme:</b>")),
    // Строка выбора, когда системная тема не задана вовсе либо названа так,
    // что среди установленных её нет. Без неё список показывал ПЕРВУЮ
    // установленную тему, и человек читал чужое значение как своё.
    ("snd.theme_unset", n_("— not set —")),
    // В системе выбрана тема, которой на диске нет (её удалили, а ссылка
    // осталась): сказать это, а не «не задана» — иначе человек не узнает,
    // почему звуков нет. %1 — имя темы из настроек системы.
    ("snd.theme_missing", n_("“%1” — theme not found")),
    ("snd.events_check", n_("Turn on event sounds (clicks, notifications, errors)")),
    ("snd.test_label", n_("<b>Sound test:</b>")),
    (
        "snd.themes_hint",
        n_("💡 Not many themes? There are more in Appearance → Sounds."),
    ),
    ("snd.no_events", n_("This theme has no sounds to test")),
    ("snd.event_tip", n_("Event sound: %1")),
    ("snd.theme_applied", n_("✅ Sound theme: %1")),
    ("snd.theme_failed", n_("⚠️ Could not apply the theme: %1")),
    (
        "snd.events_state",
        // TRANSLATORS: %1 is “turned on” or “turned off”.
        n_("✅ Event sounds %1"),
    ),
    // «включены/выключены» — одно слово на две секции (звуки событий и
    // иконки рабочего стола MATE), поэтому оно отдельной записью.
    (
        "state.on",
        // TRANSLATORS: Shown as “Event sounds turned on”, “Desktop icons turned on”.
        n_("turned on"),
    ),
    (
        "state.off",
        // TRANSLATORS: Shown as “Event sounds turned off”, “Desktop icons turned off”.
        n_("turned off"),
    ),
    ("snd.events_failed", n_("⚠️ Could not switch event sounds")),
    ("snd.cannot_play", "🔇 %1"),
    // MATE: иконки рабочего стола
    ("mate.icons_on", n_("✅ Show Icons")),
    ("mate.icons_off", n_("⏹️ Hide Icons")),
    ("mate.restart_caja", n_("🔄 Restart Caja")),
    ("mate.no_api", n_("⚠️ MATE settings are unavailable")),
    (
        "mate.icons_state",
        // TRANSLATORS: %1 is “turned on” or “turned off”.
        n_("✅ Desktop icons %1"),
    ),
    ("mate.caja_restarted", n_("🔄 Caja restarted")),
    ("mate.icons_failed", n_("⚠️ Could not switch MATE desktop icons")),
    ("mate.caja_failed", n_("⚠️ Could not restart the Caja desktop")),
    // системные шрифты
    // Шрифт не задан — значит, действует шрифт рабочего стола по умолчанию;
    // «не задан» читалось так, будто шрифта нет вовсе.
    ("font.unset", n_("<i>(default)</i>")),
    ("font.value", "<i>%1</i>"),
    ("font.placeholder", "—"),
    (
        "font.de_hint",
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("Current desktop: <b>%1</b>"),
    ),
    ("font.applied", "✅ %1: %2 %3pt"),
    (
        "font.failed",
        // TRANSLATORS: %1 is the font kind exactly as titled in the list (“System Font”, “Monospace Font”…), keep it at the start.
        n_("⚠️ %1: could not apply"),
    ),
    ("font.all_mode", n_("Change:")),
    ("font.dpi", n_("Force font DPI:")),
    ("font.render_title", n_("<b>Text smoothing</b>")),
    ("font.aa", n_("Anti-aliasing")),
    ("font.aa_exclude", n_("Don't smooth sizes from")),
    (
        "font.aa_to",
        // TRANSLATORS: between two font sizes: “Don't smooth sizes from [8] to [15] pt”.
        n_("to"),
    ),
    (
        "font.pt",
        // TRANSLATORS: abbreviation of “points”, the unit of font size.
        n_("pt"),
    ),
    ("font.subpixel", n_("Sub-pixel rendering:")),
    ("font.hinting", n_("Hinting:")),
    (
        "font.render_hint",
        n_("Saved to the shared font settings file that apps on GTK, Qt and Electron and web \
            browsers read, and to this desktop's own settings"),
    ),
    (
        "font.render_saved",
        n_("✅ Text smoothing saved. Apps that are already open may need a restart"),
    ),
    (
        "font.render_native_failed",
        n_("⚠️ Text smoothing: the desktop's own settings could not be changed"),
    ),
    (
        "font.fc_refused",
        // TRANSLATORS: %1 is the path of the fontconfig file (fonts.conf).
        n_("⚠️ The font settings file %1 can't be read safely, so Rexon left it untouched. \
            Fix or remove it and try again"),
    ),
    (
        "font.fc_failed",
        // TRANSLATORS: %1 is the path of the fontconfig file (fonts.conf).
        n_("⚠️ Text smoothing: could not write %1"),
    ),
    // защита приложений по умолчанию
    ("prot.enable", n_("Turn on protection  (recommended)")),
    (
        "prot.remembered",
        // TRANSLATORS: %1 is an app type (e.g. Browser), %2 is the remembered app.
        n_("%1   ·  remembered: %2"),
    ),
    (
        "prot.not_remembered",
        // TRANSLATORS: %1 is an app type (e.g. Browser).
        n_("%1   ·  not remembered yet"),
    ),
    ("prot.desktop_file", n_("Shortcut file: %1")),
    ("prot.note", "ℹ️ %1"),
    ("prot.snapshot_title", n_("<b>Remember the current choice</b>")),
    (
        "prot.snapshot_sub",
        n_("Rexon will note which apps are your defaults right now and bring exactly \
            those back if they get replaced"),
    ),
    ("prot.snapshot", n_("📌 Remember Now")),
    ("prot.restore", n_("↩️ Restore Previous")),
    ("prot.snapshot_ok", n_("✅ Default apps saved")),
    // Починка: было «⚠️ Ошибка snapshot: …» — жаргон в лицо покупателю
    // (правило #13). Смысл тот же, слова человеческие.
    ("prot.snapshot_error", n_("⚠️ Could not remember the choice: %1")),
    ("prot.restore_ok", n_("✅ Default apps restored")),
    // панель Plasma
    ("plasma.backup_title", n_("<b>Save the current panel layout</b>")),
    (
        "plasma.backup_sub",
        n_("Rexon will make a copy of the panel layout, widgets and keyboard shortcuts — \
            you can bring it back in one click"),
    ),
    (
        "plasma.backup_tip",
        n_("Settings files that get copied: plasma-org.kde.plasma.desktop-appletsrc, \
            plasmashellrc, kglobalshortcutsrc"),
    ),
    ("plasma.backup", n_("📦 Save Copy")),
    ("plasma.backups_header", n_("<b>Saved copies (%1):</b>")),
    ("plasma.no_backups", n_("No saved copies yet")),
    ("plasma.restore", n_("Restore")),
    ("plasma.backup_dir", n_("Copy folder: %1")),
    ("plasma.confirm_title", n_("Restore?")),
    (
        "plasma.confirm_text",
        // TRANSLATORS: %1 is the date and time of the saved copy.
        n_("Restore the Plasma panel from the copy made %1?\n\nThe current panel layout \
            will be replaced."),
    ),
    // Починка: было «⚠️ Ошибка backup: …» — то же жаргонное слово.
    ("plasma.backup_error", n_("⚠️ Could not save a copy: %1")),
    // оформление рабочего стола (AeroShell)
    ("aero.check", n_("Check the desktop appearance when Rexon starts")),
    (
        "aero.check_tip",
        n_("Rexon compares the version of the desktop appearance with the system version \
            and offers to restore it if they no longer match. A rebuild NEVER starts \
            until you click"),
    ),
    ("aero.check_now", n_("🔍 Check Now")),
    ("aero.on", n_("🪟 The desktop appearance will be checked at startup")),
    ("aero.off", n_("🪟 Desktop appearance check turned off")),
    ("aero.up_to_date", n_("✅ The desktop appearance matches the current system version")),
    // ── Значок в системном лотке ──────────────────────────────────────────
    //
    // ⚠️ Подпись флажка — та же строка, что в меню самого значка
    // (`crate::tray`, ключ `autostart`), и это не совпадение: настройка одна,
    // дверей в неё две. Расхождение ловит тест `tray::tests`.
    ("tray.autostart", n_("Start at login")),
    (
        "tray.autostart_hint",
        n_("Rexon will start in the tray when you log in, without showing the window"),
    ),
    ("tray.autostart_on", n_("✅ Rexon will start when you log in")),
    ("tray.autostart_off", n_("Start at login turned off")),
    ("tray.autostart_failed", n_("⚠️ Could not change autostart — see the log for details")),
    (
        "tray.missing",
        n_("⚠️ There's no tray in this session: the icon wasn't added, and closing the \
            window quits Rexon as usual"),
    ),
    // докачка значков и курсора к теме (`theme_watch`)
    ("tray.theme_deps", n_("Download icons and cursors for the theme automatically")),
    (
        "tray.theme_deps_hint",
        n_("When a theme you pick — in Rexon or in your desktop's own settings — needs icons \
            or a cursor that aren't installed, Rexon downloads them into your home folder and \
            applies them. Turned off, it asks first. Sets that only come as system packages \
            are always asked about."),
    ),
    ("tray.theme_deps_on", n_("✅ Icons and cursors for the theme will be downloaded automatically")),
    ("tray.theme_deps_off", n_("Rexon will ask before downloading icons and cursors for the theme")),
];

/// Исходный (английский) шаблон по ключу каталога — `msgid`, без перевода.
///
/// Нужен сверкам, которые держат одну формулировку на две двери (подпись
/// автозапуска в меню значка и в «Настройках»): сравнивать надо исходники,
/// а не то, что выбрал язык машины.
pub fn text_msgid(key: &str) -> Option<&'static str> {
    TEXTS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Шаблон строки по ключу каталога — переведённый на язык интерфейса.
pub fn text_template(key: &str) -> Option<String> {
    text_msgid(key).map(tr)
}

/// Готовая строка каталога: `%1`, `%2` заменяются на `args` по порядку.
/// Неизвестный ключ — `None`: зовущий обязан упасть с внятной ошибкой, а не
/// показать покупателю пустую надпись.
pub fn text(key: &str, args: &[String]) -> Option<String> {
    // Фраза, где имя стола не стоит после предлога («Restore %1 Apps»):
    // для неопознанного стола — целая своя фраза, а не склейка.
    if key == "quickfix.button"
        && args.first().is_some_and(|a| crate::screenshots::is_unknown_de_display(a))
    {
        // TRANSLATORS: button: restore the default apps of a desktop
        // environment Rexon couldn't recognize.
        return Some(tr("Restore Your Desktop's Apps"));
    }
    let mut out = text_template(key)?;
    for (i, arg) in args.iter().enumerate() {
        // Любое место подстановки может получить имя неопознанного стола
        // («quickfix.unknown_de», «quickfix.button»): грамотную форму после
        // предлога выбирает общий `place_de_name`, прочие значения он
        // подставляет как есть.
        out = crate::screenshots::place_de_name(&out, &format!("%{}", i + 1), arg);
    }
    Some(out)
}

/// Обрезка длинного сообщения движка по символам (не по байтам — иначе
/// русский текст рвётся посреди буквы).
pub fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// Готовая строка каталога без подстановок — короткая форма [`text`].
fn t(key: &str) -> String {
    text_template(key).unwrap_or_default()
}

/// Строка каталога с подстановками — короткая форма для кода ядра.
fn tf(key: &str, args: &[&str]) -> String {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    text(key, &owned).unwrap_or_default()
}

// ============================================================================
// Решения вкладки: какую именно строку показать и что делать дальше
// ============================================================================

/// Подпись «установлен / не установлен» для строки файлового менеджера.
pub fn fm_badge(installed: bool) -> String {
    if installed {
        t("fm.installed")
    } else {
        t("fm.missing")
    }
}

/// Итог применения файлового менеджера.
pub fn fm_apply_status(ok: bool) -> String {
    if ok {
        t("fm.applied")
    } else {
        t("fm.not_applied")
    }
}

/// Строка и кнопка yt-dlp по текущему состоянию системы:
/// (надпись, подпись кнопки, действие кнопки — `install` либо `update`).
///
/// Починка (была в оригинале до переноса): состояние читалось один раз при
/// постройке секции, и после установки кнопка оставалась «Установить».
/// Решение здесь — чтобы обе стороны (постройка и обновление после установки)
/// читали одно правило.
pub fn ytdlp_row(installed: bool, version: &str) -> (String, String, String) {
    if installed {
        (
            tf("ytdlp.have", &[version]),
            t("ytdlp.update"),
            "update".to_string(),
        )
    } else {
        (
            t("ytdlp.missing"),
            t("ytdlp.install"),
            "install".to_string(),
        )
    }
}

/// Подписи для хода установки/обновления yt-dlp: (подпись кнопки, статус).
pub fn ytdlp_working(action: &str) -> (String, String) {
    let verb = if action == "update" {
        t("ytdlp.updating")
    } else {
        t("ytdlp.installing")
    };
    (
        tf("ytdlp.working", &[&verb]),
        tf("ytdlp.password", &[&verb]),
    )
}

/// Итог установки/обновления yt-dlp.
pub fn ytdlp_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 180);
    if ok {
        tf("ytdlp.done", &[&msg])
    } else {
        tf("status.warn", &[&msg])
    }
}

/// Надо ли переприменить обои при переключении звука видео.
///
/// Переприменяем, только если видео сейчас играет и его файл на месте —
/// иначе настройка просто ждёт следующего видео.
pub fn wallpaper_audio_reapply(running: bool, format: &str, path_present: bool) -> bool {
    running && format == "video" && path_present
}

/// Сообщение о переключении звука видео-обоев: `reapply` — переприменяем
/// прямо сейчас, иначе «применится к следующему видео».
pub fn wallpaper_audio_status(on: bool, reapply: bool) -> String {
    if reapply {
        let verb = if on {
            t("wp.audio_enabling")
        } else {
            t("wp.audio_disabling")
        };
        return tf("wp.audio_reapply", &[&verb]);
    }
    tf("wp.audio_next", &[&audio_word(on)])
}

/// Сообщение о переключении звука обоев, который применяется НА ХОДУ.
///
/// Отдельно от [`wallpaper_audio_status`] намеренно: та обещала «применится
/// к следующему видео» либо «переприменяю», и обе половины обещания больше
/// не про наш хост — он подхватывает настройку сам, ничего не перезапуская.
/// Говорить человеку про перезапуск, которого нет, — врать в мелочи.
pub fn wallpaper_audio_live_status(on: bool) -> String {
    tf("wp.audio_live", &[&audio_word(on)])
}

/// Подпись ползунка громкости обоев.
pub fn wallpaper_volume_label(percent: i64) -> String {
    tf(
        "wp.audio_volume",
        &[&crate::wallpaper_audio::clamp_volume_pct(percent).to_string()],
    )
}

/// Сообщение о переключении звука YouTube-обоев.
pub fn youtube_audio_status(on: bool) -> String {
    tf("yt.audio_next", &[&audio_word(on)])
}

/// «вкл» либо «выкл» — одна формулировка на всю вкладку.
fn audio_word(on: bool) -> String {
    if on {
        t("wp.audio_on")
    } else {
        t("wp.audio_off")
    }
}

/// Статус начала применения обоев из файла.
pub fn wallpaper_applying_status(basename: &str, format: &str) -> String {
    tf("wp.applying", &[basename, format])
}

/// Итог применения обоев (общий для файла, библиотеки и переключения звука).
pub fn wallpaper_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 160);
    if ok {
        tf("wp.applied", &[&msg])
    } else {
        tf("status.error", &[&msg])
    }
}

/// Заголовок окна выбора файла обоев: «Choose wallpaper» / «Choose a video»
/// (по-русски — прежние «Выбрать: обои» / «Выбрать: видео»).
pub fn wallpaper_dialog_title(kind: &str) -> String {
    match kind {
        "video" => tr("Choose a video"),
        "swf" => tr("Choose a Flash file"),
        "static" => tr("Choose an image"),
        _ => tr("Choose wallpaper"),
    }
}

/// Статус начала применения YouTube-обоев.
pub fn youtube_starting_status(quality: &str, mode: &str, audio: bool) -> String {
    // ⭐ Починка: в строке стояли служебные значения («stream», «best»).
    // Подписи — из тех же списков, что в выпадающих меню вкладки; незнакомое
    // значение показывается как есть.
    let label = |table: &[(&str, &str)], value: &str| {
        table
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(l, _)| tr(l))
            .unwrap_or_else(|| value.to_string())
    };
    tf(
        "yt.starting",
        &[&label(YT_QUALITIES, quality), &label(YT_MODES, mode), &audio_word(audio)],
    )
}

/// Итог применения YouTube-обоев.
pub fn youtube_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 180);
    if ok {
        tf("yt.applied", &[&msg])
    } else {
        tf("status.error", &[&msg])
    }
}

/// Адрес веб-обоев в том виде, в каком его понимает движок: локальный путь
/// превращается в `file://`. `exists` — есть ли такой файл (наблюдение).
pub fn web_wallpaper_url(url: &str, exists: bool) -> String {
    let url = url.trim();
    if url.starts_with('/') && exists {
        return format!("file://{url}");
    }
    url.to_string()
}

/// Статус начала загрузки веб-обоев.
pub fn web_loading_status(url: &str) -> String {
    tf("web.loading", &[&clip(url, 80)])
}

/// Итог применения веб-обоев.
pub fn web_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 180);
    if ok {
        tf("web.applied", &[&msg])
    } else {
        tf("status.error", &[&msg])
    }
}

/// Итог остановки живых обоев.
pub fn wallpaper_stop_status(ok: bool, msg: &str) -> String {
    if ok {
        t("wp.stopped")
    } else {
        tf("status.warn", &[msg])
    }
}

// ── Библиотека обоев ─────────────────────────────────────────────────

/// Показывать ли у плитки кнопку предпросмотра «открыть системно».
///
/// Починка: оригинал сверял формат со своим списком
/// `("video","gif","swf","webm","mkv","mp4")`, где смешаны формат обоев и
/// расширения файла, — и `.avi`, `.mov`, `.m4v` кнопку не получали, хотя
/// движок эти видео проигрывает. Теперь решение читает общий список
/// [`VIDEO_EXTS`] плюс флеш (`swf`), который движок тоже отдаёт наружу.
pub fn library_shows_preview(format: &str) -> bool {
    let fmt = format.trim().to_lowercase();
    fmt == "video" || fmt == "swf" || VIDEO_EXTS.contains(&fmt.as_str())
}

/// Сколько РЯДОВ плиток показывать в секции библиотеки.
pub const LIBRARY_ROWS: usize = 3;

/// Статус начала применения обоев из библиотеки.
pub fn library_applying_status(basename: &str, format: &str) -> String {
    tf("lib.applying", &[basename, format])
}

// ── Импорт тем Windows ───────────────────────────────────────────────

/// Понятная причина, почему тема не импортировалась.
///
/// `missing` — файла не нашлось; иначе берём сообщение движка, а пустое
/// заменяем общей формулировкой (человеку «» ни о чём не говорит).
pub fn import_failure_reason(missing: bool, raw: &str) -> String {
    if missing {
        return t("imp.not_found");
    }
    let raw = raw.trim();
    if raw.is_empty() {
        t("imp.broken")
    } else {
        raw.to_string()
    }
}

/// Надпись секции и статус после неудачного импорта: (надпись, статус).
pub fn import_failure_texts(reason: &str) -> (String, String) {
    (
        tf("imp.failed", &[reason]),
        tf("imp.failed", &[&clip(reason, 160)]),
    )
}

/// Надпись секции и статус после удачного импорта: (надпись, статус).
pub fn import_success_texts(name: &str, notes: &[String]) -> (String, String) {
    let shown = if name.trim().is_empty() {
        t("imp.default_name")
    } else {
        name.trim().to_string()
    };
    (
        // Предупреждения о пропущенных шагах идут в ПОЯСНЕНИЕ (оно остаётся
        // на странице), а не в бегущую строку состояния: строка исчезнет, а
        // «почему обложки нет» человеку нужно дочитать.
        crate::importers::with_skipped_notes(&tf("imp.installed", &[&shown]), notes),
        tf("imp.installed_status", &[&shown]),
    )
}

// ── Размытие обоев ───────────────────────────────────────────────────

/// Ползунок силы размытия: (минимум, максимум, шаг, значение по умолчанию).
pub const BLUR_RANGE: (i32, i32, i32, i32) = (5, 50, 5, 20);

/// Итог размытия/снятия размытия — сообщение самой операции.
pub fn blur_result(action: &str, ok: bool) -> String {
    match (action, ok) {
        ("blur", true) => t("blur.done"),
        ("blur", false) => t("blur.failed"),
        (_, true) => t("blur.removed"),
        (_, false) => t("blur.nothing_to_restore"),
    }
}

/// Итог размытия для строки состояния.
pub fn blur_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 160);
    if ok {
        format!("✅ {msg}")
    } else {
        format!("⚠️ {msg}")
    }
}

// ── Фоновые звуки ────────────────────────────────────────────────────

/// Громкость нового фонового звука по умолчанию, %.
pub const AMBIENT_DEFAULT_VOLUME: i32 = 50;

/// Строка активного фонового звука: (имя, громкость).
pub fn ambient_row_texts(name: Option<&str>, volume: Option<&str>) -> (String, String) {
    let unknown = t("amb.row_unknown");
    (
        tf("amb.row_name", &[name.unwrap_or(&unknown)]),
        tf("amb.row_volume", &[volume.unwrap_or(&unknown)]),
    )
}

/// Итог запуска фонового звука.
pub fn ambient_start_status(ok: bool, basename: &str, volume: i32) -> String {
    if ok {
        tf("amb.started", &[basename, &volume.to_string()])
    } else {
        t("amb.start_failed")
    }
}

/// Отказ запуска фонового звука — с настоящей причиной.
///
/// ⭐ Починка: прежний ответ «нужен mpv» звучал на ЛЮБОЙ отказ, в том числе
/// при установленном mpv, когда не играет сам файл, — и человек шёл ставить
/// то, что у него уже есть. `has_mpv` — есть ли mpv в системе (наблюдение).
pub fn ambient_start_failed_status(has_mpv: bool) -> String {
    if has_mpv {
        t("amb.start_unplayable")
    } else {
        t("amb.start_failed")
    }
}

/// Итог смены громкости фонового звука.
///
/// `name` — имя звука для человека, `None` — такого звука уже нет (доиграл
/// или снят). `ok` — перезапустился ли звук с новой громкостью.
///
/// ⭐ Две починки: Python показывал служебный идентификатор звука
/// («Громкость «manual_дождь.ogg»»), а итог перезапуска не смотрел вовсе —
/// звук, не переживший смену громкости, рапортовался как «громкость задана».
pub fn ambient_volume_status(name: Option<&str>, volume: i64, ok: bool, has_mpv: bool) -> String {
    match (name, ok) {
        (None, _) => t("amb.not_playing"),
        (Some(name), true) => tf("amb.volume_set", &[name, &volume.to_string()]),
        (Some(_), false) => ambient_start_failed_status(has_mpv),
    }
}

/// Итог остановки одного фонового звука. `name` — имя звука для человека,
/// `None` — такого звука уже нет.
///
/// ⭐ Починка: Python писал служебный идентификатор звука («Остановлен:
/// manual_дождь.ogg») и рапортовал остановку даже о звуке, которого уже нет.
pub fn ambient_stopped_status(name: Option<&str>) -> String {
    match name {
        Some(name) => tf("amb.stopped", &[name]),
        None => t("amb.not_playing"),
    }
}

// ── Звуки событий ────────────────────────────────────────────────────

/// Сколько кнопок «послушать» показывать в секции звуков.
pub const SOUND_PREVIEW_BUTTONS: usize = 5;

/// Итог смены звуковой темы.
pub fn sound_theme_status(ok: bool, theme: &str) -> String {
    if ok {
        tf("snd.theme_applied", &[theme])
    } else {
        tf("snd.theme_failed", &[theme])
    }
}

/// Итог переключения звуков событий.
pub fn event_sounds_status(ok: bool, enabled: bool) -> String {
    if !ok {
        return t("snd.events_failed");
    }
    tf("snd.events_state", &[&state_word(enabled)])
}

/// «включены» либо «выключены» — общее слово для обеих секций-переключателей.
fn state_word(on: bool) -> String {
    if on {
        t("state.on")
    } else {
        t("state.off")
    }
}

// ── MATE: иконки рабочего стола ──────────────────────────────────────

/// Кнопки секции MATE: (подпись, действие — `on` / `off` / `restart`).
pub fn mate_buttons() -> Vec<(String, String)> {
    vec![
        (t("mate.icons_on"), "on".to_string()),
        (t("mate.icons_off"), "off".to_string()),
        (t("mate.restart_caja"), "restart".to_string()),
    ]
}

/// Итог переключения иконок рабочего стола MATE.
pub fn mate_icons_status(visible: bool) -> String {
    tf("mate.icons_state", &[&state_word(visible)])
}

/// Итог кнопки иконок MATE с учётом того, отработали ли команды.
///
/// ⭐ Починка: Python рапортовал «иконки включены», не глядя на итог.
pub fn mate_icons_result(visible: bool, ok: bool) -> String {
    if ok {
        mate_icons_status(visible)
    } else {
        t("mate.icons_failed")
    }
}

/// Итог перезапуска рабочего стола Caja.
pub fn caja_restart_result(ok: bool) -> String {
    if ok {
        t("mate.caja_restarted")
    } else {
        t("mate.caja_failed")
    }
}

// ── Шрифты ───────────────────────────────────────────────────────────

/// Надпись под кнопкой шрифта: что сейчас стоит в системе.
///
/// ⚠️ Показывается ИМЯ И КЕГЛЬ, а не то, как шрифт записан в конфиге. KDE
/// хранит сериализацию QFont из десяти полей
/// (`Segoe UI,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0`), и раньше эта строка
/// уезжала покупателю как есть — сырая техническая запись прямо в окне
/// настроек (правило #13). Разбирает её тот же [`parse_font_spec_face`],
/// которым вкладка открывает окно выбора шрифта, так что надпись и
/// предвыбранный шрифт говорят одно и то же — вместе с начертанием
/// («Sans Bold 10»: слова Pango, как их показывают GNOME и XFCE).
///
/// Строку, из которой не вышло достать семейство, показываем как есть: это
/// лучше, чем «не задан» там, где что-то всё-таки записано.
pub fn font_value_text(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return t("font.unset");
    }
    let (family, size, face) = parse_font_spec_face(raw);
    let shown = if family.is_empty() { raw.to_string() } else { font_face_spec(&family, size, face) };
    tf("font.value", &[&shown])
}

/// Итог применения шрифта.
pub fn font_apply_status(ok: bool, title: &str, family: &str, size: i32) -> String {
    if ok {
        tf("font.applied", &[title, family, &size.to_string()])
    } else {
        // Название — как в списке, без понижения регистра: в немецком
        // существительные пишутся с заглавной, и `to_lowercase` их ломал.
        tf("font.failed", &[title])
    }
}

// ── Защита приложений по умолчанию ───────────────────────────────────

/// Типы приложений, которые показывает секция защиты, в порядке показа.
///
/// Тот же порядок, по которому ядро считает состояние
/// ([`crate::defaults::get_status_summary`]): «терминал» идёт последним, потому
/// что настраивается не через MIME. Раньше список собирался в окне из
/// `MIME_TYPES.keys()` — вторая сборка того же порядка.
pub fn protection_types() -> Vec<&'static str> {
    crate::defaults::MIME_TYPES
        .iter()
        .map(|(k, _)| *k)
        .chain(std::iter::once("terminal"))
        .collect()
}

/// Подпись галочки типа приложения и подсказка к ней: (подпись, подсказка).
/// Пустая подсказка — показывать нечего.
pub fn protection_row_texts(app_type: &str, remembered: &str) -> (String, String) {
    let label = crate::defaults::app_type_label(app_type);
    let label = label.as_str();
    let pretty = pretty_desktop_id(remembered);
    if pretty.is_empty() {
        return (tf("prot.not_remembered", &[label]), String::new());
    }
    (
        tf("prot.remembered", &[label, &pretty]),
        tf("prot.desktop_file", &[remembered]),
    )
}

// ── Панель Plasma ────────────────────────────────────────────────────

/// Сколько последних копий панели показывать в секции.
pub const PLASMA_BACKUPS_SHOWN: usize = 5;

/// Заголовок списка копий панели.
pub fn plasma_backups_header(total: usize) -> String {
    tf("plasma.backups_header", &[&total.to_string()])
}

/// Вопрос перед восстановлением копии: (заголовок окна, текст).
///
/// Починка (правило #13): в вопросе стояла машинная метка времени
/// («20.06.2026 в 01:10» превращалось в «20260620_011005»). В списке выше та
/// же копия подписана по-человечески ([`backup_row_label`]), и человек не
/// узнавал в вопросе строку, на которую только что нажал. Метку приводит к
/// человеческому виду тот же [`pretty_backup_ts`] — второй таблицы дат тут
/// быть не должно.
pub fn plasma_restore_confirm(ts: &str) -> (String, String) {
    (
        t("plasma.confirm_title"),
        tf("plasma.confirm_text", &[&pretty_backup_ts(ts)]),
    )
}

/// Ответ движка копий как строка состояния: значок по успеху + его сообщение.
pub fn plasma_status(ok: bool, msg: &str) -> String {
    if ok {
        format!("✅ {msg}")
    } else {
        format!("⚠️ {msg}")
    }
}

// ── Прочие итоги ─────────────────────────────────────────────────────

/// Строка состояния после восстановления родных приложений окружения:
/// значок зависит от того, всё ли нашлось.
pub fn restore_native_status(msg: &str, has_missing: bool) -> String {
    if has_missing {
        format!("⚠️ {msg}")
    } else {
        format!("✅ {msg}")
    }
}

/// Итог проверки комплектности для всплывающей строки: без проверок — «ℹ️».
pub fn compliance_status(msg: &str, has_results: bool) -> String {
    if has_results {
        msg.to_string()
    } else {
        format!("ℹ️ {msg}")
    }
}

/// Сообщение переключателя обучения.
pub fn onboarding_toggle_status(enabled: bool) -> String {
    if enabled {
        t("ob.on")
    } else {
        t("ob.off")
    }
}

/// Подпись кнопки «пройти обучение заново» — берётся у самого обучения.
pub fn onboarding_replay_button() -> String {
    format!("▶️ {}", crate::onboarding::settings_replay())
}

/// Сообщение переключателя проверки оформления.
pub fn aeroshell_toggle_status(enabled: bool) -> String {
    if enabled {
        t("aero.on")
    } else {
        t("aero.off")
    }
}

// ============================================================================
// Язык интерфейса
// ============================================================================
//
// Список и подписи — здесь, механизм — в `i18n`: какие языки есть (из
// `po/LINGUAS` и найденных каталогов), как их зовут носители, какой язык
// даст выбор. Язык меняется после перезапуска, а не на лету — почему, см.
// `i18n::restart_needed`.

/// Пункты списка «Язык»: (значение настройки `ui_language`, подпись).
///
/// Первым — «как в системе» с языком, который он сейчас даст; дальше
/// английский и языки собранных каталогов, каждый — своим самоназванием.
pub fn language_choices() -> Vec<(String, String)> {
    let system = crate::i18n::native_name(&crate::i18n::language_for_setting(None));
    let mut out = vec![(
        crate::i18n::SYSTEM_SETTING.to_string(),
        text("lang.system", &[system]).unwrap_or_default(),
    )];
    for code in crate::i18n::available_languages() {
        let label = crate::i18n::native_name(&code);
        out.push((code, label));
    }
    out
}

/// Знакомо ли значение списку (сохранять незнакомое незачем: мусор в
/// настройке дал бы английский без объяснения).
pub fn language_choice_valid(value: &str) -> bool {
    language_choices().iter().any(|(code, _)| code == value)
}

/// Ответ на выбор языка: «сохранён: …». Подпись — самоназванием; для
/// «как в системе» — та же строка, что в списке.
pub fn language_saved_status(value: &str) -> String {
    let label = language_choices()
        .into_iter()
        .find(|(code, _)| code == value)
        .map(|(_, label)| label)
        .unwrap_or_else(|| value.to_string());
    tf("lang.saved", &[label.as_str()])
}

/// Отказ кнопки «Перезапустить Rexon» с причиной.
pub fn language_restart_failed(reason: &str) -> String {
    tf("lang.restart_failed", &[reason])
}

/// Аргументы нового процесса при перезапуске ради языка (`args` — без
/// имени программы).
///
/// Окно открывается снова на «Настройках»: человек только что был там и
/// должен увидеть, что язык сменился. Поэтому снимаются прежний раздел
/// (`--section=`, `--page=`) и `--tray` (иначе окно ушло бы в лоток, и
/// перезапуск выглядел бы как закрытие), а служебный `--selftest` не
/// повторяется вовсе.
pub fn restart_args(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .filter(|a| {
            *a != crate::tray::START_HIDDEN_FLAG
                && *a != "--selftest"
                && !a.starts_with(crate::cli::SECTION_FLAG)
                && !a.starts_with(crate::app::PAGE_FLAG)
        })
        .cloned()
        .collect();
    out.push(format!("{}settings", crate::cli::SECTION_FLAG));
    out
}

// ============================================================================
// Состав разделов: какие из них вообще есть на этой машине
// ============================================================================

/// Наблюдаемые факты о машине, от которых зависит состав разделов вкладки.
///
/// Решение «показывать ли раздел» живёт ЗДЕСЬ, а не в теле окна. Раньше оно
/// было размазано по постройке секций (`if not available(): return`,
/// `if self._de != "mate": return`, две проверки Plasma), и раздел, добавленный
/// в [`SECTIONS`], мог молча не появиться — либо появиться там, где ему нечего
/// показывать. Правило #12 в его втором смысле: перечисление сущностей ведётся
/// одним списком, а не набором условий по месту рисования.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SectionFacts {
    /// Обучение вообще доступно (собрано колесо с его API).
    pub onboarding_available: bool,
    /// Текущее окружение — MATE (иконки рабочего стола есть только там).
    pub is_mate: bool,
    /// Plasma запущена ЛИБО от неё остались копии панели.
    pub plasma_available: bool,
    /// В системе стоит форк оформления Plasma (AeroShell и родня).
    pub aeroshell_installed: bool,
}

impl SectionFacts {
    /// Факты из ответов живых проб — в том порядке, в каком их собирает окно.
    pub fn new(
        onboarding_available: bool,
        is_mate: bool,
        plasma_available: bool,
        aeroshell_installed: bool,
    ) -> Self {
        Self {
            onboarding_available,
            is_mate,
            plasma_available,
            aeroshell_installed,
        }
    }
}

/// Показывать ли раздел на этой машине.
///
/// Разделы, не названные здесь, показываются всегда: у них нет условия, при
/// котором им нечего показать (внутри они сами говорят «движок обоев не
/// поднялся», «Pillow не установлен» и так далее — это СОДЕРЖИМОЕ раздела, а
/// не его отсутствие).
pub fn section_visible(id: &str, facts: &SectionFacts) -> bool {
    match id {
        "obGroup" => facts.onboarding_available,
        "mateIconsGroup" => facts.is_mate,
        "plasmaGroup" => facts.plasma_available,
        "aeroGroup" => facts.aeroshell_installed,
        _ => true,
    }
}

/// Разделы, которые вкладка покажет на этой машине, в порядке [`SECTIONS`].
pub fn visible_sections(facts: &SectionFacts) -> Vec<&'static str> {
    section_order()
        .into_iter()
        .filter(|id| section_visible(id, facts))
        .collect()
}

// ============================================================================
// Цвета состояний
// ============================================================================
//
// Один цвет — одно решение. QML берёт его голым, само число живёт здесь, а
// записанный эталон держит сторож `qt_settings_parity::status_colors_are_frozen`
// (раскладку и краски карты `styles` — `layout_and_colors_are_frozen`).
// Прежняя сверка с QSS виджетной вкладки на Python снята: с 02.10.2026
// Python — только архив.

/// «Всё на месте»: установленный файловый менеджер, удачная проверка.
pub const COLOR_OK: &str = "#4ade80";

/// «Не установлено / остановить»: отметка отсутствия и кнопки остановки.
pub const COLOR_BAD: &str = "#ef4444";

/// «Внимание»: движок не поднялся, Pillow нет, mpv нет, импорт недоступен.
pub const COLOR_WARN: &str = "#f59e0b";

/// Заливка красной кнопки («Остановить живые обои», «Остановить все звуки»)
/// в покое и под курсором.
///
/// Числа перенесены один в один из QSS виджетной вкладки
/// (`rgba(220,38,38,0.18)` → `rgba(220,38,38,0.32)`): сам оттенок задаёт
/// [`COLOR_BAD`], здесь только его прозрачность. QML-страница красит теми же
/// значениями через палитру, поэтому обе вкладки гасят кнопку одинаково.
pub const DANGER_FILL_ALPHA: f64 = 0.18;
pub const DANGER_FILL_HOVER_ALPHA: f64 = 0.32;

/// Цвет отметки «установлен / не установлен» рядом с файловым менеджером —
/// пара к [`fm_badge`], которая отдаёт саму отметку.
pub fn fm_badge_color(installed: bool) -> &'static str {
    if installed {
        COLOR_OK
    } else {
        COLOR_BAD
    }
}

// ============================================================================
// Раскладка и оформление, которого нет в палитре скина
// ============================================================================
//
// Числа и краски виджетной вкладки, разбросанные по её компоновкам и таблицам
// стилей (`rexon/qt_settings/window.py`). QML-разметка своих не заводит: всё
// приходит отсюда картой `styles` (rust/rexon-gui/src/settings_page.rs) —
// тем же набором, каким красится виджетная версия. Совпадающие с соседними
// вкладками числа берутся у них по ссылке, а не второй копией (правило #12).

/// Поля страницы и шаг между её блоками — те же, что у «Расширений».
pub const PAGE_MARGIN: i32 = crate::qt_extensions::PAGE_MARGIN;
pub const PAGE_SPACING: i32 = crate::qt_extensions::PAGE_SPACING;

/// Группа-раздел: поля, шаг строк и скругление — общие со Steam-страницей.
pub const GROUP_MARGIN_H: i32 = crate::qt_steam::GROUP_MARGIN_H;
pub const GROUP_MARGIN_V: i32 = crate::qt_steam::GROUP_MARGIN_V;
pub const GROUP_SPACING: i32 = crate::qt_steam::GROUP_SPACING;
pub const GROUP_RADIUS: i32 = crate::qt_steam::GROUP_RADIUS;

/// Пара «фон, рамка» группы-раздела.
///
/// Нейтральный серый с прозрачностью, а не белый, как у Steam-страницы:
/// одинаково читается на тёмном и светлом скине, и своей развилки «тёмная
/// ли тема» у вкладки нет (`neutral_rgba`).
pub fn group_colors() -> (String, String) {
    (
        crate::ui_base::neutral_rgba(0.04),
        crate::ui_base::neutral_rgba(0.14),
    )
}

/// Полоса-предупреждение «нет polkit»: скругление, поле вокруг текста и
/// пара «фон, рамка».
pub const POLKIT_BANNER_RADIUS: i32 = 8;
pub const POLKIT_BANNER_PADDING: i32 = 10;
pub fn polkit_banner_colors() -> (String, String) {
    (
        crate::ui_base::neutral_rgba(0.10),
        crate::ui_base::neutral_rgba(0.25),
    )
}

/// Синяя кнопка действия («Применить», «Восстановить», «Размыть»…):
/// поля вокруг подписи и скругление. Подпись — жирная.
pub const PRIMARY_BTN_PADDING_V: i32 = 6;
pub const PRIMARY_BTN_PADDING_H: i32 = 14;
pub const PRIMARY_BTN_RADIUS: i32 = crate::qt_steam::BTN_RADIUS;

/// Краски синей кнопки действия.
pub struct PrimaryButtonColors {
    pub background: String,
    pub hover: String,
    pub pressed: String,
    pub border: String,
    pub disabled_background: String,
    pub disabled_border: String,
    pub disabled_text: String,
}

/// `rgba(r,g,b,a)` акцента продукта — та же запись, что в QSS виджетов.
fn accent_rgba(alpha: &str) -> String {
    let (r, g, b) = crate::qt_steam::ACCENT_RGB;
    format!("rgba({r},{g},{b},{alpha})")
}

/// Синяя кнопка действия: тот же голубой, что у остальных вкладок, но
/// прозрачнее (0.30 / 0.45 / 0.60) и с собственной рамкой 0.45 — у
/// «Сравнения» рамки нет, у Steam заливка 0.85. Выключенная кнопка серая,
/// а не бледная: фон, рамка и текст — нейтральные 0.15 / 0.25 / 0.7.
pub fn primary_button_colors() -> PrimaryButtonColors {
    PrimaryButtonColors {
        background: accent_rgba("0.30"),
        hover: accent_rgba("0.45"),
        pressed: accent_rgba("0.60"),
        border: accent_rgba("0.45"),
        disabled_background: crate::ui_base::neutral_rgba(0.15),
        disabled_border: crate::ui_base::neutral_rgba(0.25),
        disabled_text: crate::ui_base::neutral_rgba(0.7),
    }
}

/// Красная кнопка остановки («Остановить живые обои», «Остановить все»):
/// поля вокруг подписи; подпись — [`COLOR_BAD`] жирным, заливка —
/// [`DANGER_FILL_ALPHA`] / [`DANGER_FILL_HOVER_ALPHA`].
pub const DANGER_BTN_PADDING_V: i32 = 6;
pub const DANGER_BTN_PADDING_H: i32 = 12;

/// Пара «фон, текст» ВЫКЛЮЧЕННОЙ красной кнопки: серая, а не бледно-красная.
pub fn danger_button_disabled_colors() -> (String, String) {
    (
        crate::ui_base::neutral_rgba(0.12),
        crate::ui_base::neutral_rgba(0.6),
    )
}

/// Строка выбора файлового менеджера: поля, шаг, скругление и полоса фокуса
/// (голубая полупрозрачная подсветка под сфокусированной радиокнопкой).
pub const FM_ROW_MARGIN_H: i32 = 8;
pub const FM_ROW_MARGIN_V: i32 = 4;
pub const FM_ROW_SPACING: i32 = 8;
pub const FM_ROW_RADIUS: i32 = 4;
pub fn fm_row_focus_color() -> String {
    accent_rgba("0.22")
}

/// Наименьшие ширины подписей перед контролами — чтобы контролы в соседних
/// строках вставали в один столбец: «Скин:» и «Звуковая тема:», подпись
/// громкости обоев, «URL:», «Сила:», значение силы размытия, значение «NN %».
pub const FIELD_LABEL_MIN_WIDTH: i32 = 150;
pub const VOLUME_LABEL_MIN_WIDTH: i32 = 190;
pub const URL_LABEL_MIN_WIDTH: i32 = 60;
pub const BLUR_LABEL_MIN_WIDTH: i32 = 60;
pub const BLUR_VALUE_MIN_WIDTH: i32 = 48;
pub const AMBIENT_PERCENT_MIN_WIDTH: i32 = 42;

/// Наибольшие ширины ползунков громкости: обоев и фоновых звуков.
pub const VOLUME_SLIDER_MAX_WIDTH: i32 = 200;
pub const AMBIENT_SLIDER_MAX_WIDTH: i32 = 160;

/// Список играющих фоновых звуков: отступ сверху, шаг строк; строка —
/// поля, шаг, скругление, ширина кнопок «🔊» и «⏹».
pub const AMBIENT_LIST_TOP_MARGIN: i32 = 4;
pub const AMBIENT_LIST_SPACING: i32 = 4;
pub const AMBIENT_ROW_MARGIN_H: i32 = 8;
pub const AMBIENT_ROW_MARGIN_V: i32 = 4;
pub const AMBIENT_ROW_SPACING: i32 = 8;
pub const AMBIENT_ROW_RADIUS: i32 = 4;
pub const AMBIENT_ROW_BUTTON_WIDTH: i32 = 40;

/// Фон строки играющего звука.
///
/// Починка оригинала (правило #16): у виджетов строка красилась белым с
/// прозрачностью при ЛЮБОЙ теме — на светлой невидима. Светлой теме —
/// чёрный той же прозрачности.
pub fn ambient_row_color(dark: bool) -> String {
    if dark {
        "rgba(255,255,255,0.05)".to_string()
    } else {
        "rgba(0,0,0,0.05)".to_string()
    }
}

/// Кнопка «⏹» в строке звука: тот же красный, что у кнопок удаления
/// «Расширений» (220,80,80), но своя заливка 0.25 / 0.4 и поля 4 / 6.
pub const AMBIENT_STOP_COLOR: &str = crate::qt_extensions::DANGER_COLOR;
pub const AMBIENT_STOP_FILL_ALPHA: f64 = 0.25;
pub const AMBIENT_STOP_FILL_HOVER_ALPHA: f64 = 0.4;
pub const AMBIENT_STOP_PADDING_V: i32 = 4;
pub const AMBIENT_STOP_PADDING_H: i32 = 6;

/// Библиотека обоев: шаг сетки и её отступ сверху; плитка — скругление,
/// поля и шаг блока подписей, зазор кнопок, ширина кнопки «▶»; кегль
/// значка-заглушки; поле вокруг надписи «пусто».
pub const LIB_GRID_SPACING: i32 = 8;
pub const LIB_GRID_TOP_MARGIN: i32 = 4;
pub const LIB_CARD_RADIUS: i32 = 6;
pub const LIB_INFO_MARGIN_H: i32 = 10;
pub const LIB_INFO_MARGIN_V: i32 = 8;
pub const LIB_INFO_SPACING: i32 = 2;
pub const LIB_BUTTON_SPACING: i32 = 4;
pub const LIB_PREVIEW_BUTTON_WIDTH: i32 = 36;
pub const LIB_PLACEHOLDER_POINT_SIZE: i32 = 26;
pub const LIB_EMPTY_PADDING: i32 = 8;

/// Пара «рамка, рамка под курсором» плитки библиотеки — те же 0.12 / 0.30,
/// что у плиток «Расширений» и карточек Steam; фона у плитки нет.
pub fn lib_card_colors(dark: bool) -> (String, String) {
    let (_bg, border, hover) = crate::qt_extensions::card_colors(dark);
    (border, hover)
}

/// Фон заглушки на месте превью плитки (та же заливка, что у строки звука).
pub fn lib_placeholder_color(dark: bool) -> String {
    ambient_row_color(dark)
}

/// Ряд кнопок «послушать» звуковой темы, ряд и столбец кнопок шрифтов,
/// строка копии панели Plasma; блок «заголовок + подпись» перед кнопкой
/// («Запомнить текущий выбор», «Сохранить текущий вид панели») — без шага.
pub const SOUND_PREVIEW_SPACING: i32 = 6;
pub const FONT_ROW_SPACING: i32 = 6;
pub const FONT_COL_SPACING: i32 = 2;
pub const PLASMA_ROW_SPACING: i32 = 6;
pub const INFO_BOX_SPACING: i32 = 0;
