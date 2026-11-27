//! Golden check of the categories port against the original `rexon/appearance/categories.py`.
//!
//! The golden file `golden/categories_all.json` was captured from Python itself with
//! `HOME=/home/tester`: `json.dumps({"categories": [asdict(c) for c in
//! CATEGORIES.values()], "sidebar": [asdict(c) for c in SIDEBAR]},
//! ensure_ascii=False, indent=1)`. Any mismatch (id/name/icon/path/
//! apply_kind/flags/catalog/order) is a transcription error in the table.
//!
//! Deliberate edits of the golden file: «Виджеты» 706 → 705 (04.10.2026, the
//! «Plasma 6 Extensions» umbrella instead of applets alone — as in Discover); the pair of
//! fields `local_only` + `store_cats` was replaced with one catalog field `store`
//! (17.10.2026: a section's catalog is given explicitly, not derived from the number).

use std::path::Path;

use rexon_core::categories::{
    categories, category_by_id, category_by_kind, get_category, is_de_section, sidebar,
    AGGREGATE_CATS, DE_SECTIONS, LOCAL_VIEW_KEYS,
};
use rexon_core::models::to_json_indent1;

mod i18n_pin;

/// The same fixed home as in the golden capture script.
fn home() -> &'static Path {
    Path::new("/home/tester")
}

