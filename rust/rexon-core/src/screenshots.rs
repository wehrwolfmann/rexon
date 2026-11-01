//! Скриншоты DE — порт детерминированных частей `rexon/screenshots.py`
//! (`DEScreenshotManager`).
//!
//! Портированы ТОЛЬКО чистые вход→выход куски (сверяются golden-тестом
//! `screenshots_parity.rs` с самим Python):
//!   * таблицы `DE_COLORS`, `DE_DISPLAY_NAMES`, `SCREENSHOT_URLS`, карта
//!     файлов Wikimedia (`filename_for_de`);
//!   * генерация встроенного SVG-плейсхолдера (`generate_svg`) — чистый текст,
//!     сверяется байт-в-байт для каждого DE;
//!   * выбор argv инструмента снятия по наличию (`screenshot_tools`,
//!     `select_screenshot_tool`) — наличие подаётся замыканием, сам subprocess
//!     НЕ исполняется;
//!   * имена файлов и кандидаты путей (`auto_name`, `custom_name`,
//!     `placeholder_name`, `screenshot_path_candidates`, `extra_candidates`,
//!     `save_ext`/`save_dest`);
//!   * URL-хелперы (`official_screenshot_urls`, `online_url`, `online_urls`);
//!   * разрешение реального CDN-URL через API Wikimedia (`wikimedia_api_url`,
//!     `parse_wikimedia_imageinfo` — порт `_resolve_via_api`);
//!   * приёмка скачанного ответа (`validate_image_response`, `retry_wait_secs`);
//!   * план автоскачивания (`auto_download_plan` — порт сборки `tasks` в
//!     `auto_download_all`, ФС-наблюдения подаются замыканиями).
//!
//! Побочные эффекты НЕ портированы (тонкая обёртка над ФС/подпроцессами/сетью
//! поверх этих чистых функций — как в `backup.rs`/`distro_compat.rs`):
//!   * реальное снятие скриншота (`subprocess.run`) — ядро отдаёт argv/план,
//!     исполнение при подключении к живому Python;
//!   * чтение/запись каталога скриншотов, `shutil.copy2`, `os.remove`;
//!   * скачивание по HTTP/Wikimedia API, параллельный `auto_download_all`.
//!
//! Квирки оригинала сохранены для паритета (помечены ниже).

use serde_json::Value;

use crate::i18n::{n_, tr, trc, trf};
use crate::net::{self, LogLevel, NetError, NetIo};

/// Подкаталог скриншотов под `$HOME` при СТАНДАРТНОМ корне конфигов
/// (`~/.config/rexon/screenshots`). Для построения пути не годится — только
/// для сверки с эталоном Python, снятым без `$XDG_CONFIG_HOME`; настоящий
/// путь даёт [`screenshot_dir_xdg`].
pub const SCREENSHOT_SUBDIR: &str = ".config/rexon/screenshots";

/// Абсолютный путь каталога скриншотов при стандартном корне конфигов.
///
/// ⚠️ Оставлена ради сигнатуры PyO3-моста. Зовущим в продукте нужна
/// [`screenshot_dir_xdg`]: без `$XDG_CONFIG_HOME` путь уезжает мимо того,
/// что читает Python (`DEScreenshotManager.SCREENSHOT_DIR` строится
/// `expand_user("~/.config/rexon/screenshots")`, то есть XDG-осведомлённо).
pub fn screenshot_dir(home: &str) -> String {
    screenshot_dir_xdg(None, home)
}

/// Каталог скриншотов с учётом `$XDG_CONFIG_HOME` — порт
/// `expand_user("~/.config/rexon/screenshots")`.
///
/// ⚠️ Починка: раньше путь склеивался литералом от голого `$HOME`. Python
/// при этом раскрывал его через `expand_user`, и на машине с нестандартным
/// `XDG_CONFIG_HOME` карусель вкладки «Рабочие столы» смотрела в ОДНУ
/// папку, а снятые скриншоты ложились в ДРУГУЮ: снимок сделан, а на
/// карточке по-прежнему рисованный плейсхолдер. Строится единой точкой
/// [`crate::utils::rexon_config_dir_at`] — как и все хранилища Rexon.
pub fn screenshot_dir_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!("{}/screenshots", crate::utils::rexon_config_dir_at(xdg_config_home, home))
}

/// Цвета градиента по умолчанию для неизвестного DE (`DE_COLORS.get` fallback).
pub const FALLBACK_COLORS: (&str, &str) = ("#607D8B", "#37474F");

/// Таблица цветов градиента SVG-плейсхолдера (`DEScreenshotManager.DE_COLORS`).
/// Порядок = порядок объявления в оригинале (важен для генераторов/сверки).
pub const DE_COLORS: &[(&str, &str, &str)] = &[
    ("cinnamon-mint", "#4CAF50", "#2E7D32"),
    ("cinnamon", "#8BC34A", "#558B2F"),
    ("xfce", "#2196F3", "#1565C0"),
    ("mate", "#4CAF50", "#1B5E20"),
    ("gnome", "#3F51B5", "#1A237E"),
    ("kde", "#1565C0", "#0D47A1"),
    ("budgie", "#5E35B1", "#311B92"),
    ("pantheon", "#3689E6", "#0D52BF"),
    ("deepin", "#0081FF", "#0050A0"),
    ("cosmic", "#E64A19", "#BF360C"),
    ("hyprland", "#00BCD4", "#006064"),
    ("lxqt", "#03A9F4", "#01579B"),
    ("niri", "#EC407A", "#AD1457"),
    ("sway", "#26A69A", "#00695C"),
    ("river", "#5C6BC0", "#283593"),
    ("wayfire", "#FF7043", "#BF360C"),
];

/// Человекочитаемые имена DE (`DEScreenshotManager.DE_DISPLAY_NAMES`).
pub const DE_DISPLAY_NAMES: &[(&str, &str)] = &[
    ("cinnamon-mint", "Cinnamon Mint"),
    ("cinnamon", "Cinnamon"),
    ("xfce", "XFCE"),
    ("mate", "MATE"),
    ("gnome", "GNOME"),
    ("kde", "KDE Plasma"),
    ("budgie", "Budgie"),
    ("pantheon", "Pantheon"),
    ("deepin", "Deepin"),
    ("cosmic", "COSMIC"),
    ("hyprland", "Hyprland"),
    ("lxqt", "LXQt"),
    ("niri", "niri"),
    ("sway", "Sway"),
    ("river", "River"),
    ("wayfire", "Wayfire"),
];

/// Как Rexon называет рабочий стол, которого не опознал. Одно имя на весь
/// продукт: его отдают и [`display_name`], и `qt_settings::de_display`.
///
/// ⚠️ В фразу его подставлять ТОЛЬКО через [`place_de_name`]: имена столов
/// («KDE», «Sway») не склоняются, а это — склоняется, и прямая подстановка
/// давала «для ВАШ РАБОЧИЙ СТОЛ».
///
/// Это `msgid`; показывается перевод — [`unknown_de_name`].
pub const UNKNOWN_DE_NAME: &str = n_("YOUR DESKTOP");

/// [`UNKNOWN_DE_NAME`] на языке интерфейса — само имя, без предлога
/// («Текущее окружение: <b>ВАШ РАБОЧИЙ СТОЛ</b>»).
pub fn unknown_de_name() -> String {
    // TRANSLATORS: name shown for a desktop environment Rexon couldn't
    // recognize, e.g. “Current desktop: YOUR DESKTOP”. Capitals match how
    // known names (KDE, GNOME) are shown.
    tr(UNKNOWN_DE_NAME)
}

/// Неопознанный стол внутри фразы, после предлога («for your desktop»).
///
/// Для языков, где имя не склоняется: одна форма на все предлоги. Русский
/// склоняет его сам — по таблице падежей в [`place_de_name`].
fn unknown_de_in_sentence() -> String {
    // TRANSLATORS: the unrecognized desktop inside a sentence, after a
    // preposition: “Restored 2 apps for your desktop”.
    trc("unknown desktop in a sentence", "your desktop")
}

/// Имя — неопознанный стол (на любом языке интерфейса)?
fn is_unknown_de_name(name: &str) -> bool {
    name == UNKNOWN_DE_NAME || name == unknown_de_name()
}

