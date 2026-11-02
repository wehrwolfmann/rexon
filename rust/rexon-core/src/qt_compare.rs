//! Порт `rexon/qt_compare/window.py` — вкладка «Сравнение рабочих столов».
//!
//! Вкладка целиком состоит из ДАННЫХ И РЕШЕНИЙ: какие пары предложить в
//! шапке, как прочитать таблицу, что показать в раскрытой карточке стола,
//! какого цвета полоса памяти, кому что порекомендовать, что написано в
//! таблице «X против Y». Ни один из этих вопросов не требует виджета, поэтому
//! отвечает на них ядро, а Qt-сторона осталась постройкой карточек.
//!
//! Сами ТТХ рабочих столов НЕ живут здесь: они уже есть в
//! [`crate::system::DE_META`] (память, нагрузка, тулкит, композитор, панель,
//! файловый менеджер, терминал, звёзды) — модуль их только читает и
//! оформляет. Второй копии таблицы сравнения в проекте нет и заводить её
//! нельзя: расхождение чисел на витрине покупатель заметит раньше нас.
//!
//! Значки рабочих столов ([`DE_ICONS`]) — тоже единый источник: до переноса
//! одна и та же таблица лежала ДВУМЯ копиями, в `qt_compare/window.py` и в
//! `qt_desktops/window.py` (правило #12). Теперь оба окна спрашивают ядро.
//!
//! ⚠️ Починки оригинала (правило #16 — баги при порте чиним, не
//! консервируем), каждая помечена `Починка:` по месту:
//!   1. значение «X11 (Wayland WIP)» красилось зелёным, как настоящий
//!      Wayland: проверка была на подстроку «Wayland» без оглядки на «WIP».
//!      Покупателю Budgie и LXQt показывались готовыми к Wayland;
//!   2. подсветка строки под курсором и подложка полосы памяти были
//!      прибиты к `rgba(0,0,0,…)` при любой теме — на тёмной (штатной для
//!      Rexon) это ход в неверную сторону, правило #9 требует пары цветов;
//!   3. в диалоге «X против Y» кнопка установки ПЕРВОГО стола была
//!      выделенной, а второго — обычной: одинаковые по смыслу кнопки
//!      выглядели по-разному.
//!
//! Сверка — `rust/rexon-core/tests/qt_compare_parity.rs` по снимку живого
//! Python-пути (`tests/golden/qt_compare_cases.json`).

use serde::Serialize;

use crate::i18n::{n_, tr, trc, trf};
use crate::system::{CpuLoad, DeMeta, DE_META};

// ============================================================================
// Значки рабочих столов
// ============================================================================

/// Значок-эмодзи каждого рабочего стола: список слева во «Рабочих столах»,
/// шапка карточки в «Сравнении».
///
/// `cinnamon` — псевдоним `cinnamon-mint`: под этим именем стол приходит из
/// переменных сессии, тогда как в базе пакетов он зовётся полным именем.
pub const DE_ICONS: &[(&str, &str)] = &[
    ("cinnamon-mint", "🍃"),
    ("cinnamon", "🍃"),
    ("xfce", "🐭"),
    ("mate", "🦣"),
    ("gnome", "🦶"),
    ("kde", "💎"),
    ("budgie", "🐦"),
    ("pantheon", "🏛️"),
    ("deepin", "🌊"),
    ("cosmic", "🪐"),
    ("hyprland", "🪟"),
    ("lxqt", "🪶"),
    ("niri", "🎞️"),
    ("sway", "🧩"),
    ("river", "🏞️"),
    ("wayfire", "🔥"),
];

/// Значок неизвестного рабочего стола.
pub const DE_ICON_FALLBACK: &str = "🖥️";

/// Значок стола по идентификатору.
pub fn de_icon(de_id: &str) -> &'static str {
    DE_ICONS
        .iter()
        .find(|(k, _)| *k == de_id)
        .map(|(_, v)| *v)
        .unwrap_or(DE_ICON_FALLBACK)
}

fn meta(de_id: &str) -> Option<&'static DeMeta> {
    DE_META.iter().find(|m| m.id == de_id)
}

// ============================================================================
// Неизменные подписи и размеры вкладки
// ============================================================================

