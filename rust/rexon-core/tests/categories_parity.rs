//! Golden-сверка порта категорий с оригиналом `rexon/appearance/categories.py`.
//!
//! Эталон `golden/categories_all.json` снят с самого Python при
//! `HOME=/home/tester`: `json.dumps({"categories": [asdict(c) for c in
//! CATEGORIES.values()], "sidebar": [asdict(c) for c in SIDEBAR]},
//! ensure_ascii=False, indent=1)`. Любое расхождение (id/имя/иконка/путь/
//! apply_kind/флаги/каталог/порядок) — ошибка транскрипции таблицы.
//!
//! Осознанные правки эталона: «Виджеты» 706 → 705 (04.10.2026, зонт
//! «Plasma 6 Extensions» вместо одних апплетов — как у Discover); пара
//! полей `local_only` + `store_cats` заменена одним полем каталога `store`
//! (17.10.2026: каталог раздела задаётся явно, а не выводится из номера).

use std::path::Path;

use rexon_core::categories::{
    categories, category_by_id, category_by_kind, get_category, is_de_section, sidebar,
    AGGREGATE_CATS, DE_SECTIONS, LOCAL_VIEW_KEYS,
};
use rexon_core::models::to_json_indent1;

mod i18n_pin;

/// Тот же фиксированный дом, что в скрипте снятия эталона.
fn home() -> &'static Path {
    Path::new("/home/tester")
}

/// Полная таблица категорий + сайдбар сериализуются байт-в-байт как
/// `asdict`-дамп Python (проверяет всю транскрипцию и порядок).
///
/// Эталон снят с русского интерфейса, поэтому сверка идёт на русском
/// каталоге `po/ru.po`: заодно она держит слово, что имена разделов
/// по-русски остались прежними.
#[test]
fn categories_and_sidebar_byte_identical() {
    let _lang = i18n_pin::ru();
    let golden = include_str!("golden/categories_all.json");
    // Разделы окружений (04.10.2026), «Панель MATE» (17.10.2026) и разделы
    // настроек (01.11.2026) эталон Python не знал — сверяется всё
    // остальное; сами разделы держат сверки ниже.
    let python_era = |v: Vec<rexon_core::models::ThemeCategory>| {
        v.into_iter()
            .filter(|c| {
                !is_de_section(c.id)
                    && c.key != rexon_core::categories::MATE_PANEL_KEY
                    && !rexon_core::categories::is_settings_panel(&c.key)
            })
            .collect::<Vec<_>>()
    };
    let value = serde_json::json!({
        "categories": python_era(categories(home())),
        "sidebar": python_era(sidebar(home())),
    });
    assert_eq!(to_json_indent1(&value), golden);
}

/// По-английски — исходные имена разделов.
#[test]
fn section_names_in_english() {
    let _lang = i18n_pin::en();
    let name = |key: &str| get_category(home(), key).unwrap().name;
    assert_eq!(name("colors"), "Color Schemes");
    assert_eq!(name("aurorae"), "Window Decorations (Aurorae)");
    assert_eq!(name("sddm"), "SDDM (Login Screen)");
    assert_eq!(name("widgetstyle"), "Application Style");
    assert_eq!(name("active"), "Active");
    assert_eq!(name("favorites"), "Favorites");
    assert_eq!(category_by_id(home(), 123).unwrap().name, "Kvantum");
    assert_eq!(name("global"), "Global Themes");
}

/// Число категорий и сайдбара — как в Python (21 реальная, 23 в сайдбаре)
/// плюс разделы окружений: каждый и в таблице, и в боковой панели; плюс
/// «Панель MATE» — только в боковой панели (ставить в неё нечего).
#[test]
fn counts_match_python() {
    assert_eq!(categories(home()).len(), 21 + DE_SECTIONS.len());
    assert_eq!(
        sidebar(home()).len(),
        23 + DE_SECTIONS.len() + 1 + rexon_core::categories::SETTINGS_PANELS.len()
    );
}

/// Разделы окружений: у каждого свой тип, кнопка из `de_apply`, каталог из
/// матрицы, и виден он только там, где тип применим.
#[test]
fn de_sections_follow_the_matrix() {
    let _lang = i18n_pin::en();
    for (id, key) in DE_SECTIONS {
        let c = category_by_id(home(), *id).unwrap();
        assert_eq!(c.key, *key);
        // Каталог — из поля раздела, не из номера.
        assert_eq!(c.store, rexon_core::categories::de_section_store(key), "{key}");
        let kind = rexon_core::categories::category_kind(key).unwrap();
        assert!(rexon_core::de_apply::mode(kind).is_some(), "{key}: нет кнопки");
        assert!(!c.install_dir.is_empty(), "{key}: каталог из матрицы");
        assert!(rexon_core::appearance_installer::applied_by_button(kind), "{key}");
    }
    let shown = |key: &str, de: &str| rexon_core::categories::category_available(key, de);
    assert!(shown("xfwm4", "xfce") && !shown("xfwm4", "kde") && !shown("xfwm4", "mate"));
    assert!(shown("marco", "mate") && !shown("marco", "xfce"));
    assert!(shown("gnomeshell", "gnome") && !shown("gnomeshell", "cinnamon"));
    assert!(shown("cinnamontheme", "cinnamon") && shown("nemoactions", "cinnamon"));
    assert!(shown("lxqttheme", "lxqt") && !shown("lxqttheme", "kde"));
    assert!(shown("tabbox", "kde") && shown("kwinscripts", "kde") && !shown("kwinscripts", "gnome"));
    assert!(shown("gnomeext", "gnome") && !shown("gnomeext", "kde"));
    // Conky — отдельная программа: работает везде.
    for de in ["kde", "gnome", "xfce", "mate", "cinnamon", "lxqt", "sway"] {
        assert!(shown("conky", de), "conky в {de}");
    }
    assert_eq!(get_category(home(), "xfwm4").unwrap().name, "Window Borders (XFCE)");
}