/// Показанное имя стола — заглушка неопознанного (`msgid` или перевод)?
/// Зовущим, у которых для неопознанного стола своя целая фраза.
pub fn is_unknown_de_display(name: &str) -> bool {
    is_unknown_de_name(name)
}

/// Не опознан ли стол: пусто, `unknown` (опознание окружения) или `generic`
/// (опознание движка обоев).
pub fn is_unknown_de(de: &str) -> bool {
    let de = de.trim();
    de.is_empty() || de.eq_ignore_ascii_case("unknown") || de.eq_ignore_ascii_case("generic")
}

/// Форма [`UNKNOWN_DE_NAME`] после слова, которое требует падежа, либо
/// `None` — слово падежа не требует (двоеточие, тег, существительное).
///
/// Русские падежи — только после русского слова (в русском шаблоне). После
/// английского предлога («for %1», «on %1») — одна форма «внутри фразы» из
/// каталога: английскому таблица падежей ни к чему. Прочие места
/// («Restore %1 Apps») предлога не несут — там нужна целая фраза у
/// зовущего, а не склейка.
fn unknown_de_form(word: &str) -> Option<String> {
    const EN_PREPOSITIONS: &[&str] = &[
        "for", "on", "in", "at", "of", "with", "to", "from", "by", "under", "about", "into",
        "onto", "without",
    ];
    if EN_PREPOSITIONS.contains(&word) {
        return Some(unknown_de_in_sentence());
    }
    russian_unknown_de_form(word).map(str::to_string)
}

/// Русская форма «вашего рабочего стола» по предшествующему слову.
fn russian_unknown_de_form(word: &str) -> Option<&'static str> {
    match word {
        // родительный
        "для" | "у" | "из" | "от" | "без" | "до" | "около" | "после" | "кроме" | "среди"
        | "окружения" | "приложения" => Some("вашего рабочего стола"),
        // предложный
        "на" | "в" | "во" | "о" | "об" | "при" => Some("вашем рабочем столе"),
        // дательный
        "к" | "ко" | "по" => Some("вашему рабочему столу"),
        // творительный
        "с" | "со" | "под" | "над" | "перед" | "за" | "между" => Some("вашим рабочим столом"),
        _ => None,
    }
}

