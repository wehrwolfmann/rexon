//! Обвязка вкладки «Wallpaper Engine» — порт `rexon/qt_wallpaper_engine/window.py`.
//!
//! Само содержимое вкладки — вендоренный QML waywallen (MIT, (c) hypengw,
//! атрибуция в NOTICE), а его данные уже ведут [`crate::we_client`],
//! [`crate::we_query`] и соседи. Модуль-окно оставался ОБВЯЗКОЙ вокруг
//! переехавшего моста: Navigation Rail и его пункты, состояния
//! «подключение…»/«нет демона»/«QML-модули не собраны», скрытая страница
//! «Разработчик» и три сетевых воркера (проверка релизов, установка
//! AppImage, установка OWE-плагина).
//!
//! Здесь живёт всё, что в этой обвязке РЕШАЕТСЯ и НАЗЫВАЕТСЯ: состав рейла и
//! соответствие «пункт рейла → страница стека», разбор `REXON_DEV`, тексты и
//! цвета статусов dev-страницы, пошаговые вердикты воркеров, вердикт
//! health-ответа демона и таблицы стилей. Qt-стороне остаётся строить
//! виджеты по готовому плану — ровно та же граница, что у
//! [`crate::qt_settings`] и [`crate::onboarding`].
//!
//! Сеть, файловая система и WebSocket сюда не заходят: пути и разборы уже
//! есть в [`crate::engine_update`], сверка схемы — в [`crate::we_parity`].
//!
//! ⚠️ Починки оригинала (правило #16 — баги при порте чиним, не консервируем),
//! каждая помечена `Починка:` по месту:
//!   1. воркер установки AppImage удалял скачанный ~200 МиБ временный файл
//!      только при отмене и провале валидации. Провалилась САМА установка —
//!      файл оставался: `install_appimage` зовёт `os.remove(new_path)` лишь
//!      на успешной ветке, а воркер за собой не убирал. На машине с `/tmp`
//!      в ОЗУ это утечка оперативной памяти до перезагрузки. Теперь вердикт
//!      каждого шага несёт флаг `remove_tmp`, и провал установки его ставит;
//!   2. таймаут IPC-установки OWE-плагина (60 с) взводился одноразовым
//!      таймером без опознания попытки. Вторая установка, начатая в течение
//!      минуты после ответа демона на первую, получала СТАРЫЙ таймер: он
//!      удалял её временный zip и рапортовал ложный «демон не ответил».
//!      Теперь у попытки есть номер, и вердикт таймаута его сверяет;
//!   3. подпись версии движка называлась двумя разными именами: до первой
//!      проверки «Версия движка: —», после — «Версия waywallen: …». Одно
//!      поле, две подписи; теперь имя одно.
//!
//! Сверка — `rust/rexon-core/tests/we_window_parity.rs`.

// ============================================================================
// Скрытый режим разработчика
// ============================================================================

/// Переменная окружения разового включения/выключения dev-режима.
pub const DEV_ENV_VAR: &str = "REXON_DEV";

/// Маркер-файл постоянного dev-режима (раскрывается `expand_user`, который
/// уже уважает `XDG_CONFIG_HOME` — второй развилки для XDG здесь не нужно).
pub const DEV_MARKER_PATH: &str = "~/.config/rexon/dev_mode";

/// Во что разбирается значение `REXON_DEV`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevEnv {
    /// `1`/`true`/`yes` — включить, маркер не спрашивать.
    ForceOn,
    /// `0`/`false`/`no` — выключить даже при маркере («как увидит покупатель»).
    ForceOff,
    /// Пусто или что-то другое — решает маркер-файл.
    Unset,
}

/// Разбор `REXON_DEV` (регистр и окружающие пробелы не важны).
pub fn parse_dev_env(raw: &str) -> DevEnv {
    match raw.trim().to_lowercase().as_str() {
        "0" | "false" | "no" => DevEnv::ForceOff,
        "1" | "true" | "yes" => DevEnv::ForceOn,
        _ => DevEnv::Unset,
    }
}

/// Итог `is_dev_mode()`: env старше маркера в обе стороны.
pub fn dev_mode(raw_env: &str, marker_exists: bool) -> bool {
    match parse_dev_env(raw_env) {
        DevEnv::ForceOff => false,
        DevEnv::ForceOn => true,
        DevEnv::Unset => marker_exists,
    }
}

use crate::i18n::n_;
use std::collections::BTreeMap;

// ============================================================================
// Navigation Rail и страницы
// ============================================================================

/// Ширина свёрнутого Navigation Rail, пикс.
pub const RAIL_WIDTH: i32 = 96;

/// Буква-логотип над пунктами рейла.
pub const RAIL_LOGO: &str = "W";

/// Минимальная высота кнопки пункта рейла, пикс.
pub const RAIL_BUTTON_MIN_HEIGHT: i32 = 56;

/// Пометка на кнопке «Разраб.» при расхождении паритета.
pub const RAIL_DRIFT_MARK: &str = " ⚠️";

/// Скругление кнопки пункта рейла, пикс. — ПОЛОВИНА её высоты.
///
/// ⭐ Своего числа здесь больше нет. У waywallen скругления-константы не
/// существует вовсе: подсветку выбранного пункта его рейл рисует
/// «пилюлей» — `radius: height / 2` (`qml/navigation/RailItem.qml`,
/// строки 148 и 156 модуля `Qcm.Material`, редакция закреплена в
/// `rexon/qt_wallpaper_engine/qml_ui/waywallen/ui/QML_MATERIAL_PIN`).
/// Прежние 12 пикс. были НАШИ и ни на что у вендора не опирались: они
/// приехали из таблицы стилей нашей же виджетной реплики. Здесь записано
/// то же ПРАВИЛО, а не подогнанное под него число, — поменяется высота
/// пункта, поедет и скругление.
///
/// ⚠️ Именованной константа остаётся не ради красоты: у вкладки две
/// версии, и QML-версия рисует кнопки сама, а не таблицей стилей Qt. Пока
/// число жило литералом внутри `rail_qss`, спросить его QML было неоткуда
/// — и рейл на QML вышел с чужим скруглением. Вычислять `height / 2` на
/// месте тоже нельзя: таблица стилей Qt считать не умеет, ей нужны пиксели.
pub const RAIL_BUTTON_RADIUS: i32 = RAIL_BUTTON_MIN_HEIGHT / 2;

/// Размер надписи на кнопке пункта рейла, пикс.
///
/// ⭐ Число ВЕНДОРНОЕ, а не наше: свёрнутый пункт рейла waywallen подписан
/// начертанием `MD.Token.typescale.label_medium` (`RailItem.qml`, строка
/// 111), а `label_medium` в `Qcm.Material` — ровно 12 пикс.
/// (`include/qml_material/token/type_scale.hpp`, строка 106; та же
/// закреплённая редакция). Совпало с кеглем нашей виджетной реплики —
/// значит менять нечего, но источником считается вендор.
///
/// ⚠️ Снять константу нельзя по тому же доводу, что у скругления: обе
/// версии вкладки рисуют подпись сами, спросить кегль у `Qcm.Material` им
/// неоткуда. Модуль `Qcm.Material` в `WallpaperEnginePage.qml` не
/// импортируется НАМЕРЕННО — иначе вкладка не откроется там, где
/// QML-плагины не собраны (довод записан в шапке того файла).
pub const RAIL_BUTTON_FONT_SIZE: i32 = 12;

/// Один пункт рейла и, для QML-страниц, файл её QML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailItem {
    /// Идентификатор пункта (`wallpapers_qml`, `dev`, …).
    pub id: String,
    /// Эмодзи-иконка.
    pub icon: String,
    /// Подпись под иконкой — уже на языке интерфейса (конструкторы
    /// получают английский msgid и переводят его сами).
    pub label: String,
    /// Имя QML-файла страницы; пусто для не-QML пунктов.
    pub qml: String,
    /// Страница ВЕНДОРНАЯ (файл внутри `qml_ui/` waywallen)?
    ///
    /// ⚠️ Признак завёлся не для порядка. Пока «есть имя QML» значило
    /// «страница вендорная», у нашей собственной QML-страницы
    /// («Разработчик») не было способа получить имя файла: любое непустое
    /// имя тут же уезжало в склейку вендорного пути
    /// (`qml_modules::vendor_page_path`) — и страница искалась бы в чужом
    /// каталоге. Теперь вопросов два: «есть ли у пункта QML» и «чей он».
    pub vendor: bool,
    /// Показывать ли вендорную ШАПКУ страницы (`MD.AppBar` с её заголовком
    /// и кнопками действий).
    ///
    /// ⭐ Признак не про красоту, а про то, доступна ли страница вообще.
    /// Шапку `MD.Page` рисует по `MD.MProp.page.showHeader` — свойству
    /// ХОЗЯИНА страницы, а не самой страницы
    /// (`Qcm.Material/qml/control/Page.qml`, строка 25). У `PageContext`
    /// это `false` по умолчанию (`src/control/page_context.cpp`, строка
    /// 71), и глобальный контекст, который достаётся странице без хозяина,
    /// умолчания не меняет.
    ///
    /// У waywallen хозяев ДВА, и решают они по-разному:
    ///   * окно с рейлом (`ui/qml/Window.qml`, `MD.PageContext` контейнера
    ///     страниц) — `showHeader: false`;
    ///   * всплывающее окно страницы (`component/PagePopup.qml`,
    ///     `MD.PageContext`) — `showHeader: true`.
    /// Страницы рейла либо шапки не имеют вовсе, либо включают её СЕБЕ
    /// САМИ (`SettingsPage.qml` — `showHeader: true`, `StatusPage.qml` и
    /// `DisplaysPage.qml` — только на узком окне). Страницы всплывающего
    /// окна на своё `showHeader` не полагаются: за них его ставит
    /// `PagePopup`.
    ///
    /// ⚠️ Наш рейл держит и те и другие, поэтому решение о шапке принимает
    /// хозяин — `qml_ui/PageHost.qml`, — а знание о том, какая страница
    /// чья, лежит ЗДЕСЬ, одно на обе версии вкладки. Без него вендорная
    /// страница из всплывающего окна открывается в рейле БЕЗ шапки, то
    /// есть без своего заголовка и без ВСЕХ кнопок действий: у
    /// `PluginManagePage.qml` это «Неактивные плагины», «Проверить
    /// обновления» и «Установить из .zip» — вся установка и всё обновление
    /// плагинов разом. Список плагинов при этом рисуется, и снимок
    /// выглядит рабочим (поймано живым снимком 08.09.2026).
    ///
    /// Правило, по которому признак ставится, сторожит
    /// `tests/we_rail_vendor_parity.rs`: он читает саму вендорную страницу.
    pub header: bool,
}