// Подписи — функции, а не константы: язык интерфейса выбирается при запуске,
// и функцию, в отличие от константы, нельзя забыть перевести.

pub fn window_title() -> String {
    tr("Rexon — Desktop Comparison")
}
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 720);

pub fn header_title() -> String {
    tr("📊 Desktop Comparison")
}
pub const HEADER_TITLE_POINT_SIZE: i32 = 20;
pub fn pairs_label() -> String {
    tr("Quick pairs:")
}

pub fn legend_title() -> String {
    tr("<b>How to read the table</b>")
}
pub fn legend_body() -> String {
    tr("<b>Memory</b> — how much it takes right after login (depends on the computer)<br>\
<b>Customization ⭐</b> — from 1 (minimal) to 5 (most flexible)<br>\
<b>Gaming 🎮</b> — how well it suits games<br>\
<b>Expand a row</b> → what it is built on, what draws the windows, panel, terminal")
}

pub fn table_header() -> String {
    tr("All Desktops")
}
pub fn rec_header() -> String {
    tr("🎯 Quick Recommendations")
}
pub const SECTION_POINT_SIZE: i32 = 14;

/// Быстрые пары в шапке: `(подпись кнопки, слева, справа)`.
///
/// Набор продуктовый: самые частые вопросы покупателя, пришедшего с Windows
/// («что легче», «что привычнее», «что из тайлового»). Что идентификаторы
/// существуют в базе столов — проверяет `qt_compare_parity.rs`.
pub const QUICK_PAIRS: &[(&str, &str, &str)] = &[
    ("XFCE ↔ KDE", "xfce", "kde"),
    ("GNOME ↔ Cinnamon", "gnome", "cinnamon-mint"),
    ("MATE ↔ Budgie", "mate", "budgie"),
    ("Hyprland ↔ Sway", "hyprland", "sway"),
];

/// Быстрые рекомендации: `(кому, чем обосновано, какой стол предложить)`.
///
/// Тексты — `msgid` каталога; переводит их место показа ([`rec_rows`]).
pub const RECOMMENDATIONS: &[(&str, &str, &str)] = &[
    (n_("🎮 For gamers"), n_("Minimal overhead → XFCE or KDE"), "xfce"),
    (n_("💻 Low-end hardware"), n_("Under 2 GB of RAM → MATE or XFCE"), "mate"),
    (n_("🎨 For eye candy fans"), n_("Animations and widgets → KDE Plasma"), "kde"),
    (
        n_("🟢 For newcomers"),
        n_("Simple out of the box → Cinnamon or GNOME"),
        "cinnamon-mint",
    ),
    (n_("🔧 For power users"), n_("Full customization → KDE Plasma"), "kde"),
    (n_("⚡ Maximum speed"), n_("The fastest one → XFCE"), "xfce"),
    (
        n_("⌨️ For keyboard lovers"),
        n_("Keyboard-driven window management → Hyprland"),
        "hyprland",
    ),
    (
        n_("🧩 Tiling out of the box"),
        n_("Automatic window layout → Sway, niri or River"),
        "sway",
    ),
    (
        n_("🎞️ Scrollable tiling"),
        n_("Windows on an endless strip → niri"),
        "niri",
    ),
    (
        n_("✨ Effects like in Compiz"),
        n_("Wobbly windows and the desktop cube → Wayfire"),
        "wayfire",
    ),
];

// ============================================================================
// Оформление карточек
// ============================================================================

/// Стиль карточки-рамки по её `objectName`, с оглядкой на тему.
///
/// Сброс рамки и подложки нужен и `QLabel`, и вложенным `QWidget`: тема Qt
/// иначе рисует серые рамки внутри карточки.
pub fn card_qss(object_name: &str, dark: bool) -> String {
    // Заливка — общая поверхность продукта (`ui_base::CARD_FILL_*`), см.
    // `qt_steam::card_colors`: QML кладёт ту же краску готовой
    // (`surface_card`).
    let (bg, border) = if dark {
        (crate::ui_base::CARD_FILL_DARK, "rgba(255,255,255,0.12)")
    } else {
        (crate::ui_base::CARD_FILL_LIGHT, "rgba(0,0,0,0.15)")
    };
    format!(
        "QFrame#{name} {{ background-color: {bg}; \
border: 1px solid {border}; border-radius: 6px; }} \
QFrame#{name} QLabel {{ border: none; background: transparent; }} \
QFrame#{name} QWidget {{ border: none; background: transparent; }}",
        name = object_name
    )
}