/// `category_by_id` — как `CATEGORIES.get`: находит реальную по cat_id.
/// Для дублирующихся ключей (splash: 902/716/488) id уникальны и резолвятся
/// в свою запись.
#[test]
fn category_by_id_resolves() {
    let c = category_by_id(home(), 722).unwrap();
    assert_eq!(c.key, "global");
    assert_eq!(c.apply_kind, "global");
    assert_eq!(c.install_dir, "/home/tester/.local/share/plasma/look-and-feel");
    assert!(category_by_id(home(), 999999).is_none());
    // splash-«зонт» 902 и реальные 716/488 — разные записи с общим ключом.
    assert_eq!(category_by_id(home(), 902).unwrap().ocs_cats(), [716, 488]);
    assert_eq!(category_by_id(home(), 716).unwrap().ocs_cats(), [716]);
}

/// Разделы с магазином KDE — те же категории, что у Discover (knsrc):
/// «Виджеты» — зонт 705, KWin — 719/720/721. Товар приходит с номером
/// своей подкатегории, и этот номер обязан вести в раздел: от него зависят
/// каталог установки, вид и кнопка.
#[test]
fn store_sections_and_item_type_ids() {
    let widgets = category_by_id(home(), 705).unwrap();
    assert_eq!(widgets.key, "plasmoids");
    assert!(!widgets.local_only() && widgets.ocs_cats() == [705]);
    assert_eq!(widgets.install_dir, "/home/tester/.local/share/plasma/plasmoids");
    // Апплеты, календари, часы, меню, мониторинг, мультимедиа, погода.
    for typeid in 706..=712 {
        let c = category_by_id(home(), typeid).unwrap_or_else(|| panic!("{typeid}"));
        assert_eq!(c.id, 705, "{typeid}");
        assert_eq!(c.apply_kind, "plasmoids", "{typeid}");
    }
    for (id, key, dir) in [
        (719, "kwineffects", "/home/tester/.local/share/kwin/effects"),
        (720, "kwinscripts", "/home/tester/.local/share/kwin/scripts"),
        (721, "tabbox", "/home/tester/.local/share/kwin/tabbox"),
    ] {
        let c = category_by_id(home(), id).unwrap();
        assert_eq!((c.key.as_str(), c.install_dir.as_str()), (key, dir));
        assert_eq!(c.ocs_cats(), [id], "{key}: у раздела есть магазин");
        assert!(is_de_section(id), "{key}: раздел окружения");
    }
    // Прежние местные номера (до 04.10.2026) ведут туда же.
    assert_eq!(category_by_id(home(), 911).unwrap().key, "tabbox");
    assert_eq!(category_by_id(home(), 912).unwrap().key, "kwinscripts");
    assert_eq!(category_by_id(home(), 913).unwrap().key, "kwineffects");
    // Прочие разделы окружений по-прежнему местные.
    assert!(category_by_id(home(), 914).unwrap().local_only());
    // Видны только в KDE — это решает матрица.
    let shown = rexon_core::categories::category_available;
    assert!(shown("kwineffects", "kde") && !shown("kwineffects", "xfce"));
    assert!(shown("plasmoids", "kde"));
}

/// `category_by_kind` — первая в порядке объявления `CATEGORIES` с этим
/// apply_kind (для splash это «зонт» 902, объявленный раньше 716/488).
#[test]
fn category_by_kind_first_declared() {
    assert_eq!(category_by_kind(home(), "icons").unwrap().id, 132);
    assert_eq!(category_by_kind(home(), "splash").unwrap().id, 902);
    assert!(category_by_kind(home(), "no_such_kind").is_none());
}

/// `get_category` перебирает САЙДБАР (включая виртуальные) и берёт первую с
/// ключом.
#[test]
fn get_category_from_sidebar() {
    assert!(get_category(home(), "active").unwrap().virtual_);
    assert_eq!(get_category(home(), "popular").unwrap().default_sort, "high");
    let global = get_category(home(), "global").unwrap();
    assert!(!global.virtual_);
    assert_eq!(global.id, 722);
    assert!(get_category(home(), "missing").is_none());
}

/// Константы обзорных ключей и агрегата совпадают с Python.
#[test]
fn constants_match() {
    assert_eq!(LOCAL_VIEW_KEYS, ["active", "downloaded", "system", "favorites"]);
    assert_eq!(AGGREGATE_CATS, [722, 104, 112, 132, 107]);
}

// ============================================================================
// Фильтр каталога по окружению
// ============================================================================

/// Каждый раздел боковой панели должен быть либо привязан к типу матрицы,
/// либо осознанно «всегда виден» (обзоры установленного и витрины). Забыть
/// новый раздел нельзя — тест перечисляет исключения поимённо.
#[test]
fn every_sidebar_section_is_classified() {
    use rexon_core::categories::category_kind;

    const ALWAYS_VISIBLE: &[&str] =
        &["active", "downloaded", "system", "favorites", "popular", "new"];
    for c in sidebar(home()) {
        if ALWAYS_VISIBLE.contains(&c.key.as_str()) {
            assert!(category_kind(&c.key).is_none(), "{} — обзор, тип не нужен", c.key);
            continue;
        }
        // «Панель MATE» — не тип оформления, а раскладки mate-panel: где она
        // видна, решает своё правило (`category_available`, только MATE).
        if c.key == rexon_core::categories::MATE_PANEL_KEY {
            assert!(category_kind(&c.key).is_none());
            continue;
        }
        // Разделы настроек — не тип оформления: где они видны, решает их
        // правило (`category_available`, по разделу «Настроек»).
        if rexon_core::categories::is_settings_panel(&c.key) {
            assert!(category_kind(&c.key).is_none());
            continue;
        }
        let kind = category_kind(&c.key)
            .unwrap_or_else(|| panic!("раздел «{}» не привязан к типу матрицы", c.name));
        assert!(
            rexon_core::de_customization::row(kind).is_some(),
            "{}: тип {kind} не описан матрицей",
            c.key
        );
    }
}

