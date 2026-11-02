//! Порт чистой части `rexon/ui_base.py` — контракт оформления UI.
//!
//! В Python модуль наполовину состоит из Qt-кода (палитра приложения,
//! QGraphicsDropShadowEffect, регистрация шрифтов, guard над QMessageBox).
//! Сюда переехало всё, что от Qt НЕ зависит: таблицы цветов и скинов,
//! сборка QSS-фрагментов, детект и экранирование rich-text, SVG-галочка
//! индикаторов. Наблюдения среды (тёмная ли активная тема) приходят
//! параметром, запись файла галочки остаётся исполнителю.
//!
//! Мотивация single-source: скин `steam_dark` — стандарт оформления ВСЕХ
//! приложений проекта (правило #17), и его палитра не должна расходиться
//! между Rexon и остальными.

use std::collections::BTreeMap;

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Цвета текста
// ============================================================================

/// Основной текст тёмной темы. Только белый: иерархия задаётся весом и
/// размером шрифта, не цветом (серые/бежевые оттенки сливались с фоном).
pub const DARK_PRIMARY: &str = "#ffffff";
/// Вторичный текст тёмной темы.
pub const DARK_SECONDARY: &str = "#ffffff";
/// Приглушённый текст тёмной темы.
pub const DARK_MUTED: &str = "#ffffff";
/// Основной текст светлой темы.
pub const LIGHT_PRIMARY: &str = "#1f2326";
/// Вторичный текст светлой темы.
pub const LIGHT_SECONDARY: &str = "#3d4147";
/// Приглушённый текст светлой темы.
pub const LIGHT_MUTED: &str = "#5d6166";

/// Цвет основного текста; `dark` — тёмная ли активная тема (наблюдение
/// исполнителя: в Python это яркость `palette(Window)`).
pub fn primary_text_color(dark: bool) -> &'static str {
    if dark { DARK_PRIMARY } else { LIGHT_PRIMARY }
}

/// Цвет вторичного текста.
pub fn secondary_text_color(dark: bool) -> &'static str {
    if dark { DARK_SECONDARY } else { LIGHT_SECONDARY }
}

/// Цвет приглушённого текста.
pub fn muted_text_color(dark: bool) -> &'static str {
    if dark { DARK_MUTED } else { LIGHT_MUTED }
}

/// Насыщенность вторичного и приглушённого текста по шкале CSS/Qt6
/// (400 — обычный, 600 — полужирный). Тонкое начертание нечитаемо на
/// тёмном фоне даже белым. Одно число на обе обёртки: виджеты получают его
/// строкой QSS (`font-weight: 600`), QML-вкладки — свойством `font.weight`
/// из палитры страницы; второе число в разметке разводило бы вкладки молча.
pub const SECONDARY_TEXT_WEIGHT: i32 = 600;

/// Общая сборка QSS-фрагмента текста: база + необязательные свойства.
fn text_qss(base: &str, extra: &str) -> String {
    if extra.is_empty() {
        base.to_string()
    } else {
        format!("{base} {extra}")
    }
}

/// QSS вторичного текста. Цвет — через `palette(window-text)`, а не
/// запечённый hex: иначе при ЖИВОЙ смене скина цвет застревал в прежнем
/// (тёмный текст на тёмном фоне = невидим).
pub fn secondary_text_qss(extra: &str) -> String {
    text_qss(
        &format!("color: palette(window-text); font-weight: {SECONDARY_TEXT_WEIGHT};"),
        extra,
    )
}

/// QSS приглушённого текста (то же основание — см. [`secondary_text_qss`]).
pub fn muted_text_qss(extra: &str) -> String {
    text_qss(
        &format!("color: palette(window-text); font-weight: {SECONDARY_TEXT_WEIGHT};"),
        extra,
    )
}

/// QSS основного текста (заголовки страниц и секций).
pub fn primary_text_qss(extra: &str) -> String {
    text_qss("color: palette(window-text);", extra)
}

/// Нейтральный полупрозрачный серый — hover, рамки, разделители. Средне-серый
/// с альфой одинаково читается на тёмной и светлой теме.
///
/// Формат альфы — как `%g` в Python: без хвостовых нулей, целое без точки.
pub fn neutral_rgba(alpha: f64) -> String {
    format!("rgba(127,127,127,{})", format_g(alpha))
}

/// CSS-цвет из QSS (`rgba(r,g,b,a)` либо `rgb(r,g,b)`) строкой `#AARRGGBB`.
///
/// ЗАЧЕМ. Краски продукта набраны так, как их понимает QSS виджетов, — и в
/// этом же виде их до сих пор отдавали QML-страницам. QML такой записи не
/// знает вовсе: `color: "rgba(59,130,246,0.85)"` он молча читает как
/// НЕДОПУСТИМЫЙ цвет и рисует ЧЁРНЫМ. Молча — потому и жило долго: страница
/// собирается, ошибки нет, а бейджи, чипы и звёздочки на ней чёрные
/// (поймано на вкладке «Steam» 01.09.2026 попиксельной сверкой с
/// оригиналом: бейдж карточки (0,0,0) против (57,119,219)).
///
/// Чинить это в каждом мосте отдельно нельзя (правило #12): цвет один и
/// тот же, а переводчик обязан быть один. Всё, чего перевести нельзя
/// (`#rrggbb`, `white`, `transparent`), возвращается как есть — такие
/// записи QML понимает сам.
pub fn css_color_to_qml(css: &str) -> String {
    match parse_css_rgba(css) {
        Some((r, g, b, a)) => format!("#{a:02x}{r:02x}{g:02x}{b:02x}"),
        None => css.trim().to_string(),
    }
}

/// Разобрать запись QSS `rgba(r,g,b,доля)` либо `rgb(r,g,b)` в готовые
/// восьмибитные каналы. `None` — запись не та (именованный цвет, `#rrggbb`,
/// мусор).
///
/// ⚠️ ДОЛЯ ПРОЗРАЧНОСТИ УСЕКАЕТСЯ, А НЕ ОКРУГЛЯЕТСЯ. Это не вкусовщина: так
/// читает долю сам разбор таблиц стилей Qt, а виджетная часть — эталон.
/// Опыт 04.09 (одна накладка на фоне `#1b2838`, доля против целой альфы
/// «вниз» и «вверх»): 0.85 даёт (220,222,225) — как альфа 216, а не 217;
/// 0.45 → (129,136,145) — как 114, не 115; 0.14 → (58,70,83) — как 35, не
/// 36; 0.25 → (83,93,105) — как 63, не 64; 0.03 → (33,46,61) — как 7, не 8;
/// 0.12 → (54,65,79) — как 30, не 31. Шесть долей из семи различают способ,
/// и все шесть за усечение.
///
/// Пока здесь стояло округление, КАЖДАЯ такая краска приезжала в QML на
/// ступень гуще виджетной, то есть на тёмном фоне — на ступень светлее.
/// Порог стенда (24 уровня) этого не показывал, и класс жил долго: дважды
/// его ловили поимённо (подложка полосы 2916fcc, выбранный пункт рейла
/// 5ac5d42), каждый раз убирая долю из одной разметки, — а корень был
/// общий и лежал в переводчике.
pub fn parse_css_rgba(css: &str) -> Option<(u8, u8, u8, u8)> {
    let text = css.trim();
    let body = text
        .strip_prefix("rgba(")
        .or_else(|| text.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = body.split(',').map(str::trim).collect();
    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }
    let mut rgb = [0u8; 3];
    for (i, part) in parts[..3].iter().enumerate() {
        match part.parse::<f64>() {
            // Дробные доли в QSS законны у альфы, но не у составляющих;
            // округляем так же, как это сделал бы Qt.
            Ok(v) if (0.0..=255.0).contains(&v) => rgb[i] = v.round() as u8,
            _ => return None,
        }
    }
    let alpha = match parts.get(3) {
        Some(a) => match a.parse::<f64>() {
            Ok(v) if (0.0..=1.0).contains(&v) => (v * 255.0) as u8,
            _ => return None,
        },
        None => 255,
    };
    Some((rgb[0], rgb[1], rgb[2], alpha))
}

