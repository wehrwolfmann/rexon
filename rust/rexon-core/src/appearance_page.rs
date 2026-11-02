//! Вкладка «Оформление» — числа оформления, подписи и таблицы стилей.
//!
//! Здесь живёт то, что вкладка ЗНАЕТ о своём виде: размеры плитки и панели
//! деталей, пороги загрузчика превью, паузы дебаунса, список сортировок и
//! готовые таблицы стилей. Python-сторона остаётся Qt-слоем: она строит
//! виджеты и подставляет то, что ей сказали, — своих чисел и своих цветов
//! у неё нет.
//!
//! ⚠️ Цвета берутся из ДЕЙСТВУЮЩЕГО скина (`ui_base::skin_colors`), а не из
//! запечённой Steam-палитры. Копия этой палитры жила во вкладке отдельным
//! словарём и не обновлялась при смене скина: подписи деталки, путь
//! установки, комментарии и выпадашка подсказок оставались белыми и на
//! светлом скине становились невидимы.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::appearance_cache;
use crate::i18n::{fill, n_, tr, trc, trf};
use crate::appearance_installer;
use crate::categories;
use crate::installed_scan;
use crate::ocs;
use crate::ocs_store;
use crate::translator;
use crate::models::{CatalogSource, ThemeCategory, ThemeItem};
use crate::ui_base;

// ═══════════════════════════════════════════════════════════════════════
//  Геометрия каталога
// ═══════════════════════════════════════════════════════════════════════

/// Сколько работ просим у OCS за одну серверную страницу. Сервер реально
/// отдаёт около десятка — это учтено в подсчёте числа страниц.
///
/// Это же — НИЖНЯЯ граница страницы Rexon: страница сетки набирается целыми
/// рядами ([`grid_page_size`]) и бывает чуть больше, но не меньше. Серверная
/// страница от числа столбцов не зависит, иначе кеш магазина (`ocs_cache`,
/// ключ без размера) отдавал бы ломти другой длины.
pub const PAGE_SIZE: u32 = 20;

/// Размер страницы сетки при `cols` столбцах: целые ряды, не меньше
/// [`PAGE_SIZE`] работ. Иначе при девяти столбцах страница из двадцати
/// кончалась рядом из двух плиток и семью пустыми местами.
pub fn grid_page_size(cols: u32) -> u32 {
    let cols = cols.max(1);
    ui_base::grid_page_size(cols, PAGE_SIZE.div_ceil(cols))
}

/// Номер страницы после смены её размера: первая работа прежней страницы
/// остаётся на экране.
pub fn page_after_resize(page: i64, old_size: u32, new_size: u32) -> i64 {
    let first = page.max(0).saturating_mul(i64::from(old_size.max(1)));
    first / i64::from(new_size.max(1))
}

/// Ширина плитки каталога. Постоянная: по ней общий движок сеток считает
/// число столбцов, и «плавающая» плитка сломала бы расчёт.
pub const CARD_W: i32 = 300;
/// Полная высота плитки каталога.
pub const CARD_H: i32 = 285;
/// Высота картинки в плитке.
pub const PREVIEW_H: i32 = 170;
/// Поля тела плитки (12 + 12) плюс запас на скруглении: на столько подписи
/// внутри плитки уже её самой.
pub const CARD_TEXT_MARGIN: i32 = 30;

/// Ширина подписей внутри плитки.
pub fn card_text_width() -> i32 {
    CARD_W - CARD_TEXT_MARGIN
}

/// Поля тела плитки по бокам.
pub const CARD_BODY_MARGIN: i32 = 12;
/// Поле тела плитки сверху.
///
/// ⚠️ Число взято с ОТРИСОВАННОЙ плитки оригинала, а не с его исходника, и
/// расходится с ним намеренно. В исходнике объявлено 14, но плитка имеет
/// жёсткую высоту (`CARD_H`), и содержимому тела её не хватает: `QVBoxLayout`
/// в такой тесноте съедает поля и зазоры, и верхнее поле в живой отрисовке
/// вырождается почти в ноль (замер снимка 01.09.2026: подпись начинается на
/// 2 точки ниже границы картинки, а не на 14). Раскладка QML полей не
/// сжимает — она вылезает за нижний край, и кнопка плитки обрезалась.
/// Поэтому здесь записано то, что человек ВИДИТ.
pub const CARD_BODY_TOP_MARGIN: i32 = 2;
/// Поле тела плитки снизу.
pub const CARD_BODY_BOTTOM_MARGIN: i32 = 10;
/// Зазор между строками внутри плитки.
pub const CARD_BODY_SPACING: i32 = 4;
/// Высота кнопки плитки (замер отрисованной кнопки оригинала).
pub const CARD_BUTTON_HEIGHT: i32 = 32;
/// Отступ ряда кнопок от стат-строки.
pub const CARD_BUTTON_TOP_MARGIN: i32 = 4;
/// Кегль названия темы в плитке. Крупнее прочих строк: имя — то, по чему
/// человек находит тему в сетке.
pub const CARD_NAME_POINT_SIZE: i32 = 11;
/// Кегль второстепенных строк плитки (автор, источник, статистика).
pub const CARD_META_POINT_SIZE: i32 = 10;

/// Зазор между плитками сетки.
pub const GRID_SPACING: i32 = 14;
/// Поля сетки по бокам: те же 16, что у шапки и строки состояния, — иначе
/// левый край плиток не совпадает с левым краем подписей над ними.
pub const GRID_MARGIN_H: i32 = 16;
/// Поля сетки сверху и снизу.
pub const GRID_MARGIN_V: i32 = 12;
/// Поля шапки каталога по бокам.
pub const HEADER_MARGIN_H: i32 = 16;
/// Поля шапки каталога сверху и снизу.
pub const HEADER_MARGIN_V: i32 = 12;

/// Ширина боковой панели категорий.
pub const SIDEBAR_W: i32 = 210;
/// Высота строки боковой панели.
pub const SIDEBAR_ROW_H: i32 = 40;
/// Ширина строки поиска.
pub const SEARCH_W: i32 = 240;

/// Размер картинки на панели деталей.
pub const DETAIL_PREVIEW_W: i32 = 600;
/// Высота картинки на панели деталей.
pub const DETAIL_PREVIEW_H: i32 = 340;

// ── Накладные кнопки поверх картинки деталей ───────────────────────────
/// Сторона круглой кнопки (лупа, стрелки галереи). Не меньше 40 —
/// требование Big Picture к размеру цели нажатия.
pub const OVERLAY_BTN: i32 = 44;
/// Отступ накладной кнопки от края картинки.
pub const OVERLAY_MARGIN: i32 = 10;
/// Высота бейджа-счётчика «2/5».
pub const COUNTER_H: i32 = 24;
/// Наименьшая ширина бейджа-счётчика.
pub const COUNTER_MIN_W: i32 = 48;
/// Поля внутри бейджа-счётчика (слева и справа вместе).
pub const COUNTER_PAD: i32 = 20;

/// Ширина бейджа-счётчика под текст такой-то ширины.
pub fn counter_width(text_w: i32) -> i32 {
    COUNTER_MIN_W.max(text_w + COUNTER_PAD)
}

// ── Выпадашка подсказок поиска ─────────────────────────────────────────
/// Высота строки подсказки.
pub const SUGGEST_ROW_H: i32 = 54;
/// Ширина миниатюры в подсказке (16:9).
pub const SUGGEST_THUMB_W: i32 = 72;
/// Высота миниатюры в подсказке.
pub const SUGGEST_THUMB_H: i32 = 40;
/// Сколько подсказок показываем самое большее.
pub const SUGGEST_MAX_ROWS: i32 = 12;
/// Наименьшая ширина выпадашки: миниатюра плюс две строки текста.
pub const SUGGEST_MIN_W: i32 = 360;
/// Зазор от правого края страницы, за который выпадашка не заходит.
pub const SUGGEST_EDGE: i32 = 12;
/// Наименьший отступ выпадашки от левого края страницы.
pub const SUGGEST_LEFT: i32 = 8;
/// Зазор между строкой поиска и выпадашкой.
pub const SUGGEST_GAP: i32 = 3;
/// Рамка выпадашки сверху и снизу вместе.
pub const SUGGEST_FRAME: i32 = 2;
/// По какой ширине обрезаются название и автор в строке подсказки.
pub const SUGGEST_TEXT_W: i32 = 300;
/// С какого числа введённых знаков вообще спрашиваем подсказки.
pub const SUGGEST_MIN_CHARS: usize = 2;

/// Ширина выпадашки подсказок: не уже строки поиска и не уже разумного
/// минимума.
pub fn suggest_width(anchor_w: i32) -> i32 {
    anchor_w.max(SUGGEST_MIN_W)
}

/// Высота выпадашки под столько-то строк.
pub fn suggest_height(rows: i32) -> i32 {
    rows.clamp(0, SUGGEST_MAX_ROWS) * SUGGEST_ROW_H + SUGGEST_FRAME
}

/// Левый край выпадашки: под строкой поиска, но не за краем страницы.
pub fn suggest_x(anchor_x: i32, width: i32, page_w: i32) -> i32 {
    let right_limit = page_w - SUGGEST_EDGE;
    if anchor_x + width > right_limit {
        SUGGEST_LEFT.max(right_limit - width)
    } else {
        anchor_x
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Паузы
// ═══════════════════════════════════════════════════════════════════════

/// Пауза набора перед запросом подсказок: посимвольно магазин не дёргаем.
pub const SUGGEST_DEBOUNCE_MS: u32 = 220;
/// Пауза перед сетевым запросом каталога: пока человек пробегает список
/// категорий мышью, сеть не трогаем.
pub const CATALOG_DEBOUNCE_MS: u32 = 200;
/// Пауза набора перед поиском в магазине: запрос уходит, когда человек
/// перестал печатать, а не на каждую букву (Enter — сразу).
pub const SEARCH_DEBOUNCE_MS: u32 = 300;
/// Через сколько после показа вкладки грузим первую категорию.
pub const FIRST_LOAD_MS: u32 = 120;
/// Через сколько начинается фоновый прогрев остальных категорий: он не
/// должен спорить за сеть с той категорией, что открыта.
pub const PREFETCH_START_MS: u32 = 900;
/// Через сколько чистим протухшие переводы (разово, не на горячем пути).
pub const PURGE_TRANSLATIONS_MS: u32 = 1500;
/// Пауза между фоновыми прогревами категорий — не долбить сервер.
pub const PREFETCH_PAUSE_MS: u32 = 450;
/// Пауза перед пересборкой сетки после применения темы: применение Global
/// Theme вызывает в нашем же процессе смену стиля Qt, и пересборка виджетов
/// прямо в этот момент валила приложение.
pub const STYLE_STORM_MS: u32 = 600;
/// Пауза перед фоновым прогревом галереи показанной сетки: сначала должны
/// доехать сами плитки, и категория должна «устояться».
pub const GALLERY_WARM_DELAY_MS: u32 = 800;

// ═══════════════════════════════════════════════════════════════════════
//  Загрузчик превью
// ═══════════════════════════════════════════════════════════════════════

/// Сколько картинок качаем одновременно.
pub const PREVIEW_MAX_INFLIGHT: usize = 6;
/// Сколько из них разрешено занять ФОНОВОМУ прогреву: живым плиткам всегда
/// остаётся место.
pub const PREVIEW_MAX_INFLIGHT_LOW: usize = 2;
/// Сколько добавочных попыток даём на временный сбой сети.
pub const PREVIEW_RETRIES: u32 = 2;
/// Пауза перед повтором (растёт с номером попытки).
pub const PREVIEW_RETRY_DELAY_MS: u32 = 700;
/// Больше этого объёма ответ считаем сбоем, а не картинкой.
pub const PREVIEW_MAX_BYTES: usize = 20 * 1024 * 1024;

/// Ответы сервера, при которых повторять бессмысленно: файла нет либо
/// доступ закрыт. Всё остальное (обрыв соединения, таймаут, 5xx) временно.
pub const PREVIEW_FATAL_HTTP: &[u16] = &[400, 401, 403, 404, 405, 410, 451];

/// Стоит ли повторять загрузку превью после такого ответа сервера.
pub fn preview_retry_allowed(status: Option<u16>, tries: u32) -> bool {
    if let Some(code) = status {
        if PREVIEW_FATAL_HTTP.contains(&code) {
            return false;
        }
    }
    tries <= PREVIEW_RETRIES
}

// ═══════════════════════════════════════════════════════════════════════
//  Сортировки каталога
// ═══════════════════════════════════════════════════════════════════════

/// Порядок сортировок в выпадающем списке: `(подпись, ключ OCS)`. Подпись —
/// `msgid`, переводится в месте показа ([`sort_label`]).
/// Умолчание — «Новые»: по рейтингу наверху годами стоят одни и те же
/// работы, и витрина выглядит мёртвой при каждом заходе.
pub const SORTS: &[(&str, &str)] = &[
    (n_("🆕 Newest"), "new"),
    (n_("⬇️ Most Downloaded"), "down"),
    (n_("⭐ Top Rated"), "high"),
    (n_("🔤 Alphabetical"), "alpha"),
];

/// Подпись сортировки из [`SORTS`] на языке интерфейса.
pub fn sort_label(label: &str) -> String {
    tr(label)
}

/// Сортировка по умолчанию.
pub const DEFAULT_SORT: &str = "new";

/// Место сортировки по умолчанию в списке (для выставления в комбобоксе).
pub fn default_sort_index() -> usize {
    SORTS
        .iter()
        .position(|(_, key)| *key == DEFAULT_SORT)
        .expect("сортировка по умолчанию обязана быть в таблице")
}

// ═══════════════════════════════════════════════════════════════════════
//  Числа для человека
// ═══════════════════════════════════════════════════════════════════════

/// Счётчик скачиваний/голосов в короткой записи: 1234 → «1.2K».
pub fn human_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Размер архива товара для человека: «13 КиБ», «1,4 МиБ».
///
/// ⚠️ Магазин отдаёт `downloadsize1` в КИБИБАЙТАХ, а не в байтах: живой
/// ответ 04.10.2026 — курсоры «Aemeath» `downloadsize1=13` при архиве в
/// 13 КиБ, «ba-click-fx» `1419` при 1,4 МиБ (так же их показывает Discover).
/// Пусто — размер неизвестен (0 или меньше).
pub fn human_size_kib(kib: i64) -> String {
    if kib <= 0 {
        return String::new();
    }
    // Десятичный разделитель — из перевода: у русского и немецкого он запятая.
    // TRANSLATORS: decimal separator in numbers like "1.4 MiB".
    let sep = trc("decimal separator", ".");
    let one_decimal = |v: f64| format!("{v:.1}").replace('.', &sep);
    let kib_f = kib as f64;
    if kib < 1024 {
        // TRANSLATORS: file size in kibibytes, %1 is the number.
        fill(&trc("file size", "%1 KiB"), &[&kib])
    } else if kib_f < 1024.0 * 1024.0 {
        // TRANSLATORS: file size in mebibytes, %1 is the number.
        fill(&trc("file size", "%1 MiB"), &[&one_decimal(kib_f / 1024.0)])
    } else {
        // TRANSLATORS: file size in gibibytes, %1 is the number.
        fill(&trc("file size", "%1 GiB"), &[&one_decimal(kib_f / (1024.0 * 1024.0))])
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Таблицы стилей
// ═══════════════════════════════════════════════════════════════════════

fn token<'a>(colors: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    colors
        .get(key)
        .map(String::as_str)
        .or_else(|| {
            ui_base::APPEARANCE_STEAM_COLORS
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v)
        })
        .unwrap_or("#ffffff")
}

/// Токены вкладки для действующего скина. Полный набор ключей гарантирован
/// ядром, поэтому подстановка в таблицу стилей никогда не разваливается.
pub fn colors() -> BTreeMap<String, String> {
    ui_base::active_skin_colors()
}

/// Таблица стилей всей вкладки, собранная из токенов скина `c`.
///
/// Для скинов `system`/`steam_dark` токены совпадают с исторической
/// Steam-палитрой, поэтому вид вкладки остаётся прежним.
pub fn page_qss(c: &BTreeMap<String, String>) -> String {
    let bg_primary = token(c, "bg_primary");
    let bg_secondary = token(c, "bg_secondary");
    let bg_card = token(c, "bg_card");
    let bg_card_hover = token(c, "bg_card_hover");
    let accent = token(c, "accent");
    let accent_green = token(c, "accent_green");
    let accent_green_hover = token(c, "accent_green_hover");
    let text_primary = token(c, "text_primary");
    let text_secondary = token(c, "text_secondary");
    let text_dim = token(c, "text_dim");
    let border = token(c, "border");
    let sb = token(c, "scrollbar");
    let rating_star = token(c, "rating_star");
    let author_self = token(c, "author_self");

    format!(
        r#"
#RightPane, #GridHost, #GridScroll, #TopBar, #Pagination, #DetailView,
#DetailHost {{
    background: {bg_primary};
}}
QScrollArea {{ background: {bg_primary}; border: none; }}

/* Скроллбар — заметный handle по умолчанию (не только на hover).
   Цвет — скин-зависимая RGB-основа c['scrollbar'] (белый на тёмных скинах,
   чёрный на светлом): 0.28 в покое → 0.45 hover → 0.60 pressed. */
QScrollBar:vertical {{
    background: transparent;
    width: 11px;
    margin: 0;
    border: none;
}}
QScrollBar::handle:vertical {{
    background: rgba({sb},0.28);
    border-radius: 5px;
    min-height: 28px;
}}
QScrollBar::handle:vertical:hover   {{ background: rgba({sb},0.45); }}
QScrollBar::handle:vertical:pressed {{ background: rgba({sb},0.60); }}
QScrollBar:horizontal {{
    background: transparent;
    height: 11px;
    margin: 0;
    border: none;
}}
QScrollBar::handle:horizontal {{
    background: rgba({sb},0.28);
    border-radius: 5px;
    min-width: 28px;
}}
QScrollBar::handle:horizontal:hover   {{ background: rgba({sb},0.45); }}
QScrollBar::handle:horizontal:pressed {{ background: rgba({sb},0.60); }}
QScrollBar::add-line, QScrollBar::sub-line {{
    width: 0; height: 0; background: none; border: none;
}}
QScrollBar::add-page, QScrollBar::sub-page {{ background: none; }}

#Sidebar {{
    background: {bg_secondary};
    border: none;
    outline: 0;
    color: {text_primary};
    font-size: 11pt;
    font-weight: 600;
}}
#Sidebar::item {{
    padding: 8px 4px;
    border-left: 3px solid transparent;
    color: {text_secondary};
}}
#Sidebar::item:selected {{
    background: {bg_card_hover};
    border-left: 3px solid {accent};
    color: {accent};
}}
#Sidebar::item:hover {{
    background: {bg_card};
    color: {text_primary};
}}

#PageTitle, #DetailName {{ color: {text_primary}; }}
#StatusBar {{ color: {text_secondary}; font-size: 10pt; font-weight: 600; }}
/* Объяснение «этого раздела в вашем рабочем столе нет» — вместо пустой
   сетки. Крупно, по центру, читаемо и в Big Picture (шрифт ≥ 14pt). */
#DeNote {{ color: {text_secondary}; font-size: 14pt; line-height: 150%; }}

#SrcLabel {{ color: {text_secondary}; padding-right: 2px; font-weight: 600; }}
#SrcChip {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 5px 10px;
    color: {text_secondary};
}}
#SrcChip:hover {{
    background: {bg_card};
    color: {text_primary};
}}
#SrcChip:checked {{
    background: {bg_card_hover};
    border: 1px solid {accent};
    color: {accent};
    font-weight: bold;
}}

#SearchBox {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 6px 8px;
    color: {text_primary};
}}
#SearchBox:focus {{ border: 1px solid {accent}; }}
#SortCombo {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 5px 8px;
    color: {text_primary};
    min-width: 130px;
}}
#SortCombo QAbstractItemView {{
    background: {bg_secondary};
    color: {text_primary};
    selection-background-color: {bg_card_hover};
}}
#RefreshBtn, #PageBtn, #BackBtn {{
    background: {bg_card};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 6px 12px;
    color: {text_primary};
}}
#RefreshBtn:hover, #PageBtn:hover, #BackBtn:hover {{
    background: {bg_card_hover};
    border: 1px solid {accent};
}}
/* Неактивная кнопка страницы — приглушённый ТОТ ЖЕ цвет, что у скроллбара.
   Раньше здесь стоял белый со своей прозрачностью, и на светлом скине
   надпись «← Назад» пропадала на светлом фоне. */
#PageBtn:disabled {{ color: rgba({sb},0.55); }}
#PageLabel {{ color: {text_secondary}; padding: 0 12px; font-weight: 600; }}

#ThemeCard {{
    background: {bg_card};
    border: 1px solid {border};
    border-radius: 4px;
}}
#ThemeCard:hover {{
    border: 1px solid {accent};
    background: {bg_card_hover};
}}
#ThemeCard QLabel {{ background: transparent; border: none; }}
#CardPreview {{
    background: {bg_secondary};
    border-top-left-radius: 4px;
    border-top-right-radius: 4px;
    color: {text_dim};
}}
#CardName {{ color: {text_primary}; }}
#CardAuthor {{ color: {text_primary}; font-size: 10pt; font-weight: 600; }}
#CardStats {{ color: {text_primary}; font-size: 10pt; font-weight: bold; }}
/* Значок оценки (★ у каталога оформления, 👍 у AUR) — единственное цветное
   пятно стат-строки. Раньше он был <span style='color:…'> внутри той же
   подписи: цвет запекался в текст при построении плитки и за сменой скина
   не шёл. Отдельная подпись со своим objectName красится таблицей стилей
   даром — тем же setStyleSheet, что и вся вкладка. Размер и насыщенность
   повторяют #CardStats: значок жил в её шрифте. */
#CardStar {{ color: {rating_star}; font-size: 10pt; font-weight: bold; }}

#InstallBtn, #InstallApplyBtn {{
    background: {accent_green};
    border: none;
    border-radius: 3px;
    padding: 6px 10px;
    color: white;
    font-weight: bold;
}}
#InstallBtn:hover, #InstallApplyBtn:hover {{ background: {accent_green_hover}; }}
/* «✓ Применено» — приглушённый серо-зелёный (товар уже установлен/применён).
   Остаётся кликабельной для переприменения; видна на тёмном фоне, но не
   кричит как ярко-зелёная. */
#InstallApplyBtn[applied="true"] {{
    background: #3a5a3f;
    color: #cfe8d2;
    font-weight: bold;
}}
#InstallApplyBtn[applied="true"]:hover {{ background: #46704c; }}
/* «✓ Установлено» на карточке каталога — НЕ заливка, а контур: залитая
   зелёная кнопка = предложение действия, контурная бирюзовая = состояние.
   Одного оттенка мало, «Установить» и «Установлено» на одинаковом зелёном
   сливались. */
#InstallBtn[installed="true"] {{
    background: transparent;
    border: 1px solid #4fd1c5;
    color: #7fe3da;
    font-weight: bold;
}}
#InstallBtn[installed="true"]:hover {{
    background: rgba(79, 209, 197, 0.16);
    border: 1px solid #7fe3da;
    color: #a8efe8;
}}
/* «✓ Применено» на карточке каталога — ЗАЛИТАЯ бирюзовая. Третье состояние
   должно отличаться от обоих соседних, поэтому меняются оба признака сразу:
   заливка (против контура «установлено») и оттенок (против зелёной кнопки
   «Установить»). Рамка того же цвета, что фон, — чтобы кнопка не «прыгала»
   в размере при переключении с контурного вида. */
#InstallBtn[installed="applied"] {{
    background: #2f9e94;
    border: 1px solid #2f9e94;
    color: #ffffff;
    font-weight: bold;
}}
#InstallBtn[installed="applied"]:hover {{
    background: #3ab8ac;
    border: 1px solid #4fd1c5;
    color: #ffffff;
}}
/* Деталь: «✓ Установлено» — тот же контурный вид, что у карточки, чтобы
   сетка и деталь не рассказывали про товар разное. */
#InstallApplyBtn[applied="installed"] {{
    background: transparent;
    border: 1px solid #4fd1c5;
    color: #7fe3da;
    font-weight: bold;
}}
#InstallApplyBtn[applied="installed"]:hover {{
    background: rgba(79, 209, 197, 0.16);
    border: 1px solid #7fe3da;
    color: #a8efe8;
}}
#DetailsBtn, #DownloadOnlyBtn, #CommentsMoreBtn {{
    background: {bg_card_hover};
    border: 1px solid {border};
    border-radius: 3px;
    padding: 6px 10px;
    color: {text_primary};
}}
#DetailsBtn:hover, #DownloadOnlyBtn:hover, #CommentsMoreBtn:hover {{
    border: 1px solid {accent};
}}
/* Та же починка, что у «← Назад»: приглушение от цвета скина, а не белым. */
#CommentsMoreBtn:disabled {{ color: rgba({sb},0.55); }}
#UninstallBtn {{
    background: #5a2230;
    border: 1px solid #803040;
    border-radius: 3px;
    padding: 6px 12px;
    color: #ffc0cb;
}}
#UninstallBtn:hover {{ background: #803040; }}

#DetailView QLabel {{ background: transparent; }}
#DetailPreview {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    color: {text_dim};
}}
#DetailDesc {{ color: {text_primary}; font-size: 11pt; }}
/* Мета-строка товара («автор • лицензия • обновлено»), счётчики и строка
   «Категория / путь установки» тоже красятся отсюда, а не собираются
   HTML-ом в момент показа: иначе смена скина оставляла УЖЕ ОТКРЫТУЮ
   деталь в цветах прошлого скина, пока окно вокруг было перекрашено. */
#DetailMeta {{ color: {text_secondary}; }}
#DetailStars {{ color: {rating_star}; font-size: 14pt; }}
#DetailDownloads {{ color: {text_secondary}; }}
#DetailPath {{ color: {text_dim}; font-weight: 600; font-size: 10.5pt; }}
#CommentsHeader {{ color: {text_primary}; margin-top: 10px; }}
#CommentsHost {{ background: transparent; }}
#CommentItem {{ color: {text_primary}; font-size: 10.5pt; }}
/* Ник и дата комментария. Ник САМОГО автора товара — ярко (author_self):
   признак ставится динамическим свойством, чтобы правило жило в таблице
   стилей, а не в тексте подписи. */
