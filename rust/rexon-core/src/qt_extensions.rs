//! Порт `rexon/qt_extensions/window.py` — вкладка «Расширения».
//!
//! Вкладка была ЕДИНСТВЕННОЙ крупной Qt-вкладкой, не подключённой к ядру
//! вовсе: в её файле не было ни одного обращения к `rexon_py`, хотя вся
//! механика расширений давно живёт в [`crate::ext_manager`],
//! [`crate::extension`], [`crate::ext_utils`] и [`crate::constants`]. Поэтому
//! переносить надо было не логику, а ИНТЕРФЕЙС — и заодно подключить его к
//! тому, что уже есть.
//!
//! Что решает этот модуль: какие подписи показать, какие фильтры собрать по
//! составу установленного, что написать в статистике и в пустом состоянии,
//! из чего состоит карточка, строка списка, подробный разбор и справка
//! «как собрать своё расширение», какие файлы принимать перетаскиванием и
//! какой текст показать в строке состояния после каждого действия.
//!
//! Чего здесь НЕТ: самих расширений (их читает [`installed`] с диска через
//! [`crate::extension::Extension`]) и побочных действий — установка,
//! удаление и переключение ходят в систему и остаются у исполнителя
//! (сейчас `rexon/extensions/manager.py`).
//!
//! ⚠️ Починки оригинала (правило #16 — баги при порте чиним, не
//! консервируем), каждая помечена `Починка:` по месту:
//!   1. имя, автор, описание, теги и README вставлялись в HTML БЕЗ
//!      экранирования: расширение с `<` в описании ломало разбор подписи, а
//!      README с угловыми скобками терял куски текста прямо на витрине;
//!   2. вторичные подписи (тип, автор, статистика, путь, тех. данные,
//!      строка состояния) были прибиты к `color: #ffffff` при любой теме —
//!      на светлой это белым по белому. Цвет теперь приходит парой из
//!      [`crate::ui_base`], как требует правило #9;
//!   3. в светлой теме у выключенной кнопки блока «Установить» стоял
//!      `color: rgba(255,255,255,0.55)` — «Steam Workshop» был неразличим;
//!   4. нажатие на кнопку фильтра при ПУСТОМ списке расширений прятало
//!      подсказку «Расширений ещё нет» вместо того, чтобы её показать
//!      (в `_refresh` та же ветка работала правильно). Пустое состояние
//!      теперь одно — [`empty_text`].
//!
//! Сверка — `rust/rexon-core/tests/qt_extensions_parity.rs` по снимку живого
//! Python-пути (`tests/golden/qt_extensions_cases.json`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::constants;
use crate::ext_manager;
use crate::extension::Extension;
use crate::i18n::{tr, trc, trf};

// ============================================================================
// Неизменные подписи и размеры вкладки
// ============================================================================

// Подписи — функции, а не константы: язык интерфейса выбирается при запуске,
// и функцию, в отличие от константы, нельзя забыть перевести.

pub fn window_title() -> String {
    tr("Rexon — Extensions")
}
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 700);

pub fn header_title() -> String {
    tr("🧩 Extensions")
}
pub const HEADER_TITLE_POINT_SIZE: i32 = 20;
pub const SECTION_POINT_SIZE: i32 = 14;

/// Ширина плитки расширения. ПОСТОЯННАЯ: по ней движок сеток считает
/// столбцы. Вилка «220…280» давала карточки разной ширины в одном ряду, и
/// расчёт столбцов врал — «ширины карточки» попросту не существовало.
pub const CARD_W: i32 = 260;
/// Ширина подписей внутри плитки: минус поля компоновки (8+8).
pub const CARD_TEXT_W: i32 = CARD_W - 16;
pub const PREVIEW_H: i32 = 120;

pub fn search_placeholder() -> String {
    tr("Search by name, author, tags…")
}
pub fn install_button_text() -> String {
    tr("＋ Install from File")
}

pub const VIEW_GRID_TEXT: &str = "⊞";
pub fn view_grid_tooltip() -> String {
    trc("extensions view", "Grid")
}
pub const VIEW_LIST_TEXT: &str = "☰";
pub fn view_list_tooltip() -> String {
    trc("extensions view", "List")
}

/// Идентификаторы режима показа. Строками, а не числами: их же кладут в
/// роли модели и читает разметка.
pub const VIEW_GRID: &str = "grid";
pub const VIEW_LIST: &str = "list";

pub const FILTER_ALL_ID: &str = "all";
pub fn filter_all_label() -> String {
    tr("All")
}

pub fn my_header_text() -> String {
    tr("My Extensions")
}
pub fn install_section_header() -> String {
    tr("⬇️ Install")
}
pub fn roadmap_note() -> String {
    tr("🚧 Coming soon: Steam Workshop for .rxn appearance packs and extensions.")
}

pub fn empty_no_extensions() -> String {
    tr("📭 No extensions yet. Drag an .rxn file into the window or \
click <b>“+ Install from File”</b>. Windows .themepack, .deskthemepack \
and .msstyles files work too.")
}
pub fn empty_no_match() -> String {
    tr("🔍 Nothing matches the current filters.")
}

pub const UNINSTALL_ICON: &str = "🗑️";
/// Подпись кнопки удаления НА ПЛИТКЕ и в подробном разборе.
///
/// ⚠️ Отступление от виджетного эталона, сделанное намеренно (правило #13).
/// На плитке у оригинала стоит голая корзина шириной 40 точек, и живая
/// проверка 06.09 показала ровно то, чего это правило не допускает: у
/// НЕОБРАТИМОГО действия нет подписи, значок покупателю приходится
/// угадывать, а подсказка появляется только под курсором — на геймпаде и в
/// Big Picture её не существует вовсе. Место на плитке есть: ширина 260
/// минус поля 8+8 даёт 244, а самая тесная пара кнопок («▶ Включить» и
/// «🗑 Удалить») занимает около 214.
/// В СТРОКЕ СПИСКА подпись не ставится: там компактный вид, и выключатель
/// рядом тоже нарисован одним значком — обе кнопки объясняет подсказка.
pub fn uninstall_text() -> String {
    tr("🗑️ Remove")
}
pub fn uninstall_tooltip() -> String {
    trc("extension action", "Remove")
}
pub fn toggle_on_text() -> String {
    tr("▶️ Enable")
}
pub fn toggle_off_text() -> String {
    tr("⏸️ Disable")
}
pub const TOGGLE_ON_ICON: &str = "▶️";
pub const TOGGLE_OFF_ICON: &str = "⏸️";
pub fn toggle_on_tooltip() -> String {
    trc("extension action", "Enable")
}
pub fn toggle_off_tooltip() -> String {
    trc("extension action", "Disable")
}