/// Покупателю не показывают то, что в его окружении не работает; обзоры и
/// витрины видны всегда.
#[test]
fn sections_filtered_by_desktop() {
    use rexon_core::categories::category_available;

    // KDE: своё всё на месте, и GTK-темы тоже (Firefox, GIMP, kde-gtk-config).
    for key in ["plasmoids", "global", "aurorae", "gtk", "wallpapers", "widgetstyle"] {
        assert!(category_available(key, "kde"), "{key} пропал в KDE");
    }
    // GNOME: KDE-шного нет, общее есть.
    for key in ["plasmoids", "aurorae", "plasma"] {
        assert!(!category_available(key, "gnome"), "{key} не должен быть в GNOME");
    }
    for key in [
        "gtk", "icons", "cursor", "wallpapers", "fonts", "sounds", "plymouth",
        // Kvantum — движок стилей Qt: под GNOME живут VLC, qBittorrent,
        // Krita, OBS, и он красит их точно так же.
        "widgetstyle", "kvantum",
        // Цветовые схемы KDE вне Plasma (f83e0bff): kdeglobals для программ
        // KDE и палитра qt6ct для Qt — раздел нарочно открыт и в GNOME.
        "colors",
        // Глобальные темы вне Plasma (01.11.2026) — по частям, тем же
        // путём, что «Как в дистрибутиве»: цвета, стиль приложений, значки,
        // курсоры, шрифты, кнопки заголовка (`themes::apply_global_choice_plan`).
        "global",
    ] {
        assert!(category_available(key, "gnome"), "{key} пропал в GNOME");
    }
    // Тайловые: обои, шрифты, значки, звуки (их играет демон уведомлений,
    // а не оболочка) — да; плазмоиды — нет.
    for de in ["sway", "hyprland", "niri"] {
        assert!(category_available("wallpapers", de));
        assert!(category_available("fonts", de));
        assert!(category_available("sounds", de), "звуки пропали в {de}");
        assert!(!category_available("plasmoids", de));
    }
    // Qt-приложения есть везде — значит и раздел стилей есть везде.
    for de in ["lxqt", "xfce", "cosmic", "sway"] {
        assert!(category_available("widgetstyle", de), "стили Qt пропали в {de}");
    }
    // Обзоры и витрины — всегда.
    for key in ["active", "downloaded", "system", "favorites", "popular", "new"] {
        assert!(category_available(key, "gnome"));
    }
    // Незнакомое окружение — не прячем ничего.
    assert!(category_available("plasmoids", "какой-то-новый-wm"));
}

/// Пустой экран без объяснения — недопустим: у скрытого раздела обязан быть
/// человеческий текст, у доступного — не должно быть никакого.
#[test]
fn hidden_section_explains_itself() {
    use rexon_core::categories::category_note;

    {
        let _lang = i18n_pin::ru();
        assert!(category_note(home(), "gtk", "kde").is_none());
        let ru = category_note(home(), "plasmoids", "gnome").expect("объяснение");
        assert!(ru.starts_with("«Виджеты»\n"), "нет названия раздела: {ru}");
        assert!(ru.contains("KDE Plasma") && ru.contains("GNOME"));
        assert!(
            ru.ends_with("Выберите раздел слева — доступное для вашего рабочего стола осталось на месте."),
            "{ru}"
        );
    }
    let _lang = i18n_pin::en();
    let en = category_note(home(), "plasmoids", "gnome").unwrap();
    assert!(en.starts_with("“Widgets”\n"), "{en}");
    assert!(en.ends_with("everything your desktop can actually use is still there."), "{en}");
}