#CommentAuthor {{ color: {text_primary}; font-size: 10.5pt; font-weight: bold; }}
#CommentAuthor[own="true"] {{ color: {author_self}; }}
#CommentDate {{ color: {text_dim}; font-size: 9.5pt; font-weight: 600; }}
#CommentsEmpty {{ color: {text_dim}; font-weight: 600; }}
#DetailProgress {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 3px;
    text-align: center;
    color: {text_primary};
}}
#DetailProgress::chunk {{ background: {accent}; border-radius: 3px; }}
"#
    )
}

/// Таблица стилей выпадашки подсказок поиска.
///
/// Раньше набор цветов здесь был свой и запечён под Steam: на светлом скине
/// посреди светлой страницы разворачивалась тёмно-синяя панель, а серо-синий
/// автор нарушал правило «на тёмном фоне только белый». Теперь панель
/// собирается из тех же токенов, что и вся вкладка.
pub fn suggestions_qss(c: &BTreeMap<String, String>) -> String {
    let bg_secondary = token(c, "bg_secondary");
    let bg_card = token(c, "bg_card");
    let bg_card_hover = token(c, "bg_card_hover");
    let accent = token(c, "accent");
    let text_primary = token(c, "text_primary");
    let text_dim = token(c, "text_dim");
    format!(
        "#SuggestPopup {{ background:{bg_secondary};\
 border:1px solid {accent}; border-radius:6px; }}\
#SuggestList {{ background:transparent; border:none;\
 outline:none; }}\
#SuggestList::item {{ border:none; padding:0px;\
 border-radius:4px; }}\
#SuggestList::item:selected, #SuggestList::item:hover {{\
 background:{bg_card_hover}; }}\
#SuggTitle {{ color:{text_primary}; font-weight:700; background:transparent; }}\
#SuggAuthor {{ color:{text_dim}; background:transparent; }}\
#SuggThumb {{ background:{bg_card};\
 border-radius:3px; color:{accent}; }}"
    )
}

/// Накладная кнопка поверх картинки: круглая, полупрозрачно-чёрная.
///
/// Цвет здесь намеренно НЕ скин-зависимый: кнопка лежит на фотографии, а не
/// на фоне страницы, и читается на любой картинке только затемнением.
/// `point_size` отличает лупу (крупная) от стрелок галереи.
pub fn image_overlay_qss(point_size: f32) -> String {
    let radius = OVERLAY_BTN / 2;
    format!(
        "QPushButton {{ background-color: rgba(0,0,0,0.55); \
color: white; border: none; border-radius: {radius}px; \
font-size: {point_size}pt; }}\
QPushButton:hover, QPushButton:focus \
{{ background-color: rgba(0,0,0,0.85); }}"
    )
}

/// Кегль лупы «увеличить на весь экран».
pub const OVERLAY_ZOOM_PT: f32 = 20.0;
/// Кегль стрелок галереи.
pub const OVERLAY_NAV_PT: f32 = 16.0;

/// Таблица стилей бейджа-счётчика «2/5» поверх картинки.
pub fn gallery_counter_qss() -> String {
    let radius = COUNTER_H / 2;
    format!(
        "QLabel {{ background-color: rgba(0,0,0,0.55); color: white; \
border: none; border-radius: {radius}px; \
font-size: 10.5pt; font-weight: 600; }}"
    )
}

// ═══════════════════════════════════════════════════════════════════════
//  Журнал установленного: «установлена» и «применена» — разные состояния
// ═══════════════════════════════════════════════════════════════════════

/// `rexon_type` записи журнала → `apply_kind` активной темы: обратная сторона
/// `appearance_installer::kind_to_rexon_type`. По типу из журнала узнаём, в
/// какой строке карты активных тем искать имя товара.
///
/// ⚠️ СОВМЕСТИМОСТЬ СО СТАРЫМИ ЗАПИСЯМИ. До 2026-08-22 ядро сваливало обои,
/// звуки, шрифты и виджеты в один тип «wallpaper-pack», а заставку, экран
/// блокировки, GTK и стиль приложений — в «plugin». У покупателя такие записи
/// в журнале уже лежат, и ломать их нельзя. Поэтому обе бывшие свалки здесь
/// НЕ упомянуты вовсе: по ним вид определить нельзя, и запись честно уходит
/// на второй заход — по OCS-категории (`category_id` в журнале есть всегда).
pub fn rexon_type_to_apply_kind(rexon_type: &str) -> &'static str {
    match rexon_type.trim() {
        "theme-kde-global" => "global",
        "theme-kde-plasma" => "plasma",
        "theme-kde-colors" => "colors",
        "theme-kde-aurorae" => "aurorae",
        "theme-icons" => "icons",
        "theme-cursors" => "cursor",
        "theme-kvantum" => "kvantum",
        "theme-sddm" => "sddm",
        "theme-plymouth" => "plymouth",
        // Виды, у которых с 2026-08-22 есть свой тип вместо свалки «plugin».
        "theme-gtk" => "gtk",
        "theme-widget-style" => "widgetstyle",
        "theme-kde-splash" => "splash",
        "theme-kde-lockscreen" => "lockscreen",
        // Звук — полноценный вид: у него есть «применена сейчас».
        "theme-sounds" => "sounds",
        // Шрифты, виджеты и плагины обоев тоже получили свои типы; применения
        // у них нет, но по типу теперь видно, что это именно они, а не обои.
        "font-pack" => "fonts",
        "theme-plasmoid" => "plasmoids",
        "kde-wallpaper-plugin" => "wallpaper_plugins",
        _ => "",
    }
}

/// Имена тем записи журнала — basename каждого установленного пути.
///
/// Цветовая схема лежит файлом «<Имя>.colors», а в карте активных тем зовётся
/// без расширения — снимаем суффикс, иначе схема никогда не совпадёт.
pub fn log_theme_names(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let trimmed = p.trim_end_matches('/');
        let base = trimmed.rsplit('/').next().unwrap_or("");
        let base = if base.to_ascii_lowercase().ends_with(".colors") {
            &base[..base.len() - ".colors".len()]
        } else {
            base
        };
        if !base.is_empty() {
            out.push(base.to_string());
        }
    }
    out
}

/// `apply_kind` записи журнала («» — понятия «применена» у вида нет).
///
/// Сперва по `rexon_type`, затем по OCS-категории. Второй заход обязателен и
/// для старых записей (обе бывшие свалки типов), и для «wallpaper-pack»,
/// которым помечены и обычные обои, и фон экрана блокировки.
///
/// Есть ли у вида понятие «применена сейчас», решает
/// `appearance_installer::kind_has_active_state` — список рядом с картой
/// активных тем, а не второй такой же список во вкладке (правило #12).
pub fn log_apply_kind(home: &Path, rec: &Map<String, Value>, item_category_id: i64) -> String {
    let rexon_type = rec.get("rexon_type").and_then(Value::as_str).unwrap_or("");
    let mut kind = rexon_type_to_apply_kind(rexon_type).to_string();
    if kind.is_empty() {
        // Ноль в записи — не «категория 0», а «поля нет»: тогда берём
        // категорию с самой карточки, как делал оригинал.
        let mut cid = rec.get("category_id").and_then(Value::as_i64).unwrap_or(0);
        if cid == 0 {
            cid = item_category_id;
        }
        kind = categories::category_by_id(home, cid)
            .map(|c| c.apply_kind.trim().to_string())
            .unwrap_or_default();
    }
    if kind.is_empty() || !appearance_installer::kind_has_active_state(&kind) {
        return String::new();
    }
    kind
}

/// Что делать с картой активных тем при штамповке пачки карточек.
///
/// Карту тянет `installed_scan`, а он дёргает `kreadconfig6` — процесс. Зовём
/// его ТОЛЬКО когда в журнале реально нашёлся применимый товар, и ровно один
/// раз на пачку. Вне KDE утилиты может не быть вовсе: сетка не должна ни
/// падать, ни ждать процесс на каждую карточку.
pub struct LazyActiveMap<F: FnMut() -> Map<String, Value>> {
    fetch: F,
    cached: Option<Map<String, Value>>,
}

impl<F: FnMut() -> Map<String, Value>> LazyActiveMap<F> {
    pub fn new(fetch: F) -> Self {
        Self { fetch, cached: None }
    }

    /// Карта активных тем; первый вызов её и добывает.
    pub fn get(&mut self) -> &Map<String, Value> {
        if self.cached.is_none() {
            self.cached = Some((self.fetch)());
        }
        self.cached.as_ref().expect("карта только что заполнена")
    }

    /// Дёргали ли источник вообще (для тестов и замеров).
    pub fn was_fetched(&self) -> bool {
        self.cached.is_some()
    }
}

/// Проставить пачке OCS-карточек `installed` и `applied` по журналу.
///
/// Журнал установленных OCS-тем (`~/.config/rexon/ocs_installed.json`,
/// ключ = `content_id` = `item.id`) — единственный источник правды о том,
/// установлен ли товар: при загрузке каталога OCS возвращает `installed=false`,
/// и без этой сверки кнопка после перезапуска снова показывала «Установить и
/// применить».
///
/// `applied` — ОТДЕЛЬНОЕ состояние: тему можно скачать кнопкой «Только
/// скачать» и не включать. «Применена» она, только если её имя совпало с
/// активной темой своего вида.
pub fn stamp_installed_from_log<F: FnMut() -> Map<String, Value>>(
    home: &Path,
    log: &Map<String, Value>,
    items: &mut [ThemeItem],
    active: &mut LazyActiveMap<F>,
) {
    if items.is_empty() || log.is_empty() {
        return;
    }
    for it in items.iter_mut() {
        let Some(rec) = log.get(&it.id) else { continue };
        it.installed = true;
        it.applied = false;
        let Some(rec) = rec.as_object() else { continue };
        let kind = log_apply_kind(home, rec, it.category_id);
        if kind.is_empty() {
            continue; // вид без понятия «применён» (шрифты, виджеты, плагины)
        }
        let cur = active
            .get()
            .get(&kind)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if cur.is_empty() {
            continue; // вид оформления в этом окружении не определяется
        }
        let paths: Vec<String> = rec
            .get("paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        it.applied = log_theme_names(&paths)
            .iter()
            .any(|n| n.trim().to_lowercase() == cur);
    }
}

/// Та же штамповка, но журнал читается с диска по пути `log_path`.
///
/// Нет файла, битый JSON, не-объект наверху — журнал пуст и карточки не
/// трогаются (`ocs_store::parse_installed_log`). Пустая пачка файл не читает.
pub fn stamp_installed_from_log_file<F: FnMut() -> Map<String, Value>>(
    home: &Path,
    log_path: &str,
    items: &mut [ThemeItem],
    active: &mut LazyActiveMap<F>,
) {
    if items.is_empty() {
        return;
    }
    // Читается через общий журнал установки (шаг 3 слияния установщиков).
    let log = crate::install_journal::store_log_view(log_path);
    stamp_installed_from_log(home, &log, items, active);
}

/// Итог установки на карточках одного товара.
///
/// Удалось — товар «установлен» сразу, даже если журнал записать не вышло
/// (тема на диске есть). «Применён» — только по факту: по журналу и карте
/// активных тем, а не по намерению «установить и применить». У видов без
/// применения ядро отвечает «применение не требуется», а применение могло и
/// не удаться — в обоих случаях товар только установлен. Не удалось —
/// прежнее состояние не трогаем.
pub fn mark_installed_from_log_file<F: FnMut() -> Map<String, Value>>(
    home: &Path,
    log_path: &str,
    items: &mut [ThemeItem],
    ok: bool,
    active: &mut LazyActiveMap<F>,
) {
    if !ok {
        return;
    }
    for it in items.iter_mut() {
        it.installed = true;
        it.applied = false;
    }
    stamp_installed_from_log_file(home, log_path, items, active);
}

/// Путь журнала установленных с учётом `$XDG_CONFIG_HOME` — тот же, по
/// которому его пишет установщик.
fn live_install_log_path(home: &str) -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    ocs_store::install_log_path_xdg(xdg.as_deref(), home)
}

/// Живая карта активных тем для штамповки (её добывает `installed_scan`).
///
/// Окружение — запущенное сейчас: без него в карте нет разделов окружения
/// (рамки Marco/xfwm4, оболочки), и их товар из магазина не становился
/// «применённым».
fn live_active_map() -> Map<String, Value> {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let de = crate::system::detect_current_de(
        &var("XDG_CURRENT_DESKTOP"),
        &var("DESKTOP_SESSION"),
        !var("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_default();
    installed_scan::LiveScan::new(Vec::new(), std::collections::HashMap::new())
        .with_de(&de)
        .active_map()
        .into_iter()
        .map(|(k, v)| (k, Value::from(v)))
        .collect()
}

/// Живая штамповка пачки OCS-карточек: журнал с диска, карта активных тем —
/// лениво и не больше одного раза на пачку. Её зовут все места окна, где
/// товары магазина уходят на экран: сетка, «Избранное», полные детали.
pub fn stamp_installed_live(home: &str, items: &mut [ThemeItem]) {
    let mut active = LazyActiveMap::new(live_active_map);
    stamp_installed_from_log_file(
        Path::new(home),
        &live_install_log_path(home),
        items,
        &mut active,
    );
}

/// Живой вариант [`mark_installed_from_log_file`] — итог установки.
pub fn mark_installed_live(home: &str, items: &mut [ThemeItem], ok: bool) {
    let mut active = LazyActiveMap::new(live_active_map);
    mark_installed_from_log_file(
        Path::new(home),
        &live_install_log_path(home),
        items,
        ok,
        &mut active,
    );
}


// ═══════════════════════════════════════════════════════════════════════
//  Загрузчик превью: очередь, приоритет, дедуп и повторы
// ═══════════════════════════════════════════════════════════════════════

/// Каталог кеша превью AUR-карточек — рядом с обычным, отдельной папкой.
pub fn aur_preview_dir(home: &str) -> String {
    format!("{}/aur_previews", appearance_cache::cache_dir(home))
}

/// Каталог нарисованных плейсхолдеров плиток местных тем.
///
/// Свой каталог, а не общий с превью магазина: там лежит скачанное, здесь —
/// нарисованное нами, и чистить их однажды придётся по-разному.
pub fn placeholder_dir(home: &str) -> String {
    format!("{}/local_placeholders", appearance_cache::cache_dir(home))
}

/// Файл плейсхолдера местной темы, у которой своей картинки на диске нет.
///
/// В ключ входит ВСЁ, от чего зависит кадр: вид темы, путь установки, имя,
/// подпись и отметка времени файла. Последнее не для порядка: у цветовой
/// схемы кадр рисуется по ЕЁ содержимому — свотчи настоящих цветов, — и
/// после правки схемы обязан перерисоваться, а не отдаться из кеша прежним.
pub fn placeholder_cache_path(
    home: &str,
    category_id: i64,
    install_path: &str,
    name: &str,
    author: &str,
    stamp: i64,
) -> String {
    // Разделитель — символ, которого не бывает ни в пути, ни в имени темы:
    // без него «имя A + подпись B» и «имя AB + пустая подпись» дали бы один
    // ключ и один кадр на две разные плитки.
    let key = [
        category_id.to_string().as_str(),
        install_path,
        name,
        author,
        stamp.to_string().as_str(),
    ]
    .join("\u{0}");
    format!(
        "{}/{}.png",
        placeholder_dir(home),
        crate::translator::sha1_hex(key.as_bytes())
    )
}

/// Хост, с которого приходят OpenGraph-карточки GitHub (превью AUR-пакетов).
pub const GITHUB_OPENGRAPH_HOST: &str = "opengraph.githubassets.com";

/// Локальный путь превью по URL.
///
/// Схема имён — общая с клиентом магазина (`ocs::preview_cache_name`), и
/// это принципиально: разойдись два правила хоть в выборе расширения — и
/// весь кеш превью считается пустым, картинки качаются заново поверх уже
/// лежащих рядом файлов. Оригинал держал во вкладке ВТОРУЮ копию этого
/// правила (свой sha1 и свой разбор расширения) — здесь копии нет
/// (правило #12).
///
/// OpenGraph-карточки GitHub (превью AUR) уходят в свой каталог и всегда
/// PNG: у их ссылок расширения в пути нет вовсе.
pub fn preview_cache_path(home: &str, url: &str) -> String {
    if ocs::url_netloc(url) == GITHUB_OPENGRAPH_HOST {
        return format!(
            "{}/{}.png",
            aur_preview_dir(home),
            crate::translator::sha1_hex(url.as_bytes())
        );
    }
    format!(
        "{}/{}",
        appearance_cache::preview_dir(home),
        ocs::preview_cache_name(url)
    )
}

/// Одна заявка на превью: чья карточка, какая ссылка и фоновая ли она.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewJob {
    pub content_id: String,
    pub url: String,
    /// Фоновый прогрев: такие не показываются и уступают живым плиткам.
    pub low: bool,
}

/// Что делать после неудачной загрузки превью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewRetry {
    /// Повторить через `delay_ms`; `attempt` — какая попытка была неудачной.
    Again { delay_ms: u32, attempt: u32 },
    /// Сдаться. `fatal` — сервер ответил «файла нет / доступ закрыт»: такое
    /// в журнал идёт спокойной строкой, а не предупреждением.
    GiveUp { fatal: bool, attempts: u32 },
}

/// Очередь загрузчика превью — всё, что он ЗНАЕТ, без единого сетевого вызова.
///
/// Qt-слой остаётся снаружи: он лишь спрашивает «что запускать», сообщает
/// «этот ответ пришёл» и отдаёт готовые пути наверх. Правила же — сколько
/// картинок тянуть разом, кого пропускать вперёд, что считать дублем и когда
/// повторять — живут здесь и покрыты приёмкой.
///
/// ⚠ Сбой запроса — НЕ «картинки нет»: images.pling.com периодически
/// закрывает соединение и роняет СРАЗУ всю пачку одновременных ответов.
/// Поэтому временные сбои повторяются, а окончательные (404/403/410) — нет.
#[derive(Debug, Default)]
pub struct PreviewQueue {
    queue: Vec<PreviewJob>,
    /// Один URL — одна загрузка: без дедупа повторный заход в категорию слал
    /// те же запросы заново и топил канал.
    wanted: BTreeSet<String>,
    /// URL → сделано попыток.
    tries: BTreeMap<String, u32>,
    inflight: usize,
    /// Сколько ФОНОВЫХ загрузок в полёте: фон не имеет права занять все слоты.
    low_inflight: usize,
    started: u64,
    /// Поколение: `cancel_all` его увеличивает, отложенные повторы прошлого
    /// поколения (категорию уже сменили) молча выбрасываются.
    gen: u64,
    /// Готовые превью придерживаются и отдаются по одному: декодирование
    /// шести JPEG за один проход цикла событий даёт заметный рывок.
    ready: VecDeque<(String, String)>,
}

impl PreviewQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Текущее поколение (его несут отложенные повторы).
    pub fn generation(&self) -> u64 {
        self.gen
    }

    /// Сколько загрузок запущено за всю жизнь очереди — этим меряют, не
    /// «утопил» ли прогрев канал.
    pub fn started(&self) -> u64 {
        self.started
    }

    pub fn inflight(&self) -> usize {
        self.inflight
    }

    pub fn low_inflight(&self) -> usize {
        self.low_inflight
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Одна картинка: попадание в кеш → сразу наверх (или молча), иначе в
    /// очередь.
    ///
    /// Дедуп держим только для ФОНОВЫХ заявок. Живую пропускать по тому же
    /// ключу нельзя: две карточки каталога изредка делят один URL, и тогда
    /// вторая осталась бы без картинки — её «готово» некому было бы послать.
    pub fn want(
        &mut self,
        content_id: &str,
        url: &str,
        low: bool,
        cached: Option<&str>,
        emit_cached: bool,
    ) {
        if url.is_empty() || (low && self.wanted.contains(url)) {
            return;
        }
        if let Some(local) = cached {
            if emit_cached && !low {
                self.ready.push_back((content_id.to_string(), local.to_string()));
            }
            return;
        }
        self.wanted.insert(url.to_string());
        self.push(content_id, url, low);
    }

    /// Положить в очередь с учётом приоритета: живые плитки перед фоном.
    ///
    /// Очередь держим «сначала все живые, потом фон», иначе полсотни фоновых
    /// кадров отодвигают плитки той сетки, на которую человек смотрит.
    fn push(&mut self, content_id: &str, url: &str, low: bool) {
        let job = PreviewJob {
            content_id: content_id.to_string(),
            url: url.to_string(),
            low,
        };
        if low {
            self.queue.push(job);
            return;
        }
        let pos = self.queue.iter().position(|j| j.low).unwrap_or(self.queue.len());
        self.queue.insert(pos, job);
    }

    /// Что запускать прямо сейчас. Фону отдаём не больше
    /// `PREVIEW_MAX_INFLIGHT_LOW` слотов из `PREVIEW_MAX_INFLIGHT`: остальные
    /// всегда свободны под живые карточки, иначе прогрев галереи занял бы
    /// канал целиком и «ускорение» обернулось бы тормозами сетки.
    pub fn pump(&mut self) -> Vec<PreviewJob> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.queue.len() && self.inflight < PREVIEW_MAX_INFLIGHT {
            if self.queue[i].low && self.low_inflight >= PREVIEW_MAX_INFLIGHT_LOW {
                i += 1;
                continue;
            }
            let job = self.queue.remove(i);
            self.started += 1;
            self.inflight += 1;
            if job.low {
                self.low_inflight += 1;
            }
            out.push(job);
        }
        out
    }

    /// Ответ по заявке пришёл (любой — удачный, сбойный, оборванный).
    ///
    /// Дедуп снимаем ЗДЕСЬ, а не при постановке в очередь: пока картинка
    /// летит, повторная просьба о ней не должна порождать второй запрос.
    pub fn finished(&mut self, url: &str, low: bool) {
        self.inflight = self.inflight.saturating_sub(1);
        if low {
            self.low_inflight = self.low_inflight.saturating_sub(1);
        }
        self.wanted.remove(url);
    }

    /// Тело ответа годится в картинку? Пустое и неправдоподобно большое —
    /// это сбой, а не превью, и его надо повторить.
    pub fn body_is_image(len: usize) -> bool {
        len > 0 && len <= PREVIEW_MAX_BYTES
    }

    /// Картинка записана в кеш: снять счётчик попыток и, если заявка живая,
    /// поставить её в выдачу.
    pub fn succeeded(&mut self, content_id: &str, url: &str, low: bool, path: &str) {
        self.tries.remove(url);
        if !low {
            self.ready.push_back((content_id.to_string(), path.to_string()));
        }
    }

    /// Сбой загрузки: повторить (временный) либо сдаться (постоянный).
    ///
    /// Обрыв соединения сервером кладёт ВСЮ пачку одновременных запросов —
    /// без повтора это `PREVIEW_MAX_INFLIGHT` карточек с вечной заглушкой.
    pub fn failed(&mut self, url: &str, status: Option<u16>) -> PreviewRetry {
        let attempt = self.tries.get(url).copied().unwrap_or(1);
        if !preview_retry_allowed(status, attempt) {
            self.tries.remove(url);
            let fatal = status.is_some_and(|c| PREVIEW_FATAL_HTTP.contains(&c));
            return PreviewRetry::GiveUp { fatal, attempts: attempt };
        }
        self.tries.insert(url.to_string(), attempt + 1);
        PreviewRetry::Again {
            delay_ms: PREVIEW_RETRY_DELAY_MS * attempt,
            attempt,
        }
    }

    /// Вернуть превью в очередь, если категорию с тех пор не сменили.
    /// `false` — заявка устарела либо ту же картинку уже просят заново.
    pub fn requeue(&mut self, gen: u64, content_id: &str, url: &str, low: bool) -> bool {
        if gen != self.gen || self.wanted.contains(url) {
            return false;
        }
        self.wanted.insert(url.to_string());
        self.push(content_id, url, low);
        true
    }

    /// Отменить всё активное и очередное (смена категории). Счётчики полёта
    /// снимут сами ответы: оборванный запрос всё равно вернётся в `finished`.
    pub fn cancel_all(&mut self) {
        self.gen += 1;
        self.queue.clear();
        self.ready.clear();
        self.tries.clear();
        self.wanted.clear();
    }

    /// Следующее готовое превью — по одному за проход цикла событий.
    pub fn take_ready(&mut self) -> Option<(String, String)> {
        self.ready.pop_front()
    }

    /// Есть ли что отдавать (Qt-слой держит таймер только пока есть).
    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }
}


// ═══════════════════════════════════════════════════════════════════════
//  Состояние каталога: что вкладка показывает и куда идёт за данными
// ═══════════════════════════════════════════════════════════════════════

/// Источник данных категории.
pub const SOURCE_OCS: &str = "ocs";
pub const SOURCE_AUR: &str = "aur";
pub const SOURCE_BOTH: &str = "both";
/// Cinnamon Spices — единственный магазин пяти разделов Cinnamon
/// (`crate::cinnamon_spices`).
pub const SOURCE_SPICES: &str = crate::cinnamon_spices::SOURCE_KEY;

/// Ключи источников, которые есть в разделе `category_key` (порядок чипов).
/// Чего здесь нет, того в разделе нет: выбор помнится при переходе между
/// разделами, но действует KDE Store ([`CatalogView::effective_source`]).
/// Снятые источники — «vsthemes» (04.10.2026) и «github» в «Курсорах»
/// (25.10.2026) — сюда не входят и тем же путём молча становятся KDE Store.
///
/// AUR («AUR» и «Все») — только там, где у AUR есть что показать
/// ([`crate::aur::has_category`]): иначе чип вёл бы в заведомо пустую полку.
pub fn source_keys_for(category_key: &str) -> Vec<&'static str> {
    // У разделов Cinnamon свой магазин, и он один: KDE Store и AUR
    // спайсов не держат.
    if crate::cinnamon_spices::type_for_section(category_key).is_some() {
        return vec![SOURCE_SPICES];
    }
    let mut keys = vec![SOURCE_OCS];
    if crate::aur::has_category(category_key) {
        keys.extend([SOURCE_AUR, SOURCE_BOTH]);
    }
    // Темы Spices несут и тему приложений, и рамки окон: в этих разделах
    // они — ещё один магазин, отобранный по составу темы
    // (`cinnamon_spices::PART_SECTIONS`).
    if crate::cinnamon_spices::is_part_section(category_key) {
        keys.push(SOURCE_SPICES);
    }
    keys
}