impl RailItem {
    /// Пункт с ВЕНДОРНОЙ страницей waywallen, которая шапку себе рисует
    /// сама либо не имеет её вовсе — как в окне самого waywallen.
    fn vendor(id: &str, icon: &str, label: &str, qml: &str) -> Self {
        Self {
            id: id.to_string(),
            icon: icon.to_string(),
            label: crate::i18n::tr(label),
            qml: qml.to_string(),
            vendor: true,
            header: false,
        }
    }

    /// Пункт с ВЕНДОРНОЙ страницей, которая у waywallen живёт во
    /// всплывающем окне и потому ждёт шапку ОТ ХОЗЯИНА (см. `header`).
    fn vendor_headed(id: &str, icon: &str, label: &str, qml: &str) -> Self {
        Self {
            header: true,
            ..Self::vendor(id, icon, label, qml)
        }
    }

    /// Пункт с НАШЕЙ страницей (QML-модуль `org.rexon.gui`) либо вовсе без
    /// страницы — тогда имя файла пустое.
    fn own(id: &str, icon: &str, label: &str, qml: &str) -> Self {
        Self {
            id: id.to_string(),
            icon: icon.to_string(),
            label: crate::i18n::tr(label),
            qml: qml.to_string(),
            vendor: false,
            header: false,
        }
    }
}

/// Спеки QML-страниц вкладки (свитч миграции 2026-07-12). Все страницы
/// показывает ОДИН общий `SharedQmlHost`: несколько `QQmlEngine` с модулем
/// `waywallen.control` дедлочат `QQmlThread`.
pub fn qml_page_specs() -> Vec<RailItem> {
    vec![
        RailItem {
            label: wallpapers_rail_label(),
            ..RailItem::vendor("wallpapers_qml", "🖼️", "Wallpapers", "WallpaperPage.qml")
        },
        RailItem::vendor("discover_qml", "🔍", n_("Discover"), "DiscoverPage.qml"),
        RailItem::vendor("displays_qml", "🖥️", n_("Displays"), "DisplaysPage.qml"),
        RailItem::vendor("status_qml", "📈", n_("Status"), "StatusPage.qml"),
        // ⚠️ «Плагины» вписаны СЮДА, между «Состоянием» и «Настройками», не
        // по вкусу: у waywallen это нижняя тройка рейла в порядке «Плагины,
        // Настройки, О программе» (`ui/qml/Window.qml`, подвал рейла).
        //
        // У вендора все три — не страницы рейла, а кнопки, открывающие
        // всплывающее окно (`presentPopup('waywallen.ui/PagePopup', {source:
        // …})`). Две из трёх у нас давно живут пунктами-страницами: свой
        // рейл — это список страниц, и «Настройки» с «О программе» уже
        // приняты в таком виде. Третья просто пропала при переносе, и
        // покупатель не мог добраться до управления плагинами вовсе —
        // установить, обновить, удалить рендерер.
        //
        // Страница — та же самая вендорная (`page/PluginManagePage.qml`,
        // `MD.Page`, как «Настройки» и «О программе»), и всё, на чём она
        // держится, у нас уже есть: шесть типов запросов
        // (`PluginListQuery`, `PluginInstallQuery`, `PluginInspectQuery`,
        // `PluginDeleteQuery`, `PluginUpdateCheckQuery`,
        // `PluginUpdateInstallQuery`) печатает крейт `rexon-gui`
        // (`src/plugin_queries.rs`), а её собственное всплывающее окно
        // «неактивные плагины» зовёт `Window.window.presentPopup` — метод,
        // который ставит на окно наш шелл `qml_ui/PageHost.qml`.
        //
        // ⚠️ Пункт заведён `vendor_headed`, а не `vendor`: у waywallen эта
        // страница живёт во всплывающем окне, и её заголовок с тремя
        // кнопками («Неактивные плагины», «Проверить обновления»,
        // «Установить из .zip») лежит в ШАПКЕ, которую включает хозяин.
        // Заведённая обычной вендорной, она открывалась БЕЗ шапки: список
        // плагинов виден, а поставить и обновить нечем (см. `header`).
        RailItem::vendor_headed("plugins_qml", "🧩", n_("Plugins"), "PluginManagePage.qml"),
        RailItem::vendor("settings_qml", "⚙️", n_("Settings"), "SettingsPage.qml"),
        RailItem::vendor("about_qml", "ℹ️", n_("About"), "AboutPage.qml"),
    ]
}

/// Пункт-заглушка, когда QML-плагины не собраны.
pub fn no_qml_rail_item() -> RailItem {
    RailItem {
        label: wallpapers_rail_label(),
        ..RailItem::own("no_qml", "🖼️", "Wallpapers", "")
    }
}

/// Подпись пункта «Обои» в рейле — со своим контекстом перевода.
///
/// Рейл шириной 96 px, и подпись типа оформления «Wallpapers»
/// (`constants.rs`, wallpaper-pack) туда не влезает: немецкое
/// «Hintergrundbilder» Qt ужимал до 8 px (снимок 01.10.2026). Контекст
/// разводит две строки, чтобы в рейле стояло короткое «Hintergründe».
fn wallpapers_rail_label() -> String {
    // TRANSLATORS: label of the wallpapers page in a narrow 96 px side rail — keep it short
    crate::i18n::trc("side rail", "Wallpapers")
}

/// Имя файла НАШЕЙ страницы «Разработчик» в QML-модуле `org.rexon.gui`.
///
/// Вендорной страницы у пункта нет и быть не может: страницу пишем мы, а
/// не waywallen. Поэтому имя лежит здесь, а не в `qml_page_specs`.
pub const DEV_PAGE_QML: &str = "WeDeveloperPage.qml";

/// Скрытый пункт «Разработчик».
pub fn dev_rail_item() -> RailItem {
    // TRANSLATORS: short label of the hidden developer page in a 96 px rail
    RailItem::own("dev", "🛠️", n_("Dev"), DEV_PAGE_QML)
}

/// План постройки рейла и стека страниц.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailPlan {
    /// Пункты рейла по порядку; индекс пункта == индекс страницы в стеке.
    pub items: Vec<RailItem>,
    /// Индекс страницы-заглушки «QML-модули не собраны», если она есть.
    pub stub_index: Option<usize>,
    /// Индекс dev-страницы, если она есть.
    pub dev_index: Option<usize>,
}

impl RailPlan {
    /// Имя QML-файла для страницы стека `idx`; `None` — не QML-страница.
    pub fn qml_for_index(&self, idx: usize) -> Option<&str> {
        self.items
            .get(idx)
            .filter(|it| !it.qml.is_empty())
            .map(|it| it.qml.as_str())
    }

    /// Имя файла ВЕНДОРНОЙ страницы для `idx`; `None` — страница либо наша
    /// собственная, либо её нет вовсе.
    ///
    /// Спрашивать надо именно это везде, где имя уезжает в склейку пути
    /// внутрь `qml_ui/`: наша страница там не лежит.
    pub fn vendor_qml_for_index(&self, idx: usize) -> Option<&str> {
        self.items
            .get(idx)
            .filter(|it| it.vendor && !it.qml.is_empty())
            .map(|it| it.qml.as_str())
    }

    /// Нужна ли странице `idx` вендорная ШАПКА от хозяина (см.
    /// `RailItem::header`). Нет такого пункта — `false`: шапка по
    /// умолчанию выключена и у самого `PageContext`.
    pub fn header_for_index(&self, idx: usize) -> bool {
        self.items.get(idx).is_some_and(|it| it.header)
    }
}

/// Собрать план рейла: `qml_available` — собраны ли QML-плагины,
/// `dev_mode` — показывать ли скрытую страницу «Разработчик».
///
/// Индекс пункта рейла и индекс страницы в `QStackedWidget` совпадают в
/// обоих случаях — на этом держится `_nav()`.
pub fn build_rail(qml_available: bool, dev_mode: bool) -> RailPlan {
    let mut items = if qml_available {
        qml_page_specs()
    } else {
        vec![no_qml_rail_item()]
    };
    let stub_index = if qml_available { None } else { Some(0) };
    let mut dev_index = None;
    if dev_mode {
        dev_index = Some(items.len());
        items.push(dev_rail_item());
    }
    RailPlan {
        items,
        stub_index,
        dev_index,
    }
}

