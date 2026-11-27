//! Check of the main window FRAME: the set and order of tabs, their labels,
//! the title and the minimum window size, menu items.
//!
//! WHAT THE GOLDEN IS HERE (since 02.10.2026). Python in Rexon is only an archive: it is not
//! a golden and not a bridge. The golden is the snapshot `tests/golden/qt_main_cases.json`,
//! frozen as Rust's own record: tab labels and order,
//! title, window size, menu items. Each of these values is compared with
//! the core (`qt_main`, `tray`): the tab labels and the «Quit» item also live in
//! `tray.rs` (the tray icon builds its «Sections» submenu from them), and two copies
//! of human text are exactly the class that rule #12 tells us to reduce
//! to one engine.
//!
//! The former guards "a snapshot line must stand in the frozen edition of the
//! Python frame" and "the live `rexon/qt_main/` matches it byte-wise" were
//! removed together with Python. The checks that looked only at
//! the description of the Python window (the «Loading…» placeholder, the menu bar QSS, the emergency
//! `REXON_WIDGET_TAB` path) were removed too: they have no Rust counterpart, there was nothing to compare with.
//! The LGPL attribution in «About» is held by `window_menu_parity.rs` — by the text of
//! the core.
//!
//! WHAT THIS CHECK DOES NOT MEASURE. The look of the window: the tab bar placement, margins,
//! colours. That is measured by a window run (`rexon-main-window --selftest`) and the eye.
//!
//! WHAT THE CHECK CAUGHT WHEN IT WAS ADDED (07.09.2026). The label copies had already
//! diverged: the window showed «🖥 Рабочие столы» and «🚪 Выход», the core knew
//! «Рабочие столы»/«Desktops» and «Выход»/«Quit». Fixed with the table
//! `tray::ICONS` and `tray::titled`: the icon moved into the core, the label stayed
//! one for the window and the tray.

use rexon_core::tray::{self, ItemKind};
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/qt_main_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn tabs() -> Vec<(String, String)> {
    fx()["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (s(&t["id"]).to_string(), s(&t["label"]).to_string()))
        .collect()
}

// ============================================================================
// Tab table: the window versus the core
// ============================================================================

/// The set and order of tabs in the window are the same as captured by the golden, and every
/// tab is known to the core.
///
/// The order here is not cosmetics: the tray icon numbers the «Sections» submenu
/// by the tab NUMBER (`tray::ID_SECTION_BASE + index`), and a moved
/// tab would send a menu item to someone else's section.
#[test]
fn tab_table_is_known_to_core() {
    let ids: Vec<String> = tabs().into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        ids,
        rexon_core::qt_main::SECTIONS,
        "состав или порядок вкладок разошёлся с эталоном"
    );
    for (id, _) in tabs() {
        assert!(
            tray::label_msgid(&format!("tab.{id}")).is_some(),
            "вкладка {id} есть в окне, но ядро о ней не знает: подменю \
             «Разделы» в лотке подпишет её идентификатором"
        );
    }
}

/// ⭐THE MAIN CHECK. The tab label in the window and the label the core returns
/// are one and the same string, not two diverged copies.
///
/// The core must be able to return EXACTLY what the buyer sees, together with
/// the icon: otherwise there is nothing to move the frame with, and the tray and the window call one and the
/// same section differently.
#[test]
fn tab_labels_match_core() {
    let _lang = i18n_pin::ru();
    for (id, label) in tabs() {
        assert_eq!(
            tray::titled(&format!("tab.{id}")),
            label,
            "подпись вкладки {id} в окне и в ядре разошлась"
        );
    }
}

/// A tab is translated the same way as its tray item: the icon stays,
/// the text changes. Russian and English must differ everywhere except
/// proper names («Steam», «Wallpaper Engine»).
#[test]
fn tab_labels_are_translated() {
    let _lang = i18n_pin::en();
    for (id, label) in tabs() {
        let en = tray::titled(&format!("tab.{id}"));
        let icon = label.chars().next().unwrap();
        assert!(
            en.starts_with(icon),
            "английская подпись вкладки {id} потеряла значок: {en}"
        );
        if !matches!(id.as_str(), "steam" | "wallpaper_engine") {
            assert_ne!(en, label, "вкладка {id} не переведена на английский");
        }
    }
}