/// Режим категории «Экран блокировки»: полные темы либо обои блокировки.
pub const LOCK_MODE_THEMES: &str = "themes";
pub const LOCK_MODE_WALLPAPER: &str = "wallpaper";

/// Синтетическая категория «тема экрана блокировки» (apply_kind=lockscreen).
pub const LOCK_CAT_THEMES: i64 = 903;
/// Синтетическая категория «обои экрана блокировки» (apply_kind=lockwallpaper).
pub const LOCK_CAT_WALLPAPER: i64 = 904;

/// OCS-каталог полных тем блокировки (Global Themes).
pub const LOCK_STORE_THEMES: i64 = 722;
/// OCS-каталог обоев (фон экрана блокировки).
pub const LOCK_STORE_WALLPAPER: i64 = 299;

/// Наименьшая длина запроса для поиска по AUR.
pub const AUR_MIN_QUERY: usize = 2;

/// Состояние каталога — всё, из чего вкладка выводит свой вид и свои запросы.
///
/// Виджетов здесь нет: Qt-слой держит те же поля не у себя, а спрашивает их
/// у этого типа. Так «что показать» и «куда пойти за данными» перестают быть
/// двумя разными правдами.
#[derive(Debug, Clone)]
pub struct CatalogView {
    /// Открытый раздел сайдбара.
    pub category: ThemeCategory,
    /// Страница каталога, с нуля.
    pub page: i64,
    /// Сколько работ у категории всего (по ответу магазина).
    pub total: i64,
    /// Ключ сортировки, выбранный человеком.
    pub sort: String,
    /// Строка поиска.
    pub query: String,
    /// Источник: `ocs` | `aur` | `both`.
    pub source: String,
    /// Рабочий стол, под который отфильтрован каталог.
    pub de: String,
    /// Режим «Экрана блокировки».
    pub lock_mode: String,
    /// Фильтр сетки «Стиля приложений»: [`STYLE_FILTER_ALL`],
    /// [`STYLE_FILTER_QT`] или [`STYLE_FILTER_KVANTUM`]; `None` — все.
    pub style_sub: Option<String>,
    /// Фильтр тона цветовых схем и стилей Plasma, как выбрал человек
    /// (`local_preview::tone_filter` решает, какой действует); пусто — «Все».
    pub tone_filter: String,
    /// Чип «Запущенные» раздела «Виджеты Conky».
    pub running_only: bool,
    /// Под нами SteamOS (Steam Deck или Steam Machine): экран входа и
    /// заставку загрузки там не меняем, их разделы показывают причину
    /// вместо полки (`categories::steamos_block`).
    pub steamos: bool,
    /// Юнит активного экрана входа (`plasmalogin`, `sddm`, `lightdm`…) —
    /// для пояснения «сейчас его показывает …». Пусто — проба не ответила.
    pub login_manager: String,
    /// Программа Conky есть в системе (для подписи раздела «Виджеты Conky»).
    pub conky_installed: bool,
    /// Rexon может сам поставить пакет Conky на этой системе.
    pub conky_installable: bool,
    /// Сеанс Wayland — у Conky на Wayland своя оговорка.
    pub wayland: bool,
}

impl CatalogView {
    /// Стартовое состояние: раздел, сортировка «Новые», магазин KDE.
    ///
    /// Умолчание сортировки — «Новые»: по рейтингу наверху годами стоят одни
    /// и те же работы, и витрина при каждом заходе выглядит мёртвой.
    /// Умолчание режима блокировки — «Обои»: их тысячи, выбор мгновенный.
    pub fn new(category: ThemeCategory, de: &str) -> Self {
        Self {
            category,
            page: 0,
            total: 0,
            sort: DEFAULT_SORT.to_string(),
            query: String::new(),
            source: SOURCE_OCS.to_string(),
            de: de.to_string(),
            lock_mode: LOCK_MODE_WALLPAPER.to_string(),
            style_sub: None,
            tone_filter: String::new(),
            running_only: false,
            steamos: false,
            login_manager: String::new(),
            conky_installed: true,
            conky_installable: false,
            wayland: false,
        }
    }

    /// Режим «Экрана блокировки» с поправкой на окружение: где полных тем
    /// нет (Deepin), остаётся режим обоев, даже если раньше, под KDE, был
    /// выбран режим тем.
    pub fn lock_mode_effective(&self) -> &str {
        if self.lock_mode == LOCK_MODE_THEMES && !categories::lock_themes_supported(&self.de) {
            LOCK_MODE_WALLPAPER
        } else {
            &self.lock_mode
        }
    }

    /// Раздел закрыт на этой SteamOS (экран входа, заставка загрузки).
    pub fn steamos_blocked(&self) -> bool {
        self.steamos && categories::steamos_block(&self.category.key).is_some()
    }

    /// Раздел экрана входа при Plasma Login Manager — у него своя панель
    /// (объяснение и кнопка в настройки), а не полка магазина.
    pub fn is_plasma_login(&self) -> bool {
        self.category.key == categories::PLASMA_LOGIN_KEY
    }

    /// Раздел экрана входа (полка тем SDDM или панель Plasma Login
    /// Manager) — над ним блок «Менеджер входа». На SteamOS раздел закрыт
    /// целиком, и блока нет.
    pub fn is_login_screen(&self) -> bool {
        matches!(self.category.key.as_str(), "sddm" | categories::PLASMA_LOGIN_KEY) && !self.steamos_blocked()
    }

    /// Раздел «Панель MATE» — свой экран с раскладками, а не полка.
    pub fn is_mate_panel(&self) -> bool {
        self.category.key == categories::MATE_PANEL_KEY
    }

    /// Раздел настроек (анимация, шрифты, кнопки заголовка…) — свой экран
    /// с разделом вкладки «Настройки», а не полка
    /// (`categories::SETTINGS_PANELS`).
    pub fn is_settings_panel(&self) -> bool {
        categories::is_settings_panel(&self.category.key)
    }

    /// Объяснение вместо полки: раздела нет в этом рабочем столе либо он
    /// закрыт на SteamOS. `None` — раздел открыт.
    pub fn note(&self, home: &Path) -> Option<String> {
        categories::category_note(home, &self.category.key, &self.de)
            .or_else(|| categories::steamos_note(home, &self.category.key, self.steamos))
    }

    /// Какой экран показать после пересборки раздела: `note` —
    /// объяснение, `plasmalogin` — панель Plasma Login Manager, `grid` —
    /// витрина. Деталь товара сюда не входит: её открывает человек.
    pub fn screen(&self, home: &Path) -> &'static str {
        if self.note(home).is_some() {
            SCREEN_NOTE
        } else if self.is_plasma_login() {
            SCREEN_PLASMA_LOGIN
        } else if self.is_mate_panel() {
            SCREEN_MATE_PANEL
        } else if self.is_settings_panel() {
            SCREEN_SETTINGS
        } else {
            SCREEN_GRID
        }
    }

    /// Раздел — локальный обзор диска, категория без сетевого магазина либо
    /// раздел с сеткой установленного, пока магазин не открыт.
    pub fn is_local(&self) -> bool {
        categories::LOCAL_VIEW_KEYS.contains(&self.category.key.as_str())
            || self.category.local_only()
    }

    /// Есть ли у раздела сетка установленного с магазином за кнопкой
    /// «Загрузить новые…» — как страницы kcm KDE «Внешний вид»: любой
    /// раздел, у которого есть и скан диска, и магазин
    /// ([`has_installed_grid_for`]), плюс полные темы экрана блокировки.
    ///
    /// Раньше заставка и экран блокировки клеили установленное сверху
    /// первой страницы магазина. Сетка с кнопкой — один путь на все десять:
    /// установленного бывает больше страницы (значков в системе — десятки),
    /// и магазин под ним уезжал бы за край, а заголовок «Установлено» между
    /// двумя частями одной сетки не встанет.
    pub fn has_installed_view(&self) -> bool {
        let key = self.category.key.as_str();
        has_installed_grid_for(&self.category)
            || (key == "lockscreen" && self.lock_mode_effective() == LOCK_MODE_THEMES)
    }

    /// Показывает ли раздел установленное — сверху, над магазином раздела
    /// на той же странице. Кнопки «Загрузить новые…» больше нет (решение
    /// владельца 18.10.2026): магазин грузится сам.
    pub fn installed_view(&self) -> bool {
        self.has_installed_view()
    }

    /// Видна ли строка фильтра тона «Все / Светлые / Тёмные»
    /// (`local_preview::tone_bar_applies`).
    pub fn tone_bar(&self) -> bool {
        crate::local_preview::tone_bar_applies(&self.category.key, self.installed_view())
    }

    /// Действующий фильтр тона раздела (`local_preview::tone_filter`).
    pub fn active_tone_filter(&self) -> &'static str {
        crate::local_preview::tone_filter(&self.category.key, &self.tone_filter)
    }

    /// Сужает ли сетку установленного фильтр, под которым витрины магазина
    /// нет: показывается только то, что на диске.
    ///
    /// ОДНО правило на все такие фильтры раздела — «Светлые / Тёмные» у
    /// цветов и стилей Plasma (тон знаем только у тем с диска), «Qt-стили /
    /// Kvantum» у стиля приложений (фильтр сетки установленного, магазин
    /// раздела — темы Kvantum без разбора) и «Запущенные» у Conky (процесс
    /// бывает только у установленного). Под таким фильтром магазин не
    /// спрашивается (`request_key`), листалки, сортировки и «Магазин:» нет
    /// (`chrome`), поиск — среди установленного (`search_placeholder`).
    /// Новый фильтр сетки установленного добавляется СЮДА, и всё остальное
    /// он получает сам. До 01.11.2026 правило жило только у фильтра тона, и
    /// под «Kvantum» оставались «Найдено 337» и листалка «1 / 17».
    pub fn installed_only(&self) -> bool {
        if !self.installed_view() {
            return false;
        }
        let tone = self.tone_bar() && self.active_tone_filter() != crate::local_preview::TONE_ALL;
        let style = self.category.key == "widgetstyle" && self.style_filter() != STYLE_FILTER_ALL;
        let running = self.category.key == "conky" && self.running_only;
        tone || style || running
    }

    /// Выделять ли текущую тему рамкой и жирной подписью (как KDE): в сетке
    /// установленного и в разделах окружений. В обзорах «Активные» и прочих
    /// включённых много сразу — там рамка ничего не отличала бы.
    pub fn marks_current(&self) -> bool {
        self.installed_view()
            || (self.is_local() && local_scan_plan(&self.local_scan_which()).0 == "section")
    }

    /// Применим ли переключатель «Магазин:» к текущей категории.
    ///
    /// Локальные обзоры диска сети не знают вовсе. Переключатель есть там,
    /// где каталог из OCS и кроме него есть из чего выбрать
    /// ([`source_keys_for`]): у заставки (зонт 716+488), экрана блокировки,
    /// переключателя окон, Conky и рамок окружений пакетов в AUR нет, и
    /// чип «AUR» вёл бы в заведомо пустую полку.
    pub fn source_applicable(&self) -> bool {
        let ocs = matches!(self.category.store, CatalogSource::Ocs(_) | CatalogSource::OcsShowcase);
        !self.is_local() && ocs && source_keys_for(&self.category.key).len() > 1
    }

    /// Активен ли чистый AUR-источник.
    pub fn aur_mode(&self) -> bool {
        self.source_applicable() && self.source == SOURCE_AUR
    }

    /// Активен ли смешанный источник «Все».
    pub fn both_mode(&self) -> bool {
        self.source_applicable() && self.source == SOURCE_BOTH
    }

    /// Источник, который действует в этом разделе. Выбор, которого в
    /// разделе нет (или которого больше нет вовсе, как снятых «vsthemes» и
    /// «github»),
    /// помнится, но действует первый источник раздела (KDE Store, у
    /// разделов Cinnamon — Cinnamon Spices).
    pub fn effective_source(&self) -> &str {
        let keys = source_keys_for(&self.category.key);
        if keys.contains(&self.source.as_str()) {
            &self.source
        } else {
            keys.first().copied().unwrap_or(SOURCE_OCS)
        }
    }

    /// Магазин раздела — Cinnamon Spices: вид каталога сайта (`applets`,
    /// `themes`…). `None` — у раздела другой магазин либо он без сети.
    pub fn spices_json(&self) -> Option<&str> {
        match &self.category.store {
            CatalogSource::Spices(json) if !self.is_local() => Some(json.as_str()),
            // «Темы GTK» и рамки окон: выбран чип Cinnamon Spices — витрина
            // тем сайта, отобранных по составу.
            _ if self.spices_parts_mode() => crate::cinnamon_spices::type_for_key("theme").map(|t| t.json),
            _ => None,
        }
    }

    /// Открыты ли темы Cinnamon Spices в разделе GTK или рамок окон
    /// (`cinnamon_spices::PART_SECTIONS`): там это не единственный магазин,
    /// а чип рядом с KDE Store.
    pub fn spices_parts_mode(&self) -> bool {
        self.source_applicable()
            && self.source == SOURCE_SPICES
            && crate::cinnamon_spices::is_part_section(&self.category.key)
    }

    /// Открыт ли магазин Cinnamon Spices.
    pub fn spices_mode(&self) -> bool {
        self.spices_json().is_some()
    }

    /// Раздела нет в выбранном рабочем столе.
    pub fn de_blocked(&self) -> bool {
        !categories::category_available(&self.category.key, &self.de)
    }

    /// `cat_id` (или зонт «id1,id2») для запроса каталога.
    ///
    /// Строка, а не число: OCS принимает несколько категорий через запятую, и
    /// категории-зонты этим пользуются.
    pub fn query_category_id(&self) -> String {
        if self.category.key == "lockscreen" {
            return if self.lock_mode_effective() == LOCK_MODE_THEMES {
                LOCK_STORE_THEMES.to_string()
            } else {
                LOCK_STORE_WALLPAPER.to_string()
            };
        }
        store_query(&self.category, &self.de)
    }

    /// Сортировка, которая реально уйдёт в запрос за эту категорию.
    ///
    /// У «Популярных» и «Новых» она закреплена; для обычных — выбор человека.
    /// Тем же способом её считает фоновый прогрев — иначе ключ кеша не сойдётся
    /// и прогретая страница откроется заново через сеть.
    pub fn sort_for(&self, cat: &ThemeCategory) -> String {
        match cat.key.as_str() {
            "new" => "new".to_string(),
            "popular" => "high".to_string(),
            _ => self.sort.clone(),
        }
    }

    /// Сортировка для открытого раздела.
    pub fn active_sort(&self) -> String {
        self.sort_for(&self.category)
    }

    /// Ключ сессионного кеша AUR: список категории — по её ключу, поиск — по
    /// запросу. Тот же ключ несёт ответ воркера, и по нему отбрасываются
    /// ответы категорий, с которых человек уже ушёл.
    pub fn aur_cache_key(&self) -> String {
        if self.query.is_empty() {
            format!("cat:{}", self.category.key)
        } else {
            format!("search:{}", self.query)
        }
    }

    /// Что показывать в шапке вкладки при текущем состоянии.
    pub fn chrome(&self) -> Chrome {
        if self.de_blocked() || self.steamos_blocked() || self.is_plasma_login() || self.is_mate_panel()
            || self.is_settings_panel()
        {
            // Раздела в этом рабочем столе нет (или он закрыт на SteamOS, или
            // у экрана входа нет тем вовсе): магазин, сортировка и страницы не
            // нужны — показывать нечего, и предлагать «искать» здесь нечестно.
            // У Plasma Login Manager остаётся пояснение «общий для всех
            // рабочих столов»: в MATE иначе непонятно, почему здесь Plasma.
            let caveat = if self.is_plasma_login() && !self.steamos_blocked() {
                self.caveat()
            } else {
                None
            };
            return Chrome { caveat, ..Chrome::default() };
        }
        let local = self.is_local();
        // Под фильтром «только установленное» магазина нет — и его
        // переключателя, сортировки и страниц тоже (`installed_only`).
        let store = !self.installed_only();
        Chrome {
            search: true,
            search_placeholder: self.search_placeholder(),
            caveat: self.caveat(),
            source_bar: self.source_applicable() && store,
            // Фильтр «Все / Qt-стили / Kvantum» — над сеткой установленного;
            // в магазине раздела (темы Kvantum) фильтровать нечего.
            style_bar: self.category.key == "widgetstyle" && self.installed_view(),
            lock_bar: self.category.key == "lockscreen",
            cursor_size_bar: self.category.key == "cursor",
            sort_combo: !local && !self.aur_mode() && store,
            pagination: !local && store,
            install_file: crate::local_install::button_for(
                &self.category.key,
                self.category.virtual_,
            ),
        }
    }

    /// Подсказка в поле поиска. Под фильтром «только установленное»
    /// ([`Self::installed_only`]) магазина (и AUR, и Spices) нет, ищется
    /// только среди установленного — подсказка про выбранный магазин была
    /// бы неправдой.
    pub fn search_placeholder(&self) -> String {
        if self.installed_only() {
            tr("🔍 Search installed…")
        } else if self.aur_mode() {
            tr("🔍 Search AUR…")
        } else if self.spices_mode() {
            tr("🔍 Search Cinnamon Spices…")
        } else {
            tr("🔍 Search themes…")
        }
    }

    /// Подпись-условие под заголовком раздела: у экрана входа и заставки
    /// загрузки — что они общие для всех рабочих столов
    /// (`categories::system_section_note`), у остальных — условие матрицы.
    fn caveat(&self) -> Option<String> {
        let key = self.category.key.as_str();
        let conky = (key == "conky").then(|| {
            crate::de_apply::conky_section_note(
                self.conky_installed,
                self.conky_installable,
                &self.de,
                self.wayland,
            )
        });
        let parts: Vec<String> = [
            categories::system_section_note(key, &self.login_manager),
            categories::category_condition(key, &self.de),
            conky.flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    /// Какой скан диска запускать для местной категории.
    ///
    /// «Стиль приложений» несёт фильтр сетки (`widgetstyle:<фильтр>`):
    /// стили Qt и темы Kvantum вместе, только стили Qt или только Kvantum.
    pub fn local_scan_which(&self) -> String {
        if self.category.key != "widgetstyle" {
            return self.category.key.clone();
        }
        format!("widgetstyle:{}", self.style_filter())
    }

    /// Действующий фильтр «Стиля приложений»: незнакомое значение (в том
    /// числе прежний под-выбор `qt:<Имя>`) — «Все».
    pub fn style_filter(&self) -> &'static str {
        match self.style_sub.as_deref() {
            Some(STYLE_FILTER_QT) => STYLE_FILTER_QT,
            Some(STYLE_FILTER_KVANTUM) => STYLE_FILTER_KVANTUM,
            _ => STYLE_FILTER_ALL,
        }
    }

    /// Принять то, что человек набрал в строке поиска.
    ///
    /// ⚠️ ЗДЕСЬ ЖИЛА ДЫРА (живая проверка 06.09.2026). Ввод непустого слова
    /// в запрос НЕ попадал вовсе: страница умела только СБРАСЫВАТЬ поиск,
    /// когда поле опустело, а набранное слово молча теряла. Запрос уходил в
    /// магазин и в скан диска пустым, и ни один раздел не фильтровался —
    /// в «Активных» на слово «bree» оставались все пять тем. Оригинал на
    /// виджетах записывает набранное по Enter (`_on_search`), и вся разница
    /// между ним и нами была в этой пропущенной записи.
    ///
    /// Возвращает, что вкладке делать дальше: сама она про поиск больше
    /// ничего не решает, и оттого правило одно на ВСЕ разделы страницы —
    /// магазин, AUR и обзоры диска ходят за данными одним и тем же
    /// `request_key` с одним и тем же `query`.
    pub fn edit_query(&mut self, text: &str) -> QueryEdit {
        let text = text.trim();
        if text.is_empty() {
            // Опустевшее поле сбрасывает поиск СРАЗУ: иначе запрос залипал
            // на прошлом слове при визуально пустом поле (починка оригинала).
            if self.query.is_empty() {
                return QueryEdit::Idle;
            }
            self.query.clear();
            self.page = 0;
            return QueryEdit::Cleared;
        }
        if self.query == text {
            return QueryEdit::Idle;
        }
        // Набранное запоминаем, но за данными не идём: посимвольно магазин
        // не дёргаем — поиск применяет Enter (`submit_query`).
        //
        // Разница с оригиналом намеренная: там набранное лежит только в
        // поле ввода и попадает в запрос по Enter, поэтому «Обновить» или
        // смена сортировки при набранном, но не отправленном слове
        // перечитывали раздел БЕЗ него — на экране слово есть, в витрине
        // его не видно. Здесь запрос всегда тот, что человек видит в поле.
        self.query = text.to_string();
        QueryEdit::Kept
    }

    /// Через сколько применять набранное слово, пока человек печатает.
    ///
    /// Обзор диска (и сетка под фильтром «только установленное») ищет по
    /// уже прочитанному списку — без сети, поэтому сразу (0). Магазин и
    /// раздел «установленное + магазин» (одним запросом) — после паузы
    /// [`SEARCH_DEBOUNCE_MS`]: посимвольно магазин не дёргаем. Enter
    /// применяет сразу при любом виде раздела.
    pub fn search_delay_ms(&self) -> u32 {
        if self.is_local() || self.installed_only() {
            0
        } else {
            SEARCH_DEBOUNCE_MS
        }
    }

    /// Ключ запроса за составом раздела: им же сверяется ответ, и чужой
    /// отбрасывается.
    ///
    /// ⚠️ Запрос входит в ключ у ВСЕХ видов раздела, а не только у магазина.
    /// У обзоров диска ключ был `local:<вид>` без запроса, и два разных
    /// запроса за одним разделом выглядели для дедупа одинаковыми: набранное
    /// слово не отправлялось вовсе, а в сетку принимался ответ ещё летящего
    /// НЕотфильтрованного скана.
    pub fn request_key(&self) -> String {
        // Под фильтром «только установленное» магазин не спрашивается —
        // ключ свой, чтобы не слиться с летящим запросом «установленное +
        // магазин» того же раздела.
        if self.installed_only() {
            return format!("installed:{}:{}", self.local_scan_which(), self.query);
        }
        // Установленное и магазин одним запросом: ключ несёт обе части.
        if self.installed_view() {
            return format!("combined:{}:{}", self.local_scan_which(), self.store_request_key());
        }
        self.store_request_key()
    }

    /// Ключ без части установленного: обзор диска либо страница магазина.
    fn store_request_key(&self) -> String {
        if self.is_local() {
            return format!("local:{}:{}", self.local_scan_which(), self.query);
        }
        if self.aur_mode() {
            return format!("aur:{}", self.aur_cache_key());
        }
        if let Some(json) = self.spices_json() {
            // Темы по составу — своя витрина у каждого раздела.
            let json = if self.spices_parts_mode() { format!("{json}@{}", self.category.key) } else { json.to_string() };
            return format!("spices:{json}:{}:{}:{}", self.page, self.active_sort(), self.query);
        }
        format!(
            "ocs:{}:{}:{}:{}",
            self.query_category_id(),
            self.page,
            self.active_sort(),
            self.query
        )
    }
}

/// Что делать вкладке после правки строки поиска.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryEdit {
    /// Ничего не изменилось — трогать нечего.
    Idle,
    /// Набранное принято, раздел перечитывать рано: поиск применяет Enter.
    Kept,
    /// Поле опустело — запрос сброшен, раздел перечитать сразу.
    Cleared,
}

/// Видимость и подписи шапки каталога.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chrome {
    pub search: bool,
    /// Подсказка в строке поиска (пусто, когда строки нет).
    pub search_placeholder: String,
    /// «Работает, но при условии» — подпись под заголовком раздела. Раздел при
    /// этом остаётся на месте: прятать работающее вместо того, чтобы назвать
    /// условие, — не наш способ (правило #13).
    pub caveat: Option<String>,
    pub source_bar: bool,
    pub style_bar: bool,
    pub lock_bar: bool,
    /// Чипы размера курсора — в разделе «Курсоры».
    pub cursor_size_bar: bool,
    pub sort_combo: bool,
    pub pagination: bool,
    /// Кнопка «Установить из файла…» (`crate::local_install::button_for`).
    pub install_file: bool,
}

// ── Экраны раздела ─────────────────────────────────────────────────────

/// Витрина плиток.
pub const SCREEN_GRID: &str = "grid";
/// Объяснение, почему раздела здесь нет.
pub const SCREEN_NOTE: &str = "note";
/// Панель экрана входа при Plasma Login Manager.
pub const SCREEN_PLASMA_LOGIN: &str = "plasmalogin";
/// Раскладки панели MATE (`categories::MATE_PANEL_KEY`).
pub const SCREEN_MATE_PANEL: &str = "matepanel";
/// Раздел настроек (`categories::SETTINGS_PANELS`): тот же раздел, что во
/// вкладке «Настройки», ключ раздела — ключ категории.
pub const SCREEN_SETTINGS: &str = "settings";

/// Модуль «Параметров системы», которым настраивается Plasma Login Manager
/// (`/usr/share/applications/kcm_plasmalogin.desktop`,
/// `Exec=systemsettings kcm_plasmalogin`).
pub const PLASMA_LOGIN_KCM: &str = "kcm_plasmalogin";