/// Подставить имя стола на место `slot` в шаблоне — грамотно.
///
/// Известное имя подставляется как есть. [`UNKNOWN_DE_NAME`] после слова,
/// требующего падежа, заменяется формой в этом падеже: «для {0}» → «для
/// вашего рабочего стола», «на {de}» → «на вашем рабочем столе». Имя в
/// кавычках после «окружения» уходит вместе с этим словом: «Для окружения
/// «{0}»» → «Для вашего рабочего стола». В остальных местах («Текущее
/// окружение: <b>{0}</b>») остаётся именительный падеж.
pub fn place_de_name(template: &str, slot: &str, name: &str) -> String {
    if slot.is_empty() || !is_unknown_de_name(name) {
        return template.replace(slot, name);
    }
    let name = unknown_de_name();
    let name = name.as_str();
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(pos) = rest.find(slot) {
        let head = &rest[..pos];
        let mut after = &rest[pos + slot.len()..];
        // Кавычки у русского шаблона — «», у английского — “”.
        let quote = ['«', '“'].into_iter().find(|q| head.ends_with(*q));
        let quoted = quote.is_some();
        let bare = quote.and_then(|q| head.strip_suffix(q)).unwrap_or(head);
        let form = bare.strip_suffix(' ').and_then(|before| {
            let word_len: usize = before
                .chars()
                .rev()
                .take_while(|c| c.is_alphabetic())
                .map(char::len_utf8)
                .sum();
            let word = &before[before.len() - word_len..];
            let form = unknown_de_form(&word.to_lowercase())?;
            Some((before, word, form))
        });
        match form {
            Some((before, word, form)) => {
                if word.to_lowercase() == "окружения" && quoted {
                    // «Для окружения «X»» — слово уходит вместе с кавычками.
                    out.push_str(&before[..before.len() - word.len()]);
                } else {
                    // «for “%1”» → «for your desktop»: предлог остаётся,
                    // кавычки (если были) уходят.
                    out.push_str(before);
                    out.push(' ');
                }
                out.push_str(&form);
                if quoted {
                    after = after.strip_prefix(['»', '”']).unwrap_or(after);
                }
            }
            None => {
                out.push_str(head);
                out.push_str(name);
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Цвета градиента для DE — `DE_COLORS.get(de, FALLBACK)`.
pub fn de_colors(de: &str) -> (&'static str, &'static str) {
    DE_COLORS
        .iter()
        .find(|(k, _, _)| *k == de)
        .map(|(_, a, b)| (*a, *b))
        .unwrap_or(FALLBACK_COLORS)
}

/// Отображаемое имя DE — `DE_DISPLAY_NAMES.get(de, de.upper())`.
///
/// Квирк сохранён: для неизвестного DE fallback = `de.upper()` (Python
/// `str.upper()`), в Rust `to_uppercase()` — эквивалент для ASCII-id
/// (намеренное расхождение сохранено для паритета: id всех DE — ASCII).
pub fn display_name(de: &str) -> String {
    // ⭐ Починка (правило #13): неопознанный стол назывался «UNKNOWN» или
    // пустой строкой — и в подписи вкладки стояло «Для окружения «»».
    if is_unknown_de(de) {
        return unknown_de_name();
    }
    table_display_name(de)
}

/// Имя по таблице без подмены неопознанного стола — для картинки-заглушки
/// [`generate_svg`]: она сверяется с оригиналом байт в байт, и у пустого id
/// подпись на ней пустая. В тексты для человека идёт [`display_name`].
fn table_display_name(de: &str) -> String {
    DE_DISPLAY_NAMES
        .iter()
        .find(|(k, _)| *k == de)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| de.to_uppercase())
}

/// Встроенный SVG-плейсхолдер для DE (`_generate_svg`) — чистый текст,
/// байт-в-байт как оригинал. Без завершающего перевода строки.
pub fn generate_svg(de: &str) -> String {
    generate_svg_for(de, PlaceholderState::Loading)
}

/// Заглушка с подписью под состояние закачки. Отличается от [`generate_svg`]
/// только нижней строкой: у [`PlaceholderState::Loading`] байты те же, что
/// у оригинала.
pub fn generate_svg_for(de: &str, state: PlaceholderState) -> String {
    let caption = state.caption();
    let (bg1, bg2) = de_colors(de);
    let display = table_display_name(de);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"480\" height=\"160\" viewBox=\"0 0 480 160\">\n  \
<defs>\n    \
<linearGradient id=\"bg\" x1=\"0%\" y1=\"0%\" x2=\"100%\" y2=\"100%\">\n      \
<stop offset=\"0%\" style=\"stop-color:{bg1}\"/>\n      \
<stop offset=\"100%\" style=\"stop-color:{bg2}\"/>\n    \
</linearGradient>\n  \
</defs>\n  \
<rect width=\"480\" height=\"160\" rx=\"10\" fill=\"url(#bg)\"/>\n  \
<rect y=\"0\" width=\"480\" height=\"24\" fill=\"rgba(0,0,0,0.25)\"/>\n  \
<rect x=\"8\" y=\"6\" width=\"10\" height=\"10\" rx=\"5\" fill=\"#FF5F56\"/>\n  \
<rect x=\"26\" y=\"6\" width=\"10\" height=\"10\" rx=\"5\" fill=\"#FFBD2E\"/>\n  \
<rect x=\"44\" y=\"6\" width=\"10\" height=\"10\" rx=\"5\" fill=\"#27C93F\"/>\n  \
<rect x=\"60\" y=\"42\" width=\"360\" height=\"98\" rx=\"8\" fill=\"rgba(255,255,255,0.10)\" stroke=\"rgba(255,255,255,0.18)\" stroke-width=\"1\"/>\n  \
<text x=\"240\" y=\"98\" font-family=\"sans-serif\" font-size=\"28\" font-weight=\"bold\" fill=\"white\" text-anchor=\"middle\">{display}</text>\n  \
<text x=\"240\" y=\"148\" font-family=\"sans-serif\" font-size=\"11\" fill=\"rgba(255,255,255,0.45)\" text-anchor=\"middle\">{caption}</text>\n\
</svg>"
    )
}

// ── Имена файлов ───────────────────────────────────────────────────────────

/// Имя авто-снятого скриншота — `{de}_auto.png`.
pub fn auto_name(de: &str) -> String {
    format!("{de}_auto.png")
}

/// Имя пользовательского скриншота — `{de}.png` (`set_custom_screenshot`).
pub fn custom_name(de: &str) -> String {
    format!("{de}.png")
}

/// Имя файла SVG-плейсхолдера — `{de}_placeholder.svg`.
pub fn placeholder_name(de: &str) -> String {
    format!("{de}_placeholder.svg")
}

// ── Состояние заглушки ─────────────────────────────────────────────────────
//
// Раньше заглушка всегда обещала «Скриншот загружается…» — и без сети
// обещала вечно: покупатель ждал того, что не придёт (правило #13). Теперь
// подпись честная и зависит от того, что знает ядро о закачке.

/// Что говорит заглушка вместо снимка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderState {
    /// Закачка снимков этого стола идёт.
    Loading,
    /// Закачка кончилась ничем: сети нет либо источник не ответил.
    NoInternet,
    /// Скачивать неоткуда — у стола нет ни одного адреса снимка.
    NoScreenshot,
}

/// Подпись заглушки на время закачки — текст оригинала.
pub const CAPTION_LOADING: &str = n_("Loading screenshot…");
/// Подпись заглушки, когда закачка не удалась.
pub const CAPTION_NO_INTERNET: &str = n_("The screenshot will appear once you're online");
/// Подпись заглушки стола без источника снимков.
pub const CAPTION_NO_SCREENSHOT: &str = n_("No screenshot of this desktop yet");

impl PlaceholderState {
    /// Нижняя строка заглушки на языке интерфейса (`CAPTION_*` — `msgid`).
    pub fn caption(self) -> String {
        tr(match self {
            PlaceholderState::Loading => CAPTION_LOADING,
            PlaceholderState::NoInternet => CAPTION_NO_INTERNET,
            PlaceholderState::NoScreenshot => CAPTION_NO_SCREENSHOT,
        })
    }
}

/// Имя файла заглушки в данном состоянии. Своё имя на каждое состояние —
/// потому что QML кэширует картинку по адресу: перепиши один файл, и окно
/// показывало бы старую подпись. У `Loading` имя прежнее
/// ([`placeholder_name`]), общее с Python.
pub fn placeholder_name_for(de: &str, state: PlaceholderState) -> String {
    match state {
        PlaceholderState::Loading => placeholder_name(de),
        PlaceholderState::NoInternet => format!("{de}_placeholder_offline.svg"),
        PlaceholderState::NoScreenshot => format!("{de}_placeholder_none.svg"),
    }
}

/// Есть ли у стола откуда скачать снимки — список [`SCREENSHOT_URLS`], по
/// которому идёт автоскачивание.
pub fn has_download_source(de: &str) -> bool {
    SCREENSHOT_URLS.iter().any(|(d, urls)| *d == de && !urls.is_empty())
}

/// Состояние заглушки: `pending` — ждёт ли стол своей пачки закачки.
pub fn placeholder_state(de: &str, pending: bool) -> PlaceholderState {
    if !has_download_source(de) {
        PlaceholderState::NoScreenshot
    } else if pending {
        PlaceholderState::Loading
    } else {
        PlaceholderState::NoInternet
    }
}

// ── Кандидаты путей (без ФС-проверок; проверки — эффект в обёртке) ──────────

/// Порядок суффиксов при поиске скриншота (`get_screenshot_path`).
pub const PATH_SUFFIXES: &[&str] = &["", "_auto"];
/// Порядок расширений при поиске скриншота.
pub const PATH_EXTS: &[&str] = &[".png", ".jpg", ".webp"];
/// Минимальный размер файла-кандидата в байтах — принимается при `size > 100`.
pub const MIN_SIZE: u64 = 100;

/// Кандидаты путей основного скриншота в порядке перебора
/// (`get_screenshot_path`): `(suffix, ext)` для `suffix ∈ {"", "_auto"}` и
/// `ext ∈ {.png, .jpg, .webp}`. Живой перебор берёт первый существующий с
/// `size > MIN_SIZE`. Тот же список кандидатов использует `delete_screenshot`
/// (но без фильтра размера — удаляет любой существующий).
pub fn screenshot_path_candidates(dir: &str, de: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(PATH_SUFFIXES.len() * PATH_EXTS.len());
    for suffix in PATH_SUFFIXES {
        for ext in PATH_EXTS {
            out.push(format!("{dir}/{de}{suffix}{ext}"));
        }
    }
    out
}

/// Кандидаты дополнительных скриншотов карусели (`get_screenshot_paths`):
/// `{de}_1..{de}_5` × `{.png, .jpg, .webp}`, плоским списком в порядке
/// перебора. Живой перебор для каждого индекса берёт ПЕРВОЕ существующее
/// расширение (`break`) — эта ФС-семантика в обёртке, здесь только кандидаты.
pub fn extra_candidates(dir: &str, de: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(5 * PATH_EXTS.len());
    for i in 1..=5 {
        for ext in PATH_EXTS {
            out.push(format!("{dir}/{de}_{i}{ext}"));
        }
    }
    out
}

// ── Снятие скриншота (план argv; subprocess — эффект в обёртке) ─────────────

/// Сообщение об отсутствии инструментов снятия (`take_screenshot`).
pub fn no_tools_msg() -> String {
    tr("No screenshot program found (grim, spectacle, gnome-screenshot, scrot, maim)")
}

/// Кандидаты инструментов снятия скриншота в порядке приоритета
/// (`take_screenshot`): каждый — полный argv, пишущий в `{dir}/{de}_auto.png`.
/// Порядок сохранён: scrot → gnome-screenshot → maim → spectacle → import.
pub fn screenshot_tools(dir: &str, de: &str) -> Vec<Vec<String>> {
    let out = format!("{dir}/{de}_auto.png");
    vec![
        // Wayland-первыми: на sway/Hyprland/niri (цель №4 списка платформ)
        // grim — единственный работающий вариант, а X11-утилиты там либо
        // отсутствуют, либо снимают чёрный кадр через XWayland.
        vec!["grim".into(), out.clone()],
        vec!["spectacle".into(), "--nonotify".into(), "-b".into(),
             "-o".into(), out.clone()],
        vec!["gnome-screenshot".into(), "-f".into(), out.clone()],
        vec!["scrot".into(), "-z".into(), out.clone()],
        vec!["maim".into(), out.clone()],
        vec!["import".into(), "-window".into(), "root".into(), out],
    ]
}

/// Выбор argv первого доступного инструмента снятия (`take_screenshot` без
/// исполнения subprocess): наличие команды `cmd[0]` подаётся замыканием
/// `is_available` (порт `safe_check_command` = `shutil.which(cmd) is not None`).
/// Возвращает argv выбранного инструмента, либо None если ни один недоступен —
/// вызывающему (Python) остаётся исполнить subprocess и проверить результат.
pub fn select_screenshot_tool<F>(dir: &str, de: &str, is_available: F) -> Option<Vec<String>>
where
    F: Fn(&str) -> bool,
{
    screenshot_tools(dir, de)
        .into_iter()
        .find(|cmd| is_available(&cmd[0]))
}

// ── Сохранение скачанных данных (`_save_screenshot_data`) ───────────────────

/// Расширение файла по Content-Type (`_save_screenshot_data`): webp → `.webp`,
/// jpeg/jpg → `.jpg`, иначе `.png`. `None` трактуется как пустая строка.
pub fn save_ext(content_type: Option<&str>) -> &'static str {
    let ct = content_type.unwrap_or("");
    if ct.contains("webp") {
        ".webp"
    } else if ct.contains("jpeg") || ct.contains("jpg") {
        ".jpg"
    } else {
        ".png"
    }
}

/// Путь сохранения скачанного скриншота — `{dir}/{de}{ext}` по Content-Type.
pub fn save_dest(dir: &str, de: &str, content_type: Option<&str>) -> String {
    format!("{dir}/{de}{}", save_ext(content_type))
}

// ── Wikimedia-имена и URL скриншотов ───────────────────────────────────────