/// Стиль кликабельной шапки карточки.
///
/// Починка: подсветка под курсором была `rgba(0,0,0,0.04)` при ЛЮБОЙ теме —
/// на тёмной строка под курсором темнела вместо того, чтобы подсвечиваться
/// (правило #9: цвет выбирается парой).
pub fn card_header_qss(dark: bool) -> String {
    let hover = if dark {
        "rgba(255,255,255,0.06)"
    } else {
        "rgba(0,0,0,0.04)"
    };
    format!(
        "QPushButton {{ text-align: left; padding: 10px 14px; \
border: none; border-radius: 6px; }}\
QPushButton:hover {{ background-color: {hover}; }}"
    )
}

/// Стиль выделенной кнопки установки (синяя, с подсветкой под курсором).
pub const INSTALL_BUTTON_QSS: &str = "QPushButton { background-color: rgba(59,130,246,0.3); \
padding: 6px 14px; border-radius: 4px; font-weight: bold; }\
QPushButton:hover { background-color: rgba(59,130,246,0.45); }";

/// Те же краски кнопки установки голым цветом `#AARRGGBB` — для QML, где
/// QSS не работает: подложка и подложка под курсором. Второе число в
/// разметке развело бы две вкладки на одних данных; согласие с QSS выше
/// сторожит `qt_compare_parity::widget_qss_and_qml_colors_agree`.
pub const INSTALL_BUTTON_COLOR: &str = "#4d3b82f6";
pub const INSTALL_BUTTON_HOVER_COLOR: &str = "#733b82f6";
/// Скругление той же кнопки (`border-radius: 4px` в QSS).
pub const INSTALL_BUTTON_RADIUS: i32 = 4;

// ============================================================================
// Полоса памяти
// ============================================================================

/// Верх шкалы памяти: столов тяжелее гигабайта с небольшим у нас нет, а
/// фиксированный верх позволяет сравнивать полосы разных карточек на глаз.
pub const RAM_SCALE_MAX: u32 = 1200;
/// Высота полосы в точках.
pub const RAM_BAR_HEIGHT: u32 = 8;

/// Порог «зелёная → жёлтая» и «жёлтая → красная» в долях шкалы.
const RAM_GREEN_UPTO: f64 = 0.35;
const RAM_YELLOW_UPTO: f64 = 0.65;

/// Полоса потребления памяти: что выставить в виджет.
#[derive(Debug, Clone, Serialize)]
pub struct RamBar {
    pub max: u32,
    pub value: u32,
    pub height: u32,
    pub text_visible: bool,
    pub color: &'static str,
    pub qss: String,
}

/// Цвет полосы по потреблению памяти: зелёный — легко, жёлтый — средне,
/// красный — тяжело.
pub fn ram_color(ram_mb: u32) -> &'static str {
    let frac = (ram_mb as f64 / RAM_SCALE_MAX as f64).min(1.0);
    if frac < RAM_GREEN_UPTO {
        "#22c55e"
    } else if frac < RAM_YELLOW_UPTO {
        "#eab308"
    } else {
        "#ef4444"
    }
}

/// Канавка полосы памяти строкой QSS — для виджетной вкладки.
const RAM_TRACK_QSS_DARK: &str = "rgba(255,255,255,0.10)";
const RAM_TRACK_QSS_LIGHT: &str = "rgba(0,0,0,0.1)";

/// Та же канавка голым цветом `#AARRGGBB` — для QML: QSS там не работает,
/// а второе число в разметке развело бы две вкладки на одних данных.
/// Согласие обёрток сторожит `qt_compare_parity::widget_qss_and_qml_colors_agree`.
pub fn ram_track_color(dark: bool) -> &'static str {
    if dark { "#1affffff" } else { "#1a000000" }
}