/// Питоновский `%g` для значений альфы (0..1): до 6 значащих цифр,
/// хвостовые нули и точка убираются.
fn format_g(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let mut s = format!("{v:.6}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}

// ============================================================================
// Rich-text: детект и экранирование
// ============================================================================

/// Есть ли в тексте HTML-тег — значит автор рассчитывал на rich-text.
///
/// Порт регулярного выражения `<\s*/?\s*[a-zA-Z][\w-]*(\s[^<>]*)?/?>`
/// без движка регулярок: сравнение посимвольное. В отличие от Qt-эвристики
/// `mightBeRichText()` НЕ обрывается на первом переводе строки.
pub fn looks_like_rich_text(text: &str) -> bool {
    let b: Vec<char> = text.chars().collect();
    let n = b.len();
    let mut i = 0;
    while i < n {
        if b[i] != '<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && b[j].is_whitespace() {
            j += 1;
        }
        if j < n && b[j] == '/' {
            j += 1;
        }
        while j < n && b[j].is_whitespace() {
            j += 1;
        }
        // имя тега: первая буква ASCII, дальше [\w-]
        if j < n && b[j].is_ascii_alphabetic() {
            j += 1;
            while j < n && (b[j].is_alphanumeric() || b[j] == '_' || b[j] == '-') {
                j += 1;
            }
            // необязательные атрибуты: пробел, затем что угодно кроме < >
            let mut k = j;
            if k < n && b[k].is_whitespace() {
                k += 1;
                while k < n && b[k] != '<' && b[k] != '>' {
                    k += 1;
                }
                if k < n && b[k] == '>' {
                    j = k;
                }
            }
            if j < n && b[j] == '/' {
                j += 1;
            }
            if j < n && b[j] == '>' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Моноширинный блок rich-text диалога (пути, перечни пакетов, README),
/// который ПЕРЕНОСИТСЯ по ширине окна.
///
/// Голый `<pre>` у Qt не переносится вовсе (`white-space: pre`): строка
/// длиннее окна уходит за правый край, и её хвост обрезается рамкой. Так
/// окно удаления прятало конец пути, который удалится (снимок 30.09.2026,
/// «…/home-en-appearance/.config/rex»), а окно расширения — длинные строки
/// README. `pre-wrap` держит переводы строк и отступы как `<pre>`, но
/// лишнее переносит. Путь без пробелов переносится, только если у текста
/// стоит `Text.Wrap` (по словам, а слово длиннее строки — где придётся).
///
/// `inner` — уже экранированная разметка: функция её не трогает.
pub fn dialog_pre(inner: &str) -> String {
    format!("<pre style=\"white-space: pre-wrap\">{inner}</pre>")
}

/// Экранировать динамический текст для вставки в rich-text диалог.
///
/// Порт `html.escape(text, quote=False)` + замена перевода строки на `<br>`:
/// вывод pacman, пути и сообщения ошибок содержат `<`, `&`, `>`, которые
/// иначе съедаются разметкой, а переводы строк rich-text схлопывает в пробел.
/// Кавычки НЕ экранируются (quote=False у оригинала).
pub fn escape_dialog_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("<br>"),
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Скроллбар
// ============================================================================

/// QSS заметного скроллбара (правило единого механизма): handle виден в покое,
/// ярче на hover и нажатии. Основа цвета берётся от темы, поэтому одинаково
/// читается и на тёмной, и на светлой. `track` — фон дорожки.
pub fn scrollbar_qss(track: &str, dark: bool) -> String {
    let base = if dark { "255,255,255" } else { "0,0,0" };
    format!(
        "\nQScrollBar:vertical {{\n\
         \x20   background: {track};\n\
         \x20   width: 11px;\n\
         \x20   margin: 0;\n\
         \x20   border: none;\n\
         }}\n\
         QScrollBar::handle:vertical {{\n\
         \x20   background: rgba({base},0.28);\n\
         \x20   border-radius: 5px;\n\
         \x20   min-height: 28px;\n\
         }}\n\
         QScrollBar::handle:vertical:hover   {{ background: rgba({base},0.45); }}\n\
         QScrollBar::handle:vertical:pressed {{ background: rgba({base},0.60); }}\n\
         QScrollBar:horizontal {{\n\
         \x20   background: {track};\n\
         \x20   height: 11px;\n\
         \x20   margin: 0;\n\
         \x20   border: none;\n\
         }}\n\
         QScrollBar::handle:horizontal {{\n\
         \x20   background: rgba({base},0.28);\n\
         \x20   border-radius: 5px;\n\
         \x20   min-width: 28px;\n\
         }}\n\
         QScrollBar::handle:horizontal:hover   {{ background: rgba({base},0.45); }}\n\
         QScrollBar::handle:horizontal:pressed {{ background: rgba({base},0.60); }}\n\
         QScrollBar::add-line, QScrollBar::sub-line {{\n\
         \x20   width: 0; height: 0; background: none; border: none;\n\
         }}\n\
         QScrollBar::add-page, QScrollBar::sub-page {{ background: none; }}\n"
    )
}

/// SVG-галочка индикаторов чек/радио. Текст файла строит ядро, запись в
/// `~/.cache/rexon/ui` остаётся исполнителю (I/O-граница).
///
/// Нужна потому, что Qt QSS не рисует галочку в индикаторе, который скин
/// перекрасил заливкой — без неё выбранный пункт выглядит цветным пятном.
pub fn indicator_check_svg(color: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 14 14\">\
         <path d=\"M3 7.6l2.7 2.7L11.2 4.2\" fill=\"none\" stroke=\"{color}\" \
         stroke-width=\"2.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\
         </svg>"
    )
}

/// Имя файла галочки для данного цвета (`#` в начале отбрасывается).
pub fn indicator_check_filename(color: &str) -> String {
    format!("indicator-check-{}.svg", color.trim_start_matches('#'))
}

// ============================================================================
// Скины интерфейса
// ============================================================================

/// Токены Synthwave / Neon Space Retro.
///
/// ⚠️ Qt QSS не поддерживает box-shadow и text-shadow — настоящего свечения
/// в QSS нет. «Неон» здесь приближён яркой рамкой и скруглением; точечный
/// программный glow навешивает исполнитель.
pub const SW_TOKENS: &[(&str, &str)] = &[
        ("violet", "#1a0533"),
        ("navy", "#0d1b2a"),
        ("pink", "#ff2d95"),
        ("green", "#39ff14"),
        ("cyan", "#00f0ff"),
        ("card", "rgba(20,8,40,0.55)"),
        ("card_hi", "rgba(40,12,70,0.75)"),
        ("field", "rgba(13,27,42,0.75)"),
        ("text", "#eaf6ff"),
];

/// Токены вкладки «Оформление» по умолчанию (историческая Steam-палитра).
/// Гарантируют полный набор ключей: частичный набор скина не должен ломать
/// подстановку в локальный QSS вкладки.
pub const APPEARANCE_STEAM_COLORS: &[(&str, &str)] = &[
        ("bg_primary", "#1b2838"),
        ("bg_secondary", "#16202d"),
        ("bg_card", "#1e2d3d"),
        ("bg_card_hover", "#263d52"),
        ("accent", "#66c0f4"),
        ("accent_green", "#5ba32b"),
        ("accent_green_hover", "#6fc733"),
        ("text_primary", "#ffffff"),
        ("text_secondary", "#ffffff"),
        ("text_dim", "#ffffff"),
        ("rating_star", "#ffc300"),
        ("border", "#2a4055"),
        ("author_self", "#ff5252"),
        ("scrollbar", "255,255,255"),
];

/// Скин по умолчанию — стандарт оформления приложений проекта.
pub const DEFAULT_SKIN: &str = "steam_dark";

/// Затемнение фона под тем, что перехватило внимание, RGBA — ОДНО на весь
/// продукт. Гасят фон две подсистемы: обучение (`onboarding`, накладка с
/// вырезом вокруг подсвеченного места) и модальные окна вкладок на QML.
/// Держать у каждой свой оттенок значило показывать покупателю два разных
/// «выключенных» фона в одном окне, поэтому число тут одно, а обе стороны
/// только берут его: `onboarding::DIM_COLOR` и палитра QML-вкладки
/// (ключ `modal_dim`).
pub const MODAL_DIM_RGBA: (u8, u8, u8, u8) = (0, 0, 0, 170);

/// То же затемнение строкой `#AARRGGBB` — в этом виде цвет понимает QML
/// (у QtWidgets на руках `QColor(r, g, b, a)`, у QML — строка).
pub fn modal_dim_argb_hex() -> String {
    let (r, g, b, a) = MODAL_DIM_RGBA;
    format!("#{a:02x}{r:02x}{g:02x}{b:02x}")
}


/// Шаблон QSS-оверлея Synthwave. Подстановки помечены `%ТОКЕН%` — вместо
/// форматирования: сам QSS насыщен фигурными скобками.
const SYNTHWAVE_TEMPLATE: &str = r#"
/* ── Synthwave / Neon Space Retro (application overlay) ── */
QWidget {
    color: %TEXT%;
}
QMainWindow, QDialog, QScrollArea > QWidget > QWidget {
    background-color: qlineargradient(x1:0, y1:0, x2:0.4, y2:1,
        stop:0 %VIOLET%, stop:1 %NAVY%);
}
QTabWidget::pane {
    border: 1px solid %CYAN%;
    border-radius: 8px;
    background: transparent;
}
QTabBar::tab {
    background: %CARD%;
    color: %TEXT%;
    border: 1px solid rgba(0,240,255,0.35);
    border-bottom: none;
    border-top-left-radius: 8px;
    border-top-right-radius: 8px;
    padding: 8px 16px;
    margin-right: 2px;
    font-weight: bold;
}
QTabBar::tab:selected {
    background: %CARD_HI%;
    color: %CYAN%;
    border: 2px solid %CYAN%;
    border-bottom: none;
}
QTabBar::tab:hover { color: %PINK%; }
QPushButton {
    background: %CARD%;
    color: %CYAN%;
    border: 2px solid %CYAN%;
    border-radius: 10px;
    padding: 7px 16px;
    font-weight: bold;
}
QPushButton:hover {
    background: %CARD_HI%;
    border: 2px solid %PINK%;
    color: %PINK%;
}
QPushButton:pressed { background: rgba(255,45,149,0.25); }
QPushButton:disabled {
    border: 2px solid rgba(120,120,140,0.4);
    color: rgba(180,180,200,0.5);
}
QLineEdit, QComboBox, QSpinBox, QPlainTextEdit, QTextEdit {
    background: %FIELD%;
    color: %TEXT%;
    border: 1px solid %CYAN%;
    border-radius: 8px;
    padding: 5px 8px;
    selection-background-color: %PINK%;
}
QLineEdit:focus, QComboBox:focus { border: 2px solid %PINK%; }
QComboBox QAbstractItemView {
    background: %NAVY%;
    color: %TEXT%;
    border: 1px solid %CYAN%;
    selection-background-color: rgba(255,45,149,0.45);
}
QCheckBox, QRadioButton, QLabel { color: %TEXT%; background: transparent; }
/* Полная стилизация индикаторов: если задать только :checked, Qt рисует
   невыбранные как пустой бокс, а радио без border-radius выходит КВАДРАТНЫМ
   (баг 2026-06-22: зелёный квадрат вместо круга). Описываем обе стадии +
   форму. */
QCheckBox::indicator, QRadioButton::indicator {
    width: 14px;
    height: 14px;
    border: 2px solid %CYAN%;
    background: %FIELD%;
}
QRadioButton::indicator { border-radius: 9px; }   /* круг */
QCheckBox::indicator { border-radius: 4px; }      /* скруглённый квадрат */
QCheckBox::indicator:checked, QRadioButton::indicator:checked {
    background: %GREEN%;
    border: 2px solid %GREEN%;
    /* галочка внутри — иначе «просто зелёное пятно», не видно что
       применилось (общий хелпер indicator_check_url, правило #12) */
    image: url(%CHECK%);
}
QGroupBox {
    border: 1px solid %CYAN%;
    border-radius: 8px;
    margin-top: 8px;
}
QProgressBar {
    background: %FIELD%;
    border: 1px solid %CYAN%;
    border-radius: 6px;
    text-align: center;
    color: %TEXT%;
}
QProgressBar::chunk {
    background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
        stop:0 %PINK%, stop:1 %CYAN%);
    border-radius: 6px;
}
QScrollBar:vertical, QScrollBar:horizontal {
    background: transparent; width: 11px; height: 11px; margin: 0; border: none;
}
QScrollBar::handle:vertical, QScrollBar::handle:horizontal {
    background: rgba(0,240,255,0.35); border-radius: 5px;
    min-height: 28px; min-width: 28px;
}
QScrollBar::handle:vertical:hover, QScrollBar::handle:horizontal:hover {
    background: rgba(255,45,149,0.55);
}
QScrollBar::add-line, QScrollBar::sub-line {
    width: 0; height: 0; background: none; border: none;
}
QScrollBar::add-page, QScrollBar::sub-page { background: none; }
"#;

/// QSS-оверлей скина Synthwave. `check_url` — путь SVG-галочки индикаторов
/// (его файл пишет исполнитель, см. [`indicator_check_svg`]).
pub fn synthwave_qss(check_url: &str) -> String {
    let mut out = SYNTHWAVE_TEMPLATE.to_string();
    for (key, value) in SW_TOKENS {
        out = out.replace(&format!("%{}%", key.to_uppercase()), value);
    }
    out.replace("%CHECK%", check_url)
}


/// Описание скина: палитра ролей Qt, необязательный QSS-оверлей и токены
/// вкладки «Оформление».
pub struct Skin {
    /// Признак скина: его пишет настройка `ui_skin`, по нему сравнивает код.
    /// Не переводится и не меняется — иначе сохранённый выбор пропадёт.
    pub id: &'static str,
    /// Подпись в списке — `msgid` каталога; показывать через `tr`
    /// ([`available_skins`]).
    pub label: &'static str,
    /// Описание под списком — `msgid` каталога ([`skin_description`]).
    pub description: &'static str,
    /// `None` — сброс к палитре приложения (скин «Системный»).
    pub palette: Option<&'static [(&'static str, &'static str)]>,
    /// `None` — токены вкладки совпадают с [`APPEARANCE_STEAM_COLORS`].
    pub colors_override: Option<&'static [(&'static str, &'static str)]>,
    /// Есть ли у скина application-level QSS-оверлей.
    pub has_qss: bool,
    /// Файл бандл-шрифта заголовков (в `rexon/assets/fonts`), если скин
    /// рисует заголовки своим шрифтом. Регистрирует файл исполнитель.
    pub heading_font: Option<&'static str>,
    /// Программное свечение заголовка `(цвет, радиус размытия)`: Qt QSS не
    /// умеет text-shadow, свечение навешивает исполнитель эффектом.
    pub glow: Option<(&'static str, u32)>,
    /// Цвет штриха SVG-галочки индикаторов для QSS-оверлея. `None` — оверлея
    /// нет, галочку рисовать незачем (и файл в кеш писать не нужно).
    pub check_color: Option<&'static str>,
}

/// Все скины в порядке показа в комбобоксе настроек.
pub const SKINS: &[Skin] = &[
    Skin {
        id: "system",
        label: n_("System (follow the system theme)"),
        description: n_("Colors come from the system Plasma/Qt theme."),
        palette: None,
        colors_override: None,
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
    Skin {
        id: "steam_dark",
        // TRANSLATORS: skin name; “Steam” is the Steam client whose look it copies.
        label: n_("Dark (Steam)"),
        description: n_("Dark blue colors in the style of the Steam client. The default."),
        palette: Some(&[
            ("Window", "#1b2838"),
            ("WindowText", "#ffffff"),
            ("Base", "#16202d"),
            ("AlternateBase", "#1e2d3d"),
            ("Text", "#ffffff"),
            ("Button", "#2a4055"),
            ("ButtonText", "#ffffff"),
            ("Highlight", "#66c0f4"),
            ("HighlightedText", "#0b1722"),
            ("ToolTipBase", "#1e2d3d"),
            ("ToolTipText", "#ffffff"),
            ("PlaceholderText", "#9fb4c6"),
            ("Link", "#66c0f4"),
        ]),
        colors_override: None,
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
    Skin {
        id: "light",
        // TRANSLATORS: skin name.
        label: n_("Light"),
        description: n_("Light neutral colors (dark text on a white background)."),
        palette: Some(&[
            ("Window", "#f4f5f6"),
            ("WindowText", "#1f2326"),
            ("Base", "#ffffff"),
            ("AlternateBase", "#eceef0"),
            ("Text", "#1f2326"),
            ("Button", "#e3e6e8"),
            ("ButtonText", "#1f2326"),
            ("Highlight", "#3b82f6"),
            ("HighlightedText", "#ffffff"),
            ("ToolTipBase", "#ffffff"),
            ("ToolTipText", "#1f2326"),
            ("PlaceholderText", "#7a8086"),
            ("Link", "#2563eb"),
        ]),
        colors_override: Some(&[
            ("bg_primary", "#f4f5f6"),
            ("bg_secondary", "#eceef0"),
            ("bg_card", "#ffffff"),
            ("bg_card_hover", "#e3eaf2"),
            ("accent", "#2563eb"),
            ("accent_green", "#2e9e3f"),
            ("accent_green_hover", "#37b84a"),
            ("text_primary", "#1f2326"),
            ("text_secondary", "#3d4147"),
            ("text_dim", "#5d6166"),
            ("rating_star", "#e0a800"),
            ("border", "#cdd3da"),
            ("author_self", "#d22d2d"),
            ("scrollbar", "0,0,0"),
        ]),
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
    Skin {
        id: "synthwave",
        // Имя стиля — латиницей на всех языках, поэтому без `n_`.
        label: "Synthwave / Neon Space Retro",
        description: n_("Neon retro: a purple→blue gradient, pink/green/cyan accents, neon borders and rounded corners. The glow is approximated (Qt QSS doesn't support box-shadow)."),
        palette: Some(&[
            ("Window", "#1a0533"),
            ("WindowText", "#eaf6ff"),
            ("Base", "#0d1b2a"),
            ("AlternateBase", "#160a2e"),
            ("Text", "#eaf6ff"),
            ("Button", "#1f0a3a"),
            ("ButtonText", "#00f0ff"),
            ("Highlight", "#ff2d95"),
            ("HighlightedText", "#0d1b2a"),
            ("ToolTipBase", "#0d1b2a"),
            ("ToolTipText", "#00f0ff"),
            ("PlaceholderText", "#8aa6c0"),
            ("Link", "#00f0ff"),
        ]),
        colors_override: Some(&[
            ("bg_primary", "#1a0533"),
            ("bg_secondary", "#0d1b2a"),
            ("bg_card", "#1c0a38"),
            ("bg_card_hover", "#2a0f52"),
            ("accent", "#00f0ff"),
            ("accent_green", "#1f7a30"),
            ("accent_green_hover", "#39ff14"),
            ("text_primary", "#eaf6ff"),
            ("text_secondary", "#00f0ff"),
            ("text_dim", "#9fb4c6"),
            ("rating_star", "#ffd400"),
            ("border", "#00f0ff"),
            ("author_self", "#ff2d95"),
            ("scrollbar", "0,240,255"),
        ]),
        has_qss: true,
        // Tektur (OFL) — квадратный sci-fi С КИРИЛЛИЦЕЙ. Прежний Orbitron
        // кириллицы не имел: русские заголовки рисовал системный запасной
        // шрифт, блёкло и слипшимися буквами.
        heading_font: Some("Tektur-VariableFont_wdth_wght.ttf"),
        glow: Some(("#00f0ff", 18)),
        // Тёмный штрих: индикатор залит неон-зелёным.
        check_color: Some("#0d1b2a"),
    },
    // Схема самого waywallen. Вкладка Wallpaper Engine по умолчанию
    // красится скином Rexon (`we_window::vendor_theme`), а этот скин
    // возвращает ей родные цвета waywallen и его собственные настройки
    // «Тема» и «Акцент». Остальное окно получает те же краски Material:
    // тёмная схема QmlMaterial (тип палитры «нейтральный», как у
    // waywallen) от его акцента по умолчанию #6750A4 — снято с самого
    // плагина `Qcm.Material` (`MD.Token.color` при этих настройках).
    Skin {
        id: VENDOR_SCHEME_SKIN,
        // TRANSLATORS: skin name, a pair to “Dark (Steam)”; Material is the design style of the waywallen interface.
        label: n_("Dark (Material)"),
        description: n_("Material colors of the waywallen interface. The Wallpaper Engine tab follows waywallen's own theme and accent settings."),
        palette: Some(&[
            ("Window", "#201f21"),
            ("WindowText", "#e6e1e3"),
            ("Base", "#1c1b1d"),
            ("AlternateBase", "#2b292b"),
            ("Text", "#e6e1e3"),
            ("Button", "#363436"),
            ("ButtonText", "#e6e1e3"),
            ("Highlight", "#cbc3d5"),
            ("HighlightedText", "#322e3c"),
            ("ToolTipBase", "#363436"),
            ("ToolTipText", "#e6e1e3"),
            ("PlaceholderText", "#c9c5c7"),
            ("Link", "#cbc3d5"),
        ]),
        colors_override: Some(&[
            ("bg_primary", "#201f21"),
            ("bg_secondary", "#1c1b1d"),
            ("bg_card", "#2b292b"),
            ("bg_card_hover", "#363436"),
            ("accent", "#cbc3d5"),
            ("text_primary", "#e6e1e3"),
            ("text_secondary", "#c9c5c7"),
            ("text_dim", "#c9c5c7"),
            ("border", "#484648"),
            ("author_self", "#ffb4ab"),
        ]),
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
];

/// Скин, при котором вкладка Wallpaper Engine показывает схему самого
/// waywallen, а не цвета Rexon (см. `we_window::vendor_theme`).
pub const VENDOR_SCHEME_SKIN: &str = "waywallen";

/// Скин по id; неизвестный id — как в оригинале, откат к скину по умолчанию.
pub fn skin(skin_id: &str) -> &'static Skin {
    SKINS
        .iter()
        .find(|s| s.id == skin_id)
        .or_else(|| SKINS.iter().find(|s| s.id == DEFAULT_SKIN))
        .expect("скин по умолчанию обязан быть в таблице")
}

/// Список `(id, подпись)` для комбобокса настроек. Подпись — на языке
/// интерфейса; выбор сохраняется по `id`, поэтому смена языка сохранённый
/// скин не теряет.
pub fn available_skins() -> Vec<(&'static str, String)> {
    SKINS.iter().map(|s| (s.id, tr(s.label))).collect()
}

/// Краткое описание скина на языке интерфейса.
pub fn skin_description(skin_id: &str) -> String {
    tr(skin(skin_id).description)
}

/// Известен ли скин.
pub fn is_valid_skin(skin_id: &str) -> bool {
    SKINS.iter().any(|s| s.id == skin_id)
}

/// Токены вкладки «Оформление» для скина: база плюс переопределения скина.
/// Полный набор ключей гарантирован.
pub fn skin_colors(skin_id: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = APPEARANCE_STEAM_COLORS
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    if let Some(over) = skin(skin_id).colors_override {
        for (k, v) in over {
            out.insert((*k).to_string(), (*v).to_string());
        }
    }
    out
}

/// Действующий скин по СОХРАНЁННОМУ значению настройки `ui_skin`.
///
/// Решение одно на весь продукт: незнакомое или отсутствующее имя (файл
/// настроек от будущей версии, правка руками, снятый скин) откатывается к
/// [`DEFAULT_SKIN`]. Раньше эта развилка жила отдельной копией в
/// `rexon/ui_base.py::_current_skin_id`, и вторая копия молчала бы при
/// расхождении — окно красилось бы одним скином, а вкладка другим.
pub fn active_skin_id_from(saved: Option<&str>) -> &'static str {
    match saved {
        Some(id) => skin(id).id,
        None => DEFAULT_SKIN,
    }
}

/// Действующий скин: читает `ui_skin` из настроек и применяет
/// [`active_skin_id_from`]. Каталог настроек НЕ создаётся — это чтение.
pub fn active_skin_id() -> &'static str {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    active_skin_id_from(settings.get("ui_skin").and_then(|v| v.as_str()))
}

/// Токены цвета ДЕЙСТВУЮЩЕГО скина — то, чем красятся страницы на QML.
///
/// ⚠️ Именно этим, а не `skin_colors(DEFAULT_SKIN)`: вкладки на виджетах
/// красятся выбранным скином (`apply_skin`), и запечённое имя стандартного
/// скина оставляло бы перенесённые вкладки тёмно-синими в окне, где
/// покупатель выбрал «Светлый» или «Synthwave».
pub fn active_skin_colors() -> BTreeMap<String, String> {
    skin_colors(active_skin_id())
}

/// Палитра ролей Qt для скина (`None` — сброс к палитре приложения).
pub fn skin_palette(skin_id: &str) -> Option<&'static [(&'static str, &'static str)]> {
    skin(skin_id).palette
}

/// Цвет РОЛИ палитры Qt у скина (`Base`, `Window`, `Button`…), если скин
/// палитру задаёт. `None` — системный скин: роли остаются системными, и
/// ядру их знать неоткуда.
pub fn skin_role_color(skin_id: &str, role: &str) -> Option<&'static str> {
    skin_palette(skin_id)?
        .iter()
        .find(|(name, _)| *name == role)
        .map(|(_, hex)| *hex)
}

/// Цвет ссылок в форматированном тексте (роли `Link` и `LinkVisited`
/// палитры приложения) для скина: его роль `Link`. `None` — системный скин,
/// ссылки остаются цвета системной темы.
///
/// ⚠️ Почему палитра приложения, а не `linkColor` у элемента. У `Text` из
/// QML `linkColor` действует только на `Text.StyledText`; в `Text.RichText`
/// (описания и комментарии тем, «О программе», окна расширений) ссылку
/// красит разборщик HTML самой Qt — ролью `Link` палитры ПРИЛОЖЕНИЯ, в
/// момент разбора текста. Не заданная роль — это `#0000ff`, на тёмных
/// скинах нечитаемый (дефект 01.11.2026, коммит 504655a9 его не лечил).
pub fn skin_link_rgb(skin_id: &str) -> Option<(u8, u8, u8)> {
    skin_role_color(skin_id, "Link").and_then(parse_hex)
}

/// То же для ДЕЙСТВУЮЩЕГО скина.
pub fn active_skin_link_rgb() -> Option<(u8, u8, u8)> {
    skin_link_rgb(active_skin_id())
}

// ============================================================================
// Поверхности скина: ГОТОВАЯ краска для QML
// ============================================================================
//
// ЗАЧЕМ ЦЕЛЫЙ РАЗДЕЛ. Поверхность — это заливка, которую у виджетной части
// кладёт НЕ разметка вкладки: поле ввода красит сама Qt по роли палитры
// (`Base`), карточку — полупрозрачная накладка из таблицы стилей. У QML ни
// ролей палитры, ни каскада таблиц стилей нет, и страницы выбирали похожий
// токен скина на глаз. Похожий — не тот же: на стандартном скине поле
// выходило `bg_card` (#1e2d3d, (30,45,61)) там, где Qt красит роль `Base`
// (#16202d, (22,32,45)), то есть на ступень светлее оригинала, и так на
// четырёх вкладках сразу. Лечится не переписыванием разметки по одной
// (правило #12), а тем, что краску поверхности целиком собирает ЯДРО и
// отдаёт готовой строкой, а разметка её только кладёт.

/// Заливка карточки поверх фона страницы — ТА ЖЕ запись, какой красит
/// карточку таблица стилей виджетов (`qt_steam::card_colors`,
/// `qt_compare::card_qss`, `qt_extensions::_card_qss`).
pub const CARD_FILL_DARK: &str = "rgba(255,255,255,0.04)";
pub const CARD_FILL_LIGHT: &str = "rgba(0,0,0,0.03)";
/// Обводка обычной кнопки и поля ввода: её рисует САМ стиль Qt, а не роль
/// палитры, — ровно так же, как рукоятку сплиттера
/// (`qt_desktops::split_handle_qss`). Замер 05.09 на живой Qt (стиль
/// breeze, скин steam_dark): кнопка «Быстрых пар» «Сравнения» залита
/// (42,64,85) ролью `Button` и обведена (85,102,119) — это ровно накладка
/// белого 0.20 поверх заливки (42+0,2*213 = 85, 64+0,2*191 = 102,
/// 85+0,2*170 = 119). Ни одна роль палитры такого цвета не даёт.
/// У ПОЛЯ ввода та же накладка ложится на фон страницы, а не на заливку
/// поля: рамка стоит по краю, куда заливка не заходит.
pub const CONTROL_OUTLINE_DARK: &str = "rgba(255,255,255,0.20)";
pub const CONTROL_OUTLINE_LIGHT: &str = "rgba(0,0,0,0.20)";

/// Положить полупрозрачную накладку QSS на непрозрачную краску и вернуть
/// ГОТОВЫЙ `#rrggbb` — ровно то, что покупатель увидит на экране.
///
/// Считает так же, как Qt: долю прозрачности разбор таблицы стилей УСЕКАЕТ
/// (см. [`parse_css_rgba`]), а смешивание растр округляет до ближайшего
/// уровня. Сверено с экраном: белая накладка 0.04 на `#1b2838` даёт
/// (36,48,64) и там, и там. Неразобранная накладка возвращается как есть.
pub fn blend_css_over(base: &str, overlay: &str) -> String {
    let (Some((br, bg, bb)), Some((r, g, b, a))) = (parse_hex(base), parse_css_rgba(overlay))
    else {
        return overlay.trim().to_string();
    };
    let k = f64::from(a) / 255.0;
    let mix = |dst: u8, src: u8| (f64::from(dst) + (f64::from(src) - f64::from(dst)) * k).round();
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(br, r) as u8,
        mix(bg, g) as u8,
        mix(bb, b) as u8
    )
}

/// Поверхности, которые ядро отдаёт QML готовой краской: ключ для разметки
/// и роль палитры Qt, по которой ту же поверхность красит Qt у виджетов.
/// Запасной токен нужен системному скину, у которого ролей ядро не видит.
pub const QML_SURFACES: &[(&str, &str, &str)] = &[
    // Поле ввода, выпадающий список и его список пунктов: `QLineEdit`,
    // `QComboBox`, `QAbstractItemView` — всё это Qt красит ролью `Base`.
    ("surface_field", "Base", "bg_secondary"),
    // Чередующаяся строка списка.
    ("surface_field_alt", "AlternateBase", "bg_card"),
    // Кнопка и НЕРЕДАКТИРУЕМЫЙ выпадающий список: и то и другое Qt красит
    // ролью `Button`, а не `Base`. Замер 04.09 на живой Qt при ролях
    // steam_dark: `QLineEdit` даёт (22,32,45) роли `Base`, а `QComboBox` —
    // (42,64,85) роли `Button`, и по всей своей высоте.
    ("surface_button", "Button", "border"),
    // Фон страницы.
    ("surface_page", "Window", "bg_primary"),
];

/// Готовые краски всех поверхностей скина.
///
/// Карточки тут нет среди ролей: её краску даёт не палитра, а накладка
/// поверх фона страницы, — поэтому она считается [`blend_css_over`] по тем
/// же литералам, что стоят в таблице стилей виджетной карточки.
pub fn skin_surface_colors(skin_id: &str) -> BTreeMap<String, String> {
    let tokens = skin_colors(skin_id);
    let token = |name: &str| tokens.get(name).cloned().unwrap_or_default();
    let mut out = BTreeMap::new();
    for (key, role, fallback) in QML_SURFACES {
        let value = match skin_role_color(skin_id, role) {
            Some(hex) => hex.to_string(),
            None => token(fallback),
        };
        out.insert((*key).to_string(), value);
    }
    let page = out
        .get("surface_page")
        .cloned()
        .unwrap_or_else(|| token("bg_primary"));
    let dark = parse_hex(&page)
        .map(|(r, g, b)| is_dark_rgb(r, g, b))
        .unwrap_or(true);
    let fill = if dark { CARD_FILL_DARK } else { CARD_FILL_LIGHT };
    out.insert("surface_card".to_string(), blend_css_over(&page, fill));
    // Обводка кнопки и поля: накладка поверх ИХ заливки, а не поверх фона
    // страницы, — стиль обводит каждый контрол по его собственной краске.
    let outline = if dark {
        CONTROL_OUTLINE_DARK
    } else {
        CONTROL_OUTLINE_LIGHT
    };
    let button = out
        .get("surface_button")
        .cloned()
        .unwrap_or_else(|| token("border"));
    out.insert(
        "surface_button_outline".to_string(),
        blend_css_over(&button, outline),
    );
    // ⚠️ Обводка ПОЛЯ считается поверх фона страницы, а не поверх заливки
    // поля: заливка под рамку не заходит. Замер 05.09 (поле поиска
    // «Расширений», скин steam_dark): фон страницы (27,40,56), заливка поля
    // (22,32,45), рамка (73,83,96) — это 0,20 белого поверх ФОНА
    // (27+0,2*228 = 73), а поверх заливки вышло бы (69,77,87).
    out.insert(
        "surface_field_outline".to_string(),
        blend_css_over(&page, outline),
    );
    out
}

/// Краска НЕАКТИВНОГО текста — ровно та, что скин пишет в группу `Disabled`
/// палитры Qt (см. [`palette_plan`]), записью `#aarrggbb`.
///
/// ⚠️ ЗАЧЕМ ОНА В QML. Виджетная часть выключенную кнопку НЕ гасит целиком:
/// заливка и рамка остаются прежними, а Qt приглушает одну надпись цветом
/// группы `Disabled`. Ровно то же говорят таблицы стилей вкладок там, где
/// они пишут это правило руками: `#PageBtn:disabled` у «Оформления» и
/// `QPushButton:disabled` у блока «Установить» «Расширений» — оба ставят
/// прозрачность 0.55, которую разбор таблицы стилей усекает до 140, то есть
/// до [`DISABLED_TEXT_ALPHA`]. Разметка QML вместо этого гасила ВСЮ кнопку
/// прозрачностью 0.45 и меняла цвет заливки: замер 05.09 на «Расширениях» —
/// 10 108 точек.
pub fn disabled_text_color(skin_id: &str) -> String {
    // Тот же источник, что у `palette_plan`: роль `WindowText` скина, а у
    // скина без своей палитры — белый форс тёмной системной темы.
    let hex = skin_role_color(skin_id, "WindowText").unwrap_or("#ffffff");
    let (r, g, b) = parse_hex(hex).unwrap_or((255, 255, 255));
    format!("#{DISABLED_TEXT_ALPHA:02x}{r:02x}{g:02x}{b:02x}")
}

/// Палитра, которую ядро отдаёт QML-странице: токены скина ПЛЮС готовые
/// краски поверхностей. Одна дверь на все страницы — иначе новая вкладка
/// снова выберет токен на глаз.
pub fn qml_palette(skin_id: &str) -> BTreeMap<String, String> {
    let mut out = skin_colors(skin_id);
    out.extend(skin_surface_colors(skin_id));
    // Не поверхность, а КРАСКА ТЕКСТА, поэтому кладётся рядом, а не в
    // таблицу поверхностей: у неё есть прозрачность, и `parse_hex` её не
    // разбирает по замыслу.
    out.insert("text_disabled".to_string(), disabled_text_color(skin_id));
    out
}

/// То же для ДЕЙСТВУЮЩЕГО скина.
pub fn active_qml_palette() -> BTreeMap<String, String> {
    qml_palette(active_skin_id())
}

/// QSS-оверлей скина; пустая строка, если оверлея нет.
pub fn skin_qss(skin_id: &str, check_url: &str) -> String {
    let s = skin(skin_id);
    if !s.has_qss {
        return String::new();
    }
    synthwave_qss(check_url)
}

/// Скругление плашки тоста, точек.
pub const TOAST_RADIUS: i32 = 8;
/// Толщина рамки плашки, точек.
pub const TOAST_BORDER_WIDTH: i32 = 1;
/// Поле слева и справа внутри плашки, точек.
pub const TOAST_PADDING_H: i32 = 14;
/// Поле сверху и снизу внутри плашки, точек.
pub const TOAST_PADDING_V: i32 = 10;

/// Краски всплывающего тоста записями QSS `rgba(...)`.
///
/// Имена полей — не токены скина, а готовые краски: плашка накладывается
/// поверх содержимого и обязана оставаться читаемой на ЛЮБОМ скине, поэтому
/// её тон задан прямо и зависит только от тёмности темы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastSkin {
    /// Заливка плашки (полупрозрачная).
    pub background: &'static str,
    /// Рамка плашки.
    pub border: &'static str,
    /// Надпись.
    pub text: &'static str,
}