pub fn state_enabled_text() -> String {
    trc("extension state", "✅ Active")
}
pub fn state_disabled_text() -> String {
    trc("extension state", "⏸️ Disabled")
}
/// Цвет отметки «активно» и голубой заголовок группы/тегов. Пишутся здесь, а
/// не в .qml: своих цветов у разметки быть не должно.
pub const STATE_ENABLED_COLOR: &str = "#4ade80";
pub const STATE_DISABLED_COLOR: &str = "#888888";
pub const ACCENT_INFO_COLOR: &str = "#6cb4ff";
/// Красная подложка кнопки удаления (в виджетах — `rgba(220,80,80,0.2)`).
pub const DANGER_COLOR: &str = "#dc5050";
pub const DANGER_FILL_ALPHA: f64 = 0.2;
pub const DANGER_FILL_HOVER_ALPHA: f64 = 0.4;

/// Заголовок и фильтр окна выбора файла.
pub fn file_dialog_title() -> String {
    tr("Choose an .rxn file")
}
pub fn file_dialog_filter() -> String {
    // TRANSLATORS: file filter of a file dialog; keep the ";;" separators
    // and the patterns in parentheses exactly as they are.
    tr("Rexon extensions (*.rxn);;\
Windows themes (*.themepack *.deskthemepack *.theme *.msstyles);;\
All files (*)")
}

/// Расширения файлов, которые вкладка принимает перетаскиванием.
pub const DROP_EXTENSIONS: &[&str] = &[
    ".rxn",
    ".themepack",
    ".deskthemepack",
    ".theme",
    ".msstyles",
    ".cur",
    ".ani",
    ".inf",
    ".curxptheme",
    ".7tsp",
];

/// Принимаем ли этот файл перетаскиванием (сравнение суффикса без регистра).
pub fn is_supported_drop(path: &str) -> bool {
    let lower = path.to_lowercase();
    let suffix = match lower.rfind('.') {
        Some(pos) => &lower[pos..],
        None => return false,
    };
    DROP_EXTENSIONS.contains(&suffix)
}

// ============================================================================
// Раскладка и оформление, которого нет в палитре скина
// ============================================================================
//
// Числа и краски виджетной вкладки, разбросанные по её компоновкам и таблицам
// стилей. QML-разметка своих не заводит: всё приходит отсюда картой `styles`
// (rust/rexon-gui/src/extensions_page.rs) — тем же набором, каким красится
// виджетная версия.

/// Поля страницы и шаг между её блоками.
pub const PAGE_MARGIN: i32 = 24;
pub const PAGE_SPACING: i32 = 14;

/// Кнопки вида (⊞ / ☰): кегль значка и наименьшая ширина содержимого.
pub const VIEW_BUTTON_POINT_SIZE: i32 = 14;
pub const VIEW_BUTTON_MIN_WIDTH: i32 = 32;
/// Поля чипа (кнопки вида и фильтры) по горизонтали и по вертикали,
/// скругление — общее с чипами Steam.
pub const CHIP_PADDING_H: i32 = 10;
pub const CHIP_PADDING_V: i32 = 4;
pub const CHIP_RADIUS: i32 = crate::qt_steam::BTN_RADIUS;
/// Зазор между кнопками фильтров.
pub const FILTER_SPACING: i32 = 4;

/// Шаг сетки плиток. Тем же шагом идут строки и заголовки групп списка: у
/// виджетной вкладки оба вида живут в одной сетке.
pub const GRID_SPACING: i32 = 10;
/// Плитка: поле компоновки, шаг строк, скругление, зазор между кнопками.
/// Ширины кнопки удаления здесь больше нет: с подписью (`UNINSTALL_TEXT`)
/// кнопка меряется по содержимому, как и соседний выключатель.
pub const CARD_MARGIN: i32 = 8;
pub const CARD_SPACING: i32 = 6;
pub const CARD_RADIUS: i32 = 6;
pub const CARD_BUTTON_SPACING: i32 = 4;
/// Кегль значка-заглушки на месте превью.
pub const PLACEHOLDER_POINT_SIZE: i32 = 32;

/// Строка списка: поля компоновки (по горизонтали и вертикали), добавка
/// таблицы стилей (`padding: 4px` ложится ПОВЕРХ полей компоновки — строка
/// у виджетов на 8 точек выше, чем подсказывают одни поля), шаг между
/// значком, подписями и кнопками, скругление, ширина и кегль значка,
/// ширина кнопок «⏸»/«🗑».
pub const LIST_ROW_MARGIN_H: i32 = 10;
pub const LIST_ROW_MARGIN_V: i32 = 6;
pub const LIST_ROW_PADDING: i32 = 4;
pub const LIST_ROW_SPACING: i32 = 10;
pub const LIST_ROW_RADIUS: i32 = 4;
pub const LIST_ICON_WIDTH: i32 = 36;
pub const LIST_ICON_POINT_SIZE: i32 = 18;
pub const LIST_BUTTON_WIDTH: i32 = 36;
/// Заголовок группы списка: отступ сверху и снизу (`padding: 6px 0 2px 0`).
pub const GROUP_HEADER_PADDING_TOP: i32 = 6;
pub const GROUP_HEADER_PADDING_BOTTOM: i32 = 2;