/// Раздел, который работает при условии, показывается — но условие названо.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn conditional_section_names_its_condition() {
    use rexon_core::categories::{category_available, category_condition};

    // Звуки в тайловых: раздел на месте, оговорка про демон уведомлений.
    assert!(category_available("sounds", "sway"));
    let en = {
        let _lang = i18n_pin::en();
        category_condition("sounds", "sway").expect("condition")
    };
    let _lang = i18n_pin::ru();
    let ru = category_condition("sounds", "sway").expect("оговорка");
    assert!(ru.contains("swaync"), "нет условия: {ru}");
    assert_ne!(ru, en);
    assert!(category_condition("sounds", "kde").is_none());

    // Kvantum вне KDE/LXQt: раздел на месте, оговорка про перезаход.
    for key in ["kvantum", "widgetstyle"] {
        let note = category_condition(key, "gnome").expect("оговорка");
        assert!(note.contains("вход"), "нет условия про вход: {note}");
        assert!(category_condition(key, "kde").is_none());
        assert!(category_condition(key, "lxqt").is_none());
        // Д1: где переменные сеанса до композитора дойти не могут (sway,
        // river, Wayfire), оговорка говорит это прямо; Hyprland, niri и
        // XFCE их получают — там лишнего не пишем.
        let sway = category_condition(key, "sway").expect("оговорка sway");
        assert!(sway.starts_with(&note) && sway.contains("sway") && sway.contains("uwsm"), "{sway}");
        for de in ["hyprland", "niri", "xfce", "mate"] {
            assert!(!category_condition(key, de).unwrap().contains("uwsm"), "{de}");
        }
    }
    // Обзоры диска и незнакомые окружения — без условий.
    assert!(category_condition("active", "gnome").is_none());
    assert!(category_condition("sounds", "какой-то-новый-wm").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
//  Экран входа: SDDM, Plasma Login Manager, SteamOS (28.09.2026)
// ═══════════════════════════════════════════════════════════════════════

/// При SDDM панель ровно прежняя; при Plasma Login Manager на месте
/// «SDDM (экран входа)» стоит его раздел, остальное не сдвинулось.
#[test]
fn login_section_follows_the_login_manager() {
    use rexon_core::categories::{plasma_login_category, sidebar_for, PLASMA_LOGIN_KEY};
    let _lang = i18n_pin::en();
    let keys = |v: &[rexon_core::models::ThemeCategory]| -> Vec<String> {
        v.iter().map(|c| c.key.clone()).collect()
    };

    let sddm = sidebar_for(home(), false);
    assert_eq!(keys(&sddm), keys(&sidebar(home())), "при SDDM панель не меняется");

    let plm = sidebar_for(home(), true);
    assert_eq!(plm.len(), sddm.len());
    let at = sddm.iter().position(|c| c.key == "sddm").expect("раздел SDDM есть");
    assert_eq!(plm[at].key, PLASMA_LOGIN_KEY, "раздел PLM на месте SDDM");
    assert!(plm.iter().all(|c| c.key != "sddm" && c.id != 101), "полки SDDM-тем нет");
    for (i, (a, b)) in sddm.iter().zip(&plm).enumerate() {
        if i != at {
            assert_eq!(a.key, b.key, "строка {i} сдвинулась");
        }
    }

    let c = plasma_login_category();
    assert_eq!(c.name, "Login Screen (Plasma Login Manager)");
    assert_eq!(c.icon, "🔐");
    // Без сети и без прогрева: номер вне магазина, раздел местный.
    assert!(c.local_only() && c.id > 900 && !c.requires_root);
    // Раздел находят по ключу, но в таблицу товаров он не попал.
    assert_eq!(get_category(home(), PLASMA_LOGIN_KEY).unwrap().id, c.id);
    assert!(categories(home()).iter().all(|x| x.key != PLASMA_LOGIN_KEY));
    assert!(category_by_kind(home(), "plasmalogin").is_none());
    // Матрица окружений у него та же, что у SDDM-полки.
    assert_eq!(
        rexon_core::categories::category_kind(PLASMA_LOGIN_KEY),
        rexon_core::categories::category_kind("sddm")
    );

    let _ru = {
        drop(_lang);
        i18n_pin::ru()
    };
    assert_eq!(plasma_login_category().name, "Экран входа (Plasma Login Manager)");
    // Имя SDDM-раздела по-русски — прежнее.
    assert_eq!(get_category(home(), "sddm").unwrap().name, "SDDM (вход)");
}

/// SteamOS: экран входа и заставка загрузки закрыты ЗАРАНЕЕ — раздел на
/// месте и сразу говорит почему; прочие разделы не задеты.
#[test]
fn steamos_closes_login_and_boot_splash_up_front() {
    use rexon_core::categories::{steamos_block, steamos_note};
    use rexon_core::distro_compat::SteamOsBlock;
    let _lang = i18n_pin::en();

    assert_eq!(steamos_block("sddm"), Some(SteamOsBlock::LoginScreen));
    assert_eq!(steamos_block("plasmalogin"), Some(SteamOsBlock::LoginScreen));
    assert_eq!(steamos_block("plymouth"), Some(SteamOsBlock::BootSplash));
    for key in ["global", "icons", "splash", "lockscreen", "wallpapers", "active", "system"] {
        assert_eq!(steamos_block(key), None, "{key}");
        assert_eq!(steamos_note(home(), key, true), None, "{key}");
    }
    // Не SteamOS — ничего не закрыто.
    assert_eq!(steamos_note(home(), "sddm", false), None);
    assert_eq!(steamos_note(home(), "plymouth", false), None);

    assert_eq!(
        steamos_note(home(), "sddm", true).unwrap(),
        "“SDDM (Login Screen)”\n\
         On SteamOS the login screen can't be changed: switching to Game Mode depends on it.\n\
         Pick another section on the left — everything your desktop can actually use is still there."
    );
    assert_eq!(
        steamos_note(home(), "plymouth", true).unwrap(),
        "“Plymouth”\n\
         On SteamOS the boot splash can't be changed: every system update puts the original one back.\n\
         Pick another section on the left — everything your desktop can actually use is still there."
    );
    // Тексты — те же, что у отказа установщика: одна причина на продукт.
    assert!(steamos_note(home(), "sddm", true).unwrap().contains(&SteamOsBlock::LoginScreen.reason()));

    drop(_lang);
    let _ru = i18n_pin::ru();
    assert_eq!(
        steamos_note(home(), "sddm", true).unwrap(),
        "«SDDM (вход)»\n\
         На SteamOS экран входа менять нельзя: на нём держится переход в игровой режим.\n\
         Выберите раздел слева — доступное для вашего рабочего стола осталось на месте."
    );
}

// ============================================================================
// Группы боковой панели и подписи по окружению (16.10.2026)
// ============================================================================

use rexon_core::categories::{
    category_available, category_group, display_name, group_title, grouped_sidebar,
    system_section_note, SidebarGroup,
};

/// Видимые в окружении разделы по группам, в порядке экрана.
fn visible_groups(de: &str) -> Vec<(SidebarGroup, Vec<String>)> {
    let mut out: Vec<(SidebarGroup, Vec<String>)> = Vec::new();
    for c in grouped_sidebar(&sidebar(home()), de) {
        if !category_available(&c.key, de) {
            continue;
        }
        let g = category_group(&c.key, de);
        match out.last_mut() {
            Some((last, keys)) if *last == g => keys.push(c.key.clone()),
            _ => out.push((g, vec![c.key.clone()])),
        }
    }
    out
}

#[test]
fn mate_sidebar_is_grouped_by_what_it_changes() {
    let _lang = i18n_pin::ru();
    let groups = visible_groups("mate");
    let order: Vec<SidebarGroup> = groups.iter().map(|(g, _)| *g).collect();
    assert_eq!(
        order,
        [
            SidebarGroup::Overview,
            SidebarGroup::Desktop,
            SidebarGroup::AllApps,
            SidebarGroup::QtApps,
            SidebarGroup::Widgets,
            SidebarGroup::System
        ],
        "{groups:?}"
    );
    let keys = |g: SidebarGroup| groups.iter().find(|(x, _)| *x == g).unwrap().1.clone();
    // Глобальные темы — первыми, как в KDE (вне Plasma — по частям,
    // 01.11.2026); кнопки заголовка — сразу за рамками Marco, анимация —
    // последней (разделы настроек, 01.11.2026).
    assert_eq!(keys(SidebarGroup::Desktop), ["global", "marco", "windowbuttons", "mate-panel", "animations"]);
    assert_eq!(
        keys(SidebarGroup::AllApps),
        ["icons", "cursor", "launchfeedback", "gtk", "appwidgets", "wallpapers", "fontsettings", "fonts", "sounds"]
    );
    assert_eq!(keys(SidebarGroup::QtApps), ["colors", "widgetstyle"]);
    assert_eq!(keys(SidebarGroup::Widgets), ["conky"]);
    assert_eq!(keys(SidebarGroup::System), ["sddm", "plymouth"]);
    // Каждая группа идёт одним куском: grouped_sidebar не рвёт группы.
    let mut seen = std::collections::BTreeSet::new();
    for (g, _) in &groups {
        assert!(seen.insert(*g), "группа {g:?} встречается дважды");
    }
    assert_eq!(group_title(SidebarGroup::Desktop, "mate"), "Рабочий стол «MATE»");
    assert_eq!(group_title(SidebarGroup::System, "mate"), "Система: загрузка и вход");
}

#[test]
fn kde_keeps_qt_sections_on_the_desktop_and_their_old_names() {
    let _lang = i18n_pin::ru();
    let groups = visible_groups("kde");
    assert!(groups.iter().all(|(g, _)| *g != SidebarGroup::QtApps), "{groups:?}");
    let desktop = &groups.iter().find(|(g, _)| *g == SidebarGroup::Desktop).unwrap().1;
    for k in ["global", "plasma", "colors", "aurorae", "widgetstyle"] {
        assert!(desktop.iter().any(|x| x == k), "{k} не в группе рабочего стола KDE");
    }
    let all = categories(home());
    let by = |k: &str| all.iter().find(|c| c.key == k).unwrap().clone();
    assert_eq!(display_name(&by("colors"), "kde"), by("colors").name);
    assert_eq!(display_name(&by("widgetstyle"), "kde"), by("widgetstyle").name);
}

#[test]
fn section_names_tell_the_truth_outside_plasma() {
    let _lang = i18n_pin::ru();
    let all = categories(home());
    let by = |k: &str| all.iter().find(|c| c.key == k).unwrap().clone();
    assert_eq!(display_name(&by("widgetstyle"), "mate"), "Стиль Qt-программ (Kvantum)");
    assert_eq!(display_name(&by("colors"), "mate"), "Цвета Qt-программ");
    assert_eq!(display_name(&by("marco"), "mate"), "Рамки окон");
    assert_eq!(display_name(&by("xfwm4"), "xfce"), "Рамки окон");
    assert_eq!(display_name(&by("sddm"), "mate"), "Экран входа в систему");
    assert_eq!(display_name(&by("plymouth"), "kde"), "Заставка загрузки (Plymouth)");
    // Неизвестное окружение видит обе рамки — приписки остаются.
    assert_eq!(display_name(&by("marco"), "nonesuch"), by("marco").name);
}

#[test]
fn login_screen_says_it_is_shared_and_who_draws_it() {
    let _lang = i18n_pin::ru();
    let plm = system_section_note("plasmalogin", "plasmalogin").unwrap();
    assert!(plm.contains("Общий для всех рабочих столов"), "{plm}");
    assert!(plm.contains("Plasma Login Manager"), "{plm}");
    let sddm = system_section_note("sddm", "sddm").unwrap();
    assert!(sddm.contains("SDDM") && !sddm.contains("только если"), "{sddm}");
    // Полка SDDM-тем при LightDM — предупредить заранее.
    let light = system_section_note("sddm", "lightdm").unwrap();
    assert!(light.contains("LightDM") && light.contains("SDDM"), "{light}");
    assert!(system_section_note("plymouth", "").unwrap().contains("до входа"));
    assert!(system_section_note("icons", "sddm").is_none());
}

/// Сторож общего вида «установленные + «Загрузить новые…»»: у КАЖДОГО
/// раздела боковой панели, у которого есть и скан диска, и магазин, есть
/// сетка установленного и кнопка магазина — без списка разделов. Conky и
/// разделы KWin до 17.10.2026 показывали только магазин, хотя скан у них был.
#[test]
fn every_section_with_scan_and_store_shows_installed_and_store_together() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    let _lang = i18n_pin::ru();
    let mut checked = Vec::new();
    for c in sidebar(home()) {
        if c.virtual_ || c.key == "lockscreen" {
            continue;
        }
        let scan = !ap::local_scan_plan(&c.key).0.is_empty();
        let store = !c.local_only();
        // Неизвестное окружение видит все разделы — проверяются все сразу.
        let v = CatalogView::new(c.clone(), "nonesuch");
        assert_eq!(v.has_installed_view(), scan && store, "{}", c.key);
        if scan && store {
            // Установленное сверху, магазин раздела под ним сразу — без
            // кнопки «Загрузить новые…» (решение владельца 18.10.2026).
            assert!(v.installed_view() && !v.is_local() && v.marks_current(), "{}", c.key);
            assert!(v.request_key().starts_with("combined:"), "{}", c.key);
            let ch = v.chrome();
            assert!(ch.pagination && ch.sort_combo, "{}: магазин листается и сортируется", c.key);
            assert!(ap::has_installed_grid(&c.key), "{}", c.key);
            checked.push(c.key.clone());
        }
    }
    for key in ["conky", "tabbox", "kwinscripts", "kwineffects", "icons", "global", "splash"] {
        assert!(checked.iter().any(|k| k == key), "{key}: {checked:?}");
    }
    // Магазинный раздел без скана (обои, виджеты Plasma) — по-прежнему магазин.
    assert!(!ap::has_installed_grid("wallpapers") && !ap::has_installed_grid("plasmoids"));
}