/// Краски тоста для тёмной либо светлой темы.
///
/// ⚠️ ОДНА ДВЕРЬ НА ОБА ПУТИ. Отсюда их берёт и виджетная таблица стилей
/// ([`toast_qss`]), и QML-разметка (через карту `styles` вкладки). Пока
/// числа лежат здесь, у плашки не может завестись второй набор красок «на
/// глаз»: в QML тост рисовался токенами `bg_secondary`/`border`/
/// `text_primary` и получался НЕПРОЗРАЧНЫМ, другого скругления и в другом
/// углу, чем у виджетной версии.
pub fn toast_colors(dark: bool) -> ToastSkin {
    if dark {
        ToastSkin {
            background: "rgba(30,30,30,0.94)",
            border: "rgba(255,255,255,0.18)",
            text: "rgba(255,255,255,0.92)",
        }
    } else {
        ToastSkin {
            background: "rgba(250,250,250,0.96)",
            border: "rgba(0,0,0,0.18)",
            text: "rgba(0,0,0,0.88)",
        }
    }
}

/// QSS всплывающего тоста-накладки — полупрозрачная плашка с рамкой.
/// `dark` — тёмная ли активная тема (наблюдение исполнителя, как у
/// [`primary_text_color`]).
pub fn toast_qss(dark: bool) -> String {
    let c = toast_colors(dark);
    let (bg, border, fg) = (c.background, c.border, c.text);
    let (bw, radius) = (TOAST_BORDER_WIDTH, TOAST_RADIUS);
    let (pad_v, pad_h) = (TOAST_PADDING_V, TOAST_PADDING_H);
    format!(
        "QLabel {{ background-color: {bg}; color: {fg}; \
         border: {bw}px solid {border}; border-radius: {radius}px; \
         padding: {pad_v}px {pad_h}px; }}"
    )
}