/// Поле вокруг подсказки пустого состояния (`padding: 16px`).
pub const EMPTY_PADDING: i32 = 16;

/// Кнопки блока «⬇ Установить»: зазор, скругление, поле вокруг подписи.
pub const INSTALL_SOURCE_SPACING: i32 = 10;
pub const INSTALL_SOURCE_RADIUS: i32 = 6;
pub const INSTALL_SOURCE_PADDING: i32 = 10;

/// Тройка «фон, рамка, рамка под курсором» плитки.
///
/// Та же, что у карточек витрины Steam: таблицы стилей обеих вкладок были
/// набраны одними числами (0.04 / 0.12 / 0.30 на тёмной, 0.03 / 0.15 / 0.35
/// на светлой), и второй копии в ядре не место (правило #12). Под курсором
/// у плитки меняется ТОЛЬКО рамка, фон остаётся.
pub fn card_colors(dark: bool) -> (String, String, String) {
    crate::qt_steam::card_colors(false, dark)
}

/// Пара «фон, цвет значка» заглушки на месте превью.
pub fn placeholder_colors(dark: bool) -> (String, String) {
    if dark {
        ("rgba(255,255,255,0.06)".to_string(), "#aaaaaa".to_string())
    } else {
        ("rgba(0,0,0,0.06)".to_string(), "#555555".to_string())
    }
}

/// Пара «фон, фон под курсором» строки списка.
///
/// Починка №5 (правило #16): у оригинала строка красилась белым с
/// прозрачностью при ЛЮБОЙ теме — на светлой это «светлее светлого», то есть
/// невидимая строка. Светлой теме — чёрный с той же прозрачностью, как у
/// плитки.
pub fn list_row_colors(dark: bool) -> (String, String) {
    if dark {
        (
            "rgba(255,255,255,0.03)".to_string(),
            "rgba(255,255,255,0.06)".to_string(),
        )
    } else {
        ("rgba(0,0,0,0.03)".to_string(), "rgba(0,0,0,0.06)".to_string())
    }
}

/// Тройка «фон, фон под курсором, рамка» крупной кнопки блока «Установить».
pub fn install_source_colors(dark: bool) -> (String, String, String) {
    if dark {
        (
            "rgba(255,255,255,0.04)".to_string(),
            "rgba(255,255,255,0.08)".to_string(),
            "rgba(255,255,255,0.15)".to_string(),
        )
    } else {
        (
            "rgba(0,0,0,0.03)".to_string(),
            "rgba(0,0,0,0.07)".to_string(),
            "rgba(0,0,0,0.15)".to_string(),
        )
    }
}

/// Четвёрка «рамка, фон выбранного, рамка выбранного, фон под курсором»
/// чипа — кнопок вида и фильтров.
///
/// Чип один на продукт (`qt_steam::switch_chip_colors`): выбранное состояние
/// у виджетных «Расширений» набрано теми же `rgba(59,130,246,0.4)` и
/// `rgba(59,130,246,0.6)`, что у подвкладок Steam.
pub fn chip_colors() -> (String, String, String, String) {
    crate::qt_steam::switch_chip_colors()
}

// ============================================================================
// Типы расширений: включаемые, фильтры, группы
// ============================================================================

/// Тип, у которого есть выключатель: скриптовые (`plugin`, `desktop-tool`).
///
/// У оформления выключателя нет и быть не может — оно либо стоит в системе,
/// либо нет. Ответ берётся из ядра (`SCRIPTED_TYPES`), а не повторяется
/// парой строк в UI: типов со скриптами может стать больше.
pub fn is_toggleable(ext_type: &str) -> bool {
    constants::is_scripted(ext_type)
        // Дополнения окружений (скрипты и эффекты KWin, расширения GNOME,
        // апплеты/десклеты/расширения Cinnamon, действия Nemo) включает и
        // выключает человек: установка их больше не включает (04.10.2026).
        || crate::de_apply::mode(ext_type).map(crate::de_apply::Mode::button)
            == Some(crate::de_apply::Button::Toggle)
}

/// Кнопка фильтра: идентификатор типа и подпись.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilterItem {
    pub id: String,
    pub label: String,
}

/// Кнопки фильтров по СОСТАВУ установленного: «Все» плюс только те типы,
/// что реально стоят, в порядке таблицы подписей.
///
/// Новый тип попадает в фильтр сам, без правки этого места (правило #12).
pub fn filter_items(installed_types: &[String]) -> Vec<FilterItem> {
    let mut items = vec![FilterItem {
        id: FILTER_ALL_ID.to_string(),
        label: filter_all_label(),
    }];
    for (t, label) in constants::type_labels_ordered() {
        if installed_types.iter().any(|x| x == t) {
            items.push(FilterItem {
                id: (*t).to_string(),
                label: format!(
                    "{} {}",
                    constants::type_icon(t).unwrap_or(""),
                    crate::i18n::tr(label)
                ),
            });
        }
    }
    items
}

/// Остаётся ли выбранный фильтр допустимым при этом составе кнопок.
///
/// Если кнопок кроме «Все» не осталось (расширения удалили), выбор
/// сбрасывается — иначе витрина молча показывала бы пустоту по фильтру,
/// которого на экране уже нет.
pub fn normalize_filter(filter_type: &str, items: &[FilterItem]) -> String {
    if items.iter().any(|i| i.id == filter_type) {
        filter_type.to_string()
    } else {
        FILTER_ALL_ID.to_string()
    }
}

// ============================================================================
// Строка статистики и заголовки
// ============================================================================

/// Строка статистики: «Установлено · Активных · по типам» (только ненулевые).
///
/// На вход — готовый ответ [`ext_manager::stats`], чтобы счёт был один на
/// продукт: ту же карту показывает и Python-путь.
pub fn stats_line(stats: &Map<String, Value>) -> String {
    let num = |key: &str| stats.get(key).and_then(Value::as_u64).unwrap_or(0);
    let mut parts = vec![
        trf("Installed: <b>%1</b>", &[&num("total")]),
        trf("Active: <b>%1</b>", &[&num("enabled")]),
    ];
    let by_type = stats.get("by_type").and_then(Value::as_object);
    for (t, label) in constants::type_labels_ordered() {
        let count = by_type
            .and_then(|m| m.get(*t))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if count > 0 {
            parts.push(format!(
                "{} {}: <b>{}</b>",
                constants::type_icon(t).unwrap_or(""),
                crate::i18n::tr(label),
                count
            ));
        }
    }
    parts.join("  •  ")
}