/// Полоса памяти целиком.
///
/// Починка: подложка полосы была `rgba(0,0,0,0.1)` при любой теме (правило
/// #9) — на тёмной канавка сливалась с фоном карточки.
pub fn ram_bar(ram_mb: u32, dark: bool) -> RamBar {
    let color = ram_color(ram_mb);
    let track = if dark {
        RAM_TRACK_QSS_DARK
    } else {
        RAM_TRACK_QSS_LIGHT
    };
    RamBar {
        max: RAM_SCALE_MAX,
        value: ram_mb.min(RAM_SCALE_MAX),
        height: RAM_BAR_HEIGHT,
        text_visible: false,
        color,
        qss: format!(
            "QProgressBar {{ background: {track}; border-radius: 4px; }}\
QProgressBar::chunk {{ background: {color}; border-radius: 4px; }}"
        ),
    }
}

// ============================================================================
// Звёзды и цвет значения ТТХ
// ============================================================================

/// Оценка из пяти: `n` заполненных значков плюс пустые звёзды до пяти.
pub fn stars(n: u8, emoji: &str) -> String {
    let filled = n.min(5) as usize;
    let mut s = emoji.repeat(filled);
    s.push_str(&"☆".repeat(5 - filled));
    s
}

/// «Хорошие» значения ТТХ — их подсвечиваем зелёным.
///
/// Здесь только имена технологий: они не переводятся, поэтому сравнивать
/// их текст безопасно. Нагрузка на процессор — переводимая подпись, и её
/// цвет решается по признаку ([`cpu_load_color`]), а не по тексту: раньше
/// в этом списке стояло «Низкое», и английская подпись осталась бы без цвета.
const GOOD_VALUES: &[&str] = &["Qt6", "GTK4", "iced (Rust)"];

pub const GOOD_COLOR: &str = "#22c55e";
pub const BAD_COLOR: &str = "#ef4444";

/// Цвет нагрузки на процессор: низкая — зелёная, высокая — красная.
pub fn cpu_load_color(load: CpuLoad) -> &'static str {
    match load {
        CpuLoad::Low => GOOD_COLOR,
        CpuLoad::Medium => "",
        CpuLoad::High => BAD_COLOR,
    }
}

/// Цвет значения-имени (тулкит, сессия, композитор…) в строке ТТХ; пустая
/// строка — цвет не менять. Нагрузку сюда не подавать — у неё
/// [`cpu_load_color`].
///
/// Решение о цвете отделено от способа его нанести: QSS понимает только
/// QtWidgets, а QML-вкладка (`rexon-gui::compare_page`) красит значение
/// свойством `color`. Правило #12 — движок один, обёртки разные.
///
/// Починка: зелёным красилось любое значение со словом «Wayland», включая
/// «X11 (Wayland WIP)» у Budgie и LXQt — покупателю они выглядели готовыми к
/// Wayland, хотя сессия там X11, а Wayland в работе.
pub fn spec_value_color(value: &str) -> &'static str {
    let wayland_ready = value.contains("Wayland") && !value.contains("WIP");
    if GOOD_VALUES.contains(&value) || wayland_ready {
        return GOOD_COLOR;
    }
    ""
}

/// Стиль по цвету значения для QtWidgets; пустая строка — не менять.
pub fn color_qss(color: &str) -> String {
    match color {
        "" => String::new(),
        color => format!("color: {color};"),
    }
}

/// Стиль значения-имени в строке ТТХ для QtWidgets; пустая строка — не менять.
pub fn spec_value_qss(value: &str) -> String {
    color_qss(spec_value_color(value))
}

// ============================================================================
// Карточка одного стола
// ============================================================================

/// Подписи строк ТТХ — в том порядке, в котором их видит покупатель.
const SPEC_LABELS: [&str; 7] = [
    n_("🧰 Toolkit"),
    n_("🖥️ Session"),
    n_("🪟 Compositor"),
    n_("📊 Panel"),
    n_("📁 File manager"),
    n_("⌨️ Terminal"),
    n_("⚡ CPU load"),
];

