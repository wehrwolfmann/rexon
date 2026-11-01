//! Категории каталога оформления — порт `rexon/appearance/categories.py`.
//!
//! cat_id → тип → путь установки → apply-команда → requires_root. Данные
//! (реальные OCS-категории KDE Store + виртуальные/локальные) и порядок
//! сайдбара переносятся один-в-один; правильность транскрипции таблицы
//! проверяется побайтовой сверкой `asdict`-дампа с самим Python
//! (golden-тест `categories_parity.rs`).
//!
//! Пути `install_dir` в Python раскрываются `os.path.expanduser` на этапе
//! импорта (от `$HOME`). Здесь дом передаётся параметром — так функции
//! остаются чистыми и тестируются с фиксированным `$HOME` (как `settings.rs`
//! выносит предикат `command_exists` параметром).

use std::path::Path;

use crate::i18n::{tr, trc};
use crate::models::{CatalogSource, ThemeCategory};

/// Аналог `os.path.expanduser`: заменяет ведущую `~` на домашний каталог.
/// Путь без `~` (абсолютный `/usr/share/...` или пустой) возвращается как есть.
fn exp(home: &Path, p: &str) -> String {
    if p == "~" {
        return home.to_string_lossy().into_owned();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home.join(rest).to_string_lossy().into_owned();
    }
    p.to_string()
}

/// Хелпер конструктора реальной категории с install_dir/apply_kind.
fn cat(id: i64, key: &str, name: String, icon: &str, install_dir: String, apply_kind: &str) -> ThemeCategory {
    ThemeCategory {
        id,
        key: key.to_string(),
        name,
        icon: icon.to_string(),
        install_dir,
        apply_kind: apply_kind.to_string(),
        requires_root: false,
        virtual_: false,
        store: CatalogSource::Ocs(vec![id]),
        default_sort: "new".to_string(),
    }
}

/// Реальные категории KDE Store в порядке объявления Python-словаря
/// `CATEGORIES` (важен для сверки — порядок ключей виден в `asdict`-дампе).
///
/// `home` — домашний каталог для раскрытия `~` в `install_dir`.
///
/// `apply_kind` у КАЖДОЙ реальной категории свой и совпадает с её ключом.
/// Общей свалки `"none"` больше нет: пока обои, шрифты, звуки и виджеты
/// делили один вид, журнал установленных не мог отличить звуковую тему от
/// пачки обоев (обе писались типом `wallpaper-pack`), а карточка не умела
/// показать «применено» ни той, ни другой. Wehrwolfmann 2026-08-22.
pub fn categories(home: &Path) -> Vec<ThemeCategory> {
    let e = |p: &str| exp(home, p);
    vec![
        cat(722, "global", trc("appearance section", "Global Themes"), "💎", e("~/.local/share/plasma/look-and-feel"), "global"),
        cat(104, "plasma", trc("appearance section", "Plasma Styles"), "🖥️", e("~/.local/share/plasma/desktoptheme"), "plasma"),
        cat(112, "colors", trc("appearance section", "Color Schemes"), "🌈", e("~/.local/share/color-schemes"), "colors"),
        cat(114, "aurorae", trc("appearance section", "Window Decorations (Aurorae)"), "🪟", e("~/.local/share/aurorae/themes"), "aurorae"),
        cat(132, "icons", trc("appearance section", "Icons"), "📁", e("~/.local/share/icons"), "icons"),
        cat(107, "cursor", trc("appearance section", "Cursors"), "🖱️", e("~/.local/share/icons"), "cursor"),
        cat(123, "kvantum", "Kvantum".to_string(), "🔲", e("~/.config/Kvantum"), "kvantum"),
        cat(135, "gtk", trc("appearance section", "GTK Themes"), "🎨", e("~/.local/share/themes"), "gtk"),
        cat(299, "wallpapers", trc("appearance section", "HD Wallpapers"), "🖼️", e("~/.local/share/wallpapers"), "wallpapers"),
        cat(715, "wallpaper_plugins", trc("appearance section", "Wallpaper Plugins"), "🎞️", e("~/.local/share/plasma/wallpapers"), "wallpaper_plugins"),
        cat(103, "fonts", trc("appearance section", "Fonts"), "🔤", e("~/.local/share/fonts"), "fonts"),
        cat(316, "sounds", trc("appearance section", "Sounds"), "🔔", e("~/.local/share/sounds"), "sounds"),
        // «Виджеты» — зонт 705 «Plasma 6 Extensions», как у Discover
        // (`plasmoids.knsrc`): апплеты 706 и подкатегории 707–712 (календари,
        // часы, меню, мониторинг, мультимедиа, погода). Товары приходят с
        // typeid подкатегории — к разделу их ведёт [`STORE_TYPE_ALIASES`].
        cat(705, "plasmoids", trc("appearance section", "Widgets"), "🧊", e("~/.local/share/plasma/plasmoids"), "plasmoids"),
        ThemeCategory {
            requires_root: true,
            ..cat(101, "sddm", trc("appearance section", "SDDM (Login Screen)"), "🔐", "/usr/share/sddm/themes".to_string(), "sddm")
        },
        ThemeCategory {
            requires_root: true,
            ..cat(108, "plymouth", "Plymouth".to_string(), "⚡", "/usr/share/plymouth/themes".to_string(), "plymouth")
        },
        // «Стиль приложений»: стили Qt и темы Kvantum одной сеткой, магазин —
        // темы Kvantum (категория 123, ставятся по её же записи выше в
        // `~/.config/Kvantum`). Готовые стили Qt (Darkly, Lightly, Klassy)
        // онлайн-каталога не имеют — они в AUR (`aur::category_queries`).
        ThemeCategory {
            store: CatalogSource::Ocs(vec![123]),
            ..cat(901, "widgetstyle", trc("appearance section", "Application Style"), "🎛️", String::new(), "widgetstyle")
        },
        ThemeCategory {
            store: CatalogSource::Ocs(vec![716, 488]),
            ..cat(902, "splash", trc("appearance section", "KDE Splash Screen"), "🌅", e("~/.local/share/plasma/look-and-feel"), "splash")
        },
        ThemeCategory {
            // Номер по режиму (полные темы 722 / обои 299) выбирает
            // `CatalogView::query_category_id`.
            store: CatalogSource::Ocs(vec![722]),
            ..cat(903, "lockscreen", trc("appearance section", "Lock Screen"), "🔒", e("~/.local/share/plasma/look-and-feel"), "lockscreen")
        },
        cat(904, "lockwallpaper", trc("appearance section", "Lock Screen Wallpaper"), "🔒", e("~/.local/share/wallpapers"), "lockwallpaper"),
        cat(716, "splash", trc("appearance section", "Plasma 6 Splashscreens"), "🌅", e("~/.local/share/plasma/look-and-feel"), "splash"),
        cat(488, "splash", trc("appearance section", "Plasma Splashscreens"), "🌅", e("~/.local/share/plasma/look-and-feel"), "splash"),
    ]
    .into_iter()
    // ── Разделы окружений (04.10.2026) ─────────────────────────────────
    // То, что раньше включала сама установка `.rxn`, получило свой раздел и
    // свою кнопку: «Применить» у тем, «Включить»/«Выключить» у дополнений,
    // «Запустить»/«Остановить» у Conky (`crate::de_apply`). Каталог
    // установки берётся из матрицы, в каком окружении раздел виден — тоже
    // из неё (`category_available`).
    .chain(DE_SECTIONS.iter().map(|(id, key)| de_section(home, *id, key)))
    .collect()
}