/// Заголовок «Мои расширения (отфильтровано/всего)».
///
/// `None` — расширений нет вовсе: заголовок над пустотой прячется.
pub fn my_header(filtered: usize, total: usize) -> Option<String> {
    if total == 0 {
        return None;
    }
    if filtered == total {
        Some(my_header_text())
    } else {
        Some(format!("{} ({filtered}/{total})", my_header_text()))
    }
}

/// Текст пустого состояния, либо `None`, если показывать нечего.
///
/// Починка №4: одна функция на обе ветки. В оригинале обработчик фильтра
/// прятал подсказку «Расширений ещё нет», хотя список был пуст.
pub fn empty_text(filtered: usize, total: usize) -> Option<String> {
    if total == 0 {
        Some(empty_no_extensions())
    } else if filtered == 0 {
        Some(empty_no_match())
    } else {
        None
    }
}

// ============================================================================
// Блок «Установить»
// ============================================================================

/// Кнопка блока «⬇ Установить».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallSource {
    /// Что делать по нажатию: `file` | `workshop` | `create`.
    pub kind: String,
    pub icon: String,
    pub label: String,
    pub tooltip: String,
    pub enabled: bool,
}

impl InstallSource {
    /// Подпись кнопки целиком: значок, перевод строки, название.
    pub fn text(&self) -> String {
        format!("{}\n{}", self.icon, self.label)
    }
}

pub const INSTALL_SOURCE_MIN_HEIGHT: i32 = 70;
pub const INSTALL_SOURCE_MIN_WIDTH: i32 = 160;
pub const INSTALL_SOURCE_MAX_WIDTH: i32 = 220;

/// Три источника расширений: файл, Мастерская Steam (пока выключена), своё.
pub fn install_sources() -> Vec<InstallSource> {
    vec![
        InstallSource {
            kind: "file".into(),
            icon: "📦".into(),
            label: tr("From File"),
            tooltip: tr("Open an .rxn, .themepack or Windows theme"),
            enabled: true,
        },
        InstallSource {
            kind: "workshop".into(),
            icon: "🎮".into(),
            // Кнопка неактивна, а подсказку на неактивной не увидеть:
            // причина стоит прямо в подписи, иначе серая плитка без слов
            // читалась поломкой.
            label: tr("Steam Workshop — coming soon"),
            tooltip: tr("Coming soon — Steam Workshop integration"),
            enabled: false,
        },
        InstallSource {
            kind: "create".into(),
            icon: "✏️".into(),
            label: tr("Create Your Own"),
            tooltip: tr("How to make your own .rxn extension"),
            enabled: true,
        },
    ]
}

// ============================================================================
// Строка состояния
// ============================================================================

/// Обрезка текста ошибки, как в оригинале: 200 символов.
///
/// ⚠️ Считаем СИМВОЛЫ, а не байты: в Python срез `msg[:200]` посимвольный, а
/// в Rust `&s[..200]` на русском тексте просто уронил бы процесс на границе
/// многобайтового символа.
fn clip(msg: &str) -> String {
    msg.chars().take(200).collect()
}

pub fn status_install_busy() -> String {
    tr("⚠️ An installation is already running")
}
pub fn status_uninstall_busy() -> String {
    tr("⚠️ A removal is already running")
}

pub fn status_installing(file_name: &str) -> String {
    trf("⏳ Installing “%1”…", &[&file_name])
}

pub fn status_installed(name: &str) -> String {
    trf("✅ Installed: %1", &[&name])
}

/// Имя установленного расширения ПЛЮС то, о чём человек обязан узнать.
///
/// Установка прошла, но часть необязательных шагов могла быть пропущена
/// (нет конвертера, нечем измерить обои). Молчать об этом нельзя: покупатель
/// решит, что программа так и работает, или что тема плохая. Строка
/// состояния переносится по словам, поэтому каждое предупреждение идёт своей
/// строкой. Предупреждений нет — возвращается одно имя, и вид сообщения не
/// меняется ни на символ.
pub fn installed_with_notes(name: &str, notes: &[String]) -> String {
    crate::importers::with_skipped_notes(name, notes)
}

pub fn status_install_error(msg: &str) -> String {
    trf("⚠️ Error: %1", &[&clip(msg)])
}

pub fn status_uninstalling(name: &str) -> String {
    trf("⏳ Removing “%1”…", &[&name])
}

pub fn status_uninstalled(ext_id: &str) -> String {
    trf("🗑️ Removed: %1", &[&ext_id])
}

pub fn status_uninstall_error(msg: &str) -> String {
    trf("⚠️ Couldn't remove: %1", &[&clip(msg)])
}

pub fn status_toggled(enabled: bool, ext_id: &str) -> String {
    if enabled {
        trf("▶️ Enabled: %1", &[&ext_id])
    } else {
        trf("⏸️ Disabled: %1", &[&ext_id])
    }
}

pub fn status_toggle_error(msg: &str) -> String {
    trf("⚠️ Couldn't turn it on or off: %1", &[&msg])
}

// ============================================================================
// Подтверждения
// ============================================================================

pub fn confirm_title() -> String {
    tr("Remove Extension?")
}

/// Подтверждение удаления с витрины: коротко, без подробностей.
pub fn confirm_uninstall(name: &str) -> String {
    trf("Remove “%1”?", &[&name])
}

/// Подтверждение удаления из подробного разбора: с указанием каталога.
pub fn confirm_uninstall_detailed(name: &str, dir: &str) -> String {
    trf(
        "Remove “%1”?\n\nThe files in %2 will be deleted permanently.",
        &[&name, &dir],
    )
}