/// The «Sections» submenu is built from the same tabs and in the same order.
#[test]
fn tray_sections_follow_window_tabs() {
    let _lang = i18n_pin::ru();
    let ids: Vec<String> = tabs().into_iter().map(|(id, _)| id).collect();
    let items = tray::menu(&ids, tray::TrayState::default());
    let sections: Vec<&tray::MenuItem> = items
        .iter()
        .skip_while(|i| i.kind != ItemKind::SubmenuBegin)
        .skip(1)
        .take_while(|i| i.kind != ItemKind::SubmenuEnd)
        .collect();
    assert_eq!(sections.len(), ids.len(), "разделов в лотке не столько же");
    for (index, (id, _)) in tabs().into_iter().enumerate() {
        assert_eq!(sections[index].id, tray::ID_SECTION_BASE + index as i32);
        assert_eq!(sections[index].label, tray::section_label(&id));
    }
}

// ============================================================================
// Window and menu
// ============================================================================

#[test]
fn window_title_and_minimum_size() {
    let _lang = i18n_pin::ru();
    let g = fx();
    // The core in Russian says the same as what was captured from the window.
    assert_eq!(rexon_core::qt_main::window_title(), s(&g["window"]["title"]));
    let (w, h) = (
        g["window"]["min_width"].as_i64().unwrap(),
        g["window"]["min_height"].as_i64().unwrap(),
    );
    assert_eq!(
        rexon_core::qt_main::WINDOW_MIN_SIZE,
        (w as i32, h as i32),
        "наименьший размер окна разошёлся с эталоном"
    );
    // Rule #16 (Steam Deck): the window must not require a screen larger than
    // what the Steam Deck desktop mode gives (1280×800).
    assert!(w <= 1280 && h <= 800, "окно не влезет на Steam Deck: {w}×{h}");
}

/// The frame's menu items are the same as captured by the golden, and «Quit» is labelled the
/// same way as in the tray.
#[test]
fn menu_items_match_snapshot_and_core() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(s(&g["menu"]["button"]), rexon_core::window_menu::MENU_BUTTON, "кнопка меню");
    let actions = g["menu"]["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 4, "состав меню каркаса изменился");
    let items = rexon_core::window_menu::menu_items();
    for (a, item) in actions.iter().zip(&items) {
        assert_eq!(s(&a["label"]), item.label, "пункт меню");
        assert_eq!(a["shortcut"].as_str().unwrap_or(""), item.shortcut, "горячая клавиша пункта меню");
    }
    // The only item with a counterpart in the core: it is also «Quit» in the tray.
    assert_eq!(
        tray::titled("quit"),
        s(&actions[3]["label"]),
        "«Выход» в меню окна и в лотке разошёлся"
    );
}

// ============================================================================
// Translation: the tray and the window frame on the catalog
// ============================================================================
//
// The labels of the tray, the tabs and the window title used to be hard-coded in Russian (the tray in
// "Russian/English" pairs); now the source language is English, and Russian
// lives in `po/ru.po`. Below are the VERBATIM Russian texts before the move.