/// Строка ТТХ: подпись, готовое значение, его цвет и стиль.
#[derive(Debug, Clone, Serialize)]
pub struct SpecRow {
    pub label: String,
    pub value: String,
    pub value_html: String,
    /// Цвет значения голым `#RRGGBB` (для QML); пусто — не менять.
    pub color: &'static str,
    pub qss: String,
}

/// Кнопка действия в карточке: установить или переключиться.
#[derive(Debug, Clone, Serialize)]
pub struct CardAction {
    pub de_id: String,
    /// `"install"` либо `"switch"` — какой сигнал послать.
    pub kind: &'static str,
    pub text: String,
    pub qss: &'static str,
}

/// Раскрытое тело карточки: всё, что в нём написано и нарисовано.
#[derive(Debug, Clone, Serialize)]
pub struct CardBody {
    pub ram_label: String,
    pub ram_bar: RamBar,
    /// Подписи «Настройка: ⭐…» и «Гейминг: 🎮…».
    pub stars_labels: Vec<String>,
    pub specs: Vec<SpecRow>,
    pub description: String,
    pub action: CardAction,
}

/// Строка-шапка карточки: стрелка, значок, имя, отметка установки и краткие
/// ТТХ одной строкой.
pub fn card_header(de_id: &str, expanded: bool, installed: bool) -> Option<String> {
    let m = meta(de_id)?;
    let arrow = if expanded { "▼" } else { "▶" };
    let mark = if installed { "  ✅" } else { "" };
    Some(format!(
        "{arrow}  {icon}  {name}{mark}   ·   {mem_word} {ram}   ·   {cpu_word} {cpu}   ·   {toolkit}   ·   {comp}",
        mem_word = trc("desktop spec", "Memory"),
        cpu_word = trc("desktop spec", "Processor"),
        icon = de_icon(de_id),
        name = m.display_name,
        ram = megabytes(m.ram_mb),
        cpu = m.cpu_usage.label(),
        toolkit = m.toolkit,
        comp = m.compositor_text(),
    ))
}

/// Объём памяти с единицей: «700 MB» / «700 МБ».
fn megabytes(mb: u32) -> String {
    trf("%1 MB", &[&mb])
}

