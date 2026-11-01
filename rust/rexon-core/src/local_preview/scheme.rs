//! Цветовые схемы KDE и стили Plasma: цвета схемы так, как их видит сама
//! KDE, светлая она или тёмная, и плитка схемы — окно, нарисованное её
//! цветами (вкладка «Оформление», разделы «Цветовые схемы» и «Стили
//! Plasma»).
//!
//! Поведение — как у Plasma 6.8 (замерено на ней самой):
//!   * цвета схемы — фон и текст окна, поля, выделения и кнопки; чего в файле
//!     нет — цвет KDE по умолчанию (Breeze), а не соседний цвет схемы;
//!   * заголовок окна: есть группа `[Colors:Header]` — её фон и текст; нет —
//!     `[WM] activeBackground` / `activeForeground`; нет и их — цвета
//!     выделения;
//!   * светлая или тёмная: яркость фона окна (`qGray` Qt) меньше 192 — тёмная;
//!   * стиль Plasma без своего файла `colors` берёт цвета из текущей схемы —
//!     Plasma подписывает его «Поддерживает цветовые схемы» и не относит ни к
//!     светлым, ни к тёмным;
//!   * плитка схемы — окно её цветами: заголовок, строка текста окна и
//!     кнопка, ниже поле со строками — обычной, выделенной и недоступной.
//!     Надписи — полосками: картинка одна на все языки.
//!
//! Вне Plasma те же файлы `.colors` Rexon кладёт слоем (qt6ct, `kdeglobals`),
//! поэтому и деление, и плитка одни во всех окружениях.
//!
//! Разбор сверен с KDE (программа на `KColorScheme`, KF6 6.31) на настоящих
//! схемах и краевых случаях — `tests/scheme_look_parity.rs`.

use std::path::Path;

use super::{DrawOp, Rgb};
use crate::appearance_page::Chip;
use crate::i18n::{tr, trc};

/// Цвета схемы, из которых KDE рисует её плитку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemeLook {
    pub window: Rgb,
    pub window_text: Rgb,
    pub view: Rgb,
    pub view_text: Rgb,
    pub selection: Rgb,
    pub selection_text: Rgb,
    pub button: Rgb,
    pub button_text: Rgb,
    /// Заголовок активного окна.
    pub title: Rgb,
    pub title_text: Rgb,
}

/// Цвета KDE по умолчанию (KF6 6.31, Breeze) — для ключей, которых в файле нет.
const DEFAULT_TEXT: Rgb = (35, 38, 41);
const DEFAULT_WINDOW: Rgb = (239, 240, 241);
const DEFAULT_VIEW: Rgb = (255, 255, 255);
const DEFAULT_SELECTION: Rgb = (61, 174, 233);
const DEFAULT_SELECTION_TEXT: Rgb = (255, 255, 255);
const DEFAULT_BUTTON: Rgb = (252, 252, 252);
const DEFAULT_HEADER: Rgb = (222, 224, 226);

/// Граница KDE: яркость фона окна (`qGray`) ниже неё — схема тёмная.
pub const DARK_GRAY_BELOW: u8 = 192;

/// Яркость цвета так, как её считает Qt (`qGray`): (11·r + 16·g + 5·b) / 32.
pub fn q_gray(c: Rgb) -> u8 {
    ((c.0 as u32 * 11 + c.1 as u32 * 16 + c.2 as u32 * 5) / 32) as u8
}

/// Значение ключа в группе — ПОСЛЕДНЕЕ найденное, как читает KConfig.
fn last_value<'t>(text: &'t str, group: &str, key: &str) -> Option<&'t str> {
    let mut inside = false;
    let mut found = None;
    for line in text.lines() {
        let s = line.trim();
        if let Some(name) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inside = name == group;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                found = Some(v.trim());
            }
        }
    }
    found
}

/// Есть ли в файле группа `[group]` (тогда заголовок берётся из неё).
fn has_group(text: &str, group: &str) -> bool {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix('[').and_then(|r| r.strip_suffix(']')))
        .any(|name| name == group)
}

/// Цвет так, как его читает KConfig: `r,g,b` или `r,g,b,a` (каждая часть
/// 0..=255, пробелы допустимы), `#rgb`, `#rrggbb`, `white`, `black`. Другое
/// (две части, часть вне 0..=255) — нет цвета, KDE берёт цвет по умолчанию.
/// Прочие имена цветов SVG здесь не знаем — тоже по умолчанию.
fn kconfig_color(value: &str) -> Option<Rgb> {
    let v = value.trim();
    if v.starts_with('#') || v.chars().all(|c| c.is_ascii_alphabetic()) {
        return super::css_color(v);
    }
    let parts: Vec<&str> = v.split(',').collect();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let byte = |s: &str| super::color_byte(s);
    if parts.len() == 4 {
        byte(parts[3])?;
    }
    Some((byte(parts[0])?, byte(parts[1])?, byte(parts[2])?))
}