/// Каталог раздела — поле `store`, а не номер: запрос и прогрев берут
/// номера из него, а переключатель «Магазин:» есть только там, где у AUR
/// есть что показать.
#[test]
fn catalog_source_is_a_field_not_a_number() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    use rexon_core::models::CatalogSource;
    let _lang = i18n_pin::ru();
    let by = |k: &str| get_category(home(), k).unwrap();
    assert_eq!(by("popular").store, CatalogSource::OcsShowcase);
    assert_eq!(by("active").store, CatalogSource::None);
    assert_eq!(ap::store_query(&by("splash"), "kde"), "716,488");
    assert_eq!(ap::store_query(&by("icons"), "kde"), "132");
    let open_store = |k: &str| CatalogView::new(by(k), "kde");
    assert!(open_store("global").source_applicable());
    assert!(CatalogView::new(by("popular"), "kde").source_applicable());
    // У AUR нет пакетов переключателей окон и наборов Conky — чипа «AUR» нет.
    for k in ["tabbox", "conky", "splash"] {
        let v = open_store(k);
        assert!(!v.is_local() && !v.source_applicable(), "{k}");
        assert_eq!(ap::source_keys_for(k), ["ocs"], "{k}");
    }
    assert!(open_store("kwinscripts").source_applicable());
}

