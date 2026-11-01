//! Сверка КАРКАСА главного окна: состав и порядок вкладок, их подписи,
//! заголовок и наименьший размер окна, пункты меню.
//!
//! ЧТО ЗДЕСЬ ЭТАЛОН (с 02.10.2026). Python в Rexon — только архив: он не
//! эталон и не мост. Эталон — снимок `tests/golden/qt_main_cases.json`,
//! замороженный как собственная запись Rust: подписи и порядок вкладок,
//! заголовок, размер окна, пункты меню. Каждое из этих значений сличается с
//! ядром (`qt_main`, `tray`): подписи вкладок и пункт «Выход» живут ещё и в
//! `tray.rs` (значок в лотке строит по ним подменю «Разделы»), и две копии
//! человеческого текста — ровно тот класс, который правило #12 велит сводить
//! к одному движку.
//!
//! Прежние сторожа «строка снимка обязана стоять в замороженной редакции
//! Python-каркаса» и «живой `rexon/qt_main/` совпадает с ней побайтово»
//! сняты вместе с Python. Сняты и проверки, которые смотрели только в
//! описание Python-окна (заглушка «Загрузка…», QSS полосы меню, аварийный
//! ход `REXON_WIDGET_TAB`): двойника в Rust у них нет, сверять было не с чем.
//! Атрибуцию LGPL в «О программе» держит `window_menu_parity.rs` — по тексту
//! ядра.
//!
//! ЧЕГО ЭТА СВЕРКА НЕ МЕРИТ. Вид окна: расположение полосы вкладок, отступы,
//! краски. Это мерит прогон окна (`rexon-main-window --selftest`) и глаз.
//!
//! ЧТО СВЕРКА ПОЙМАЛА ПРИ ЗАВЕДЕНИИ (07.09.2026). Копии подписей уже
//! разъехались: окно показывало «🖥 Рабочие столы» и «🚪 Выход», ядро знало
//! «Рабочие столы»/«Desktops» и «Выход»/«Quit». Починено таблицей
//! `tray::ICONS` и `tray::titled`: значок переехал в ядро, подпись осталась
//! одна на окно и на лоток.

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
// Таблица вкладок: окно против ядра
// ============================================================================

/// Состав и порядок вкладок в окне — тот же, что снят эталоном, и каждая
/// вкладка известна ядру.
///
/// Порядок здесь не косметика: значок в лотке нумерует подменю «Разделы»
/// НОМЕРОМ вкладки (`tray::ID_SECTION_BASE + порядковый`), и переставленная
/// вкладка увела бы пункт меню на чужой раздел.
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

/// ⭐ГЛАВНАЯ ПРОВЕРКА. Подпись вкладки в окне и подпись, которую отдаёт ядро,
/// — одна и та же строка, а не две разошедшиеся копии.
///
/// Ядро обязано уметь выдать ИМЕННО ТО, что видит покупатель, вместе со
/// значком: иначе перенести каркас нечем, а лоток и окно называют один и тот
/// же раздел по-разному.
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

/// Вкладка переводится так же, как её пункт в лотке: значок остаётся,
/// текст меняется. Русский и английский обязаны различаться везде, кроме
/// собственных имён («Steam», «Wallpaper Engine»).
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

/// Подменю «Разделы» строится по тем же вкладкам и в том же порядке.
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
// Окно и меню
// ============================================================================

#[test]
fn window_title_and_minimum_size() {
    let _lang = i18n_pin::ru();
    let g = fx();
    // Ядро по-русски говорит то же, что снято с окна.
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
    // Правило #16 (Steam Deck): окно не должно требовать экрана больше того,
    // что даёт режим рабочего стола Steam Deck (1280×800).
    assert!(w <= 1280 && h <= 800, "окно не влезет на Steam Deck: {w}×{h}");
}

/// Пункты меню каркаса — те же, что сняты эталоном, и «Выход» подписан так
/// же, как в лотке.
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
    // Единственный пункт с двойником в ядре: он же «Выход» в лотке.
    assert_eq!(
        tray::titled("quit"),
        s(&actions[3]["label"]),
        "«Выход» в меню окна и в лотке разошёлся"
    );
}

// ============================================================================
// Перевод: лоток и каркас окна на каталоге
// ============================================================================
//
// Подписи лотка, вкладок и заголовок окна были зашиты по-русски (лоток —
// парами «русский/английский»); теперь исходный язык английский, а русский
// живёт в `po/ru.po`. Ниже — ДОСЛОВНЫЕ русские тексты до переноса.

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
    // Запись автозапуска не зависит от языка интерфейса: русский в ней свой.
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

/// Английский — витрина: пункты меню и вкладки с большой буквы в каждом
/// слове (Title Case), фразы — обычным предложением.
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