/// `(обводка, заливка, ширина)` заголовка скина либо `None`.
///
/// Обводка тонкая и светлая намеренно: при реальном кегле заголовка толстая
/// тёмная обводка съедала букву и текст выглядел нечитаемым.
pub fn heading_outline_colors(skin_id: &str) -> Option<(&'static str, &'static str, u32)> {
    if skin_id == "synthwave" {
        Some(("#2bd83a", "#9dff7a", 1))
    } else {
        None
    }
}

/// Обводка заголовка по умолчанию `(обводка, заливка, ширина)` — какой её
/// заводит сам виджет заголовка до включения скином.
pub fn heading_default_outline() -> (&'static str, &'static str, u32) {
    ("#ff2d95", "#eaf6ff", 2)
}

/// Файл бандл-шрифта заголовков скина (в `rexon/assets/fonts`) либо `None`.
/// Регистрирует файл в шрифтовой базе Qt исполнитель.
pub fn skin_heading_font(skin_id: &str) -> Option<&'static str> {
    skin(skin_id).heading_font
}

/// Свечение заголовка скина `(цвет, радиус размытия)` либо `None` — тяжёлый
/// эффект вешается только там, где он часть оформления.
pub fn skin_glow(skin_id: &str) -> Option<(&'static str, u32)> {
    skin(skin_id).glow
}