/// Разделы окружений с каталогом pling: установленное — сверху, магазин —
/// под ним на той же странице, товар с номером категории магазина ведёт в
/// свой раздел (вид установки и кнопка — оттуда), прогрев спрашивает ту же
/// категорию, что живой запрос.
#[test]
fn de_sections_get_their_pling_catalogs() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    let _lang = i18n_pin::ru();
    for (key, ocs, de) in [
        ("xfwm4", 138, "xfce"),
        ("marco", 125, "mate"),
        ("gnomeshell", 134, "gnome"),
        ("lxqttheme", 446, "lxqt"),
    ] {
        let c = get_category(home(), key).unwrap();
        assert_eq!(c.ocs_cats(), [ocs], "{key}");
        assert!(c.id > 900, "{key}: номер раздела прежний");
        assert_eq!(category_by_id(home(), ocs).unwrap().key, key, "{key}");
        let v = CatalogView::new(c.clone(), de);
        assert!(v.installed_view(), "{key}: сначала установленное");
        assert_eq!(v.query_category_id(), ocs.to_string(), "{key}");
        assert!(v.request_key().contains(&format!(":ocs:{ocs}:")), "{key}");
        let targets = ap::prefetch_targets(&v, &sidebar(home()));
        assert!(targets.iter().any(|(id, _)| *id == ocs.to_string()), "{key}: {targets:?}");
    }
}

/// Пять разделов Cinnamon — магазин Cinnamon Spices (как у окна настроек
/// Cinnamon): установленное сверху и магазин раздела под ним сразу, без
/// кнопки «Загрузить новые…» (решение владельца 18.10.2026); свой ключ
/// запроса, поиск, сортировка и страницы; KDE Store и AUR там нет. Прежний
/// номер каталога тем (cinnamon-look, 133) ведёт в раздел тем Cinnamon.
#[test]
fn cinnamon_sections_show_installed_and_spices_store() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    use rexon_core::models::CatalogSource;
    let _lang = i18n_pin::ru();
    for (key, json, id) in [
        ("cinnamontheme", "themes", 909),
        ("cinnamonapplets", "applets", 915),
        ("cinnamondesklets", "desklets", 916),
        ("cinnamonext", "extensions", 917),
        ("nemoactions", "actions", 918),
    ] {
        let c = get_category(home(), key).unwrap();
        assert_eq!(c.id, id, "{key}");
        assert_eq!(c.store, CatalogSource::Spices(json.into()), "{key}");
        assert!(!c.local_only() && c.ocs_cats().is_empty(), "{key}");
        assert!(ap::has_installed_grid(key), "{key}");
        let mut v = CatalogView::new(c.clone(), "cinnamon");
        assert!(v.installed_view() && !v.is_local() && v.spices_mode(), "{key}");
        assert_eq!(v.spices_json(), Some(json), "{key}");
        assert_eq!(ap::source_keys_for(key), vec![ap::SOURCE_SPICES], "{key}");
        assert!(!v.source_applicable() && !v.aur_mode(), "{key}");
        assert_eq!(v.effective_source(), ap::SOURCE_SPICES, "{key}");
        // Выбор «AUR» из другого раздела здесь не действует.
        v.source = ap::SOURCE_AUR.into();
        assert!(!v.aur_mode() && v.spices_mode(), "{key}");
        assert_eq!(v.effective_source(), ap::SOURCE_SPICES, "{key}");
        let rk = v.request_key();
        assert!(rk.starts_with("combined:") && rk.contains(&format!(":spices:{json}:0:")), "{key}: {rk}");
        v.query = "weather".into();
        v.page = 2;
        assert!(v.request_key().ends_with(&format!(":spices:{json}:2:{}:weather", v.active_sort())), "{}", v.request_key());
        let ch = v.chrome();
        assert!(ch.search && ch.sort_combo && ch.pagination && !ch.source_bar, "{key}");
        assert!(ch.search_placeholder.contains("Cinnamon Spices"), "{}", ch.search_placeholder);
    }
    assert_eq!(category_by_id(home(), 133).unwrap().key, "cinnamontheme");
    // Остальные разделы магазина своих источников не потеряли.
    assert_eq!(ap::source_keys_for("icons")[0], ap::SOURCE_OCS);
    assert_eq!(CatalogView::new(get_category(home(), "icons").unwrap(), "kde").effective_source(), ap::SOURCE_OCS);
}

/// Темы Cinnamon Spices вне Cinnamon: в «Темах GTK» (во всех окружениях), в
/// рамках окон XFCE и MATE и в теме GNOME Shell — чип «Cinnamon Spices»
/// рядом с KDE Store.
/// Выбран — витрина тем сайта своего раздела (ключ запроса у каждого
/// раздела свой: отбор по составу разный), поиск без подсказок KDE Store.
/// В разделах Cinnamon всё как было: Spices — единственный магазин.
#[test]
fn spices_themes_live_in_gtk_and_border_sections_too() {
    use rexon_core::appearance_page::{self as ap, CatalogView, SuggestAction};
    let _lang = i18n_pin::ru();
    for (key, de) in [("gtk", "kde"), ("gtk", "budgie"), ("gtk", "mate"), ("xfwm4", "xfce"), ("marco", "mate"), ("gnomeshell", "gnome")] {
        let c = get_category(home(), key).unwrap();
        assert!(ap::source_keys_for(key).contains(&ap::SOURCE_SPICES), "{key}");
        assert_eq!(ap::source_keys_for(key)[0], ap::SOURCE_OCS, "{key}: KDE Store по-прежнему первый");
        assert!(ap::source_chips_for(key).iter().any(|ch| ch.key == ap::SOURCE_SPICES && ch.label == "Cinnamon Spices" && !ch.tooltip.is_empty()), "{key}");
        let mut v = CatalogView::new(c, de);
        assert!(v.source_applicable() && !v.spices_mode(), "{key}: по умолчанию — KDE Store");
        assert!(v.chrome().source_bar, "{key}: переключатель магазинов на месте");
        v.source = ap::SOURCE_SPICES.into();
        assert!(v.spices_mode() && v.spices_parts_mode() && !v.aur_mode(), "{key}");
        assert_eq!(v.spices_json(), Some("themes"), "{key}");
        assert_eq!(v.effective_source(), ap::SOURCE_SPICES, "{key}");
        let rk = v.request_key();
        assert!(rk.contains(&format!(":spices:themes@{key}:0:")), "{key}: {rk}");
        assert!(v.chrome().search_placeholder.contains("Cinnamon Spices"), "{key}");
        assert_eq!(ap::suggest_action("adapta", true, &v), SuggestAction::Local, "{key}");
    }
    // Разделы Cinnamon: чипа нет (магазин один), подсказки тоже местные.
    let v = CatalogView::new(get_category(home(), "cinnamontheme").unwrap(), "cinnamon");
    assert_eq!(ap::source_keys_for("cinnamontheme"), vec![ap::SOURCE_SPICES]);
    assert!(ap::source_chips_for("cinnamontheme").is_empty());
    assert!(v.spices_mode() && !v.spices_parts_mode());
    assert_eq!(ap::suggest_action("adapta", true, &v), SuggestAction::Local);
    // Где тем Spices быть не может, чипа нет.
    for key in ["icons", "cursor", "lxqttheme", "plasma", "aurorae"] {
        assert!(!ap::source_keys_for(key).contains(&ap::SOURCE_SPICES), "{key}");
    }
}