// ============================================================================
// Экранирование
// ============================================================================

/// Экранировать текст для вставки в подпись с разметкой.
///
/// Починка №1: оригинал подставлял имя, автора, описание, теги и README в
/// HTML как есть. Расширение с `<` в описании ломало подпись, а README с
/// угловыми скобками терял на витрине куски текста.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

// ============================================================================
// Плоский разбор расширения
// ============================================================================

/// Расширение, разложенное по полям, которые нужны витрине.
///
/// В ядре [`Extension`] — это каталог, манифест и флаг включения; всё
/// остальное достаётся из манифеста запросами. Витрине нужен плоский набор,
/// причём ровно тот же, что видел Python (`get_info`), поэтому собирается он
/// из `get_info()`, а не вторым разбором манифеста.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExtView {
    pub ext_id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    /// Автор указан в манифесте. ПРИЗНАК, а не сравнение с подписью: раньше
    /// «автор неизвестен» узнавался по тексту «Неизвестно», который с
    /// переводом у каждого языка свой.
    pub author_known: bool,
    pub description: String,
    pub ext_type: String,
    pub type_label: String,
    pub type_icon: String,
    pub tags: Vec<String>,
    pub de_support: Vec<String>,
    pub enabled: bool,
    pub dir: String,
    pub preview_path: String,
    pub icon_path: String,
    pub readme_path: String,
    pub license: String,
    pub homepage: String,
    pub format_version: String,
    pub rexon_min_version: String,
}

fn map_str(info: &Map<String, Value>, key: &str) -> String {
    info.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn map_list(info: &Map<String, Value>, key: &str) -> Vec<String> {
    info.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Значение манифеста как строка «для показа»: строки как есть, числа без
/// кавычек, отсутствующее — пустая строка.
fn manifest_display(manifest: &Map<String, Value>, key: &str) -> String {
    match manifest.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

impl ExtView {
    /// Разложить ядровое расширение по полям витрины.
    pub fn from_extension(ext: &Extension) -> Self {
        let info = ext.get_info();
        let manifest = &ext.manifest;
        // Оригинал спрашивал сначала `rexon_min_version`, потом
        // `min_rexon_version` — оба имени встречались в чужих манифестах.
        let mut rexon_min = manifest_display(manifest, "rexon_min_version");
        if rexon_min.is_empty() {
            rexon_min = manifest_display(manifest, "min_rexon_version");
        }
        ExtView {
            ext_id: map_str(&info, "id"),
            name: map_str(&info, "name"),
            version: map_str(&info, "version"),
            author: map_str(&info, "author"),
            author_known: !matches!(manifest.get("author"), None | Some(Value::Null)),
            description: map_str(&info, "description"),
            ext_type: map_str(&info, "type"),
            type_label: map_str(&info, "type_label"),
            type_icon: map_str(&info, "type_icon"),
            tags: map_list(&info, "tags"),
            de_support: map_list(&info, "de_support"),
            enabled: info
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            dir: ext.dir.to_string_lossy().into_owned(),
            preview_path: map_str(&info, "preview_path"),
            icon_path: map_str(&info, "icon_path"),
            readme_path: map_str(&info, "readme_path"),
            license: map_str(&info, "license"),
            homepage: map_str(&info, "homepage"),
            format_version: manifest_display(manifest, "format_version"),
            rexon_min_version: rexon_min,
        }
    }

    /// Показывать ли автора: пустой и не указанный не сообщают ничего.
    pub fn has_author(&self) -> bool {
        self.author_known && !self.author.is_empty()
    }

    /// Есть ли выключатель (скриптовый тип).
    pub fn toggleable(&self) -> bool {
        is_toggleable(&self.ext_type)
    }

    /// Картинка карточки: превью, иначе иконка, иначе ничего.
    pub fn card_image(&self) -> &str {
        if !self.preview_path.is_empty() {
            &self.preview_path
        } else {
            &self.icon_path
        }
    }

    /// Совпадает ли расширение с поисковым запросом.
    ///
    /// Запрос уже приведён к нижнему регистру и обрезан по краям вызывающим
    /// (см. [`normalize_query`]). Ищем по названию, автору, описанию и тегам
    /// — ровно тот же набор полей, что и в оригинале.
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        self.name.to_lowercase().contains(query)
            || self.author.to_lowercase().contains(query)
            || self.description.to_lowercase().contains(query)
            || self
                .tags
                .iter()
                .any(|t| t.to_lowercase().contains(query))
    }
}

/// Привести поисковый запрос к виду, в котором его сравнивают.
pub fn normalize_query(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Отобрать расширения по типу и запросу — порядок исходного списка.
pub fn apply_filters<'a>(exts: &'a [ExtView], filter_type: &str, query: &str) -> Vec<&'a ExtView> {
    let q = normalize_query(query);
    exts.iter()
        .filter(|e| filter_type == FILTER_ALL_ID || e.ext_type == filter_type)
        .filter(|e| e.matches(&q))
        .collect()
}

// ============================================================================
// Карточка
// ============================================================================

/// Содержимое плитки расширения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    /// Путь к картинке (пусто — рисуем значок типа).
    pub image: String,
    /// Значок-заглушка, если картинки нет.
    pub placeholder: String,
    /// «значок Тип · vВерсия».
    pub type_line: String,
    pub name: String,
    /// Полное имя в подсказке: подпись может быть обрезана многоточием.
    pub name_tooltip: String,
    /// «👤 Автор» либо пусто.
    pub author_line: String,
    pub has_toggle: bool,
    pub state_text: String,
    pub state_color: String,
    pub toggle_text: String,
}