/// Цвет штриха галочки индикаторов для QSS-оверлея скина либо `None`.
///
/// `None` означает и «оверлея нет»: файл галочки в кеш писать незачем.
/// Раньше цвет был вписан в Python рядом с вызовом, и скину без оверлея всё
/// равно готовился SVG-файл.
pub fn skin_indicator_check_color(skin_id: &str) -> Option<&'static str> {
    skin(skin_id).check_color
}

// ============================================================================
// Тёмная ли активная тема
// ============================================================================

/// Порог яркости, ниже которого тема считается тёмной.
pub const DARK_LEVEL: u32 = 128;

/// Тёмная ли тема при таком цвете фона окна.
///
/// Единственное место, где живёт этот критерий: средняя яркость каналов ниже
/// половины. До переноса вкладки решали по-разному — часть считала среднее по
/// каналам, часть спрашивала у Qt `lightness()` (полусумму крайних каналов), и
/// на насыщенном цвете ответы расходились: жёлтый `#ffff00` даёт среднее 170
/// (светлая) и `lightness` 127 (тёмная). Правило #12 — критерий один на весь
/// продукт, наблюдение (сам цвет) приходит параметром.
pub fn is_dark_rgb(r: u8, g: u8, b: u8) -> bool {
    u32::from(r) + u32::from(g) + u32::from(b) < 3 * DARK_LEVEL
}

/// Тёмный ли ДЕЙСТВУЮЩИЙ скин — по его основному фону и общим критерием
/// [`is_dark_rgb`], а не по имени: скин выбирает покупатель, и «Светлый»
/// обязан считаться светлым. Одно место на все QML-страницы, которым краски
/// вкладки нужны парой «тёмная/светлая» («Steam», «Расширения»); до этого
/// каждая держала свою копию в мосте.
pub fn active_skin_is_dark() -> bool {
    active_skin_colors()
        .get("bg_primary")
        .and_then(|c| parse_hex(c))
        .map(|(r, g, b)| is_dark_rgb(r, g, b))
        .unwrap_or(true)
}

/// Цвет плейсхолдера (`QPalette::PlaceholderText`) скина — им виджетные
/// вкладки красят невыбираемые заголовки секций списков. У скина с палитрой
/// роль задана явно; у системного скина роль остаётся системной, и ближайший
/// ответ ядра — вторичный текст с той прозрачностью, какой план палитры
/// красит плейсхолдер на тёмной базе (`SYSTEM_PLACEHOLDER_ALPHA`).
/// Одно место на все QML-страницы: до этого страница брала вторичный текст
/// с прозрачностью 0.7 — своё число вместо числа ядра.
pub fn placeholder_text_color(skin_id: &str) -> String {
    if let Some(spec) = skin_palette(skin_id) {
        if let Some((_, hex)) = spec.iter().find(|(role, _)| *role == "PlaceholderText") {
            return (*hex).to_string();
        }
    }
    let colors = skin_colors(skin_id);
    let (r, g, b) = colors
        .get("text_secondary")
        .and_then(|c| parse_hex(c))
        .unwrap_or((255, 255, 255));
    format!("#{SYSTEM_PLACEHOLDER_ALPHA:02x}{r:02x}{g:02x}{b:02x}")
}

/// То же для ДЕЙСТВУЮЩЕГО скина.
pub fn active_placeholder_text_color() -> String {
    placeholder_text_color(active_skin_id())
}

// ============================================================================
// Палитра приложения: план присваиваний
// ============================================================================
//
// Сама QPalette собирается Qt-исполнителем, но ЧТО в неё писать — решение, и
// оно живёт здесь: какие роли, в каких группах, какой прозрачности. Иначе
// правило «неактивный текст приглушаем» и форс белого на тёмной системной
// теме лежали бы в Python второй копией знания о скинах.

/// Группы палитры Qt, в которые скин пишет цвета.
pub const PALETTE_GROUPS: &[&str] = &["Active", "Inactive", "Disabled"];

/// Роли текста, которым приглушается неактивное (disabled) состояние.
pub const TEXT_ROLES: &[&str] = &["WindowText", "Text", "ButtonText"];

/// Прозрачность приглушённого текста: читаемая индикация неактивного.
pub const DISABLED_TEXT_ALPHA: u8 = 140;

/// Прозрачность плейсхолдера при форсе белого на тёмной системной теме.
pub const SYSTEM_PLACEHOLDER_ALPHA: u8 = 190;

/// Одно присваивание цвета в палитре: группа, роль, цвет с прозрачностью.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteAssign {
    pub group: &'static str,
    pub role: &'static str,
    pub rgba: (u8, u8, u8, u8),
}

/// Разобрать `#rgb` или `#rrggbb` в каналы. Другие формы (именованные цвета,
/// `#aarrggbb`) не поддерживаются намеренно: в таблицах скинов их нет, а
/// сторож [`skin_palettes_parse`] следит, чтобы не появились.
pub fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let s = value.strip_prefix('#')?;
    let d = |c: char| c.to_digit(16).map(|v| v as u8);
    let cs: Vec<char> = s.chars().collect();
    match cs.len() {
        3 => {
            let (r, g, b) = (d(cs[0])?, d(cs[1])?, d(cs[2])?);
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let hx = |a: char, b: char| Some(d(a)? * 16 + d(b)?);
            Some((hx(cs[0], cs[1])?, hx(cs[2], cs[3])?, hx(cs[4], cs[5])?))
        }
        _ => None,
    }
}