/// Разделы окружений: номер и ключ (он же вид применения). Номер больше
/// [`STORE_MAX_ID`] — синтетический (после местных 901–905), не больше —
/// настоящая категория магазина KDE. Откуда раздел берёт каталог, номер не
/// решает: это [`de_section_store`]. Эталон Python этих разделов не знал.
/// Тип раздела — [`category_kind`], подпись — [`de_section_name`]; новый
/// раздел окружения добавляется сюда, туда и строкой матрицы, больше никуда
/// (скан диска и кнопка подхватят его сами).
///
/// Переключатель окон, скрипты и эффекты KWin — категории магазина
/// «Kwin Switching Layouts / Kwin Scripts / KWin Effects Plasma 6» (те же,
/// что в `kwinswitcher.knsrc`, `kwinscripts.knsrc`, `kwineffect.knsrc`).
/// До 04.10.2026 они были местными 911–913 — старые номера ведёт
/// [`STORE_TYPE_ALIASES`].
pub const DE_SECTIONS: &[(i64, &str)] = &[
    (906, "xfwm4"),
    (907, "marco"),
    (908, "gnomeshell"),
    (909, "cinnamontheme"),
    (910, "lxqttheme"),
    // Рамки Openbox и labwc — менеджеров окон LXQt (и сеанса labwc).
    (OPENBOX_SECTION_ID, OPENBOX_KEY),
    (721, "tabbox"),
    (720, "kwinscripts"),
    (719, "kwineffects"),
    (914, "gnomeext"),
    (915, "cinnamonapplets"),
    (916, "cinnamondesklets"),
    (917, "cinnamonext"),
    (918, "nemoactions"),
    // Conky — настоящая категория магазина KDE Store («Conky», 124, больше
    // тысячи наборов): раздел показывает её каталог, и виджет ставится
    // кнопкой, а не командами в терминале (просьба Wehrwolfmann 10.10.2026).
    // До этого раздел был местным 919 — старый номер ведёт
    // [`STORE_TYPE_ALIASES`].
    (CONKY_SECTION_ID, "conky"),
];

/// Разделы KDE «Внешнего вида», которые сканируются как разделы окружений:
/// номер категории и ключ (он же вид применения и ключ карты активных тем,
/// `installed_scan::active_map_from_sources`).
///
/// Есть ли у раздела сетка установленного с кнопкой «Загрузить новые…»,
/// этот список больше не решает: она есть у любого раздела со сканом и
/// магазином (`appearance_page::has_installed_grid_for`). Скан раздела — тот же
/// [`DE_SECTIONS`]-ный (`appearance_page::local_scan_plan` → `"section"`,
/// `installed_scan::LiveScan::section`), каталоги — из таблицы типов
/// `installed_scan::types_for`.
pub const KDE_INSTALLED_SECTIONS: &[(i64, &str)] = &[
    (722, "global"),
    (104, "plasma"),
    (112, "colors"),
    (114, "aurorae"),
    (132, "icons"),
    (107, "cursor"),
    (135, "gtk"),
    (316, "sounds"),
    // Экран входа (SDDM) и заставка загрузки (Plymouth) — как kcm sddm и
    // kcm plymouth: сетка установленных тем с выделенной текущей, а не
    // магазин с установленным только в обзоре «Системные». Темы лежат в
    // системных каталогах; «Применить» и «Удалить» идут через помощника
    // от root (`ApplyAction::SystemTheme`, `theme_removal`). При Plasma Login
    // Manager раздела SDDM в панели нет ([`sidebar_for`]), на SteamOS оба
    // раздела объясняют отказ ([`steamos_note`]).
    (101, "sddm"),
    (108, "plymouth"),
];

/// Наибольший номер настоящей категории магазина KDE. Всё, что выше, —
/// синтетические номера разделов: в магазин они не уходят никогда, каталог
/// раздела задаёт его поле `store`.
pub const STORE_MAX_ID: i64 = 900;

/// Номер товара магазина (его `typeid`) → номер раздела, где он живёт,
/// когда они не совпадают.
///
/// Раздел-зонт запрашивает магазин родительской категорией, а товары
/// приходят с номером своей подкатегории: «Виджеты» спрашивают 705, а
/// виджет часов приходит с 708. Без этой таблицы такой товар не находил
/// раздела — ни вида установки (плазмоид), ни кнопки, ни подписи.
/// Здесь же прежние номера разделов, чтобы записи журнала и избранного,
/// сделанные до смены номера, не потеряли свой раздел.
pub const STORE_TYPE_ALIASES: &[(i64, i64)] = &[
    // Подкатегории «Plasma 6 Extensions»: апплеты, календари, часы, меню,
    // мониторинг, мультимедиа, погода. 706 — и прежний номер раздела.
    (706, 705),
    (707, 705),
    (708, 705),
    (709, 705),
    (710, 705),
    (711, 705),
    (712, 705),
    // Местные номера разделов KWin до 04.10.2026.
    (911, 721),
    (912, 720),
    (913, 719),
    // Местный номер раздела Conky до 10.10.2026.
    (919, 124),
    // «Cinnamon Themes» магазина (cinnamon-look): до 24.10.2026 раздел тем
    // Cinnamon показывал этот каталог, теперь — Cinnamon Spices. Записи
    // журнала тех установок остаются в своём разделе.
    (133, 909),
];

/// Номер раздела для номера товара: подкатегория зонта → зонт, прежний
/// номер раздела → нынешний, остальное — как есть.
pub fn section_id_for(type_id: i64) -> i64 {
    STORE_TYPE_ALIASES
        .iter()
        .find(|(from, _)| *from == type_id)
        .map(|(_, to)| *to)
        .unwrap_or(type_id)
}

/// Раздел окружения ли это (по номеру).
pub fn is_de_section(id: i64) -> bool {
    DE_SECTIONS.iter().any(|(i, _)| *i == id)
}

/// Подпись раздела окружения на языке интерфейса.
fn de_section_name(key: &str) -> String {
    match key {
        "xfwm4" => trc("appearance section", "Window Borders (XFCE)"),
        "marco" => trc("appearance section", "Window Borders (MATE)"),
        "gnomeshell" => trc("appearance section", "GNOME Shell Theme"),
        "cinnamontheme" => trc("appearance section", "Cinnamon Theme"),
        "lxqttheme" => trc("appearance section", "LXQt Theme"),
        OPENBOX_KEY => trc("appearance section", "Window Borders (Openbox)"),
        "tabbox" => trc("appearance section", "Window Switcher (Alt+Tab)"),
        "kwinscripts" => trc("appearance section", "KWin Scripts"),
        "kwineffects" => trc("appearance section", "KWin Effects"),
        "gnomeext" => trc("appearance section", "GNOME Extensions"),
        "cinnamonapplets" => trc("appearance section", "Cinnamon Applets"),
        "cinnamondesklets" => trc("appearance section", "Cinnamon Desklets"),
        "cinnamonext" => trc("appearance section", "Cinnamon Extensions"),
        "nemoactions" => trc("appearance section", "Nemo Actions"),
        "conky" => trc("appearance section", "Conky Widgets"),
        other => other.to_string(),
    }
}