/// Имя файла на Wikimedia Commons для DE (`_filename_for_de`), None если нет.
/// Квирк: для `cinnamon` (в отличие от `cinnamon-mint`) записи нет.
pub fn filename_for_de(de: &str) -> Option<&'static str> {
    let m: &[(&str, &str)] = &[
        ("kde", "KDE_neon_6_Breeze_Desktop_English.png"),
        ("gnome", "Fedora_Workstation_39_Desktop_English.png"),
        ("xfce", "Xfce_4.18_in_Arch_Linux.png"),
        (
            "mate",
            "Ubuntu_MATE_24.04_LTS_default_desktop_-_English.png",
        ),
        ("cinnamon-mint", "LinuxMint22-Wilma-English.png"),
        ("budgie", "Ubuntu_Budgie_22.04_Desktop.png"),
        ("pantheon", "Elementary_OS_8.0_default_desktop_-_English.png"),
        ("deepin", "Deepin_Linux_23.1_default_desktop_-_English.png"),
        (
            "cosmic",
            "Pop!_OS_24.04_Alpha_7_default_desktop_-_English.png",
        ),
        ("hyprland", "Hyprland_screen.png"),
        (
            "lxqt",
            "Lubuntu_24.04_LTS_default_desktop_-_English.jpg",
        ),
        ("niri", "Screenshot_LXQt-Niri_2025-06-11.png"),
        ("sway", "Sway_1.4_Window_Manager.png"),
    ];
    m.iter().find(|(k, _)| *k == de).map(|(_, v)| *v)
}

/// Прямые URL скриншотов по DE (`SCREENSHOT_URLS`), порядок сохранён.
pub const SCREENSHOT_URLS: &[(&str, &[&str])] = &[
    (
        "cinnamon-mint",
        &[
            "https://www.linuxmint.com/web/img/screenshots/c1.jpg",
            "https://www.linuxmint.com/web/img/screenshots/c2.jpg",
            "https://commons.wikimedia.org/wiki/Special:FilePath/LinuxMint22-Wilma-English.png",
        ],
    ),
    (
        "xfce",
        &[
            "https://cdn.xfce.org/about/screenshots/4.20-1.png",
            "https://cdn.xfce.org/about/screenshots/4.18-1.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Xfce_4.18_in_Arch_Linux.png",
        ],
    ),
    (
        "mate",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Ubuntu_MATE_24.04_LTS_default_desktop_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Ubuntu_MATE_24.04_LTS_menu_-_English.png",
            "https://mate-desktop.org/screens/screenshot.jpg",
        ],
    ),
    (
        "gnome",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Fedora_Workstation_39_Desktop_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Fedora_35_GNOME_Activities_Menu_en.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Fedora_41_With_Gnome_47.png",
        ],
    ),
    (
        "kde",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/KDE_neon_6_Breeze_Desktop_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Arco_Linux_24.05.11_Plasma_menu_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Arco_Linux_24.05.11_Plasma_Dolphin_24.02.2_Breeze_-_English.png",
        ],
    ),
    (
        "budgie",
        &[
            "https://buddiesofbudgie.org/images/blog/2023/01/MainDesktop.jpg",
            "https://buddiesofbudgie.org/images/blog/2023/01/BudgieMenu.png",
            "https://buddiesofbudgie.org/images/blog/2023/01/Raven-WidgetView.jpg",
        ],
    ),
    (
        "pantheon",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Elementary_OS_8.0_default_desktop_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Elementary_OS_8.0_menu_1_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Elementary_OS_8.0_Settings_-_English.png",
        ],
    ),
    (
        "deepin",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Deepin_Linux_23.1_default_desktop_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Deepin_Linux_25_Alpha_default_desktop_-_English.png",
        ],
    ),
    (
        "cosmic",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Pop!_OS_24.04_Alpha_7_default_desktop_-_English.png",
            "https://cdn11.bigcommerce.com/s-pywjnxrcr2/images/stencil/original/image-manager/cosmic-screensr20.png",
            "https://cdn11.bigcommerce.com/s-pywjnxrcr2/images/stencil/original/image-manager/themingr5.png",
        ],
    ),
    (
        "hyprland",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Hyprland_screen.png",
            "https://github.com/YAWTUUYAS/hyprland-riced/raw/main/screenshots/desktop.png",
            "https://github.com/YAWTUUYAS/hyprland-riced/raw/main/screenshots/wofi.png",
        ],
    ),
    (
        "lxqt",
        &[
            "https://lxqt-project.org/images/screenshots/light.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Lubuntu_24.04_LTS_default_desktop_-_English.jpg",
            "https://lxqt-project.org/images/screenshots/pcmanfm-qt.png",
        ],
    ),
    (
        "niri",
        &[
            "https://github.com/user-attachments/assets/dea5909e-1859-4aaa-9d88-d37f9663e00b",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Screenshot_LXQt-Niri_2025-06-11.png?width=1200",
        ],
    ),
    (
        "sway",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Sway_1.4_Window_Manager.png?width=1200",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Sway_window_manager_on_two_displays.jpg?width=1200",
        ],
    ),
    // River: ни у river-classic, ни на Wikimedia Commons снимка экрана нет
    // (на Commons — только логотип), поэтому строки нет: снимки River —
    // свои, поставляются в пакете (`BUNDLED_SUBDIR`), а не скачиваются.
    // Wayfire: галерея wf-shell (MIT; панель и фон по умолчанию — ровно тот
    // вид, что ставит Rexon) и снимки эффектов из официальной вики Wayfire
    // (куб рабочих столов, переключатель окон). На Commons — только логотип.
    (
        "wayfire",
        &[
            "https://raw.githubusercontent.com/WayfireWM/wf-shell/master/gallery/default.png",
            "https://raw.githubusercontent.com/WayfireWM/wf-shell/master/gallery/legacy.png",
            "https://raw.githubusercontent.com/WayfireWM/wayfire-wiki/master/images/cube-normal.png",
            "https://raw.githubusercontent.com/WayfireWM/wayfire-wiki/master/images/switcher.png",
        ],
    ),
];

/// Все URL скриншотов DE (`get_online_screenshot_urls` = `list(...)`).
pub fn online_urls(de: &str) -> Vec<&'static str> {
    SCREENSHOT_URLS
        .iter()
        .find(|(k, _)| *k == de)
        .map(|(_, v)| v.to_vec())
        .unwrap_or_default()
}

/// Первый URL каждого DE с непустым списком (`OFFICIAL_SCREENSHOT_URLS`).
pub fn official_screenshot_urls() -> Vec<(&'static str, &'static str)> {
    SCREENSHOT_URLS
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (*k, v[0]))
        .collect()
}

// ── Wikimedia API: разрешение реального CDN-URL (`_resolve_via_api`) ────────

/// User-Agent запросов к Wikimedia (`ua_bot` в `download_screenshot_from_url`).
/// Wikimedia требует информативный UA и режет анонимные загрузки.
pub const WIKIMEDIA_USER_AGENT: &str = "Rexon/5.0 (Linux DE Manager; \
https://github.com/rexon-app/rexon; rexon@proton.me) Python/urllib";

/// URL запроса `imageinfo` к API Commons — порт `api_url` из `_resolve_via_api`.
/// `iiurlwidth=800` просит миниатюру нужной ширины (`thumburl` в ответе).
pub fn wikimedia_api_url(filename: &str) -> String {
    format!(
        "https://commons.wikimedia.org/w/api.php?action=query&titles=File:{}\
&prop=imageinfo&iiprop=url&iiurlwidth=800&format=json&formatversion=2",
        url_quote(filename)
    )
}

/// Реальный CDN-URL картинки из ответа `imageinfo` — порт разбора в
/// `_resolve_via_api`: `thumburl` (миниатюра 800px), иначе `url` (оригинал),
/// иначе `None`.
///
/// `None` также на любом «не том» ответе: нет `query.pages`, страница помечена
/// `missing` (файла на Commons нет), `imageinfo` отсутствует или пуст.
///
/// ⚠️ Починены хрупкие места оригинала (`pages[0].get("imageinfo", [{}])[0]`):
/// пустой список `imageinfo` давал `IndexError`, а ответ старого
/// `formatversion=1` (там `pages` — объект, а не список) — `KeyError`. Оба
/// исключения глушились внешним `except`, то есть отличить «файла нет» от
/// «разбор сломался» было нельзя. Здесь оба случая — честный `None`.
pub fn parse_wikimedia_imageinfo(data: &Value) -> Option<String> {
    let pages = data.get("query")?.get("pages")?;
    // formatversion=2 → массив; formatversion=1 → объект {pageid: page}.
    let page = match pages {
        Value::Array(a) => a.first()?,
        Value::Object(o) => o.values().next()?,
        _ => return None,
    };
    // `missing` проверяется на ИСТИННОСТЬ, как в Python: formatversion=2 ставит
    // его только у отсутствующих файлов, но явный `"missing": false` тоже
    // должен читаться как «файл есть».
    if json_truthy(page.get("missing")) {
        return None;
    }
    let ii = page.get("imageinfo")?.as_array()?.first()?;
    for key in ["thumburl", "url"] {
        if let Some(u) = ii.get(key).and_then(Value::as_str) {
            if !u.is_empty() {
                return Some(u.to_string());
            }
        }
    }
    None
}