/// Осветлить цвет подмешиванием белого — общий приём наведения на элемент
/// в акцентном цвете.
///
/// Своего оттенка «наведено» у скинов нет, и заводить его четырьмя ручными
/// значениями значило бы, что пятый скин снова про него забудет. `k` — доля
/// белого, 0.0 оставляет цвет как есть, 1.0 даёт белый; неразобранный цвет
/// возвращается без изменений, чтобы наведение не гасило кнопку в пустоту.
pub fn lighten_hex(value: &str, k: f64) -> String {
    let Some((r, g, b)) = parse_hex(value) else {
        return value.to_string();
    };
    let k = k.clamp(0.0, 1.0);
    let mix = |c: u8| -> u8 {
        (f64::from(c) + (255.0 - f64::from(c)) * k).round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", mix(r), mix(g), mix(b))
}

/// Что скин пишет в палитру приложения поверх системной.
///
/// `base_is_dark` — тёмная ли палитра, какой её отдала система при старте
/// (наблюдение исполнителя; см. [`is_dark_rgb`]). Порядок присваиваний
/// значим: последнее по роли и группе побеждает.
///
/// Скин «Системный» (без своей палитры) не навязывает ничего, кроме
/// исторического форса белого текста на тёмной системной теме: Breeze Dark
/// отдаёт сероватый `WindowText`, и подписи сливались с фоном. На светлой
/// системной теме план пустой — палитра остаётся такой, как её дала система.
pub fn palette_plan(skin_id: &str, base_is_dark: bool) -> Vec<PaletteAssign> {
    let mut out: Vec<PaletteAssign> = Vec::new();
    let mut push = |group: &'static str, role: &'static str, rgba| {
        out.push(PaletteAssign { group, role, rgba })
    };
    match skin(skin_id).palette {
        None => {
            if !base_is_dark {
                return out;
            }
            for role in TEXT_ROLES {
                push("Active", role, (255, 255, 255, 255));
                push("Inactive", role, (255, 255, 255, 255));
                push("Disabled", role, (255, 255, 255, DISABLED_TEXT_ALPHA));
            }
            // Плейсхолдер задаётся сразу во всех группах (в Qt это форма
            // `setColor(role, color)` без указания группы).
            for group in PALETTE_GROUPS {
                push(
                    group,
                    "PlaceholderText",
                    (255, 255, 255, SYSTEM_PLACEHOLDER_ALPHA),
                );
            }
        }
        Some(spec) => {
            for (role, hex) in spec {
                let Some((r, g, b)) = parse_hex(hex) else {
                    continue;
                };
                for group in PALETTE_GROUPS {
                    push(group, role, (r, g, b, 255));
                }
            }
            // Незаданные роли остаются системными — скин перечисляет только
            // то, что меняет.
            let win = spec
                .iter()
                .find(|(k, _)| *k == "WindowText")
                .map(|(_, v)| *v)
                .unwrap_or("#ffffff");
            let (r, g, b) = parse_hex(win).unwrap_or((255, 255, 255));
            for role in TEXT_ROLES {
                push("Disabled", role, (r, g, b, DISABLED_TEXT_ALPHA));
            }
        }
    }
    out
}

// ============================================================================
// Всплывающий тост
// ============================================================================
//
// Куда и какого размера лечь плашке — арифметика, и она вся здесь: Qt только
// мерит текст и переносит виджет. До переноса расчёт лежал в Python внутри
// `_reposition`, и каждый его край («не выше видимой области», «не уже 160»)
// был отдельной строкой без теста.

/// Отступ тоста от краёв родителя.
pub const TOAST_MARGIN: i32 = 12;
/// Сколько тост держится по умолчанию, миллисекунды (0 — до снятия вручную).
pub const TOAST_TIMEOUT_MS: u32 = 5000;
/// Сколько мс на символ даём прочесть длинный тост сверх обычного срока.
/// Взрослый читает около 15 знаков в секунду; 60 мс — с запасом.
pub const TOAST_READ_MS_PER_CHAR: u32 = 60;
/// Дольше этого информационный тост не держится даже при длинном тексте:
/// плашка лежит поверх кнопок и не должна закрывать их надолго.
pub const TOAST_MAX_TIMEOUT_MS: u32 = 12_000;

/// Срок жизни информационного тоста с этим текстом, мс.
///
/// Короткий — общий [`TOAST_TIMEOUT_MS`], как у всех плашек Rexon; длинный
/// получает время прочесть (символы × [`TOAST_READ_MS_PER_CHAR`] плюс
/// секунда на то, чтобы заметить), но не больше [`TOAST_MAX_TIMEOUT_MS`].
/// Никогда не 0: ноль значит «висеть до закрытия», а это удел ошибок, по
/// которым человеку есть что сделать (дефект 30.10.2026 — сообщение об
/// откате обоев висело больше минуты поверх фильтра).
pub fn toast_timeout_for_text(text: &str) -> u32 {
    let chars = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    let read = chars.saturating_mul(TOAST_READ_MS_PER_CHAR).saturating_add(1000);
    read.clamp(TOAST_TIMEOUT_MS, TOAST_MAX_TIMEOUT_MS)
}

/// Минимальная ширина, ниже которой плашку не жмём.
pub const TOAST_MIN_WIDTH: i32 = 160;
/// Наибольшая ширина плашки: длинный ответ переносится, а не тянет тост во
/// всю страницу.
pub const TOAST_MAX_WIDTH: i32 = 520;
/// Запас к измеренной ширине текста, точек.
///
/// ⚠️ Не вкусовщина. И `QTextDocument::idealWidth` у виджетной метки, и
/// `implicitWidth` у форматированного текста в QML отдают ДРОБНОЕ число, а
/// в расчёт оно попадает целым (усечением). Без запаса строка, которая
/// влезала целиком, переносилась на вторую: замер 08.09.2026 — текст
/// «✅ Копия панели сохранена» мерился как 173,59, плашка выходила по 173, и
/// последнее слово уезжало вниз.
pub const TOAST_TEXT_SLACK: i32 = 4;
/// Минимальная высота видимой области родителя, на которую рассчитываем.
pub const TOAST_MIN_VISIBLE_HEIGHT: i32 = 48;
/// Минимальная высота самой плашки.
pub const TOAST_MIN_HEIGHT: i32 = 24;

/// Ширина плашки: ширина текста в одну строку, но не шире максимума и не
/// шире родителя за вычетом полей.
///
/// `ideal_w` — ширина текста в одну строку, `hint_w` — подсказка виджета
/// (у метки с переносом она «квадратная», поэтому одной её мало).
pub fn toast_width(parent_w: i32, ideal_w: i32, hint_w: i32, max_w: i32) -> i32 {
    let avail = (parent_w - 2 * TOAST_MARGIN).max(TOAST_MIN_WIDTH);
    ideal_w.max(hint_w).min(max_w).min(avail)
}

/// Высота ВИДИМОЙ части родителя.
///
/// `chain` — цепочка `(смещение узла внутри родителя, высота родителя)` от
/// самого родителя тоста вверх до окна. Считать по высоте одного родителя
/// нельзя: вкладка бывает выше отведённого ей места (её окно тянет свой
/// минимальный размер), нижние пиксели обрезаются, и двухстрочный тост
/// уезжал за край окна.
pub fn toast_visible_height(parent_h: i32, chain: &[(i32, i32)]) -> i32 {
    let mut view = parent_h;
    let mut offset = 0;
    for (node_y, parent_height) in chain {
        offset += node_y;
        view = view.min(parent_height - offset);
    }
    view.max(TOAST_MIN_VISIBLE_HEIGHT)
}

/// Место и высота плашки: `(x, y, высота)`.
///
/// `height_for_width` — высота текста при выбранной ширине (её меряет Qt с
/// учётом переноса строк); неположительное значение означает «мерить нечем»,
/// тогда берётся подсказка виджета `hint_h`. Длинный текст лучше подрезать,
/// чем увести тост за границу окна, поэтому высота зажата видимой областью.
pub fn toast_placement(visible_h: i32, height_for_width: i32, hint_h: i32) -> (i32, i32, i32) {
    let mut h = if height_for_width > 0 {
        height_for_width
    } else {
        hint_h
    };
    h = h.min((visible_h - 2 * TOAST_MARGIN).max(TOAST_MIN_HEIGHT));
    (
        TOAST_MARGIN,
        TOAST_MARGIN.max(visible_h - h - TOAST_MARGIN),
        h,
    )
}

// ============================================================================
// Сообщения Qt в журнал
// ============================================================================
//
// ⛔ Обработчика сообщений на Python здесь нет и быть не может: PySide6
// захватывает GIL в том потоке, который печатает, а Qt печатает и из
// QQmlThread — приложение встаёт намертво. Печатает штатный обработчик Qt на
// C++, мы лишь задаём ему формат и направление переменными окружения.

/// Формат сообщений Qt — 1-в-1 формат строк общего журнала Rexon
/// (`crate::applog::format_line`: «время.мс [процесс] УРОВЕНЬ область:
/// текст», уровень ровно в пять знаков), чтобы строки Qt не выбивались из
/// файла. Область — `qt`; категория Qt (`qt.qml`, `qt.quick`…) идёт перед
/// текстом, если задана. Номер процесса обязателен: в тот же файл пишут и
/// окно, и хост обоев, и строки Qt иначе не отличить.
pub const QT_MESSAGE_PATTERN: &str = "%{time yyyy-MM-dd hh:mm:ss.zzz} [%{pid}] \
     %{if-debug}DEBUG%{endif}%{if-info}INFO %{endif}\
     %{if-warning}WARN %{endif}%{if-critical}ERROR%{endif}\
     %{if-fatal}ERROR%{endif} qt: \
     %{if-category}%{category}: %{endif}%{message}";

/// Переменные окружения, которыми Qt направляется в журнал Rexon.
///
/// `QT_FORCE_STDERR_LOGGING` нужен потому, что Qt на многих дистрибутивах
/// (CachyOS среди них) собран с journald и по умолчанию шлёт сообщения в
/// системный журнал мимо stderr.
pub fn qt_logging_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("QT_FORCE_STDERR_LOGGING", "1"),
        ("QT_MESSAGE_PATTERN", QT_MESSAGE_PATTERN),
    ]
}

/// Заводить ли дескриптор 2 в файл журнала.
///
/// Да — только при запуске без консоли (ярлык «Пуск», systemd-юнит): stderr
/// там либо сокет journald, либо символьное устройство `/dev/null`. Терминал,
/// файл и пайп — явный выбор запустившего, его не перехватываем, иначе
/// разработчик перестанет видеть сообщения там, где ожидает.
pub fn should_redirect_stderr(
    stderr_is_tty: bool,
    fd_is_socket: bool,
    fd_is_chardev: bool,
    fd_is_tty: bool,
) -> bool {
    if stderr_is_tty {
        return false;
    }
    fd_is_socket || (fd_is_chardev && !fd_is_tty)
}

// ============================================================================
// Сетевые сбои человеческим языком
// ============================================================================

/// Сырая ошибка сети → фраза, понятная покупателю (правило #13).
///
/// Показывать человеку `<urlopen error https://aur.archlinux.org/rpc/v5/...:
/// Network Error: Unexpected EOF>` нельзя: он видит скобки, URL и английский
/// жаргон и решает, что сломалась программа. Техническая строка нужна в
/// журнале, а на экране — что случилось и что с этим делать.
///
/// `source` — человеческое имя каталога («AUR», «KDE Store»), чтобы было
/// видно, ЧТО именно недоступно: остальные разделы могут работать. Имя
/// магазина не склоняется, склоняется слово «каталог».
pub fn humanize_network_error(raw: &str, source: &str) -> String {
    let low = raw.to_lowercase();
    // Фразы целиком, по две на причину — с именем каталога и без: падеж
    // слова «каталог» зависит от места во фразе, и склеивать его из кусков
    // значило бы навязать русский порядок слов всем языкам.
    let named = !source.is_empty();

    const DNS: &[&str] = &[
        "name or service not known",
        "nodename nor servname",
        "temporary failure in name resolution",
        "getaddrinfo",
        "no address associated",
    ];
    const BROKEN: &[&str] = &[
        "unexpected eof",
        "connection reset",
        "connection refused",
        "connection aborted",
        "ssl",
        "tls",
        "certificate",
    ];

    if low.contains("timed out") || low.contains("timeout") {
        return if named {
            // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
            trf("⚠️ The %1 catalog didn't respond in time. The service may be overloaded — try again.", &[&source])
        } else {
            tr("⚠️ The catalog didn't respond in time. The service may be overloaded — try again.")
        };
    }
    if DNS.iter().any(|s| low.contains(s)) {
        return if named {
            // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
            trf("⚠️ Couldn't find the address of the %1 catalog. Check your internet connection and DNS settings.", &[&source])
        } else {
            tr("⚠️ Couldn't find the catalog's address. Check your internet connection and DNS settings.")
        };
    }
    if BROKEN.iter().any(|s| low.contains(s)) {
        return if named {
            trf(
                // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
                "⚠️ The connection to the %1 catalog was interrupted. This happens when the \
                 connection drops, or when access to it is blocked by a firewall, VPN or \
                 internet provider.",
                &[&source],
            )
        } else {
            tr("⚠️ The connection to the catalog was interrupted. This happens when the \
                connection drops, or when access to it is blocked by a firewall, VPN or \
                internet provider.")
        };
    }
    if low.contains("unreachable") || low.contains("no route to host") {
        return if named {
            // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
            trf("⚠️ The network is unreachable — couldn't contact the %1 catalog.", &[&source])
        } else {
            tr("⚠️ The network is unreachable — couldn't contact the catalog.")
        };
    }
    if let Some(code) = http_status_code(raw) {
        return if named {
            // TRANSLATORS: %1 is a catalog name (AUR, KDE Store…), %2 an HTTP status code.
            trf("⚠️ The %1 catalog returned error %2. This is most likely temporary — try again later.", &[&source, &code])
        } else {
            // TRANSLATORS: %1 is an HTTP status code.
            trf("⚠️ The catalog returned error %1. This is most likely temporary — try again later.", &[&code])
        };
    }
    if named {
        // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
        trf("⚠️ Couldn't contact the %1 catalog. Check your internet connection and try again.", &[&source])
    } else {
        tr("⚠️ Couldn't contact the catalog. Check your internet connection and try again.")
    }
}