/// Местный раздел окружения: ключ раздела = вид применения, тип — из
/// [`category_kind`], каталог и значок — из матрицы и таблицы типов.
fn de_section(home: &Path, id: i64, key: &str) -> ThemeCategory {
    let name = de_section_name(key);
    let kind = category_kind(key).expect("раздел окружения связан с типом в category_kind");
    let dir = crate::de_customization::row(kind)
        .map(|r| r.dest_path(&home.to_string_lossy()))
        .unwrap_or_default();
    ThemeCategory {
        store: de_section_store(key),
        ..cat(id, key, name, crate::constants::type_icon(kind).unwrap_or("🧩"), dir, key)
    }
}

/// Каталог раздела окружения — явно, по ключу, а не по номеру: номер
/// раздела держит журнал установленного и избранное и меняться не может, а
/// каталог у раздела бывает в магазине под другим номером. Нет записи —
/// каталога нет, раздел показывает только диск.
pub fn de_section_store(key: &str) -> CatalogSource {
    // Пять разделов Cinnamon — магазин Cinnamon Spices (тот же, что у окна
    // настроек Cinnamon): вид каталога — из таблицы типов спайсов.
    if let Some(t) = crate::cinnamon_spices::type_for_section(key) {
        return CatalogSource::Spices(t.json.to_string());
    }
    match key {
        // Те же категории, что у Discover (`kwinswitcher.knsrc`,
        // `kwinscripts.knsrc`, `kwineffect.knsrc`).
        "tabbox" => CatalogSource::Ocs(vec![721]),
        "kwinscripts" => CatalogSource::Ocs(vec![720]),
        "kwineffects" => CatalogSource::Ocs(vec![719]),
        "conky" => CatalogSource::Ocs(vec![CONKY_SECTION_ID]),
        // Разделы окружений под синтетическими номерами: категории pling
        // (xfce-look, gnome-look, cinnamon-look; тот же API OCS).
        // «XFCE/XFWM4 Themes».
        "xfwm4" => CatalogSource::Ocs(vec![138]),
        // «Metacity Themes»: Marco читает тот же формат `metacity-1`.
        "marco" => CatalogSource::Ocs(vec![125]),
        // «Gnome Shell Themes».
        "gnomeshell" => CatalogSource::Ocs(vec![134]),
        // «LXQt Themes».
        "lxqttheme" => CatalogSource::Ocs(vec![446]),
        // «Openbox Themes» (box-look): тот же формат читает и labwc.
        OPENBOX_KEY => CatalogSource::Ocs(vec![140]),
        _ => CatalogSource::None,
    }
}

/// Виртуальная категория сайдбара (без cat_id, без сети).
fn virtual_cat(key: &str, name: String, icon: &str, default_sort: &str) -> ThemeCategory {
    ThemeCategory {
        id: 0,
        key: key.to_string(),
        name,
        icon: icon.to_string(),
        install_dir: String::new(),
        apply_kind: String::new(),
        requires_root: false,
        virtual_: true,
        // Обзоры диска сети не знают; «Популярные» и «Новые» — сборная
        // витрина ходовых категорий окружения.
        store: if LOCAL_VIEW_KEYS.contains(&key) { CatalogSource::None } else { CatalogSource::OcsShowcase },
        default_sort: default_sort.to_string(),
    }
}

/// Виртуальные категории сайдбара в порядке Python-констант.
/// popular/new агрегируют реальные категории с разной сортировкой;
/// active/downloaded/system/favorites — локальный обзор диска без сети.
pub fn popular() -> ThemeCategory {
    virtual_cat("popular", trc("appearance section", "Popular"), "⭐", "high")
}
pub fn new_() -> ThemeCategory {
    virtual_cat("new", trc("appearance section", "New"), "🆕", "new")
}
pub fn active() -> ThemeCategory {
    virtual_cat("active", trc("appearance section", "Active"), "✅", "new")
}
pub fn downloaded() -> ThemeCategory {
    virtual_cat("downloaded", trc("appearance section", "Downloaded"), "📥", "new")
}
pub fn system() -> ThemeCategory {
    virtual_cat("system", trc("appearance section", "System"), "🖥️", "new")
}
pub fn favorites() -> ThemeCategory {
    virtual_cat("favorites", trc("appearance section", "Favorites"), "⭐", "new")
}

/// Порядок сайдбара: локальные обзоры → магазин.
///
/// `home` раскрывает `~` в реальных категориях. Виртуальные категории
/// перечислены явно (не из сети). Реальные берутся из `categories(home)` по
/// cat_id — как `CATEGORIES[…]` в Python.
pub fn sidebar(home: &Path) -> Vec<ThemeCategory> {
    let all = categories(home);
    let pick = |id: i64| -> ThemeCategory {
        all.iter()
            .find(|c| c.id == id)
            .cloned()
            .expect("cat_id есть в categories()")
    };
    vec![
        active(),
        downloaded(),
        system(),
        favorites(),
        popular(),
        new_(),
        pick(722),
        pick(104),
        pick(112),
        pick(114),
        // Разделы настроек (01.11.2026) стоят там, где их держит KDE 6.8:
        // кнопки заголовка и размер рамки — в модуле «Оформление окон»,
        // отклик при запуске — в «Курсорах», значки и панели инструментов —
        // в «Стиле приложений», шрифты — в «Тексте и шрифтах»
        // ([`SETTINGS_PANELS`]).
        settings_panel(WINDOW_BUTTONS_KEY),
        pick(132),
        pick(107),
        settings_panel("launchfeedback"),
        pick(135),
        pick(901),
        settings_panel("appwidgets"),
        pick(299),
        pick(715),
        settings_panel("fontsettings"),
        pick(103),
        pick(316),
        pick(705),
        // Виджеты Conky — сразу под «Виджетами»: это тоже виджеты рабочего
        // стола, и на другом конце списка их искали бы зря (10.10.2026).
        pick(CONKY_SECTION_ID),
        pick(101),
        pick(902),
        pick(903),
        pick(108),
    ]
    .into_iter()
    // Разделы окружений — после общих; в каждом окружении видны только
    // свои (`category_available`).
    .chain(DE_SECTIONS.iter().filter(|(id, _)| *id != CONKY_SECTION_ID).map(|(id, _)| pick(*id)))
    // «Панель MATE» — не каталог тем, а готовые раскладки панели; стоит
    // среди разделов рабочего стола MATE.
    .chain(std::iter::once(mate_panel_category()))
    // «Анимация» — последним разделом «Внешнего вида», как в KDE 6.8
    // (`kcm_animations`, вес 40 в категории `appearance`); «Эффекты
    // рабочего стола» — рядом с ней: в KDE они в «Управлении окнами», а
    // такой вкладки у Rexon нет.
    .chain(["animations", "desktopeffects"].into_iter().map(settings_panel))
    .collect()
}