/// «Панель MATE» — раздел рабочего стола MATE со своим экраном: ни
/// магазина, ни скана, ни шапки каталога; в других окружениях его нет.
#[test]
fn mate_panel_lives_in_the_mate_desktop_group() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    use rexon_core::categories::{category_available, category_group, display_name, SidebarGroup, MATE_PANEL_KEY};
    let _lang = i18n_pin::ru();
    let c = get_category(home(), MATE_PANEL_KEY).unwrap();
    assert!(c.local_only() && !ap::has_installed_grid_for(&c));
    assert!(category_available(MATE_PANEL_KEY, "mate"));
    for de in ["kde", "xfce", "gnome", "cinnamon", "nonesuch"] {
        assert!(!category_available(MATE_PANEL_KEY, de), "{de}");
    }
    assert_eq!(category_group(MATE_PANEL_KEY, "mate"), SidebarGroup::Desktop);
    assert_eq!(display_name(&c, "mate"), "Раскладка панели");
    assert!(categories(home()).iter().all(|c| c.key != MATE_PANEL_KEY));
    let v = CatalogView::new(c, "mate");
    assert_eq!(v.screen(home()), ap::SCREEN_MATE_PANEL);
    assert_eq!(v.chrome(), ap::Chrome::default());
    let targets = ap::prefetch_targets(&v, &sidebar(home()));
    assert!(targets.iter().all(|(id, _)| id != "920"), "{targets:?}");
}

// ============================================================================
// Виджеты на рабочем столе в каждом окружении (24.10.2026)
// ============================================================================

/// В каждом окружении есть группа «Виджеты рабочего стола»: родные виджеты
/// стола первыми (Plasma, десклеты Cinnamon), виджеты Conky — рядом, а где
/// своих нет — одни они. Пояснение группы говорит без жаргона, что это
/// такое, а где на Wayland у стола нет слоёв (GNOME, Cinnamon, Pantheon) —
/// честно, что там виджет Conky — обычное окно.
#[test]
fn every_desktop_has_desktop_widgets_explained() {
    use rexon_core::categories::group_note;
    use rexon_core::de_customization::{de_from_id, De};
    let _lang = i18n_pin::ru();
    let mut checked = 0;
    for m in rexon_core::system::DE_META {
        let Some(de) = de_from_id(m.id) else { continue };
        checked += 1;
        let groups = visible_groups(m.id);
        let widgets = &groups
            .iter()
            .find(|(g, _)| *g == SidebarGroup::Widgets)
            .unwrap_or_else(|| panic!("{}: нет группы виджетов: {groups:?}", m.id))
            .1;
        let expect: &[&str] = match de {
            De::Kde => &["plasmoids", "conky"],
            De::Cinnamon => &["cinnamondesklets", "conky"],
            _ => &["conky"],
        };
        assert_eq!(widgets, expect, "{}", m.id);
        assert_eq!(group_title(SidebarGroup::Widgets, m.id), "Виджеты рабочего стола");
        let note = group_note(SidebarGroup::Widgets, m.id);
        assert!(note.contains("Conky"), "{}: {note}", m.id);
        let no_layers = matches!(de, De::Gnome | De::Cinnamon | De::Pantheon);
        assert_eq!(note.contains("Wayland"), no_layers, "{}: {note}", m.id);
        if !matches!(de, De::Kde | De::Cinnamon) {
            assert!(note.starts_with("Часы, погода и датчики системы"), "{}: {note}", m.id);
        }
        if de == De::Cinnamon {
            assert!(note.starts_with("Десклеты — родные виджеты Cinnamon"), "{note}");
        }
    }
    assert!(checked >= 10, "окружений проверено мало: {checked}");
    // Неизвестное окружение видит все разделы виджетов — пояснять нечем.
    assert!(group_note(SidebarGroup::Widgets, "nonesuch").is_empty());
}

/// Cinnamon на Wayland: заголовок раздела Conky показывает дорогу к
/// десклетам — они держатся на столе в любом сеансе.
#[test]
fn conky_section_in_cinnamon_wayland_points_to_desklets() {
    use rexon_core::de_apply::conky_section_note;
    let _lang = i18n_pin::ru();
    let cin = conky_section_note(true, true, "cinnamon", true).unwrap();
    assert!(cin.contains("в соседнем разделе «Десклеты Cinnamon»"), "{cin}");
    assert!(conky_section_note(true, true, "cinnamon", false).is_none());
    let gnome = conky_section_note(true, true, "gnome", true).unwrap();
    assert!(!gnome.contains("Десклеты"), "{gnome}");
    assert!(conky_section_note(true, true, "sway", true).is_none());
}