/// The full category table + sidebar serialize byte for byte as
/// Python's `asdict` dump (checks the whole transcription and the order).
///
/// The golden file was captured from the Russian interface, so the check runs on the Russian
/// catalog `po/ru.po`: at the same time it keeps the promise that the section names
/// in Russian stayed the same.
#[test]
fn categories_and_sidebar_byte_identical() {
    let _lang = i18n_pin::ru();
    let golden = include_str!("golden/categories_all.json");
    // The desktop sections (04.10.2026), «Панель MATE» (17.10.2026) and the settings
    // sections (01.11.2026) were unknown to the Python golden file — everything
    // else is checked; the sections themselves are held by the checks below.
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

/// In English — the source section names.
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

/// The number of categories and sidebar entries — as in Python (21 real, 23 in the sidebar)
/// plus the desktop sections: each both in the table and in the sidebar; plus
/// «Панель MATE» — only in the sidebar (nothing can be installed into it).
#[test]
fn counts_match_python() {
    assert_eq!(categories(home()).len(), 21 + DE_SECTIONS.len());
    assert_eq!(
        sidebar(home()).len(),
        23 + DE_SECTIONS.len() + 1 + rexon_core::categories::SETTINGS_PANELS.len()
    );
}

/// Desktop sections: each has its own type, a button from `de_apply`, a catalog from
/// the matrix, and it is visible only where the type applies.
#[test]
fn de_sections_follow_the_matrix() {
    let _lang = i18n_pin::en();
    for (id, key) in DE_SECTIONS {
        let c = category_by_id(home(), *id).unwrap();
        assert_eq!(c.key, *key);
        // The catalog comes from the section's field, not from the number.
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
    // Conky is a separate program: it works everywhere.
    for de in ["kde", "gnome", "xfce", "mate", "cinnamon", "lxqt", "sway"] {
        assert!(shown("conky", de), "conky в {de}");
    }
    assert_eq!(get_category(home(), "xfwm4").unwrap().name, "Window Borders (XFCE)");
}

/// `category_by_id` — like `CATEGORIES.get`: finds the real one by cat_id.
/// For duplicated keys (splash: 902/716/488) the ids are unique and resolve
/// to their own entry.
#[test]
fn category_by_id_resolves() {
    let c = category_by_id(home(), 722).unwrap();
    assert_eq!(c.key, "global");
    assert_eq!(c.apply_kind, "global");
    assert_eq!(c.install_dir, "/home/tester/.local/share/plasma/look-and-feel");
    assert!(category_by_id(home(), 999999).is_none());
    // The splash "umbrella" 902 and the real 716/488 — different entries with a shared key.
    assert_eq!(category_by_id(home(), 902).unwrap().ocs_cats(), [716, 488]);
    assert_eq!(category_by_id(home(), 716).unwrap().ocs_cats(), [716]);
}

/// Sections with the KDE store are the same categories as in Discover (knsrc):
/// «Виджеты» is the umbrella 705, KWin — 719/720/721. An item arrives with the number
/// of its subcategory, and that number must lead to the section: the install
/// directory, the view and the button depend on it.
#[test]
fn store_sections_and_item_type_ids() {
    let widgets = category_by_id(home(), 705).unwrap();
    assert_eq!(widgets.key, "plasmoids");
    assert!(!widgets.local_only() && widgets.ocs_cats() == [705]);
    assert_eq!(widgets.install_dir, "/home/tester/.local/share/plasma/plasmoids");
    // Applets, calendars, clocks, menus, monitoring, multimedia, weather.
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
    // The former local numbers (before 04.10.2026) lead to the same place.
    assert_eq!(category_by_id(home(), 911).unwrap().key, "tabbox");
    assert_eq!(category_by_id(home(), 912).unwrap().key, "kwinscripts");
    assert_eq!(category_by_id(home(), 913).unwrap().key, "kwineffects");
    // «Расширения GNOME» — the extensions.gnome.org storefront (13.11.2026).
    let ego = category_by_id(home(), 914).unwrap();
    assert_eq!(ego.store, rexon_core::models::CatalogSource::GnomeExtensions);
    assert!(!ego.local_only() && ego.ocs_cats().is_empty());
    // Visible only in KDE — the matrix decides that.
    let shown = rexon_core::categories::category_available;
    assert!(shown("kwineffects", "kde") && !shown("kwineffects", "xfce"));
    assert!(shown("plasmoids", "kde"));
}

/// `category_by_kind` — the first in `CATEGORIES` declaration order with this
/// apply_kind (for splash it is the "umbrella" 902, declared before 716/488).
#[test]
fn category_by_kind_first_declared() {
    assert_eq!(category_by_kind(home(), "icons").unwrap().id, 132);
    assert_eq!(category_by_kind(home(), "splash").unwrap().id, 902);
    assert!(category_by_kind(home(), "no_such_kind").is_none());
}

/// `get_category` walks the SIDEBAR (including virtual ones) and takes the first with
/// the key.
#[test]
fn get_category_from_sidebar() {
    assert!(get_category(home(), "active").unwrap().virtual_);
    assert_eq!(get_category(home(), "popular").unwrap().default_sort, "high");
    let global = get_category(home(), "global").unwrap();
    assert!(!global.virtual_);
    assert_eq!(global.id, 722);
    assert!(get_category(home(), "missing").is_none());
}

/// The constants of the overview keys and the aggregate match Python.
#[test]
fn constants_match() {
    assert_eq!(LOCAL_VIEW_KEYS, ["active", "downloaded", "system", "favorites"]);
    assert_eq!(AGGREGATE_CATS, [722, 104, 112, 132, 107]);
}

// ============================================================================
// Catalog filter by desktop
// ============================================================================

/// Every sidebar section must be either bound to a matrix type
/// or deliberately "always visible" (overviews of what is installed and storefronts). Forgetting
/// a new section is impossible — the test lists the exceptions by name.
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
        // «Панель MATE» is not a decoration type but mate-panel layouts: where it is
        // visible is decided by its own rule (`category_available`, MATE only).
        if c.key == rexon_core::categories::MATE_PANEL_KEY {
            assert!(category_kind(&c.key).is_none());
            continue;
        }
        // Settings sections are not a decoration type: where they are visible is decided by their
        // rule (`category_available`, by the «Настройки» section).
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

/// The buyer is not shown what does not work in their desktop; overviews and
/// storefronts are always visible.
#[test]
fn sections_filtered_by_desktop() {
    use rexon_core::categories::category_available;

    // KDE: all of its own is in place, and GTK themes too (Firefox, GIMP, kde-gtk-config).
    for key in ["plasmoids", "global", "aurorae", "gtk", "wallpapers", "widgetstyle"] {
        assert!(category_available(key, "kde"), "{key} пропал в KDE");
    }
    // GNOME: nothing KDE-specific, the common things are there.
    for key in ["plasmoids", "aurorae", "plasma"] {
        assert!(!category_available(key, "gnome"), "{key} не должен быть в GNOME");
    }
    for key in [
        "gtk", "icons", "cursor", "wallpapers", "fonts", "sounds", "plymouth",
        // Kvantum is a Qt style engine: VLC, qBittorrent,
        // Krita, OBS live under GNOME, and it paints them just the same.
        "widgetstyle", "kvantum",
        // KDE color schemes outside Plasma (f83e0bff): kdeglobals for KDE
        // programs and the qt6ct palette for Qt — the section is deliberately open in GNOME too.
        "colors",
        // Global themes outside Plasma (01.11.2026) — in parts, the same
        // way as «Как в дистрибутиве»: colors, application style, icons,
        // cursors, fonts, titlebar buttons (`themes::apply_global_choice_plan`).
        "global",
    ] {
        assert!(category_available(key, "gnome"), "{key} пропал в GNOME");
    }
    // Tiling ones: wallpapers, fonts, icons, sounds (played by the notification daemon,
    // not the shell) — yes; plasmoids — no.
    for de in ["sway", "hyprland", "niri"] {
        assert!(category_available("wallpapers", de));
        assert!(category_available("fonts", de));
        assert!(category_available("sounds", de), "звуки пропали в {de}");
        assert!(!category_available("plasmoids", de));
    }
    // Qt applications are everywhere — so the styles section is everywhere too.
    for de in ["lxqt", "xfce", "cosmic", "sway"] {
        assert!(category_available("widgetstyle", de), "стили Qt пропали в {de}");
    }
    // Overviews and storefronts — always.
    for key in ["active", "downloaded", "system", "favorites", "popular", "new"] {
        assert!(category_available(key, "gnome"));
    }
    // An unfamiliar desktop — we hide nothing.
    assert!(category_available("plasmoids", "какой-то-новый-wm"));
}

/// An empty screen without an explanation is unacceptable: a hidden section must have
/// human text, an available one must have none.
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

/// A section that works under a condition is shown — but the condition is named.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn conditional_section_names_its_condition() {
    use rexon_core::categories::{category_available, category_condition};

    // Sounds in tiling ones: the section is in place, with a caveat about the notification daemon.
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

    // Kvantum outside KDE/LXQt: the section is in place, with a caveat about logging in again.
    for key in ["kvantum", "widgetstyle"] {
        let note = category_condition(key, "gnome").expect("оговорка");
        assert!(note.contains("вход"), "нет условия про вход: {note}");
        assert!(category_condition(key, "kde").is_none());
        assert!(category_condition(key, "lxqt").is_none());
        // D1: where session variables cannot reach the compositor (sway,
        // river, Wayfire), the caveat says so directly; Hyprland, niri and
        // XFCE get them — we write nothing extra there.
        let sway = category_condition(key, "sway").expect("оговорка sway");
        assert!(sway.starts_with(&note) && sway.contains("sway") && sway.contains("uwsm"), "{sway}");
        // With Plasma installed the variables are not in environment.d: uwsm
        // would not help, and the file to copy from is Rexon's own.
        let bound = rexon_core::categories::category_condition_on(key, "sway", true).expect("оговорка sway");
        assert!(bound.starts_with(&note) && !bound.contains("uwsm"), "{bound}");
        assert!(bound.contains("~/.config/rexon/session-env/rexon-qt-style.conf"), "{bound}");
        assert_eq!(rexon_core::categories::category_condition_on(key, "gnome", true).as_deref(), Some(note.as_str()));
        for de in ["hyprland", "niri", "xfce", "mate"] {
            assert!(!category_condition(key, de).unwrap().contains("uwsm"), "{de}");
        }
    }
    // Disk overviews and unfamiliar desktops — without conditions.
    assert!(category_condition("active", "gnome").is_none());
    assert!(category_condition("sounds", "какой-то-новый-wm").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
//  Login screen: SDDM, Plasma Login Manager, SteamOS (28.09.2026)
// ═══════════════════════════════════════════════════════════════════════

/// With SDDM the sidebar is exactly as before; with Plasma Login Manager its section stands
/// in place of «SDDM (экран входа)», the rest has not moved.
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
    // Without a network and without warm-up: the number is outside the store, the section is local.
    assert!(c.local_only() && c.id > 900 && !c.requires_root);
    // The section is found by key, but it did not get into the items table.
    assert_eq!(get_category(home(), PLASMA_LOGIN_KEY).unwrap().id, c.id);
    assert!(categories(home()).iter().all(|x| x.key != PLASMA_LOGIN_KEY));
    assert!(category_by_kind(home(), "plasmalogin").is_none());
    // Its desktop matrix is the same as the SDDM shelf's.
    assert_eq!(
        rexon_core::categories::category_kind(PLASMA_LOGIN_KEY),
        rexon_core::categories::category_kind("sddm")
    );

    let _ru = {
        drop(_lang);
        i18n_pin::ru()
    };
    assert_eq!(plasma_login_category().name, "Экран входа (Plasma Login Manager)");
    // The SDDM section name in Russian is the same as before.
    assert_eq!(get_category(home(), "sddm").unwrap().name, "SDDM (вход)");
}

/// SteamOS: the login screen and the boot splash are closed IN ADVANCE — the section is in
/// place and says why right away; other sections are not affected.
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
    // Not SteamOS — nothing is closed.
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
    // The texts are the same as the installer's refusal: one reason per product.
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
// Sidebar groups and per-desktop captions (16.10.2026)
// ============================================================================

use rexon_core::categories::{
    category_available, category_group, display_name, group_title, grouped_sidebar,
    system_section_note, SidebarGroup,
};

/// Sections visible in a desktop, by group, in screen order.
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
    // Global themes first, as in KDE (outside Plasma — in parts,
    // 01.11.2026). Titlebar buttons are in Marco's «⚙️ Настройки», there is no «Анимации»
    // in MATE (08.11.2026): Marco has no window animations, the checkbox for
    // programs is in «GTK-темы → ⚙️ Настройки».
    assert_eq!(keys(SidebarGroup::Desktop), ["global", "marco", "mate-panel", "screeneffects"]);
    assert_eq!(
        keys(SidebarGroup::AllApps),
        ["icons", "cursor", "gtk", "wallpapers", "fonts", "sounds"]
    );
    assert_eq!(keys(SidebarGroup::QtApps), ["colors", "widgetstyle"]);
    assert_eq!(keys(SidebarGroup::Widgets), ["conky"]);
    assert_eq!(keys(SidebarGroup::System), ["sddm", "plymouth"]);
    // Each group comes in one piece: grouped_sidebar does not split groups.
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
    // An unknown desktop sees both frames — the annotations stay.
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
    // The SDDM themes shelf with LightDM — warn in advance.
    let light = system_section_note("sddm", "lightdm").unwrap();
    assert!(light.contains("LightDM") && light.contains("SDDM"), "{light}");
    assert!(system_section_note("plymouth", "").unwrap().contains("до входа"));
    assert!(system_section_note("icons", "sddm").is_none());
}

/// Guard of the common "installed + «Загрузить новые…»" view: EVERY
/// sidebar section that has both a disk scan and a store has
/// a grid of installed items and a store button — without a list of sections. Conky and
/// the KWin sections until 17.10.2026 showed only the store, although they had a scan.
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
        // An unknown desktop sees all sections — all are checked at once.
        let v = CatalogView::new(c.clone(), "nonesuch");
        assert_eq!(v.has_installed_view(), scan && store, "{}", c.key);
        if scan && store {
            // Installed on top, the section's store right below it — without
            // the «Загрузить новые…» button (owner's decision 18.10.2026).
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
    // A store section without a scan (wallpapers, Plasma widgets) — still a store.
    assert!(!ap::has_installed_grid("wallpapers") && !ap::has_installed_grid("plasmoids"));
}

/// A section's catalog is the `store` field, not the number: the query and the warm-up take
/// the numbers from it, and the «Магазин:» switch exists only where AUR
/// has something to show.
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
    // AUR has no window switcher packages and Conky sets — there is no «AUR» chip.
    for k in ["tabbox", "conky", "splash"] {
        let v = open_store(k);
        assert!(!v.is_local() && !v.source_applicable(), "{k}");
        assert_eq!(ap::source_keys_for(k), ["ocs"], "{k}");
    }
    assert!(open_store("kwinscripts").source_applicable());
}