/// Подпись кнопки рейла: иконка и подпись через перевод строки, плюс
/// пометка расхождения паритета для «Разраб.».
pub fn rail_button_label(icon: &str, label: &str, drift: bool) -> String {
    format!(
        "{icon}\n{label}{}",
        if drift { RAIL_DRIFT_MARK } else { "" }
    )
}

// ============================================================================
// Цвета и таблицы стилей
// ============================================================================

/// Прозрачность приглушённого текста (`on_surface_variant` оригинала).
pub const VARIANT_ALPHA: f32 = 0.6;

/// Прозрачность подсветки выбранного пункта рейла.
pub const RAIL_SELECTED_ALPHA: f32 = 0.25;

/// Прозрачность подложки полосы рейла на ТЁМНОМ скине.
///
/// ⚠️ Число выехало из таблицы стилей в именованную константу по той же
/// причине, что скругление и кегль кнопки: полосу рейла QML-версия рисует
/// сама, и пока число жило литералом внутри `rail_qss`, у неё была своя
/// копия — `Qt.rgba(1, 1, 1, 0.03)` в разметке.
pub const RAIL_BG_ALPHA_DARK: f32 = 0.03;

/// Прозрачность подложки полосы рейла на СВЕТЛОМ скине.
pub const RAIL_BG_ALPHA_LIGHT: f32 = 0.02;

/// Подложка полосы рейла строкой `#AARRGGBB` — в этом виде цвет берёт
/// QML-версия.
///
/// ⚠️ Прозрачность отдаётся ВОСЬМИБИТНЫМ числом, а не долей, и это не
/// придирка. Долю `0.03` парсер таблиц стилей Qt усекает до 7/255, а
/// смеситель сцены QML считает по самой доле (7,65/255) — на одном и том
/// же скине полоса выходила у версий разной на уровень: (33,46,61) у
/// виджетов против (34,47,62) у QML (замер 04.09.2026 на steam_dark,
/// фон 27,40,56). Восьмибитная альфа одна на обе стороны, и уровень
/// совпадает.
pub fn rail_background_argb_hex(dark: bool) -> String {
    let (alpha, rgb) = if dark {
        (RAIL_BG_ALPHA_DARK, 0x00ff_ffff_u32)
    } else {
        (RAIL_BG_ALPHA_LIGHT, 0x0000_0000_u32)
    };
    // Усечение, а НЕ округление: ровно так долю читает парсер QSS.
    let a = (alpha * 255.0) as u8;
    format!("#{a:02x}{rgb:06x}")
}

/// Подсветка ВЫБРАННОГО пункта рейла строкой `#AARRGGBB` — в этом виде
/// краску берёт QML-версия. `hl_*` — цвет роли `Highlight` скина.
///
/// ⚠️ Та же ловушка, что у подложки полосы, и ловится она только точным
/// сличением: долю `0.25` парсер таблиц стилей Qt усекает до 63/255, а
/// сцена QML считала по самой доле (63,75/255) и округляла премножённую
/// краску вверх — подсветка выходила у версий разной на уровень:
/// (50,82,106) у виджетов против (51,82,107) у QML (замер 04.09.2026 на
/// steam_dark, полоса рейла 33,46,61, `Highlight` #66c0f4; расхождение
/// на 4224 точках выбранного пункта). Отдельный опыт на тех же красках:
/// доля `0.25` в сцене даёт (51,82,107), восьмибитная альфа `0x40` —
/// тоже (51,82,107), а `0x3f` — ровно (50,82,106) виджетной версии.
pub fn rail_selected_argb_hex(hl_r: u8, hl_g: u8, hl_b: u8) -> String {
    // Усечение, а НЕ округление: ровно так долю читает парсер QSS.
    let a = (RAIL_SELECTED_ALPHA * 255.0) as u8;
    format!("#{a:02x}{hl_r:02x}{hl_g:02x}{hl_b:02x}")
}

// ============================================================================
// Цвета вендорного интерфейса: скин Rexon или схема самого waywallen
// ============================================================================

/// Чем красить вендорный интерфейс waywallen внутри вкладки.
///
/// Его элементы рисует QmlMaterial, и краски у него не таблица, а схема
/// Material, которую плагин СЧИТАЕТ из одного цвета-затравки (акцента),
/// тёмности и типа палитры. Поэтому «как в остальных вкладках» складывается
/// из трёх частей: затравка — акцент скина Rexon, тёмность — тёмность
/// скина, а фон и основной текст шелла — ровно краски страницы скина (их
/// плагин берёт из наследуемых `MProp.backgroundColor` и `MProp.textColor`).
/// Поверх посчитанной схемы фоны, поверхности и текст задаются красками
/// скина поштучно ([`VendorTheme::roles`]) — у QmlMaterial со свойством
/// `MdColorMgr::overrides`. У более старого QmlMaterial роли поштучно не
/// задаются, и карточки, диалоги и поля остаются тонами схемы от акцента.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorTheme {
    /// Красить схемой самого waywallen: его тип палитры, его настройки
    /// «Тема» и «Акцент» в окне «Настройки» вкладки. Остальные поля тогда
    /// шеллу не нужны (ими красятся только стартовые экраны страницы).
    pub follow_vendor: bool,
    /// Затравка схемы Material — акцент скина, `#rrggbb`.
    pub accent: String,
    /// Фон шелла и рейки — фон страницы скина (`surface_page`), `#rrggbb`.
    pub background: String,
    /// Основной текст поверх фона — `text_primary` скина, `#rrggbb`.
    pub text: String,
    /// Тёмный ли скин — им выбирается тёмная или светлая схема Material.
    pub dark: bool,
    /// Роли схемы Material, заданные красками скина поштучно: имя роли
    /// QmlMaterial → `#rrggbb` (см. [`VENDOR_SCHEME_ROLES`]). Кладутся в
    /// `MD.Token.color.overrides`; у QmlMaterial без этого свойства шелл их
    /// молча пропускает, и действует одна затравка. У схемы самого
    /// waywallen — пусто.
    pub roles: BTreeMap<String, String>,
}

/// Какие роли схемы Material красятся скином и какой его краской.
///
/// Раскладка — как в самом waywallen: окно у него `surface_container`, и
/// карточки настроек и списков — тоже `surface_container`, то есть ровно
/// фон окна; рабочая область под ними (`surface`) — на тон глубже, а
/// плашки и чипы (`surface_container_high`/`_highest`) — на тон светлее.
/// У скина Rexon окно — фон страницы, поэтому карточки вкладки красятся им
/// же, рабочая область — краской поля (у «Тёмного (Steam)» `#16202d`),
/// приподнятое — карточкой и кнопкой соседних вкладок. Текст —
/// `text_primary`, вторичный (у waywallen это почти все подписи) —
/// `text_secondary`.
pub const VENDOR_SCHEME_ROLES: &[(&str, &str)] = &[
    ("background", "surface_field"),
    ("surface", "surface_field"),
    ("surface_dim", "surface_field"),
    ("surface_container_lowest", "surface_field"),
    ("surface_container_low", "surface_page"),
    ("surface_container", "surface_page"),
    ("surface_container_high", "surface_card"),
    ("surface_container_highest", "surface_button"),
    ("surface_variant", "surface_button"),
    ("surface_bright", "surface_button"),
    ("on_background", "text_primary"),
    ("on_surface", "text_primary"),
    ("on_surface_variant", "text_secondary"),
    ("outline", "surface_field_outline"),
    ("outline_variant", "surface_button"),
];