/// Цвета схемы из текста её файла (`.colors` либо `colors` стиля Plasma).
pub fn scheme_look(text: &str) -> SchemeLook {
    let get = |group: &str, key: &str, default: Rgb| {
        last_value(text, group, key).and_then(kconfig_color).unwrap_or(default)
    };
    let selection = get("Colors:Selection", "BackgroundNormal", DEFAULT_SELECTION);
    let selection_text = get("Colors:Selection", "ForegroundNormal", DEFAULT_SELECTION_TEXT);
    let (title, title_text) = if has_group(text, "Colors:Header") {
        (
            get("Colors:Header", "BackgroundNormal", DEFAULT_HEADER),
            get("Colors:Header", "ForegroundNormal", DEFAULT_TEXT),
        )
    } else {
        (get("WM", "activeBackground", selection), get("WM", "activeForeground", selection_text))
    };
    SchemeLook {
        window: get("Colors:Window", "BackgroundNormal", DEFAULT_WINDOW),
        window_text: get("Colors:Window", "ForegroundNormal", DEFAULT_TEXT),
        view: get("Colors:View", "BackgroundNormal", DEFAULT_VIEW),
        view_text: get("Colors:View", "ForegroundNormal", DEFAULT_TEXT),
        selection,
        selection_text,
        button: get("Colors:Button", "BackgroundNormal", DEFAULT_BUTTON),
        button_text: get("Colors:Button", "ForegroundNormal", DEFAULT_TEXT),
        title,
        title_text,
    }
}

/// Тёмная ли схема — по фону окна, как KDE.
pub fn is_dark(look: &SchemeLook) -> bool {
    q_gray(look.window) < DARK_GRAY_BELOW
}

/// Светлая, тёмная или (у стиля Plasma) берёт цвета текущей схемы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Light,
    Dark,
    /// Стиль Plasma без своего файла `colors`.
    FollowsScheme,
}

impl Tone {
    /// Слово для эталонов и журнала.
    pub fn key(self) -> &'static str {
        match self {
            Tone::Light => "light",
            Tone::Dark => "dark",
            Tone::FollowsScheme => "follows",
        }
    }
}

/// Тон по тексту файла цветов.
pub fn tone_of_text(text: &str) -> Tone {
    if is_dark(&scheme_look(text)) {
        Tone::Dark
    } else {
        Tone::Light
    }
}

/// Тон стиля Plasma по его каталогу: нет файла `colors` — берёт цвета
/// схемы; файл есть — по его фону окна (нечитаемый файл KDE читает как
/// пустой, то есть светлый по умолчанию).
pub fn plasma_style_tone(dir: &Path) -> Tone {
    let colors = dir.join("colors");
    if !colors.exists() {
        return Tone::FollowsScheme;
    }
    tone_of_text(&super::read_text(&colors).unwrap_or_default())
}

/// Текст файла схемы: путь плитки бывает и с окончанием `.colors`, и без.
fn scheme_text(install_path: &str) -> Option<String> {
    super::read_text(Path::new(install_path))
        .or_else(|| super::read_text(Path::new(&format!("{install_path}.colors"))))
}

/// Вид темы по номеру плитки (`local:<вид>:<имя>`).
fn item_kind(item_id: &str) -> &str {
    item_id.strip_prefix("local:").and_then(|r| r.split(':').next()).unwrap_or("")
}

/// Тон темы с диска; `None` — у вида темы тона нет (не схема и не стиль
/// Plasma) или файл схемы не прочитать.
pub fn item_tone(item_id: &str, install_path: &str) -> Option<Tone> {
    if install_path.is_empty() {
        return None;
    }
    match item_kind(item_id) {
        "colors" => scheme_text(install_path).map(|t| tone_of_text(&t)),
        "plasma" => Some(plasma_style_tone(Path::new(install_path))),
        _ => None,
    }
}

// ── фильтр «Все / Светлые / Тёмные» ──────────────────────────────────────

/// Фильтр тона: всё.
pub const TONE_ALL: &str = "all";
/// Фильтр тона: светлые.
pub const TONE_LIGHT: &str = "light";
/// Фильтр тона: тёмные.
pub const TONE_DARK: &str = "dark";
/// Фильтр тона: стили Plasma, берущие цвета схемы.
pub const TONE_FOLLOWS: &str = "follows";

/// Есть ли фильтр тона у раздела: «Цветовые схемы» и «Стили Plasma», сетка
/// установленного (у KDE фильтр тоже только там; в магазине тон не
/// известен до загрузки).
pub fn tone_bar_applies(category_key: &str, installed_view: bool) -> bool {
    installed_view && matches!(category_key, "colors" | "plasma")
}

/// Действующий фильтр: незнакомое значение — «Все»; «Поддерживает цветовые
/// схемы» — только у стилей Plasma.
pub fn tone_filter(category_key: &str, chosen: &str) -> &'static str {
    match chosen {
        TONE_LIGHT => TONE_LIGHT,
        TONE_DARK => TONE_DARK,
        TONE_FOLLOWS if category_key == "plasma" => TONE_FOLLOWS,
        _ => TONE_ALL,
    }
}