// ── Разделы настроек в «Оформлении» (01.11.2026) ───────────────────────

/// Разделы, которые до 01.11.2026 жили во вкладке «Настройки», а в KDE 6.8
/// стоят во «Внешнем виде и стиле» (замечание пересчётов паритета 30–31.10):
/// номер (синтетический, магазина у них нет) и ключ. Содержимое — те же
/// разделы вкладки «Настройки» (`SettingsPage.qml`, режим `panel`), здесь
/// только место в боковой панели. Где какой стоит — [`sidebar`],
/// в какой группе — [`category_group`], где виден — [`category_available`].
pub const SETTINGS_PANELS: &[(i64, &str)] = &[
    (921, WINDOW_BUTTONS_KEY),
    (922, "launchfeedback"),
    (923, "appwidgets"),
    (924, "fontsettings"),
    (925, "animations"),
    (926, "desktopeffects"),
];

/// Ключ раздела «Кнопки заголовка окна» (с размером рамки и флажками рамок).
pub const WINDOW_BUTTONS_KEY: &str = "windowbuttons";

/// Разделы рамок окон, за которыми встаёт [`WINDOW_BUTTONS_KEY`]: в KDE
/// кнопки заголовка — вкладка модуля «Оформление окон», и в каждом
/// окружении они идут сразу за его разделом рамок (Aurorae, XFWM4, Marco).
/// У Cinnamon своего раздела рамок нет: с 5.4 рамки Muffin рисует тема GTK
/// (`de_customization::UNAVAILABLE_REASONS`), и кнопки встают за темой
/// Cinnamon — последним разделом его рабочего стола.
pub const WINDOW_BORDER_SECTIONS: &[&str] = &["aurorae", "xfwm4", "marco", "cinnamontheme", OPENBOX_KEY];

/// Раздел настроек ли это (по ключу).
pub fn is_settings_panel(key: &str) -> bool {
    SETTINGS_PANELS.iter().any(|(_, k)| *k == key)
}

/// Подпись раздела настроек — тот же заголовок, что у самого раздела.
fn settings_panel_name(key: &str) -> String {
    match key {
        WINDOW_BUTTONS_KEY => tr("Window title bar buttons"),
        "launchfeedback" => tr("Launch feedback"),
        "appwidgets" => tr("Icons and toolbars in applications"),
        // Заголовок раздела шрифтов вкладки «Настройки» без значка в начале:
        // значок у строки боковой панели свой.
        "fontsettings" => crate::qt_settings::section_texts("fontsGroup", "")
            .map(|(title, _)| title.trim_start_matches(|c: char| !c.is_alphanumeric()).trim().to_string())
            .unwrap_or_default(),
        "animations" => tr("Animation"),
        "desktopeffects" => tr("Desktop effects"),
        other => other.to_string(),
    }
}

/// Раздел настроек боковой панели: магазина и скана нет, свой экран
/// (`appearance_page::SCREEN_SETTINGS`). В [`categories`] его нет
/// намеренно — ставить в него нечего.
fn settings_panel(key: &str) -> ThemeCategory {
    let (id, key) = *SETTINGS_PANELS
        .iter()
        .find(|(_, k)| *k == key)
        .expect("раздел настроек есть в SETTINGS_PANELS");
    let icon = match key {
        WINDOW_BUTTONS_KEY => "🔘",
        "launchfeedback" => "⏳",
        "appwidgets" => "🛠️",
        "fontsettings" => "🔠",
        "animations" => "💫",
        _ => "🪄",
    };
    ThemeCategory {
        store: CatalogSource::None,
        ..cat(id, key, settings_panel_name(key), icon, String::new(), "")
    }
}

/// Ссылки «Оформление → …» во вкладке «Настройки» на месте переехавших
/// разделов: ключ раздела и подпись, в порядке боковой панели
/// «Оформления», только разделы, видимые в окружении `de_id` (запущенном).
/// ⏳ Переходные — на одну версию, чтобы человек, привыкший искать
/// анимацию и шрифты в «Настройках», не потерялся; затем снять вместе с
/// [`moved_settings_heading`].
pub fn moved_settings_links(de_id: &str) -> Vec<(String, String)> {
    grouped_sidebar(&sidebar(Path::new("/")), de_id)
        .into_iter()
        .filter(|c| is_settings_panel(&c.key) && category_available(&c.key, de_id))
        // TRANSLATORS: link in the Settings tab to a section that moved; %1 is the section name.
        .map(|c| (c.key.clone(), crate::i18n::trf("Appearance → %1", &[&c.name])))
        .collect()
}

/// Пояснение над ссылками [`moved_settings_links`].
pub fn moved_settings_heading() -> String {
    tr("These settings have moved to the Appearance tab — where KDE System Settings keeps them too.")
}

/// Где раздел настроек виден — по тем же правилам, по которым его раздел
/// прячется сам: «Эффекты рабочего стола» — только KDE
/// (`animations::desktop_effects_state`), «Значки и панели инструментов» —
/// везде, кроме Deepin (`app_widgets::available`). Остальные разделы
/// говорят о своём окружении сами и видны везде. Неизвестное окружение —
/// видно всё.
fn settings_panel_available(key: &str, de_id: &str) -> bool {
    if crate::de_customization::de_from_id(de_id).is_none() {
        return true;
    }
    match key {
        "desktopeffects" => de_id == "kde",
        "appwidgets" => crate::app_widgets::available(de_id),
        _ => true,
    }
}

/// Ключ раздела рамок Openbox и labwc.
pub const OPENBOX_KEY: &str = "openbox";
/// Его номер (синтетический, после разделов настроек 921–926).
pub const OPENBOX_SECTION_ID: i64 = 927;

/// Где раздел рамок Openbox виден: в LXQt (Openbox — его менеджер окон на
/// X11, labwc — на Wayland) и в сеансе labwc. Матрица сводит тайловые в
/// одно окружение, а рамок нет ни у sway, ни у Hyprland, ни у niri.
/// Неизвестное окружение — видно, как всё остальное.
fn openbox_available(de_id: &str) -> bool {
    matches!(de_id, "lxqt" | "labwc") || crate::de_customization::de_from_id(de_id).is_none()
}

/// Ключ раздела «Панель MATE».
pub const MATE_PANEL_KEY: &str = "mate-panel";

/// Раздел «Панель MATE»: готовые раскладки mate-panel одной кнопкой
/// (`mate_panel`). Тем у него нет — ни магазина, ни скана диска, свой экран
/// (`appearance_page::SCREEN_MATE_PANEL`). В [`categories`] его нет
/// намеренно: ставить в него нечего. Номер синтетический, на нём ничего не
/// держится.
pub fn mate_panel_category() -> ThemeCategory {
    ThemeCategory {
        store: CatalogSource::None,
        ..cat(920, MATE_PANEL_KEY, trc("appearance section", "MATE Panel"), "🧰", String::new(), "")
    }
}