const TRAY_RU_BEFORE: &[(&str, &str)] = &[
    ("open", "Открыть Rexon"),
    ("sections", "Разделы"),
    ("next", "Следующие обои"),
    ("prev", "Предыдущие обои"),
    ("shuffle", "Вперемешку"),
    ("rotate", "Смена обоев"),
    ("pause", "Пауза"),
    ("resume", "Возобновить"),
    ("mute", "Заглушить"),
    ("unmute", "Включить звук"),
    ("rescan", "Пересканировать обои"),
    ("autostart", "Запускать при входе в систему"),
    ("quit", "Выход"),
    ("rot.off", "Выкл."),
    ("rot.30s", "30 секунд"),
    ("rot.1m", "1 минута"),
    ("rot.5m", "5 минут"),
    ("rot.15m", "15 минут"),
    ("rot.1h", "1 час"),
    ("tab.desktops", "Рабочие столы"),
    ("tab.appearance", "Оформление"),
    ("tab.extensions", "Расширения"),
    ("tab.wallpaper_engine", "Wallpaper Engine"),
    ("tab.steam", "Steam"),
    ("tab.settings", "Настройки"),
    ("tip.title", "Rexon"),
    ("tip.body", "Менеджер рабочих столов"),
    ("hidden.title", "Rexon свернулся в лоток"),
    (
        "hidden.body",
        "Окно закрыто, но Rexon продолжает работать. \
         Открыть его снова — щелчком по значку; выйти — «Выход» в меню значка.",
    ),
];

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    for (key, before) in TRAY_RU_BEFORE {
        assert_eq!(tray::label(key), *before, "подпись лотка «{key}»");
    }
    assert_eq!(rexon_core::qt_main::window_title(), "Rexon — менеджер рабочих столов");
    assert_eq!(rexon_core::qt_main::section_tab_label("desktops"), "🖥️ Рабочие столы");
    assert_eq!(tray::titled("quit"), "🚪 Выход");
    assert_eq!(
        rexon_core::qt_main::not_ported_notice("steam"),
        "Раздел «Steam» в этом окне пока не готов."
    );
    assert_eq!(
        rexon_core::qt_main::subsection_label("compare"),
        "Сравнение рабочих столов"
    );
    let (title, body) = tray::hidden_notice();
    assert_eq!(title, "Rexon свернулся в лоток");
    assert!(body.starts_with("Окно закрыто"), "{body}");
    // The autostart entry does not depend on the interface language: it has its own Russian.
    let desktop = tray::autostart_desktop_content("/x --tray");
    assert!(desktop.contains("\nComment[ru]=Запускать Rexon в системном лотке при входе\n"));
    assert!(desktop.contains("\nGenericName[ru]=Менеджер рабочих столов\n"));
}

fn cyrillic(text: &str) -> bool {
    text.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё'))
}

#[test]
fn english_tray_and_frame_have_no_russian() {
    let _lang = i18n_pin::en();
    let ids: Vec<String> = rexon_core::qt_main::SECTIONS.iter().map(|s| s.to_string()).collect();
    let mut all: Vec<String> = Vec::new();
    for state in [
        tray::TrayState::default(),
        tray::TrayState { daemon: true, paused: true, muted: true, ..Default::default() },
    ] {
        all.extend(
            tray::menu(&ids, state)
                .into_iter()
                .filter(|i| !matches!(i.kind, ItemKind::Separator | ItemKind::SubmenuEnd))
                .map(|i| i.label),
        );
    }
    all.push(tray::tooltip_title());
    all.push(tray::tooltip_body());
    let (title, body) = tray::hidden_notice();
    all.extend([title, body]);
    all.push(rexon_core::qt_main::window_title());
    for id in &ids {
        all.push(rexon_core::qt_main::section_tab_label(id));
        all.push(rexon_core::qt_main::not_ported_notice(id));
    }
    all.push(rexon_core::qt_main::subsection_label("compare"));
    assert!(all.len() > 40, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!text.is_empty(), "пустая подпись в английском интерфейсе");
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// English is the storefront: menu items and tabs capitalize every
/// word (Title Case), phrases are written as an ordinary sentence.
#[test]
fn english_labels_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(rexon_core::qt_main::window_title(), "Rexon — Desktop Environment Manager");
    let ids: Vec<String> = rexon_core::qt_main::SECTIONS.iter().map(|s| s.to_string()).collect();
    let tabs: Vec<String> = ids.iter().map(|id| rexon_core::qt_main::section_tab_label(id)).collect();
    assert_eq!(
        tabs,
        ["🖥️ Desktops", "🎨 Appearance", "🧩 Extensions", "🖼️ Wallpaper Engine", "🎮 Steam", "⚙️ Settings"]
    );
    let st = tray::TrayState { daemon: true, ..Default::default() };
    let labels: Vec<String> = tray::menu(&ids, st)
        .into_iter()
        .filter(|i| !i.label.is_empty())
        .map(|i| i.label)
        .collect();
    for want in ["Open Rexon", "Next Wallpaper", "Previous Wallpaper", "Rescan Wallpapers", "5 Minutes", "Quit"] {
        assert!(labels.iter().any(|l| l == want), "нет «{want}» в меню: {labels:?}");
    }
    assert_eq!(
        rexon_core::qt_main::not_ported_notice("steam"),
        "The “Steam” section is not available in this window yet."
    );
}