// ── Проверка скачанного ответа (`_download_url`) ────────────────────────────

/// Минимальный размер принимаемой картинки в байтах (`len(data) < 512`).
pub const MIN_DOWNLOAD_BYTES: usize = 512;
/// Число попыток скачивания при HTTP 429 (`retries=3`).
pub const DOWNLOAD_RETRIES: u32 = 3;
/// Сообщение после исчерпания попыток при 429.
pub fn rate_limit_msg() -> String {
    tr("Too many requests (429)")
}

/// Годен ли ответ как картинка — порт проверок `_download_url`:
/// `Content-Type` обязан начинаться с `image/`, тело — быть не меньше
/// [`MIN_DOWNLOAD_BYTES`]. `Err` — текст причины ровно как в оригинале (он
/// уходит в UI и в лог).
///
/// Отсекает главный практический случай «не тот ответ»: HTML-страница ошибки
/// (403/404/капча) вместо картинки — у неё `Content-Type: text/html`.
pub fn validate_image_response(content_type: Option<&str>, body_len: usize) -> Result<(), String> {
    let ct = content_type.unwrap_or("");
    if !ct.starts_with("image/") {
        // TRANSLATORS: download check failed; %1 is the content type the server sent.
        return Err(trf("Not an image: %1", &[&ct]));
    }
    if body_len < MIN_DOWNLOAD_BYTES {
        return Err(tr("The file is too small"));
    }
    Ok(())
}

/// Пауза перед повтором при HTTP 429 — порт `wait = (attempt + 1) * 3`
/// (`attempt` считается с нуля).
///
/// Значение больше не своё: оно берётся из ОБЩЕГО механизма повторов
/// ([`crate::net::retry_delay_ms`]), где та же пара 3 с / 6 с закреплена
/// константой [`crate::net::RATE_LIMIT_DELAY_MS`]. Функция оставлена ради
/// сверки с оригиналом — правило одно, копий больше нет.
pub fn retry_wait_secs(attempt: u32) -> u64 {
    let rate_limited = NetError::Status { code: 429, reason: String::new() };
    net::retry_delay_ms(attempt + 1, &rate_limited) / 1000
}

// ── Скачивание снимка (`download_screenshot_from_url`) ─────────────────────
//
// Раньше здесь заканчивалась портированная часть: сам HTTP делал Python
// (`urllib`) со своим циклом повторов на один только HTTP 429. Обрыв
// соединения — тот же класс отказа, из-за которого гибли превью каталога, —
// не повторялся вовсе. Теперь запрос идёт через общий сетевой слой ядра
// (`net.rs`), значит и повторы у снимков ровно те же, что у каталога тем
// (правило #12), а Python-копии urllib-логики не остаётся.

/// Таймаут скачивания самой картинки — порт `urlopen(req, timeout=20)`.
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 20;
/// Таймаут запроса к API Commons — порт `urlopen(req, timeout=15)`.
pub const API_TIMEOUT_SECS: u64 = 15;

/// Причина отказа человеческим текстом — то, что оригинал отдаёт вторым
/// элементом кортежа и показывает в строке прогресса.
fn why(e: &NetError) -> String {
    match e {
        // Код ответа печатается как у оригинала (`f"HTTP {e.code}"`), кроме
        // 429: после исчерпания повторов он значит именно «лимит запросов».
        NetError::Status { code: 429, .. } => rate_limit_msg(),
        NetError::Status { code, .. } => format!("HTTP {code}"),
        // Сырой `str(e.reason)` оригинала («[Errno -2] Name or service not
        // known») покупателю ничего не говорит — правило #13.
        NetError::Transport { .. } => e.human(),
    }
}

/// Скачать картинку по одному URL — порт `_download_url`.
///
/// `Ok((байты, Content-Type))` либо причина отказа. Тип ответа проверяется ДО
/// чтения тела, как в оригинале: HTML-страница ошибки бывает в мегабайты.
/// Повторы — общие ([`net::with_retry`]): обрыв, таймаут, 5xx и 429; «файла
/// нет» (4xx) и «не изображение» не повторяются.
pub fn download_image<IO: NetIo + ?Sized>(
    io: &mut IO,
    url: &str,
    user_agent: &str,
) -> Result<(Vec<u8>, String), String> {
    let what = format!("Снимок рабочего стола {url}");
    // Внешний слой — сетевой сбой (его решает повторять `with_retry`),
    // внутренний — «ответ пришёл, но он не тот»: это НЕ сбой сети, второй
    // запрос вернёт ту же страницу, поэтому такой отказ окончателен.
    let res = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        io.open(url, user_agent, DOWNLOAD_TIMEOUT_SECS)?;
        let ctype = io.content_type();
        // Длину подставляем заведомо достаточную: на этом шаге проверяется
        // ТОЛЬКО тип — тела ещё нет и качать его незачем.
        if let Err(msg) = validate_image_response(Some(&ctype), MIN_DOWNLOAD_BYTES) {
            io.close();
            return Ok(Err(msg));
        }
        let mut body = Vec::new();
        loop {
            match io.read(DOWNLOAD_CHUNK) {
                Ok(c) if c.is_empty() => break,
                Ok(c) => body.extend_from_slice(&c),
                Err(e) => {
                    io.close();
                    return Err(e);
                }
            }
        }
        io.close();
        Ok(validate_image_response(Some(&ctype), body.len()).map(|()| (body, ctype)))
    });
    match res {
        Ok(inner) => inner,
        Err(e) => Err(why(&e)),
    }
}

/// Кусок чтения тела снимка. Тот же размер, что у каталога тем.
const DOWNLOAD_CHUNK: usize = 64 * 1024;

/// Скачать снимок рабочего стола `de` — порт `download_screenshot_from_url`
/// целиком: прямой URL, при неудаче — разрешение реального CDN-адреса через
/// API Commons и вторая попытка по нему.
///
/// `Ok((байты, Content-Type))` — что писать на диск (путь считает
/// [`save_dest`]); `Err` — готовая строка для покупателя.
pub fn download_screenshot<IO: NetIo + ?Sized>(
    io: &mut IO,
    de: &str,
    url: &str,
) -> Result<(Vec<u8>, String), String> {
    let direct = match download_image(io, url, WIKIMEDIA_USER_AGENT) {
        Ok(got) => return Ok(got),
        Err(info) => info,
    };
    io.log(
        LogLevel::Info,
        &format!("Прямой URL не сработал ({direct}), пробуем API…"),
    );
    let Some(filename) = filename_for_de(de) else {
        return Err(trf("Couldn't download the screenshot: %1", &[&direct]));
    };
    let api = wikimedia_api_url(filename);
    let cdn = net::with_retry(io, net::Retry::IDEMPOTENT, &format!("Commons API {api}"), |io| {
        io.get(&api, WIKIMEDIA_USER_AGENT, API_TIMEOUT_SECS)
    })
    .ok()
    .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
    .as_ref()
    .and_then(parse_wikimedia_imageinfo);
    let Some(cdn) = cdn else {
        // Оригинал в этой ветке докладывает причину ПЕРВОЙ неудачи, а не
        // сбоя API: для покупателя важно, что снимок взять неоткуда.
        return Err(trf("Couldn't download the screenshot: %1", &[&direct]));
    };
    download_image(io, &cdn, WIKIMEDIA_USER_AGENT).map_err(|info| format!("CDN: {info}"))
}

// ── План автоскачивания (`auto_download_all`) ──────────────────────────────

/// Одна задача автоскачивания: какому DE принадлежит, под каким тегом
/// сохранять (`{de}` для первого URL, `{de}_{idx}` для остальных) и что качать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDownloadTask {
    pub de: String,
    pub tag: String,
    pub url: String,
}