/// Номер раздела «Виджеты Conky» ([`DE_SECTIONS`]): в боковой панели он стоит
/// рядом с «Виджетами», а не в хвосте с разделами окружений.
pub const CONKY_SECTION_ID: i64 = 124;

/// Ключ раздела «Экран входа (Plasma Login Manager)».
pub const PLASMA_LOGIN_KEY: &str = "plasmalogin";

/// Раздел экрана входа, когда его рисует Plasma Login Manager.
///
/// Встаёт в боковую панель НА МЕСТО «SDDM (экран входа)» (см.
/// [`sidebar_for`]). Товаров у него нет: у Plasma Login Manager нет тем, вид
/// собран в сам бинарь, а категории в магазине KDE для него не существует.
/// Поэтому раздел местный (без каталога, без сети), а номер — синтетический,
/// больше девятисот, как у прочих разделов вне магазина: прогрев такие
/// пропускает. В [`categories`] его НЕТ намеренно — ставить в него нечего, и
/// `category_by_kind`/`category_by_id` находить его не должны.
pub fn plasma_login_category() -> ThemeCategory {
    ThemeCategory {
        store: CatalogSource::None,
        ..cat(
            905,
            PLASMA_LOGIN_KEY,
            trc("appearance section", "Login Screen (Plasma Login Manager)"),
            "🔐",
            String::new(),
            "plasmalogin",
        )
    }
}

/// Боковая панель с учётом того, что рисует экран входа.
///
/// `plasma_login` — активен Plasma Login Manager
/// (`system_plan::is_plasma_login`). Тогда на месте раздела SDDM-тем стоит
/// [`plasma_login_category`]: полка SDDM-тем там — обман, ни одна из них на
/// экран входа не попадёт. Иначе — ровно [`sidebar`].
pub fn sidebar_for(home: &Path, plasma_login: bool) -> Vec<ThemeCategory> {
    let mut out = sidebar(home);
    if plasma_login {
        for c in out.iter_mut() {
            if c.key == "sddm" {
                *c = plasma_login_category();
            }
        }
    }
    out
}

/// Что запрещено этому разделу на SteamOS (Steam Deck и Steam Machine).
///
/// Экран входа и заставку загрузки Rexon там не меняет вовсе
/// ([`crate::distro_compat::SteamOsBlock`]); раздел остаётся на месте и
/// СРАЗУ говорит почему, а не отказывает после скачивания темы по кнопке
/// «Установить» (правило #13). `None` — раздел на SteamOS работает.
pub fn steamos_block(cat_key: &str) -> Option<crate::distro_compat::SteamOsBlock> {
    use crate::distro_compat::SteamOsBlock;
    match cat_key {
        "sddm" | PLASMA_LOGIN_KEY => Some(SteamOsBlock::LoginScreen),
        "plymouth" => Some(SteamOsBlock::BootSplash),
        _ => None,
    }
}

/// Объяснение вместо полки раздела, закрытого на SteamOS. `None` — раздел
/// открыт (или это не SteamOS).
pub fn steamos_note(home: &Path, cat_key: &str, steamos: bool) -> Option<String> {
    if !steamos {
        return None;
    }
    let why = steamos_block(cat_key)?.reason();
    Some(note_text(home, cat_key, &why))
}

/// Ключи локальных категорий-обзоров диска (без сети, без чипов «Магазин:»).
pub const LOCAL_VIEW_KEYS: [&str; 4] = ["active", "downloaded", "system", "favorites"];

/// Для popular/new — какие реальные cat_id запрашивать (самые ходовые).
/// В окружении остаются только действующие там ([`aggregate_cats`]).
pub const AGGREGATE_CATS: [i64; 5] = [722, 104, 112, 132, 107];

/// Добор витрин «Популярные»/«Новые» там, где часть [`AGGREGATE_CATS`] не
/// действует (вне KDE глобальные темы и стили Plasma — мимо): GTK-темы, обои
/// и виджеты Conky — то, что ставят чаще всего после значков и курсоров.
pub const AGGREGATE_EXTRA_CATS: [i64; 3] = [135, 299, CONKY_SECTION_ID];

/// Категории магазина для «Популярных» и «Новых» в этом окружении.
///
/// До 16.10.2026 витрины спрашивали только первую из [`AGGREGATE_CATS`] —
/// глобальные темы Plasma, и в MATE обе верхние витрины целиком состояли из
/// того, что там не применяется. Теперь берутся ходовые категории, которые
/// в окружении действуют (`category_available`, та же матрица, что прячет
/// разделы); если какие-то отпали — добираются [`AGGREGATE_EXTRA_CATS`].
/// В KDE отпавших нет, и набор — ровно [`AGGREGATE_CATS`].
pub fn aggregate_cats(de_id: &str) -> Vec<i64> {
    let all = categories(Path::new(""));
    let works = |id: &i64| {
        all.iter()
            .find(|c| c.id == *id)
            .is_some_and(|c| category_available(&c.key, de_id))
    };
    let mut out: Vec<i64> = AGGREGATE_CATS.iter().copied().filter(works).collect();
    if out.len() < AGGREGATE_CATS.len() {
        out.extend(AGGREGATE_EXTRA_CATS.iter().copied().filter(works));
    }
    out
}

/// [`aggregate_cats`] строкой для запроса: магазин понимает список через
/// запятую, как у зонта заставки «716,488».
pub fn aggregate_query(de_id: &str) -> String {
    aggregate_cats(de_id).iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
}

/// Найти категорию по внутреннему ключу — первую в порядке сайдбара
/// (как `get_category` перебирает `SIDEBAR`). Виртуальные включены.
pub fn get_category(home: &Path, key: &str) -> Option<ThemeCategory> {
    sidebar(home)
        .into_iter()
        .find(|c| c.key == key)
        .or_else(|| (key == PLASMA_LOGIN_KEY).then(plasma_login_category))
}

/// Найти реальную категорию по OCS cat_id (как `CATEGORIES.get`).
///
/// Номер товара подкатегории (или прежний номер раздела) ведёт в свой
/// раздел через [`STORE_TYPE_ALIASES`]: виджет часов (708) — это раздел
/// «Виджеты» со всем, что из этого следует (каталог установки, вид, кнопка).
///
/// Товар из каталога раздела окружения приходит с номером категории
/// магазина, а не раздела: рамка из «XFCE/XFWM4 Themes» — это 138, раздел
/// же — 906. Такой номер ведёт в раздел, чей каталог (`store`) его
/// содержит, — без отдельной таблицы псевдонимов на каждый раздел.
pub fn category_by_id(home: &Path, cat_id: i64) -> Option<ThemeCategory> {
    let all = categories(home);
    let pos = all
        .iter()
        .position(|c| c.id == cat_id)
        .or_else(|| all.iter().position(|c| c.id == section_id_for(cat_id)))
        .or_else(|| all.iter().position(|c| c.ocs_cats().contains(&cat_id)))?;
    all.into_iter().nth(pos)
}