/// Чем открыть настройки экрана входа Plasma Login Manager — по порядку
/// попыток. Первой — сами «Параметры системы» на нужной странице (так
/// открывает пункт меню), запасной — тот же модуль отдельным окном
/// (`kcmshell6`). `have(имя)` — есть ли такая программа. Пусто — открыть
/// нечем, кнопку не показывать, а человека направить словами
/// ([`plasma_login_panel`] с `can_open = false`).
///
/// Прямо на подстраницу «Appearance» модуль не открыть: её он открывает
/// сам изнутри, аргумента для неё нет.
pub fn login_settings_commands(have: &dyn Fn(&str) -> bool) -> Vec<Vec<String>> {
    ["systemsettings", "kcmshell6"]
        .iter()
        .filter(|bin| have(bin))
        .map(|bin| vec![bin.to_string(), PLASMA_LOGIN_KCM.to_string()])
        .collect()
}

/// Подписи панели экрана входа при Plasma Login Manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlasmaLoginPanel {
    /// Заголовок панели.
    pub title: String,
    /// Что это и что тут можно поменять — человеческим языком.
    pub body: String,
    /// Кнопка в настройки экрана входа.
    pub open_label: String,
    /// Подсказка тем, кто пришёл за темами из магазина.
    pub store_hint: String,
    /// Строка состояния, если открыть настройки не вышло.
    pub open_failed: String,
}

/// Подписи панели Plasma Login Manager. `can_open` — есть чем открыть
/// настройки ([`login_settings_commands`] не пуст); если нечем, кнопки не
/// будет, и текст сам говорит, куда идти.
pub fn plasma_login_panel(can_open: bool) -> PlasmaLoginPanel {
    let mut body = tr(
        "It comes with KDE Plasma 6.6 and later and doesn't support third-party themes: \
         the login screen always looks like Breeze. You can change its background and \
         clock and bring your desktop's colors, cursor and fonts over to it — that's done \
         in System Settings.",
    );
    let where_to_go = tr("Open “System Settings → Login Screen”.");
    if !can_open {
        body = format!("{body}\n{where_to_go}");
    }
    PlasmaLoginPanel {
        title: tr("The login screen is drawn by Plasma Login Manager"),
        body,
        open_label: tr("Open Login Screen Settings"),
        store_hint: tr("Looking for login screen themes from the store? They only work with SDDM."),
        open_failed: format!(
            "{} {where_to_go}",
            tr("Couldn't open the login screen settings.")
        ),
    }
}

// ── Звуки событий выключены ────────────────────────────────────────────

/// Полоса над разделом «Звуки», когда системные звуки в окружении
/// выключены (в MATE — `org.mate.sound event-sounds`, в остальных — ключ из
/// `qt_settings::sound_backend`). В таком состоянии любая звуковая тема
/// немая: применить её можно, услышать — нет, и человек решит, что тема
/// сломана (правило #13 — назвать условие заранее).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventSoundsBanner {
    pub visible: bool,
    pub text: String,
    pub button: String,
}

/// Решение полосы: раздел `cat_key` и сырой ответ чтения выключателя
/// (`qt_settings::sound_read_argv_for(.., EventSounds, ..)`).
///
/// Пустой ответ — прочитать не удалось (нет схемы, нет утилиты): тогда
/// молчим, а не пугаем «выключено» там, где мы просто не знаем.
pub fn event_sounds_banner(cat_key: &str, raw: &str) -> EventSoundsBanner {
    let off = cat_key == "sounds"
        && !raw.trim().is_empty()
        && !crate::qt_settings::parse_event_sounds(raw);
    EventSoundsBanner {
        visible: off,
        text: tr(
            "System sounds are turned off, so no sound theme will be heard. \
             Turn them on to hear the theme.",
        ),
        button: tr("Turn on"),
    }
}

// ── «Вернуть как было» после «Применить» ───────────────────────────────

/// Полоса «Вернуть как было» раздела: удачное «Применить» оставило снимок
/// того, что было до него (`install_journal::Applied`), и человек может
/// вернуть это одной кнопкой.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AppliedRevertBanner {
    pub visible: bool,
    /// Раздел, который вернётся (ключ категории).
    pub section: String,
    /// Что вернётся — словами.
    pub text: String,
    /// Подпись кнопки.
    pub button: String,
}

/// Когда было «Применить» — коротко, для подписи: `25.10, 14:02`. Метка
/// журнала — местное время ISO (`2026-10-25T14:02:33`); не разобралась —
/// `None`, и подпись обходится без времени.
pub fn applied_when(applied_at: Option<&str>) -> Option<String> {
    let s = applied_at?;
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r.clone()).is_some_and(|x| x.iter().all(u8::is_ascii_digit));
    if !(digits(0..4) && b.get(4) == Some(&b'-') && digits(5..7) && digits(8..10) && digits(11..13) && digits(14..16)) {
        return None;
    }
    Some(format!("{}.{}, {}", &s[8..10], &s[5..7], &s[11..16]))
}

/// Решение полосы раздела `cat_key`.
///
/// * `applied` — запись «Применить» этого раздела в общем журнале;
/// * `layer_memo` — у цветов вне Plasma есть своя памятка слоя
///   (`shell_palette::read_layer_memo`) с правками, которые можно вернуть;
/// * `shell_revert_shown` — у полосы «Палитра оболочки» видна своя кнопка
///   «Вернуть как было» (`PaletteBanner::can_revert` при живом шелле).
///
/// Кнопка на раздел одна. У цветов, пока видна кнопка полосы шелла,
/// своей нет: та возвращает и файлы шелла, и прежнюю схему.
pub fn applied_revert_banner(
    cat_key: &str,
    applied: Option<&crate::install_journal::Applied>,
    layer_memo: bool,
    shell_revert_shown: bool,
) -> AppliedRevertBanner {
    let colors = cat_key == "colors";
    let applied = applied.filter(|a| !a.files.is_empty());
    let hidden = AppliedRevertBanner { section: cat_key.to_string(), ..Default::default() };
    if colors && shell_revert_shown {
        return hidden;
    }
    let (text, button) = match applied {
        Some(a) => match applied_when(a.applied_at.as_deref()) {
            Some(when) => (
                trf(
                    // TRANSLATORS: %1 is a theme name, %2 is a short date and time (25.10, 14:02).
                    "Before applying “%1” (%2), Rexon saved what was there. You can return it.",
                    &[&a.theme, &when],
                ),
                trf(
                    // TRANSLATORS: button; %1 is a theme name, %2 is a short date and time (25.10, 14:02).
                    "Return as It Was Before “%1” (%2)",
                    &[&a.theme, &when],
                ),
            ),
            None => (
                trf(
                    // TRANSLATORS: %1 is a theme name.
                    "Before applying “%1”, Rexon saved what was there. You can return it.",
                    &[&a.theme],
                ),
                trf(
                    // TRANSLATORS: button; %1 is a theme name.
                    "Return as It Was Before “%1”",
                    &[&a.theme],
                ),
            ),
        },
        None if colors && layer_memo => (
            tr("Rexon changed the colors of applications outside Plasma. You can return the colors that were there before."),
            // TRANSLATORS: button; returns the application colors that were there before Rexon.
            tr("Return Colors as They Were"),
        ),
        None => return hidden,
    };
    AppliedRevertBanner { visible: true, section: cat_key.to_string(), text, button }
}

/// Итог «Вернуть как было» по записи «Применить» (`revert_applied_at`):
/// `(получилось, текст для человека)`.
pub fn applied_revert_outcome(theme: &str, result: &std::io::Result<Option<Vec<String>>>, home: &str) -> (bool, String) {
    match result {
        Ok(None) => (false, tr("There is nothing to return: Rexon has no saved copy for this section.")),
        Ok(Some(failed)) if failed.is_empty() => (
            true,
            trf(
                // TRANSLATORS: %1 is a theme name.
                "Returned what was there before “%1”. Programs that are already open may show it only after a restart or the next login.",
                &[&theme],
            ),
        ),
        Ok(Some(failed)) => {
            let short = |p: &String| match p.strip_prefix(home).filter(|r| !home.is_empty() && r.starts_with('/')) {
                Some(rest) => format!("~{rest}"),
                None => p.clone(),
            };
            // Пути — файлы; остальное — ключи настроек стола (gsettings,
            // dconf, xfconf): у них своя строка, файлами их не называть.
            let (values, files): (Vec<&String>, Vec<&String>) =
                failed.iter().partition(|f| crate::install_journal::is_value_key(f));
            let mut lines = Vec::new();
            if !files.is_empty() {
                let list: Vec<String> = files.iter().map(|p| short(p)).collect();
                lines.push(fill(
                    &crate::i18n::ntr(
                        // TRANSLATORS: %n is the number of files, %1 is their list. The copies stay saved, so the button keeps working.
                        "Not everything returned: %n file could not be put back (%1). Its copy is kept — try again.",
                        "Not everything returned: %n files could not be put back (%1). Their copies are kept — try again.",
                        files.len() as u64,
                    ),
                    &[&list.join(", ")],
                ));
            }
            if !values.is_empty() {
                let list: Vec<&str> = values.iter().map(|k| crate::install_journal::value_key_label(k)).collect();
                lines.push(fill(
                    &crate::i18n::ntr(
                        // TRANSLATORS: %n is the number of desktop settings (theme, icons… kept by the desktop itself, not in files), %1 is their list. The old values stay saved, so the button keeps working.
                        "Not everything returned: %n desktop setting could not be put back (%1). Its old value is kept — try again.",
                        "Not everything returned: %n desktop settings could not be put back (%1). Their old values are kept — try again.",
                        values.len() as u64,
                    ),
                    &[&list.join(", ")],
                ));
            }
            (false, lines.join("\n"))
        }
        Err(e) => (false, trf("Could not return: %1", &[&e.to_string()])),
    }
}

/// «Вернуть как было» раздела `section`: одна кнопка полосы
/// [`applied_revert_banner`].
///
/// У цветов с памяткой слоя первой работает она
/// (`shell_palette::revert_scheme_layer`: по ключам, правки после Rexon
/// бережёт), затем запись «Применить» — только для путей, которых памятка не
/// касается (и не для самой памятки: иначе вернулась бы её прежняя копия).
/// Остальные разделы — запись «Применить» целиком.
pub fn revert_applied_section(
    rexon_dir: &Path,
    palette: &crate::shell_palette::PaletteHome,
    section: &str,
) -> (bool, String) {
    use crate::install_journal as journal;
    use crate::shell_palette as sp;
    let rec = journal::view_at(rexon_dir).applied.get(section).cloned();
    let theme = rec.as_ref().map(|r| r.theme.clone()).unwrap_or_default();
    let memo = sp::read_layer_memo(palette);
    if section == "colors" && memo.has_changes() {
        let memo_file = sp::layer_memo_path(palette).to_string_lossy().into_owned();
        let mine: BTreeSet<String> = memo.files.iter().map(|f| f.path.clone()).chain([memo_file]).collect();
        let msg = match sp::revert_scheme_layer(palette) {
            Ok(m) => m,
            Err(e) => return (false, trf("Could not return: %1", &[&e])),
        };
        if rec.is_none() {
            return (true, msg);
        }
        let rest = journal::revert_applied_except_at(rexon_dir, section, &|p| mine.contains(p));
        return match rest {
            Ok(Some(failed)) if failed.is_empty() => (true, msg),
            other => applied_revert_outcome(&theme, &other, &palette.home),
        };
    }
    applied_revert_outcome(&theme, &journal::revert_applied_at(rexon_dir, section), &palette.home)
}

// ── Страницы ───────────────────────────────────────────────────────────

/// Сколько всего страниц у категории.
///
/// Магазин листает ровно по запрошенному размеру (`page × page_size`), а
/// скрытые им работы просто выпадают из своей страницы: на странице бывает
/// 18 карточек из двадцати, но следующая всё равно начинается с 21-й.
/// Поэтому число страниц считается по ЗАПРОШЕННОМУ размеру. Прежний счёт по
/// пришедшей странице (318 / 18 = 18 вместо 16) показывал в конце пустые
/// страницы — проверено 03.10.2026 на Global Themes: 18, 20 × 14, 18, 0.
pub fn total_pages(total: i64, page_size: u32) -> i64 {
    if total <= 0 {
        return 1;
    }
    let per = i64::from(page_size.max(1));
    (1).max((total + per - 1) / per)
}

/// Состояние блока «Страница N / M».
#[derive(Debug, Clone, PartialEq)]
pub struct Pagination {
    pub label: String,
    pub prev_enabled: bool,
    pub next_enabled: bool,
}

/// Блок страниц под текущий ответ каталога.
pub fn pagination(page: i64, total: i64, shown: usize, page_size: u32) -> Pagination {
    let pages = total_pages(total, page_size);
    Pagination {
        label: trf("Page %1 / %2", &[&(page + 1), &pages]),
        prev_enabled: page > 0,
        next_enabled: shown > 0 && (page + 1) < pages,
    }
}

/// Есть ли следующая страница, которую стоит прогреть заранее.
///
/// ⚠️ Починка оригинала (правило #16). Прогрев считал страницы по
/// ЗАПРОШЕННОМУ размеру (двадцать), а кнопка «Вперёд» — по реально пришедшему
/// (около десяти). Из-за этого во второй половине каталога кнопка листала
/// дальше, а прогрев считал, что страниц уже нет, и та страница каждый раз
/// открывалась через сеть. Счёт теперь один на обоих.
pub fn has_page_to_prefetch(page: i64, total: i64, page_size: u32) -> bool {
    total > 0 && (page + 1) < total_pages(total, page_size)
}

/// Страница Rexon, набранная из потока источника.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPage<T> {
    /// Ровно `page_size` работ подряд (меньше — только в самом конце потока).
    pub items: Vec<T>,
    /// Сколько работ источник назвал всего (`totalitems`) — с теми, что он
    /// потом скрывает, поэтому видимых бывает меньше.
    pub total: i64,
    /// За страницей есть хотя бы одна работа: «Вперёд» горит по факту, а не
    /// по `total`.
    pub has_more: bool,
}

/// Добор страницы: страница Rexon номер `page` — работы потока
/// `[page × size, page × size + size)`, где поток — серверные страницы
/// подряд по `server_size` работ. Размеры разные: страница Rexon — целые
/// ряды сетки ([`grid_page_size`]), серверная — всегда [`PAGE_SIZE`].
///
/// Так ведёт себя KDE Store (OCS): листает строго по запрошенным двадцати, а
/// скрытые им работы выпадают из своей страницы, и на экране оставалась дыра
/// (Global Themes 03.10.2026: 18, 20 × 14, 18). Здесь недостачу закрывают
/// работы следующей серверной страницы, а следующая страница Rexon начинается
/// ровно с того места, где кончилась эта, — без повторов и пропусков.
///
/// `fetch(k)` — серверная страница `k` (с нуля, по `server_size` работ):
/// работы и `totalitems`.
/// Источник не ответил — `(пусто, 0)`. Серверные страницы тянутся с нулевой
/// по порядку, потому что, где в потоке начинается страница Rexon, известно
/// только сложив длины всех предыдущих; при листании подряд они уже в кеше
/// (`ocs_cache`), так что сеть видит одну-две новые страницы за шаг.
/// Тянется на одну работу больше страницы — чтобы честно знать, есть ли
/// следующая (`has_more`). Конец потока — по счёту самого источника
/// (`ceil(total / server_size)` серверных страниц), а не по короткой странице:
/// короткие страницы бывают и в середине. Источник замолчал посреди потока —
/// о следующей странице судим по `total`, как раньше, а не гасим «Вперёд».
pub fn stream_page<T>(
    page: i64,
    page_size: u32,
    server_size: u32,
    mut fetch: impl FnMut(i64) -> (Vec<T>, i64),
) -> StreamPage<T> {
    let size = page_size.max(1) as usize;
    let server_size = i64::from(server_size.max(1));
    let start = (page.max(0) as usize).saturating_mul(size);
    let want = start.saturating_add(size + 1);
    let mut items: Vec<T> = Vec::with_capacity(size + 1);
    let mut seen = 0usize;
    let mut total = 0i64;
    let mut broken = false;
    let mut server = 0i64;
    loop {
        let (chunk, reported) = fetch(server);
        if reported <= 0 && chunk.is_empty() {
            // Нулевая страница пуста — пустой поток; дальше — сбой источника.
            broken = server > 0;
            break;
        }
        if reported > 0 {
            total = reported;
        }
        for it in chunk {
            if seen >= start && seen < want {
                items.push(it);
            }
            seen += 1;
        }
        server += 1;
        if seen >= want || server.saturating_mul(server_size) >= total {
            break;
        }
    }
    let has_more = if broken {
        (page + 1) < total_pages(total, page_size)
    } else {
        items.len() > size
    };
    items.truncate(size);
    StreamPage { items, total, has_more }
}

/// Блок страниц для потока с добором ([`stream_page`]): «Вперёд» — по факту
/// (`has_more`), число страниц — по `total`, пока поток не кончился, и по
/// факту, когда кончился (скрытых работ в конце может набраться на целую
/// страницу — пустую последнюю не обещаем).
pub fn stream_pagination(page: i64, total: i64, has_more: bool, page_size: u32) -> Pagination {
    let pages = if has_more {
        total_pages(total, page_size).max(page + 2)
    } else {
        page + 1
    };
    Pagination {
        label: trf("Page %1 / %2", &[&(page + 1), &pages]),
        prev_enabled: page > 0,
        next_enabled: has_more,
    }
}

/// Ломоть AUR-списка под текущую страницу: у AUR серверных страниц нет,
/// RPC отдаёт всё разом, и страницы режутся на месте.
pub fn aur_page_range(len: usize, page: i64) -> std::ops::Range<usize> {
    let per = PAGE_SIZE as usize;
    let start = (page.max(0) as usize).saturating_mul(per).min(len);
    let end = start.saturating_add(per).min(len);
    start..end
}

/// Страница, на которую сажаем свежий AUR-список: дальше конца не уводим.
pub fn aur_clamp_page(len: usize, page: i64) -> i64 {
    let last = if len == 0 { 0 } else { (len as i64 - 1) / PAGE_SIZE as i64 };
    page.min(last.max(0)).max(0)
}

/// Можно ли листать AUR-список на `delta` страниц.
pub fn aur_can_change_page(len: usize, page: i64, delta: i64) -> bool {
    let new = page + delta;
    if new < 0 {
        return false;
    }
    if delta > 0 && new * PAGE_SIZE as i64 >= len as i64 {
        return false;
    }
    true
}

/// Слишком короткий запрос для поиска по AUR — RPC на одной букве отдаёт
/// половину репозитория.
pub fn aur_query_too_short(query: &str) -> bool {
    !query.is_empty() && query.chars().count() < AUR_MIN_QUERY
}

/// Подпись при слишком коротком запросе к AUR.
pub fn aur_query_too_short_msg() -> String {
    trf("AUR search needs at least %1 characters", &[&AUR_MIN_QUERY])
}

/// Подпись раздела и поиска, когда магазин KDE страницу не дал
/// ([`ocs::StoreFailure`]): честно «не отвечает», а не «ничего не найдено».
/// Установленное раздел показывает по-прежнему — это только строка состояния.
pub fn store_failure_text(failure: &ocs::StoreFailure) -> String {
    match failure {
        ocs::StoreFailure::Refused { code, message } if !message.is_empty() => trf(
            // TRANSLATORS: %1 is the store's error code, %2 its own message (English, from the server).
            "The KDE Store isn't responding right now (error %1: %2). Please try again later.",
            &[code as &dyn std::fmt::Display, message],
        ),
        ocs::StoreFailure::Refused { code, .. } if !code.is_empty() => trf(
            // TRANSLATORS: %1 is the store's error code.
            "The KDE Store isn't responding right now (error %1). Please try again later.",
            &[code as &dyn std::fmt::Display],
        ),
        ocs::StoreFailure::Refused { .. } | ocs::StoreFailure::Malformed => {
            tr("The KDE Store isn't responding right now. Please try again later.")
        }
        ocs::StoreFailure::Network(raw) => ui_base::humanize_network_error(raw, "KDE Store"),
    }
}

// ── Местный поиск и сканы диска ────────────────────────────────────────

/// Остаётся ли тема с диска в поле зрения при действующем запросе.
///
/// Единое мерило на ВСЕ места, где страница показывает темы с диска:
/// разделы-обзоры («Активные», «Загруженные», «Системные», «Стиль
/// приложений», «Избранное»), подсказки строки поиска и установленные темы,
/// доклеенные сверху магазина в «Сплэш-экране» и «Экране блокировки».
/// Последние поиск раньше не замечал вовсе — витрина фильтровалась, а они
/// оставались на месте.
pub fn local_matches(name: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || name.to_lowercase().contains(&q)
}

/// Что именно сканировать на диске по ключу раздела.
///
/// Обратная сторона [`AppearanceView::local_scan_which`]: ключ собирает
/// ядро (`widgetstyle:<под-выбор>` для «Стиля приложений», иначе голый ключ
/// раздела), и разбирать его обратно тоже ядру. Пока разбор жил у
/// исполнителя, формат ключа знали двое, и разойтись они могли молча —
/// сборщик добавил бы префикс, а разборщик о нём не узнал.
///
/// Отдаёт `(вид скана, имя Qt-стиля)`:
///   * `active`, `downloaded`, `system`, `splash`, `lockscreen` — свой скан
///     на каждый, имя пустое;
///   * `widgetstyle` — «Стиль приложений» одной сеткой: стили Qt и темы
///     Kvantum; вторым элементом — фильтр (`all`, `qt`, `kvantum`);
///   * `section` — раздел целиком, вторым элементом — ключ раздела;
///   * `` (пусто) — такого скана нет, показывать нечего.
pub fn local_scan_plan(which: &str) -> (String, String) {
    if let Some(sub) = which.strip_prefix("widgetstyle:") {
        let filter = match sub {
            STYLE_FILTER_QT | STYLE_FILTER_KVANTUM => sub,
            _ => STYLE_FILTER_ALL,
        };
        return ("widgetstyle".to_string(), filter.to_string());
    }
    match which {
        "widgetstyle" => ("widgetstyle".to_string(), STYLE_FILTER_ALL.to_string()),
        "active" | "downloaded" | "system" | "splash" | "lockscreen" => {
            (which.to_string(), String::new())
        }
        // Раздел окружения: всё его на диске — включённое и нет.
        k if categories::DE_SECTIONS.iter().any(|(_, key)| *key == k) => {
            ("section".to_string(), k.to_string())
        }
        // Раздел KDE с сеткой установленного (глобальные темы, стили Plasma,
        // цвета, рамки, значки, курсоры, GTK, звуки): тот же скан раздела —
        // текущая, свои, системные.
        k if categories::KDE_INSTALLED_SECTIONS.iter().any(|(_, key)| *key == k) => {
            ("section".to_string(), k.to_string())
        }
        _ => (String::new(), String::new()),
    }
}

/// Раздел с сеткой установленного при любом режиме (без экрана блокировки,
/// у которого она есть только в режиме полных тем). Тема, поставленная в
/// такой раздел, видна в нём самом (`local_install`), а не только в
/// «Загруженных».
///
/// Ключ ищется в таблице разделов ([`categories::categories`]); для
/// открытого раздела — [`has_installed_grid_for`].
pub fn has_installed_grid(key: &str) -> bool {
    categories::categories(Path::new(""))
        .iter()
        .find(|c| c.key == key)
        .is_some_and(has_installed_grid_for)
}

/// Сетка установленного у раздела: она есть у ЛЮБОГО раздела, у которого
/// есть и скан диска ([`local_scan_plan`]), и магазин.
///
/// До 17.10.2026 это решал жёсткий список восьми разделов KDE и заставки,
/// и раздел видел либо только диск, либо только магазин: у Conky и
/// разделов KWin (переключатель, скрипты, эффекты) скан был, а
/// установленное в своём разделе не показывалось вовсе. Теперь новый
/// раздел с магазином и сканом получает кнопку «Загрузить новые…» сам.
///
/// Экран блокировки сюда не входит: у него сетка есть только в режиме
/// полных тем ([`CatalogView::has_installed_view`]).
pub fn has_installed_grid_for(cat: &ThemeCategory) -> bool {
    if cat.virtual_ || cat.key == "lockscreen" || categories::LOCAL_VIEW_KEYS.contains(&cat.key.as_str()) {
        return false;
    }
    let has_store = !cat.local_only();
    has_store && !local_scan_plan(&cat.key).0.is_empty()
}

/// Остаётся ли КАРТОЧКА темы с диска в поле зрения при действующем запросе.
///
/// Сверх показанного имени ([`local_matches`]) тема находится и по тому, как
/// её зовут в оригинале: по имени без перевода (`source_name`, `Name=` без
/// языка) и по имени каталога из номера карточки (`local:<вид>:<каталог>`).
/// Иначе системные темы с переводом в метаданных терялись: «Adwaita» и
/// «HighContrast» не находились, потому что на карточке стоят «Адвайта» и
/// «Высококонтрастная» (живая проверка r1434, 16.10.2026).
///
/// По оригинальным именам сверка ещё и без пробелов, дефисов и
/// подчёркиваний: каталог `HighContrast` находится по «High Contrast»,
/// `adw-gtk3` — по «adw gtk3».
pub fn local_item_matches(item: &ThemeItem, query: &str) -> bool {
    if local_matches(&item.name, query) {
        return true;
    }
    let q = query.trim().to_lowercase();
    let q_squeezed = squeeze_name(&q);
    let dir = local_theme_name(&item.id, "");
    for original in [item.source_name.as_str(), dir.as_str()] {
        if original.is_empty() {
            continue;
        }
        let low = original.to_lowercase();
        if low.contains(&q) || (!q_squeezed.is_empty() && squeeze_name(&low).contains(&q_squeezed)) {
            return true;
        }
    }
    false
}