/// План автоскачивания: что качать и у каких DE всё уже на месте.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoDownloadPlan {
    pub tasks: Vec<AutoDownloadTask>,
    /// `(de, сколько файлов уже есть)` — по ним оригинал сразу пишет
    /// «уже есть (N шт.)» и ставит результат `True`, не качая ничего.
    pub already_complete: Vec<(String, usize)>,
}

/// Тег файла для i-го URL списка DE — порт `f"{de_name}{suffix}"`,
/// где `suffix = "" if idx == 0 else f"_{idx}"`.
pub fn auto_download_tag(de: &str, idx: usize) -> String {
    if idx == 0 {
        de.to_string()
    } else {
        format!("{de}_{idx}")
    }
}

/// Кандидаты путей для тега автоскачивания — `{dir}/{tag}{.png,.jpg,.webp}`.
/// Исполнитель проверяет их существование и размер (`> MIN_SIZE`).
pub fn tag_candidates(dir: &str, tag: &str) -> Vec<String> {
    PATH_EXTS.iter().map(|ext| format!("{dir}/{tag}{ext}")).collect()
}

/// План автоскачивания — порт сборки `tasks` в `auto_download_all`.
///
/// ФС не трогается: `existing_count(de)` отдаёт число уже найденных снимков DE
/// (порт `len(get_screenshot_paths(de))`), `tag_exists(tag)` — есть ли годный
/// файл под этим тегом (порт перебора [`tag_candidates`] с фильтром размера).
/// Обход DE идёт в порядке [`SCREENSHOT_URLS`], как в Python по dict.
pub fn auto_download_plan<C, E>(existing_count: C, tag_exists: E) -> AutoDownloadPlan
where
    C: Fn(&str) -> usize,
    E: Fn(&str) -> bool,
{
    let mut plan = AutoDownloadPlan::default();
    for (de, urls) in SCREENSHOT_URLS {
        let have = existing_count(de);
        // Снимков не меньше, чем URL-ов → у этого DE всё скачано.
        if have >= urls.len() {
            plan.already_complete.push(((*de).to_string(), have));
            continue;
        }
        for (idx, url) in urls.iter().enumerate() {
            let tag = auto_download_tag(de, idx);
            if tag_exists(&tag) {
                continue;
            }
            plan.tasks.push(AutoDownloadTask {
                de: (*de).to_string(),
                tag,
                url: (*url).to_string(),
            });
        }
    }
    plan
}

// ── Живой исполнитель автоскачивания (`auto_download_all`) ─────────────────
//
// До сих пор ядро держало только ПЛАН закачки, а исполнял его Python-хозяин
// (`DEScreenshotManager.auto_download_all`, воркер QThread вкладки). Rust-окно
// Python не зовёт, и на свежей машине покупатель видел бы в карточках одни
// заглушки — навсегда. Исполнитель переехал сюда целиком: одна копия на оба
// окна (правило #12), каталог, теги и имена файлов — ровно те же, что у
// Python, так что кэш у двух окон общий.

/// Годный ли файл снимка: существует и крупнее [`MIN_SIZE`] — порт
/// `os.path.exists(p) and os.path.getsize(p) > 100`.
pub fn usable_file(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > MIN_SIZE)
        .unwrap_or(false)
}

/// Снимки стола на диске в порядке карусели — порт `get_screenshot_paths`:
/// основной (первый годный кандидат), затем `_1`…`_5`, по первому годному
/// расширению на индекс.
pub fn screenshot_paths_on_disk(dir: &str, de: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(main) = screenshot_path_candidates(dir, de)
        .into_iter()
        .find(|p| usable_file(p))
    {
        out.push(main);
    }
    let extra = extra_candidates(dir, de);
    for group in extra.chunks(PATH_EXTS.len()) {
        if let Some(found) = group.iter().find(|p| usable_file(p)) {
            out.push(found.clone());
        }
    }
    out
}

// ── Поставляемые снимки ────────────────────────────────────────────────────
//
// Где снимка стола нет ни у проекта, ни на Wikimedia Commons (River), Rexon
// везёт свои снимки в пакете — в каталоге данных продукта
// (`data/assets/screenshots/` в дереве, `/usr/lib/rexon/rexon/assets/
// screenshots/` у пакета). Имена — те же, что у скачанных: `{de}.png`,
// `{de}_1.png`…, поэтому перебор общий ([`screenshot_paths_on_disk`]).

/// Подкаталог поставляемых снимков внутри каталога данных Rexon.
pub const BUNDLED_SUBDIR: &str = "assets/screenshots";

/// Каталоги поставляемых снимков по убыванию точности — те же раскладки, что
/// у данных продукта (`installer::package_data_dir_candidates`: дерево
/// разработчика, пакет, установка в систему и в домашнюю папку).
pub fn bundled_screenshot_dirs(repo_root: &str, home: &str) -> Vec<String> {
    crate::installer::package_data_dir_candidates(repo_root, home)
        .into_iter()
        .map(|dir| format!("{dir}/{BUNDLED_SUBDIR}"))
        .collect()
}

/// Снимки стола для карусели: поставляемые (из первого каталога, где у стола
/// они есть) идут первыми, за ними — скачанные в `dir`. Пусто — показывать
/// заглушку ([`placeholder_on_disk`]).
pub fn carousel_paths_on_disk(dir: &str, bundled_dirs: &[String], de: &str) -> Vec<String> {
    let mut out = bundled_dirs
        .iter()
        .map(|b| screenshot_paths_on_disk(b, de))
        .find(|found| !found.is_empty())
        .unwrap_or_default();
    out.extend(screenshot_paths_on_disk(dir, de));
    out
}

/// План автоскачивания над настоящим каталогом `dir` — те же наблюдения, что
/// собирает `auto_download_all` оригинала.
pub fn auto_download_plan_on_disk(dir: &str) -> AutoDownloadPlan {
    auto_download_plan(
        |de| screenshot_paths_on_disk(dir, de).len(),
        |tag| tag_candidates(dir, tag).iter().any(|p| usable_file(p)),
    )
}

/// Сколько снимков качается одновременно — `ThreadPoolExecutor(max_workers=4)`.
pub const AUTO_DOWNLOAD_WORKERS: usize = 4;

/// Итог одной задачи автоскачивания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDownloadOutcome {
    pub de: String,
    pub tag: String,
    /// Путь записанного файла либо причина отказа человеческим текстом.
    pub result: Result<String, String>,
    /// Это последняя задача своего стола: вся его пачка отработала.
    pub de_finished: bool,
    /// Сколько файлов стола скачано к этому моменту (включая эту задачу).
    pub de_downloaded: usize,
}

/// Записать скачанные байты — порт `_save_screenshot_data`: путь по
/// Content-Type считает [`save_dest`].
///
/// ⚠️ Починка оригинала (правило #16): Python писал прямо в итоговый файл, и
/// оборванная запись (закрыли окно посреди закачки) оставляла огрызок крупнее
/// 100 байт — его принимали за снимок и больше не докачивали, а карусель
/// показывала битую картинку. Здесь байты уходят общей дверью
/// [`crate::utils::atomic_write_bytes`]: рядом во временный файл и одним
/// шагом на место.
pub fn save_screenshot(dir: &str, tag: &str, data: &[u8], content_type: &str) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| trf("Write error: %1", &[&e]))?;
    let dest = save_dest(dir, tag, Some(content_type));
    crate::utils::atomic_write_bytes(&dest, data).map_err(|e| trf("Write error: %1", &[&e]))?;
    Ok(dest)
}

/// Исполнить план автоскачивания — порт `auto_download_all` целиком.
///
/// Транспорт даёт `make_io` (по одному на рабочий поток: соединения
/// переиспользуются внутри пачки); `on_task` зовётся по каждой завершённой
/// задаче В ПОТОКЕ ВЫЗЫВАЮЩЕГО — ему не нужно быть `Sync`, и окно может сразу
/// перерисовать карточку. Возвращает `(de, есть ли снимки)` в порядке
/// [`SCREENSHOT_URLS`] — словарь `results` оригинала. Сети нет — задачи просто
/// заканчиваются отказом; паники и ошибки наружу не выходят.
pub fn run_auto_download<IO, F, R>(dir: &str, make_io: F, on_task: R) -> Vec<(String, bool)>
where
    IO: NetIo,
    F: Fn() -> IO + Sync,
    R: FnMut(&AutoDownloadOutcome),
{
    run_plan(dir, auto_download_plan_on_disk(dir), make_io, on_task)
}