/// Найти категорию по apply_kind — первую в порядке объявления `CATEGORIES`
/// (как `category_by_kind` перебирает `CATEGORIES.values()`). Устойчиво к
/// смене OCS cat_id: install-пути привязаны к apply_kind.
pub fn category_by_kind(home: &Path, apply_kind: &str) -> Option<ThemeCategory> {
    categories(home).into_iter().find(|c| c.apply_kind == apply_kind)
}

// ============================================================================
// Связь с матрицей: что показывать в каком окружении
// ============================================================================

/// Тип `.rxn`, которым оформляется категория каталога.
///
/// Это ЕДИНСТВЕННОЕ место, где раздел боковой панели связывается с типом
/// оформления; дальше всё решает матрица `de_customization` — какие разделы
/// показать покупателю и почему остальные скрыты. Списков типов в коде
/// интерфейса быть не должно (правило #12).
///
/// `None` — раздел не про конкретный тип: обзоры уже установленного
/// (активные, загруженные, системные, избранное) и витрины (популярные,
/// новые) показываются всегда.
pub fn category_kind(key: &str) -> Option<&'static str> {
    Some(match key {
        "global" => "theme-kde-global",
        "plasma" => "theme-kde-plasma",
        "colors" => "theme-kde-colors",
        "aurorae" => "theme-window-decoration",
        "icons" => "theme-icons",
        "cursor" => "theme-cursors",
        "gtk" => "theme-gtk",
        // «Стиль приложений» — выбор Qt-стиля и темы Kvantum: штатно это
        // умеют KDE и LXQt.
        "widgetstyle" | "kvantum" => "theme-kvantum",
        "wallpapers" => "wallpaper-pack",
        "wallpaper_plugins" => "kde-wallpaper-plugin",
        "fonts" => "font-pack",
        "sounds" => "theme-sounds",
        "plasmoids" => "theme-plasmoid",
        "sddm" | PLASMA_LOGIN_KEY => "theme-login-screen",
        "splash" => "theme-kde-splash",
        "lockscreen" | "lockwallpaper" => "theme-kde-lockscreen",
        "plymouth" => "theme-plymouth",
        "xfwm4" => "theme-xfwm4",
        "marco" => "theme-marco",
        "gnomeshell" => "theme-gnome-shell",
        "cinnamontheme" => "theme-cinnamon",
        "lxqttheme" => "theme-lxqt",
        OPENBOX_KEY => "theme-openbox",
        "tabbox" => "kde-window-switcher",
        "kwinscripts" => "kde-kwin-script",
        "kwineffects" => "kde-kwin-effect",
        "gnomeext" => "gnome-shell-extension",
        "cinnamonapplets" => "cinnamon-applet",
        "cinnamondesklets" => "cinnamon-desklet",
        "cinnamonext" => "cinnamon-extension",
        "nemoactions" => "nemo-action",
        "conky" => "theme-conky",
        _ => return None,
    })
}

/// Показывать ли раздел каталога в этом окружении.
///
/// Неизвестный идентификатор окружения — показываем всё: «не знаю, где я»
/// не повод прятать от человека половину программы.
pub fn category_available(cat_key: &str, de_id: &str) -> bool {
    // Раскладки mate-panel есть только у MATE; есть ли сама панель на этой
    // машине, решает окно (`mate_panel::is_available`).
    if cat_key == MATE_PANEL_KEY {
        return crate::de_customization::de_from_id(de_id) == Some(crate::de_customization::De::Mate);
    }
    if is_settings_panel(cat_key) {
        return settings_panel_available(cat_key, de_id);
    }
    if cat_key == OPENBOX_KEY {
        return openbox_available(de_id);
    }
    let Some(kind) = category_kind(cat_key) else {
        return true;
    };
    let Some(de) = crate::de_customization::de_from_id(de_id) else {
        return true;
    };
    // «Экран блокировки» держит два режима: полные темы (тип матрицы —
    // только KDE) и обои, у которых свой путь применения. Раздел есть там,
    // где работает хотя бы обоевый.
    if matches!(cat_key, "lockscreen" | "lockwallpaper") && LOCK_WALLPAPER_DES.contains(&de) {
        return true;
    }
    crate::de_customization::row(kind).is_none_or(|r| r.covers(de))
}

/// Где Rexon ставит обои экрана блокировки
/// (`appearance_installer::lock_wallpaper_plan`): KDE — `kscreenlockerrc`,
/// Deepin — Центр управления в сеансе Treeland
/// (`dde::control_center_wallpaper_argv`). Новое окружение
/// с этим режимом — сюда и в план, больше никуда.
pub const LOCK_WALLPAPER_DES: &[crate::de_customization::De] =
    &[crate::de_customization::De::Kde, crate::de_customization::De::Deepin];

/// Есть ли в окружении режим «Полные темы» экрана блокировки (тип матрицы
/// `theme-kde-lockscreen`). Неизвестное окружение — есть: прятать не повод.
pub fn lock_themes_supported(de_id: &str) -> bool {
    match crate::de_customization::de_from_id(de_id) {
        None => true,
        Some(de) => crate::de_customization::row("theme-kde-lockscreen").is_none_or(|r| r.covers(de)),
    }
}

/// При каком условии раздел работает в этом окружении — подпись под
/// заголовком (правило #13: назвать условие заранее, а не оставить человека
/// гадать, почему «ничего не произошло»). `None` — условий нет.
///
/// Пара к [`category_note`]: та объясняет, почему раздела НЕТ, эта — что
/// нужно, чтобы имеющийся раздел сработал.
///
/// Текст берётся из каталога перевода на языке интерфейса.
pub fn category_condition(cat_key: &str, de_id: &str) -> Option<String> {
    crate::de_customization::condition_note_id(category_kind(cat_key)?, de_id)
}

/// Почему раздела нет в этом окружении — текст для человека (правило #13:
/// объяснять, а не показывать пустой экран). `None` — раздел доступен.
pub fn category_note(home: &Path, cat_key: &str, de_id: &str) -> Option<String> {
    if category_available(cat_key, de_id) {
        return None;
    }
    let kind = category_kind(cat_key)?;
    let de = crate::de_customization::de_from_id(de_id)?;
    // Имя рабочего стола — то же, что во вкладке «Рабочие столы».
    let here = crate::system::DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name)
        .unwrap_or("");
    let why = if cat_key == OPENBOX_KEY {
        let here = if here.is_empty() { de.display_label() } else { here.to_string() };
        crate::i18n::trf(
            "Openbox themes are for the Openbox and labwc window managers (as in LXQt). Your selected desktop is %1, \
             which draws window borders its own way, so we don't offer them.",
            &[&here],
        )
    } else {
        crate::de_customization::unavailable_note(kind, de, here)?
    };
    Some(note_text(home, cat_key, &why))
}