/// Собрать плитку.
pub fn card(ext: &ExtView) -> Card {
    Card {
        image: ext.card_image().to_string(),
        placeholder: ext.type_icon.clone(),
        type_line: format!(
            "{} {}  ·  v{}",
            ext.type_icon, ext.type_label, ext.version
        ),
        name: ext.name.clone(),
        name_tooltip: ext.name.clone(),
        author_line: if ext.has_author() {
            format!("👤 {}", ext.author)
        } else {
            String::new()
        },
        has_toggle: ext.toggleable(),
        state_text: if ext.enabled {
            state_enabled_text()
        } else {
            state_disabled_text()
        },
        state_color: if ext.enabled {
            STATE_ENABLED_COLOR.to_string()
        } else {
            STATE_DISABLED_COLOR.to_string()
        },
        toggle_text: if ext.enabled {
            toggle_off_text()
        } else {
            toggle_on_text()
        },
    }
}

// ============================================================================
// Строка режима «список»
// ============================================================================

/// Компактная строка расширения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListRow {
    pub icon: String,
    /// «<b>Имя</b>  <small>vВерсия</small>».
    pub title_html: String,
    /// «Тип  ·  👤 Автор» либо пусто.
    pub subtitle: String,
    pub has_toggle: bool,
    pub toggle_icon: String,
    pub toggle_tooltip: String,
}

pub fn list_row(ext: &ExtView) -> ListRow {
    ListRow {
        icon: ext.type_icon.clone(),
        title_html: format!(
            "<b>{}</b>  <small>v{}</small>",
            escape_html(&ext.name),
            escape_html(&ext.version)
        ),
        subtitle: if ext.has_author() {
            format!("{}  ·  👤 {}", ext.type_label, ext.author)
        } else {
            String::new()
        },
        has_toggle: ext.toggleable(),
        toggle_icon: if ext.enabled {
            TOGGLE_OFF_ICON.to_string()
        } else {
            TOGGLE_ON_ICON.to_string()
        },
        toggle_tooltip: if ext.enabled {
            toggle_off_tooltip()
        } else {
            toggle_on_tooltip()
        },
    }
}

/// Группа режима «список»: заголовок типа и номера строк внутри.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListGroup {
    pub ext_type: String,
    /// «<b>значок Подпись (N)</b>».
    pub header_html: String,
    /// Номера в переданном списке — порядок внутри группы сохраняется.
    pub rows: Vec<usize>,
}

/// Разложить расширения по типам в порядке таблицы подписей.
///
/// Типы, которых в таблице нет, уходят в конец — как `index(t) if t in
/// type_order else 99` у оригинала, но без его потолка: с 35 типами в
/// таблице «99» просто совпало с «в конец», а на большем числе типов
/// сортировка молча перемешала бы хвост.
pub fn list_groups(exts: &[&ExtView]) -> Vec<ListGroup> {
    let order: Vec<&str> = constants::type_labels_ordered()
        .iter()
        .map(|(t, _)| *t)
        .collect();
    let mut seen: Vec<String> = Vec::new();
    for e in exts {
        if !seen.iter().any(|t| *t == e.ext_type) {
            seen.push(e.ext_type.clone());
        }
    }
    seen.sort_by_key(|t| {
        order
            .iter()
            .position(|o| *o == t.as_str())
            .unwrap_or(order.len())
    });
    seen.into_iter()
        .map(|t| {
            let rows: Vec<usize> = exts
                .iter()
                .enumerate()
                .filter(|(_, e)| e.ext_type == t)
                .map(|(i, _)| i)
                .collect();
            ListGroup {
                header_html: format!(
                    "<b>{} {} ({})</b>",
                    constants::type_icon(&t).unwrap_or(""),
                    constants::type_label(&t).unwrap_or_else(|| t.clone()),
                    rows.len()
                ),
                ext_type: t,
                rows,
            }
        })
        .collect()
}

// ============================================================================
// Подробный разбор расширения
// ============================================================================

pub const DETAIL_MIN_SIZE: (u32, u32) = (680, 540);
pub const DETAIL_TITLE_POINT_SIZE: i32 = 16;
pub const DETAIL_PREVIEW_SIZE: (u32, u32) = (620, 260);
pub fn detail_fallback_title() -> String {
    tr("Extension")
}
pub fn detail_open_folder_text() -> String {
    tr("📁 Open Folder")
}
/// Подпись та же, что на плитке: второй копии текста здесь быть не должно.
pub fn detail_uninstall_text() -> String {
    uninstall_text()
}
pub fn detail_close_text() -> String {
    tr("Close")
}
pub fn detail_no_description() -> String {
    tr("<i>No description</i>")
}
/// Сколько тегов показываем: длинный хвост распирал окно.
pub const DETAIL_TAGS_LIMIT: usize = 20;

// ── Раскладка обоих модальных окон вкладки ─────────────────────────────
//
// Разбор и справка «Создать своё» — окна одного покроя: поле 16, заголовок,
// прокручиваемое тело, ряд кнопок внизу. Числа те же, какими набраны
// компоновки виджетных окон; QML-разметка своих не заводит и берёт их
// картой `styles` ключом `dialog`.

/// Поле вокруг содержимого обоих окон (`setContentsMargins(16, 16, 16, 16)`).
pub const DIALOG_MARGIN: i32 = 16;
/// Шаг между блоками подробного разбора (`layout.setSpacing(10)`).
pub const DETAIL_SPACING: i32 = 10;
/// Шаг между блоками справки: своего она не задаёт вовсе и работает на шаге
/// компоновки по умолчанию — 6 точек (замерено на живом окне под скином).
pub const CREATE_SPACING: i32 = 6;
/// Отступ технической строки разбора сверху и снизу (`padding: 4px 0`).
pub const DETAIL_TECH_PADDING_V: i32 = 4;
/// Прозрачность заливки кнопки «🗑 Удалить» В РАЗБОРЕ. У неё своя, гуще
/// плиточной: 0,25 против [`DANGER_FILL_ALPHA`] = 0,2. Под курсором обе
/// приходят к 0,4 ([`DANGER_FILL_HOVER_ALPHA`]).
pub const DETAIL_DANGER_FILL_ALPHA: f64 = 0.25;