/// Цвета вендорного интерфейса для скина `skin_id`.
///
/// По умолчанию вкладка следует скину Rexon, как все остальные вкладки.
/// Схему самого waywallen возвращает ОТДЕЛЬНЫЙ скин
/// [`crate::ui_base::VENDOR_SCHEME_SKIN`] — тогда шелл не трогает ни тип
/// палитры, ни тему, ни акцент, и работают собственные настройки waywallen.
/// Краски берутся из той же палитры, что уходит QML-страницам
/// ([`crate::ui_base::qml_palette`]): незнакомый скин откатывается к
/// стандартному там же.
pub fn vendor_theme(skin_id: &str) -> VendorTheme {
    use crate::ui_base;
    let skin = ui_base::skin(skin_id).id;
    let palette = ui_base::qml_palette(skin);
    let pick = |key: &str, fallback: &str| {
        palette
            .get(key)
            .filter(|v| ui_base::parse_hex(v).is_some())
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    let background = pick("surface_page", "#1b2838");
    let dark = ui_base::parse_hex(&background)
        .map(|(r, g, b)| ui_base::is_dark_rgb(r, g, b))
        .unwrap_or(true);
    let follow_vendor = skin == ui_base::VENDOR_SCHEME_SKIN;
    let roles = if follow_vendor {
        BTreeMap::new()
    } else {
        VENDOR_SCHEME_ROLES
            .iter()
            .filter_map(|(role, key)| {
                let value = palette.get(*key)?;
                ui_base::parse_hex(value)?;
                Some(((*role).to_string(), value.clone()))
            })
            .collect()
    };
    VendorTheme {
        follow_vendor,
        accent: pick("accent", "#66c0f4"),
        background,
        text: pick("text_primary", if dark { "#ffffff" } else { "#000000" }),
        dark,
        roles,
    }
}

/// Роли схемы [`VendorTheme::roles`] объектом JSON — в таком виде их берёт
/// QML (`JSON.parse` → `MD.Token.color.overrides`).
pub fn vendor_roles_json(theme: &VendorTheme) -> String {
    serde_json::to_string(&theme.roles).unwrap_or_else(|_| "{}".to_string())
}

/// То же для ДЕЙСТВУЮЩЕГО скина (сохранённый `ui_skin`).
pub fn active_vendor_theme() -> VendorTheme {
    vendor_theme(crate::ui_base::active_skin_id())
}

/// Кегль буквы-логотипа над пунктами рейла, пикс.
///
/// Тот же довод, что у остальных чисел рейла: букву QML-версия рисует
/// сама, и своей копии кегля у неё быть не должно.
pub const LOGO_FONT_SIZE: i32 = 22;

/// Насыщенность буквы-логотипа (`font-weight`).
pub const LOGO_FONT_WEIGHT: i32 = 700;

/// `on_surface_variant` — цвет роли `Text` с альфой 60%.
pub fn variant_color(r: u8, g: u8, b: u8) -> String {
    format!("rgba({r},{g},{b},{VARIANT_ALPHA})")
}

/// Стиль буквы-логотипа над рейлом (`hl` — `#rrggbb` роли `Highlight`).
pub fn logo_qss(hl_hex: &str) -> String {
    format!(
        "font-size: {LOGO_FONT_SIZE}px; font-weight: {LOGO_FONT_WEIGHT}; \
color: {hl_hex};"
    )
}

/// Таблица стилей рейла: фон полосы, форма кнопок, подсветка выбранного.
pub fn rail_qss(dark: bool, hl_r: u8, hl_g: u8, hl_b: u8) -> String {
    let rail_bg = if dark {
        format!("rgba(255,255,255,{RAIL_BG_ALPHA_DARK})")
    } else {
        format!("rgba(0,0,0,{RAIL_BG_ALPHA_LIGHT})")
    };
    let sel_bg = format!("rgba({hl_r},{hl_g},{hl_b},{RAIL_SELECTED_ALPHA})");
    format!(
        "QFrame#weRail {{ background: {rail_bg}; }}\
QFrame#weRail QToolButton {{ border-radius: {RAIL_BUTTON_RADIUS}px; \
font-size: {RAIL_BUTTON_FONT_SIZE}px; padding: 4px; }}\
QFrame#weRail QToolButton:checked {{ background: {sel_bg}; }}"
    )
}

/// Текст с готовой таблицей стилей — то, что виджету остаётся поставить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styled {
    /// Текст метки.
    pub text: String,
    /// Таблица стилей метки.
    pub qss: String,
}

impl Styled {
    fn new(text: impl Into<String>, qss: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            qss: qss.into(),
        }
    }
}

/// Строка статуса до того, как она превратилась в таблицу стилей: текст,
/// краска и насыщенность по отдельности.
///
/// ⚠️ Заведено потому, что у вкладки ДВЕ версии. Виджетной нужна готовая
/// таблица стилей ([`Styled`]), сцене QML — краска сама по себе: `color:`
/// у `Text` таблиц стилей не понимает. Решение «какого цвета эта строка»
/// обязано быть ОДНО (правило #12), поэтому первичен `StatusLine`, а
/// `Styled` из него выводится.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// Текст строки.
    pub text: String,
    /// Краска `#rrggbb`; пусто — краски нет, цвет обычный.
    pub color: String,
    /// Полужирная ли строка.
    pub bold: bool,
}

impl StatusLine {
    fn new(text: impl Into<String>, color: &str, bold: bool) -> Self {
        Self {
            text: text.into(),
            color: color.to_string(),
            bold,
        }
    }

    /// Та же строка с готовой таблицей стилей — вид для виджетной версии.
    pub fn to_styled(&self) -> Styled {
        if self.color.is_empty() {
            return Styled::new(self.text.clone(), STATUS_NEUTRAL_QSS);
        }
        Styled::new(self.text.clone(), status_qss(&self.color, self.bold))
    }
}

/// Зелёный «всё хорошо».
pub const COLOR_OK: &str = "#2e7d32";
/// Оранжевый «есть обновление / рассинхрон».
pub const COLOR_WARN: &str = "#e65100";
/// Красный «ошибка / расхождение».
pub const COLOR_ERR: &str = "#c62828";

fn status_qss(color: &str, bold: bool) -> String {
    if bold {
        format!("font-size: 13px; color: {color}; font-weight: 600;")
    } else {
        format!("font-size: 13px; color: {color};")
    }
}

// ============================================================================
// Стартовые состояния вкладки
// ============================================================================

/// Индекс экрана «Подключение…» в стеке состояний.
pub const STATE_CONNECTING: usize = 0;
/// Индекс экрана «Демон недоступен» в стеке состояний.
pub const STATE_NO_DAEMON: usize = 1;
/// Индекс экрана со страницами в стеке состояний.
pub const STATE_PAGES: usize = 2;

/// Задержка отложенного старта (детект демона, авто-проверка upstream), мс.
pub const STARTUP_DELAY_MS: i32 = 100;

/// Сколько ждать ответа `health` от подключившегося демона, мс.
pub const HEALTH_TIMEOUT_MS: i32 = 5000;

/// Сколько ждать ответа демона на `plugin_install`, мс.
pub const OWE_IPC_TIMEOUT_MS: i32 = 60_000;

/// Сколько ждать завершения сетевых воркеров на выходе из приложения, мс.
pub const WORKER_WAIT_MS: i32 = 60_000;

/// СКОЛЬКО ВСЕГО вкладка стоит на экране «Подключение…», мс.
///
/// ⚠️ Без этого срока ожидание было БЕСКОНЕЧНЫМ, и это не теория: у двух
/// путей к экрану ожидания своего сторожа нет вовсе.
///   * движок числится на шине, а сокет не открывается — `open()` заводит
///     соединение и больше не возвращается ни успехом, ни отказом;
///   * соединение было и оборвалось — транспорт переподключается каждые
///     3 с БЕЗ предела попыток (`we_socket::RECONNECT_MS`), и пока движок
///     мёртв, покупатель смотрит на «Подключение…» столько, сколько готов
///     смотреть. Кнопка «Повторить запуск» при этом не появляется НИКОГДА.
///
/// Откуда 25 секунд. Замер холодного старта демона на этой машине
/// (изолированный стенд: своя шина, свой `XDG_RUNTIME_DIR`, свои
/// data/config/cache; ждали ровно того же, чего ждёт вкладка, — `WsPort`
/// на D-Bus): имя на шине — 0,07–0,19 с, рабочий порт — 3,2 / 3,4 / 3,2 с
/// на прогретой базе и 3,2 с на первом запуске с миграциями; под нагрузкой
/// (машина занята сборкой) — 5,0 с. То есть нормальный старт укладывается
/// в 3–5 с, и 25 с — пятикратный запас к худшему замеру.
///
/// ⚠️ Верхняя граница тут важнее нижней: срок ОБЯЗАН быть больше суммы
/// внутренних сторожей (12 с поллинга шины [`crate::we_client::POLL_LIMIT`]
/// × [`crate::we_client::POLL_MS`] + [`HEALTH_TIMEOUT_MS`]), иначе он
/// сработает первым и подменит точную причину отказа общей. Меняя одно из
/// трёх чисел, сверяйтесь с `we_window_parity.rs` — там это записано
/// проверкой, а не пожеланием.
///
/// ⚠️ С 30.10.2026 срок НЕ идёт, пока работает сторож запуска движка
/// ([`crate::we_client::StartWatch`]): тот ждёт живой процесс дольше 12 с
/// (первый запуск AppImage на медленной машине) и сам кончается отказом
/// по своим срокам. Разметка держит этот таймер выключенным, пока объект
/// действий говорит `engineStarting`, — иначе 25 с подменили бы честное
/// «запускается…» общим отказом.
pub const CONNECT_TIMEOUT_MS: i32 = 25_000;

/// Текст экрана ожидания.
///
/// «Демон» — слово системного администратора, а покупателю оно не говорит
/// ничего (правило #13). Само имя `waywallen` остаётся латиницей: это имя
/// технологии, а не перевод.
pub fn text_connecting() -> String {
    crate::i18n::tr("Starting the waywallen wallpaper engine…")
}

/// Пояснение под «Запускается…», когда запуск идёт дольше обычного
/// (тихое окно сторожа запуска `we_client::START_QUIET_MS` позади), а процесс
/// движка жив и работает. Без него человек смотрел бы на тот же экран
/// минуту и решил бы, что всё зависло (дефект 30.10.2026, r1895).
pub fn text_connecting_slow() -> String {
    crate::i18n::tr(
        "The first start after installation can take a minute or two while the engine \
unpacks itself. Please wait.",
    )
}

/// Текст экрана отказа, когда причина неизвестна.
pub fn text_no_daemon() -> String {
    crate::i18n::tr("The waywallen wallpaper engine did not start")
}

/// Иконка экрана «Демон недоступен».
pub const ICON_NO_DAEMON: &str = "🔌";

/// Подпись кнопки повторного запуска демона.
pub fn text_retry_daemon() -> String {
    crate::i18n::tr("Retry Start")
}