/// Общий пункт «Тайловые и Wayfire» в выборе окружения «Оформления»: вне
/// тайлового сеанса под ним едет Hyprland (первый в секции «Тайловые»), а
/// не Wayfire, который стоит раньше в базе столов; в тайловом сеансе — он
/// сам. Пункт один.
#[test]
fn appearance_tiling_choice_defaults_to_hyprland() {
    // Язык — общий на процесс, а сравниваются ответы разных вызовов: без
    // закрепления соседняя сверка переключает язык между ними.
    let _lang = i18n_pin::en();
    let label = rexon_core::de_customization::De::Wlroots.display_label();
    let tiling_of = |current: &str| -> Vec<String> {
        rexon_core::categories::appearance_de_choices(current)
            .into_iter()
            .filter(|(_, l)| *l == label)
            .map(|(id, _)| id)
            .collect()
    };
    assert_eq!(tiling_of("mate"), vec!["hyprland".to_string()]);
    assert_eq!(tiling_of(""), vec!["hyprland".to_string()]);
    assert_eq!(tiling_of("sway"), vec!["sway".to_string()]);
    assert_eq!(tiling_of("wayfire"), vec!["wayfire".to_string()]);
}

// ============================================================================
// Разделы настроек в «Оформлении», как в KDE 6.8 (01.11.2026)
// ============================================================================

/// Анимация, шрифты, кнопки заголовка, отклик при запуске, значки и панели
/// инструментов, эффекты рабочего стола переехали из «Настроек» в боковую
/// панель «Оформления» — туда, где их держит «Внешний вид и стиль» KDE 6.8
/// (`kcm_kwindecoration` → кнопки, `kcm_cursortheme` → отклик,
/// `kcm_style` → значки и панели, `kcm_fonts`/`kcm_fontinst` → шрифты,
/// `kcm_animations` — последним). Порядок сверяется поимённо.
#[test]
fn settings_panels_stand_where_kde_keeps_them() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    use rexon_core::categories::{is_settings_panel, SETTINGS_PANELS};
    let _lang = i18n_pin::ru();
    let groups = visible_groups("kde");
    let keys = |g: SidebarGroup| groups.iter().find(|(x, _)| *x == g).unwrap().1.clone();
    assert_eq!(
        keys(SidebarGroup::Desktop),
        [
            "global",
            "plasma",
            "colors",
            "aurorae",
            "windowbuttons",
            "widgetstyle",
            "wallpaper_plugins",
            "splash",
            "lockscreen",
            "tabbox",
            "kwinscripts",
            "kwineffects",
            "animations",
            "desktopeffects"
        ]
    );
    assert_eq!(
        keys(SidebarGroup::AllApps),
        ["icons", "cursor", "launchfeedback", "gtk", "appwidgets", "wallpapers", "fontsettings", "fonts", "sounds"]
    );
    // «Обзор» — по-прежнему первым.
    assert_eq!(groups[0].0, SidebarGroup::Overview);
    // XFCE: кнопки — за рамками XFWM4.
    let xfce = visible_groups("xfce");
    let desk = &xfce.iter().find(|(g, _)| *g == SidebarGroup::Desktop).unwrap().1;
    let at = desk.iter().position(|k| k == "xfwm4").unwrap();
    assert_eq!(desk[at + 1], "windowbuttons", "{desk:?}");
    // Где виден каждый: эффекты KWin — только KDE, значки и панели — везде,
    // кроме Deepin; прочие — везде.
    for de in ["kde", "gnome", "mate", "xfce", "cinnamon", "hyprland", "deepin", "nonesuch"] {
        let known = rexon_core::de_customization::de_from_id(de).is_some();
        assert_eq!(category_available("desktopeffects", de), de == "kde" || !known, "{de}");
        assert_eq!(category_available("appwidgets", de), de != "deepin", "{de}");
        for k in ["windowbuttons", "launchfeedback", "fontsettings", "animations"] {
            assert!(category_available(k, de), "{k} в {de}");
        }
    }
    // Подписи — заголовки самих разделов; магазина, скана и шапки нет,
    // экран — свой.
    let names: Vec<(String, String)> = SETTINGS_PANELS
        .iter()
        .map(|(_, k)| (k.to_string(), get_category(home(), k).unwrap().name))
        .collect();
    assert_eq!(
        names,
        [
            ("windowbuttons", "Кнопки заголовка окна"),
            ("launchfeedback", "Отклик при запуске"),
            ("appwidgets", "Значки и панели инструментов в программах"),
            ("fontsettings", "Системные шрифты"),
            ("animations", "Анимация"),
            ("desktopeffects", "Эффекты рабочего стола"),
        ]
        .map(|(k, n)| (k.to_string(), n.to_string()))
    );
    for (id, k) in SETTINGS_PANELS {
        assert!(*id > 900 && is_settings_panel(k));
        let c = get_category(home(), k).unwrap();
        assert!(c.local_only() && !ap::has_installed_grid_for(&c), "{k}");
        assert!(categories(home()).iter().all(|x| x.key != *k), "{k} — не каталог");
        let v = CatalogView::new(c, "kde");
        assert_eq!(v.screen(home()), ap::SCREEN_SETTINGS, "{k}");
        assert_eq!(v.chrome(), ap::Chrome::default(), "{k}");
        let targets = ap::prefetch_targets(&v, &sidebar(home()));
        assert!(targets.iter().all(|(t, _)| *t != id.to_string()), "{k}: {targets:?}");
    }
}

/// Во вкладке «Настройки» на месте переехавших разделов — ссылки
/// «Оформление → …» в порядке боковой панели и только видимые в этом столе.
#[test]
fn settings_tab_links_to_moved_panels() {
    use rexon_core::categories::moved_settings_links;
    let _lang = i18n_pin::ru();
    let keys = |de: &str| moved_settings_links(de).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
    assert_eq!(
        keys("kde"),
        ["windowbuttons", "animations", "desktopeffects", "launchfeedback", "appwidgets", "fontsettings"]
    );
    assert_eq!(keys("mate"), ["windowbuttons", "animations", "launchfeedback", "appwidgets", "fontsettings"]);
    assert!(!keys("deepin").contains(&"appwidgets".to_string()));
    let links = moved_settings_links("kde");
    assert_eq!(links[1].1, "Оформление → Анимация");
}