/// Экран-объяснение закрытого раздела: имя раздела заголовком, причина и
/// совет выбрать другой раздел. Один на все причины — и «нет в этом рабочем
/// столе», и «нельзя на SteamOS».
fn note_text(home: &Path, cat_key: &str, why: &str) -> String {
    let name = get_category(home, cat_key).map(|c| c.name).unwrap_or_default();
    let head = if name.is_empty() {
        String::new()
    } else {
        // TRANSLATORS: a section name in quotation marks, as a heading.
        format!("{}\n", crate::i18n::trf("“%1”", &[&name]))
    };
    let tail = tr(
        "Pick another section on the left — everything your desktop can \
         actually use is still there.",
    );
    format!("{head}{why}\n{tail}")
}

/// Рабочие столы для выбора «Текущее окружение» во вкладке «Оформление»:
/// `(id, подпись)` в порядке [`crate::system::DE_META`].
///
/// Тайловые сведены в ОДИН пункт. Это не упрощение на глаз, а то, что уже
/// записано в матрице кастомизации: `de_from_id` отдаёт на sway, Hyprland и
/// niri один и тот же [`De::Wlroots`], поэтому три строки подряд давали
/// покупателю ПОБИТОВО одинаковый каталог — и заставляли выбирать там, где
/// выбора нет (правило #13).
///
/// Под общим пунктом при этом едет НАСТОЯЩИЙ идентификатор: `current_de_id`,
/// если сейчас запущен (или ранее выбран) тайловый, иначе первый из секции
/// «Тайловые» ([`crate::qt_desktops::TILING_IDS`], Hyprland). Не первый
/// подходящий из [`crate::system::DE_META`]: там перед тайловыми стоит
/// Wayfire (тоже wlroots, но не тайловый), и объяснения звали бы общий пункт
/// «Wayfire». Настоящий он потому, что за пределами каталога разница есть:
/// объяснение «почему раздела нет» зовёт рабочий стол по имени («Sway», а не
/// «Тайловые»), а настройки приложений по умолчанию пишутся каждым в свой
/// `<id>-mimeapps.list`. Схлопывание живёт ТОЛЬКО в этом списке.
pub fn appearance_de_choices(current_de_id: &str) -> Vec<(String, String)> {
    use crate::de_customization::{de_from_id, De};

    let is_tiling = |id: &str| de_from_id(id) == Some(De::Wlroots);
    // Что поедет под общим пунктом: сначала то, что у человека на самом деле.
    let in_meta = |id: &str| crate::system::DE_META.iter().any(|m| m.id == id);
    let tiling_id = if is_tiling(current_de_id) {
        current_de_id
    } else {
        crate::qt_desktops::TILING_IDS
            .iter()
            .copied()
            .find(|id| is_tiling(id) && in_meta(id))
            .or_else(|| crate::system::DE_META.iter().map(|m| m.id).find(|id| is_tiling(id)))
            .unwrap_or_default()
    };

    let mut out: Vec<(String, String)> = Vec::new();
    let mut tiling_added = false;
    for m in crate::system::DE_META {
        if is_tiling(m.id) {
            if tiling_added {
                continue;
            }
            tiling_added = true;
            out.push((tiling_id.to_string(), De::Wlroots.display_label()));
        } else {
            out.push((m.id.to_string(), m.display_name.to_string()));
        }
    }
    out
}

// ============================================================================
// Группы боковой панели и подписи разделов по окружению
// ============================================================================

/// Группа боковой панели. Порядок вариантов — порядок групп на экране.
///
/// Плоский список из двух десятков разделов не говорил, что из них меняет
/// сам рабочий стол, что — все программы, а что — только программы Qt; в
/// MATE рядом стояли «Рамки окон (MATE)» и экран входа Plasma. Группа
/// отвечает на вопрос «на что это подействует» (просьба Wehrwolfmann
/// 16.10.2026: показывать только действующее и понятно подписанное).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SidebarGroup {
    /// Активные, загруженные, системные, избранное, витрины магазина.
    Overview,
    /// Родное оформление этого рабочего стола.
    Desktop,
    /// То, что подхватывают все программы: значки, курсоры, GTK, шрифты,
    /// звуки, обои.
    AllApps,
    /// Только программы Qt и KDE (вне KDE и LXQt).
    QtApps,
    /// Виджеты на рабочем столе: Plasma, Conky, десклеты Cinnamon.
    Widgets,
    /// Экран входа и заставка загрузки: видны до входа, общие для всех
    /// рабочих столов машины.
    System,
}

impl SidebarGroup {
    /// Короткий ключ — для строк модели боковой панели.
    pub fn key(self) -> &'static str {
        match self {
            SidebarGroup::Overview => "overview",
            SidebarGroup::Desktop => "desktop",
            SidebarGroup::AllApps => "all",
            SidebarGroup::QtApps => "qt",
            SidebarGroup::Widgets => "widgets",
            SidebarGroup::System => "system",
        }
    }
}

/// Окружение, у которого программы Qt — свои: там цвета и стиль Qt
/// меняют сам рабочий стол, а не «чужие» программы.
fn qt_native(de_id: &str) -> bool {
    use crate::de_customization::De;
    matches!(crate::de_customization::de_from_id(de_id), Some(De::Kde | De::Lxqt))
}

/// В какой группе боковой панели стоит раздел в этом окружении.
pub fn category_group(key: &str, de_id: &str) -> SidebarGroup {
    match key {
        "active" | "downloaded" | "system" | "favorites" | "popular" | "new" => SidebarGroup::Overview,
        "icons" | "cursor" | "gtk" | "fonts" | "sounds" | "wallpapers" => SidebarGroup::AllApps,
        // Разделы настроек, которые действуют на программы, а не на сам
        // стол: отклик при запуске, значки и панели инструментов, шрифты.
        "launchfeedback" | "appwidgets" | "fontsettings" => SidebarGroup::AllApps,
        "colors" | "widgetstyle" | "kvantum" => {
            if qt_native(de_id) {
                SidebarGroup::Desktop
            } else {
                SidebarGroup::QtApps
            }
        }
        "plasmoids" | "conky" | "cinnamondesklets" => SidebarGroup::Widgets,
        "sddm" | PLASMA_LOGIN_KEY | "plymouth" => SidebarGroup::System,
        _ => SidebarGroup::Desktop,
    }
}

/// Боковая панель по группам: внутри группы — прежний порядок
/// [`sidebar`], группы — в порядке [`SidebarGroup`]. В группе виджетов
/// Conky идёт последним: родные виджеты стола (Plasma, десклеты Cinnamon) —
/// первыми, Conky — рядом как общий для всех столов (24.10.2026).
pub fn grouped_sidebar(sidebar: &[ThemeCategory], de_id: &str) -> Vec<ThemeCategory> {
    let mut out = sidebar.to_vec();
    out.sort_by_key(|c| (category_group(&c.key, de_id), c.key == "conky"));
    // «Кнопки заголовка окна» — сразу за рамками окон этого стола, как
    // вкладка модуля «Оформление окон» KDE: у MATE рамки — раздел Marco в
    // хвосте разделов окружений, а не Aurorae в начале списка.
    if let Some(from) = out.iter().position(|c| c.key == WINDOW_BUTTONS_KEY) {
        let anchor = out
            .iter()
            .rposition(|c| WINDOW_BORDER_SECTIONS.contains(&c.key.as_str()) && category_available(&c.key, de_id));
        if let Some(anchor) = anchor {
            let buttons = out.remove(from);
            let at = if anchor > from { anchor } else { anchor + 1 };
            out.insert(at, buttons);
        }
    }
    out
}