/// Область описания (README, справка): рамка, скругление и внутреннее поле.
///
/// У виджетных окон это `QTextBrowser` — рамка в две точки от стиля,
/// скругление угла около трёх (замерено по снимку) и поле документа 4.
/// Фон области — роль `Base` палитры скина (`bg_secondary`), то есть НЕ фон
/// окна: без него текст висит прямо на подложке окна и рамки области не
/// видно вовсе.
pub const BODY_BORDER_WIDTH: i32 = 1;
pub const BODY_RADIUS: i32 = 3;
pub const BODY_PADDING: i32 = 4;
/// Ниже этого область описания не сжимается — `minimumSizeHint`
/// `QTextBrowser` под скином продукта (замер: 83×83).
///
/// Число здесь НЕ ради красоты: на окне, ужатом до минимума, компоновка
/// делит нехватку места по этой границе. У виджетного окна описание
/// остаётся читаемым, а превью уступает; без границы вся нехватка падала на
/// описание, и оно схлопывалось в полоску (замер снимком 04.09.2026:
/// 21 точка против 85 у виджетов).
pub const BODY_MIN_HEIGHT: i32 = 83;

/// Подробный разбор одного расширения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    /// Заголовок окна.
    pub window_title: String,
    /// «<b>значок Имя</b>».
    pub heading_html: String,
    /// «📦 Тип · vВерсия · 👤 Автор · 🖥 столы».
    pub meta_line: String,
    /// Картинка превью (пусто — не показываем).
    pub preview: String,
    /// README, описание либо «Нет описания» — уже разметкой.
    pub body_html: String,
    /// «#тег #тег» либо пусто.
    pub tags_line: String,
    /// Технические сведения одной строкой разметки.
    pub tech_html: String,
    /// «📁 путь» с домашним каталогом, свёрнутым в `~`.
    pub path_html: String,
    /// Каталог расширения — для кнопки «Открыть папку».
    pub dir: String,
    pub ext_id: String,
}

/// Свернуть домашний каталог в `~`, как показывал оригинал.
pub fn shorten_home(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    path.replace(home, "~")
}

/// Собрать подробный разбор. `readme` — уже прочитанный текст либо `None`.
pub fn detail(ext: &ExtView, readme: Option<&str>, home: &str) -> Detail {
    let mut meta = vec![
        format!("📦 {}", ext.type_label),
        format!("v{}", ext.version),
    ];
    if ext.has_author() {
        meta.push(format!("👤 {}", ext.author));
    }
    if !ext.de_support.is_empty() {
        meta.push(format!("🖥️ {}", ext.de_support.join(", ")));
    }

    let body_html = match readme {
        // Переносимый <pre>: строки README длиннее окна иначе обрезались.
        Some(text) if !text.is_empty() => crate::ui_base::dialog_pre(&escape_html(text)),
        _ if !ext.description.is_empty() => {
            format!("<p>{}</p>", escape_html(&ext.description))
        }
        _ => detail_no_description(),
    };

    let tags_line = if ext.tags.is_empty() {
        String::new()
    } else {
        ext.tags
            .iter()
            .take(DETAIL_TAGS_LIMIT)
            .map(|t| format!("#{t}"))
            .collect::<Vec<_>>()
            .join("  ")
    };

    // Служебный номер пакета и версия формата `.rxn` сюда не идут: человеку
    // они ни о чём не говорят, а «Формат: v3» читался как версия самой темы
    // (вопрос Wehrwolfmann 10.10.2026). Версия темы — в строке под названием.
    let mut tech: Vec<String> = Vec::new();
    if !ext.rexon_min_version.is_empty() {
        tech.push(trf(
            "<b>Minimum Rexon:</b> %1",
            &[&escape_html(&ext.rexon_min_version)],
        ));
    }
    if !ext.homepage.is_empty() {
        let safe = escape_html(&ext.homepage);
        tech.push(trf("<b>Website:</b> %1", &[&format!("<a href='{safe}'>{safe}</a>")]));
    }
    if !ext.license.is_empty() {
        tech.push(trf("<b>License:</b> %1", &[&escape_html(&ext.license)]));
    }

    Detail {
        window_title: if ext.name.is_empty() {
            detail_fallback_title()
        } else {
            ext.name.clone()
        },
        heading_html: format!(
            "<b>{} {}</b>",
            ext.type_icon,
            escape_html(&ext.name)
        ),
        meta_line: meta.join("  ·  "),
        preview: ext.preview_path.clone(),
        body_html,
        tags_line,
        tech_html: tech.join("  ·  "),
        path_html: format!(
            "📁 <tt>{}</tt>",
            escape_html(&shorten_home(&ext.dir, home))
        ),
        dir: ext.dir.clone(),
        ext_id: ext.ext_id.clone(),
    }
}

// ============================================================================
// Справка «Создать своё»
// ============================================================================

pub fn create_title() -> String {
    tr("Create Your Own .rxn Extension")
}
pub const CREATE_MIN_SIZE: (u32, u32) = (720, 580);
pub fn create_header() -> String {
    tr("<b>📦 .rxn v3 Structure</b>")
}
pub fn create_close_text() -> String {
    tr("Close")
}