/// Трёхзначный код ответа HTTP в сыром тексте ошибки: «HTTP Error 503: ...»,
/// «status 429», «status code 429».
///
/// Порт регулярки `\b(?:HTTP\s*Error|status(?:\s*code)?)\s*[:=]?\s*(\d{3})\b`
/// без движка регулярок. Границы слова считаются как в Python: буква любого
/// алфавита, цифра и подчёркивание — часть слова.
pub fn http_status_code(raw: &str) -> Option<String> {
    let cs: Vec<char> = raw.chars().collect();
    let n = cs.len();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    // Ключевое слово без учёта регистра; отдаёт позицию за ним.
    let eat = |mut i: usize, kw: &str| -> Option<usize> {
        for k in kw.chars() {
            if i >= n || cs[i].to_lowercase().next() != Some(k) {
                return None;
            }
            i += 1;
        }
        Some(i)
    };
    let spaces = |mut i: usize| -> usize {
        while i < n && cs[i].is_whitespace() {
            i += 1;
        }
        i
    };
    for start in 0..n {
        if start > 0 && word(cs[start - 1]) {
            continue; // не на границе слова
        }
        // Варианты префикса; для «status» пробуем сначала с «code» (как
        // жадная группа регулярки), затем без него.
        let mut heads: Vec<usize> = Vec::new();
        if let Some(i) = eat(start, "http") {
            if let Some(j) = eat(spaces(i), "error") {
                heads.push(j);
            }
        }
        if let Some(i) = eat(start, "status") {
            if let Some(j) = eat(spaces(i), "code") {
                heads.push(j);
            }
            heads.push(i);
        }
        for head in heads {
            let mut i = spaces(head);
            if i < n && (cs[i] == ':' || cs[i] == '=') {
                i += 1;
            }
            i = spaces(i);
            if i + 3 > n || !cs[i..i + 3].iter().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if i + 3 < n && word(cs[i + 3]) {
                continue; // «5031» — не код ответа
            }
            return Some(cs[i..i + 3].iter().collect());
        }
    }
    None
}

// ============================================================================
// Адаптивная сетка карточек
// ============================================================================
//
// Сколько карточек фиксированной ширины влезает в полосу — чистая
// арифметика, и она одна на все вкладки с сетками (правило #12). Раньше
// каждая вкладка считала по-своему: «Оформление» делила ширину на ширину
// карточки, «Расширения» — ступенями по ширине окна, вкладка Steam вообще
// держала число столбцов константой. Наблюдения (ширина viewport'а,
// ширина карточки, промежуток) приходят параметрами — Qt отсюда не видно.

/// Сколько столбцов карточек шириной `card_width` влезает в полосу
/// `viewport_width` при промежутке `spacing` и суммарных полях `margins`.
///
/// Результат зажат в `[min_cols, max_cols]` и никогда не меньше единицы:
/// пустая сетка бесполезна, а карточка шире полосы всё равно показывается
/// (её обрежет скроллбар, но пользователь увидит содержимое).
///
/// Вырожденные наблюдения не считаются ошибкой — окно спрашивает ширину и
/// до первого показа, когда её ещё нет:
/// * `viewport_width <= 0` (окно не разложено) → `min_cols`;
/// * `card_width <= 0` (карточка без размера) → `max_cols`, потому что
///   делить не на что, а прижимать к одному столбцу заведомо неверно.
pub fn grid_columns(
    viewport_width: i32,
    card_width: i32,
    spacing: i32,
    margins: i32,
    min_cols: u32,
    max_cols: u32,
) -> u32 {
    let lo = min_cols.max(1);
    let hi = max_cols.max(lo);
    if card_width <= 0 {
        return hi;
    }
    if viewport_width <= 0 {
        return lo;
    }
    let gap = spacing.max(0) as i64;
    let usable = (viewport_width as i64) - (margins.max(0) as i64);
    if usable < card_width as i64 {
        return lo;
    }
    // n карточек влезают, если n*card + (n-1)*gap <= usable.
    let fit = (usable + gap) / (card_width as i64 + gap);
    (fit.clamp(lo as i64, hi as i64)) as u32
}

/// Размер страницы сетки при `cols` столбцах и `rows` рядах.
///
/// Пагинация задаётся рядами, а не числом предметов: иначе при пяти
/// столбцах страница из 25 штук ровно пятирядная, а при четырёх —
/// шесть рядов и «хвост» в один элемент.
pub fn grid_page_size(cols: u32, rows: u32) -> u32 {
    cols.max(1) * rows.max(1)
}

/// Номер страницы (с нуля), на которой лежит элемент `index` при размере
/// страницы `page_size`.
///
/// Нужен при смене числа столбцов: страница меняет размер, и чтобы
/// пользователь не потерял место, номер пересчитывают по индексу первого
/// элемента прежней страницы.
pub fn grid_page_for_index(index: u32, page_size: u32) -> u32 {
    index / page_size.max(1)
}