/// Строки ТТХ одного стола.
pub fn specs(de_id: &str) -> Vec<SpecRow> {
    let Some(m) = meta(de_id) else {
        return Vec::new();
    };
    // Имена программ сравниваются как текст, нагрузка — по признаку.
    let values: [(String, &'static str); 7] = [
        (m.toolkit.to_string(), spec_value_color(m.toolkit)),
        (m.session_type.to_string(), spec_value_color(m.session_type)),
        (m.compositor_text(), spec_value_color(m.compositor)),
        (m.panel_type.to_string(), spec_value_color(m.panel_type)),
        (m.file_manager.to_string(), spec_value_color(m.file_manager)),
        (m.terminal.to_string(), spec_value_color(m.terminal)),
        (m.cpu_usage.label(), cpu_load_color(m.cpu_usage)),
    ];
    SPEC_LABELS
        .iter()
        .zip(values)
        .map(|(label, (value, color))| SpecRow {
            label: tr(label),
            value_html: format!("<b>{value}</b>"),
            value,
            color,
            qss: color_qss(color),
        })
        .collect()
}

/// Кнопка действия карточки: стол не стоит — предлагаем установить,
/// стоит — переключиться.
pub fn card_action(de_id: &str, installed: bool) -> CardAction {
    if installed {
        CardAction {
            de_id: de_id.to_string(),
            kind: "switch",
            text: tr("🔄 Switch"),
            qss: "",
        }
    } else {
        CardAction {
            de_id: de_id.to_string(),
            kind: "install",
            text: tr("📦 Install"),
            qss: INSTALL_BUTTON_QSS,
        }
    }
}

/// Тело карточки целиком.
pub fn card_body(de_id: &str, installed: bool, dark: bool) -> Option<CardBody> {
    let m = meta(de_id)?;
    Some(CardBody {
        ram_label: format!("<b>{}:</b>  {}", trc("desktop spec", "Memory"), megabytes(m.ram_mb)),
        ram_bar: ram_bar(m.ram_mb, dark),
        stars_labels: vec![
            trf("<b>Customization:</b>  %1", &[&stars(m.customizable, "⭐")]),
            trf("<b>Gaming:</b>  %1", &[&stars(m.gaming_friendly, "🎮")]),
        ],
        specs: specs(de_id),
        description: m.description_text(),
        action: card_action(de_id, installed),
    })
}

// ============================================================================
// Быстрые рекомендации
// ============================================================================

/// Отметка справа в строке рекомендации.
#[derive(Debug, Clone, Serialize)]
pub struct RecRow {
    pub label: String,
    /// Готовая подпись строки — полужирная, как её видит покупатель.
    pub label_html: String,
    pub sub: String,
    pub de_id: &'static str,
    /// `"badge"` — стол уже стоит, `"install"` — предложить установку,
    /// `"none"` — стол ядру неизвестен (строка остаётся без действия).
    pub mark: &'static str,
    pub text: String,
    pub qss: &'static str,
}

pub const REC_BADGE_TEXT: &str = "✅";
/// Цвет отметки «стол уже стоит». Отдельно от QSS по той же причине, что и
/// [`spec_value_color`]: QML красит свойством, а не таблицей стилей. Что обе
/// записи не разъехались — сторожит `qt_compare_parity.rs`.
pub const REC_BADGE_COLOR: &str = "#4ade80";
pub const REC_BADGE_QSS: &str = "color: #4ade80;";

/// Все строки блока рекомендаций для данного состава установленных столов.
pub fn rec_rows(installed: &[String]) -> Vec<RecRow> {
    RECOMMENDATIONS
        .iter()
        .map(|(label, sub, de_id)| {
            let (mark, text, qss) = if meta(de_id).is_none() {
                ("none", String::new(), "")
            } else if installed.iter().any(|i| i == de_id) {
                ("badge", REC_BADGE_TEXT.to_string(), REC_BADGE_QSS)
            } else {
                ("install", tr("📦 Install"), "")
            };
            let label = tr(label);
            RecRow {
                label_html: format!("<b>{label}</b>"),
                label,
                sub: tr(sub),
                de_id,
                mark,
                text,
                qss,
            }
        })
        .collect()
}

// ============================================================================
// Диалог «X против Y»
// ============================================================================

pub const PAIR_MIN_SIZE: (u32, u32) = (720, 520);
pub const PAIR_TITLE_POINT_SIZE: i32 = 16;
pub fn pair_close_text() -> String {
    tr("Close")
}

/// Кнопка установки в диалоге сравнения.
#[derive(Debug, Clone, Serialize)]
pub struct PairButton {
    pub de_id: String,
    pub text: String,
    pub qss: &'static str,
}

/// Диалог попарного сравнения целиком.
#[derive(Debug, Clone, Serialize)]
pub struct PairView {
    /// Заголовок окна.
    pub window_title: String,
    /// Крупная надпись внутри окна.
    pub heading: String,
    pub html: String,
    pub buttons: Vec<PairButton>,
    pub close_text: String,
    pub min_size: (u32, u32),
    pub title_point_size: i32,
}

fn pair_table(a: &DeMeta, b: &DeMeta) -> String {
    // Короткие слова строк таблицы — с контекстом: «Session» или «Panel»
    // в другом месте продукта могут значить другое.
    let rows: [(String, String, String); 10] = [
        (trc("desktop spec", "Memory"), megabytes(a.ram_mb), megabytes(b.ram_mb)),
        (trc("desktop spec", "CPU load"), a.cpu_usage.label(), b.cpu_usage.label()),
        // Оценки — теми же значками, что в легенде над таблицей и в
        // карточках (`card_body`). Прежняя полоска из «█» и «░» шла в
        // запасном шрифте: клетки со щелями, пустая часть — штриховкой, и
        // с легендой «⭐ от 1 до 5» она не сходилась (01.11.2026).
        (
            trc("desktop spec", "Customization"),
            stars(a.customizable, "⭐"),
            stars(b.customizable, "⭐"),
        ),
        (
            trc("desktop spec", "Gaming"),
            stars(a.gaming_friendly, "🎮"),
            stars(b.gaming_friendly, "🎮"),
        ),
        (trc("desktop spec", "Toolkit"), a.toolkit.to_string(), b.toolkit.to_string()),
        (
            trc("desktop spec", "Session"),
            a.session_type.to_string(),
            b.session_type.to_string(),
        ),
        (trc("desktop spec", "Compositor"), a.compositor_text(), b.compositor_text()),
        (
            trc("desktop spec", "Panel"),
            a.panel_type.to_string(),
            b.panel_type.to_string(),
        ),
        (
            trc("desktop spec", "Files"),
            a.file_manager.to_string(),
            b.file_manager.to_string(),
        ),
        (trc("desktop spec", "Terminal"), a.terminal.to_string(), b.terminal.to_string()),
    ];
    let mut html = String::from("<table cellpadding=\"6\" cellspacing=\"0\" border=\"0\">");
    html.push_str(&format!(
        "<tr><th align='left'>{}</th><th align='left'>{}</th><th align='left'>{}</th></tr>",
        trc("desktop spec", "Feature"),
        a.display_name,
        b.display_name
    ));
    for (label, va, vb) in rows {
        html.push_str(&format!(
            "<tr><td><b>{label}</b></td><td>{va}</td><td>{vb}</td></tr>"
        ));
    }
    html.push_str("</table>");
    html.push_str(&format!(
        "<p><b>{}:</b> {}</p><p><b>{}:</b> {}</p>",
        a.display_name,
        a.description_text(),
        b.display_name,
        b.description_text()
    ));
    html
}

/// Диалог сравнения двух столов; `None` — хотя бы один ядру неизвестен.
///
/// Починка: кнопки установки обоих столов теперь выглядят одинаково — в
/// оригинале выделенной была только первая.
pub fn pair_view(
    a_id: &str,
    b_id: &str,
    a_installed: bool,
    b_installed: bool,
) -> Option<PairView> {
    let a = meta(a_id)?;
    let b = meta(b_id)?;
    let mut buttons = Vec::new();
    for (m, installed) in [(a, a_installed), (b, b_installed)] {
        if !installed {
            buttons.push(PairButton {
                de_id: m.id.to_string(),
                text: trf("📦 Install %1", &[&m.display_name]),
                qss: INSTALL_BUTTON_QSS,
            });
        }
    }
    Some(PairView {
        window_title: format!("⚔️ {} ↔ {}", a.display_name, b.display_name),
        heading: format!(
            "<b>⚔️ {}  ↔  {}</b>",
            a.display_name, b.display_name
        ),
        html: pair_table(a, b),
        buttons,
        close_text: pair_close_text(),
        min_size: PAIR_MIN_SIZE,
        title_point_size: PAIR_TITLE_POINT_SIZE,
    })
}

// ============================================================================
// Установка и переключение: тексты потока
// ============================================================================

/// Тексты установки стола: подтверждение, заголовок хода дела и итог.
#[derive(Debug, Clone, Serialize)]
pub struct InstallTexts {
    pub confirm_title: String,
    pub confirm_html: String,
    pub operation_title: String,
    pub done_title: String,
    pub done_text: String,
}

/// Тексты установки у обеих вкладок ОДНИ: берутся у
/// `qt_desktops::install_texts`. Прежняя копия здесь отличалась словом
/// («Установить KDE Plasma?» против «Установить рабочий стол KDE Plasma?»),
/// то есть уже начала разъезжаться (правило #12).
pub fn install_texts(display_name: &str) -> InstallTexts {
    let t = crate::qt_desktops::install_texts(display_name);
    InstallTexts {
        confirm_title: t.confirm_title.to_string(),
        confirm_html: t.confirm_html,
        operation_title: t.operation_title,
        done_title: t.done_title.to_string(),
        done_text: t.done_text,
    }
}

/// Тексты переключения на стол.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchTexts {
    pub confirm_title: String,
    pub confirm_text: String,
    pub operation_title: String,
}

pub fn switch_texts(display_name: &str) -> SwitchTexts {
    SwitchTexts {
        confirm_title: tr("Switch Desktops?"),
        confirm_text: trf(
            "Switch to <b>%1</b>?\n\n\
Rexon will back up the current desktop's settings and log you out.",
            &[&display_name],
        ),
        operation_title: trf("Switching to %1", &[&display_name]),
    }
}