/// Исполнить уже построенный план — тело [`run_auto_download`].
fn run_plan<IO, F, R>(dir: &str, plan: AutoDownloadPlan, make_io: F, mut on_task: R) -> Vec<(String, bool)>
where
    IO: NetIo,
    F: Fn() -> IO + Sync,
    R: FnMut(&AutoDownloadOutcome),
{
    // `(de, скачано, отработало, всего)` — `de_counts` оригинала плюс счёт
    // завершённых, чтобы окно знало, когда пачка стола кончилась.
    let mut downloaded: Vec<(String, usize, usize, usize)> = Vec::new();
    for task in &plan.tasks {
        match downloaded.iter_mut().find(|(de, ..)| *de == task.de) {
            Some(slot) => slot.3 += 1,
            None => downloaded.push((task.de.clone(), 0, 0, 1)),
        }
    }
    if !plan.tasks.is_empty() {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let (tx, rx) = std::sync::mpsc::channel::<AutoDownloadOutcome>();
        std::thread::scope(|scope| {
            let workers = AUTO_DOWNLOAD_WORKERS.min(plan.tasks.len());
            for _ in 0..workers {
                let tx = tx.clone();
                let (next, tasks, make_io) = (&next, &plan.tasks, &make_io);
                scope.spawn(move || {
                    let mut io = make_io();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(task) = tasks.get(i) else { break };
                        let result = download_screenshot(&mut io, &task.tag, &task.url)
                            .and_then(|(body, ctype)| save_screenshot(dir, &task.tag, &body, &ctype));
                        let outcome = AutoDownloadOutcome {
                            de: task.de.clone(),
                            tag: task.tag.clone(),
                            result,
                            de_finished: false,
                            de_downloaded: 0,
                        };
                        if tx.send(outcome).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(tx);
            for mut outcome in rx {
                if let Some(slot) = downloaded.iter_mut().find(|(de, ..)| *de == outcome.de) {
                    slot.1 += usize::from(outcome.result.is_ok());
                    slot.2 += 1;
                    outcome.de_downloaded = slot.1;
                    outcome.de_finished = slot.2 == slot.3;
                }
                on_task(&outcome);
            }
        });
    }
    let mut results = Vec::new();
    for (de, _) in SCREENSHOT_URLS {
        if plan.already_complete.iter().any(|(d, _)| d == de) {
            results.push(((*de).to_string(), true));
        } else if let Some((_, n, ..)) = downloaded.iter().find(|(d, ..)| d == de) {
            results.push(((*de).to_string(), *n > 0));
        }
    }
    results
}

/// [`run_auto_download`] на боевом транспорте ядра (`net::UreqNet`).
pub fn auto_download_all_live<R>(dir: &str, on_task: R) -> Vec<(String, bool)>
where
    R: FnMut(&AutoDownloadOutcome),
{
    run_auto_download(dir, net::UreqNet::new, on_task)
}

/// Живая закачка этого процесса: `Some(столы, ждущие своей пачки)`, пока она
/// идёт. По ней заглушка решает, обещать ли «загружается».
static LIVE_RUN: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Состояние заглушки стола по живой закачке процесса.
pub fn live_placeholder_state(de: &str) -> PlaceholderState {
    let pending = LIVE_RUN
        .lock()
        .map(|run| run.as_ref().is_some_and(|p| p.iter().any(|d| d == de)))
        .unwrap_or(false);
    placeholder_state(de, pending)
}

/// Заглушка стола на диске под текущее состояние: файл пишется, если его нет
/// или подпись в нём устарела. `None` — каталог не пишется.
pub fn placeholder_on_disk(dir: &str, de: &str) -> Option<String> {
    placeholder_on_disk_for(dir, de, live_placeholder_state(de))
}

/// Заглушка стола на диске под ЗАДАННОЕ состояние. Для исполнителя, который
/// ведёт закачку сам и знает, ждёт ли стол своей пачки (виджетная вкладка
/// Python через мост): состояние он получает из [`placeholder_state`], а
/// имя файла, подпись и запись — отсюда же, что и у Rust-страницы.
pub fn placeholder_on_disk_for(dir: &str, de: &str, state: PlaceholderState) -> Option<String> {
    let path = format!("{dir}/{}", placeholder_name_for(de, state));
    let svg = generate_svg_for(de, state);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(svg.as_str()) {
        std::fs::create_dir_all(dir).ok()?;
        crate::utils::atomic_write(&path, &svg).ok()?;
    }
    Some(path)
}

/// Запустить автоскачивание в своём потоке и вести по нему состояние
/// заглушек. Столы, которым есть что качать, помечаются «ждущими» ДО возврата
/// — первая же карточка честно скажет «загружается». Стол снимается с
/// ожидания раньше, чем зовётся `on_task` с `de_finished`, так что окно,
/// перерисовавшись по нему, уже увидит итоговую подпись.
///
/// Одна закачка на процесс: пока идёт первая, вторая не стартует (`None`) —
/// иначе две страницы писали бы в одни файлы. Кончилась — можно снова.
pub fn start_auto_download<IO, F, R>(
    dir: String,
    make_io: F,
    mut on_task: R,
) -> Option<std::thread::JoinHandle<()>>
where
    IO: NetIo,
    F: Fn() -> IO + Send + Sync + 'static,
    R: FnMut(&AutoDownloadOutcome) + Send + 'static,
{
    let plan = auto_download_plan_on_disk(&dir);
    {
        let mut run = LIVE_RUN.lock().ok()?;
        if run.is_some() {
            return None;
        }
        let mut pending: Vec<String> = Vec::new();
        for task in &plan.tasks {
            if !pending.contains(&task.de) {
                pending.push(task.de.clone());
            }
        }
        *run = Some(pending);
    }
    let finish = || {
        if let Ok(mut run) = LIVE_RUN.lock() {
            *run = None;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("rexon-scr".into())
        .spawn(move || {
            run_plan(&dir, plan, make_io, |outcome| {
                if outcome.de_finished {
                    if let Ok(mut run) = LIVE_RUN.lock() {
                        if let Some(pending) = run.as_mut() {
                            pending.retain(|d| *d != outcome.de);
                        }
                    }
                }
                on_task(outcome);
            });
            finish();
        });
    match spawned {
        Ok(handle) => Some(handle),
        Err(_) => {
            finish();
            None
        }
    }
}

/// [`start_auto_download`] на боевом транспорте ядра. `false` — закачка уже
/// идёт либо поток не поднялся.
pub fn start_auto_download_live<R>(dir: &str, on_task: R) -> bool
where
    R: FnMut(&AutoDownloadOutcome) + Send + 'static,
{
    start_auto_download(dir.to_string(), net::UreqNet::new, on_task).is_some()
}

/// Python-истинность JSON-значения: null/отсутствие/`false`/0/""/пустой
/// список/пустой объект → `false`.
fn json_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Percent-encode как `urllib.parse.quote(s)` — safe = ALPHA/DIGIT/`_.-~/`.
/// Локальная копия по образцу соседних модулей ядра (`aur.rs`, `ocs.rs`):
/// общего url-хелпера в `utils` пока нет.
fn url_quote(s: &str) -> String {
    fn safe(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~' | b'/')
    }
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if safe(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// URL первого скриншота DE (`get_online_screenshot_url`): сначала прямой
/// `SCREENSHOT_URLS[0]`, иначе Wikimedia `Special:FilePath/{fname}?width=800`,
/// иначе пустая строка. Квирк сохранён: возвращает `""` (не None) при отсутствии.
pub fn online_url(de: &str) -> String {
    let urls = online_urls(de);
    if let Some(first) = urls.first() {
        return (*first).to_string();
    }
    match filename_for_de(de) {
        Some(fname) => {
            format!("https://commons.wikimedia.org/wiki/Special:FilePath/{fname}?width=800")
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod live_download_tests {
    use super::*;

    /// Подставная сеть: снимки xfce отдаются PNG-картинкой, всё остальное —
    /// «сети нет» (обрыв соединения на каждом запросе, API Commons тоже).
    #[derive(Default)]
    struct FakeNet {
        body: Vec<u8>,
        ctype: String,
    }

    fn offline() -> NetError {
        NetError::Transport { detail: "сети нет".into() }
    }

    impl NetIo for FakeNet {
        fn get(&mut self, _: &str, _: &str, _: u64) -> Result<Vec<u8>, NetError> {
            Err(offline())
        }
        fn open(&mut self, url: &str, _: &str, _: u64) -> Result<i64, NetError> {
            // Третий адрес xfce — `Xfce_4.18_…` с Commons, с большой буквы.
            if !url.to_ascii_lowercase().contains("xfce") {
                return Err(offline());
            }
            self.body = vec![0x89; 2048];
            self.ctype = if url.ends_with(".png") { "image/png" } else { "image/jpeg" }.into();
            Ok(self.body.len() as i64)
        }
        fn read(&mut self, _: usize) -> Result<Vec<u8>, NetError> {
            Ok(std::mem::take(&mut self.body))
        }
        fn close(&mut self) {}
        fn makedirs(&mut self, _: &str) {}
        fn create(&mut self, _: &str) -> Result<(), NetError> {
            Ok(())
        }
        fn write(&mut self, _: &[u8]) -> Result<(), NetError> {
            Ok(())
        }
        fn close_file(&mut self) {}
        fn exists(&mut self, _: &str) -> bool {
            false
        }
        fn remove(&mut self, _: &str) {}
        fn progress(&mut self, _: i64, _: i64) {}
        fn log(&mut self, _: LogLevel, _: &str) {}
        fn sleep(&mut self, _: u64) {}
        fn content_type(&mut self) -> String {
            self.ctype.clone()
        }
    }

    fn scratch(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!("rexon_scr_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn empty_cache_fills_with_python_names_and_offline_is_quiet() {
        let dir = scratch("fill");
        let mut outcomes = Vec::new();
        let results = run_auto_download(&dir, FakeNet::default, |o| outcomes.push(o.clone()));

        // Имена — те же, что пишет Python: тег `{de}`/`{de}_{idx}` плюс
        // расширение по Content-Type. Временных `.part` не остаётся.
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, ["xfce.png", "xfce_1.png", "xfce_2.png"]);
        assert_eq!(screenshot_paths_on_disk(&dir, "xfce").len(), 3);

        // Итог — словарь `results` оригинала: у xfce есть снимки, у
        // остальных нет, и никакой паники без сети.
        assert!(results.contains(&("xfce".to_string(), true)));
        assert!(results.contains(&("kde".to_string(), false)));
        assert_eq!(results.len(), SCREENSHOT_URLS.len());

        // Каждый стол сообщает об окончании своей пачки ровно один раз.
        for (de, urls) in SCREENSHOT_URLS {
            let mine: Vec<_> = outcomes.iter().filter(|o| o.de == *de).collect();
            assert_eq!(mine.len(), urls.len(), "{de}");
            assert_eq!(mine.iter().filter(|o| o.de_finished).count(), 1, "{de}");
        }
        let xfce_last = outcomes.iter().find(|o| o.de == "xfce" && o.de_finished).unwrap();
        assert_eq!(xfce_last.de_downloaded, 3);
        let kde_last = outcomes.iter().find(|o| o.de == "kde" && o.de_finished).unwrap();
        assert_eq!(kde_last.de_downloaded, 0);

        // Тёплый кэш: xfce больше не качается.
        let plan = auto_download_plan_on_disk(&dir);
        assert!(plan.tasks.iter().all(|t| t.de != "xfce"));
        assert!(plan.already_complete.contains(&("xfce".to_string(), 3)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn placeholder_caption_follows_download_state() {
        // Подпись «загружается» — байты оригинала, остальные отличаются
        // только нижней строкой.
        let loading = generate_svg_for("kde", PlaceholderState::Loading);
        assert_eq!(loading, generate_svg("kde"));
        let offline = generate_svg_for("kde", PlaceholderState::NoInternet);
        assert_eq!(offline, loading.replace(CAPTION_LOADING, CAPTION_NO_INTERNET));
        assert!(placeholder_state("нет-такого", true) == PlaceholderState::NoScreenshot);

        let dir = scratch("live");
        // Транспорт не отдаёт ни байта, пока тест не отпустит: так видно
        // состояние «идёт закачка».
        static GO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let make_io = || {
            while !GO.load(std::sync::atomic::Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            FakeNet::default()
        };
        let finished = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = finished.clone();
        let handle = start_auto_download(dir.clone(), make_io, move |o| {
            if o.de_finished {
                // Стол уже снят с ожидания, когда окну приходит весть.
                seen.lock().unwrap().push((o.de.clone(), live_placeholder_state(&o.de)));
            }
        })
        .expect("первая закачка стартует");

        assert_eq!(live_placeholder_state("kde"), PlaceholderState::Loading);
        let path = placeholder_on_disk(&dir, "kde").unwrap();
        assert!(path.ends_with("/kde_placeholder.svg"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(CAPTION_LOADING));
        assert!(
            start_auto_download(dir.clone(), FakeNet::default, |_| {}).is_none(),
            "вторая закачка во время первой не стартует"
        );

        GO.store(true, std::sync::atomic::Ordering::Release);
        handle.join().unwrap();

        assert_eq!(live_placeholder_state("kde"), PlaceholderState::NoInternet);
        let path = placeholder_on_disk(&dir, "kde").unwrap();
        assert!(path.ends_with("/kde_placeholder_offline.svg"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(CAPTION_NO_INTERNET));
        assert!(finished
            .lock()
            .unwrap()
            .contains(&("kde".to_string(), PlaceholderState::NoInternet)));
        assert_eq!(screenshot_paths_on_disk(&dir, "xfce").len(), 3);

        // Заданное состояние (закачку ведёт вызывающий — виджетная вкладка
        // через мост): стол без источника получает свою подпись и свой файл.
        let state = placeholder_state("нет-такого", false);
        let path = placeholder_on_disk_for(&dir, "нет-такого", state).unwrap();
        assert!(path.ends_with("/нет-такого_placeholder_none.svg"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(CAPTION_NO_SCREENSHOT));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundled_screenshots_come_first_then_downloaded() {
        let root = scratch("bundled");
        let cache = format!("{root}/cache");
        let missing = format!("{root}/missing");
        let bundled = format!("{root}/bundled");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&bundled).unwrap();
        let big = vec![0u8; 200];
        for name in ["river.png", "river_1.png", "river_2.jpg"] {
            std::fs::write(format!("{bundled}/{name}"), &big).unwrap();
        }
        std::fs::write(format!("{cache}/river_1.webp"), &big).unwrap();
        // Пустышка не в счёт — тот же фильтр размера, что у скачанных.
        std::fs::write(format!("{bundled}/river_3.png"), b"x").unwrap();
        let dirs = vec![missing, bundled.clone()];
        assert_eq!(
            carousel_paths_on_disk(&cache, &dirs, "river"),
            vec![
                format!("{bundled}/river.png"),
                format!("{bundled}/river_1.png"),
                format!("{bundled}/river_2.jpg"),
                format!("{cache}/river_1.webp"),
            ]
        );
        // Стол без поставляемых — ровно скачанные.
        assert!(carousel_paths_on_disk(&cache, &dirs, "kde").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_dirs_follow_product_data_layouts() {
        let dirs = bundled_screenshot_dirs("/src/rexon", "/home/u");
        assert_eq!(dirs[0], "/src/rexon/data/assets/screenshots");
        assert!(dirs.contains(&"/usr/lib/rexon/rexon/assets/screenshots".to_string()));
    }

    /// У River снимков нет ни у проекта, ни на Commons — дерево обязано везти
    /// свои (иначе карточка снова покажет «снимка пока нет»), а Wayfire —
    /// иметь несколько адресов, как остальные столы.
    #[test]
    fn river_ships_own_screenshots_and_wayfire_has_several() {
        let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/assets/screenshots");
        let river = screenshot_paths_on_disk(data, "river");
        assert!(river.len() >= 3, "поставляемых снимков River: {river:?}");
        for path in &river {
            let size = std::fs::metadata(path).unwrap().len();
            assert!(size <= 500 * 1024, "{path}: {size} байт — слишком крупно для пакета");
        }
        assert!(online_urls("wayfire").len() >= 3);
    }
}