/// Справка по сборке своего расширения — та же, что показывала виджетная
/// вкладка. Разметку держит ядро: это продуктовый текст, а не оформление.
pub fn create_body_html() -> String {
    // TRANSLATORS: help page in HTML. Keep the tags, the file names, the
    // JSON keys and the type names (theme-gtk, plugin…) as they are;
    // translate the prose and the sample values of "name", "author" and
    // "description".
    // Без ведущего перевода строки: xgettext срезает его у msgid, и
    // строка в коде разошлась бы с ключом каталога.
    tr(r#"<p>A Rexon extension is a plain <b>ZIP archive</b> with the <code>.rxn</code> extension.
Every type has the same layout:</p>

<pre>
extension.rxn (ZIP):
├── manifest.json     ← REQUIRED (format_version: 3)
├── preview.png       ← 620×348 (for the catalog and cards)
├── icon.png          ← 128×128 (lists in Rexon)
├── install.sh        ← for desktop-tool / plugin
├── uninstall.sh      ← for desktop-tool / plugin
├── app.desktop       ← for desktop-tool
├── README.md         ← optional, the description
└── data/             ← the extension's files
</pre>

<h3>📋 Minimal manifest.json</h3>
<pre>
{
  "format_version": 3,
  "id": "my-cool-theme",
  "name": "My Cool Theme",
  "version": "1.0.0",
  "author": "Me",
  "description": "What the extension does",
  "type": "theme-gtk",
  "tags": ["dark", "modern"],
  "license": "MIT",
  "homepage": "https://github.com/me/my-theme",
  "de_support": ["any"]
}
</pre>

<h3>🎨 Categories (the <code>type</code> field)</h3>
<p><b>Appearance</b> — Rexon installs the files automatically:<br>
<code>theme-gtk, theme-icons, theme-cursors, theme-sounds,
theme-kde-global, theme-kde-colors, theme-kde-plasma, theme-kvantum,
theme-window-decoration, theme-login-screen, theme-plymouth, theme-conky,
theme-bundle, theme-plasmoid, wallpaper-pack, font-pack</code></p>

<p><b>Desktop customization</b> — Rexon puts it into that desktop's own
folder and turns it on with the desktop's own tools:<br>
<code>kde-kwin-script, kde-kwin-effect, kde-window-switcher,
kde-wallpaper-plugin, theme-kde-splash, theme-kde-lockscreen,
gnome-shell-extension, theme-gnome-shell, cinnamon-applet, cinnamon-desklet,
cinnamon-extension, theme-cinnamon, nemo-action, theme-xfwm4, theme-marco,
theme-lxqt</code></p>

<p><b>Extensions</b> (you get a warning on install — they run code):<br>
<code>desktop-tool</code>, <code>plugin</code></p>

<h3>📦 Packing</h3>
<pre>
cd my-extension/
zip -r ../my-extension.rxn .
</pre>

<p>Drag the finished <code>.rxn</code> into the Rexon window or click
<b>「📦 From File」</b>.</p>

<h3>🔗 Full documentation</h3>
<p>In the Rexon sources: <code>extensions/constants.py</code> has the full list
of types and install paths.</p>
"#)
}

// ============================================================================
// Живой диск: где лежат расширения и что там стоит
// ============================================================================

// Где лежат расширения, решает ЕДИНАЯ точка ядра
// ([`crate::utils::rexon_config_dir`]), а не своя склейка `~/.config`: она
// стоила бы витрине содержимого. Исполнитель (`rexon/extensions/manager.py`)
// считает свой путь через `utils.expand_user`, то есть С УЧЁТОМ
// `$XDG_CONFIG_HOME`, — и на машине с нестандартным корнем конфигов
// установка шла в один каталог, а витрина читала другой и оставалась
// пустой (правило #12).

/// `$XDG_CONFIG_HOME/rexon/extensions` — тот же каталог, что у Python-менеджера.
pub fn extensions_dir() -> PathBuf {
    crate::utils::rexon_config_dir().join("extensions")
}

/// `$XDG_CONFIG_HOME/rexon/extensions_state.json` — карта «включено ли».
pub fn state_file() -> PathBuf {
    crate::utils::rexon_config_dir().join("extensions_state.json")
}

/// Прочитать карту состояний с диска (отсутствие файла — пустая карта).
pub fn load_state() -> Map<String, Value> {
    let text = std::fs::read_to_string(state_file()).ok();
    ext_manager::load_state(text.as_deref())
}

/// Прочитать установленные расширения С ДИСКА.
///
/// Сестра [`ext_manager::discover`] для живой файловой системы — так же, как
/// [`Extension::load_from_dir`] приходится сестрой `from_manifest`. Семантика
/// та же: каталог с разбираемым `manifest.json`, дубли по `id`
/// перезаписываются последним, порядок — порядок первого появления `id`.
///
/// ⚠️ Имена каталогов сортируются: `read_dir` отдаёт их в порядке файловой
/// системы, а `os.scandir` у Python — в своём. Без сортировки витрина
/// меняла бы порядок карточек от машины к машине.
pub fn installed_in(dir: &Path, state: &Map<String, Value>) -> Vec<Extension> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();

    let mut order: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, Extension> = std::collections::HashMap::new();
    for name in names {
        let path = dir.join(&name);
        if !path.is_dir() {
            continue;
        }
        let Ok(ext) = Extension::load_from_dir(&path, state) else {
            continue;
        };
        let id = ext.ext_id().to_string();
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, ext);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

/// Установленные расширения из штатного каталога, уже разложенные по полям.
pub fn installed() -> Vec<ExtView> {
    let state = load_state();
    installed_in(&extensions_dir(), &state)
        .iter()
        .map(|ext| {
            let mut view = ExtView::from_extension(ext);
            // У дополнения окружения «включено» — правда окружения, а не
            // отметка Rexon: человек мог переключить его в настройках стола.
            if !crate::ext_exec::de_addon_objects(ext).is_empty() {
                view.enabled = crate::ext_exec::de_addon_enabled(ext);
            }
            view
        })
        .collect()
}

/// Прочитать README расширения (для подробного разбора).
///
/// В ядре был только путь (`readme_path`), а сам текст читал Python.
pub fn read_readme(ext: &ExtView) -> Option<String> {
    if ext.readme_path.is_empty() {
        return None;
    }
    std::fs::read_to_string(&ext.readme_path).ok()
}

/// Статистика по разложенным расширениям — счёт один на продукт.
pub fn stats(exts: &[ExtView]) -> Map<String, Value> {
    let pairs: Vec<(String, bool)> = exts
        .iter()
        .map(|e| (e.ext_type.clone(), e.enabled))
        .collect();
    ext_manager::stats(&pairs)
}

/// Типы, которые реально стоят — вход для [`filter_items`].
pub fn installed_types(exts: &[ExtView]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in exts {
        if !out.iter().any(|t| *t == e.ext_type) {
            out.push(e.ext_type.clone());
        }
    }
    out
}