/// Кегль текста стартовых экранов («подключение…», «демон недоступен»),
/// пикс.
///
/// ⚠️ Числа стартовых экранов выехали из таблиц стилей виджетной вкладки
/// в именованные константы по той же причине, что и у рейла: у вкладки
/// две версии, и QML-версия рисует эти экраны сама. Пока числа жили
/// литералами в `window.py`, QML-версия держала свою копию — и экран
/// «демон недоступен» разъехался с оригиналом по зазорам (замер 03.09).
pub const STATE_TEXT_FONT_SIZE: i32 = 15;

/// Кегль значка-эмодзи на экране «Демон недоступен», пикс.
pub const NO_DAEMON_ICON_FONT_SIZE: i32 = 48;

/// Зазор между значком, текстом и кнопкой стартового экрана, пикс. У
/// оригинала это умолчание `QVBoxLayout` под стилем продукта (замерено
/// 03.09 на живом объекте: 6); здесь оно записано явно, чтобы обе версии
/// брали одно число, а не каждая своё умолчание.
pub const STATE_COLUMN_SPACING: i32 = 6;

/// Поля колонки стартового экрана, пикс. Тот же довод, что у зазора.
pub const STATE_COLUMN_MARGIN: i32 = 6;

/// Стиль значка на экране «Демон недоступен» — виджетной версии.
pub fn no_daemon_icon_qss() -> String {
    format!("font-size: {NO_DAEMON_ICON_FONT_SIZE}px;")
}

/// Стиль текста на экране «Демон недоступен» — виджетной версии.
pub fn no_daemon_text_qss() -> String {
    format!("font-size: {STATE_TEXT_FONT_SIZE}px;")
}

/// Текст заглушки, когда `Qcm.Material` собран под другую младшую версию
/// Qt, чем запущенная (`qml_modules::incompatible_qcm`). Такую сборку
/// вкладка не грузит: частный API Qt между выпусками несовместим, и
/// страницы падали (см. `qml_modules`, 09.10.2026).
pub fn text_qml_modules_wrong_qt(built: (u32, u32), running: (u32, u32)) -> String {
    crate::i18n::tr(
        "The Wallpaper Engine interface components were built for Qt {built}, \
but the system now runs Qt {running}. They are not loaded, so the tab does not crash.\n\n\
Update or reinstall Rexon so they are rebuilt for the new Qt version, then restart Rexon.",
    )
    .replace("{built}", &format!("{}.{}", built.0, built.1))
    .replace("{running}", &format!("{}.{}", running.0, running.1))
}

/// Текст заглушки, когда QML-плагины не собраны.
pub fn text_no_qml_modules() -> String {
    crate::i18n::tr("The interface QML modules are not built.\n\n\
Build them and restart Rexon:\n\
  bash tools/build_qml_material.sh\n\
  bash tools/build_qml_control.sh")
}

// ── Экран отказа: слова покупателю, подробности — в журнал ──────────────────
//
// ⚠️ Так было до 18.09.2026: причину отказа показывали покупателю ДОСЛОВНО,
// как её составил клиент демона. Человек, купивший программу ради обоев,
// читал на весь экран «Ожидается вшитый AppImage (rexon/vendor/),
// ~/.local/share/waywallen/waywallen.AppImage, в PATH или ~/.local/bin/
// waywallen» — четыре пути, слово PATH и ни одного указания, что ему,
// собственно, делать (правило #13). Теперь сырая строка идёт в журнал, а на
// экране стоит то, что покупатель может ПОНЯТЬ и ВЫПОЛНИТЬ.
//
// Разбирать чужую строку приходится потому, что канал у причины один — она
// проходит через сигнал объекта действий и разметку, где никаких типов нет.
// Чтобы разбор не разъехался с самими сообщениями, опознание идёт по
// ИСХОДНЫМ функциям [`crate::we_client`], а не по переписанным сюда копиям
// их текста, и каждое опознание закреплено проверкой в `we_window_parity.rs`.

/// Почему вкладка показывает экран отказа — в понятиях покупателя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonFailure {
    /// Движка нет на диске вовсе.
    NotInstalled,
    /// Файл движка есть, но запустить его не вышло.
    LaunchFailed,
    /// Запустили и не дождались готовности.
    StartTimeout,
    /// Запустился, но на запросы молчит.
    NotResponding,
    /// Ответил, но работать не готов (старая версия, чужая база).
    NotReady,
    /// Причина не опознана — слова общие, подробности в журнале.
    Unknown,
}

/// Начало причины, которую складывает [`health_verdict`]: по нему отказ
/// опознаётся как «ответил, но не готов». Своей копии текста ниже нет —
/// сообщение собирается из этой же константы.
const REASON_NOT_READY_HEAD: &str = "движок ответил, но не готов";

/// Причина для журнала, когда `health` не ответил вовсе.
///
/// ⚠️ Это СЫРАЯ причина (её пишет журнал и читает [`daemon_failure`]), а не
/// подпись для человека: подпись складывает [`no_daemon_text`].
pub const TEXT_HEALTH_TIMEOUT: &str = "движок подключился, но не отвечает на \
проверку готовности (health)";

/// Причина для журнала, когда истёк общий срок ожидания
/// ([`CONNECT_TIMEOUT_MS`]).
pub const REASON_CONNECT_TIMEOUT: &str = "движок не вышел на связь за \
отведённое время: сокет не открылся либо соединение рвётся раз за разом";

/// Опознать причину отказа по сырой строке клиента демона.
pub fn daemon_failure(reason: &str) -> DaemonFailure {
    let reason = reason.trim();
    if reason.is_empty() {
        return DaemonFailure::Unknown;
    }
    if reason == crate::we_client::daemon_not_found_message() {
        return DaemonFailure::NotInstalled;
    }
    if reason == crate::we_client::daemon_poll_timeout_message()
        || reason.starts_with(crate::we_client::START_TIMEOUT_HEAD)
    {
        return DaemonFailure::StartTimeout;
    }
    // Процесс движка умер, не выйдя на связь: для человека это то же
    // «движок не запускается», что и неудачный запуск файла.
    if reason == crate::we_client::daemon_exited_message() {
        return DaemonFailure::LaunchFailed;
    }
    // Сообщение неудачного запуска несёт путь к бинарю — сверяем начало,
    // собранное той же функцией, а не переписанное сюда руками.
    let launch_head = crate::we_client::daemon_launch_failed_message("");
    let launch_head = launch_head.trim_end();
    if reason.starts_with(launch_head) {
        return DaemonFailure::LaunchFailed;
    }
    if reason == TEXT_HEALTH_TIMEOUT || reason == REASON_CONNECT_TIMEOUT {
        return DaemonFailure::NotResponding;
    }
    if reason.starts_with(REASON_NOT_READY_HEAD) {
        return DaemonFailure::NotReady;
    }
    DaemonFailure::Unknown
}

/// Текст экрана отказа — то, что читает покупатель (Rexon не из пакета).
///
/// Форма для копии Rexon, поставленной не пакетом дистрибутива; на экране
/// продукта — [`no_daemon_text_in`] с настоящим признаком установки.
pub fn no_daemon_text(reason: &str) -> String {
    no_daemon_text_in(reason, false)
}

/// Текст экрана отказа — то, что читает покупатель.
///
/// Каждый случай отвечает на два его вопроса: что случилось и что ему
/// делать. Машинных подробностей здесь нет ни в одном варианте — они уходят
/// строкой [`daemon_failure_log_line`] в журнал Rexon.
///
/// `packaged` — Rexon поставлен пакетом дистрибутива, то есть движок обоев
/// приехал вместе с ним — с 30.10.2026 отдельным пакетом-зависимостью
/// `waywallen` (`we_client::SYSTEM_DAEMON`). Только тогда совет
/// «переустановите пакет waywallen» правдив: переустановка вернёт файл движка.
///
/// ⚠️ До 28.09.2026 экран «движок не установлен» говорил «он ставится
/// вместе с Rexon — переустановите Rexon» ВСЕМ, хотя в пакете движка не было
/// вовсе, и переустановка ничего не меняла. А «работать не готов» уверял,
/// что движок «устарел», хотя причину (состояние `health`) экран не знает.
/// Теперь каждый совет выполним и не обещает того, чего не будет.
pub fn no_daemon_text_in(reason: &str, packaged: bool) -> String {
    use crate::i18n::tr;
    match (daemon_failure(reason), packaged) {
        (DaemonFailure::NotInstalled, true) => tr(
            "The waywallen wallpaper engine is missing.\n\nThe Rexon package \
installs it as a separate system package, waywallen, but its file is not in \
place. Reinstall the waywallen package, and wallpapers will work.",
        ),
        (DaemonFailure::NotInstalled, false) => tr(
            "The waywallen wallpaper engine is not installed.\n\nThis copy of \
Rexon was installed without the engine. Install Rexon from its package — the \
engine comes with it — and wallpapers will work.",
        ),
        (DaemonFailure::LaunchFailed, true) => tr(
            "The waywallen wallpaper engine does not start.\n\nPress “Retry \
Start”. If that does not help, reinstall the waywallen package: the engine \
file may be damaged.",
        ),
        (DaemonFailure::LaunchFailed, false) => tr(
            "The waywallen wallpaper engine does not start.\n\nPress “Retry \
Start”. If that does not help, restart the computer; the details are in the \
Rexon log.",
        ),
        (DaemonFailure::StartTimeout, _) => tr(
            "The waywallen wallpaper engine is taking too long to start.\n\n\
Press “Retry Start”. If that does not help, restart the computer.",
        ),
        (DaemonFailure::NotResponding, _) => tr(
            "The waywallen wallpaper engine started but does not respond.\n\n\
Press “Retry Start”. If that does not help, restart the computer.",
        ),
        (DaemonFailure::NotReady, _) => tr(
            "The waywallen wallpaper engine started but reports that it is not \
ready to work.\n\nPress “Retry Start”. If that does not help, restart the \
computer; the details are in the Rexon log.",
        ),
        (DaemonFailure::Unknown, _) => tr(
            "The waywallen wallpaper engine did not start.\n\nPress “Retry \
Start”. If that does not help, restart the computer; the details are in the \
Rexon log.",
        ),
    }
}