/// Desktop sections with a pling catalog: installed on top, the store
/// below it on the same page, an item with the store's category number leads to
/// its section (the install view and the button come from there), the warm-up asks for the same
/// category as the live query.
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

/// The five Cinnamon sections — the Cinnamon Spices store (as in the Cinnamon settings
/// window): installed on top and the section's store right below it, without
/// the «Загрузить новые…» button (owner's decision 18.10.2026); their own query
/// key, search, sorting and pages; there is no KDE Store and AUR there. The former
/// theme catalog number (cinnamon-look, 133) leads to the Cinnamon themes section.
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
        // Choosing «AUR» from another section has no effect here.
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
    // The other store sections have not lost their sources.
    assert_eq!(ap::source_keys_for("icons")[0], ap::SOURCE_OCS);
    assert_eq!(CatalogView::new(get_category(home(), "icons").unwrap(), "kde").effective_source(), ap::SOURCE_OCS);
}

/// Cinnamon Spices themes outside Cinnamon: in «Темы GTK» (in all desktops), in
/// the XFCE and MATE window frames and in the GNOME Shell theme — a «Cinnamon Spices» chip
/// next to KDE Store.
/// When chosen — the site's themes storefront of its section (each section has its own
/// query key: the selection by contents differs), search without KDE Store suggestions.
/// In the Cinnamon sections everything is as it was: Spices is the only store.
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
    // Cinnamon sections: no chip (one store), the suggestions are local too.
    let v = CatalogView::new(get_category(home(), "cinnamontheme").unwrap(), "cinnamon");
    assert_eq!(ap::source_keys_for("cinnamontheme"), vec![ap::SOURCE_SPICES]);
    assert!(ap::source_chips_for("cinnamontheme").is_empty());
    assert!(v.spices_mode() && !v.spices_parts_mode());
    assert_eq!(ap::suggest_action("adapta", true, &v), SuggestAction::Local);
    // Where there can be no Spices themes, there is no chip.
    for key in ["icons", "cursor", "lxqttheme", "plasma", "aurorae"] {
        assert!(!ap::source_keys_for(key).contains(&ap::SOURCE_SPICES), "{key}");
    }
}