/// Чипы фильтра над сеткой раздела (как выпадающий список в «Цветах» и
/// «Стиле Plasma» у Plasma). Пусто — у раздела фильтра нет.
pub fn tone_chips(category_key: &str) -> Vec<Chip> {
    let plasma = category_key == "plasma";
    if !plasma && category_key != "colors" {
        return Vec::new();
    }
    let (all_tip, light_tip, dark_tip) = if plasma {
        (
            tr("All Plasma styles"),
            tr("Plasma styles with a light window background"),
            tr("Plasma styles with a dark window background"),
        )
    } else {
        (
            tr("All color schemes"),
            tr("Color schemes with a light window background"),
            tr("Color schemes with a dark window background"),
        )
    };
    let mut chips = vec![
        Chip { key: TONE_ALL, label: trc("tone filter", "All"), tooltip: all_tip },
        Chip { key: TONE_LIGHT, label: trc("tone filter", "Light"), tooltip: light_tip },
        Chip { key: TONE_DARK, label: trc("tone filter", "Dark"), tooltip: dark_tip },
    ];
    if plasma {
        chips.push(Chip {
            key: TONE_FOLLOWS,
            label: tr("Color scheme compatible"),
            tooltip: tr("Plasma styles without their own colors: they take the colors of the current color scheme"),
        });
    }
    chips
}

/// Остаётся ли тема с диска в сетке при фильтре. Тему, тон которой не
/// узнать, фильтр не прячет только при «Все».
pub fn tone_passes(item_id: &str, install_path: &str, filter: &str) -> bool {
    let want = match filter {
        TONE_LIGHT => Tone::Light,
        TONE_DARK => Tone::Dark,
        TONE_FOLLOWS => Tone::FollowsScheme,
        _ => return true,
    };
    item_tone(item_id, install_path) == Some(want)
}

/// Подпись «Поддерживает цветовые схемы» под стилем Plasma (как подпись
/// плитки в «Стиле Plasma»): у стиля без своих цветов, кроме случая, когда
/// и так отобраны только такие.
pub fn follows_scheme_badge(item_id: &str, install_path: &str, filter: &str) -> Option<String> {
    (item_kind(item_id) == "plasma"
        && filter != TONE_FOLLOWS
        && item_tone(item_id, install_path) == Some(Tone::FollowsScheme))
    .then(|| tr("Follows color scheme"))
}

// ── плитка схемы ─────────────────────────────────────────────────────────

/// Плитка схемы — окно её цветами во весь кадр, как плитка в «Цветах» Plasma:
/// заголовок с надписью посередине; строка текста окна и кнопка; поле со
/// строками — обычной, выделенной и недоступной.
pub fn scheme_window_sample(l: &SchemeLook) -> Vec<DrawOp> {
    let fill = |x: f64, y: f64, w: f64, h: f64, c: Rgb| DrawOp::Fill {
        rect: (x, y, w, h),
        color: (c.0, c.1, c.2, 255),
    };
    let (w, h) = (super::PREVIEW_WIDTH as f64, super::PREVIEW_HEIGHT as f64);
    // Кайма поля у KDE — цвет текста с прозрачностью 0,3 поверх поля.
    let frame_edge = super::mix(l.view, l.view_text, 0.3);
    let button_edge = super::mix(l.button, l.button_text, 0.3);
    // Недоступный текст KDE выводит эффектами схемы; здесь — текст,
    // наполовину ушедший в фон поля.
    let disabled = super::mix(l.view, l.view_text, 0.45);
    vec![
        fill(0.0, 0.0, w, h, l.window),
        // Заголовок.
        fill(0.0, 0.0, w, 26.0, l.title),
        fill(w / 2.0 - 45.0, 10.0, 90.0, 6.0, l.title_text),
        // «Текст окна» и кнопка.
        fill(12.0, 42.0, 110.0, 7.0, l.window_text),
        fill(204.0, 33.0, 84.0, 25.0, button_edge),
        fill(205.0, 34.0, 82.0, 23.0, l.button),
        fill(223.0, 42.0, 46.0, 6.0, l.button_text),
        // Поле со строками.
        fill(10.0, 66.0, w - 20.0, 96.0, frame_edge),
        fill(11.0, 67.0, w - 22.0, 94.0, l.view),
        fill(20.0, 78.0, 128.0, 7.0, l.view_text),
        fill(11.0, 97.0, w - 22.0, 30.0, l.selection),
        fill(20.0, 109.0, 150.0, 7.0, l.selection_text),
        fill(20.0, 140.0, 104.0, 7.0, disabled),
    ]
}

/// Образец плитки схемы: окно её цветами. `None` — файла нет или в нём нет
/// ни одной группы цветов (рисовать было бы одни цвета по умолчанию).
pub fn colors_sample(input: &super::SampleInput) -> Option<Vec<DrawOp>> {
    let text = scheme_text(input.install_path)?;
    if !text.lines().any(|l| l.trim().starts_with("[Colors:") || l.trim() == "[WM]") {
        return None;
    }
    Some(scheme_window_sample(&scheme_look(&text)))
}