/// Строка журнала с СЫРОЙ причиной отказа — та самая машинная кухня, что
/// раньше стояла на экране.
///
/// Пустая причина строки не даёт: писать в журнал «отказ без причины»
/// значит засорять его на каждое открытие вкладки без движка.
pub fn daemon_failure_log_line(reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return None;
    }
    Some(format!(
        "rexon: вкладка Wallpaper Engine, движок не поднялся — {reason}"
    ))
}

/// Вердикт по ответу `health` подключившегося демона.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthVerdict {
    /// Показывать страницы (демон здоров либо старый, без поля `health`).
    Ready,
    /// Показывать экран отказа с этой СЫРОЙ причиной.
    NotReady(String),
}

/// Разбор ответа `health`: пустое состояние — старый демон, ему верим.
///
/// WS-соединение само по себе ещё не значит, что демон рабочий (мог упасть
/// на миграции БД) — без этой проверки у покупателя чёрный экран вместо обоев.
pub fn health_verdict(state: &str) -> HealthVerdict {
    let state = state.trim().to_lowercase();
    if state.is_empty() || state == "healthy" {
        return HealthVerdict::Ready;
    }
    HealthVerdict::NotReady(format!("{REASON_NOT_READY_HEAD} (состояние: {state})"))
}

// ============================================================================
// Страница «Разработчик»
// ============================================================================

/// Ключ настройки «авто-УСТАНОВКА обновлений» (по умолчанию выключена).
pub const KEY_AUTO_DOWNLOAD: &str = "dev_engine_auto_download";
/// Ключ настройки «авто-ПРОВЕРКА upstream при запуске» (включена).
pub const KEY_AUTO_UPSTREAM: &str = "dev_auto_upstream_check";
/// Ключ настройки «авто-сверка паритета при открытии страницы» (включена).
pub const KEY_AUTO_PARITY: &str = "dev_auto_parity_check";

/// Значение по умолчанию для [`KEY_AUTO_DOWNLOAD`].
pub const DEFAULT_AUTO_DOWNLOAD: bool = false;
/// Значение по умолчанию для [`KEY_AUTO_UPSTREAM`].
pub const DEFAULT_AUTO_UPSTREAM: bool = true;
/// Значение по умолчанию для [`KEY_AUTO_PARITY`].
pub const DEFAULT_AUTO_PARITY: bool = true;

/// Заголовок dev-страницы.
pub fn dev_title() -> String {
    crate::i18n::tr("Developer")
}
/// Стиль заголовка dev-страницы.
pub const DEV_TITLE_QSS: &str = "font-size: 18px; font-weight: 600;";
/// Подзаголовок dev-страницы.
pub fn dev_subtitle() -> String {
    crate::i18n::tr("Hidden page (REXON_DEV=1). Not available to buyers in the Steam build.")
}
/// Стиль заголовка секции dev-страницы.
pub const DEV_SECTION_QSS: &str = "font-size: 15px; font-weight: 500;";
/// Стиль моноширинных панелей (changelog, отчёт о паритете).
pub const DEV_MONO_QSS: &str = "font-family: monospace; font-size: 12px;";

// ── Числа dev-страницы ──────────────────────────────────────────────────
//
// ⚠️ Те же числа, что стоят в таблицах стилей и раскладках выше, но
// поимённо. Тот же довод, что у чисел рейла и стартовых экранов: страницу
// рисуют ДВЕ версии вкладки, и сцена QML таблиц стилей Qt не понимает —
// пока число живёт только внутри строки `"font-size: 18px; …"`, спросить
// его разметке QML неоткуда, и у неё заводится своя копия, которая молча
// разъезжается. Сверку «строка стиля собрана из этих чисел» держит тест
// `rust/rexon-core/tests/we_window_parity.rs`.

/// Кегль заголовка dev-страницы, пикс.
pub const DEV_TITLE_FONT_SIZE: i32 = 18;
/// Насыщенность заголовка dev-страницы.
pub const DEV_TITLE_FONT_WEIGHT: i32 = 600;
/// Кегль заголовка секции, пикс.
pub const DEV_SECTION_FONT_SIZE: i32 = 15;
/// Насыщенность заголовка секции.
pub const DEV_SECTION_FONT_WEIGHT: i32 = 500;
/// Кегль моноширинных панелей (changelog, отчёт о паритете), пикс.
pub const DEV_MONO_FONT_SIZE: i32 = 12;
/// Кегль приглушённых подписей (подзаголовок, строка хода операции), пикс.
pub const DEV_MUTED_FONT_SIZE: i32 = 12;
/// Кегль строк статуса целей и бейджа паритета, пикс.
pub const DEV_STATUS_FONT_SIZE: i32 = 13;
/// Насыщенность подписи над панелью changelog.
pub const DEV_CHANGELOG_LABEL_WEIGHT: i32 = 500;
/// Поля страницы, пикс (`root.setContentsMargins(12, 12, 12, 12)`).
pub const DEV_PAGE_MARGIN: i32 = 12;
/// Зазор между секциями страницы, пикс.
pub const DEV_COLUMN_SPACING: i32 = 12;
/// Поля панели секции, пикс (`sv.setContentsMargins(16, 16, 16, 16)`).
pub const DEV_PANE_PADDING: i32 = 16;
/// Зазор внутри панели секции, пикс.
pub const DEV_PANE_SPACING: i32 = 8;
/// Зазор в ряду кнопок, пикс. У виджетной версии это умолчание
/// `QHBoxLayout` под стилем продукта — то же число, что у зазора
/// стартовых экранов; здесь оно записано явно, чтобы обе версии брали
/// одно, а не каждая своё умолчание.
pub const DEV_ROW_SPACING: i32 = 6;
/// Наименьшая высота панели changelog, пикс.
pub const DEV_CHANGELOG_MIN_HEIGHT: i32 = 120;
/// Наибольшая высота панели changelog, пикс.
pub const DEV_CHANGELOG_MAX_HEIGHT: i32 = 260;

/// Подпись кнопки согласия в вопросах страницы.
///
/// У виджетной версии её ставит сам Qt (`QMessageBox.StandardButton.Yes`);
/// сцена QML рисует кнопки вопроса сама, и подпись ей нужна словом.
pub fn dialog_yes() -> String {
    crate::i18n::tr("Yes")
}
/// Подпись кнопки отказа в вопросах страницы.
pub fn dialog_no() -> String {
    crate::i18n::tr("No")
}

/// Стиль приглушённой подписи (подзаголовок, строка статуса операции).
pub fn muted_qss(variant_color: &str, size_px: i32) -> String {
    format!("font-size: {size_px}px; color: {variant_color};")
}

/// Заголовок секции обновлений upstream.
pub fn section_upstream() -> String {
    crate::i18n::tr("Upstream Updates")
}
/// Заголовок секции отчёта о паритете.
pub fn section_parity() -> String {
    crate::i18n::tr("UI Parity Report")
}

/// Подпись галки авто-проверки upstream.
pub fn cb_auto_upstream() -> String {
    crate::i18n::tr("Check for upstream updates on startup (network)")
}
/// Подпись галки авто-установки.
pub fn cb_auto_download() -> String {
    crate::i18n::tr("Install waywallen and OWE plugin updates automatically (after validation, with .bak)")
}
/// Подпись галки авто-сверки паритета.
pub fn cb_auto_parity() -> String {
    crate::i18n::tr("Check UI parity when the page opens")
}

/// Подпись кнопки «Проверить сейчас».
pub fn btn_check() -> String {
    crate::i18n::tr("Check Now")
}
/// Подпись кнопки установки движка.
pub fn btn_update() -> String {
    crate::i18n::tr("Download and Install waywallen")
}
/// Подпись кнопки установки OWE-плагина.
pub fn btn_owe() -> String {
    crate::i18n::tr("Download and Install OWE Plugin")
}
/// Подпись кнопки «отметить изученным».
pub fn btn_seen() -> String {
    crate::i18n::tr("OWE: Mark as Reviewed")
}
/// Подпись кнопки копирования changelog.
pub fn btn_copy() -> String {
    crate::i18n::tr("Copy All")
}
/// Подпись кнопки сверки паритета.
pub fn btn_parity() -> String {
    crate::i18n::tr("Compare with Engine")
}
/// Подпись над панелью changelog.
pub fn label_changelog() -> String {
    crate::i18n::tr("What's changed:")
}
/// Заглушка пустой панели changelog.
pub fn placeholder_changelog() -> String {
    crate::i18n::tr("The list of releases with release notes will appear after the check.")
}
/// Начальный текст панели отчёта о паритете.
pub fn placeholder_parity() -> String {
    crate::i18n::tr("Press “Compare with Engine” or open the page.")
}

/// Цели, по которым dev-страница показывает отдельную строку статуса:
/// `(ключ цели, показываемое имя)`.
pub const STATUS_TARGETS: [(&str, &str); 2] =
    [("waywallen", "waywallen"), ("owe", "open-wallpaper-engine")];