/// Имя без пробелов и знаков-разделителей — для сверки оригинальных имён.
fn squeeze_name(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Фильтр списка по строке поиска — для разделов, у которых сети нет.
pub fn local_filter(items: &[ThemeItem], query: &str) -> Vec<ThemeItem> {
    items
        .iter()
        .filter(|it| local_item_matches(it, query))
        .cloned()
        .collect()
}

/// Подпись статуса по виду скана; `None` — у вида своей подписи нет и берётся
/// имя раздела.
pub fn scan_status(which: &str, n: usize) -> Option<String> {
    match which {
        "active" => Some(trf("Active themes: %1", &[&n])),
        "downloaded" => Some(trf("Downloaded themes: %1", &[&n])),
        "system" => Some(trf("System themes: %1", &[&n])),
        _ => None,
    }
}

/// Значок «активна» в подписи автора локальной темы.
///
/// Подпись, а НЕ признак: страница Rust узнаёт включённую тему по
/// `ThemeItem::local_kind`, который ставит сам скан. Поиск бейджа в подписи
/// ([`scan_local_kind`]) остался только для окна на Python.
pub fn local_active_badge() -> String {
    tr("✅ Active")
}

/// Какой вид карточки проставить элементу скана.
///
/// «Активные» — все с бейджем; «Загруженные» и «Системные» — обычные; у
/// остальных сканов (стиль приложений, заставка, экран блокировки) активный
/// элемент узнаётся по подписи автора.
pub fn scan_local_kind(which: &str, author: &str) -> &'static str {
    match which {
        "active" => "active",
        "downloaded" | "system" => "downloaded",
        _ => {
            if author.contains(&local_active_badge()) {
                "active"
            } else {
                "downloaded"
            }
        }
    }
}

/// Ответ скана устарел — человек уже ушёл в другой раздел.
///
/// Сканы «Стиля приложений» приходят с под-выбором в ключе
/// (`widgetstyle:<что>`) и снимаются по своему счётчику, а не по имени
/// раздела. Оригинал выражал ровно это же тремя вложенными условиями, одно из
/// которых было недостижимо.
pub fn scan_answer_stale(which: &str, active_cat_key: &str) -> bool {
    !which.starts_with("widgetstyle") && which != active_cat_key
}

// ── Экран блокировки ───────────────────────────────────────────────────

/// Переписать категорию онлайн-карточек под режим «Экрана блокировки».
///
/// Карточки приходят из каталога Global Themes (722) или Wallpapers (299), и
/// установщик применил бы ВЕСЬ Global Theme либо просто скопировал обои.
/// Синтетическая категория заставляет его тронуть именно компонент блокировки
/// (`lockscreen`) или её фон (`lockwallpaper`). Установленные местные карточки
/// приходят уже с верной категорией — их не трогаем.
pub fn stamp_lock_kind(items: &mut [ThemeItem], lock_mode: &str) {
    let forced = if lock_mode == LOCK_MODE_THEMES {
        LOCK_CAT_THEMES
    } else {
        LOCK_CAT_WALLPAPER
    };
    for it in items.iter_mut() {
        if it.id.starts_with("local:") {
            continue;
        }
        it.category_id = forced;
    }
}

// ── Фоновый прогрев категорий ──────────────────────────────────────────

/// Первые страницы всех СЕТЕВЫХ категорий сверху вниз — `(cat_id, sort)`.
///
/// `cat_id` строкой — тем же значением, что уйдёт в запрос
/// ([`CatalogView::query_category_id`]), иначе ключ кеша не сойдётся.
/// Виртуальные «Популярные»/«Новые» включены (они агрегируют ходовые
/// категории окружения с закреплённой сортировкой); локальные обзоры диска,
/// разделы без магазина и разделы, которых в окружении нет, пропускаются.
/// Номера берутся из каталога раздела (`store`), а не из номера раздела:
/// синтетический номер больше девятисот в магазин не уходит. Дедуп по паре.
pub fn prefetch_targets(view: &CatalogView, sidebar: &[ThemeCategory]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for cat in sidebar {
        if !prefetchable(cat) || steamos_closed(view, cat) || !shown_here(view, cat) {
            continue;
        }
        let key = (prefetch_cat_id(view, cat), view.sort_for(cat));
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// Номер категории для прогрева раздела — тот же, что уйдёт в живой запрос:
/// у «Популярных» и «Новых» это ходовые категории окружения
/// ([`categories::aggregate_query`]).
fn prefetch_cat_id(view: &CatalogView, cat: &ThemeCategory) -> String {
    store_query(cat, &view.de)
}

/// Номера категорий OCS раздела строкой для запроса (зонт — через запятую,
/// как понимает магазин): из поля `store`, а не из номера раздела. Сборная
/// витрина — ходовые категории окружения ([`categories::aggregate_query`]).
/// Каталог не из OCS — пусто.
pub fn store_query(cat: &ThemeCategory, de: &str) -> String {
    match &cat.store {
        CatalogSource::OcsShowcase => categories::aggregate_query(de),
        CatalogSource::Ocs(cats) => cats.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","),
        _ => String::new(),
    }
}

/// Раздел виден в окружении каталога — раздел, которого здесь нет, человек
/// не откроет, и греть его витрину незачем.
fn shown_here(view: &CatalogView, cat: &ThemeCategory) -> bool {
    categories::category_available(&cat.key, &view.de)
}

/// Раздел закрыт на SteamOS — его витрина человеку не покажется, греть её
/// незачем.
fn steamos_closed(view: &CatalogView, cat: &ThemeCategory) -> bool {
    view.steamos && categories::steamos_block(&cat.key).is_some()
}

/// Ходит ли раздел в сеть за каталогом.
///
/// Каталог — из поля `store`: разделы окружений со своей категорией OCS
/// под синтетическим номером (рамки XFCE — 138) греются тем же запросом,
/// что уйдёт живьём. Экран блокировки — нет: номер у него по режиму, и
/// угадать его здесь значило бы греть не ту витрину.
fn prefetchable(cat: &ThemeCategory) -> bool {
    !categories::LOCAL_VIEW_KEYS.contains(&cat.key.as_str())
        && cat.key != "lockscreen"
        && matches!(cat.store, CatalogSource::Ocs(_) | CatalogSource::OcsShowcase)
}

/// Соседи открытого раздела (N+1, затем N+2…), которые стоит прогреть первыми.
///
/// Прогрессивность: открыл раздел — сразу греем следующий, при переключении он
/// откроется мгновенно из кеша.
pub fn prefetch_neighbours(
    view: &CatalogView,
    sidebar: &[ThemeCategory],
    done: &BTreeSet<(String, String)>,
) -> Vec<(String, String)> {
    let Some(idx) = sidebar.iter().position(|c| c.key == view.category.key) else {
        return Vec::new();
    };
    let mut front: Vec<(String, String)> = Vec::new();
    for cat in &sidebar[idx + 1..] {
        if !prefetchable(cat) || steamos_closed(view, cat) || !shown_here(view, cat) {
            continue;
        }
        let key = (prefetch_cat_id(view, cat), view.sort_for(cat));
        if done.contains(&key) || front.contains(&key) {
            continue;
        }
        front.push(key);
    }
    front
}

/// Новая очередь прогрева: соседи впереди, остальное — следом и без дублей.
pub fn prefetch_reorder(
    front: &[(String, String)],
    queue: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out = front.to_vec();
    for k in queue {
        if !front.contains(k) {
            out.push(k.clone());
        }
    }
    out
}

// ── Выбор рабочего стола ───────────────────────────────────────────────

/// С каким рабочим столом открыть каталог и что при этом запомнить.
#[derive(Debug, Clone, PartialEq)]
pub struct DeChoice {
    /// Рабочий стол каталога.
    pub de_id: String,
    /// Что записать в настройки: `(выбор, какой стол был запущен)`. `None` —
    /// записывать нечего.
    pub remember: Option<(String, String)>,
}

/// Побеждает ФАКТИЧЕСКИ запущенный рабочий стол: зашёл в сеанс GNOME —
/// каталог сам стал гномьим, помнить прошлый выбор тут неправильно.
///
/// Но выбор, сделанный руками, при этом не затирается, и вот как одно
/// отличается от другого: вместе с выбором мы запоминаем, какой стол был
/// запущен В МОМЕНТ выбора. Совпадает с нынешним — значит человек так и
/// решил, оставляем его выбор; не совпадает — значит он сменил рабочий стол,
/// и выбор был унаследован, а не сделан.
///
/// Совсем ничего не известно — показываем самый широкий набор разделов, а не
/// прячем половину каталога.
pub fn initial_de(detected: &str, saved: &str, seen: &str) -> DeChoice {
    if !detected.is_empty() && detected != seen {
        return DeChoice {
            de_id: detected.to_string(),
            remember: Some((detected.to_string(), detected.to_string())),
        };
    }
    if !saved.is_empty() {
        return DeChoice { de_id: saved.to_string(), remember: None };
    }
    let de_id = if detected.is_empty() { "kde" } else { detected };
    DeChoice { de_id: de_id.to_string(), remember: None }
}

/// Пункт «Определять автоматически» в выборе рабочего стола.
pub const DE_AUTO_ID: &str = "auto";

/// Пункты выбора рабочего стола с подписью ручного выбора.
///
/// Ручной выбор живёт между запусками — так задумано (`initial_de`). Но
/// когда он отличается от запущенного сеанса, на экране это должно быть
/// видно: 19.10.2026 в сеансе MATE стояло просто «KDE Plasma», и казалось,
/// что Rexon спутал рабочий стол. Тогда выбранный пункт подписан «выбрано
/// вручную, сейчас <сеанс>», а первым идёт «Определять автоматически»,
/// который этот выбор сбрасывает.
pub fn de_picker_choices(selected: &str, detected: &str) -> Vec<(String, String)> {
    let mut out = crate::categories::appearance_de_choices(selected);
    // Тайловые окружения стоят одним общим пунктом: sway против Hyprland —
    // не ручной выбор, а тот же пункт.
    let tiling = |id: &str| {
        crate::de_customization::de_from_id(id) == Some(crate::de_customization::De::Wlroots)
    };
    if detected.is_empty() || selected == detected || (tiling(selected) && tiling(detected)) {
        return out;
    }
    let session_name = out
        .iter()
        .find(|(id, _)| id == detected)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| {
            crate::categories::appearance_de_choices(detected)
                .into_iter()
                .find(|(id, _)| id == detected)
                .map(|(_, n)| n)
                .unwrap_or_else(|| detected.to_string())
        });
    for (id, name) in out.iter_mut() {
        if id == selected {
            *name = trf("%1 (chosen manually, now %2)", &[&name.clone(), &session_name]);
        }
    }
    out.insert(
        0,
        (DE_AUTO_ID.to_string(), trf("Detect automatically (%1)", &[&session_name])),
    );
    out
}

/// Что делать по выбранному пункту: `(стол каталога, что записать выбором)`.
/// «Определять автоматически» — стол сеанса и ПУСТОЙ выбор: с ним
/// `initial_de` на каждом запуске сам идёт за сеансом.
pub fn resolve_de_pick(picked: &str, detected: &str) -> (String, String) {
    if picked == DE_AUTO_ID {
        let de = if detected.is_empty() { "kde" } else { detected };
        return (de.to_string(), String::new());
    }
    (picked.to_string(), picked.to_string())
}

// ═══════════════════════════════════════════════════════════════════════
//  Панель деталей: галерея, кнопки состояния, комментарии
// ═══════════════════════════════════════════════════════════════════════

/// ⚠️ Разделители между слотами стат-строки — настоящий НЕРАЗРЫВНЫЙ ПРОБЕЛ, а
/// не мнемоника `&nbsp;`: без тегов Qt считает такой текст простым и печатает
/// мнемонику как есть (замечено попиксельной сверкой со старой версией).
pub const NBSP: &str = "\u{a0}";

// ── Галерея скриншотов ─────────────────────────────────────────────────

/// Скриншоты карусели — РОВНО как на сайте: без первой картинки.
///
/// Первый слот на pling — «лицо товара»: сайт показывает его только иконкой в
/// каталоге и в шапке, в карусели его НЕТ. У нас так же: первый остаётся
/// превью плитки, галерея — со второго. Картинка у товара всего одна —
/// показываем её.
///
/// Ссылки из ответа магазина — уменьшённые копии (770×540); в полноэкранном
/// просмотре они растягивались на монитор и мылились. Каждая гонится через
/// общий пересчёт в оригинал полного разрешения.
pub fn gallery_urls(preview_urls: &[String], preview_url: &str) -> Vec<String> {
    let mut urls: Vec<String> = preview_urls.to_vec();
    if urls.is_empty() && !preview_url.is_empty() {
        urls.push(preview_url.to_string());
    }
    if urls.len() > 1 {
        urls.remove(0);
    }
    urls.iter().map(|u| ocs::full_preview_url(u)).collect()
}

/// Картинки плитки по очереди: первая не загрузилась (сеть, 404, вместо
/// картинки пришло что-то другое) — берём следующую.
///
/// У магазина первая картинка бывает мёртвой при живых остальных: у «Sirin
/// Cursor» (2370501) `previewpic1` — GIF с иероглифами в имени, сервер на
/// него отвечает 404, и плитка стояла пустой. Порядок: превью плитки, потом
/// все `previewpicN` по номерам; последним запасом — оригиналы полного
/// размера (уменьшенная копия у pling может не собраться при живом
/// оригинале). Дублей нет.
pub fn preview_candidates(preview_url: &str, preview_urls: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |u: &str, out: &mut Vec<String>| {
        if !u.is_empty() && !out.iter().any(|x| x == u) {
            out.push(u.to_string());
        }
    };
    push(preview_url, &mut out);
    for u in preview_urls {
        push(u, &mut out);
    }
    let small = out.clone();
    for u in &small {
        push(&ocs::full_preview_url(u), &mut out);
    }
    out
}

/// Следующая картинка после той, что не загрузилась; `None` — запас кончился.
pub fn next_preview_candidate(candidates: &[String], failed: &str) -> Option<String> {
    let pos = candidates.iter().position(|u| u == failed)?;
    candidates.get(pos + 1).cloned()
}

/// Картинка с движением? Решение — по первым байтам файла, а не по имени:
/// кеш превью берёт расширение из ссылки магазина, и у ссылки без него файл
/// ложится как `.jpg`, будь он хоть GIF. Анимация — GIF (`GIF87a`/`GIF89a`)
/// и WebP с флагом анимации в заголовке `VP8X`.
pub fn is_animated_image_header(head: &[u8]) -> bool {
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return true;
    }
    head.len() >= 21
        && &head[0..4] == b"RIFF"
        && &head[8..12] == b"WEBP"
        && &head[12..16] == b"VP8X"
        && head[20] & 0x02 != 0
}

/// Похоже ли начало файла на картинку. Сервер иногда отвечает «успехом» со
/// страницей ошибки вместо картинки — такой файл в плитку не ставим, а
/// берём следующую картинку товара.
pub fn looks_like_image(head: &[u8]) -> bool {
    let starts = |sig: &[u8]| head.starts_with(sig);
    if starts(b"\x89PNG\r\n\x1a\n")
        || starts(&[0xFF, 0xD8, 0xFF])
        || starts(b"GIF87a")
        || starts(b"GIF89a")
        || starts(b"BM")
        || starts(&[0, 0, 1, 0])
        || (starts(b"RIFF") && head.get(8..12) == Some(b"WEBP"))
        || head.get(4..8) == Some(b"ftyp")
    {
        return true;
    }
    // SVG — текст: пропускаем пробелы и смотрим на корень.
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start().to_ascii_lowercase();
    if t.contains("<html") || t.starts_with("<!doctype html") {
        return false;
    }
    t.starts_with("<svg") || t.starts_with("<?xml") || (t.starts_with("<!--") && t.contains("<svg"))
}

/// Шаг галереи с обходом битых кадров: кадр, который не загрузился, при
/// листании пропускается. Битые все, кроме текущего, — остаёмся на месте.
pub fn gallery_step_skipping(index: i64, total: i64, delta: i64, failed: &BTreeSet<i64>) -> i64 {
    if total <= 1 || delta == 0 {
        return index;
    }
    let mut next = index;
    for _ in 0..total {
        next = gallery_step(next, total, delta);
        if next == index {
            break;
        }
        if !failed.contains(&next) {
            return next;
        }
    }
    index
}

/// Счётчик «2/4» без битых кадров: человек видит только живые.
pub fn gallery_counter_skipping(index: i64, total: i64, failed: &BTreeSet<i64>) -> Option<String> {
    let dead = failed.iter().filter(|&&i| i >= 0 && i < total).count() as i64;
    let alive = total - dead;
    let before = failed.iter().filter(|&&i| i >= 0 && i < index).count() as i64;
    gallery_counter(index - before, alive)
}

/// Листание стрелками — с закольцовкой (удобно с геймпада и с клавиатуры).
pub fn gallery_step(index: i64, total: i64, delta: i64) -> i64 {
    if total <= 1 {
        return index;
    }
    (index + delta).rem_euclid(total)
}

/// Сколько кадров у галереи: полные детали приносят больше ссылок, чем список,
/// поэтому счётчик только растёт — иначе «2/5» превратилось бы в «2/1».
pub fn gallery_total(known: i64, src_len: usize, loaded_len: usize) -> i64 {
    known.max(src_len as i64).max(loaded_len as i64)
}

/// Что показывать на месте картинки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GalleryFrame {
    /// Кадр есть — показать файл.
    Image(String),
    /// Кадр не доедет: сервер отказал либо обход ссылок уже закончен.
    Failed,
    /// Кадр ещё едет.
    Loading,
    /// Одиночная картинка, которой пока нет: панель остаётся как была.
    Unchanged,
    /// Картинки у товара нет и не будет: обход ссылок закончен пустым либо
    /// единственный кадр не доехал. Раньше на этом месте навсегда оставалось
    /// «Загрузка превью…» — ожидание того, что не придёт.
    Unavailable,
}

/// Надпись на месте кадра, который не загрузился. Раньше и отказ, и ожидание
/// показывались одним «⏳ Загрузка скриншота…», и на отказе надпись висела до
/// закрытия окна.
pub fn gallery_failed_msg() -> String {
    tr("🚫 The screenshot didn't load.\nUse the ◀️ ▶️ arrows to see the other ones.")
}
/// Надпись на месте кадра, который ещё едет.
pub fn gallery_loading_msg() -> String {
    tr("⏳ Loading screenshot…")
}

/// Состояние текущего кадра галереи.
pub fn gallery_frame(
    index: i64,
    loaded: &BTreeMap<i64, String>,
    failed: &BTreeSet<i64>,
    finished: bool,
    total: i64,
) -> GalleryFrame {
    if let Some(path) = loaded.get(&index) {
        if !path.is_empty() {
            return GalleryFrame::Image(path.clone());
        }
    }
    if total <= 1 {
        if (total == 0 && finished) || failed.contains(&index) {
            return GalleryFrame::Unavailable;
        }
        return GalleryFrame::Unchanged;
    }
    // Пустые «дыры» между удачными кадрами: ссылки на них у магазина не было
    // вовсе — такой кадр тоже больше не появится.
    if failed.contains(&index) || finished {
        GalleryFrame::Failed
    } else {
        GalleryFrame::Loading
    }
}

/// Счётчик «2/5» поверх картинки: виден только когда кадров больше одного.
pub fn gallery_counter(index: i64, total: i64) -> Option<String> {
    if total > 1 {
        Some(format!("{}/{}", index + 1, total))
    } else {
        None
    }
}

/// На какой кадр встать, когда из кеша подхватились уже готовые картинки.
///
/// Фоновый прогрев кладёт первый кадр каждой видимой плитки на диск заранее;
/// при открытии товара он просто подхватывается — без сети. Если текущего
/// кадра среди готовых нет, а человек ещё не листал руками, встаём на самый
/// ранний готовый. `None` — оставить как есть.
pub fn adopt_gallery_index(
    loaded: &BTreeMap<i64, String>,
    index: i64,
    user_navigated: bool,
) -> Option<i64> {
    if loaded.is_empty() || user_navigated {
        return None;
    }
    if loaded.get(&index).is_some_and(|p| !p.is_empty()) {
        return None;
    }
    loaded.keys().next().copied()
}

// ── Главная кнопка деталей ─────────────────────────────────────────────

/// Вид главной кнопки: текст, подсказка и признак для таблицы стилей.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallButton {
    pub text: String,
    pub tooltip: String,
    /// Значение свойства `applied` для селекторов таблицы стилей.
    pub state: &'static str,
}

/// Состояния главной кнопки — те же, что у плитки каталога
/// ([`card_install_button`]).
///
/// Как в KDE: «Установить» только ставит; у поставленной темы та же кнопка —
/// «Применить» (`can_apply` — у вида есть применение,
/// [`store_item_can_apply`]); у включённой — «Применено», нажатие
/// переприменяет.
pub fn install_button(installed: bool, applied: bool, can_apply: bool) -> InstallButton {
    if applied {
        return InstallButton {
            text: tr("✅ Applied"),
            tooltip: tr("This theme is on now — click to apply it again"),
            state: "true",
        };
    }
    if installed && can_apply {
        return InstallButton {
            text: tr("🎨 Apply"),
            tooltip: tr("The theme is downloaded but not turned on — click to apply it"),
            state: "installed",
        };
    }
    if installed {
        return InstallButton {
            text: trc("theme", "✅ Installed"),
            tooltip: String::new(),
            state: "installed",
        };
    }
    InstallButton {
        text: tr("Install"),
        tooltip: String::new(),
        state: "false",
    }
}

/// Что делает нажатие кнопки товара магазина (плитка и главная кнопка
/// деталей — один вход).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreCardAction {
    /// Скачать и поставить, НЕ применяя.
    Install,
    /// Применить уже поставленное — по записи журнала, без скачивания.
    ApplyInstalled,
}

/// Решение кнопки товара: поставленный товар вида с применением —
/// применить; всё остальное — поставить (заново).
pub fn store_card_action(installed: bool, can_apply: bool) -> StoreCardAction {
    if installed && can_apply {
        StoreCardAction::ApplyInstalled
    } else {
        StoreCardAction::Install
    }
}

/// Есть ли у товара магазина «применить» — по виду его раздела. У шрифтов,
/// виджетов, плагинов обоев, дополнений и Conky кнопки «Применить» на плитке
/// нет: включаются они в своей сетке установленного.
pub fn store_item_can_apply(home: &Path, item: &ThemeItem) -> bool {
    categories::category_by_id(home, item.category_id)
        .is_some_and(|c| appearance_installer::kind_has_active_state(c.apply_kind.trim()))
}

/// Имя, под которым товар `content_id` стоит на диске, — по журналу
/// установок: первый путь записи, который ещё есть (`exists`). `None` —
/// записи нет или файлы темы сняты руками.
pub fn logged_theme_name(
    log: &Map<String, Value>,
    content_id: &str,
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let paths: Vec<String> = log
        .get(content_id)?
        .get("paths")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|p| exists(p))
        .map(str::to_string)
        .collect();
    log_theme_names(&paths).into_iter().next()
}

/// Живой [`logged_theme_name`]: журнал с диска.
pub fn live_logged_theme_name(home: &str, content_id: &str) -> Option<String> {
    let log = crate::install_journal::store_log_view(&live_install_log_path(home));
    logged_theme_name(&log, content_id, |p| Path::new(p).exists())
}

/// Цвета главной кнопки деталей — сестра `install_button` для QML.
///
/// Та же причина, что у `card_button_colors`: QSS в QML не работает, и
/// голые цвета нужны отдельно. Контракт значений тот же — с решёткой это
/// готовый цвет, без решётки имя токена скина, пусто — цвета нет.
///
/// ⚠️ «✓ Применено» у деталей приглушённое серо-зелёное, а не бирюзовое,
/// как у плитки: у панели деталей кнопка крупная и во всю ширину, и
/// кричащий цвет на ней читался бы как незакрытое дело. «✓ Установлено»
/// намеренно совпадает с плиткой — сетка и деталь не должны рассказывать
/// про товар разное.
pub fn detail_button_colors(state: &str) -> (&'static str, &'static str, &'static str) {
    match state {
        "true" => ("#3a5a3f", "#cfe8d2", ""),
        "installed" => ("", "#7fe3da", "#4fd1c5"),
        _ => ("accent_green", "#ffffff", ""),
    }
}

/// Значок перед сообщением о результате действия.
pub fn status_prefix(ok: bool) -> &'static str {
    if ok {
        "✅ "
    } else {
        "❌ "
    }
}

// ── Местные темы ───────────────────────────────────────────────────────

/// Имя темы для применения: у местных карточек оно зашито в номер
/// (`local:<вид>:<имя>`), у остальных — обычное название.
pub fn local_theme_name(id: &str, name: &str) -> String {
    if let Some(rest) = id.strip_prefix("local:") {
        if let Some((_, theme)) = rest.split_once(':') {
            return theme.to_string();
        }
    }
    name.to_string()
}

/// Почему тема старого формата не применится.
pub fn stale_global_msg(name: &str) -> String {
    trf(
        "❌ The theme “%1” uses the old Plasma 5 format (no metadata.json), and \
         Plasma 6 can't apply it. You need a version of the theme made for Plasma 6.",
        &[&name],
    )
}

// ── Подписи панели деталей ─────────────────────────────────────────────

/// Мета-строка товара: автор, лицензия, дата обновления, версия.
pub fn detail_meta_line(author: &str, license: &str, updated: &str, version: &str) -> String {
    let author = if author.is_empty() { "—" } else { author };
    let license = if license.is_empty() { "—" } else { license };
    let upd: String = updated.chars().take(10).collect();
    let upd = if upd.is_empty() { "—".to_string() } else { upd };
    // ⚠️ Починка оригинала (правило #16): букву «v» он приписывал всегда, а
    // магазин отдаёт версию и как «5.0», и как «v5.0» — во втором случае в
    // деталях печаталось «vv5.0».
    let ver = if version.is_empty() {
        String::new()
    } else if version.starts_with(['v', 'V']) {
        version.to_string()
    } else {
        format!("v{version}")
    };
    // TRANSLATORS: author • license • updated <date> <version>.
    trf("%1 • %2 • updated %3 %4", &[&author, &license, &upd, &ver])
}

/// Мета-строка с размером архива в конце: «автор • лицензия • обновлено …
/// • 13 КиБ». Размер неизвестен — строка как была.
pub fn detail_meta_with_size(meta_line: &str, download_size_kib: i64) -> String {
    let size = human_size_kib(download_size_kib);
    if size.is_empty() {
        meta_line.to_string()
    } else {
        format!("{} • {size}", meta_line.trim_end())
    }
}