/// «Панель MATE» is a MATE desktop section with its own screen: no
/// store, no scan, no catalog header; it does not exist in other desktops.
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
// Desktop widgets in every desktop (24.10.2026)
// ============================================================================

/// Every desktop has the «Виджеты рабочего стола» group: the desktop's native widgets
/// first (Plasma, Cinnamon desklets), Conky widgets next to them, and where
/// there are no native ones — Conky alone. The group's explanation says without jargon what it
/// is; on desktops without Wayland layers (GNOME, Cinnamon, Pantheon) conky goes
/// through XWayland, so there is no «обычное окно» caveat here.
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
        // Through XWayland the Conky widget stays on the desktop even where
        // Wayland has no layers — nobody has a caveat in the group's explanation.
        assert!(!note.contains("Wayland"), "{}: {note}", m.id);
        if !matches!(de, De::Kde | De::Cinnamon) {
            assert!(note.starts_with("Часы, погода и датчики системы"), "{}: {note}", m.id);
        }
        if de == De::Cinnamon {
            assert!(note.starts_with("Десклеты — родные виджеты Cinnamon"), "{note}");
        }
    }
    assert!(checked >= 10, "окружений проверено мало: {checked}");
    // An unknown desktop sees all widget sections — nothing to explain with.
    assert!(group_note(SidebarGroup::Widgets, "nonesuch").is_empty());
}