/// Подпись версии движка.
///
/// ⚠️ Починка 3: до первой проверки оригинал писал «Версия движка: —», а
/// после — «Версия waywallen: …». Одно поле не должно называться двумя
/// именами; здесь имя одно, а пустая версия остаётся честным прочерком.
pub fn version_label(version: &str) -> String {
    if version.trim().is_empty() {
        crate::i18n::tr("waywallen version: (unknown)")
    } else {
        crate::i18n::trf("waywallen version: %1", &[&version])
    }
}

/// Та же подпись до первой проверки (версию ещё не спрашивали).
pub fn version_label_initial() -> String {
    crate::i18n::trf("waywallen version: %1", &[&"—"])
}

/// Стиль строки статуса до первой проверки — цвета ещё нет.
pub const STATUS_NEUTRAL_QSS: &str = "font-size: 13px;";

/// Начальная строка статуса цели.
pub fn target_status_initial_parts(display_name: &str) -> StatusLine {
    StatusLine::new(crate::i18n::trf("%1: not checked yet", &[&display_name]), "", false)
}

/// Она же с готовой таблицей стилей — для виджетной версии.
pub fn target_status_initial(display_name: &str) -> Styled {
    target_status_initial_parts(display_name).to_styled()
}

/// Строка статуса цели по результату проверки.
///
/// `ok=false` — цель не проверилась (сеть/рейт-лимит), красный без пометки.
/// Иначе «⬆️» при доступном обновлении (оранжевый) либо «✅» (зелёный).
pub fn target_status_parts(
    ok: bool,
    title: &str,
    note: &str,
    update_available: bool,
) -> StatusLine {
    if !ok {
        return StatusLine::new(format!("{title}: {note}"), COLOR_ERR, false);
    }
    let mark = if update_available { "⬆️" } else { "✅" };
    let color = if update_available { COLOR_WARN } else { COLOR_OK };
    StatusLine::new(format!("{mark} {title}: {note}"), color, false)
}

/// Она же с готовой таблицей стилей — для виджетной версии.
pub fn target_status_line(ok: bool, title: &str, note: &str, update_available: bool) -> Styled {
    target_status_parts(ok, title, note, update_available).to_styled()
}

/// Строка статуса waywallen после успешной установки.
pub fn waywallen_installed_parts() -> StatusLine {
    StatusLine::new(
        crate::i18n::tr("✅ waywallen (interface app): updated, daemon restart required"),
        COLOR_OK,
        false,
    )
}

/// Она же с готовой таблицей стилей — для виджетной версии.
pub fn waywallen_installed_line() -> Styled {
    waywallen_installed_parts().to_styled()
}

/// Строка статуса OWE после успешной установки плагина.
pub fn owe_installed_parts(version: &str) -> StatusLine {
    let v = if version.trim().is_empty() { "?" } else { version };
    StatusLine::new(
        crate::i18n::trf("✅ open-wallpaper-engine (renderer engine): plugin %1 installed", &[&v]),
        COLOR_OK,
        false,
    )
}

/// Она же с готовой таблицей стилей — для виджетной версии.
pub fn owe_installed_line(version: &str) -> Styled {
    owe_installed_parts(version).to_styled()
}

/// Строка статуса OWE после «отметить изученным».
pub fn owe_seen_parts(title: &str, version: &str) -> StatusLine {
    StatusLine::new(
        crate::i18n::trf("✅ %1: %2 marked as reviewed", &[&title, &version]),
        COLOR_OK,
        false,
    )
}

/// Она же с готовой таблицей стилей — для виджетной версии.
pub fn owe_seen_line(title: &str, version: &str) -> Styled {
    owe_seen_parts(title, version).to_styled()
}

/// Статус «идёт проверка upstream».
pub fn text_check_running() -> String {
    crate::i18n::tr("Checking upstream…")
}
/// Статус «проверка закончилась».
pub fn text_check_done() -> String {
    crate::i18n::tr("Check complete.")
}
/// Статус «проверка не удалась» (сеть/рейт-лимит — без крешей).
pub fn text_check_failed() -> String {
    crate::i18n::tr("Could not check for updates (network / GitHub rate limit).")
}
/// Статус после копирования changelog.
pub fn text_changelog_copied() -> String {
    crate::i18n::tr("Changelog copied to the clipboard.")
}
/// Панель changelog, когда изменений нет.
pub fn text_changelog_empty() -> String {
    crate::i18n::tr("No changes relative to our version.")
}

/// Что показать в панели changelog: собранный текст либо честное «изменений нет».
pub fn changelog_panel_text(rendered: &str) -> String {
    if rendered.is_empty() {
        text_changelog_empty()
    } else {
        rendered.to_string()
    }
}

/// Подтверждение установки нового движка.
pub fn confirm_update_text(latest: &str, asset_url: &str) -> String {
    let v = if latest.trim().is_empty() { "?" } else { latest };
    crate::i18n::trf(
        "A new engine version is out (%1).\nDownload and install it?\n\nSource:\n%2",
        &[&v, &asset_url],
    )
}

/// Заголовок окна подтверждения установки движка.
pub fn confirm_update_title() -> String {
    crate::i18n::tr("Engine Update")
}

/// Подтверждение установки OWE-плагина.
pub fn confirm_owe_text(latest: &str, asset_url: &str) -> String {
    let v = if latest.trim().is_empty() { "?" } else { latest };
    crate::i18n::trf(
        "Install open-wallpaper-engine %1 as a waywallen plugin?\n\nSource:\n%2",
        &[&v, &asset_url],
    )
}

/// Заголовок окна подтверждения установки OWE-плагина.
pub fn confirm_owe_title() -> String {
    crate::i18n::tr("OWE Plugin Installation")
}

/// Итоговая строка установки движка.
pub fn install_done_text(ok: bool, message: &str) -> String {
    if ok {
        crate::i18n::trf("Installed: %1 Restart the daemon to apply.", &[&message])
    } else {
        crate::i18n::trf("Error: %1", &[&message])
    }
}

/// Итоговая строка установки OWE-плагина.
pub fn owe_done_text(ok: bool, message: &str) -> String {
    if ok {
        crate::i18n::trf("OWE plugin installed: %1", &[&message])
    } else {
        crate::i18n::trf("OWE plugin error: %1", &[&message])
    }
}

/// Строка отказа гейта совместимости waywallen ≥ 0.2.3.
pub fn owe_gate_refused_text(why: &str) -> String {
    crate::i18n::trf("OWE plugin not installed: %1", &[&why])
}

/// Статус «отдали zip демону».
pub fn text_owe_ipc_running() -> String {
    crate::i18n::tr("OWE plugin: installing through the daemon…")
}

/// Сообщение об успешной установке через демон.
pub fn owe_ipc_success_text(plugin_id: &str, needs_restart: bool) -> String {
    // Две целые фразы, а не склейка «…через демон.» + хвост: порядок слов
    // у переводчика свой.
    if needs_restart {
        crate::i18n::trf(
            "plugin %1 installed through the daemon. Daemon restart required.",
            &[&plugin_id],
        )
    } else {
        crate::i18n::trf(
            "plugin %1 installed through the daemon. The daemon restarted the renderers (hot-install).",
            &[&plugin_id],
        )
    }
}

/// Сообщение отказа демона, когда он не прислал своего текста.
pub fn text_owe_ipc_refused() -> String {
    crate::i18n::tr("the daemon rejected the plugin installation")
}

/// Сообщение истёкшего таймаута IPC-установки.
pub fn text_owe_ipc_timeout() -> String {
    crate::i18n::tr("the daemon did not answer the plugin installation within 60 seconds — check its log and try again")
}

/// Сработал ли таймаут IPC-установки на самом деле.
///
/// ⚠️ Починка 2: одноразовый таймер оригинала не знал, какую попытку он
/// сторожит. Ответ демона на первую установку приходил быстро, а таймер жил
/// ещё до минуты — и следующая установка, начатая в этом окне, получала
/// чужой будильник: он удалял её временный zip и рапортовал ложный «демон
/// не ответил за 60 секунд». Теперь у попытки есть номер, и таймаут молчит,
/// если он не от текущей.
pub fn owe_ipc_timeout_fires(armed_attempt: u64, current_attempt: u64, waiting: bool) -> bool {
    waiting && armed_attempt == current_attempt
}

// ============================================================================
// Сверка паритета
// ============================================================================

/// Текст панели, когда демон не подключён.
pub fn text_parity_no_daemon() -> String {
    crate::i18n::tr("Daemon not connected.")
}

/// Текст панели, когда движок ответил ошибкой без своего сообщения.
pub fn text_parity_request_failed() -> String {
    crate::i18n::tr("Schema request to the engine failed")
}

/// Шапка отчёта: версия живого демона против тега нашего proto-снапшота.
pub fn parity_report_header(daemon_version: &str, proto_tag: &str) -> String {
    let dv = if daemon_version.trim().is_empty() {
        crate::i18n::tr("(version unknown)")
    } else {
        daemon_version.to_string()
    };
    let tag = if proto_tag.trim().is_empty() {
        crate::i18n::tr("(tag not recorded)")
    } else {
        proto_tag.to_string()
    };
    format!(
        "{}\n",
        crate::i18n::trf("Daemon: %1 · Rexon proto snapshot: %2", &[&dv, &tag])
    )
}