/// Адрес страницы товара в магазине — тот, что магазин отдаёт полем
/// `detailpage` (`https://store.kde.org/p/<номер>`). Пусто — номера нет.
pub fn store_page_url(item_id: &str) -> String {
    if item_id.is_empty() {
        String::new()
    } else {
        format!("https://store.kde.org/p/{item_id}")
    }
}

/// Заголовок блока выбора файла в панели деталей товара магазина (у товара
/// несколько файлов).
pub fn detail_files_title() -> String {
    tr("Files of this item")
}
/// Пояснение под заголовком блока выбора файла.
pub fn detail_files_hint() -> String {
    tr("Rexon has picked the main archive. You can choose another file and press “Install”.")
}
/// Пометка у файла, который поставит кнопка «Установить».
pub fn detail_file_will_install() -> String {
    tr("will be installed")
}
/// Главная кнопка деталей, когда выбран не тот файл, что берётся
/// автоматически: ставит выбранный, даже если товар уже стоит.
pub fn detail_file_install_btn() -> String {
    tr("Install")
}

/// Строки блока выбора файла для страницы (JSON-массив): `name` — ключ
/// выбора (его страница отдаёт обратно в `installById`), `meta` — размер и
/// день выкладки для человека, `chosen` — файл автоматического выбора.
/// Файл один или товар не из магазина — `[]`, блока нет.
pub fn store_file_choices_json(item: &ThemeItem) -> String {
    let rows: Vec<Value> = ocs::file_choices(item)
        .into_iter()
        .map(|c| {
            let meta = [human_size_kib(c.size_kib), c.date]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            serde_json::json!({ "name": c.name, "meta": meta, "chosen": c.chosen })
        })
        .collect();
    Value::Array(rows).to_string()
}

/// Кнопка «Копировать ссылку» в карточке товара.
pub fn detail_copy_link_btn() -> String {
    tr("🔗 Copy Link")
}
pub fn detail_copy_link_tooltip() -> String {
    tr("Copy the address of this item's store page")
}

/// Строка состояния после копирования ссылки.
pub fn link_copied_status(url: &str) -> String {
    // TRANSLATORS: status line after "Copy Link"; %1 is the address.
    trf("Link copied: %1", &[&url])
}

/// Подпись со звёздами рейтинга.
pub fn detail_stars_label(rating_stars: f64) -> String {
    format!("⭐ {rating_stars:.1}")
}

/// Подпись со счётчиком скачиваний.
pub fn detail_downloads_label(downloads: i64) -> String {
    format!("{NBSP}{NBSP}{}", trf("⬇️ %1 downloads", &[&human_count(downloads)]))
}

/// Подпись «Категория / Путь установки».
pub fn detail_path_label(category_name: &str, install_path: &str) -> String {
    trf("Category: %1\nInstalled to: %2", &[&category_name, &install_path])
}

/// Заглушка, когда описания у товара нет вовсе.
pub fn no_description_html() -> String {
    format!("<i>{}</i>", tr("No description"))
}

/// Переключатель «перевод ⇄ оригинал» под текстом.
///
/// Цвет ссылки берётся из ДЕЙСТВУЮЩЕГО скина и подставляется в момент сборки:
/// это единственное место, где цвет остаётся в самом тексте, потому что
/// таблица стилей не красит фрагмент внутри подписи.
fn toggle_link(accent: &str, label: &str, point_size: &str, lead: &str) -> String {
    format!(
        "{lead}<a href='rexon:toggle' style='color:{accent};\
text-decoration:none;font-size:{point_size}'>{label}</a>"
    )
}

/// Тело комментария вместе с переключателем перевода.
pub fn comment_body_html(
    original: &str,
    translated: &str,
    was_translated: bool,
    show_original: bool,
    accent: &str,
) -> String {
    let body = if show_original { original } else { translated };
    if !was_translated {
        return body.to_string();
    }
    let label = if show_original { tr("🌐 translation") } else { tr("📄 original") };
    format!("{body}{}", toggle_link(accent, &label, "9pt", "&nbsp;&nbsp;"))
}

/// Смайлы магазина — знаком, а не картинкой из сети.
///
/// Магазин подставляет в описания `<img src="…/media/emoticons/<имя>.png">`.
/// Qt тянет такую картинку сам, и на каждую карточку это давало десяток строк
/// в журнале: сокет закрывается раньше ответа (`QSslSocket: device not open`),
/// данные приходят пустыми, дальше `Error decoding … Неподдерживаемый формат
/// изображения`. Сервер при этом исправен — PNG в 1 КБ отдаётся по тому же
/// адресу руками. Чинить сетевой путь ради картинки 30×30 незачем: знак
/// набирается шрифтом, виден всегда и не зависит от сети вовсе.
///
/// ⚠️Имена берутся из живого кеша магазина, а не из головы: в описаниях
/// встречаются ровно девять (`happy` 88 раз, `wink` 23, `cool` 17,
/// `ecstatic` 10, `tongue` 6, `gah` 4, `heart` 4, `sad` и `kiss` по одному).
/// Незнакомое имя — тег просто выбрасывается: пустое место честнее
/// сломанной картинки и уж точно честнее чужой рожицы наугад.
///
/// Заодно видно, откуда смайлы берутся: `(default 15&deg;)` магазин
/// прочитал как `;)` и подставил подмигивание. Это его разбор, не наш, и
/// поправить его мы не можем — но знак хотя бы читается.
pub fn inline_emoticons(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<img") {
        let Some(end_rel) = rest[start..].find('>') else { break };
        let end = start + end_rel + 1;
        let tag = &rest[start..end];
        out.push_str(&rest[..start]);
        if let Some(name) = emoticon_name(tag) {
            out.push_str(emoticon_char(name));
        } else if !tag.contains("/media/emoticons/") {
            // Не смайл — картинка описания, её не трогаем.
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Имя смайла из тега `<img>`, если это он.
fn emoticon_name(tag: &str) -> Option<&str> {
    let after = tag.split("/media/emoticons/").nth(1)?;
    let name = after.split(".png").next()?;
    if name.is_empty() || name.contains('"') || name.contains('/') {
        return None;
    }
    Some(name)
}

/// Знак вместо смайла магазина; незнакомое имя — пусто.
fn emoticon_char(name: &str) -> &'static str {
    match name {
        "happy" => "🙂",
        "wink" => "😉",
        "cool" => "😎",
        "ecstatic" => "😄",
        "tongue" => "😛",
        "gah" => "😩",
        "heart" => "❤️",
        "sad" => "🙁",
        "kiss" => "😘",
        _ => "",
    }
}

/// Описание товара вместе с переключателем перевода.
pub fn description_html(
    original: &str,
    translated: &str,
    was_translated: bool,
    show_original: bool,
    accent: &str,
) -> String {
    let body = if show_original { original } else { translated };
    let empty = no_description_html();
    let body = if body.is_empty() { empty.as_str() } else { body };
    let body = inline_emoticons(body);
    if !was_translated {
        return body;
    }
    let label = if show_original {
        tr("🌐 show translation")
    } else {
        tr("📄 show original")
    };
    format!("{body}{}", toggle_link(accent, &label, "9.5pt", "<br>"))
}

// ── Комментарии ────────────────────────────────────────────────────────

/// Сколько комментариев просим у магазина за раз.
pub const COMMENTS_PAGESIZE: usize = 10;

/// Насколько глубоко отбиваем ответы в ветке.
pub const COMMENT_INDENT_PX: i64 = 28;

/// Текст комментария из магазина → безопасный кусок разметки (теги вырезаны).
pub fn clean_comment_text(raw: &str) -> String {
    let text = translator::html_unescape(raw);
    // re.sub(r"<[^>]+>", " ", …): пара «<…>» с хотя бы одним знаком внутри.
    // Пустое «<>» правилу не подходит и остаётся текстом.
    let chars: Vec<char> = text.chars().collect();
    let mut stripped = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }
            if j < chars.len() && j > i + 1 {
                stripped.push(' ');
                i = j + 1;
                continue;
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    translator::html_escape(&stripped).replace('\n', "<br>")
}

/// Ник для сравнения «автор комментария == автор товара».
///
/// Магазин отдаёт автора товара одним полем, а автора комментария другим. На
/// pling это обычно один и тот же логин, но возможны расхождения регистра,
/// крайних пробелов и изредка отображаемого имени против логина. Сравниваем
/// без регистра и крайних пробелов, внутренние схлопываем. Полного
/// соответствия магазин не обещает, поэтому это честная эвристика: ошибка в
/// любую сторону стоит только цвета ника.
pub fn norm_author(name: &str) -> String {
    let squeezed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    // Оригинал складывает регистр `casefold`, а не `lower`: он вдобавок
    // приравнивает «ß» к «ss» и конечную сигму к обычной. Полной таблицы
    // складывания в стандартной библиотеке Rust нет, поэтому доводим эти два
    // случая руками — на латинице и на греческом других расхождений с
    // `lower` нет, а сравнение ников других алфавитов от них не зависит.
    squeezed
        .to_lowercase()
        .replace('\u{00df}', "ss")
        .replace('\u{03c2}', "\u{03c3}")
}

/// Комментирует ли САМ автор товара — его ник красится отдельно.
pub fn comment_is_product_author(comment_author: &str, product_author: &str) -> bool {
    !comment_author.is_empty() && norm_author(comment_author) == norm_author(product_author)
}

/// Ник комментатора для показа.
pub fn comment_author_display(author: &str) -> String {
    if author.is_empty() {
        "—".to_string()
    } else {
        author.to_string()
    }
}

/// Дата комментария — только день, без времени.
pub fn comment_date(raw: &str) -> String {
    raw.chars().take(10).collect()
}

/// Глубина ответа с потолком: глубже пятого уровня отступ съел бы всю ширину.
pub fn comment_depth(raw: i64) -> i64 {
    raw.clamp(0, 4)
}

/// Отступ ветки ответа.
pub fn comment_indent(depth: i64) -> i64 {
    comment_depth(depth) * COMMENT_INDENT_PX
}

/// Значок ответа перед ником.
pub fn comment_reply_prefix(depth: i64) -> &'static str {
    if comment_depth(depth) > 0 {
        "↪️ "
    } else {
        ""
    }
}

/// Есть ли ещё страница комментариев.
///
/// Общего числа магазин не отдаёт, поэтому считаем по корневым: пришла полная
/// страница — значит, скорее всего, есть и следующая.
pub fn comments_has_more(depths: &[i64]) -> bool {
    depths.iter().filter(|d| **d == 0).count() >= COMMENTS_PAGESIZE
}

/// Заголовок секции комментариев.
///
/// Точное число приходит с полными деталями товара; из кеша оно теряется —
/// тогда честное «показано N(+)», а не выдуманная точность.
pub fn comments_header_text(total: Option<i64>, shown: usize, has_more: bool) -> String {
    if let Some(n) = total {
        if n != 0 {
            return trf("💬 Comments (%1)", &[&n]);
        }
    }
    let plus = if has_more { "+" } else { "" };
    trf("💬 Comments (%1)", &[&format!("{shown}{plus}")])
}

/// Заголовок, когда комментариев нет вовсе.
pub fn comments_empty_header() -> String {
    tr("💬 Comments")
}
/// Подпись вместо пустого списка.
pub fn comments_empty_text() -> String {
    tr("No comments yet")
}

// ═══════════════════════════════════════════════════════════════════════
//  Запросы каталога: поколения, дедуп и фоновый прогрев
// ═══════════════════════════════════════════════════════════════════════

/// Что делать с отложенным запросом каталога, когда истёк дебаунс.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FireOutcome {
    /// Отправлять нечего.
    Nothing,
    /// Человек уже ушёл дальше — запрос обесценился, не отправляем.
    Stale,
    /// Ровно этот запрос уже летит: второй в сеть НЕ уходит, летящему просто
    /// обновлено поколение, и его ответ будет принят как свежий.
    Deduplicated { key: String },
    /// Отправлять.
    Send { key: String, generation: u64 },
}

/// Отложенный запрос каталога.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCatalog {
    pub generation: u64,
    pub key: String,
}

/// Кто сейчас летит за каталогом и что стоит в очереди на прогрев.
///
/// До переноса поколение лишь отбрасывало ЧУЖОЙ ответ, а сам запрос доживал и
/// занимал канал: замер показал до тринадцати одновременных запросов и рост
/// времени ответа с одной секунды до двенадцати. Здесь ушедший запрос честно
/// снимается, а дубль не отправляется вовсе.
#[derive(Debug, Default)]
pub struct CatalogRequests {
    generation: u64,
    pending: Option<PendingCatalog>,
    /// Ключ списка → поколение летящего за ним запроса.
    inflight: BTreeMap<String, u64>,
    /// Страницы, чей прогрев уже запускался: повторный заход в категорию не
    /// должен слать тот же запрос заново.
    prefetched_pages: BTreeSet<String>,
    prefetch_queue: Vec<(String, String)>,
    prefetch_done: BTreeSet<(String, String)>,
    prefetch_busy: bool,
    closing: bool,
}

impl CatalogRequests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Смена вида (раздел, источник, поиск, страница) обесценивает всё, что
    /// летит за прежнюю категорию. Возвращает новое поколение.
    pub fn bump(&mut self) -> u64 {
        self.generation += 1;
        self.pending = None;
        self.generation
    }

    /// Отложить запрос до конца дебаунса: пока человек пробегает список
    /// категорий мышью, сеть не трогаем.
    pub fn schedule(&mut self, generation: u64, key: &str) {
        self.pending = Some(PendingCatalog {
            generation,
            key: key.to_string(),
        });
    }

    /// Дебаунс истёк — что делать с отложенным запросом.
    pub fn fire(&mut self) -> FireOutcome {
        let Some(pending) = self.pending.take() else {
            return FireOutcome::Nothing;
        };
        if pending.generation != self.generation {
            return FireOutcome::Stale;
        }
        if let Some(live) = self.inflight.get_mut(&pending.key) {
            *live = pending.generation;
            return FireOutcome::Deduplicated { key: pending.key };
        }
        self.inflight.insert(pending.key.clone(), pending.generation);
        FireOutcome::Send {
            key: pending.key,
            generation: pending.generation,
        }
    }

    /// Ответ за `key` пришёл. `true` — он устарел и в сетку не попадёт.
    ///
    /// Снимает запрос из летящих в любом случае: оборванный вернётся сюда же
    /// обычным путём.
    pub fn answer(&mut self, key: &str) -> bool {
        match self.inflight.remove(key) {
            Some(gen) => gen != self.generation,
            // Прямой вызов (попадание в кеш) отправителя не имеет — такой
            // ответ всегда свежий.
            None => false,
        }
    }

    /// Каталог видимой категории сейчас грузится и ждёт сети?
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.inflight.is_empty()
    }

    /// Какие запросы оборвать при уходе из категории: все, кроме того, за
    /// которым мы сюда и пришли. Канал нужнее открытой категории.
    pub fn stale_keys(&self, keep: Option<&str>) -> Vec<String> {
        self.inflight
            .keys()
            .filter(|k| Some(k.as_str()) != keep)
            .cloned()
            .collect()
    }

    /// Стоит ли прогревать следующую страницу.
    ///
    /// Один прогрев на страницу за сессию: без этого каждый повторный заход в
    /// категорию слал тот же запрос заново и топил канал. Живой запрос
    /// человека за ту же страницу тоже считается прогревом.
    pub fn claim_page_prefetch(&mut self, key: &str) -> bool {
        if self.prefetched_pages.contains(key) || self.inflight.contains_key(key) {
            return false;
        }
        self.prefetched_pages.insert(key.to_string());
        true
    }

    /// Кеш списков сброшен вручную — прогревы страниц можно запускать заново.
    pub fn forget_page_prefetches(&mut self) {
        self.prefetched_pages.clear();
    }

    /// Вкладка закрывается: новых прогревов не выдаём.
    pub fn close(&mut self) {
        self.closing = true;
    }

    /// Добавить в хвост очереди прогрева всё, чего в ней ещё нет.
    pub fn enqueue_prefetch(&mut self, targets: &[(String, String)]) {
        for key in targets {
            if !self.prefetch_done.contains(key) && !self.prefetch_queue.contains(key) {
                self.prefetch_queue.push(key.clone());
            }
        }
    }

    /// Переставить соседей открытого раздела в начало очереди.
    pub fn prioritize_prefetch(&mut self, front: &[(String, String)]) {
        if front.is_empty() {
            return;
        }
        self.prefetch_queue = prefetch_reorder(front, &self.prefetch_queue);
    }

    /// Уже прогретые пары — для отбора соседей.
    pub fn prefetch_done(&self) -> &BTreeSet<(String, String)> {
        &self.prefetch_done
    }

    pub fn prefetch_queue_len(&self) -> usize {
        self.prefetch_queue.len()
    }

    /// Следующая цель прогрева — по одной за раз.
    ///
    /// Живой запрос человека важнее фонового прогрева: пока категория, на
    /// которую он смотрит, ещё грузится, канал ему не занимаем. Очередь при
    /// этом не трогаем — она догонит после ответа.
    pub fn next_prefetch(&mut self) -> Option<(String, String)> {
        if self.closing || self.prefetch_busy || self.busy() {
            return None;
        }
        while let Some(key) = self.prefetch_queue.first().cloned() {
            self.prefetch_queue.remove(0);
            if self.prefetch_done.contains(&key) {
                continue;
            }
            self.prefetch_busy = true;
            return Some(key);
        }
        None
    }

    /// Прогрев цели закончился (удачно или нет — кеш просто не наполнится).
    pub fn prefetch_finished(&mut self, key: (String, String)) {
        self.prefetch_busy = false;
        self.prefetch_done.insert(key);
    }
}

// ── Подсказки поиска ───────────────────────────────────────────────────

/// Что делать с подсказками при текущем вводе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestAction {
    /// Убрать выпадашку.
    Hide,
    /// Сети нет — фильтруем уже загруженный список на месте, мгновенно.
    Local,
    /// Спросить магазин.
    Network,
}

/// Нужны ли подсказки и откуда их брать.
///
/// Посимвольно магазин не дёргаем: до порога длины выпадашка просто закрыта.
/// На панели деталей подсказок нет — там нет и сетки, к которой они ведут.
pub fn suggest_action(query: &str, on_grid: bool, view: &CatalogView) -> SuggestAction {
    if query.trim().chars().count() < SUGGEST_MIN_CHARS || !on_grid {
        return SuggestAction::Hide;
    }
    // Cinnamon Spices подсказок не даёт, а подсказки KDE Store вели бы не
    // в тот магазин — ищется по своей витрине.
    if view.is_local() || view.aur_mode() || view.spices_mode() {
        SuggestAction::Local
    } else {
        SuggestAction::Network
    }
}

/// Принимать ли ответ с подсказками.
///
/// Человек печатал дальше (выросло поколение), ушёл из строки поиска или
/// стёр введённое — ответ уже не про то, что он видит.
pub fn suggest_answer_fresh(
    answer_generation: u64,
    current_generation: u64,
    search_focused: bool,
    query: &str,
) -> bool {
    answer_generation == current_generation
        && search_focused
        && query.trim().chars().count() >= SUGGEST_MIN_CHARS
}

// ── Подписи состояния каталога ─────────────────────────────────────────

/// Строка «Найдено: N · показано M» либо честное «ничего».
pub fn catalog_status(total: i64, shown: usize) -> String {
    if shown == 0 {
        return tr("Nothing found");
    }
    trf("Found: %1 · showing %2", &[&total, &shown])
}

/// Строка состояния, пока действует поиск: видно, ЧТО отбирает раздел.
///
/// 19.10.2026 в поле осталось «MishMty» от прошлого поиска, раздел
/// показывал одну плитку, а строка писала обычное «Найдено» — казалось,
/// что магазин пропал.
pub fn search_status(query: &str, found: i64) -> String {
    if found <= 0 {
        return trf("Search: “%1” — nothing found", &[&query]);
    }
    trf("Search: “%1” — found %2", &[&query, &found])
}

/// Подсказка кнопки ✕ у строки поиска.
pub fn search_clear_tooltip() -> String {
    tr("Clear search")
}

/// Пока едет страница каталога.
pub fn status_loading_catalog() -> String {
    tr("⏳ Loading catalog…")
}
/// Пока едет список AUR.
pub fn status_loading_aur() -> String {
    tr("⏳ Loading AUR…")
}
/// Пока идёт скан диска.
pub fn status_loading_scan() -> String {
    tr("⏳ Loading…")
}
/// Пока идёт классификация системных тем (она дёргает пакетный менеджер).
pub fn status_scanning_system() -> String {
    tr("🖥️ Looking for system themes…")
}

/// Подпись во время скана: системные темы честно предупреждают о задержке.
pub fn scan_busy_status(which: &str) -> String {
    if which == "system" {
        status_scanning_system()
    } else {
        status_loading_scan()
    }
}

/// Системные темы не удалось классифицировать достоверно.
///
/// Список тогда НЕ показываем вовсе: иначе темы из AUR протекли бы в раздел
/// «Системные» и человек принял бы их за часть дистрибутива.
pub fn status_system_scan_unavailable() -> String {
    tr("⚠️ Couldn't tell where packages came from — system themes are hidden (so that \
        AUR themes are not shown as system ones). Try again later.")
}

// ── Ход установки ──────────────────────────────────────────────────────

/// Первая строка журнала установки: работа началась, но что именно делается,
/// установщик ещё не сказал.
pub fn install_preparing() -> String {
    tr("⬇️ Preparing…")
}

/// Отказ установки: архив темы не скачался.
///
/// Причины у сети свои и человеку не нужны — ему нужен ответ на нажатую
/// кнопку. Строка одна на обе версии вкладки.
pub fn install_download_failed() -> String {
    tr("Couldn't download the archive")
}

/// Виды долгого действия над товаром: у каждого своя подпись, и полоса хода
/// бывает не у всех.
pub const ACTION_INSTALL: &str = "install";
pub const ACTION_AUR: &str = "aur";
pub const ACTION_APPLY: &str = "apply";
/// «Установить из файла…»: доли выполненного у неё нет (распаковка и
/// копирование идут без счёта), подпись — как у установки.
pub const ACTION_LOCAL: &str = "local";

/// Подпись в строке состояния на время долгого действия.
pub fn action_busy_status(kind: &str, name: &str) -> String {
    match kind {
        ACTION_AUR => trf("🔨 Building “%1” from AUR…", &[&name]),
        ACTION_APPLY => trf("🎨 Applying “%1”…", &[&name]),
        _ => trf("⬇️ Installing “%1”…", &[&name]),
    }
}

/// Пересобирать ли сетку после того, как действие кончилось.
///
/// Только применение темы: активная тема сменилась, и бейдж «✅ Активна»
/// обязан переехать на неё. Установка сетку не трогает (там меняется одна
/// плитка), а удаление перестраивает её сразу — ждать нечего.
///
/// ⚠ И ждать перед пересборкой ОБЯЗАТЕЛЬНО (`STYLE_STORM_MS`): применение
/// полной темы поднимает в НАШЕМ же процессе смену стиля Qt по D-Bus, и
/// пересборка прямо в этот момент валила приложение.
pub fn reload_after_action(kind: &str, ok: bool) -> bool {
    ok && kind == ACTION_APPLY
}

/// Показывать ли полосу хода.
///
/// Долю выполненного отдаёт только загрузка из магазина. Помощник AUR и
/// применение темы её не считают вовсе, и полоса на нуле врала бы про
/// «идёт, вот столько» — им остаются строки журнала.
pub fn action_shows_progress(kind: &str) -> bool {
    kind == ACTION_INSTALL
}

/// Доля выполненного для полосы хода: установщик изредка отдаёт и больше ста,
/// и меньше нуля, а полоса обязана оставаться полосой.
pub fn install_percent(pct: i64) -> i64 {
    pct.clamp(0, 100)
}

/// Итог действия со значком впереди.
///
/// ⚠ Значок ставится, только если сообщение ещё не начинается с него.
/// Установщик отдаёт готовые «✅ Установлено: …», и безусловный второй
/// значок давал «✅ ✅ Установлено» — в оригинале это чинили в строке
/// состояния, но не на панели деталей, где двойной значок так и остался
/// (правило #12: чиним в одном месте для всех).
pub fn action_result_status(ok: bool, msg: &str) -> String {
    let text = msg.trim_start();
    if text.starts_with('✅') || text.starts_with('❌') {
        return msg.to_string();
    }
    format!("{}{msg}", status_prefix(ok))
}

/// Подпись раздела «Избранное».
pub fn favorites_status(n: usize) -> String {
    trf("In favorites: %1", &[&n])
}

/// Подпись сетки установленного: заголовок над плитками и строка состояния.
pub fn installed_status(n: usize) -> String {
    trf("Installed: %1", &[&n])
}

/// Строка состояния раздела, где установленное и магазин на одной
/// странице: обе части; установленного нет — только магазин.
pub fn combined_status(installed: usize, total: i64, store_shown: usize) -> String {
    let store = catalog_status(total, store_shown);
    if installed == 0 {
        store
    } else {
        format!("{} · {store}", installed_status(installed))
    }
}

/// Подпись местного раздела, у которого своей нет: имя раздела и счёт.
pub fn local_category_status(category_name: &str, n: usize) -> String {
    format!("{category_name}: {n}")
}

// ═══════════════════════════════════════════════════════════════════════
//  Плитки каталога: подписи, подсказки и виды кнопок
// ═══════════════════════════════════════════════════════════════════════

/// Название на плитке (у товара без имени — прочерк, а не пустая строка).
pub fn card_name(name: &str) -> &str {
    if name.is_empty() {
        "—"
    } else {
        name
    }
}

/// Автор на плитке.
pub fn card_author(author: &str) -> &str {
    card_name(author)
}