/// Cinnamon on Wayland: the Conky section header shows the way to
/// desklets — they stay on the desktop in any session.
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

/// The common item «Тайловые и Wayfire» in the desktop choice of «Оформление»: outside
/// a tiling session Hyprland (the first in the «Тайловые» section) goes under it, not
/// Wayfire, which comes earlier in the desktops database; in a tiling session — the session's
/// own one. There is one item.
#[test]
fn appearance_tiling_choice_defaults_to_hyprland() {
    // The language is common to the process, while the answers of different calls are compared: without
    // pinning, a neighbouring check switches the language between them.
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
// Settings sections in «Оформление», as in KDE 6.8 (01.11.2026)
// ============================================================================

/// Animation, fonts, titlebar buttons, launch feedback, icons and toolbars,
/// desktop effects moved from «Настройки» to the sidebar
/// of «Оформление» — where KDE 6.8 «Внешний вид и стиль» keeps them
/// (`kcm_kwindecoration` → buttons, `kcm_cursortheme` → feedback,
/// `kcm_style` → icons and toolbars, `kcm_fonts`/`kcm_fontinst` → fonts,
/// `kcm_animations` — last). The order is checked by name.
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
            "widgetstyle",
            "wallpaper_plugins",
            "splash",
            "lockscreen",
            "tabbox",
            "kwinscripts",
            "kwineffects",
            "animations",
            "screeneffects"
        ]
    );
    assert_eq!(
        keys(SidebarGroup::AllApps),
        ["icons", "cursor", "gtk", "wallpapers", "fonts", "sounds"]
    );
    // «Обзор» is still first.
    assert_eq!(groups[0].0, SidebarGroup::Overview);
    // Where each is visible: KWin effects — only KDE, icons and toolbars — everywhere
    // except Deepin; the rest — everywhere.
    for de in ["kde", "gnome", "mate", "xfce", "cinnamon", "hyprland", "deepin", "nonesuch"] {
        let known = rexon_core::de_customization::de_from_id(de).is_some();
        // «Эффекты рабочего стола» is the «⚙️ Настройки» view of «Эффекты KWin»
        // (08.11.2026): it has no row of its own in any desktop.
        assert!(get_category(home(), "desktopeffects").is_none(), "{de}");
        let _ = known;
        // «Анимация» — where the window manager has window animations.
        assert_eq!(category_available("animations", de), ["kde", "gnome", "cinnamon", "hyprland", "nonesuch"].contains(&de), "{de}");
        // Screen effects — where they can be enabled in at least one session kind.
        assert_eq!(category_available("screeneffects", de), de != "deepin", "{de}");
    }
    // Captions are the headings of the sections themselves; no store, scan or header,
    // the screen is their own.
    let names: Vec<(String, String)> = SETTINGS_PANELS
        .iter()
        .map(|(_, k)| (k.to_string(), get_category(home(), k).unwrap().name))
        .collect();
    assert_eq!(
        names,
        [
            ("animations", "Анимация"),
            ("screeneffects", "Эффекты экрана"),
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

/// A catalog and settings of one kind live in one section (08.11.2026), like
/// the tabs of KDE modules: for «Шрифты», «Значки», «Курсоры», window frames and
/// «GTK-темы» the settings open with the «⚙️ Настройки» button in the header, and these settings
/// have no rows of their own in the sidebar in any desktop. A setting
/// the desktop does not have is not in the view either. One mechanism for all pairs.
#[test]
fn section_settings_open_inside_catalog_section() {
    use rexon_core::appearance_page::{self as ap, CatalogView};
    use rexon_core::categories::{
        category_available, get_category, grouped_sidebar, section_settings_panels, ANIMATIONS_KEY,
        APP_WIDGETS_PANEL, DESKTOP_EFFECTS_PANEL, FONT_SETTINGS_PANEL, LAUNCH_FEEDBACK_PANEL, SECTION_SETTINGS, WALLPAPER_BLUR_PANEL, AMBIENT_SOUNDS_PANEL, EVENT_SOUNDS_PANEL, WINDOW_BUTTONS_KEY,
        AXON_SETTINGS_PANEL, WALLPAPER_PLAYLIST_PANEL,
    };
    let _lang = i18n_pin::ru();
    let des = ["kde", "gnome", "mate", "xfce", "cinnamon-mint", "lxqt", "labwc", "budgie", "pantheon", "cosmic",
               "hyprland", "sway", "deepin", "nonesuch"];
    for de in des {
        let side: Vec<String> = grouped_sidebar(&sidebar(home()), de).into_iter().map(|c| c.key).collect();
        for (_, panel) in SECTION_SETTINGS {
            if *panel != ANIMATIONS_KEY {
                assert!(!side.iter().any(|k| k == panel), "{de}: {panel} в боковой панели");
                assert!(get_category(home(), panel).is_none(), "{panel} — не раздел");
            }
        }
        let p = |cat: &str| section_settings_panels(cat, de);
        assert_eq!(p("fonts"), [FONT_SETTINGS_PANEL], "{de}");
        // Wallpaper blur is in «Обои HD» (it left the «Настройки» tab);
        // next to it is the «Razer Axon» block (the catalog view selects it by whether
        // Axon is found).
        // The wallpaper playlist comes first: it is searched for most often.
        assert_eq!(p("wallpapers"), [WALLPAPER_PLAYLIST_PANEL, WALLPAPER_BLUR_PANEL, AXON_SETTINGS_PANEL], "{de}");
        // Event sounds and background sounds are in «Звуки».
        assert_eq!(p("sounds"), [EVENT_SOUNDS_PANEL, AMBIENT_SOUNDS_PANEL], "{de}");
        // Icons and toolbars in Deepin are set by its Control Center.
        assert_eq!(p("icons"), if de == "deepin" { vec![] } else { vec![APP_WIDGETS_PANEL] }, "{de}");
        // Launch feedback exists only in KDE: where it does not exist, there is no button either
        // (an empty page with «в MATE нет настройки…» is a dead end).
        assert_eq!(p("cursor"), if de == "kde" { vec![LAUNCH_FEEDBACK_PANEL] } else { vec![] }, "{de}");
        // Enabling KWin effects — with a button in «Эффекты KWin» (08.11.2026);
        // only KWin has effects, for an unfamiliar desktop the button is visible.
        let kde_like = de == "kde" || de == "nonesuch";
        assert_eq!(p("kwineffects"), if kde_like { vec![DESKTOP_EFFECTS_PANEL] } else { vec![] }, "{de}");
        // Titlebar buttons are in exactly one visible section of the desktop: with
        // window frames, and without them — in «GTK-темы» (the feature is not lost).
        // For an unfamiliar desktop all frame sections are visible — buttons in each.
        let with_buttons: Vec<&str> = ["aurorae", "xfwm4", "marco", "cinnamontheme", "openbox", "gtk"]
            .into_iter()
            .filter(|c| category_available(c, de) && p(c).contains(&WINDOW_BUTTONS_KEY))
            .collect();
        let expect: &[&str] = match de {
            "kde" => &["aurorae"],
            "xfce" => &["xfwm4"],
            "mate" => &["marco"],
            "cinnamon-mint" => &["cinnamontheme"],
            "lxqt" | "labwc" => &["openbox"],
            "nonesuch" => &["aurorae", "xfwm4", "marco", "cinnamontheme", "openbox"],
            _ => &["gtk"],
        };
        assert_eq!(with_buttons, expect, "{de}");
        // The «Без анимаций» checkbox is in «GTK-темы» exactly where there is no own
        // «Анимация» row; for an unfamiliar desktop the «Анимация» row is visible.
        let gtk_anim = p("gtk").contains(&ANIMATIONS_KEY);
        assert_eq!(gtk_anim, !category_available(ANIMATIONS_KEY, de), "{de}");
    }
    assert_eq!(section_settings_panels("gtk", "sway"), [WINDOW_BUTTONS_KEY, ANIMATIONS_KEY]);
    assert!(section_settings_panels("plymouth", "kde").is_empty());
    for (cat, de, panels) in [
        ("fonts", "kde", "fontsettings"),
        ("icons", "kde", "appwidgets"),
        ("cursor", "kde", "launchfeedback"),
        ("wallpapers", "kde", "wallpaperplaylist,wallpaperblur"),
        ("sounds", "mate", "eventsounds,ambientsounds"),
        ("aurorae", "kde", "windowbuttons"),
        ("kwineffects", "kde", "desktopeffects"),
        ("marco", "mate", "windowbuttons"),
        ("gtk", "mate", "animations"),
        ("gtk", "gnome", "windowbuttons"),
        ("gtk", "deepin", "windowbuttons,animations"),
    ] {
        let mut v = CatalogView::new(get_category(home(), cat).unwrap(), de);
        // Catalog first: the button is there, the screen is tiles, the header is the catalog's.
        assert!(v.has_section_settings(), "{cat}/{de}");
        assert!(!v.section_settings_open());
        assert_eq!(v.screen(home()), ap::SCREEN_GRID, "{cat}/{de}");
        assert_eq!(v.settings_panel_key(), cat);
        assert!(v.chrome().search, "{cat}/{de}");
        // «⚙️ Настройки»: the same sections of the «Настройки» tab, without search and
        // store.
        v.section_settings = true;
        assert!(v.section_settings_open());
        assert_eq!(v.screen(home()), ap::SCREEN_SETTINGS, "{cat}/{de}");
        assert_eq!(v.settings_panel_key(), panels);
        assert_eq!(v.chrome(), ap::Chrome::default(), "{cat}/{de}");
    }
    // A foreign section does not open the settings view, even if the flag remained.
    let mut v = CatalogView::new(get_category(home(), "plymouth").unwrap(), "kde");
    v.section_settings = true;
    assert_eq!(v.screen(home()), ap::SCREEN_GRID);
    assert_eq!(v.settings_panel_key(), "plymouth");
    // In MATE «Курсоры» has no button (there is no launch feedback there).
    let mut v = CatalogView::new(get_category(home(), "cursor").unwrap(), "mate");
    v.section_settings = true;
    assert!(!v.has_section_settings());
    assert_eq!(v.screen(home()), ap::SCREEN_GRID);
    assert_eq!(ap::section_settings_btn(), "⚙️ Настройки");
    assert_eq!(ap::detail_back_btn(), "←  Назад к каталогу");
    assert_eq!(
        ap::section_settings_tooltip(&["windowbuttons", "animations"]).lines().count(),
        2,
        "по строке подсказки на раздел"
    );
    // The «Razer Axon» block — only when Axon is found, together with its chip.
    let mut v = CatalogView::new(get_category(home(), "wallpapers").unwrap(), "kde");
    assert_eq!(v.section_settings_panels(), [WALLPAPER_PLAYLIST_PANEL, WALLPAPER_BLUR_PANEL]);
    v.axon = true;
    assert_eq!(v.section_settings_panels(), [WALLPAPER_PLAYLIST_PANEL, WALLPAPER_BLUR_PANEL, AXON_SETTINGS_PANEL]);
    v.section_settings = true;
    assert_eq!(v.settings_panel_key(), "wallpaperplaylist,wallpaperblur,axonsettings");
    assert!(!ap::section_settings_tooltip(&[WALLPAPER_PLAYLIST_PANEL]).is_empty());
    assert!(!ap::section_settings_tooltip(&[AXON_SETTINGS_PANEL]).is_empty());
}

/// The session kind of the chosen desktop (08.11.2026): what is impossible in it is not visible.
/// The MATE, Xfce and LXQt screen effects work only on X11 (picom) — on
/// Wayland there is no section; "session unknown" — visible if it works in at least
/// one kind.
#[test]
fn sections_follow_session_kind() {
    use rexon_core::appearance_page::CatalogView;
    use rexon_core::categories::{category_available_in, grouped_sidebar, Session, SCREEN_EFFECTS_KEY};
    for de in ["mate", "xfce", "lxqt"] {
        assert!(category_available_in(SCREEN_EFFECTS_KEY, de, Session::X11), "{de}");
        assert!(!category_available_in(SCREEN_EFFECTS_KEY, de, Session::Wayland), "{de}");
        assert!(category_available_in(SCREEN_EFFECTS_KEY, de, Session::Any), "{de}");
        let side = |s: Session| {
            grouped_sidebar(&sidebar(home()), de)
                .into_iter()
                .filter(|c| category_available_in(&c.key, de, s))
                .any(|c| c.key == SCREEN_EFFECTS_KEY)
        };
        assert!(side(Session::X11) && !side(Session::Wayland), "{de}");
    }
    for de in ["kde", "gnome", "hyprland", "cinnamon-mint"] {
        for s in [Session::X11, Session::Wayland, Session::Any] {
            assert!(category_available_in(SCREEN_EFFECTS_KEY, de, s), "{de} {s:?}");
        }
    }
    let mut v = CatalogView::new(get_category(home(), SCREEN_EFFECTS_KEY).unwrap(), "mate");
    assert!(!v.de_blocked());
    v.session = Session::Wayland;
    assert!(v.de_blocked());
    // Sections without a session condition do not depend on it.
    for key in ["icons", "cursor", "marco", "gtk", "fonts"] {
        assert!(category_available_in(key, "mate", Session::Wayland), "{key}");
    }
}