/// Сколько всего страниц у `total` элементов при размере страницы
/// `page_size`. Пустой список — одна (пустая) страница, чтобы подпись
/// «Страница 1 / 1» не превращалась в «1 / 0».
pub fn grid_page_count(total: u32, page_size: u32) -> u32 {
    let size = page_size.max(1);
    if total == 0 {
        1
    } else {
        (total + size - 1) / size
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    #[test]
    fn fits_by_exact_arithmetic() {
        // 3 карточки по 180 + 2 промежутка по 12 = 564, плюс поля 16 = 580.
        assert_eq!(grid_columns(580, 180, 12, 16, 1, 12), 3);
        // На пиксель меньше — третья уже не влезает.
        assert_eq!(grid_columns(579, 180, 12, 16, 1, 12), 2);
        // На пиксель больше — по-прежнему три, четвёртой нужно ещё 192.
        assert_eq!(grid_columns(581, 180, 12, 16, 1, 12), 3);
        // Ровно на границе четвёртой: 4*180 + 3*12 + 16 = 772.
        assert_eq!(grid_columns(772, 180, 12, 16, 1, 12), 4);
    }

    #[test]
    fn zero_and_negative_width_give_minimum() {
        assert_eq!(grid_columns(0, 180, 12, 16, 1, 12), 1);
        assert_eq!(grid_columns(-1000, 180, 12, 16, 1, 12), 1);
        assert_eq!(grid_columns(0, 180, 12, 16, 2, 12), 2);
    }

    #[test]
    fn card_wider_than_viewport_still_shows_one() {
        assert_eq!(grid_columns(100, 180, 12, 16, 1, 12), 1);
        // Карточка ровно в полосу без полей — уже влезает.
        assert_eq!(grid_columns(180, 180, 12, 0, 1, 12), 1);
        // ...а с полями — нет, но столбец всё равно один.
        assert_eq!(grid_columns(180, 180, 12, 16, 1, 12), 1);
    }

    #[test]
    fn degenerate_card_width_gives_maximum() {
        assert_eq!(grid_columns(900, 0, 12, 16, 1, 6), 6);
        assert_eq!(grid_columns(900, -50, 12, 16, 1, 6), 6);
    }

    #[test]
    fn clamped_to_range() {
        assert_eq!(grid_columns(4000, 180, 12, 16, 1, 5), 5);
        assert_eq!(grid_columns(300, 180, 12, 16, 3, 8), 3);
        // Перепутанные границы не роняют расчёт: max подтягивается к min.
        assert_eq!(grid_columns(4000, 180, 12, 16, 4, 2), 4);
        // Нулевой минимум означает «хотя бы один».
        assert_eq!(grid_columns(0, 180, 12, 16, 0, 5), 1);
    }

    #[test]
    fn zero_spacing_is_plain_division() {
        assert_eq!(grid_columns(900, 180, 0, 0, 1, 99), 5);
        assert_eq!(grid_columns(899, 180, 0, 0, 1, 99), 4);
        // Отрицательный промежуток трактуется как нулевой.
        assert_eq!(grid_columns(900, 180, -20, 0, 1, 99), 5);
    }

    #[test]
    fn page_size_follows_columns() {
        assert_eq!(grid_page_size(5, 5), 25);
        assert_eq!(grid_page_size(4, 5), 20);
        assert_eq!(grid_page_size(0, 5), 5);
        assert_eq!(grid_page_size(7, 0), 7);
    }

    #[test]
    fn page_recomputed_from_first_visible_item() {
        // Была страница 3 из 25 (элементы 50..74) — первый элемент 50.
        // При четырёх столбцах страница стала 20, элемент 50 попал на 2-ю.
        assert_eq!(grid_page_for_index(50, 20), 2);
        // Обратно к пяти столбцам — элемент 40 (первый второй страницы).
        assert_eq!(grid_page_for_index(40, 25), 1);
        assert_eq!(grid_page_for_index(0, 25), 0);
        assert_eq!(grid_page_for_index(7, 0), 7);
    }

    #[test]
    fn page_count_never_zero() {
        assert_eq!(grid_page_count(0, 25), 1);
        assert_eq!(grid_page_count(25, 25), 1);
        assert_eq!(grid_page_count(26, 25), 2);
        assert_eq!(grid_page_count(202, 25), 9);
        assert_eq!(grid_page_count(202, 20), 11);
        assert_eq!(grid_page_count(10, 0), 10);
    }
}

#[cfg(test)]
mod skin_extras_tests {
    use super::*;

    #[test]
    fn skin_palettes_parse() {
        // Сторож формата: в таблицах допускаются только #rgb/#rrggbb, иначе
        // роль молча выпала бы из плана и цвет остался системным.
        for s in SKINS {
            if let Some(spec) = s.palette {
                for (role, hex) in spec {
                    assert!(
                        parse_hex(hex).is_some(),
                        "скин {}: роль {role} задана как {hex}",
                        s.id
                    );
                }
            }
        }
    }

    #[test]
    fn hex_parsed_both_forms() {
        assert_eq!(parse_hex("#1b2838"), Some((27, 40, 56)));
        assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#0d1b2a"), Some((13, 27, 42)));
        assert_eq!(parse_hex("1b2838"), None);
        assert_eq!(parse_hex("#xyzxyz"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn dark_criterion_is_channel_average() {
        assert!(is_dark_rgb(0x1b, 0x28, 0x38)); // steam_dark
        assert!(is_dark_rgb(0x1a, 0x05, 0x33)); // synthwave
        assert!(!is_dark_rgb(0xf4, 0xf5, 0xf6)); // светлый
        // Ровно на пороге: 128,128,128 — среднее 128, уже НЕ тёмная.
        assert!(!is_dark_rgb(128, 128, 128));
        assert!(is_dark_rgb(127, 128, 128));
        // Насыщенный жёлтый — там расходились прежние два критерия.
        assert!(!is_dark_rgb(255, 255, 0));
    }

    /// Каждый скин со своей палитрой задаёт цвет ссылок, и он отличим от
    /// фона: иначе ссылки в форматированном тексте выходят системными
    /// `#0000ff` (дефект 01.11.2026 на «Тёмном (Steam)»).
    #[test]
    fn every_palette_skin_has_readable_link() {
        assert_eq!(skin_link_rgb("system"), None);
        for s in SKINS.iter().filter(|s| s.palette.is_some()) {
            let link = skin_link_rgb(s.id).unwrap_or_else(|| panic!("{}: нет роли Link", s.id));
            let bg = skin_colors(s.id)
                .get("bg_primary")
                .and_then(|c| parse_hex(c))
                .unwrap_or_else(|| panic!("{}: нет bg_primary", s.id));
            assert_ne!(
                is_dark_rgb(link.0, link.1, link.2),
                is_dark_rgb(bg.0, bg.1, bg.2),
                "{}: ссылка {link:?} сливается с фоном {bg:?}",
                s.id
            );
        }
    }

    #[test]
    fn system_skin_forces_white_only_on_dark() {
        assert!(palette_plan("system", false).is_empty());
        let plan = palette_plan("system", true);
        // Три роли текста × три группы + плейсхолдер в трёх группах.
        assert_eq!(plan.len(), 12);
        let dis: Vec<_> = plan
            .iter()
            .filter(|a| a.group == "Disabled" && a.role == "Text")
            .collect();
        assert_eq!(dis.len(), 1);
        assert_eq!(dis[0].rgba, (255, 255, 255, DISABLED_TEXT_ALPHA));
        let ph: Vec<_> = plan
            .iter()
            .filter(|a| a.role == "PlaceholderText")
            .collect();
        assert_eq!(ph.len(), 3);
        assert_eq!(ph[0].rgba, (255, 255, 255, SYSTEM_PLACEHOLDER_ALPHA));
    }

    #[test]
    fn explicit_skin_writes_all_groups_and_dims_disabled() {
        let plan = palette_plan("steam_dark", true);
        let spec = skin_palette("steam_dark").unwrap();
        assert_eq!(plan.len(), spec.len() * 3 + TEXT_ROLES.len());
        // Окно — во всех трёх группах ровно тем цветом, что в таблице.
        let win: Vec<_> = plan.iter().filter(|a| a.role == "Window").collect();
        assert_eq!(win.len(), 3);
        assert!(win.iter().all(|a| a.rgba == (0x1b, 0x28, 0x38, 255)));
        // Приглушённый текст — последним словом, поверх записи из таблицы.
        let last = plan
            .iter()
            .rev()
            .find(|a| a.group == "Disabled" && a.role == "ButtonText")
            .unwrap();
        assert_eq!(last.rgba, (255, 255, 255, DISABLED_TEXT_ALPHA));
        assert_eq!(plan.last().unwrap().role, "ButtonText");
        // Светлый скин приглушает СВОИМ цветом текста, а не белым.
        let light = palette_plan("light", true);
        let dim = light
            .iter()
            .rev()
            .find(|a| a.group == "Disabled" && a.role == "Text")
            .unwrap();
        assert_eq!(dim.rgba, (0x1f, 0x23, 0x26, DISABLED_TEXT_ALPHA));
        // Неизвестный скин — как везде, откат к умолчанию.
        assert_eq!(palette_plan("нет-такого", true), plan);
    }

    #[test]
    fn skin_extras_only_where_declared() {
        assert_eq!(skin_glow("synthwave"), Some(("#00f0ff", 18)));
        assert_eq!(skin_glow("steam_dark"), None);
        assert_eq!(
            skin_heading_font("synthwave"),
            Some("Tektur-VariableFont_wdth_wght.ttf")
        );
        assert_eq!(skin_heading_font("system"), None);
        assert_eq!(skin_indicator_check_color("synthwave"), Some("#0d1b2a"));
        assert_eq!(skin_indicator_check_color("light"), None);
        assert_eq!(heading_default_outline(), ("#ff2d95", "#eaf6ff", 2));
        // Галочка нужна ровно тем скинам, у которых есть оверлей.
        for s in SKINS {
            assert_eq!(s.has_qss, s.check_color.is_some(), "скин {}", s.id);
        }
    }

    /// Информационный тост гаснет всегда: коротко — общий срок, длинно —
    /// время прочесть, но не дольше предела.
    #[test]
    fn info_toast_timeout_reads_and_never_sticks() {
        assert_eq!(toast_timeout_for_text(""), TOAST_TIMEOUT_MS);
        assert_eq!(toast_timeout_for_text("Сохранено"), TOAST_TIMEOUT_MS);
        let revert = "The live wallpaper could not start on this computer, so Rexon put your \
                      previous desktop wallpaper back.";
        let t = toast_timeout_for_text(revert);
        assert!(t > TOAST_TIMEOUT_MS && t < TOAST_MAX_TIMEOUT_MS, "{t}");
        assert_eq!(toast_timeout_for_text(&"ж".repeat(10_000)), TOAST_MAX_TIMEOUT_MS);
    }

    #[test]
    fn toast_width_clamped_by_parent_and_maximum() {
        // Обычный случай: текст в одну строку короче и максимума, и полосы.
        assert_eq!(toast_width(900, 400, 300, 520), 400);
        // Подсказка виджета шире идеальной ширины — берём её.
        assert_eq!(toast_width(900, 200, 300, 520), 300);
        // Максимум режет.
        assert_eq!(toast_width(900, 800, 300, 520), 520);
        // Узкое окно режет сильнее максимума.
        assert_eq!(toast_width(300, 800, 300, 520), 276);
        // Совсем узкое окно — не уже минимума.
        assert_eq!(toast_width(50, 800, 300, 520), TOAST_MIN_WIDTH);
    }

    #[test]
    fn toast_visible_height_takes_the_smallest_ancestor() {
        // Один родитель — его высота.
        assert_eq!(toast_visible_height(400, &[]), 400);
        // Вкладка выше отведённого места: родитель 800, окно даёт 500.
        assert_eq!(toast_visible_height(800, &[(0, 500)]), 500);
        // Смещение узла внутри родителя вычитается.
        assert_eq!(toast_visible_height(800, &[(40, 500)]), 460);
        // Цепочка складывает смещения.
        assert_eq!(toast_visible_height(800, &[(40, 700), (10, 500)]), 450);
        // Вырожденное окно — не ниже минимума.
        assert_eq!(toast_visible_height(10, &[]), TOAST_MIN_VISIBLE_HEIGHT);
        assert_eq!(toast_visible_height(800, &[(0, 0)]), TOAST_MIN_VISIBLE_HEIGHT);
    }

    #[test]
    fn toast_placement_keeps_plate_inside_window() {
        // Двухстрочный тост в обычном окне.
        assert_eq!(toast_placement(400, 56, 30), (12, 332, 56));
        // Мерить нечем — берём подсказку виджета.
        assert_eq!(toast_placement(400, 0, 30), (12, 358, 30));
        assert_eq!(toast_placement(400, -1, 30), (12, 358, 30));
        // Текст выше видимой области: подрезаем, а не уезжаем за край.
        let (x, y, h) = toast_placement(100, 400, 30);
        assert_eq!((x, y, h), (12, 12, 76));
        assert!(y + h + TOAST_MARGIN <= 100);
        // Совсем плоская область — плашка не ниже минимума, но и не выше края.
        assert_eq!(toast_placement(40, 400, 30), (12, 12, TOAST_MIN_HEIGHT));
    }

    #[test]
    fn stderr_redirected_only_without_console() {
        // Ярлык «Пуск»: /dev/null — символьное устройство, не терминал.
        assert!(should_redirect_stderr(false, false, true, false));
        // systemd-юнит: сокет journald.
        assert!(should_redirect_stderr(false, true, false, false));
        // Терминал — не перехватываем.
        assert!(!should_redirect_stderr(true, false, true, false));
        assert!(!should_redirect_stderr(false, false, true, true));
        // Файл или пайп (ни сокет, ни устройство) — явный выбор запустившего.
        assert!(!should_redirect_stderr(false, false, false, false));
    }

    #[test]
    fn qt_logging_env_is_pattern_and_stderr() {
        let env = qt_logging_env();
        assert_eq!(env.len(), 2);
        assert_eq!(env[0], ("QT_FORCE_STDERR_LOGGING", "1"));
        assert_eq!(env[1].0, "QT_MESSAGE_PATTERN");
        assert!(env[1].1.starts_with("%{time yyyy-MM-dd hh:mm:ss.zzz} [%{pid}] "));
        assert!(env[1].1.contains("%{endif} qt: "));
        assert!(env[1].1.ends_with("%{if-category}%{category}: %{endif}%{message}"));
        assert!(!env[1].1.contains('\n'));
    }

    #[test]
    fn network_error_classified_by_cause() {
        // Язык исходный: русский текст сторожит `tests/ui_base_parity.rs`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let t = |s: &str| humanize_network_error(s, "AUR");
        assert!(t("<urlopen error timed out>").starts_with("⚠️ The AUR catalog didn't respond"));
        assert!(t("TIMEOUT while reading").contains("didn't respond in time"));
        assert!(t("Name or service not known").contains("DNS settings"));
        assert!(t("getaddrinfo failed").contains("DNS settings"));
        assert!(t("[SSL: CERTIFICATE_VERIFY_FAILED]").contains("was interrupted"));
        assert!(t("Network Error: Unexpected EOF").contains("was interrupted"));
        assert!(t("Network is unreachable").contains("network is unreachable"));
        assert!(t("No route to host").contains("network is unreachable"));
        assert!(t("HTTP Error 503: Service Unavailable").contains("returned error 503"));
        assert!(t("status code 429").contains("returned error 429"));
        assert!(t("нечто непонятное").contains("Couldn't contact the AUR catalog"));
        // Без имени каталога — своя фраза, а не дыра на месте имени.
        assert!(humanize_network_error("", "").contains("contact the catalog."));
        assert!(humanize_network_error("timed out", "").starts_with("⚠️ The catalog didn't respond"));
    }

    #[test]
    fn http_code_found_like_the_original_regex() {
        assert_eq!(http_status_code("HTTP Error 503: nope").as_deref(), Some("503"));
        assert_eq!(http_status_code("http error 404").as_deref(), Some("404"));
        // Пробел между словами необязателен и может быть двойным — как у
        // регулярки оригинала (`HTTP\s*Error`).
        assert_eq!(http_status_code("HTTPError: 500").as_deref(), Some("500"));
        assert_eq!(http_status_code("HTTP  Error  301").as_deref(), Some("301"));
        assert_eq!(http_status_code("status 429").as_deref(), Some("429"));
        assert_eq!(http_status_code("status code 429").as_deref(), Some("429"));
        assert_eq!(http_status_code("status=418").as_deref(), Some("418"));
        // Четыре цифры — не код ответа (у оригинала `\d{3}\b`).
        assert_eq!(http_status_code("HTTP Error 5031").as_deref(), None);
        // Двух цифр мало.
        assert_eq!(http_status_code("status 42").as_deref(), None);
        // Слово должно начинаться на границе.
        assert_eq!(http_status_code("substatus 429").as_deref(), None);
        // «codes» — не «code»: после него цифр нет, и откат к короткому
        // варианту тоже не находит кода (проверено на регулярке оригинала).
        assert_eq!(http_status_code("status codes 429").as_deref(), None);
        // Русский текст вокруг не мешает и не ломает границы слова.
        assert_eq!(
            http_status_code("ответ сервера: HTTP Error 502 — плохо").as_deref(),
            Some("502")
        );
        assert_eq!(http_status_code("ошибка без кода").as_deref(), None);
    }
}