/// Бейдж сверки паритета.
///
/// `proto_match`: `Some(false)` — версия демона разошлась с нашим снапшотом
/// протокола (тогда «✓» вводил бы в заблуждение и не показывается вовсе),
/// `None` — версию демона узнать не удалось, `Some(true)` — сошлись.
/// Бейдж покрывает ТОЛЬКО схему renderer-настроек, не весь UI-паритет.
/// `unchecked_count` — сколько настроек сверить было не с чем (у движка нет
/// renderer'а нужного типа): «✓» остаётся, но с оговоркой и цветом предупреждения.
pub fn parity_badge_parts(
    proto_match: Option<bool>,
    daemon_version: &str,
    proto_tag: &str,
    has_drift: bool,
    issue_count: usize,
    warning_count: usize,
    unchecked_count: usize,
) -> StatusLine {
    if proto_match == Some(false) {
        let dv = if daemon_version.trim().is_empty() {
            "?"
        } else {
            daemon_version
        };
        let tag = if proto_tag.trim().is_empty() { "?" } else { proto_tag };
        return StatusLine::new(
            crate::i18n::trf(
                "⚠️ daemon %1 ≠ protocol snapshot %2 — proto-resync needed",
                &[&dv, &tag],
            ),
            COLOR_WARN,
            true,
        );
    }
    if !has_drift {
        let text = if proto_match == Some(true) {
            crate::i18n::tr("renderer settings schema")
        } else {
            crate::i18n::tr("renderer settings schema (could not compare the daemon version)")
        };
        // Значок — по состоянию: цветной знак цвета текста не слушается, так
        // что оговорку «не проверено» несёт он сам, а не только оранжевый текст.
        if unchecked_count > 0 {
            let note = crate::i18n::trf("not checked: %1", &[&unchecked_count]);
            return StatusLine::new(format!("⚠️ {text} · {note}"), COLOR_WARN, false);
        }
        return StatusLine::new(
            format!("✅ {text}"),
            COLOR_OK,
            false,
        );
    }
    let text = if warning_count > 0 {
        crate::i18n::trf("⚠️ mismatches: %1 (%2 important)", &[&issue_count, &warning_count])
    } else {
        crate::i18n::trf("⚠️ mismatches: %1", &[&issue_count])
    };
    StatusLine::new(
        text,
        COLOR_ERR,
        true,
    )
}

/// Тот же бейдж с готовой таблицей стилей — для виджетной версии.
pub fn parity_badge(
    proto_match: Option<bool>,
    daemon_version: &str,
    proto_tag: &str,
    has_drift: bool,
    issue_count: usize,
    warning_count: usize,
    unchecked_count: usize,
) -> Styled {
    parity_badge_parts(
        proto_match,
        daemon_version,
        proto_tag,
        has_drift,
        issue_count,
        warning_count,
        unchecked_count,
    )
    .to_styled()
}

/// Показывать ли пометку на кнопке «Разраб.»: расхождение схемы ИЛИ
/// разошедшийся снапшот протокола.
pub fn parity_drift_flag(has_drift: bool, proto_match: Option<bool>) -> bool {
    has_drift || proto_match == Some(false)
}

// ============================================================================
// Сетевые воркеры: пошаговые вердикты
// ============================================================================

/// Чем закончился шаг воркера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerOutcome {
    /// Успех или провал (уходит в сигнал `done`).
    pub ok: bool,
    /// Сообщение для строки статуса.
    pub message: String,
    /// Удалить ли скачанный временный файл (см. починку 1).
    pub remove_tmp: bool,
    /// Отдать ли путь к временному файлу GUI-потоку (IPC-установка плагина).
    pub hand_over_tmp: bool,
}

impl WorkerOutcome {
    fn fail(message: impl Into<String>, remove_tmp: bool) -> Self {
        Self {
            ok: false,
            message: message.into(),
            remove_tmp,
            hand_over_tmp: false,
        }
    }
}

/// Сообщение прогресса скачивания: `prefix` пуст для движка, [`prefix_owe`]
/// для плагина. Нулевой `total` — сервер не сказал размера, прогресс молчит.
pub fn download_progress(prefix: &str, done_bytes: u64, total_bytes: u64) -> Option<String> {
    if total_bytes == 0 {
        return None;
    }
    Some(format!(
        "{prefix}{}",
        crate::i18n::trf(
            "Downloading… %1 / %2 MiB",
            &[&(done_bytes >> 20), &(total_bytes >> 20)]
        )
    ))
}

/// Префикс сообщений воркера движка (его нет).
pub const PREFIX_ENGINE: &str = "";
/// Префикс сообщений воркера OWE-плагина.
pub fn prefix_owe() -> String {
    crate::i18n::tr("OWE plugin: ")
}

/// Стартовое сообщение воркера («Скачивание…» / «OWE-плагин: скачивание…»).
pub fn download_start_text(prefix: &str) -> String {
    if prefix.is_empty() {
        crate::i18n::tr("Downloading…")
    } else {
        format!("{prefix}{}", crate::i18n::tr("downloading…"))
    }
}

/// Сообщение шага валидации движка.
pub fn text_validate_appimage() -> String {
    crate::i18n::tr("Checking the new AppImage…")
}
/// Сообщение шага валидации плагин-zip.
pub fn text_validate_plugin() -> String {
    crate::i18n::tr("OWE plugin: checking the archive…")
}
/// Сообщение шага ФС-установки плагина (демон не запущен).
pub fn text_install_plugin_fs() -> String {
    crate::i18n::tr("OWE plugin: installing (daemon not running)…")
}

/// Сообщение отмены при выходе из приложения.
pub fn text_cancelled_on_quit() -> String {
    crate::i18n::tr("cancelled (the application is closing)")
}

/// Вердикт после шага скачивания.
///
/// `tmp_ready` — файл получен; `interrupted` — попросили выйти из приложения.
/// `reason` — текст исключения исполнителя, если он был: без него человек
/// видел «не удалось/отменено» и не мог отличить обрыв сети от ошибки
/// импорта ядра (живой случай 09.09.2026: колесо `rexon_py` пересобрали
/// под работающим процессом, ленивый импорт упал, а причину съел
/// `except Exception` — две неудачи подряд остались без диагноза).
/// `None` — идём дальше.
pub fn after_download(
    prefix: &str,
    tmp_ready: bool,
    interrupted: bool,
    reason: Option<&str>,
) -> Option<WorkerOutcome> {
    if !tmp_ready {
        let head = if prefix.is_empty() {
            crate::i18n::tr("download failed")
        } else {
            crate::i18n::tr("OWE plugin download failed")
        };
        let what = match reason.map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => format!("{head}: {r}"),
            None => crate::i18n::trf("%1/cancelled", &[&head]),
        };
        // файла нет — удалять нечего
        return Some(WorkerOutcome::fail(what, false));
    }
    if interrupted {
        return Some(WorkerOutcome::fail(text_cancelled_on_quit(), true));
    }
    None
}

/// Вердикт после шага валидации. `None` — идём дальше.
pub fn after_validate(is_plugin: bool, ok: bool, message: &str) -> Option<WorkerOutcome> {
    if ok {
        return None;
    }
    let text = if is_plugin {
        crate::i18n::trf("plugin zip is invalid: %1", &[&message])
    } else {
        crate::i18n::trf("AppImage is invalid: %1", &[&message])
    };
    Some(WorkerOutcome::fail(text, true))
}

/// Вердикт после установки AppImage.
///
/// ⚠️ Починка 1: `remove_tmp` ставится ВСЕГДА. Оригинал убирал скачанный
/// временный файл только при отмене и провале валидации; провалилась сама
/// установка — ~200 МиБ оставались лежать, потому что `install_appimage`
/// зовёт `os.remove(new_path)` лишь на успешной ветке. На машине, где
/// `/tmp` смонтирован в ОЗУ, это утечка оперативной памяти до перезагрузки.
/// На успешной ветке файла уже нет, повторное удаление безвредно.
pub fn after_install_appimage(ok: bool, message: &str) -> WorkerOutcome {
    WorkerOutcome {
        ok,
        message: message.to_string(),
        remove_tmp: true,
        hand_over_tmp: false,
    }
}

/// Вердикт воркера плагина после валидации: отдать zip GUI-потоку под
/// IPC-установку либо ставить самим в файловую систему.
///
/// QWebSocket из чужого потока трогать нельзя, поэтому IPC делает GUI-поток;
/// воркер только доносит путь.
pub fn after_validate_plugin_route(fs_install: bool, message: &str) -> WorkerOutcome {
    WorkerOutcome {
        ok: true,
        message: message.to_string(),
        remove_tmp: false,
        hand_over_tmp: !fs_install,
    }
}

/// Вердикт после ФС-установки плагина: временный zip убираем всегда.
pub fn after_install_plugin_fs(ok: bool, message: &str) -> WorkerOutcome {
    WorkerOutcome {
        ok,
        message: message.to_string(),
        remove_tmp: true,
        hand_over_tmp: false,
    }
}

// ============================================================================
// Решения GUI-потока
// ============================================================================

/// Ставить ли обновление автоматически: есть что ставить, галка включена и
/// за сессию этого ещё не делали (дефолт галки — выключено).
pub fn should_auto_install(pending: bool, auto_enabled: bool, already_done: bool) -> bool {
    pending && auto_enabled && !already_done
}

/// Режим установки OWE-плагина: `true` — распаковка в файловую систему
/// (демон не подключён), `false` — hot-install через IPC демона.
pub fn plugin_fs_install(daemon_open: bool) -> bool {
    !daemon_open
}