/// Имя рабочего стола для заголовка группы: то же, что во вкладке
/// «Рабочие столы». Пусто — окружение не опознано.
fn desktop_name(de_id: &str) -> String {
    crate::system::DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name.to_string())
        .or_else(|| crate::de_customization::de_from_id(de_id).map(|d| d.display_label()))
        .unwrap_or_default()
}

/// Заголовок группы на языке интерфейса.
pub fn group_title(group: SidebarGroup, de_id: &str) -> String {
    match group {
        SidebarGroup::Overview => trc("appearance group", "Overview"),
        SidebarGroup::Desktop => {
            let name = desktop_name(de_id);
            if name.is_empty() {
                trc("appearance group", "Desktop")
            } else {
                // TRANSLATORS: sidebar group heading; %1 is the desktop (MATE, KDE Plasma…).
                crate::i18n::trf("Desktop “%1”", &[&name])
            }
        }
        SidebarGroup::AllApps => trc("appearance group", "For all apps"),
        SidebarGroup::QtApps => trc("appearance group", "Qt and KDE apps"),
        SidebarGroup::Widgets => trc("appearance group", "Desktop widgets"),
        SidebarGroup::System => trc("appearance group", "System: startup and login"),
    }
}

/// Пояснение под заголовком группы в этом окружении. Пусто — заголовка
/// хватает.
pub fn group_note(group: SidebarGroup, de_id: &str) -> String {
    match group {
        SidebarGroup::QtApps => tr("VLC, qBittorrent, Krita, Dolphin… The desktop itself doesn't change."),
        SidebarGroup::System => tr("Shared by all desktops on this computer."),
        SidebarGroup::Widgets => widgets_group_note(de_id),
        _ => String::new(),
    }
}

/// Пояснение группы «Виджеты рабочего стола» (решение Wehrwolfmann
/// 24.10.2026: виджеты на столе — в каждом окружении). Своих виджетов у
/// стола нет почти нигде, кроме KDE и Cinnamon, и человек, ищущий «виджеты
/// на стол», видит одно слово «Conky» — пояснение говорит, что это такое,
/// без жаргона. Где на Wayland у стола нет слоёв
/// (`conky_overlay::wayland_without_layers`: GNOME, Cinnamon, Pantheon),
/// виджет Conky — обычное окно, и это сказано здесь же, а не обещано как
/// рабочее. Сеанс не проверяется намеренно: выбран может быть и не тот
/// стол, что запущен, а фраза верна в обоих сеансах. Неизвестное окружение
/// видит все разделы виджетов разом — пояснять там нечем.
fn widgets_group_note(de_id: &str) -> String {
    use crate::de_customization::De;
    let Some(de) = crate::de_customization::de_from_id(de_id) else {
        return String::new();
    };
    match de {
        De::Kde => tr("Conky widgets — clocks, weather and sensors — come from a separate program and also work on other desktops."),
        De::Cinnamon => tr(
            "Desklets are Cinnamon's own widgets and stay on the desktop in any session. Conky widgets come from a \
             separate program; in the Wayland session they are regular windows.",
        ),
        _ if crate::conky_overlay::wayland_without_layers(de_id) => tr(
            "Clocks, weather and system sensors on the desktop, drawn by the Conky program. In the Wayland session \
             of this desktop they are regular windows; they stay on the desktop itself in the X11 session.",
        ),
        _ => tr("Clocks, weather and system sensors right on the desktop. They're drawn by the Conky program, which works on any desktop."),
    }
}

/// Подпись раздела в этом окружении.
///
/// Имя в [`categories`] от окружения не зависит (по нему ищут журнал и
/// сообщения установки), а в боковой панели и заголовке раздела оно должно
/// говорить правду о ЭТОМ рабочем столе: вне Plasma «Цветовые схемы» красят
/// только программы Qt и KDE, «Стиль приложений» — тоже; приписка «(MATE)» к
/// рамкам окон внутри группы «Рабочий стол «MATE»» лишняя. Неизвестное
/// окружение (видно всё) — прежние имена: там рядом стоят и рамки XFCE, и
/// рамки MATE.
pub fn display_name(cat: &ThemeCategory, de_id: &str) -> String {
    let known = crate::de_customization::de_from_id(de_id).is_some();
    match cat.key.as_str() {
        "sddm" | PLASMA_LOGIN_KEY => trc("appearance section", "Login Screen"),
        "plymouth" => trc("appearance section", "Boot Splash (Plymouth)"),
        "colors" if known && !matches!(crate::de_customization::de_from_id(de_id), Some(crate::de_customization::De::Kde)) => {
            trc("appearance section", "Qt App Colors")
        }
        "widgetstyle" if known && !qt_native(de_id) => trc("appearance section", "Qt App Style (Kvantum)"),
        "xfwm4" | "marco" | OPENBOX_KEY if known => trc("appearance section", "Window Borders"),
        MATE_PANEL_KEY if known => trc("appearance section", "Panel Layout"),
        _ => cat.name.clone(),
    }
}

/// Человеческое имя экрана входа по юниту systemd
/// (`appearance_installer::active_login_manager`). Пусто — незнакомый.
pub fn login_manager_label(unit: &str) -> &'static str {
    crate::system_plan::login_manager_spec(unit).map_or("", |s| s.label)
}

/// Пояснение под заголовком системного раздела (экран входа, заставка
/// загрузки): они видны ДО выбора рабочего стола и общие для всех столов,
/// и человек в MATE должен понимать, почему здесь экран входа Plasma.
/// `login_unit` — юнит активного экрана входа (пусто — не знаем).
/// `None` — раздел не системный.
pub fn system_section_note(key: &str, login_unit: &str) -> Option<String> {
    match key {
        "sddm" | PLASMA_LOGIN_KEY => {
            let shared = tr("Shared by all desktops.");
            let label = login_manager_label(login_unit);
            Some(if label.is_empty() {
                shared
            } else if key == "sddm" && login_unit.trim() != "sddm" {
                // Полка SDDM-тем при другом экране входа: сказать заранее,
                // что темы подействуют, только если им станет SDDM.
                format!(
                    "{shared} {}",
                    crate::i18n::trf(
                        "Right now it's shown by %1, and the themes here are for SDDM: \
                         they only take effect if SDDM becomes the login screen.",
                        &[&label],
                    )
                )
            } else {
                format!("{shared} {}", crate::i18n::trf("Right now it's shown by %1.", &[&label]))
            })
        }
        "plymouth" => Some(tr("Shown when the computer starts, before you log in.")),
        _ => None,
    }
}