/// Вид товара магазина — той же подписью, что у плиток «Активных»
/// («📁 Иконки»): по разделу его категории магазина. Вид без строки скана
/// (обои, шрифты, виджеты) — значок и имя раздела. `None` — категория
/// неизвестна.
pub fn store_item_kind_label(home: &Path, item: &ThemeItem) -> Option<String> {
    let cat = categories::category_by_id(home, item.category_id)?;
    let h = home.to_string_lossy();
    let kind = cat.apply_kind.trim();
    let scan = installed_scan::types_for(&h, "/usr")
        .into_iter()
        .chain(installed_scan::de_types(&h, "/usr", ""))
        .find(|t| t.apply_kind == kind);
    Some(match scan {
        Some(t) => format!("{} {}", t.icon, installed_scan::type_label(&t)),
        None => format!("{} {}", cat.icon, cat.name),
    })
}

/// Строка автора на плитке товара: в витринах «Новые»/«Популярные»
/// (`showcase`) — «вид · автор», иначе — как есть.
pub fn showcase_author(home: &Path, item: &ThemeItem, showcase: bool) -> String {
    let author = card_author(&item.author).to_string();
    if !showcase {
        return author;
    }
    match store_item_kind_label(home, item) {
        Some(kind) if author.is_empty() => kind,
        Some(kind) => format!("{kind} · {author}"),
        None => author,
    }
}

/// Стат-строка плитки магазина: рейтинг · скачивания · дата обновления.
///
/// Набор слотов ЕДИНЫЙ с плиткой AUR (★/⬇/📅 против 👍/📈/📅) — разнобой
/// магазинов в одной сетке недопустим.
pub fn card_stats_line(rating_stars: f64, downloads: i64, updated: &str) -> String {
    let day: String = updated.chars().take(10).collect();
    let upd = if day.is_empty() {
        String::new()
    } else {
        format!("{NBSP}{NBSP}📅 {day}")
    };
    format!(
        "{NBSP}{rating_stars:.1}{NBSP}{NBSP}⬇️ {}{upd}",
        human_count(downloads)
    )
}

/// Подсказка стат-строки плитки магазина (`None` — даты у товара нет).
pub fn card_updated_tooltip(updated: &str) -> Option<String> {
    let day: String = updated.chars().take(10).collect();
    if day.is_empty() {
        None
    } else {
        Some(trf("Updated: %1", &[&day]))
    }
}

/// Вид кнопки на плитке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardButton {
    pub text: String,
    /// Значение свойства для селекторов таблицы стилей.
    pub state: &'static str,
    pub enabled: bool,
}

/// Три состояния кнопки плитки магазина: действие / установлено / применено.
///
/// «Установить» — залитая ЗЕЛЁНАЯ кнопка-действие. «✓ Установлено» —
/// КОНТУРНАЯ бирюзовая: другой силуэт, а не только другой оттенок, чтобы
/// состояние не читалось как ещё одно предложение установить. «✓ Применено» —
/// ЗАЛИТАЯ бирюзовая: тема не просто лежит на диске, а включена прямо сейчас.
/// Так все три плитки различимы в одной сетке, даже не читая подпись.
///
/// Кнопка во всех состояниях остаётся нажимаемой: переустановить и
/// переприменить — обычные желания.
pub fn card_install_button(installed: bool, applied: bool, can_apply: bool) -> CardButton {
    if installed && applied {
        return CardButton { text: tr("✅ Applied"), state: "applied", enabled: true };
    }
    // Как в KDE: поставленная, но не включённая тема — «Применить».
    if installed && can_apply {
        return CardButton { text: tr("🎨 Apply"), state: "true", enabled: true };
    }
    if installed {
        return CardButton { text: trc("theme", "✅ Installed"), state: "true", enabled: true };
    }
    CardButton { text: tr("Install"), state: "false", enabled: true }
}

/// Кнопка местной плитки: включённая тема показывает бейдж, остальные —
/// предложение применить.
pub fn card_local_button(local_kind: &str) -> CardButton {
    if local_kind == "active" {
        // ⚠️ Состояние «false», а не «applied», и это не описка. Бирюзовый
        // «applied» носит плитка МАГАЗИНА: там он отличает применённый товар
        // от просто скачанного. У местной плитки такого соседа нет — раздел
        // «Активные» состоит из применённых целиком, — и оригинал на
        // виджетах рисует бейдж обычной зелёной кнопкой (`#InstallBtn` без
        // свойства `installed`). Бирюзовый бейдж был отсебятиной переноса:
        // замер снимков 01.09.2026 — заливка (43,129,126) против (91,163,43)
        // у оригинала.
        CardButton { text: local_active_badge(), state: "false", enabled: false }
    } else {
        CardButton { text: tr("🎨 Apply"), state: "false", enabled: true }
    }
}

/// Кнопка местной плитки с учётом вида раздела.
///
/// Темы — как [`card_local_button`]: «🎨 Применить», у включённой — бейдж.
/// У дополнений окружений и виджетов Conky включённых может быть сколько
/// угодно, и у включённого кнопка не бейдж, а обратное действие:
/// «⏸ Выключить» / «⏹ Остановить» (решение 04.10.2026). Вид — из
/// `de_apply` по номеру плитки `local:<вид>:<имя>`, отдельного списка
/// разделов здесь нет.
pub fn card_local_button_for(local_kind: &str, item_id: &str) -> CardButton {
    use crate::de_apply::{apply_kind_of_local_id, button_for_apply_kind, Button};
    let button = apply_kind_of_local_id(item_id)
        .map(button_for_apply_kind)
        .unwrap_or(Button::Apply);
    let on = local_kind == "active";
    match button {
        Button::Apply => card_local_button(local_kind),
        // Включённое — контурная кнопка состояния, выключенное — залитая
        // кнопка-действие, как у остальных плиток.
        Button::Toggle if on => CardButton { text: tr("⏸️ Disable"), state: "true", enabled: true },
        Button::Toggle => CardButton { text: tr("▶️ Enable"), state: "false", enabled: true },
        Button::Run if on => CardButton { text: tr("⏹️ Stop"), state: "true", enabled: true },
        Button::Run => CardButton { text: tr("▶️ Start"), state: "false", enabled: true },
    }
}

/// Тему экрана входа SDDM нельзя включить, пока вход рисует Plasma Login
/// Manager: «Применить» на плитке приглушено и объясняет причину подсказкой
/// ([`sddm_under_plasma_login_reason`]); нажатие по-прежнему доходит до
/// установщика, и тот отказывает теми же словами ещё до окна пароля
/// (`appearance_installer::root_preflight`). Удалить такую тему можно.
/// `plasma_login` — вход рисует Plasma Login Manager
/// (`system_plan::is_plasma_login`).
pub fn sddm_blocked_by_plasma_login(item_id: &str, plasma_login: bool) -> bool {
    plasma_login && crate::de_apply::apply_kind_of_local_id(item_id) == Some("sddm")
}

/// Подсказка на недоступном «Применить» у темы SDDM — заголовок панели
/// раздела «Экран входа» (те же слова, что человек видит там).
pub fn sddm_under_plasma_login_reason() -> String {
    plasma_login_panel(false).title
}

/// Кнопка действия на панели деталей местной темы: `(подпись, можно ли
/// нажать)`. Та же развилка, что у плитки ([`card_local_button_for`]):
/// у включённой ТЕМЫ — бейдж «✅ Активна», у дополнения и виджета —
/// обратное действие.
pub fn detail_local_button(local_kind: &str, item_id: &str) -> (String, bool) {
    use crate::de_apply::{apply_kind_of_local_id, button_for_apply_kind, Button};
    let button = apply_kind_of_local_id(item_id)
        .map(button_for_apply_kind)
        .unwrap_or(Button::Apply);
    if button == Button::Apply {
        return if local_kind == "active" {
            (local_applied_btn(), false)
        } else {
            (detail_apply_local_btn(), true)
        };
    }
    let card = card_local_button_for(local_kind, item_id);
    (card.text, true)
}

/// Виден ли на панели деталей переключатель «Запускать при входе» — только
/// у виджетов Conky (у них запуск отдельно от автозапуска).
pub fn detail_autostart_visible(item_id: &str) -> bool {
    use crate::de_apply::{apply_kind_of_local_id, button_for_apply_kind, Button};
    apply_kind_of_local_id(item_id).map(button_for_apply_kind) == Some(Button::Run)
}

/// Подпись переключателя автозапуска виджета.
pub fn detail_autostart_text() -> String {
    tr("Start at login")
}

/// Местные плитки, которые имеют смысл в выбранном рабочем столе: раздел
/// плитки доступен в нём (`categories::category_available`). Плитка
/// неизвестного раздела остаётся — прятать без причины нельзя.
///
/// Без этого в «Загруженных» на XFCE лежали бы рамки Aurorae с кнопкой
/// «Применить», которая пишет настройки KWin, — честнее не показывать
/// кнопку там, где тип неприменим (решение 04.10.2026).
pub fn local_items_for_de(items: Vec<ThemeItem>, home: &Path, de: &str) -> Vec<ThemeItem> {
    if de.is_empty() {
        return items;
    }
    let cats = categories::categories(home);
    items
        .into_iter()
        .filter(|it| {
            // Номер товара подкатегории (виджет часов — 708) ведёт в свой
            // раздел («Виджеты», 705), как у `categories::category_by_id`.
            let section = categories::section_id_for(it.category_id);
            cats.iter()
                .find(|c| c.id == it.category_id)
                .or_else(|| cats.iter().find(|c| c.id == section))
                .is_none_or(|c| categories::category_available(&c.key, de))
        })
        .collect()
}

/// Цвета кнопки плитки по её состоянию — сестра `card_install_button` для
/// QML.
///
/// ⚠️ Таблица стилей (`page_qss`) — это QtWidgets, в QML она не работает
/// вовсе. Поэтому рядом с QSS-строкой заводится сестра, отдающая ГОЛЫЕ
/// цвета, а сторож сверки следит, чтобы они не разъехались
/// (`tests/appearance_page_parity.rs`). Решение о цвете остаётся одно,
/// разными остаются только обёртки.
///
/// Порядок: заливка, надпись, рамка. Значение с решёткой — готовый цвет,
/// без решётки — ИМЯ ТОКЕНА действующего скина (его подставит вызывающий из
/// `colors()`); пустая строка — цвета нет вовсе (прозрачно / без рамки).
pub fn card_button_colors(state: &str) -> (&'static str, &'static str, &'static str) {
    match state {
        // «✓ Применено» — ЗАЛИТАЯ бирюзовая (рамка того же цвета, чтобы
        // кнопка не «прыгала» в размере против контурного вида).
        "applied" => ("#2f9e94", "#ffffff", "#2f9e94"),
        // «✓ Установлено» — КОНТУРНАЯ бирюзовая: залитая зелёная кнопка
        // читается как предложение действия, контур — как состояние.
        "true" => ("", "#7fe3da", "#4fd1c5"),
        // «Установить» / «🎨 Применить» — залитая зелёная кнопка-действие
        // цветом действующего скина.
        _ => ("accent_green", "#ffffff", ""),
    }
}

/// Краски контрола, который таблица стилей ВКЛАДКИ перекрывает поверх
/// палитры Qt.
///
/// ⚠️ ЗАЧЕМ ОТДЕЛЬНАЯ ТАБЛИЦА, а не общие поверхности из `ui_base`. Общие
/// поверхности (`surface_button`, `surface_field`) отвечают на вопрос «чем
/// Qt красит контрол САМА по роли палитры». Это верно ровно до тех пор,
/// пока у страницы нет своей таблицы стилей. У «Оформления» она есть, и
/// роль в ней не участвует вовсе: выпадающий список красится токеном
/// `bg_secondary`, полоса страниц — `bg_card`, второстепенная кнопка —
/// `bg_card_hover`. Замер 05.09 (стенд, раздел «Стиль приложений»): QML
/// клал в те же места роль `Button` (42,64,85) и расходился с оригиналом на
/// 6 475 точек у выпадающего списка, 2 339 у кнопки обновления и 2 121 у
/// «Избранного».
///
/// Значения — ИМЕНА ТОКЕНОВ действующего скина (их подставит вызывающий из
/// `colors()`), тот же контракт, что у [`card_button_colors`]; пустая
/// строка — цвета нет.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSkin {
    /// Заливка в покое.
    pub bg: &'static str,
    /// Заливка под курсором.
    pub hover: &'static str,
    /// Рамка в покое.
    pub border: &'static str,
    /// Рамка под курсором.
    pub hover_border: &'static str,
    /// Надпись.
    pub fg: &'static str,
    /// Скругление, точек.
    pub radius: i32,
    /// Поле слева и справа, точек.
    pub pad_h: i32,
    /// Поле сверху и снизу, точек.
    pub pad_v: i32,
}

/// Краски контролов вкладки по виду. Неизвестный вид — как `nav`.
///
/// Виды повторяют группы правил из [`page_qss`] один в один:
///   * `combo` — `#SortCombo`;
///   * `nav` — `#RefreshBtn, #PageBtn, #BackBtn`;
///   * `secondary` — `#DetailsBtn, #DownloadOnlyBtn, #CommentsMoreBtn`.
pub fn control_colors(kind: &str) -> ControlSkin {
    match kind {
        // Выпадающий список: своей подсветки под курсором у него в таблице
        // стилей нет — заливка и рамка в покое и под курсором одни и те же.
        "combo" => ControlSkin {
            bg: "bg_secondary",
            hover: "bg_secondary",
            border: "border",
            hover_border: "border",
            fg: "text_primary",
            radius: 4,
            pad_h: 8,
            pad_v: 5,
        },
        // Второстепенная кнопка («Избранное», «Только скачать», «Показать
        // ещё»): под курсором меняется одна рамка, заливка стоит на месте.
        "secondary" => ControlSkin {
            bg: "bg_card_hover",
            hover: "bg_card_hover",
            border: "border",
            hover_border: "accent",
            fg: "text_primary",
            radius: 3,
            pad_h: 10,
            pad_v: 6,
        },
        // Обновление, страницы каталога, «Назад».
        _ => ControlSkin {
            bg: "bg_card",
            hover: "bg_card_hover",
            border: "border",
            hover_border: "accent",
            fg: "text_primary",
            radius: 4,
            pad_h: 12,
            pad_v: 6,
        },
    }
}

/// Кнопка «Избранное»: подпись и признак для таблицы стилей.
pub fn favorite_button(on: bool) -> CardButton {
    if on {
        CardButton { text: tr("⭐ Favorite"), state: "on", enabled: true }
    } else {
        CardButton { text: tr("☆ Favorite"), state: "off", enabled: true }
    }
}

// ── Плитка AUR ─────────────────────────────────────────────────────────

/// Пометка устаревшего пакета в строке источника.
pub fn aur_out_of_date() -> String {
    format!("  {}", tr("⚠️ out of date"))
}

/// Строка источника плитки AUR: бейдж и версия.
///
/// Бейдж обязателен: в режиме «Все» плитки AUR идут вперемешку с магазинными,
/// и человек должен видеть, откуда возьмётся пакет. `version` приходит уже
/// обрезанным по фактической ширине — обрезка меряется в пикселях и живёт в
/// Qt-слое, здесь только состав строки.
pub fn aur_meta_line(version: &str, out_of_date: bool) -> String {
    let ver = if version.is_empty() { "—" } else { version };
    let ood = if out_of_date { aur_out_of_date() } else { String::new() };
    format!("🛠️ AUR · {ver}{ood}")
}

/// Сколько ширины остаётся версии в строке источника: полная ширина подписи
/// минус то, что займут бейдж и пометка. Меньше сорока не отдаём — иначе от
/// версии не останется ничего читаемого.
pub fn aur_version_budget(text_width: i32, fixed_width: i32) -> i32 {
    40.max(text_width - fixed_width)
}

/// Каркас строки источника, по которому Qt-слой меряет ширину неизменяемой
/// части (сама версия в него не входит).
pub fn aur_meta_frame(out_of_date: bool) -> String {
    format!("🛠️ AUR · {}", if out_of_date { aur_out_of_date() } else { String::new() })
}

/// Подсказка строки источника.
pub fn aur_version_tooltip(version: &str) -> String {
    trf("Version: %1", &[&if version.is_empty() { "—" } else { version }])
}

/// Стат-строка плитки AUR: голоса · популярность · дата обновления.
///
/// `updated_day` — уже готовая дата: перевод отметки времени в календарный
/// день зависит от часового пояса машины, и делать его здесь значило бы
/// заводить в ядре свою таблицу поясов.
pub fn aur_stats_line(votes: i64, popularity: f64, updated_day: &str) -> String {
    let pop = if popularity != 0.0 {
        format!("{NBSP}{NBSP}📈 {popularity:.1}")
    } else {
        String::new()
    };
    let upd = if updated_day.is_empty() {
        String::new()
    } else {
        format!("{NBSP}{NBSP}📅 {updated_day}")
    };
    format!("{NBSP}{}{pop}{upd}", human_count(votes))
}

/// Подсказка стат-строки плитки AUR.
pub fn aur_stats_tooltip(votes: i64, popularity: f64, updated_day: &str) -> String {
    let upd = if updated_day.is_empty() { "—" } else { updated_day };
    trf(
        "👍 %1 votes · 📈 AUR popularity %2 · updated %3",
        &[&votes, &format!("{popularity:.2}"), &upd],
    )
}

/// Вид кнопки установки на плитке AUR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AurButton {
    pub text: String,
    pub enabled: bool,
    pub tooltip: String,
}

/// Установить пакет AUR нечем: ни paru, ни yay в системе нет. Одна строка на
/// подсказку кнопки и на итог попытки установки — теми же словами, что отказ
/// пересборки оформления (`aeroshell::REBUILD_NO_HELPER`), без «хелпера».
pub fn aur_no_builder() -> String {
    tr("No AUR package builder found (paru or yay) — installation is unavailable")
}

/// Подсказка кнопки установки пакета AUR.
fn aur_build_hint() -> String {
    tr("Built from source — this can take a while")
}

/// Кнопка установки пакета AUR.
///
/// Без помощника (yay/paru) ставить нечем — говорим это подсказкой, а не
/// молчаливо мёртвой кнопкой (правило #13).
pub fn aur_install_button(installed: bool, can_install: bool) -> AurButton {
    if installed {
        return AurButton {
            text: trc("AUR package", "✅ Installed"),
            enabled: false,
            tooltip: aur_build_hint(),
        };
    }
    if !can_install {
        return AurButton {
            text: tr("Install"),
            enabled: false,
            tooltip: aur_no_builder(),
        };
    }
    AurButton {
        text: tr("Install"),
        enabled: true,
        tooltip: aur_build_hint(),
    }
}

/// Кнопка во время сборки пакета.
pub fn aur_busy_button(busy: bool) -> AurButton {
    if busy {
        AurButton { text: tr("⏳ Building…"), enabled: false, tooltip: String::new() }
    } else {
        AurButton {
            text: tr("Install"),
            enabled: true,
            tooltip: aur_build_hint(),
        }
    }
}

// ── Подписи панели деталей ─────────────────────────────────────────────
//
// Оставались в Qt-слое до переноса вкладки на QML: подпись — такое же
// решение, как цвет и порядок, и вторая её копия в разметке разъехалась бы
// с первой молча. Сторож — `appearance_page_parity.rs`.

/// Возврат из панели деталей к сетке. Двойной пробел — как в оригинале.
pub fn detail_back_btn() -> String {
    tr("←  Back to Catalog")
}
/// Снять установленную тему.
pub fn detail_uninstall_btn() -> String {
    tr("🗑️ Remove")
}
/// Открыть страницу товара в магазине.
pub fn detail_open_site_btn() -> String {
    tr("🌐 Open Website")
}
pub fn detail_open_site_tooltip() -> String {
    tr("Official page on pling.com (description, comments)")
}
/// Применить тему, которая уже лежит на диске.
pub fn detail_apply_local_btn() -> String {
    tr("🎨 Apply")
}
/// Подсказка картинки: по ней открывается полноэкранный просмотр.
pub fn detail_preview_tooltip() -> String {
    tr("Click the picture to view it full screen")
}
/// Пока картинка едет.
pub fn detail_preview_loading() -> String {
    tr("Loading preview…")
}
/// Картинки у товара нет вовсе.
pub fn detail_preview_unavailable() -> String {
    tr("No preview available")
}
/// Накладные контролы поверх картинки.
pub const OVERLAY_ZOOM_BTN: &str = "🔍";
pub fn overlay_zoom_tooltip() -> String {
    tr("View full screen")
}
pub const OVERLAY_PREV_BTN: &str = "◀️";
pub fn overlay_prev_tooltip() -> String {
    tr("Previous screenshot")
}
pub const OVERLAY_NEXT_BTN: &str = "▶️";
pub fn overlay_next_tooltip() -> String {
    tr("Next screenshot")
}
/// Дочитать ветку комментариев.
pub fn comments_more_btn() -> String {
    tr("Show More")
}

/// Подпись кнопки «открыть страницу пакета».
pub fn aur_open_btn() -> String {
    tr("🌐 Open in AUR")
}
/// Подсказка к ней.
pub fn aur_open_tooltip() -> String {
    tr("Package page on aur.archlinux.org")
}

// ── Прослушивание тем звуков ────────────────────────────────────────────
//
// Паритет с модулем KDE «Системные звуки»: у каждой темы ряд «Прослушать:»
// из пяти кнопок-значков (звонок, предупреждение, сообщение, батарея,
// устройство — набор и запасные события в `sound_preview::PREVIEW_SLOTS`) и
// «▶» — все пять по очереди. У товара Магазина ряд появляется после
// «Прослушать до установки»: архив качается во временный кеш, ничего не
// ставится.

/// OCS-раздел «Звуки» (`categories::categories`, ключ `sounds`).
pub const SOUND_CATEGORY_ID: i64 = 316;

/// Тема звуков на диске, которую показывает местная плитка
/// (`local:sounds:<каталог>`); `None` — плитка не тема звуков.
pub fn local_sound_theme(item_id: &str) -> Option<String> {
    item_id
        .strip_prefix("local:sounds:")
        .filter(|n| !n.is_empty() && !n.contains('/'))
        .map(str::to_string)
}

/// Что плитка умеет показать во весь экран: заставка KDE на диске —
/// «Продемонстрировать» (`ksplashqml <тема> --test`), знак — имя темы; тема
/// SDDM — «Предпросмотр» экрана входа в тестовом режиме приветствия, знак —
/// `sddm:<тема>` ([`crate::system_plan::login_preview_argv_live`]). У
/// плитки «Нет» и прочих — `None`.
pub fn local_splash_demo(item_id: &str) -> Option<String> {
    if let Some(theme) = item_id.strip_prefix("local:sddm:") {
        let ok = !theme.is_empty() && !theme.contains('/') && !theme.starts_with('-');
        return ok.then(|| format!("{LOGIN_PREVIEW_PREFIX}{theme}"));
    }
    item_id
        .strip_prefix("local:splash:")
        .filter(|n| crate::appearance_installer::splash_demo_argv(n).is_some())
        .map(str::to_string)
}

/// Знак кнопки показа на плитке темы SDDM (см. [`local_splash_demo`]).
pub const LOGIN_PREVIEW_PREFIX: &str = "sddm:";

/// Товар Магазина из раздела «Звуки» — его можно послушать до установки.
pub fn is_store_sound_item(item: &ThemeItem) -> bool {
    !item.id.starts_with("local:") && item.category_id == SOUND_CATEGORY_ID
}

/// Подпись ряда кнопок прослушивания.
pub fn sound_preview_label() -> String {
    tr("Preview:")
}
/// Подсказка кнопки одного звука; `event` — имя события freedesktop, как в
/// KDE («battery-caution»).
pub fn sound_preview_tooltip(event: &str) -> String {
    // TRANSLATORS: %1 is a freedesktop sound event name such as battery-caution.
    trf("Preview sound “%1”", &[&event])
}
/// Подсказка «▶».
pub fn sound_play_all_tooltip() -> String {
    tr("Play all five sounds in turn")
}
/// Подсказка выключенной кнопки: такого звука нет ни в теме, ни у её
/// родителей, ни в `freedesktop`.
pub fn sound_missing_tooltip() -> String {
    tr("This theme has no such sound")
}
/// Кнопка панели товара Магазина из раздела «Звуки».
pub fn sound_store_preview_btn() -> String {
    tr("🔊 Preview Before Installing")
}
/// Подсказка к ней.
pub fn sound_store_preview_tooltip() -> String {
    tr("The theme is downloaded to a temporary folder so you can hear its sounds. Nothing is installed.")
}
/// Ход скачивания для прослушивания: доля, если сервер назвал размер,
/// иначе сколько уже пришло.
pub fn sound_store_progress(done: i64, total: i64) -> String {
    if total > 0 {
        let pct = (done.max(0) * 100 / total).min(100);
        trf("Downloading the theme to preview it: %1 %", &[&pct])
    } else if done > 0 {
        let mb = format!("{:.1}", done as f64 / (1024.0 * 1024.0));
        trf("Downloading the theme to preview it: %1 MB", &[&mb])
    } else {
        tr("Downloading the theme to preview it…")
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Шапка вкладки: подписи, чипы и накладные контролы
// ═══════════════════════════════════════════════════════════════════════

/// Заголовок вкладки.
pub fn page_title() -> String {
    tr("Appearance")
}

/// Наименьший размер окна вкладки: боковая панель (210) плюс два ряда плиток
/// по 300 с полями — уже при 900 сетка не рвётся на один столбец. То же
/// число, что у «Сравнения» и «Steam»: у одного продукта один нижний предел.
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 720);

/// Кегль заголовка вкладки (`_apply_heading_skin` оригинала).
pub const PAGE_TITLE_POINT_SIZE: i32 = 15;

/// Строка сайдбара: значок и имя раздела с отбивкой, как в оригинале.
pub fn sidebar_row_text(icon: &str, name: &str) -> String {
    format!("  {icon}  {name}")
}

/// Заголовок открытого раздела над сеткой.
pub fn section_title(icon: &str, name: &str) -> String {
    format!("{icon}  {name}")
}

/// Подпись перед чипами источника.
pub fn source_label() -> String {
    tr("Store:")
}
/// Подпись перед под-чипами (вид стиля, режим блокировки).
pub fn submode_label() -> String {
    tr("View:")
}
/// Подпись перед выбором рабочего стола.
pub fn de_label() -> String {
    tr("Current desktop:")
}
/// Подсказка выбора рабочего стола.
pub fn de_tooltip() -> String {
    tr("The catalog shows themes for this desktop.\nIt is detected automatically \
        from the current session.")
}

/// Кнопка обновления каталога и подсказка к ней.
pub const REFRESH_BTN: &str = "🔄";
pub fn refresh_tooltip() -> String {
    tr("Refresh catalog")
}


/// «Установить из файла…» в шапке раздела — тема, которая уже лежит на
/// диске (`crate::local_install`).
pub fn install_file_btn() -> String {
    tr("Install from File…")
}
pub fn install_file_tooltip() -> String {
    tr("Install a theme that is already on this computer: an archive, a folder or \
        single files — fonts, cursors and icons from Windows, color schemes, wallpapers. \
        Rexon recognizes the kind by the contents.")
}
/// Пункты меню кнопки: файлы или папка.
pub fn install_file_files_item() -> String {
    tr("Choose Files…")
}
pub fn install_file_folder_item() -> String {
    tr("Choose a Folder…")
}
/// Заголовок окна выбора: куда ставим.
pub fn install_file_dialog_title(section: &str) -> String {
    trf("Install into “%1”", &[&section])
}

/// Кнопки листания страниц.
pub fn page_prev_btn() -> String {
    tr("← Previous")
}
pub fn page_next_btn() -> String {
    tr("Next →")
}

/// Чип переключателя: ключ, подпись и подсказка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Chip {
    pub key: &'static str,
    pub label: String,
    pub tooltip: String,
}

/// Чипы «Магазин:». Показываются в каждом настоящем разделе; там, где
/// выбирать не из чего, переключателя нет вовсе (см. `source_applicable`).
pub fn source_chips() -> Vec<Chip> {
    vec![
        Chip {
            key: SOURCE_OCS,
            label: "KDE Store".to_string(),
            tooltip: tr("KDE Store catalog (OCS)"),
        },
        Chip {
            key: SOURCE_AUR,
            label: "🛠️ AUR".to_string(),
            tooltip: tr("AUR packages that follow the section's naming convention (Arch-based)"),
        },
        Chip {
            key: SOURCE_BOTH,
            label: trc("store", "All"),
            tooltip: tr("KDE Store + AUR in one list"),
        },
    ]
}

/// Чипы «Магазин:» раздела — ровно те источники, что в нём есть
/// ([`source_keys_for`]): общие плюс Cinnamon Spices в разделах тем по составу.
pub fn source_chips_for(category_key: &str) -> Vec<Chip> {
    let keys = source_keys_for(category_key);
    let mut chips: Vec<Chip> = source_chips().into_iter().filter(|c| keys.contains(&c.key)).collect();
    if keys.contains(&SOURCE_SPICES) && keys.len() > 1 {
        chips.push(Chip {
            key: SOURCE_SPICES,
            label: "Cinnamon Spices".to_string(),
            tooltip: crate::cinnamon_spices::parts_chip_tooltip(category_key),
        });
    }
    chips
}

/// Чипы режима «Экрана блокировки» для окружения: где полных тем
/// блокировки нет (Deepin), остаётся только чип обоев.
pub fn lock_chips_for(de_id: &str) -> Vec<Chip> {
    let mut chips = lock_chips();
    if !categories::lock_themes_supported(de_id) {
        chips.retain(|c| c.key != LOCK_MODE_THEMES);
    }
    chips
}

/// Чипы режима «Экрана блокировки».
pub fn lock_chips() -> Vec<Chip> {
    vec![
        Chip {
            key: LOCK_MODE_THEMES,
            label: tr("🔒 Full Themes"),
            tooltip: tr("Lock screen themes (the QML greeter) live inside Global Themes. \
                Only the lock screen part is applied, not the whole theme. Not every \
                Global Theme has its own lock screen — then the default look is used."),
        },
        Chip {
            key: LOCK_MODE_WALLPAPER,
            label: tr("🖼️ Lock Screen Wallpaper"),
            tooltip: tr("A picture as the lock screen background (thousands to choose \
                from in the wallpaper catalog); the greeter stays the same."),
        },
    ]
}

/// Подпись перед чипами размера курсора.
pub fn cursor_size_label() -> String {
    trc("cursor", "Size:")
}

/// Ключи чипов размера курсора — те же числа, что [`crate::themes::CURSOR_SIZES`]
/// (сторож в сверке держит их вместе).
pub const CURSOR_SIZE_KEYS: &[&str] = &["24", "32", "40", "48", "64", "96"];

/// Чипы размера курсора («Курсоры»). Размер ложится на ТЕКУЩУЮ тему курсора
/// и доходит туда же, куда выбор темы: родной ключ окружения, GTK,
/// переменные окружения, конфиг тайлового композитора.
pub fn cursor_size_chips() -> Vec<Chip> {
    let tooltip = tr("Pointer size in pixels. Rexon applies it to the cursor theme in use \
        — in the desktop's own settings, GTK and Qt apps, and the config of \
        sway, niri or Hyprland.");
    CURSOR_SIZE_KEYS
        .iter()
        .map(|key| Chip { key, label: (*key).to_string(), tooltip: tooltip.clone() })
        .collect()
}

/// Фильтр «Стиля приложений»: стили Qt и темы Kvantum вместе.
pub const STYLE_FILTER_ALL: &str = "all";
/// Фильтр «Стиля приложений»: только стили Qt.
pub const STYLE_FILTER_QT: &str = "qt";
/// Фильтр «Стиля приложений»: только темы Kvantum.
pub const STYLE_FILTER_KVANTUM: &str = "kvantum";

/// Чипы фильтра над сеткой «Стиля приложений».
///
/// До 17.10.2026 каждый стиль Qt был чипом, а под чипом стояла ОДНА
/// карточка того же стиля (перенос из окна на Python): раздел честно
/// писал «Стиль приложений: 1», а темы Kvantum открывались только чипом
/// «Kvantum». Теперь всё в одной сетке, а чипы только сужают её.
pub fn style_filter_chips() -> Vec<Chip> {
    vec![
        Chip {
            key: STYLE_FILTER_ALL,
            label: trc("style filter", "All"),
            tooltip: tr("Qt styles and Kvantum themes together"),
        },
        Chip {
            key: STYLE_FILTER_QT,
            label: tr("Qt styles"),
            tooltip: tr("Styles built into Qt and installed as plugins (Breeze, Fusion, Oxygen…)"),
        },
        Chip {
            key: STYLE_FILTER_KVANTUM,
            label: "Kvantum".to_string(),
            tooltip: tr("Kvantum themes (Kvantum engine)"),
        },
    ]
}

/// Какие стили Qt показывать карточками «Стиля приложений».
///
/// Движок Kvantum среди них не значится — вместо него в сетке его темы.
/// Прокси-стили `qt5ct-style` / `qt6ct-style` — не внешний вид, а
/// прослойка qtNct поверх настоящего стиля: человеку такая карточка ничего
/// не говорит, а выбор её ничего не меняет.
pub fn shown_qt_styles(styles: &[String]) -> Vec<String> {
    styles
        .iter()
        .filter(|s| {
            let low = s.to_lowercase();
            !low.starts_with("kvantum") && low != "qt5ct-style" && low != "qt6ct-style"
        })
        .cloned()
        .collect()
}


// ── Накладные контролы поверх картинки деталей ─────────────────────────

/// Угол виджета внутри картинки деталей.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPos {
    pub x: i32,
    pub y: i32,
}

/// Положение лупы — правый верхний угол картинки.
pub fn overlay_zoom_pos(preview_w: i32, btn_w: i32) -> OverlayPos {
    OverlayPos {
        x: 0.max(preview_w - btn_w - OVERLAY_MARGIN),
        y: OVERLAY_MARGIN,
    }
}

/// Положение стрелки «назад» — левый край, по центру вертикали.
pub fn overlay_prev_pos(preview_h: i32, btn_h: i32) -> OverlayPos {
    OverlayPos {
        x: OVERLAY_MARGIN,
        y: 0.max((preview_h - btn_h) / 2),
    }
}

/// Положение стрелки «вперёд» — правый край, по центру вертикали.
pub fn overlay_next_pos(preview_w: i32, preview_h: i32, btn_w: i32, btn_h: i32) -> OverlayPos {
    OverlayPos {
        x: 0.max(preview_w - btn_w - OVERLAY_MARGIN),
        y: 0.max((preview_h - btn_h) / 2),
    }
}

/// Положение счётчика «2/5» — низ картинки, по центру.
pub fn overlay_counter_pos(
    preview_w: i32,
    preview_h: i32,
    counter_w: i32,
    counter_h: i32,
) -> OverlayPos {
    OverlayPos {
        x: 0.max((preview_w - counter_w) / 2),
        y: 0.max(preview_h - counter_h - OVERLAY_MARGIN),
    }
}

/// Что открыть в полноэкранном просмотре: все уже скачанные кадры по порядку
/// и место текущего среди них.
///
/// Кадр может быть один (местная тема либо одиночное превью) — тогда и
/// показываем только его: подставлять в список чужие картинки нечестно.
/// `exists` спрашивает файловую систему, есть ли файл на самом деле.
pub fn zoom_frames<F: Fn(&str) -> bool>(
    current: &str,
    loaded: &BTreeMap<i64, String>,
    exists: F,
) -> Option<(Vec<String>, usize)> {
    if current.is_empty() || !exists(current) {
        return None;
    }
    let paths: Vec<String> = loaded
        .values()
        .filter(|p| exists(p))
        .cloned()
        .collect();
    match paths.iter().position(|p| p == current) {
        Some(i) => Some((paths, i)),
        None => Some((vec![current.to_string()], 0)),
    }
}

/// Подпись кнопки применения местной темы после успеха.
pub fn local_applied_btn() -> String {
    tr("✅ Active")
}

#[cfg(test)]
mod tests {

    /// Блок выбора файла: строки в порядке выдачи, автоматический помечен,
    /// размер и день — одной строкой; файл один — блока нет.
    #[test]
    fn store_file_choices_rows() {
        let item = ThemeItem {
            name: "Theme".into(),
            download_links: vec![
                serde_json::json!({"url": "https://f/Extra 1.4.tar.xz", "name": "Extra 1.4.tar.xz", "size": 13}),
                serde_json::json!({"url": "https://f/Theme-20230519162903.tar.xz", "name": "Theme-20230519162903.tar.xz", "size": 0}),
            ],
            ..ThemeItem::default()
        };
        let rows: Value = serde_json::from_str(&store_file_choices_json(&item)).unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Extra 1.4.tar.xz");
        assert_eq!(rows[0]["chosen"], false);
        assert_eq!(rows[0]["meta"], human_size_kib(13));
        assert_eq!(rows[1]["chosen"], true);
        assert_eq!(rows[1]["meta"], "2023-05-19", "размер неизвестен — только день");
        let single = ThemeItem { download_links: vec![item.download_links[0].clone()], ..item };
        assert_eq!(store_file_choices_json(&single), "[]");
    }

    /// Живой кусок описания из кеша магазина: `(default 15&deg;)` магазин
    /// прочитал как `;)` и подставил картинку подмигивания.
    #[test]
    fn a_store_emoticon_becomes_a_character() {
        let live = "ap<br />🎚️ Configurable minimum altitude (default 15&amp;deg\
<img src=\"https://www.pling.com/media/emoticons/wink.png\" alt=\"\" /><br />🔵 Catalog filter:";
        let out = super::inline_emoticons(live);
        assert!(out.contains("15&amp;deg😉<br />"), "вышло: {out}");
        assert!(!out.contains("<img"), "картинка осталась: {out}");
    }

    /// Все девять имён, что встречаются в живом кеше, и хост может быть любым:
    /// в описаниях попадаются и pling.com, и opendesktop.org.
    #[test]
    fn every_emoticon_seen_in_the_store_has_a_character() {
        for (name, ch) in [
            ("happy", "🙂"), ("wink", "😉"), ("cool", "😎"), ("ecstatic", "😄"),
            ("tongue", "😛"), ("gah", "😩"), ("heart", "❤️"), ("sad", "🙁"),
            ("kiss", "😘"),
        ] {
            let tag = format!("<img src=\"https://www.opendesktop.org/media/emoticons/{name}.png\" alt=\"\" />");
            assert_eq!(super::inline_emoticons(&tag), ch, "смайл {name}");
        }
    }

    /// Незнакомое имя — пустое место, а не чужая рожица наугад.
    #[test]
    fn an_unknown_emoticon_leaves_nothing_behind() {
        let tag = "до<img src=\"https://www.pling.com/media/emoticons/zzz-unknown.png\" />после";
        assert_eq!(super::inline_emoticons(tag), "допосле");
    }

    /// Картинки самого описания (снимки экрана) не трогаем вовсе.
    #[test]
    fn a_screenshot_in_the_description_survives() {
        let html = "<p>вид</p><img src=\"https://cdn.pling.com/img/preview.png\" alt=\"\" />";
        assert_eq!(super::inline_emoticons(html), html);
    }

    use super::*;

    #[test]
    fn human_count_switches_units() {
        assert_eq!(human_count(0), "0");
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(1000), "1.0K");
        assert_eq!(human_count(1234), "1.2K");
        assert_eq!(human_count(999_999), "1000.0K");
        assert_eq!(human_count(1_000_000), "1.0M");
        assert_eq!(human_count(12_345_678), "12.3M");
    }

    #[test]
    fn default_sort_is_the_new_arrivals() {
        assert_eq!(SORTS[default_sort_index()].1, "new");
    }

    #[test]
    fn suggest_box_stays_inside_the_page() {
        // У правого края выпадашка сдвигается влево, но не за левое поле.
        assert_eq!(suggest_x(900, 360, 1000), 1000 - SUGGEST_EDGE - 360);
        assert_eq!(suggest_x(10, 360, 1000), 10);
        assert_eq!(suggest_x(0, 5000, 400), SUGGEST_LEFT);
    }

    #[test]
    fn fatal_http_is_not_retried() {
        assert!(!preview_retry_allowed(Some(404), 1));
        assert!(preview_retry_allowed(Some(500), 1));
        assert!(preview_retry_allowed(None, PREVIEW_RETRIES));
        assert!(!preview_retry_allowed(None, PREVIEW_RETRIES + 1));
    }

    #[test]
    fn page_qss_is_built_from_the_palette() {
        let light = ui_base::skin_colors("light");
        let qss = page_qss(&light);
        assert!(qss.contains("background: #f4f5f6;"));
        // Приглушение неактивной кнопки идёт от цвета скина, а не белым.
        assert!(qss.contains("#PageBtn:disabled { color: rgba(0,0,0,0.55); }"));
        assert!(!qss.contains("rgba(255,255,255,0.55)"));
    }

    #[test]
    fn suggestions_follow_the_skin() {
        for id in ["steam_dark", "light", "synthwave"] {
            let c = ui_base::skin_colors(id);
            let qss = suggestions_qss(&c);
            // Ни одного цвета мимо палитры скина.
            assert!(!qss.contains("#aeb9c4"), "{id}");
            assert!(qss.contains(&format!("background:{}", token(&c, "bg_secondary"))), "{id}");
        }
    }

    #[test]
    fn no_skin_leaves_the_page_text_unreadable() {
        // Подпись не должна совпадать с фоном, на котором её рисуют.
        for id in ["system", "steam_dark", "light", "synthwave"] {
            let c = ui_base::skin_colors(id);
            for key in ["text_primary", "text_secondary", "text_dim"] {
                assert_ne!(token(&c, key), token(&c, "bg_primary"), "{id}/{key}");
                assert_ne!(token(&c, key), token(&c, "bg_card"), "{id}/{key}");
            }
        }
    }

    /// Какая плитка «звучит»: местная тема звуков по номеру, товар Магазина —
    /// по разделу «Звуки»; номер с `/` темой не считается.
    #[test]
    fn sound_preview_targets() {
        assert_eq!(local_sound_theme("local:sounds:Ocean").as_deref(), Some("Ocean"));
        assert_eq!(local_sound_theme("local:sounds:"), None);
        assert_eq!(local_sound_theme("local:sounds:../x/y"), None);
        assert_eq!(local_sound_theme("local:icons:Breeze"), None);
        let sounds_cat = categories::categories(Path::new("/h"))
            .into_iter()
            .find(|c| c.key == "sounds")
            .unwrap();
        assert_eq!(sounds_cat.id, SOUND_CATEGORY_ID);
        let store = ThemeItem { id: "2131512".into(), category_id: SOUND_CATEGORY_ID, ..Default::default() };
        assert!(is_store_sound_item(&store));
        let local = ThemeItem { id: "local:sounds:Ocean".into(), category_id: SOUND_CATEGORY_ID, ..Default::default() };
        assert!(!is_store_sound_item(&local));
        let icons = ThemeItem { id: "1".into(), category_id: 132, ..Default::default() };
        assert!(!is_store_sound_item(&icons));
        assert!(sound_store_progress(50, 200).contains("25"));
        assert!(sound_store_progress(3 * 1024 * 1024, 0).contains("3.0"));
        assert!(sound_preview_tooltip("battery-caution").contains("battery-caution"));
    }

    // ── «Вернуть как было» после «Применить» ──

    fn revert_scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-applied-revert-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn applied_rec(section: &str, theme: &str, at: Option<&str>, files: Vec<crate::install_journal::AppliedFile>) -> crate::install_journal::Applied {
        crate::install_journal::Applied {
            section: section.into(),
            theme: theme.into(),
            item: None,
            applied_at: at.map(str::to_string),
            snapshot: String::new(),
            files,
        }
    }

    fn file(path: &str) -> crate::install_journal::AppliedFile {
        crate::install_journal::AppliedFile { path: path.into(), before: None, dir: false }
    }

    /// Модель «что можно вернуть»: подпись называет тему и время нажатия;
    /// записи нет (или она без файлов) — полосы нет; у цветов при видимой
    /// кнопке полосы шелла своей нет, а памятка слоя без записи — даёт её.
    #[test]
    fn applied_revert_banner_model() {
        assert_eq!(applied_when(Some("2026-10-25T14:02:33")).as_deref(), Some("25.10, 14:02"));
        assert_eq!(applied_when(Some("вчера")), None);
        assert_eq!(applied_when(None), None);

        let rec = applied_rec("icons", "Papirus", Some("2026-10-25T14:02:33"), vec![file("/x")]);
        let b = applied_revert_banner("icons", Some(&rec), false, false);
        assert!(b.visible);
        assert_eq!(b.section, "icons");
        assert_eq!(b.button, "Return as It Was Before “Papirus” (25.10, 14:02)");
        assert!(b.text.contains("Papirus") && b.text.contains("25.10, 14:02"), "{}", b.text);
        // Чужая полоса шелла и памятка цветов значков не касаются.
        assert!(applied_revert_banner("icons", Some(&rec), true, true).visible);

        let no_time = applied_rec("icons", "Papirus", None, vec![file("/x")]);
        assert_eq!(applied_revert_banner("icons", Some(&no_time), false, false).button, "Return as It Was Before “Papirus”");

        assert!(!applied_revert_banner("icons", None, false, false).visible);
        let empty = applied_rec("icons", "Papirus", None, Vec::new());
        assert!(!applied_revert_banner("icons", Some(&empty), false, false).visible);

        let colors = applied_rec("colors", "Nord", Some("2026-10-25T09:05:00"), vec![file("/x")]);
        assert!(applied_revert_banner("colors", Some(&colors), true, false).visible);
        let shell = applied_revert_banner("colors", Some(&colors), true, true);
        assert!(!shell.visible, "у цветов кнопка уже есть на полосе шелла");
        let memo_only = applied_revert_banner("colors", None, true, false);
        assert!(memo_only.visible);
        assert_eq!(memo_only.button, "Return Colors as They Were");
        assert!(!applied_revert_banner("colors", None, false, false).visible);
    }

    /// Итог возврата словами: всё вернулось / что не вернулось (путь от
    /// дома — через «~») / записи нет.
    #[test]
    fn applied_revert_outcome_texts() {
        let (ok, text) = applied_revert_outcome("Papirus", &Ok(Some(Vec::new())), "/home/u");
        assert!(ok && text.contains("Papirus"), "{text}");
        let (ok, text) = applied_revert_outcome("Papirus", &Ok(Some(vec!["/home/u/.config/kdeglobals".into()])), "/home/u");
        assert!(!ok);
        assert!(text.contains("~/.config/kdeglobals") && text.contains("1 file"), "{text}");
        let (ok, _) = applied_revert_outcome("Papirus", &Ok(None), "/home/u");
        assert!(!ok);
        // Настройки стола — своей строкой, не «файлами».
        let failed = vec!["/home/u/.config/kdeglobals".into(), "gsettings org.mate.interface gtk-theme".into(), "xfconf xsettings /Net/ThemeName".into()];
        let (ok, text) = applied_revert_outcome("Papirus", &Ok(Some(failed)), "/home/u");
        assert!(!ok);
        assert!(text.contains("1 file could not") && !text.contains("2 files"), "{text}");
        assert!(text.contains("2 desktop settings could not be put back (org.mate.interface gtk-theme, xsettings /Net/ThemeName)"), "{text}");
    }

    /// Возврат на временном профиле: запись «Применить» значков — файлы
    /// прежние, запись и снимок сняты, полосы больше нет.
    #[test]
    fn revert_applied_section_icons() {
        use crate::install_journal as journal;
        let dir = revert_scratch("icons");
        let home = dir.join("home");
        let rexon = home.join(".config/rexon");
        let ini = home.join(".config/gtk-3.0/settings.ini");
        let made = home.join(".config/gtk-4.0/settings.ini");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        std::fs::create_dir_all(made.parent().unwrap()).unwrap();
        std::fs::write(&ini, "new\n").unwrap();
        std::fs::write(&made, "made\n").unwrap();
        let snap = journal::applied_snapshot_dir(&rexon, "icons");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("0001"), "old\n").unwrap();
        let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
        let mut rec = applied_rec(
            "icons",
            "Papirus",
            Some("2026-10-25T14:02:33"),
            vec![
                journal::AppliedFile { path: s(&ini), before: Some(s(&snap.join("0001"))), dir: false },
                file(&s(&made)),
            ],
        );
        rec.snapshot = s(&snap);
        journal::record_applied_at(&rexon, rec).unwrap();
        let view = journal::view_at(&rexon);
        assert!(applied_revert_banner("icons", view.applied.get("icons"), false, false).visible);

        let palette = crate::shell_palette::PaletteHome { home: s(&home), ..Default::default() };
        let (ok, text) = revert_applied_section(&rexon, &palette, "icons");
        assert!(ok, "{text}");
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), "old\n");
        assert!(!made.exists(), "заведённый нажатием файл остался");
        assert!(!snap.exists(), "снимок остался");
        let view = journal::view_at(&rexon);
        assert!(!applied_revert_banner("icons", view.applied.get("icons"), false, false).visible);
        let (ok, _) = revert_applied_section(&rexon, &palette, "icons");
        assert!(!ok, "второй раз возвращать нечего");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Цвета вне Plasma: одна кнопка. Памятка слоя возвращает свои файлы
    /// (файл, переписанный после Rexon, оставляет как есть), запись
    /// «Применить» — только прочее; сама памятка обратно не воскресает.
    #[test]
    fn revert_applied_section_colors_layer_first() {
        use crate::install_journal as journal;
        use crate::shell_palette as sp;
        for edited in [false, true] {
            let dir = revert_scratch(if edited { "colors-edited" } else { "colors" });
            let home = dir.join("home");
            let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
            let palette = sp::PaletteHome { home: s(&home), ..Default::default() };
            let rexon = home.join(".config/rexon");
            let own = home.join(".config/qt6ct/colors/Rexon.conf");
            let other = home.join(".config/gtk-3.0/gtk.css");
            std::fs::create_dir_all(own.parent().unwrap()).unwrap();
            std::fs::create_dir_all(other.parent().unwrap()).unwrap();
            std::fs::write(&own, if edited { "user" } else { "ours" }).unwrap();
            std::fs::write(&other, "css-new").unwrap();
            let memo_file = sp::layer_memo_path(&palette);
            std::fs::create_dir_all(memo_file.parent().unwrap()).unwrap();
            let memo = sp::LayerMemo {
                files: vec![sp::FileMemo {
                    path: s(&own),
                    kind: "own".into(),
                    original: Some("theirs".into()),
                    written: "ours".into(),
                    keys: Vec::new(),
                    created_tables: Vec::new(),
                }],
                scheme: Some("Nord".into()),
            };
            std::fs::write(&memo_file, serde_json::to_string(&memo).unwrap()).unwrap();
            let snap = journal::applied_snapshot_dir(&rexon, "colors");
            std::fs::create_dir_all(&snap).unwrap();
            std::fs::write(snap.join("0001"), "{\"files\":[]}").unwrap();
            std::fs::write(snap.join("0002"), "stale").unwrap();
            std::fs::write(snap.join("0003"), "css-old").unwrap();
            let mut rec = applied_rec(
                "colors",
                "Nord",
                None,
                vec![
                    journal::AppliedFile { path: s(&memo_file), before: Some(s(&snap.join("0001"))), dir: false },
                    journal::AppliedFile { path: s(&own), before: Some(s(&snap.join("0002"))), dir: false },
                    journal::AppliedFile { path: s(&other), before: Some(s(&snap.join("0003"))), dir: false },
                ],
            );
            rec.snapshot = s(&snap);
            journal::record_applied_at(&rexon, rec).unwrap();

            let (ok, text) = revert_applied_section(&rexon, &palette, "colors");
            assert!(ok, "{text}");
            let want_own = if edited { "user" } else { "theirs" };
            assert_eq!(std::fs::read_to_string(&own).unwrap(), want_own, "правка после Rexon не сбережена / не вернулось");
            assert_eq!(std::fs::read_to_string(&other).unwrap(), "css-old");
            assert!(!memo_file.exists(), "памятка воскресла из снимка");
            assert!(journal::view_at(&rexon).applied.is_empty());
            assert!(!applied_revert_banner("colors", None, sp::read_layer_memo(&palette).has_changes(), false).visible);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
