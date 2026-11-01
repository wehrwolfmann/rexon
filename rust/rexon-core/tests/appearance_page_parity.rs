//! Приёмка перенесённых чисел вкладки «Оформление».
//!
//! Значения выписаны ДОСЛОВНО (когда-то — из виджетной вкладки на Python;
//! с 02.10.2026 Python в Rexon — только архив, и эти записи — собственный
//! эталон Rust): набор ловит не «код компилируется», а сдвиг того, что
//! вкладка показывает.

use rexon_core::appearance_page as ap;
use rexon_core::ui_base;

// Подписи вкладки переводятся; сверки с прежним видом идут на русском
// каталоге `po/ru.po` — это и есть снимок «по-русски как было». Английский
// исходник держит `english_texts`.
mod i18n_pin;

#[test]
fn catalog_geometry_matches_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::PAGE_SIZE, 20);
    assert_eq!(ap::CARD_W, 300);
    assert_eq!(ap::CARD_H, 285);
    assert_eq!(ap::PREVIEW_H, 170);
    assert_eq!(ap::card_text_width(), 270); // CARD_W - 30
    assert_eq!(ap::SIDEBAR_W, 210);
    assert_eq!(ap::SIDEBAR_ROW_H, 40);
    assert_eq!(ap::SEARCH_W, 240);
}

#[test]
fn detail_panel_matches_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::DETAIL_PREVIEW_W, 600);
    assert_eq!(ap::DETAIL_PREVIEW_H, 340);
    assert_eq!(ap::OVERLAY_BTN, 44);
    assert_eq!(ap::OVERLAY_MARGIN, 10);
    assert_eq!(ap::COUNTER_H, 24);
    // Круглая кнопка: радиус ровно половина стороны, иначе угол «срежется».
    assert!(ap::image_overlay_qss(ap::OVERLAY_ZOOM_PT).contains("border-radius: 22px"));
    assert!(ap::gallery_counter_qss().contains("border-radius: 12px"));
}

#[test]
fn counter_width_matches_the_original() {
    let _lang = i18n_pin::ru();
    // max(48, ширина текста + 20)
    for text_w in [0, 10, 27, 28, 29, 100, 500] {
        assert_eq!(ap::counter_width(text_w), 48.max(text_w + 20), "{text_w}");
    }
}

#[test]
fn delays_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::SUGGEST_DEBOUNCE_MS, 220);
    assert_eq!(ap::CATALOG_DEBOUNCE_MS, 200);
    assert_eq!(ap::FIRST_LOAD_MS, 120);
    assert_eq!(ap::PREFETCH_START_MS, 900);
    assert_eq!(ap::PURGE_TRANSLATIONS_MS, 1500);
    assert_eq!(ap::PREFETCH_PAUSE_MS, 450);
    assert_eq!(ap::STYLE_STORM_MS, 600);
    assert_eq!(ap::GALLERY_WARM_DELAY_MS, 800);
}

#[test]
fn preview_loader_thresholds_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::PREVIEW_MAX_INFLIGHT, 6);
    assert_eq!(ap::PREVIEW_MAX_INFLIGHT_LOW, 2);
    assert_eq!(ap::PREVIEW_RETRIES, 2);
    assert_eq!(ap::PREVIEW_RETRY_DELAY_MS, 700);
    assert_eq!(ap::PREVIEW_MAX_BYTES, 20 * 1024 * 1024);
    assert_eq!(
        ap::PREVIEW_FATAL_HTTP,
        &[400u16, 401, 403, 404, 405, 410, 451]
    );
    // Фон не имеет права занять все места: живой плитке всегда есть куда встать.
    assert!(ap::PREVIEW_MAX_INFLIGHT_LOW < ap::PREVIEW_MAX_INFLIGHT);
}

#[test]
fn suggestions_geometry_matches_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::SUGGEST_ROW_H, 54);
    assert_eq!(ap::SUGGEST_THUMB_W, 72);
    assert_eq!(ap::SUGGEST_THUMB_H, 40);
    assert_eq!(ap::SUGGEST_MAX_ROWS, 12);
    assert_eq!(ap::SUGGEST_TEXT_W, 300);
    assert_eq!(ap::SUGGEST_MIN_CHARS, 2);
    // n * ROW_H + 2, с обрезкой по потолку строк
    assert_eq!(ap::suggest_height(5), 5 * 54 + 2);
    assert_eq!(ap::suggest_height(30), 12 * 54 + 2);
    assert_eq!(ap::suggest_width(240), 360);
    assert_eq!(ap::suggest_width(500), 500);
}

#[test]
fn sorts_match_the_original() {
    let _lang = i18n_pin::ru();
    let sorts: Vec<(String, &str)> =
        ap::SORTS.iter().map(|(l, k)| (ap::sort_label(l), *k)).collect();
    let sorts: Vec<(&str, &str)> = sorts.iter().map(|(l, k)| (l.as_str(), *k)).collect();
    assert_eq!(
        sorts,
        [
            ("🆕 Новые", "new"),
            ("⬇️ Скачивания", "down"),
            ("⭐ Рейтинг", "high"),
            ("🔤 Алфавит", "alpha"),
        ]
    );
    assert_eq!(ap::default_sort_index(), 0);
}

#[test]
fn steam_dark_keeps_the_historic_look() {
    let _lang = i18n_pin::ru();
    // На скине по умолчанию токены равны исторической Steam-палитре: вид
    // вкладки после переноса обязан остаться прежним.
    let c = ui_base::skin_colors("steam_dark");
    let qss = ap::page_qss(&c);
    assert!(qss.contains("background: #1b2838;"));
    assert!(qss.contains("border-left: 3px solid #66c0f4;"));
    assert!(qss.contains("#InstallBtn, #InstallApplyBtn {"));
    assert!(qss.contains("background: #5ba32b;"));
    // Приглушение неактивной кнопки на этом скине даёт ровно прежний белый.
    assert!(qss.contains("#PageBtn:disabled { color: rgba(255,255,255,0.55); }"));
}

#[test]
fn every_skin_paints_the_page_without_holes() {
    let _lang = i18n_pin::ru();
    // Ни одного «пустого» токена: незакрытая подстановка сделала бы правило
    // недействительным, и участок вкладки остался бы неокрашенным.
    for id in ["system", "steam_dark", "light", "synthwave"] {
        for qss in [
            ap::page_qss(&ui_base::skin_colors(id)),
            ap::suggestions_qss(&ui_base::skin_colors(id)),
        ] {
            assert!(!qss.contains("{}"), "{id}");
            assert!(!qss.contains(": ;"), "{id}");
            assert!(!qss.contains(":;"), "{id}");
        }
    }
}

/// Подписи, которые раньше собирались HTML-ом с вшитым цветом, красятся
/// таблицей стилей — и на КАЖДОМ скине своими токенами.
///
/// Пока цвет вшивался в текст при построении, смена скина не трогала уже
/// открытую деталь: окно и вкладка вокруг перекрашивались сразу, а
/// мета-строка, счётчики, путь установки и показанные комментарии
/// оставались в цветах прошлого скина.
#[test]
fn detail_labels_are_painted_by_the_stylesheet() {
    let _lang = i18n_pin::ru();
    for id in ["system", "steam_dark", "light", "synthwave"] {
        let c = ui_base::skin_colors(id);
        let qss = ap::page_qss(&c);
        let want = |token: &str| c.get(token).unwrap().clone();
        for (rule, token) in [
            ("#CardStar", "rating_star"),
            ("#DetailMeta", "text_secondary"),
            ("#DetailStars", "rating_star"),
            ("#DetailDownloads", "text_secondary"),
            ("#DetailPath", "text_dim"),
            ("#CommentAuthor", "text_primary"),
            ("#CommentDate", "text_dim"),
        ] {
            let line = qss
                .lines()
                .find(|l| l.starts_with(&format!("{rule} {{")))
                .unwrap_or_else(|| panic!("{id}: нет правила для {rule}"));
            assert!(
                line.contains(&format!("color: {};", want(token))),
                "{id}: {rule} красится не токеном {token}: {line}"
            );
        }
        // Ник САМОГО автора товара — отдельным признаком, а не отдельной
        // веткой в Python.
        assert!(
            qss.contains(&format!(
                "#CommentAuthor[own=\"true\"] {{ color: {}; }}",
                want("author_self")
            )),
            "{id}: ник автора товара красится мимо токена author_self"
        );
    }
}

/// Во вкладке не осталось цвета, вшитого в текст подписи.
///
/// Единственное исключение — ССЫЛКА `<a>`: цвет якоря Qt берёт один раз, при
/// разборе текста, и ни правилом стилей, ни ролью `Link` в палитре его потом
/// не сдвинуть (проверено замером). Такое место одно — переключатель
/// «перевод ⇄ оригинал» (`toggle_link`), и он перекрашивается пере-разбором
/// своей же подписи: цвет подставляется из скина в момент сборки.
#[test]
fn no_color_is_baked_into_label_text() {
    let _lang = i18n_pin::ru();
    const SRC: &str = include_str!("../src/appearance_page.rs");
    // Подстановка цвета в текст подписи — `style='color:{…}`. Строки-комментарии
    // не в счёт: там вшитый цвет описан словами, а не собран.
    let baked: Vec<&str> = SRC
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .filter(|l| l.contains("style='color:{") || l.contains("style=\\\"color:{"))
        .collect();
    assert_eq!(
        baked.len(),
        1,
        "цвет вшит в текст подписи не только у ссылки-переключателя: {baked:?}"
    );
    assert!(
        baked[0].contains("<a href='rexon:toggle'"),
        "вшитый цвет вне ссылки-переключателя: {}",
        baked[0]
    );
    // То же по поведению: в собранном тексте цвет стоит ровно у ссылки.
    for html in [
        ap::comment_body_html("orig", "перевод", true, false, "#123456"),
        ap::description_html("orig", "перевод", true, true, "#123456"),
    ] {
        assert_eq!(html.matches("color:").count(), 1, "{html}");
        assert!(
            html.contains("<a href='rexon:toggle' style='color:#123456;text-decoration:none;"),
            "цвет переключателя не из скина или не у ссылки: {html}"
        );
    }
    // Без перевода переключателя нет — и цвета в тексте нет вовсе.
    assert!(!ap::comment_body_html("orig", "orig", false, false, "#123456").contains("color:"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Журнал установленного: «установлена» ≠ «применена»
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::models::ThemeItem;
use serde_json::{json, Map, Value};
use std::path::Path;

#[test]
fn rexon_type_map_matches_the_original() {
    let _lang = i18n_pin::ru();
    // Все 17 строк карты оригинала, дословно.
    for (t, k) in [
        ("theme-kde-global", "global"),
        ("theme-kde-plasma", "plasma"),
        ("theme-kde-colors", "colors"),
        ("theme-kde-aurorae", "aurorae"),
        ("theme-icons", "icons"),
        ("theme-cursors", "cursor"),
        ("theme-kvantum", "kvantum"),
        ("theme-sddm", "sddm"),
        ("theme-plymouth", "plymouth"),
        ("theme-gtk", "gtk"),
        ("theme-widget-style", "widgetstyle"),
        ("theme-kde-splash", "splash"),
        ("theme-kde-lockscreen", "lockscreen"),
        ("theme-sounds", "sounds"),
        ("font-pack", "fonts"),
        ("theme-plasmoid", "plasmoids"),
        ("kde-wallpaper-plugin", "wallpaper_plugins"),
    ] {
        assert_eq!(ap::rexon_type_to_apply_kind(t), k, "{t}");
    }
    // Обе бывшие свалки в карте отсутствуют — по ним вид определить нельзя,
    // запись обязана уйти на второй заход (по OCS-категории).
    assert_eq!(ap::rexon_type_to_apply_kind("wallpaper-pack"), "");
    assert_eq!(ap::rexon_type_to_apply_kind("plugin"), "");
    assert_eq!(ap::rexon_type_to_apply_kind(""), "");
}

#[test]
fn theme_names_strip_colors_suffix_and_slashes() {
    let _lang = i18n_pin::ru();
    let paths: Vec<String> = [
        "/home/u/.local/share/color-schemes/Nord.colors",
        "/home/u/.local/share/plasma/desktoptheme/Breeze/",
        "/",
        "",
        "/home/u/.local/share/icons/Papirus",
        "/x/Mixed.COLORS",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        ap::log_theme_names(&paths),
        vec!["Nord", "Breeze", "Papirus", "Mixed"]
    );
}

fn rec(rexon_type: &str, category_id: i64, paths: &[&str]) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("rexon_type".into(), json!(rexon_type));
    m.insert("category_id".into(), json!(category_id));
    m.insert("paths".into(), json!(paths));
    m
}

#[test]
fn apply_kind_falls_back_to_the_ocs_category() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    // Свой тип — берётся сразу, без обращения к таблице категорий.
    assert_eq!(
        ap::log_apply_kind(&home, &rec("theme-kde-global", 0, &[]), 0),
        "global"
    );
    // Бывшая свалка «plugin» + категория 722 (Global Themes) → второй заход.
    assert_eq!(
        ap::log_apply_kind(&home, &rec("plugin", 722, &[]), 0),
        "global"
    );
    // Категории в записи нет — берём с карточки.
    assert_eq!(ap::log_apply_kind(&home, &rec("plugin", 0, &[]), 722), "global");
    // Вид без понятия «применена сейчас» — пусто.
    assert_eq!(ap::log_apply_kind(&home, &rec("font-pack", 0, &[]), 0), "");
    assert_eq!(ap::log_apply_kind(&home, &rec("theme-plasmoid", 0, &[]), 0), "");
}

#[test]
fn stamping_marks_installed_and_applied_apart() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let mut log = Map::new();
    log.insert(
        "111".into(),
        Value::Object(rec(
            "theme-kde-global",
            722,
            &["/home/u/.local/share/plasma/look-and-feel/Nord"],
        )),
    );
    log.insert(
        "222".into(),
        Value::Object(rec(
            "theme-kde-global",
            722,
            &["/home/u/.local/share/plasma/look-and-feel/Other"],
        )),
    );
    log.insert(
        "333".into(),
        Value::Object(rec("font-pack", 0, &["/home/u/.local/share/fonts/Iosevka"])),
    );
    let mut items = vec![
        ThemeItem { id: "111".into(), ..Default::default() },
        ThemeItem { id: "222".into(), ..Default::default() },
        ThemeItem { id: "333".into(), ..Default::default() },
        ThemeItem { id: "444".into(), ..Default::default() },
    ];
    let mut active = ap::LazyActiveMap::new(|| {
        let mut m = Map::new();
        m.insert("global".into(), json!("nord"));
        m
    });
    ap::stamp_installed_from_log(&home, &log, &mut items, &mut active);
    // Скачана и включена.
    assert!(items[0].installed && items[0].applied);
    // Скачана, но включена другая — «установлено» без «применено».
    assert!(items[1].installed && !items[1].applied);
    // У шрифтов понятия «применён» нет вовсе.
    assert!(items[2].installed && !items[2].applied);
    // Товара в журнале нет — статус не трогаем.
    assert!(!items[3].installed && !items[3].applied);
}

/// Рамка MATE из магазина «Установить» ставит и применяет: плитка обязана
/// показать «применена», а не «установлено» (живая проверка MATE, 17.10).
/// Раздел окружения с темой (Marco, xfwm4, оболочки) — вид с «применена
/// сейчас»; дополнения и Conky — нет.
#[test]
fn de_section_theme_from_store_is_stamped_applied() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    // Товар «Metacity Themes» (125) ведёт в раздел «marco».
    assert_eq!(ap::log_apply_kind(&home, &rec("theme-marco", 125, &[]), 125), "marco");
    assert_eq!(ap::log_apply_kind(&home, &rec("theme-xfwm4", 138, &[]), 138), "xfwm4");
    let mut log = Map::new();
    log.insert(
        "555".into(),
        Value::Object(rec("theme-marco", 125, &["/home/u/.local/share/themes/Win8-Metacity"])),
    );
    let mut items = vec![ThemeItem { id: "555".into(), category_id: 125, ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(|| {
        let mut m = Map::new();
        m.insert("marco".into(), json!("Win8-Metacity"));
        m
    });
    ap::stamp_installed_from_log(&home, &log, &mut items, &mut active);
    assert!(items[0].installed && items[0].applied, "рамка применена, а плитка — «Установлено»");
}

/// Применима ли тема товара (по разделу) и под каким именем она стоит.
#[test]
fn store_item_apply_name_comes_from_the_install_log() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let item = |cat: i64| ThemeItem { id: "7".into(), category_id: cat, ..Default::default() };
    assert!(ap::store_item_can_apply(&home, &item(722))); // глобальная тема
    assert!(ap::store_item_can_apply(&home, &item(125))); // рамка Marco
    assert!(!ap::store_item_can_apply(&home, &item(rexon_core::categories::CONKY_SECTION_ID)));
    let mut log = Map::new();
    log.insert(
        "7".into(),
        Value::Object(rec("theme-kde-colors", 0, &["/home/u/.local/share/color-schemes/Nord.colors"])),
    );
    assert_eq!(ap::logged_theme_name(&log, "7", |_| true).as_deref(), Some("Nord"));
    // Файлы темы сняли руками — применять нечего.
    assert_eq!(ap::logged_theme_name(&log, "7", |_| false), None);
    assert_eq!(ap::logged_theme_name(&log, "8", |_| true), None);
}

/// Плитка витрин «Новые» и «Популярные» называет вид вещи той же
/// подписью, что «Активные»: авторы пишут «for Plasma 6» в названии
/// значков, и без вида кажется, что фильтр по окружению не работает.
#[test]
fn showcase_tiles_name_the_kind_of_item() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let item = |cat: i64| ThemeItem { id: "1".into(), category_id: cat, author: "Автор".into(), ..Default::default() };
    assert_eq!(ap::store_item_kind_label(&home, &item(132)).as_deref(), Some("📁 Иконки"));
    assert_eq!(ap::store_item_kind_label(&home, &item(135)).as_deref(), Some("🎨 GTK-тема"));
    assert_eq!(ap::store_item_kind_label(&home, &item(107)).as_deref(), Some("🖱️ Курсор"));
    // Вид без строки скана — подпись раздела.
    assert!(ap::store_item_kind_label(&home, &item(299)).is_some());
    assert_eq!(ap::store_item_kind_label(&home, &item(0)), None);
    // Строка автора на плитке витрины: вид · автор; в разделе — как была.
    assert_eq!(ap::showcase_author(&home, &item(132), true), "📁 Иконки · Автор");
    assert_eq!(ap::showcase_author(&home, &item(132), false), "Автор");
}

#[test]
fn active_map_is_pulled_at_most_once_and_only_when_needed() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let mut log = Map::new();
    // Только шрифты: применения у них нет → карту дёргать НЕ за чем.
    log.insert("1".into(), Value::Object(rec("font-pack", 0, &["/x/A"])));
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(Map::new);
    ap::stamp_installed_from_log(&home, &log, &mut items, &mut active);
    assert!(!active.was_fetched(), "kreadconfig6 дёрнут впустую");

    // Пустой журнал — тоже ни одного вызова.
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(Map::new);
    ap::stamp_installed_from_log(&home, &Map::new(), &mut items, &mut active);
    assert!(!active.was_fetched());

    // Две применимые карточки — источник дёргается ровно один раз.
    let mut calls = 0;
    {
        let mut log = Map::new();
        log.insert("1".into(), Value::Object(rec("theme-icons", 0, &["/x/A"])));
        log.insert("2".into(), Value::Object(rec("theme-icons", 0, &["/x/B"])));
        let mut items = vec![
            ThemeItem { id: "1".into(), ..Default::default() },
            ThemeItem { id: "2".into(), ..Default::default() },
        ];
        let mut active = ap::LazyActiveMap::new(|| {
            calls += 1;
            let mut m = Map::new();
            m.insert("icons".into(), json!("A"));
            m
        });
        ap::stamp_installed_from_log(&home, &log, &mut items, &mut active);
        assert!(items[0].applied && !items[1].applied);
    }
    assert_eq!(calls, 1);
}

/// Приватный каталог под журнал: живой `~/.config/rexon` тест не трогает.
fn scratch_log(tag: &str, text: Option<&str>) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!(
        "rexon-stamp-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ocs_installed.json");
    if let Some(t) = text {
        std::fs::write(&path, t).unwrap();
    }
    (dir, path.to_string_lossy().into_owned())
}

#[test]
fn stamp_reads_the_install_log_from_disk() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let mut log = Map::new();
    log.insert(
        "111".into(),
        Value::Object(rec(
            "theme-kde-global",
            722,
            &["/home/u/.local/share/plasma/look-and-feel/Nord"],
        )),
    );
    log.insert("333".into(), Value::Object(rec("font-pack", 0, &["/x/Iosevka"])));
    let (dir, path) = scratch_log("disk", Some(&Value::Object(log).to_string()));

    let mut items = vec![
        ThemeItem { id: "111".into(), ..Default::default() },
        ThemeItem { id: "333".into(), ..Default::default() },
        ThemeItem { id: "444".into(), ..Default::default() },
    ];
    let mut active = ap::LazyActiveMap::new(|| {
        let mut m = Map::new();
        m.insert("global".into(), json!("Nord"));
        m
    });
    ap::stamp_installed_from_log_file(&home, &path, &mut items, &mut active);
    assert!(items[0].installed && items[0].applied, "скачана и включена");
    assert!(items[1].installed && !items[1].applied, "шрифты не «применяются»");
    assert!(!items[2].installed && !items[2].applied, "нет в журнале");

    // Нет файла и битый JSON — пустой журнал: карточки как пришли, карта
    // активных тем не дёргается.
    for (tag, text) in [("missing", None), ("broken", Some("{не json"))] {
        let (d, p) = scratch_log(tag, text);
        let mut items = vec![ThemeItem { id: "111".into(), ..Default::default() }];
        let mut active = ap::LazyActiveMap::new(Map::new);
        ap::stamp_installed_from_log_file(&home, &p, &mut items, &mut active);
        assert!(!items[0].installed && !items[0].applied, "{tag}");
        assert!(!active.was_fetched(), "{tag}");
        let _ = std::fs::remove_dir_all(d);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn install_outcome_marks_installed_and_applied_only_by_fact() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let mut log = Map::new();
    log.insert(
        "1".into(),
        Value::Object(rec("theme-icons", 0, &["/home/u/.local/share/icons/Papirus"])),
    );
    log.insert(
        "2".into(),
        Value::Object(rec("theme-icons", 0, &["/home/u/.local/share/icons/Other"])),
    );
    let (dir, path) = scratch_log("outcome", Some(&Value::Object(log).to_string()));
    let active_icons = || {
        let mut m = Map::new();
        m.insert("icons".into(), json!("papirus"));
        m
    };

    // Установка не удалась — прежнее состояние остаётся.
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, false, &mut active);
    assert!(!items[0].installed && !items[0].applied);

    // Удалась и тема включена — «Применено».
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, true, &mut active);
    assert!(items[0].installed && items[0].applied);

    // Удалась, но включена другая, а прежний «Применено» — ложь: сбрасываем.
    let mut items = vec![ThemeItem {
        id: "2".into(),
        installed: true,
        applied: true,
        ..Default::default()
    }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, true, &mut active);
    assert!(items[0].installed && !items[0].applied);

    // Журнал не записался (записи нет) — тема на диске есть: «Установлено».
    let mut items = vec![ThemeItem { id: "9".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, true, &mut active);
    assert!(items[0].installed && !items[0].applied);
    let _ = std::fs::remove_dir_all(dir);
}

// ═══════════════════════════════════════════════════════════════════════
//  Загрузчик превью: очередь, приоритет, дедуп, повторы
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::appearance_page::{PreviewQueue, PreviewRetry};

#[test]
fn preview_cache_path_matches_the_original() {
    let _lang = i18n_pin::ru();
    let home = "/home/u";
    // Обычное превью магазина: sha1 URL + расширение из ПУТИ ссылки.
    let p = ap::preview_cache_path(home, "https://images.pling.com/img/00/1.png");
    assert!(p.starts_with("/home/u/.cache/rexon/ocs_previews/"), "{p}");
    assert!(p.ends_with(".png"), "{p}");
    // Расширения нет либо оно неправдоподобно длинное → .jpg.
    assert!(ap::preview_cache_path(home, "https://x/img/00/1").ends_with(".jpg"));
    assert!(ap::preview_cache_path(home, "https://x/a.verylongext").ends_with(".jpg"));
    // Запрос в расширение не попадает.
    assert!(ap::preview_cache_path(home, "https://x/a.png?v=2").ends_with(".png"));
    // OpenGraph GitHub (превью AUR) — свой каталог и всегда PNG.
    let a = ap::preview_cache_path(home, "https://opengraph.githubassets.com/1/u/repo");
    assert!(a.starts_with("/home/u/.cache/rexon/aur_previews/"), "{a}");
    assert!(a.ends_with(".png"), "{a}");
    // Имя файла — то же, что у клиента магазина: второй схемы имён нет.
    assert_eq!(
        ap::preview_cache_path(home, "https://images.pling.com/img/00/1.png"),
        format!(
            "{}/{}",
            rexon_core::appearance_cache::preview_dir(home),
            rexon_core::ocs::preview_cache_name("https://images.pling.com/img/00/1.png")
        )
    );
}

fn urls(jobs: &[rexon_core::appearance_page::PreviewJob]) -> Vec<String> {
    jobs.iter().map(|j| j.url.clone()).collect()
}

#[test]
fn live_previews_go_ahead_of_the_warm_up() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    // Сперва фон, потом живые — живые обязаны встать впереди.
    q.want("c1", "l1", true, None, false);
    q.want("c2", "l2", true, None, false);
    q.want("c3", "a1", false, None, true);
    q.want("c4", "a2", false, None, true);
    let started = q.pump();
    assert_eq!(urls(&started), vec!["a1", "a2", "l1", "l2"]);
}

#[test]
fn warm_up_never_takes_every_slot() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    for i in 0..10 {
        q.want("c", &format!("low{i}"), true, None, false);
    }
    let started = q.pump();
    // Фону отдано ровно два слота из шести — остальные ждут живых плиток.
    assert_eq!(started.len(), 2);
    assert_eq!(q.low_inflight(), 2);
    assert_eq!(q.inflight(), 2);
    // Живая заявка получает слот немедленно, несмотря на очередь из фона.
    q.want("live", "hot", false, None, true);
    let more = q.pump();
    assert_eq!(urls(&more), vec!["hot"]);
    // Ответ фоновой заявки освобождает фоновый слот.
    q.finished("low0", true);
    let after = q.pump();
    assert_eq!(after.len(), 1);
    assert!(after[0].low);
}

#[test]
fn one_url_is_downloaded_once_but_only_for_the_warm_up() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    q.want("c1", "same", true, None, false);
    q.want("c2", "same", true, None, false);
    assert_eq!(q.queued(), 1, "фоновый дубль обязан отсеяться");
    // Живые заявки по одному URL НЕ схлопываются: две карточки изредка делят
    // ссылку, и второй иначе некому послать «готово».
    let mut q = PreviewQueue::new();
    q.want("c1", "same", false, None, true);
    q.want("c2", "same", false, None, true);
    assert_eq!(q.queued(), 2);
}

#[test]
fn cache_hit_skips_the_network() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    q.want("c1", "u1", false, Some("/cache/u1.png"), true);
    assert_eq!(q.queued(), 0);
    assert_eq!(q.take_ready(), Some(("c1".into(), "/cache/u1.png".into())));
    // Фоновая заявка кешируется молча — «готово» наверх не идёт.
    q.want("c2", "u2", true, Some("/cache/u2.png"), false);
    assert_eq!(q.queued(), 0);
    assert!(!q.has_ready());
}

#[test]
fn ready_previews_are_handed_out_one_at_a_time() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    q.want("c1", "u1", false, None, true);
    q.pump();
    q.succeeded("c1", "u1", false, "/cache/u1.png");
    assert!(q.has_ready());
    assert_eq!(q.take_ready(), Some(("c1".into(), "/cache/u1.png".into())));
    assert_eq!(q.take_ready(), None);
    // Фоновая удача наверх не всплывает.
    q.succeeded("c2", "u2", true, "/cache/u2.png");
    assert!(!q.has_ready());
}

#[test]
fn temporary_failures_retry_and_permanent_ones_do_not() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    // Обрыв соединения (кода HTTP нет) — две добавочные попытки с ростом паузы.
    assert_eq!(
        q.failed("u", None),
        PreviewRetry::Again { delay_ms: 700, attempt: 1 }
    );
    assert_eq!(
        q.failed("u", None),
        PreviewRetry::Again { delay_ms: 1400, attempt: 2 }
    );
    assert_eq!(
        q.failed("u", None),
        PreviewRetry::GiveUp { fatal: false, attempts: 3 }
    );
    // Файла нет — повторять бессмысленно с первого же ответа.
    for code in [400u16, 401, 403, 404, 405, 410, 451] {
        let mut q = PreviewQueue::new();
        assert_eq!(
            q.failed("u", Some(code)),
            PreviewRetry::GiveUp { fatal: true, attempts: 1 },
            "HTTP {code}"
        );
    }
    // 5xx и таймаут — временные.
    let mut q = PreviewQueue::new();
    assert!(matches!(q.failed("u", Some(503)), PreviewRetry::Again { .. }));
}

#[test]
fn body_that_is_empty_or_absurdly_large_is_a_failure() {
    let _lang = i18n_pin::ru();
    assert!(!PreviewQueue::body_is_image(0));
    assert!(PreviewQueue::body_is_image(1));
    assert!(PreviewQueue::body_is_image(20 * 1024 * 1024));
    assert!(!PreviewQueue::body_is_image(20 * 1024 * 1024 + 1));
}

#[test]
fn switching_category_throws_away_delayed_retries() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    q.want("c1", "u1", false, None, true);
    q.pump();
    let gen = q.generation();
    q.finished("u1", false);
    assert!(matches!(q.failed("u1", None), PreviewRetry::Again { .. }));
    // Человек ушёл в другую категорию — отложенный повтор больше не нужен.
    q.cancel_all();
    assert!(!q.requeue(gen, "c1", "u1", false));
    assert_eq!(q.queued(), 0);
    // Повтор своего поколения возвращается в очередь.
    let gen = q.generation();
    assert!(q.requeue(gen, "c1", "u1", false));
    assert_eq!(q.queued(), 1);
    // Ту же картинку уже просят заново — второй заявки не плодим.
    assert!(!q.requeue(gen, "c1", "u1", false));
    assert_eq!(q.queued(), 1);
}

#[test]
fn deduplication_is_released_when_the_answer_arrives() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    q.want("c1", "u", true, None, false);
    q.pump();
    // Пока картинка летит, повторная фоновая просьба второго запроса не даёт.
    q.want("c2", "u", true, None, false);
    assert_eq!(q.queued(), 0);
    q.finished("u", true);
    // Ответ пришёл — ключ свободен, следующая просьба снова законна.
    q.want("c3", "u", true, None, false);
    assert_eq!(q.queued(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
//  Состояние каталога
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::appearance_page::CatalogView;
use rexon_core::categories;
use rexon_core::models::ThemeCategory;

fn cat(key: &str) -> ThemeCategory {
    categories::get_category(Path::new("/home/u"), key)
        .unwrap_or_else(|| panic!("нет раздела {key}"))
}

fn view(key: &str) -> CatalogView {
    CatalogView::new(cat(key), "kde")
}

/// Раздел с магазином: у разделов с установленным он идёт под ним на той
/// же странице, отдельно открывать его нечем.
fn store(key: &str) -> CatalogView {
    view(key)
}

/// Источник «vsthemes» снят 04.10.2026. Кто выбрал его раньше, попадает в
/// KDE Store молча: полка, а не пустой экран, и чип KDE Store отмечен.
#[test]
fn a_removed_vsthemes_choice_falls_back_to_the_kde_store() {
    let _lang = i18n_pin::ru();
    for key in ["cursor", "wallpapers", "global"] {
        let mut v = view(key);
        v.source = "vsthemes".to_string();
        assert_eq!(v.effective_source(), "ocs", "раздел {key}");
        assert!(!v.aur_mode() && !v.both_mode(), "раздел {key}");
        assert_eq!(v.screen(Path::new("/home/u")), ap::SCREEN_GRID, "раздел {key}");
        assert!(v.chrome().search, "раздел {key}: поиск магазина на месте");
        assert!(
            ap::source_chips_for(key).iter().all(|c| c.key != "vsthemes"),
            "раздел {key}: чип VSThemes остался"
        );
    }
}

#[test]
fn catalog_starts_on_new_and_on_the_kde_store() {
    let _lang = i18n_pin::ru();
    let v = view("global");
    assert_eq!(v.sort, "new");
    assert_eq!(v.source, "ocs");
    assert_eq!(v.lock_mode, "wallpaper");
    assert_eq!(v.page, 0);
    assert!(v.style_sub.is_none());
}

#[test]
fn source_switch_hides_where_there_is_nothing_to_switch() {
    let _lang = i18n_pin::ru();
    // Локальные обзоры диска сети не знают.
    for key in ["active", "downloaded", "system", "favorites"] {
        let v = view(key);
        assert!(v.is_local(), "{key}");
        assert!(!v.source_applicable(), "{key}");
        assert!(!v.aur_mode() && !v.both_mode(), "{key}");
    }
    // Обычная сетевая категория — переключатель на месте.
    let mut v = store("global");
    assert!(v.source_applicable());
    v.source = "aur".into();
    assert!(v.aur_mode() && !v.both_mode());
    v.source = "both".into();
    assert!(v.both_mode() && !v.aur_mode());
    // Категория-зонт: онлайн-каталог есть, AUR-источника нет.
    let v = view("splash");
    assert!(v.category.ocs_cats().len() > 1, "у заставки должен быть зонт");
    assert!(!v.source_applicable());
}

#[test]
fn lock_mode_picks_the_real_store_category() {
    let _lang = i18n_pin::ru();
    let mut v = view("lockscreen");
    assert_eq!(v.query_category_id(), "299"); // обои блокировки — умолчание
    v.lock_mode = "themes".into();
    assert_eq!(v.query_category_id(), "722"); // полные темы из Global Themes
}

#[test]
fn umbrella_category_asks_the_store_for_several_ids_at_once() {
    let _lang = i18n_pin::ru();
    let v = view("splash");
    let want = v
        .category
        .ocs_cats()
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert!(want.contains(','), "зонт из одной категории — не зонт: {want}");
    assert_eq!(v.query_category_id(), want);
}

#[test]
fn virtual_categories_use_the_aggregate_and_a_fixed_sort() {
    let _lang = i18n_pin::ru();
    let agg = categories::AGGREGATE_CATS.map(|c| c.to_string()).join(",");
    for key in ["popular", "new"] {
        let v = view(key);
        assert_eq!(v.query_category_id(), agg, "{key}: в KDE — все ходовые");
    }
    let mut v = view("popular");
    v.sort = "alpha".into(); // выбор человека виртуальные не перебивают
    assert_eq!(v.active_sort(), "high");
    let mut v = view("new");
    v.sort = "alpha".into();
    assert_eq!(v.active_sort(), "new");
    // Обычная категория слушается выбора.
    let mut v = view("global");
    v.sort = "alpha".into();
    assert_eq!(v.active_sort(), "alpha");
}

#[test]
fn chrome_matches_the_original() {
    let _lang = i18n_pin::ru();
    // Обычная сетевая категория.
    let c = store("global").chrome();
    assert!(c.search && c.source_bar && c.sort_combo && c.pagination);
    assert!(!c.style_bar && !c.lock_bar);
    assert_eq!(c.search_placeholder, "🔍 Поиск тем…");
    // Чистый AUR: сортировка закреплена по голосам — комбо прячем.
    let mut v = store("global");
    v.source = "aur".into();
    let c = v.chrome();
    assert!(!c.sort_combo && c.pagination);
    assert_eq!(c.search_placeholder, "🔍 Поиск в AUR…");
    assert_eq!(v.search_placeholder(), "🔍 Поиск в AUR…");
    // «Все»: комбо остаётся — оно правит OCS-частью списка.
    let mut v = store("global");
    v.source = "both".into();
    assert!(v.chrome().sort_combo);
    // Локальный обзор: ни сортировки, ни страниц.
    let c = view("downloaded").chrome();
    assert!(c.search && !c.sort_combo && !c.pagination && !c.source_bar);
    // Под-чипы только в своих разделах.
    assert!(view("widgetstyle").chrome().style_bar);
    assert!(view("lockscreen").chrome().lock_bar);
    assert!(!view("global").chrome().style_bar);
}

/// Каждый фильтр сетки установленного, под которым магазина нет: раздел и
/// как его включить.
fn installed_only_filters() -> Vec<(&'static str, fn(&mut CatalogView))> {
    vec![
        ("colors", |v| v.tone_filter = "light".into()),
        ("colors", |v| v.tone_filter = "dark".into()),
        ("plasma", |v| v.tone_filter = "dark".into()),
        ("plasma", |v| v.tone_filter = "follows".into()),
        ("widgetstyle", |v| v.style_sub = Some("qt".into())),
        ("widgetstyle", |v| v.style_sub = Some("kvantum".into())),
        ("conky", |v| v.running_only = true),
    ]
}

#[test]
fn an_installed_only_filter_drops_the_whole_store() {
    let _lang = i18n_pin::ru();
    // Одно правило на все фильтры сетки установленного (живая проверка
    // 01.11.2026: под «Kvantum» оставались «Найдено 337» и «1 / 17»).
    for (key, set) in installed_only_filters() {
        let mut v = store(key);
        let open = v.chrome();
        assert!(!v.installed_only(), "{key}: без фильтра магазин есть");
        assert!(open.pagination && open.sort_combo, "{key}");
        let open_key = v.request_key();
        v.source = "aur".into();
        set(&mut v);
        assert!(v.installed_only(), "{key}");
        let c = v.chrome();
        assert!(!c.pagination && !c.sort_combo && !c.source_bar, "{key}: магазина нет — нет и его шапки");
        assert!(c.search, "{key}: поиск по установленному остаётся");
        assert_eq!(c.search_placeholder, "🔍 Поиск среди установленных…", "{key}");
        assert_eq!(v.search_delay_ms(), 0, "{key}: ищем по диску — без паузы");
        let rk = v.request_key();
        assert!(rk.starts_with("installed:"), "{key}: {rk}");
        assert_ne!(rk, open_key, "{key}: свой ключ, не слить с запросом магазина");
        // Страница магазина в ключ не входит — листать нечего.
        v.page = 3;
        assert_eq!(v.request_key(), rk, "{key}");
    }
    // «Все» у каждого фильтра — магазин на месте.
    let mut v = store("widgetstyle");
    v.style_sub = Some("all".into());
    assert!(!v.installed_only() && v.chrome().pagination);
    let mut v = store("colors");
    v.tone_filter = "all".into();
    assert!(!v.installed_only());
    // Фильтр чужого раздела не действует: «С поддержкой цветовых схем»
    // есть только у стилей Plasma, «Запущенные» — только у Conky.
    let mut v = store("colors");
    v.tone_filter = "follows".into();
    assert!(!v.installed_only());
    let mut v = store("global");
    v.running_only = true;
    v.style_sub = Some("qt".into());
    assert!(!v.installed_only() && v.chrome().pagination);
}

#[test]
fn a_section_foreign_to_the_desktop_hides_the_whole_chrome() {
    let _lang = i18n_pin::ru();
    // Плазмоиды в GNOME не работают — раздел закрыт целиком.
    let mut v = view("plasmoids");
    v.de = "gnome".into();
    assert!(v.de_blocked(), "раздел обязан быть закрыт в GNOME");
    let c = v.chrome();
    assert_eq!(c, Default::default());
    assert!(!c.search && !c.pagination && !c.source_bar && !c.sort_combo);
}

#[test]
fn pagination_counts_by_the_requested_page_size() {
    let _lang = i18n_pin::ru();
    // Магазин листает по запрошенным двадцати, даже если прислал 18:
    // 318 работ — 16 страниц, а не 318 / 18 = 18 (две пустые в конце).
    let p = ap::pagination(0, 318, 18);
    assert_eq!(p.label, "Страница 1 / 16");
    assert!(!p.prev_enabled && p.next_enabled);
    let p = ap::pagination(15, 318, 18);
    assert_eq!(p.label, "Страница 16 / 16");
    assert!(p.prev_enabled && !p.next_enabled);
    // Пусто — одна страница, листать некуда.
    let p = ap::pagination(0, 0, 0);
    assert_eq!(p.label, "Страница 1 / 1");
    assert!(!p.prev_enabled && !p.next_enabled);
    // Ответ пуст, а всего работ много: «Вперёд» не должен звать в никуда.
    assert!(!ap::pagination(0, 100, 0).next_enabled);
}

#[test]
fn prefetch_and_the_forward_button_agree_on_the_page_count() {
    let _lang = i18n_pin::ru();
    // Кнопка «Вперёд» и прогрев обязаны считать страницы одинаково.
    for page in 0..15 {
        assert_eq!(
            ap::has_page_to_prefetch(page, 318, 18),
            ap::pagination(page, 318, 18).next_enabled,
            "страница {page}"
        );
    }
    assert!(!ap::has_page_to_prefetch(15, 318, 18));
    assert!(!ap::has_page_to_prefetch(0, 0, 0));
}

/// Поддельный магазин: серверные страницы заданной длины, работы пронумерованы
/// по порядку потока. Считает, какие страницы у него спросили.
struct FakeStore {
    lens: Vec<usize>,
    total: i64,
    asked: Vec<i64>,
}

impl FakeStore {
    fn new(lens: &[usize], total: i64) -> Self {
        FakeStore { lens: lens.to_vec(), total, asked: Vec::new() }
    }
    fn fetch(&mut self, k: i64) -> (Vec<usize>, i64) {
        self.asked.push(k);
        let Some(&len) = self.lens.get(k as usize) else {
            return (Vec::new(), self.total);
        };
        let first: usize = self.lens[..k as usize].iter().sum();
        ((first..first + len).collect(), self.total)
    }
    fn page(&mut self, page: i64) -> ap::StreamPage<usize> {
        ap::stream_page(page, ap::PAGE_SIZE, |k| self.fetch(k))
    }
}

#[test]
fn store_pages_are_topped_up_to_twenty_without_repeats() {
    // Живой обход Global Themes 03.10.2026: 18, 20 × 14, 18 при totalitems 318.
    let mut lens = vec![18];
    lens.extend([20; 14]);
    lens.push(18);
    let visible: usize = lens.iter().sum(); // 316
    let mut store = FakeStore::new(&lens, 318);
    let mut all = Vec::new();
    let mut page = 0;
    loop {
        let p = store.page(page);
        assert_eq!(p.total, 318);
        if p.has_more {
            assert_eq!(p.items.len(), 20, "страница {page} с дырой");
        }
        all.extend(p.items.iter().copied());
        if !p.has_more {
            break;
        }
        page += 1;
    }
    // Поток прочитан целиком, подряд, без повторов и пропусков.
    assert_eq!(all, (0..visible).collect::<Vec<_>>());
    // 316 видимых — 15 полных страниц и последняя из 16.
    assert_eq!(page, 15);
    assert_eq!(store.page(15).items.len(), 16);
}

#[test]
fn store_page_needs_only_the_pages_it_covers() {
    // Дыры 18/20/20/18: первая страница Rexon добирает 2 работы со второй
    // серверной (и одну сверху — узнать, есть ли следующая).
    let mut store = FakeStore::new(&[18, 20, 20, 18], 80);
    let p = store.page(0);
    assert_eq!(p.items, (0..20).collect::<Vec<_>>());
    assert!(p.has_more);
    assert_eq!(store.asked, vec![0, 1]);
    // Вторая начинается ровно там, где кончилась первая.
    let p = store.page(1);
    assert_eq!(p.items, (20..40).collect::<Vec<_>>());
    assert!(p.has_more);
    // Последняя: в потоке 76 работ — 16 на четвёртой странице, дальше пусто.
    let p = store.page(3);
    assert_eq!(p.items, (60..76).collect::<Vec<_>>());
    assert!(!p.has_more);
    // Число страниц по факту кончившегося потока: 4, не 80 / 20 = 4 + пустая.
    let pg = ap::stream_pagination(3, p.total, p.has_more);
    assert!(!pg.next_enabled && pg.prev_enabled);
}

#[test]
fn store_page_with_a_fully_hidden_server_page_keeps_going() {
    // Целая серверная страница скрыта (пришло 0 при живом totalitems) —
    // это не конец потока: конец считается по totalitems.
    let mut store = FakeStore::new(&[20, 0, 20], 60);
    let p = store.page(1);
    assert_eq!(p.items, (20..40).collect::<Vec<_>>());
    assert!(!p.has_more);
    assert_eq!(store.asked, vec![0, 1, 2]);
}

#[test]
fn empty_store_answer_is_one_empty_page() {
    let _lang = i18n_pin::ru();
    let mut store = FakeStore::new(&[], 0);
    let p = store.page(0);
    assert!(p.items.is_empty() && !p.has_more && p.total == 0);
    assert_eq!(store.asked, vec![0]);
    let pg = ap::stream_pagination(0, 0, false);
    assert_eq!(pg.label, "Страница 1 / 1");
    assert!(!pg.prev_enabled && !pg.next_enabled);
}

#[test]
fn store_failure_mid_stream_does_not_kill_the_forward_button() {
    // Нулевая страница пришла, первая — сеть упала: по видимым 18 «Вперёд»
    // не гасим, судим по totalitems, как до добора.
    let p = ap::stream_page(0, ap::PAGE_SIZE, |k| {
        if k == 0 {
            ((0..18).collect::<Vec<usize>>(), 318)
        } else {
            (Vec::new(), 0)
        }
    });
    assert_eq!(p.items.len(), 18);
    assert!(p.has_more);
}

#[test]
fn stream_pagination_counts_by_total_until_the_stream_ends() {
    let _lang = i18n_pin::ru();
    // Пока поток идёт — страниц по totalitems.
    let p = ap::stream_pagination(0, 318, true);
    assert_eq!(p.label, "Страница 1 / 16");
    assert!(!p.prev_enabled && p.next_enabled);
    // Поток кончился раньше счёта магазина — пустую последнюю не обещаем.
    let p = ap::stream_pagination(14, 318, false);
    assert_eq!(p.label, "Страница 15 / 15");
    assert!(p.prev_enabled && !p.next_enabled);
    // Магазин недосчитал (новые работы сдвинули поток) — страниц не меньше
    // той, на которой стоим, плюс следующая.
    assert_eq!(ap::stream_pagination(16, 318, true).label, "Страница 17 / 18");
}

#[test]
fn sort_change_is_a_new_stream_from_page_zero() {
    // Поток зависит от сортировки: смена сортировки (страница сбрасывается
    // в 0) читает свои серверные страницы, а не хвост прежней.
    let mut newest = FakeStore::new(&[18, 20], 40);
    let mut rating = FakeStore::new(&[20, 19], 40);
    assert_eq!(newest.page(0).items.len(), 20);
    let r = rating.page(0);
    assert_eq!(r.items, (0..20).collect::<Vec<_>>());
    assert_eq!(rating.asked, vec![0, 1]);
}

#[test]
fn aur_pages_are_cut_locally() {
    let _lang = i18n_pin::ru();
    // RPC отдаёт весь список разом — страницы режем на месте по 20.
    assert_eq!(ap::aur_page_range(50, 0), 0..20);
    assert_eq!(ap::aur_page_range(50, 1), 20..40);
    assert_eq!(ap::aur_page_range(50, 2), 40..50);
    assert_eq!(ap::aur_page_range(50, 9), 50..50);
    assert_eq!(ap::aur_page_range(0, 0), 0..0);
    // Свежий список короче — страницу подтягиваем к последней существующей.
    assert_eq!(ap::aur_clamp_page(50, 7), 2);
    assert_eq!(ap::aur_clamp_page(0, 7), 0);
    assert_eq!(ap::aur_clamp_page(21, 0), 0);
    // Листание за конец и до нуля запрещено.
    assert!(!ap::aur_can_change_page(50, 0, -1));
    assert!(ap::aur_can_change_page(50, 0, 1));
    assert!(!ap::aur_can_change_page(50, 2, 1));
    assert!(ap::aur_can_change_page(50, 2, -1));
}

#[test]
fn aur_search_needs_at_least_two_characters() {
    let _lang = i18n_pin::ru();
    assert!(!ap::aur_query_too_short(""));       // пусто — это подборка, не поиск
    assert!(ap::aur_query_too_short("a"));
    assert!(ap::aur_query_too_short("я"));       // считаем символы, не байты
    assert!(!ap::aur_query_too_short("ab"));
}

#[test]
fn aur_cache_key_separates_search_from_the_section() {
    let _lang = i18n_pin::ru();
    let mut v = view("global");
    assert_eq!(v.aur_cache_key(), "cat:global");
    v.query = "nord".into();
    assert_eq!(v.aur_cache_key(), "search:nord");
}

#[test]
fn typed_word_reaches_the_query() {
    let _lang = i18n_pin::ru();
    // Живая проверка 06.09.2026: набранное слово терялось по дороге, запрос
    // оставался пустым, и «Активные» показывали все темы подряд.
    let mut v = view("active");
    assert_eq!(v.edit_query("  "), ap::QueryEdit::Idle);
    assert_eq!(v.query, "");
    assert_eq!(v.edit_query("bree"), ap::QueryEdit::Kept);
    assert_eq!(v.query, "bree");                 // ← этой записи и не было
    assert_eq!(v.edit_query(" bree "), ap::QueryEdit::Idle);
    v.page = 3;
    assert_eq!(v.edit_query(""), ap::QueryEdit::Cleared);
    assert_eq!(v.query, "");
    assert_eq!(v.page, 0);                       // сброс поиска — с первой
    assert_eq!(v.edit_query(""), ap::QueryEdit::Idle);
}

/// 19.10.2026: поиск в разделах срабатывал только по Enter, а забытое
/// в поле слово («MishMty») оставляло в разделе одну плитку — казалось,
/// что магазин пропал. Теперь поиск идёт по мере набора: обзор диска —
/// сразу, магазин и раздел «установленное + магазин» — после паузы; строка
/// состояния говорит, что действует поиск.
#[test]
fn search_runs_while_typing_and_says_so() {
    let _lang = i18n_pin::ru();
    // Обзор диска сети не знает — отбор сразу.
    let local = view("active");
    assert_eq!(local.search_delay_ms(), 0);
    // Магазин (и установленное над ним) — посимвольно не дёргаем.
    let store = view("global");
    assert_eq!(store.search_delay_ms(), ap::SEARCH_DEBOUNCE_MS);
    assert_eq!(ap::SEARCH_DEBOUNCE_MS, 300);

    assert_eq!(ap::search_status("MishMty", 1), "Поиск: «MishMty» — найдено 1");
    assert_eq!(ap::search_status("MishMty", 0), "Поиск: «MishMty» — ничего не найдено");
    assert_eq!(ap::search_clear_tooltip(), "Очистить поиск");

    // Страница: Enter применяет сразу, набор — по паузе ядра, у поля
    // кнопка очистки.
    let qml = include_str!("../../rexon-gui/qml/AppearancePage.qml");
    assert!(qml.contains("page.searchDelayMs"), "пауза поиска — из ядра");
    assert!(qml.contains("page.searchClearTooltip"), "у поля поиска кнопка «Очистить»");
}

#[test]
fn request_key_carries_the_query_in_every_section() {
    let _lang = i18n_pin::ru();
    // Ключом сверяется ответ и по нему же отбрасывается дубль. Без запроса
    // в ключе поиск по обзору диска выглядел для дедупа тем же самым
    // запросом, и в сетку принимался ответ НЕотфильтрованного скана.
    let mut local = view("active");
    let plain = local.request_key();
    local.query = "bree".into();
    assert_ne!(local.request_key(), plain);
    assert!(local.request_key().contains("bree"), "{}", local.request_key());

    let mut store = view("global");
    let plain = store.request_key();
    store.query = "bree".into();
    assert_ne!(store.request_key(), plain);

    let mut aur = view("global");
    aur.source = "aur".into();
    let plain = aur.request_key();
    aur.query = "bree".into();
    assert_ne!(aur.request_key(), plain);
}

#[test]
fn local_matches_is_one_rule_for_every_disk_theme() {
    let _lang = i18n_pin::ru();
    assert!(ap::local_matches("Breeze", "bree"));
    assert!(ap::local_matches("Breeze Dark", " BREE "));
    assert!(!ap::local_matches("Nordic", "bree"));
    assert!(ap::local_matches("что угодно", "   "));   // пустой запрос не режет
}

#[test]
fn local_filter_matches_the_original() {
    let _lang = i18n_pin::ru();
    let items: Vec<ThemeItem> = ["Nord", "Breeze Dark", "тёмная тема", ""]
        .iter()
        .map(|n| ThemeItem { name: n.to_string(), ..Default::default() })
        .collect();
    assert_eq!(ap::local_filter(&items, "").len(), 4);
    assert_eq!(ap::local_filter(&items, "  ").len(), 4);
    assert_eq!(ap::local_filter(&items, "DARK").len(), 1);
    assert_eq!(ap::local_filter(&items, "ТЁМ").len(), 1);
    assert_eq!(ap::local_filter(&items, "нет такого").len(), 0);
}

/// Тема с диска находится и по показанному (переведённому) имени, и по
/// оригинальному: имени без перевода и каталогу из номера карточки.
/// Живая проверка r1434 (16.10.2026): в «GTK-темах» «Adwaita» и
/// «HighContrast» давали «Ничего не найдено», а «Высоко» находило.
#[test]
fn local_item_matches_shown_and_original_names() {
    let _lang = i18n_pin::ru();
    let adwaita = ThemeItem {
        id: "local:gtk:Adwaita".into(),
        name: "Адвайта".into(),
        ..Default::default()
    };
    let contrast = ThemeItem {
        id: "local:gtk:HighContrast".into(),
        name: "Высококонтрастная".into(),
        source_name: "High Contrast".into(),
        ..Default::default()
    };
    // Карточка магазина: номер — число, каталога в нём нет.
    let store = ThemeItem { id: "1234567".into(), name: "Nordic".into(), ..Default::default() };
    for q in ["Adwaita", "adwaita", "  ADWAITA ", "адвайта", "Адв"] {
        assert!(ap::local_item_matches(&adwaita, q), "Adwaita по {q:?}");
    }
    for q in ["HighContrast", "highcontrast", "High Contrast", "high contrast", "Высоко", "contrast"] {
        assert!(ap::local_item_matches(&contrast, q), "HighContrast по {q:?}");
    }
    assert!(!ap::local_item_matches(&adwaita, "contrast"));
    assert!(!ap::local_item_matches(&contrast, "adwaita"));
    assert!(ap::local_item_matches(&store, "nord"));
    assert!(!ap::local_item_matches(&store, "1234"), "номер магазина — не имя");
    assert!(ap::local_item_matches(&adwaita, "   "), "пустой запрос не режет");
    let items = vec![adwaita, contrast, store];
    let ids = |q: &str| -> Vec<String> {
        ap::local_filter(&items, q).into_iter().map(|it| it.id).collect()
    };
    assert_eq!(ids("Adwaita"), ["local:gtk:Adwaita"]);
    assert_eq!(ids("HighContrast"), ["local:gtk:HighContrast"]);
    assert_eq!(ids("Высоко"), ["local:gtk:HighContrast"]);
}

#[test]
fn scan_status_and_kind_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::scan_status("active", 7).unwrap(), "Активных тем: 7");
    assert_eq!(ap::scan_status("downloaded", 0).unwrap(), "Загруженных тем: 0");
    assert_eq!(ap::scan_status("system", 126).unwrap(), "Системных тем: 126");
    assert!(ap::scan_status("splash", 3).is_none()); // подпись берётся с раздела

    assert_eq!(ap::scan_local_kind("active", ""), "active");
    assert_eq!(ap::scan_local_kind("downloaded", ""), "downloaded");
    assert_eq!(ap::scan_local_kind("system", ""), "downloaded");
    // Прочие сканы узнают активный элемент по подписи автора.
    assert_eq!(ap::scan_local_kind("splash", "🎨 Заставка · ✅ Активна"), "active");
    assert_eq!(ap::scan_local_kind("splash", "🎨 Заставка"), "downloaded");
}

#[test]
fn a_stale_scan_answer_is_dropped() {
    let _lang = i18n_pin::ru();
    assert!(ap::scan_answer_stale("system", "downloaded"));
    assert!(!ap::scan_answer_stale("system", "system"));
    // Скан «Стиля приложений» несёт под-выбор и снимается своим счётчиком.
    assert!(!ap::scan_answer_stale("widgetstyle:kvantum", "widgetstyle"));
    assert!(!ap::scan_answer_stale("widgetstyle:qt", "global"));
}

#[test]
fn local_scan_which_carries_the_style_filter() {
    let _lang = i18n_pin::ru();
    assert_eq!(view("splash").local_scan_which(), "splash");
    assert_eq!(view("downloaded").local_scan_which(), "downloaded");
    let mut v = view("widgetstyle");
    assert_eq!(v.local_scan_which(), "widgetstyle:all", "по умолчанию — всё вместе");
    v.style_sub = Some("qt".into());
    assert_eq!(v.local_scan_which(), "widgetstyle:qt");
    v.style_sub = Some("kvantum".into());
    assert_eq!(v.local_scan_which(), "widgetstyle:kvantum");
    // Прежний под-выбор «один стиль» больше не сужает сетку до карточки.
    v.style_sub = Some("qt:Breeze".into());
    assert_eq!(v.local_scan_which(), "widgetstyle:all");
}

#[test]
fn local_scan_plan_reads_back_what_local_scan_which_wrote() {
    let _lang = i18n_pin::ru();
    // Ключ собирает `local_scan_which`, разбирает — `local_scan_plan`:
    // формат один, и знать его должен ОДИН хозяин. Прежде разбор жил у
    // исполнителя вкладки на Python второй копией.
    for key in ["active", "downloaded", "system", "splash", "lockscreen"] {
        assert_eq!(ap::local_scan_plan(key), (key.to_string(), String::new()));
    }

    let mut v = view("widgetstyle");
    for filter in ["all", "qt", "kvantum"] {
        v.style_sub = Some(filter.into());
        assert_eq!(
            ap::local_scan_plan(&v.local_scan_which()),
            ("widgetstyle".to_string(), filter.to_string())
        );
    }
    assert_eq!(ap::local_scan_plan("widgetstyle"), ("widgetstyle".to_string(), "all".to_string()));

    // Разделы KDE с сеткой установленного — скан раздела, как у окружений.
    for key in ["global", "plasma", "colors", "aurorae", "icons", "cursor", "gtk", "sounds"] {
        assert_eq!(ap::local_scan_plan(key), ("section".to_string(), key.to_string()));
    }

    // Раздел без скана диска — показывать нечего, и это НЕ «активные».
    assert_eq!(ap::local_scan_plan("wallpapers"), (String::new(), String::new()));
    assert_eq!(ap::local_scan_plan(""), (String::new(), String::new()));
}

#[test]
fn lock_stamping_leaves_local_cards_alone() {
    let _lang = i18n_pin::ru();
    let mut items = vec![
        ThemeItem { id: "12345".into(), category_id: 722, ..Default::default() },
        ThemeItem {
            id: "local:lockscreen:Nord".into(),
            category_id: 903,
            ..Default::default()
        },
    ];
    ap::stamp_lock_kind(&mut items, "themes");
    assert_eq!(items[0].category_id, 903);
    assert_eq!(items[1].category_id, 903, "местную карточку не трогаем");
    let mut items = vec![ThemeItem { id: "9".into(), category_id: 299, ..Default::default() }];
    ap::stamp_lock_kind(&mut items, "wallpaper");
    assert_eq!(items[0].category_id, 904);
}

#[test]
fn popular_and_new_in_mate_ask_only_for_what_works_there() {
    let _lang = i18n_pin::ru();
    for key in ["popular", "new"] {
        let v = CatalogView::new(cat(key), "mate");
        let q = v.query_category_id();
        let ids: Vec<&str> = q.split(',').collect();
        // Стили Plasma в MATE не применяются.
        assert!(!ids.contains(&"104"), "{key}: {q}");
        // Значки, курсоры, GTK-темы и обои — действуют; глобальные темы
        // (01.11.2026) — по частям: цвета, стиль, значки, курсоры, шрифты.
        for want in ["722", "132", "107", "135", "299"] {
            assert!(ids.contains(&want), "{key}: нет {want} в {q}");
        }
        for id in &ids {
            let c = categories::category_by_id(Path::new("/home/u"), id.parse().unwrap()).unwrap();
            assert!(categories::category_available(&c.key, "mate"), "{key}: {id} не для MATE");
        }
    }
    // Прогрев спрашивает то же, что живой запрос, и не греет разделы KDE.
    let home = Path::new("/home/u");
    let sidebar = categories::sidebar(home);
    let v = CatalogView::new(cat("popular"), "mate");
    let targets = ap::prefetch_targets(&v, &sidebar);
    let q = v.query_category_id();
    assert!(targets.iter().any(|(id, s)| *id == q && s == "high"), "{targets:?}");
    assert!(targets.iter().all(|(id, _)| id != "104" && id != "114"), "{targets:?}");
}

#[test]
fn prefetch_skips_everything_that_has_no_store() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let sidebar = categories::sidebar(home);
    let v = view("global");
    let targets = ap::prefetch_targets(&v, &sidebar);
    assert!(!targets.is_empty());
    for (cat_id, _) in &targets {
        for one in cat_id.split(',') {
            let n: i64 = one.parse().unwrap();
            assert!(n > 0 && n <= 900, "негодная цель прогрева {cat_id}");
        }
    }
    // Дублей нет: «Популярные» и «Global» дают одну и ту же пару.
    let mut seen = std::collections::BTreeSet::new();
    for k in &targets {
        assert!(seen.insert(k.clone()), "дубль в очереди прогрева: {k:?}");
    }
    // Ни одного локального обзора и ни одного раздела без магазина.
    let networkless: Vec<i64> = sidebar
        .iter()
        .filter(|c| c.local_only() || c.id > 900)
        .map(|c| c.id)
        .collect();
    for id in networkless {
        assert!(!targets.iter().any(|(c, _)| *c == id.to_string()), "прогрев зонта {id}");
    }
}

#[test]
fn neighbours_of_the_open_section_go_to_the_front() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let sidebar = categories::sidebar(home);
    let first_net = sidebar.iter().find(|c| c.id > 0 && c.id <= 900 && !c.local_only()).unwrap();
    let v = CatalogView::new(first_net.clone(), "kde");
    let done = std::collections::BTreeSet::new();
    let front = ap::prefetch_neighbours(&v, &sidebar, &done);
    assert!(!front.is_empty());
    // Сам открытый раздел в соседи не попадает.
    assert!(!front.iter().any(|(c, _)| *c == first_net.id.to_string()));
    // Перестановка очереди: соседи впереди, остальное следом и без дублей.
    let queue = vec![("999".to_string(), "new".to_string()), front[0].clone()];
    let out = ap::prefetch_reorder(&front, &queue);
    assert_eq!(&out[..front.len()], &front[..]);
    assert_eq!(out.iter().filter(|k| **k == front[0]).count(), 1);
    assert!(out.contains(&("999".to_string(), "new".to_string())));
}

#[test]
fn the_running_desktop_wins_but_a_manual_choice_survives() {
    let _lang = i18n_pin::ru();
    // Первый запуск: ничего не знаем — берём запущенный и запоминаем его.
    assert_eq!(
        ap::initial_de("gnome", "", ""),
        ap::DeChoice {
            de_id: "gnome".into(),
            remember: Some(("gnome".into(), "gnome".into()))
        }
    );
    // Человек сидит в том же сеансе, где выбирал руками, — его выбор в силе.
    assert_eq!(
        ap::initial_de("kde", "gnome", "kde"),
        ap::DeChoice { de_id: "gnome".into(), remember: None }
    );
    // Сменил рабочий стол — выбор был унаследован, а не сделан: каталог идёт
    // за фактическим столом.
    assert_eq!(
        ap::initial_de("gnome", "kde", "kde"),
        ap::DeChoice {
            de_id: "gnome".into(),
            remember: Some(("gnome".into(), "gnome".into()))
        }
    );
    // Стол не определяется, но выбор был — держим выбор.
    assert_eq!(
        ap::initial_de("", "gnome", "kde"),
        ap::DeChoice { de_id: "gnome".into(), remember: None }
    );
    // Не известно вообще ничего — самый широкий набор разделов.
    assert_eq!(
        ap::initial_de("", "", ""),
        ap::DeChoice { de_id: "kde".into(), remember: None }
    );
}

/// 19.10.2026: в сеансе MATE «Текущее окружение» стояло на «KDE Plasma» —
/// выбор, сделанный руками ночью, — и ничто на экране не говорило, что это
/// не определённый по сеансу стол. Ручной выбор, отличный от сеанса,
/// подписан как ручной, а первым пунктом стоит «Определять автоматически».
#[test]
fn a_manual_desktop_choice_is_labelled_and_can_be_reset() {
    let _lang = i18n_pin::ru();
    let name = |list: &[(String, String)], id: &str| {
        list.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()).unwrap_or_default()
    };
    let manual = ap::de_picker_choices("kde", "mate");
    assert_eq!(manual[0], (ap::DE_AUTO_ID.to_string(), "Определять автоматически (MATE)".to_string()));
    assert_eq!(name(&manual, "kde"), "KDE Plasma (выбрано вручную, сейчас MATE)");
    assert_eq!(name(&manual, "mate"), "MATE");

    // Выбран стол сеанса — подписывать нечего, сбрасывать тоже.
    let auto = ap::de_picker_choices("mate", "mate");
    assert!(auto.iter().all(|(id, _)| id != ap::DE_AUTO_ID));
    assert_eq!(name(&auto, "kde"), "KDE Plasma");
    assert_eq!(auto, rexon_core::categories::appearance_de_choices("mate"));

    // Сеанс не опознан — сравнивать не с чем.
    assert!(ap::de_picker_choices("kde", "").iter().all(|(id, _)| id != ap::DE_AUTO_ID));

    // Что делать по выбору пункта: (стол каталога, что запомнить как выбор).
    // «Автоматически» — стол сеанса и пустой выбор: `initial_de` дальше
    // сам идёт за сеансом.
    assert_eq!(ap::resolve_de_pick(ap::DE_AUTO_ID, "mate"), ("mate".to_string(), String::new()));
    assert_eq!(ap::resolve_de_pick("kde", "mate"), ("kde".to_string(), "kde".to_string()));
    // Страница узнаёт пункт «автоматически» по тому же имени.
    let qml = include_str!("../../rexon-gui/qml/AppearancePage.qml");
    assert!(qml.contains(&format!("picked === \"{}\"", ap::DE_AUTO_ID)));
    assert_eq!(
        ap::initial_de("mate", "", "mate"),
        ap::DeChoice { de_id: "mate".into(), remember: None }
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Панель деталей
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::appearance_page::GalleryFrame;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn gallery_drops_the_face_of_the_product_like_the_site_does() {
    let _lang = i18n_pin::ru();
    let full = |u: &str| rexon_core::ocs::full_preview_url(u);
    let urls: Vec<String> = ["a.png", "b.png", "c.png"].iter().map(|s| s.to_string()).collect();
    // Первый слот — «лицо товара», в карусель он не попадает.
    assert_eq!(
        ap::gallery_urls(&urls, "face.png"),
        vec![full("b.png"), full("c.png")]
    );
    // Картинка одна — показываем её.
    assert_eq!(ap::gallery_urls(&["only.png".into()], ""), vec![full("only.png")]);
    // Списка нет вовсе — берём превью плитки.
    assert_eq!(ap::gallery_urls(&[], "p.png"), vec![full("p.png")]);
    assert!(ap::gallery_urls(&[], "").is_empty());
}

#[test]
fn tile_falls_back_to_the_next_picture_when_the_first_is_dead() {
    // «Sirin Cursor» (2370501), ответ магазина 03.10.2026: previewpic1 —
    // GIF с иероглифами, сервер отвечает 404; остальные — уменьшенные копии.
    let base = "https://images.pling.com/img/00/00/86/48/60/2370501";
    let cache = "https://images.pling.com/cache/770x540-4/img/00/00/86/48/60/2370501";
    let pic1 = format!("{base}/6880de79-438b-40c4-87b5-365b8d186afb-展示图2.gif");
    let urls: Vec<String> = vec![
        pic1.clone(),
        format!("{cache}/sxz.png"),
        format!("{cache}/dsda.png"),
        format!("{cache}/nsa.png"),
    ];
    let c = ap::preview_candidates(&pic1, &urls);
    // Сначала превью плитки, потом остальные по номерам, без дублей,
    // последним запасом — оригиналы уменьшенных копий.
    assert_eq!(
        c,
        vec![
            pic1.clone(),
            format!("{cache}/sxz.png"),
            format!("{cache}/dsda.png"),
            format!("{cache}/nsa.png"),
            format!("{base}/sxz.png"),
            format!("{base}/dsda.png"),
            format!("{base}/nsa.png"),
        ]
    );
    assert_eq!(ap::next_preview_candidate(&c, &pic1), Some(format!("{cache}/sxz.png")));
    assert_eq!(ap::next_preview_candidate(&c, &format!("{base}/nsa.png")), None);
    assert_eq!(ap::next_preview_candidate(&c, "чужая"), None);
    // Картинка у товара одна — запаса нет.
    let one = ap::preview_candidates("only.png", &["only.png".into()]);
    assert_eq!(one, vec!["only.png".to_string()]);
    assert!(ap::preview_candidates("", &[]).is_empty());
}

#[test]
fn error_page_instead_of_picture_is_not_a_picture() {
    assert!(ap::looks_like_image(b"\x89PNG\r\n\x1a\n...."));
    assert!(ap::looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
    assert!(ap::looks_like_image(b"GIF89a...."));
    assert!(ap::looks_like_image(b"RIFF\x10\x00\x00\x00WEBPVP8 "));
    assert!(ap::looks_like_image(b"\x00\x00\x00\x1cftypavif"));
    assert!(ap::looks_like_image(b"  <svg xmlns=\"http://www.w3.org/2000/svg\">"));
    assert!(ap::looks_like_image(b"<?xml version=\"1.0\"?><svg>"));
    assert!(!ap::looks_like_image(b"<!DOCTYPE html><html><body>Bad request"));
    assert!(!ap::looks_like_image(b"Bad request [asESvo9H]"));
    assert!(!ap::looks_like_image(b""));
}

#[test]
fn gallery_skips_broken_frames() {
    let failed: BTreeSet<i64> = [1, 2].into_iter().collect();
    // 0 1 2 3: битые 1 и 2 при листании пропускаются в обе стороны.
    assert_eq!(ap::gallery_step_skipping(0, 4, 1, &failed), 3);
    assert_eq!(ap::gallery_step_skipping(3, 4, -1, &failed), 0);
    assert_eq!(ap::gallery_step_skipping(3, 4, 1, &failed), 0);
    // Счётчик считает только живые: «2/2» на третьем кадре из четырёх.
    assert_eq!(ap::gallery_counter_skipping(3, 4, &failed).as_deref(), Some("2/2"));
    assert_eq!(ap::gallery_counter_skipping(0, 4, &failed).as_deref(), Some("1/2"));
    // Живой остался один — листать некуда, счётчика нет.
    let all_but_0: BTreeSet<i64> = [1, 2, 3].into_iter().collect();
    assert_eq!(ap::gallery_step_skipping(0, 4, 1, &all_but_0), 0);
    assert_eq!(ap::gallery_counter_skipping(0, 4, &all_but_0), None);
    // Без битых — обычное закольцованное листание.
    assert_eq!(ap::gallery_step_skipping(3, 4, 1, &BTreeSet::new()), 0);
}

#[test]
fn gallery_arrows_wrap_around() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::gallery_step(0, 5, 1), 1);
    assert_eq!(ap::gallery_step(4, 5, 1), 0);
    assert_eq!(ap::gallery_step(0, 5, -1), 4);
    // Кадр один — листать нечего.
    assert_eq!(ap::gallery_step(0, 1, 1), 0);
    assert_eq!(ap::gallery_step(0, 0, -1), 0);
}

#[test]
fn gallery_counter_only_grows() {
    let _lang = i18n_pin::ru();
    // Полные детали приносят больше ссылок, чем список: «2/5» не должно
    // схлопнуться обратно в «2/1».
    assert_eq!(ap::gallery_total(5, 2, 0), 5);
    assert_eq!(ap::gallery_total(1, 5, 0), 5);
    assert_eq!(ap::gallery_total(0, 0, 3), 3);
    assert_eq!(ap::gallery_counter(1, 5).unwrap(), "2/5");
    assert!(ap::gallery_counter(0, 1).is_none());
}

#[test]
fn a_frame_that_will_not_come_says_so_instead_of_spinning() {
    let _lang = i18n_pin::ru();
    let mut loaded = BTreeMap::new();
    loaded.insert(0i64, "/c/0.png".to_string());
    let failed: BTreeSet<i64> = [2i64].into_iter().collect();
    // Кадр есть.
    assert_eq!(
        ap::gallery_frame(0, &loaded, &failed, false, 5),
        GalleryFrame::Image("/c/0.png".into())
    );
    // Кадра нет, обход ещё идёт — ждём.
    assert_eq!(ap::gallery_frame(1, &loaded, &failed, false, 5), GalleryFrame::Loading);
    // Сервер отказал именно по этому кадру.
    assert_eq!(ap::gallery_frame(2, &loaded, &failed, false, 5), GalleryFrame::Failed);
    // Обход закончен — «дыра» больше не заполнится.
    assert_eq!(ap::gallery_frame(1, &loaded, &failed, true, 5), GalleryFrame::Failed);
    // Одиночная картинка — панель остаётся как была.
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &failed, true, 1),
        GalleryFrame::Unchanged
    );
    // Картинок нет вовсе и обход закончен — «нет превью», а не вечное
    // ожидание.
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &BTreeSet::new(), true, 0),
        GalleryFrame::Unavailable
    );
    // Обход ещё идёт — пока ждём.
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &BTreeSet::new(), false, 0),
        GalleryFrame::Unchanged
    );
    // Единственный кадр не доехал.
    let only_failed: BTreeSet<i64> = [0i64].into_iter().collect();
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &only_failed, false, 1),
        GalleryFrame::Unavailable
    );
}

#[test]
fn ready_frames_from_the_cache_are_picked_up_on_open() {
    let _lang = i18n_pin::ru();
    let mut loaded = BTreeMap::new();
    loaded.insert(2i64, "/c/2.png".to_string());
    loaded.insert(4i64, "/c/4.png".to_string());
    // Текущего кадра среди готовых нет — встаём на самый ранний готовый.
    assert_eq!(ap::adopt_gallery_index(&loaded, 0, false), Some(2));
    // Человек уже листал руками — его кадр не перебиваем.
    assert_eq!(ap::adopt_gallery_index(&loaded, 0, true), None);
    // Текущий кадр готов — двигать некуда.
    assert_eq!(ap::adopt_gallery_index(&loaded, 2, false), None);
    assert_eq!(ap::adopt_gallery_index(&BTreeMap::new(), 0, false), None);
}

#[test]
fn install_button_has_three_states() {
    let _lang = i18n_pin::ru();
    // Как в KDE: «Установить» только ставит, применяет — следующее нажатие.
    let b = ap::install_button(false, false, true);
    assert_eq!(b.text, "Установить");
    assert_eq!(b.state, "false");
    assert!(b.tooltip.is_empty());
    let b = ap::install_button(true, false, true);
    assert_eq!(b.text, "🎨 Применить");
    assert_eq!(b.state, "installed");
    assert!(!b.tooltip.is_empty());
    // Вид без применения (шрифты, виджеты) — только «Установлено».
    let b = ap::install_button(true, false, false);
    assert_eq!(b.text, "✅ Установлено");
    assert_eq!(b.state, "installed");
    let b = ap::install_button(true, true, true);
    assert_eq!(b.text, "✅ Применено");
    assert_eq!(b.state, "true");
    // «Применена» перевешивает: тему могли применить, не отметив установленной.
    assert_eq!(ap::install_button(false, true, true).state, "true");
    assert_eq!(ap::status_prefix(true), "✅ ");
    assert_eq!(ap::status_prefix(false), "❌ ");
}

#[test]
fn local_theme_name_comes_from_the_synthetic_id() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::local_theme_name("local:colors:Breeze Dark", "не то"), "Breeze Dark");
    // В имени темы бывает двоеточие — оно должно уцелеть.
    assert_eq!(ap::local_theme_name("local:gtk:A:B", "x"), "A:B");
    // Не местная карточка — обычное название.
    assert_eq!(ap::local_theme_name("123456", "Sweet KDE"), "Sweet KDE");
    // Номер испорчен — тоже название.
    assert_eq!(ap::local_theme_name("local:gtk", "Sweet"), "Sweet");
    assert!(ap::stale_global_msg("Nord").contains("«Nord»"));
}

#[test]
fn detail_size_and_copy_link_labels() {
    let _lang = i18n_pin::ru();
    // `downloadsize1` магазина — в КиБ: живые 13 и 1419 (Discover: 13 КиБ, 1,4 МиБ).
    assert_eq!(ap::human_size_kib(13), "13 КиБ");
    assert_eq!(ap::human_size_kib(1419), "1,4 МиБ");
    assert_eq!(ap::human_size_kib(3 * 1024 * 1024 / 2), "1,5 ГиБ");
    assert_eq!(ap::human_size_kib(0), "");
    assert_eq!(ap::human_size_kib(-5), "");
    assert_eq!(
        ap::detail_meta_with_size(&ap::detail_meta_line("A", "GPL-3.0", "2026-01-01", ""), 13),
        "A • GPL-3.0 • обновлено 2026-01-01 • 13 КиБ"
    );
    assert_eq!(
        ap::detail_meta_with_size(&ap::detail_meta_line("A", "B", "2026-01-01", "1.0"), 1419),
        "A • B • обновлено 2026-01-01 v1.0 • 1,4 МиБ"
    );
    // Размер неизвестен — строка не меняется.
    let plain = ap::detail_meta_line("A", "B", "2026-01-01", "1.0");
    assert_eq!(ap::detail_meta_with_size(&plain, 0), plain);
    assert_eq!(ap::store_page_url("2370521"), "https://store.kde.org/p/2370521");
    assert_eq!(ap::store_page_url(""), "");
    assert_eq!(ap::detail_copy_link_btn(), "🔗 Копировать ссылку");
    assert_eq!(
        ap::link_copied_status("https://store.kde.org/p/1"),
        "Ссылка скопирована: https://store.kde.org/p/1"
    );
}

#[test]
fn animated_preview_is_told_by_its_header() {
    assert!(ap::is_animated_image_header(b"GIF89a\x10\x00"));
    assert!(ap::is_animated_image_header(b"GIF87a"));
    assert!(!ap::is_animated_image_header(b"\x89PNG\r\n\x1a\n"));
    assert!(!ap::is_animated_image_header(b"\xff\xd8\xff\xe0"));
    assert!(!ap::is_animated_image_header(b""));
    let mut webp = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\x02\0\0\0".to_vec();
    assert!(ap::is_animated_image_header(&webp));
    webp[20] = 0; // флага анимации нет — неподвижный WebP
    assert!(!ap::is_animated_image_header(&webp));
}

#[test]
fn detail_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        ap::detail_meta_line("EliverLara", "GPL-3.0", "2026-07-29 03:00:00", "5.0"),
        "EliverLara • GPL-3.0 • обновлено 2026-07-29 v5.0"
    );
    // Магазин отдаёт версию и с буквой — второй «v» приписывать нельзя.
    assert_eq!(
        ap::detail_meta_line("A", "B", "2026-01-01", "v5.0"),
        "A • B • обновлено 2026-01-01 v5.0"
    );
    // Пустые поля — прочерк, версии просто нет.
    assert_eq!(ap::detail_meta_line("", "", "", ""), "— • — • обновлено — ");
    assert_eq!(ap::detail_stars_label(4.35), "⭐ 4.3");
    assert_eq!(ap::detail_stars_label(5.0), "⭐ 5.0"); // не «⭐ 5»
    assert_eq!(ap::detail_stars_label(0.0), "⭐ 0.0");
    assert!(ap::detail_downloads_label(1234).contains("скачиваний: 1.2K"));
    assert_eq!(
        ap::detail_path_label("Глобальные темы", "~/.local/share/plasma"),
        "Категория: Глобальные темы\nПуть установки: ~/.local/share/plasma"
    );
}

#[test]
fn description_and_comment_toggles_match_the_original() {
    let _lang = i18n_pin::ru();
    // Перевода нет — переключателя тоже нет.
    assert_eq!(ap::description_html("ориг", "ориг", false, false, "#66c0f4"), "ориг");
    assert_eq!(ap::description_html("", "", false, false, "#66c0f4"), "<i>Без описания</i>");
    // Перевод есть — показываем перевод и предлагаем оригинал.
    let h = ap::description_html("ориг", "пер", true, false, "#66c0f4");
    assert!(h.starts_with("пер<br><a href='rexon:toggle'"), "{h}");
    assert!(h.contains("color:#66c0f4"), "{h}");
    assert!(h.contains("📄 показать оригинал"), "{h}");
    assert!(h.contains("font-size:9.5pt"), "{h}");
    // Переключили — показываем оригинал и предлагаем перевод.
    let h = ap::description_html("ориг", "пер", true, true, "#66c0f4");
    assert!(h.starts_with("ориг<br>"), "{h}");
    assert!(h.contains("🌐 показать перевод"), "{h}");
    // Комментарий: тот же приём, свои подписи и кегль.
    let h = ap::comment_body_html("o", "t", true, false, "#66c0f4");
    assert!(h.starts_with("t&nbsp;&nbsp;<a href='rexon:toggle'"), "{h}");
    assert!(h.contains("📄 оригинал") && h.contains("font-size:9pt"), "{h}");
    assert_eq!(ap::comment_body_html("o", "t", false, false, "#66c0f4"), "t");
}

/// Второй довод `description_html` — ВИДИМОЕ описание
/// (`ThemeItem::display_description`), а не поле перевода.
///
/// Разница видна только когда перевода НЕТ: панель обязана показать
/// английский текст магазина, а не «Без описания». В Rust-вкладке там стояло
/// `description_translated`, и до появления живого перевода описания не
/// показывались вовсе; у Python на этом месте было
/// `_desc_trans = перевод if был_переведён else оригинал`.
#[test]
fn a_description_without_a_translation_still_shows_the_original() {
    let _lang = i18n_pin::ru();
    use rexon_core::models::ThemeItem;

    let shown = |item: &ThemeItem, show_original: bool| {
        ap::description_html(
            &item.description,
            item.display_description(),
            !item.description_translated.is_empty(),
            show_original,
            "#66c0f4",
        )
    };
    let mut item =
        ThemeItem { description: "The Papirus-Mace theme.".into(), ..Default::default() };
    // Модели перевода нет — текст магазина на месте, переключателя нет.
    assert_eq!(shown(&item, false), "The Papirus-Mace theme.");
    // Перевод доехал — показываем его и предлагаем оригинал.
    item.description_translated = "Тема Papirus-Mace.".into();
    assert!(
        shown(&item, false).starts_with("Тема Papirus-Mace.<br><a href='rexon:toggle'"),
        "{}",
        shown(&item, false)
    );
    // Нажали «показать оригинал» — вернулся английский текст.
    assert!(shown(&item, true).starts_with("The Papirus-Mace theme.<br>"));
    // Описания нет вовсе — заглушка, и по-прежнему без переключателя.
    let empty = ThemeItem::default();
    assert_eq!(shown(&empty, false), ap::no_description_html());
}

#[test]
fn comment_text_is_stripped_and_escaped_like_the_original() {
    let _lang = i18n_pin::ru();
    // Эталон снят с самого Python (html.unescape → вырезать теги → html.escape).
    for (raw, want) in [
        ("<b>жирный</b> текст", " жирный  текст"),
        ("a &amp; b", "a &amp; b"),
        // Уже экранированный тег после unescape становится настоящим и
        // вырезается — иначе разметка из комментария попала бы в подпись.
        ("&lt;script&gt;alert(1)&lt;/script&gt;", " alert(1) "),
        ("стр1\nстр2", "стр1<br>стр2"),
        ("<>пусто", "&lt;&gt;пусто"),
        ("\"кавычки\" и апостроф: it's", "&quot;кавычки&quot; и апостроф: it&#x27;s"),
        ("<a href=\"x\">ссылка</a>", " ссылка "),
        ("", ""),
        ("&#039;&quot;&amp;", "&#x27;&quot;&amp;"),
        ("незакрытый <тег", "незакрытый &lt;тег"),
    ] {
        assert_eq!(ap::clean_comment_text(raw), want, "вход {raw:?}");
    }
}

#[test]
fn author_nicknames_are_compared_leniently() {
    let _lang = i18n_pin::ru();
    for (raw, want) in [
        ("  John  Doe ", "john doe"),
        ("ELIVERLARA", "eliverlara"),
        ("", ""),
        ("  ", ""),
        // casefold оригинала приравнивает «ß» к «ss».
        ("Straße", "strasse"),
    ] {
        assert_eq!(ap::norm_author(raw), want, "вход {raw:?}");
    }
    assert!(ap::comment_is_product_author("EliverLara", "eliverlara"));
    assert!(ap::comment_is_product_author("  John Doe ", "john  doe"));
    // Пустой ник автором товара не считается никогда.
    assert!(!ap::comment_is_product_author("", ""));
    assert!(!ap::comment_is_product_author("someone", "author"));
}

#[test]
fn comment_shape_matches_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::comment_author_display(""), "—");
    assert_eq!(ap::comment_author_display("nick"), "nick");
    assert_eq!(ap::comment_date("2026-07-29 03:00:00"), "2026-07-29");
    assert_eq!(ap::comment_date(""), "");
    // Глубина с потолком: глубже пятого уровня отступ съел бы всю ширину.
    assert_eq!(ap::comment_depth(9), 4);
    assert_eq!(ap::comment_depth(-1), 0);
    assert_eq!(ap::comment_indent(0), 0);
    assert_eq!(ap::comment_indent(3), 84);
    assert_eq!(ap::comment_indent(9), 112);
    assert_eq!(ap::comment_reply_prefix(0), "");
    assert_eq!(ap::comment_reply_prefix(1), "↪️ ");
}

#[test]
fn comments_header_and_paging_match_the_original() {
    let _lang = i18n_pin::ru();
    // Точное число пришло с деталями товара.
    assert_eq!(ap::comments_header_text(Some(42), 10, true), "💬 Комментарии (42)");
    // Из кеша число теряется — честное «показано N(+)».
    assert_eq!(ap::comments_header_text(None, 10, true), "💬 Комментарии (10+)");
    assert_eq!(ap::comments_header_text(None, 3, false), "💬 Комментарии (3)");
    assert_eq!(ap::comments_header_text(Some(0), 3, false), "💬 Комментарии (3)");
    // «Есть ещё» считается по КОРНЕВЫМ: ответы страницу не заполняют.
    let roots: Vec<i64> = vec![0; 10];
    assert!(ap::comments_has_more(&roots));
    let mixed: Vec<i64> = vec![0, 1, 1, 0, 2, 0, 0, 0, 0, 0, 1, 1];
    assert!(!ap::comments_has_more(&mixed), "корневых меньше страницы");
    assert!(!ap::comments_has_more(&[]));
}

// ═══════════════════════════════════════════════════════════════════════
//  Жизненный цикл запросов каталога
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::appearance_page::{CatalogRequests, FireOutcome, SuggestAction};

#[test]
fn a_second_request_for_the_same_page_never_leaves_for_the_network() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    let g = r.bump();
    r.schedule(g, "cat=722&p=0");
    assert_eq!(
        r.fire(),
        FireOutcome::Send { key: "cat=722&p=0".into(), generation: g }
    );
    // Человек вернулся в ту же категорию, пока запрос ещё летит.
    let g2 = r.bump();
    r.schedule(g2, "cat=722&p=0");
    assert_eq!(r.fire(), FireOutcome::Deduplicated { key: "cat=722&p=0".into() });
    // Ответ летящего принимается как свежий: ему обновили поколение.
    assert!(!r.answer("cat=722&p=0"));
}

#[test]
fn an_answer_from_a_category_already_left_is_dropped() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    let g = r.bump();
    r.schedule(g, "a");
    assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    r.bump(); // ушёл в другую категорию
    assert!(r.answer("a"), "ответ прежней категории обязан быть выброшен");
    // Ответа, которого никто не ждёт (попадание в кеш), это не касается.
    assert!(!r.answer("никогда-не-летел"));
}

#[test]
fn a_debounced_request_dies_if_the_user_moved_on() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    let g = r.bump();
    r.schedule(g, "a");
    r.bump();
    assert_eq!(r.fire(), FireOutcome::Nothing, "bump снимает отложенный запрос");
    // И даже поданный вручную устаревшим поколением не уходит.
    r.schedule(g, "a");
    assert_eq!(r.fire(), FireOutcome::Stale);
    assert_eq!(r.fire(), FireOutcome::Nothing);
}

#[test]
fn leaving_a_category_tears_down_everything_but_the_new_request() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    for key in ["a", "b", "c"] {
        let g = r.bump();
        r.schedule(g, key);
        assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    }
    let mut keys = r.stale_keys(Some("c"));
    keys.sort();
    assert_eq!(keys, vec!["a", "b"]);
    assert_eq!(r.stale_keys(None).len(), 3);
}

#[test]
fn each_page_is_warmed_at_most_once() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    assert!(r.claim_page_prefetch("p1"));
    assert!(!r.claim_page_prefetch("p1"), "второй прогрев той же страницы");
    // Живой запрос человека за страницу тоже считается прогревом.
    let g = r.bump();
    r.schedule(g, "p2");
    assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    assert!(!r.claim_page_prefetch("p2"));
    // Сброс кеша списков вручную разрешает греть заново.
    r.forget_page_prefetches();
    assert!(r.claim_page_prefetch("p1"));
}

#[test]
fn the_warm_up_yields_the_channel_to_a_live_request() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    r.enqueue_prefetch(&[("722".into(), "new".into()), ("104".into(), "new".into())]);
    assert_eq!(r.prefetch_queue_len(), 2);
    // Пока открытая категория ещё грузится, фон не запускаем.
    let g = r.bump();
    r.schedule(g, "live");
    assert_eq!(r.next_prefetch(), None, "отложенный запрос уже занимает канал");
    assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    assert_eq!(r.next_prefetch(), None, "запрос в полёте");
    // Ответ пришёл — очередь догоняет.
    r.answer("live");
    let first = r.next_prefetch().expect("очередь должна тронуться");
    // По одной за раз: пока идёт этот прогрев, следующий не выдаём.
    assert_eq!(r.next_prefetch(), None);
    r.prefetch_finished(first.clone());
    let second = r.next_prefetch().expect("следующая цель");
    assert_ne!(first, second);
    r.prefetch_finished(second);
    assert_eq!(r.next_prefetch(), None);
    // Уже прогретое в очередь не возвращается.
    r.enqueue_prefetch(&[("722".into(), "new".into())]);
    assert_eq!(r.prefetch_queue_len(), 0);
}

#[test]
fn closing_the_tab_stops_handing_out_warm_ups() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    r.enqueue_prefetch(&[("722".into(), "new".into())]);
    r.close();
    assert_eq!(r.next_prefetch(), None);
}

#[test]
fn neighbours_jump_to_the_head_of_the_warm_up_queue() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    r.enqueue_prefetch(&[("1".into(), "new".into()), ("2".into(), "new".into()), ("3".into(), "new".into())]);
    r.prioritize_prefetch(&[("3".into(), "new".into())]);
    assert_eq!(r.next_prefetch(), Some(("3".into(), "new".into())));
    // Дубля не осталось: переставили, а не добавили.
    r.prefetch_finished(("3".into(), "new".into()));
    assert_eq!(r.next_prefetch(), Some(("1".into(), "new".into())));
}

#[test]
fn suggestions_do_not_pester_the_store_letter_by_letter() {
    let _lang = i18n_pin::ru();
    let v = store("global");
    assert_eq!(ap::suggest_action("", true, &v), SuggestAction::Hide);
    assert_eq!(ap::suggest_action("n", true, &v), SuggestAction::Hide);
    assert_eq!(ap::suggest_action("  n  ", true, &v), SuggestAction::Hide);
    assert_eq!(ap::suggest_action("no", true, &v), SuggestAction::Network);
    // На панели деталей подсказок нет — там нет и сетки, к которой они ведут.
    assert_eq!(ap::suggest_action("nord", false, &v), SuggestAction::Hide);
    // Разделы без сети фильтруют уже загруженный список на месте.
    assert_eq!(ap::suggest_action("nord", true, &view("downloaded")), SuggestAction::Local);
    let mut v = store("global");
    v.source = "aur".into();
    assert_eq!(ap::suggest_action("nord", true, &v), SuggestAction::Local);
}

#[test]
fn a_stale_suggestion_answer_is_dropped() {
    let _lang = i18n_pin::ru();
    assert!(ap::suggest_answer_fresh(5, 5, true, "nord"));
    // Человек печатал дальше.
    assert!(!ap::suggest_answer_fresh(4, 5, true, "nord"));
    // Ушёл из строки поиска.
    assert!(!ap::suggest_answer_fresh(5, 5, false, "nord"));
    // Стёр введённое.
    assert!(!ap::suggest_answer_fresh(5, 5, true, "n"));
}

#[test]
fn catalog_status_texts_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::catalog_status(153, 10), "Найдено: 153 · показано 10");
    assert_eq!(ap::catalog_status(0, 0), "Ничего не найдено");
    assert_eq!(ap::catalog_status(153, 0), "Ничего не найдено");
    assert_eq!(ap::favorites_status(3), "В избранном: 3");
    assert_eq!(ap::local_category_status("Заставка KDE", 5), "Заставка KDE: 5");
    assert_eq!(ap::scan_busy_status("system"), "🖥️ Поиск системных тем…");
    assert_eq!(ap::scan_busy_status("downloaded"), "⏳ Загрузка…");
}

// ═══════════════════════════════════════════════════════════════════════
//  Плитки каталога
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn card_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::card_name(""), "—");
    assert_eq!(ap::card_name("Sweet KDE"), "Sweet KDE");
    assert_eq!(ap::card_author(""), "—");
    // Слоты стат-строки: рейтинг · скачивания · дата (день без времени).
    let nb = "\u{a0}";
    assert_eq!(
        ap::card_stats_line(4.4, 123456, "2026-07-29 03:00:00"),
        format!("{nb}4.4{nb}{nb}⬇️ 123.5K{nb}{nb}📅 2026-07-29")
    );
    // Даты нет — слот просто отсутствует, «—» вместо неё не рисуем.
    assert_eq!(
        ap::card_stats_line(5.0, 0, ""),
        format!("{nb}5.0{nb}{nb}⬇️ 0")
    );
    assert_eq!(
        ap::card_updated_tooltip("2026-07-29 03:00:00").unwrap(),
        "Обновлено: 2026-07-29"
    );
    assert!(ap::card_updated_tooltip("").is_none());
}

#[test]
fn card_button_has_the_same_three_states_as_the_detail_panel() {
    let _lang = i18n_pin::ru();
    let b = ap::card_install_button(false, false, true);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("Установить", "false", true));
    // Поставлена, но не включена — та же кнопка теперь «Применить».
    let b = ap::card_install_button(true, false, true);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("🎨 Применить", "true", true));
    let b = ap::card_install_button(true, false, false);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("✅ Установлено", "true", true));
    let b = ap::card_install_button(true, true, true);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("✅ Применено", "applied", true));
    // Что делает нажатие: не стоит — только поставить; стоит и применяется —
    // применить поставленное (не качая заново); стоит, но применять нечего —
    // поставить заново.
    assert_eq!(ap::store_card_action(false, true), ap::StoreCardAction::Install);
    assert_eq!(ap::store_card_action(true, true), ap::StoreCardAction::ApplyInstalled);
    assert_eq!(ap::store_card_action(true, false), ap::StoreCardAction::Install);
    // Местная плитка: включённая тема показывает бейдж и не нажимается.
    let b = ap::card_local_button("active");
    assert_eq!((b.text.as_str(), b.enabled), ("✅ Активна", false));
    let b = ap::card_local_button("downloaded");
    assert_eq!((b.text.as_str(), b.enabled), ("🎨 Применить", true));
    assert_eq!(ap::card_local_button("system").text, "🎨 Применить");
    // Избранное.
    assert_eq!(ap::favorite_button(true).text, "⭐ Избранное");
    assert_eq!(ap::favorite_button(false).text, "☆ Избранное");
    assert_eq!(ap::favorite_button(true).state, "on");
}

#[test]
fn aur_card_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    let nb = "\u{a0}";
    assert_eq!(ap::aur_meta_line("1.2.3-1", false), "🛠️ AUR · 1.2.3-1");
    assert_eq!(ap::aur_meta_line("1.2.3-1", true), "🛠️ AUR · 1.2.3-1  ⚠️ устарел");
    assert_eq!(ap::aur_meta_line("", false), "🛠️ AUR · —");
    assert_eq!(ap::aur_version_tooltip(""), "Версия: —");
    // Ширина версии: остаток от подписи, но не меньше читаемого минимума.
    assert_eq!(ap::aur_version_budget(270, 100), 170);
    assert_eq!(ap::aur_version_budget(270, 500), 40);
    // Стат-строка — те же слоты, что у плитки магазина.
    assert_eq!(
        ap::aur_stats_line(1234, 3.456, "2026-07-29"),
        format!("{nb}1.2K{nb}{nb}📈 3.5{nb}{nb}📅 2026-07-29")
    );
    // Нулевая популярность и без даты — слоты не рисуются.
    assert_eq!(ap::aur_stats_line(7, 0.0, ""), format!("{nb}7"));
    assert_eq!(
        ap::aur_stats_tooltip(1234, 3.456, "2026-07-29"),
        "👍 голосов: 1234 · 📈 популярность AUR 3.46 · обновлён 2026-07-29"
    );
    assert!(ap::aur_stats_tooltip(0, 0.0, "").contains("обновлён —"));
}

#[test]
fn aur_button_explains_why_it_cannot_install() {
    let _lang = i18n_pin::ru();
    let b = ap::aur_install_button(false, true);
    assert!(b.enabled && b.text == "Установить");
    assert!(b.tooltip.contains("Сборка из исходников"));
    // Помощника нет — говорим почему, а не оставляем мёртвую кнопку молча.
    let b = ap::aur_install_button(false, false);
    assert!(!b.enabled);
    assert_eq!(
        b.tooltip,
        "Не найден сборщик пакетов AUR (paru или yay) — установка недоступна"
    );
    // Покупателю — без жаргона (правило #13): «AUR-хелпер» заменён словами,
    // которыми тот же отказ уже говорит пересборка оформления.
    for tooltip in [
        ap::aur_install_button(false, false).tooltip,
        ap::aur_install_button(false, true).tooltip,
        ap::aur_install_button(true, true).tooltip,
        ap::aur_busy_button(true).tooltip,
        ap::aur_busy_button(false).tooltip,
    ] {
        for jargon in ["хелпер", "helper"] {
            assert!(!tooltip.contains(jargon), "в подсказке покупателю «{jargon}»: {tooltip}");
        }
    }
    // Уже стоит.
    let b = ap::aur_install_button(true, true);
    assert!(!b.enabled && b.text == "✅ Установлен");
    // Сборка идёт.
    assert_eq!(ap::aur_busy_button(true).text, "⏳ Сборка…");
    assert!(!ap::aur_busy_button(true).enabled);
    assert!(ap::aur_busy_button(false).enabled);
}

// ═══════════════════════════════════════════════════════════════════════
//  Шапка вкладки и накладные контролы
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chrome_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::page_title(), "Оформление");
    assert_eq!(ap::sidebar_row_text("🎨", "Глобальные темы"), "  🎨  Глобальные темы");
    assert_eq!(ap::section_title("🎨", "Глобальные темы"), "🎨  Глобальные темы");
    // Три источника, ключи совпадают с теми, что различает состояние каталога.
    let keys: Vec<&str> = ap::source_chips().iter().map(|c| c.key).collect();
    assert_eq!(keys, vec!["ocs", "aur", "both"]);
    assert!(ap::source_chips().iter().all(|c| !c.label.is_empty() && !c.tooltip.is_empty()));
    // Два режима блокировки, и оба объяснены — раздел гибридный.
    let keys: Vec<&str> = ap::lock_chips().iter().map(|c| c.key).collect();
    assert_eq!(keys, vec!["themes", "wallpaper"]);
    assert!(ap::lock_chips()[0].tooltip.contains("ТОЛЬКО компонент блокировки"));
}

#[test]
fn application_style_is_one_grid_with_a_filter_and_a_store() {
    let _lang = i18n_pin::ru();
    let styles: Vec<String> = ["Breeze", "Fusion", "kvantum", "Kvantum-dark", "Windows", "qt6ct-style", "qt5ct-style"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Движок Kvantum — его темами, прокси qtNct — не внешний вид.
    assert_eq!(ap::shown_qt_styles(&styles), vec!["Breeze", "Fusion", "Windows"]);
    let keys: Vec<&str> = ap::style_filter_chips().iter().map(|c| c.key).collect();
    assert_eq!(keys, ["all", "qt", "kvantum"]);
    assert_eq!(ap::style_filter_chips()[0].label, "Все");
    // Сетка установленного, фильтр над ней; магазин — темы Kvantum (123) и AUR.
    let v = view("widgetstyle");
    assert!(v.installed_view() && v.marks_current());
    let c = v.chrome();
    assert!(c.style_bar && c.pagination);
    assert_eq!(v.query_category_id(), "123");
    assert!(v.source_applicable(), "темы Kvantum и стили Qt есть в AUR");
    assert!(rexon_core::aur::has_category("widgetstyle"));
    // Тёзки семейств стилей с другим делом в полку не попадают (снимок AUR).
    let pkg = |name: &str, desc: &str| rexon_core::aur::AurPackage {
        name: name.into(),
        description: desc.into(),
        ..Default::default()
    };
    let ok = |n: &str, d: &str| rexon_core::aur::is_relevant("widgetstyle", &pkg(n, d));
    assert!(ok("darkly", "Fork of Lightly (A modern style for Qt applications)"));
    assert!(ok("kvantum", "SVG-based theme engine for Qt"));
    assert!(ok("klassy", "Highly customizable binary Window Decoration, Application Style and Global Theme"));
    assert!(!ok("darkly-gtk-git", "Darkly GTK theme"));
    assert!(!ok("darkly-art-bin", "Forbidden Editor for Artists"));
    assert!(!ok("lightlyshaders-git", "Round corners and outline effect for KWin"));
}

#[test]
fn overlays_hug_the_edges_of_the_picture() {
    let _lang = i18n_pin::ru();
    // Лупа — правый верхний угол, с отступом.
    let p = ap::overlay_zoom_pos(600, 44);
    assert_eq!((p.x, p.y), (600 - 44 - 10, 10));
    // Стрелки — по бокам, по центру вертикали.
    let p = ap::overlay_prev_pos(340, 44);
    assert_eq!((p.x, p.y), (10, (340 - 44) / 2));
    let p = ap::overlay_next_pos(600, 340, 44, 44);
    assert_eq!((p.x, p.y), (600 - 44 - 10, (340 - 44) / 2));
    // Счётчик — низ по центру.
    let p = ap::overlay_counter_pos(600, 340, 48, 24);
    assert_eq!((p.x, p.y), ((600 - 48) / 2, 340 - 24 - 10));
    // Картинка меньше контрола — за левый/верхний край не уезжаем.
    let p = ap::overlay_zoom_pos(20, 44);
    assert_eq!(p.x, 0);
    let p = ap::overlay_counter_pos(20, 20, 48, 24);
    assert_eq!((p.x, p.y), (0, 0));
}

#[test]
fn full_screen_view_shows_only_frames_that_are_really_on_disk() {
    let _lang = i18n_pin::ru();
    let mut loaded = BTreeMap::new();
    loaded.insert(0i64, "/c/0.png".to_string());
    loaded.insert(1i64, "/c/1.png".to_string());
    loaded.insert(2i64, "/c/gone.png".to_string());
    let exists = |p: &str| p != "/c/gone.png";
    // Текущий кадр среди скачанных — листаем и в полноэкранном виде.
    let (paths, idx) = ap::zoom_frames("/c/1.png", &loaded, exists).unwrap();
    assert_eq!(paths, vec!["/c/0.png", "/c/1.png"]);
    assert_eq!(idx, 1);
    // Одиночное превью местной темы: чужих картинок в список не подставляем.
    let (paths, idx) = ap::zoom_frames("/local/preview.png", &loaded, exists).unwrap();
    assert_eq!(paths, vec!["/local/preview.png"]);
    assert_eq!(idx, 0);
    // Показывать нечего.
    assert!(ap::zoom_frames("", &loaded, exists).is_none());
    assert!(ap::zoom_frames("/c/gone.png", &loaded, exists).is_none());
}

/// Сторож согласия таблицы стилей и голых цветов.
///
/// QSS в QML не работает, поэтому у кнопки плитки два вида одного решения:
/// строка для QtWidgets (`page_qss`) и голые цвета для QML
/// (`card_button_colors`). Разъехаться они не должны — сетка и деталь
/// рассказали бы про товар разное.
#[test]
fn widget_qss_and_qml_button_colors_agree() {
    let _lang = i18n_pin::ru();
    let qss = ap::page_qss(&ap::colors());

    // «✓ Применено» — залитая бирюзовая.
    let (bg, fg, border) = ap::card_button_colors("applied");
    assert_eq!((bg, fg, border), ("#2f9e94", "#ffffff", "#2f9e94"));
    assert!(qss.contains("background: #2f9e94;"));
    assert!(qss.contains("border: 1px solid #2f9e94;"));

    // «✓ Установлено» — контурная бирюзовая: заливки нет вовсе.
    let (bg, fg, border) = ap::card_button_colors("true");
    assert_eq!((bg, fg, border), ("", "#7fe3da", "#4fd1c5"));
    assert!(qss.contains("background: transparent;"));
    assert!(qss.contains("border: 1px solid #4fd1c5;"));
    assert!(qss.contains("color: #7fe3da;"));

    // «Установить» — цвет действующего скина, а не свой хардкод: имя токена
    // обязано существовать в палитре, иначе кнопка осталась бы бесцветной.
    let (bg, fg, border) = ap::card_button_colors("false");
    assert_eq!((fg, border), ("#ffffff", ""));
    assert!(ap::colors().contains_key(bg), "нет токена {bg} в палитре");
}

/// Наименьший размер окна вкладки — тот же, что у соседних вкладок.
#[test]
fn window_minimum_is_the_product_wide_one() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::WINDOW_MIN_SIZE, (900, 720));
    assert_eq!(ap::PAGE_TITLE_POINT_SIZE, 15);
}

/// Подписи панели деталей выписаны из оригинала ДОСЛОВНО.
#[test]
fn detail_button_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    // ⚠️ Два пробела после стрелки — так в оригинале.
    assert_eq!(ap::detail_back_btn(), "←  Назад к каталогу");
    assert_eq!(ap::detail_uninstall_btn(), "🗑️ Удалить");
    assert_eq!(ap::detail_open_site_btn(), "🌐 Открыть на сайте");
    assert_eq!(
        ap::detail_open_site_tooltip(),
        "Официальная страница на pling.com (описание, комментарии)"
    );
    assert_eq!(ap::detail_apply_local_btn(), "🎨 Применить");
    assert_eq!(
        ap::detail_preview_tooltip(),
        "Нажмите на картинку, чтобы увеличить во весь экран"
    );
    assert_eq!(ap::detail_preview_loading(), "Загрузка превью…");
    assert_eq!(ap::detail_preview_unavailable(), "Превью недоступно");
    assert_eq!(ap::OVERLAY_ZOOM_BTN, "🔍");
    assert_eq!(ap::overlay_zoom_tooltip(), "Увеличить на весь экран");
    assert_eq!(ap::OVERLAY_PREV_BTN, "◀️");
    assert_eq!(ap::overlay_prev_tooltip(), "Предыдущий скриншот");
    assert_eq!(ap::OVERLAY_NEXT_BTN, "▶️");
    assert_eq!(ap::overlay_next_tooltip(), "Следующий скриншот");
    assert_eq!(ap::comments_more_btn(), "Показать ещё");
}

/// Сторож согласия таблицы стилей и голых цветов главной кнопки деталей.
#[test]
fn widget_qss_and_qml_detail_button_colors_agree() {
    let _lang = i18n_pin::ru();
    let qss = ap::page_qss(&ap::colors());

    let (bg, fg, border) = ap::detail_button_colors("true");
    assert_eq!((bg, fg, border), ("#3a5a3f", "#cfe8d2", ""));
    assert!(qss.contains("background: #3a5a3f;"));
    assert!(qss.contains("color: #cfe8d2;"));

    // «✓ Установлено» на деталях — тот же контурный вид, что у плитки.
    assert_eq!(ap::detail_button_colors("installed"), ap::card_button_colors("true"));

    let (bg, fg, border) = ap::detail_button_colors("false");
    assert_eq!((fg, border), ("#ffffff", ""));
    assert!(ap::colors().contains_key(bg), "нет токена {bg} в палитре");
}

// ═══════════════════════════════════════════════════════════════════════
//  Ход установки
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn install_progress_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::install_preparing(), "⬇️ Подготовка…");
    assert_eq!(
        ap::action_busy_status(ap::ACTION_INSTALL, "Nordic"),
        "⬇️ Установка «Nordic»…"
    );
    assert_eq!(
        ap::action_busy_status(ap::ACTION_AUR, "aerothemeplasma"),
        "🔨 Сборка «aerothemeplasma» из AUR…"
    );
    assert_eq!(
        ap::action_busy_status(ap::ACTION_APPLY, "Breeze"),
        "🎨 Применение «Breeze»…"
    );
}

/// Долю выполненного отдаёт только загрузка из магазина: у сборки помощником
/// AUR и у применения темы её нет, и полоса на нуле врала бы про ход.
#[test]
fn only_the_store_download_shows_a_progress_bar() {
    let _lang = i18n_pin::ru();
    assert!(ap::action_shows_progress(ap::ACTION_INSTALL));
    assert!(!ap::action_shows_progress(ap::ACTION_AUR));
    assert!(!ap::action_shows_progress(ap::ACTION_APPLY));
}

/// Полоса хода обязана оставаться полосой: установщик изредка отдаёт и
/// больше ста, и меньше нуля.
#[test]
fn install_percent_stays_inside_the_bar() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::install_percent(-5), 0);
    assert_eq!(ap::install_percent(0), 0);
    assert_eq!(ap::install_percent(37), 37);
    assert_eq!(ap::install_percent(100), 100);
    assert_eq!(ap::install_percent(140), 100);
}

/// Второго значка у готового итога быть не должно: установщик отдаёт
/// сообщения со своим, и «✅ ✅ Установлено» было дефектом оригинала.
#[test]
fn the_result_never_gets_a_second_check_mark() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::action_result_status(true, "Готово"), "✅ Готово");
    assert_eq!(ap::action_result_status(false, "Не вышло"), "❌ Не вышло");
    assert_eq!(
        ap::action_result_status(true, "✅ Установлено: Nordic"),
        "✅ Установлено: Nordic"
    );
    assert_eq!(
        ap::action_result_status(false, "❌ Не удалось применить"),
        "❌ Не удалось применить"
    );
    // Отступ впереди значка сообщение не портит.
    assert_eq!(ap::action_result_status(true, "  ✅ Готово"), "  ✅ Готово");
}

/// Сетку пересобирает только удавшееся применение темы: бейдж «✅ Активна»
/// обязан переехать. Установка и неудача её не трогают.
#[test]
fn only_a_successful_apply_rebuilds_the_grid() {
    let _lang = i18n_pin::ru();
    assert!(ap::reload_after_action(ap::ACTION_APPLY, true));
    assert!(!ap::reload_after_action(ap::ACTION_APPLY, false));
    assert!(!ap::reload_after_action(ap::ACTION_INSTALL, true));
    assert!(!ap::reload_after_action(ap::ACTION_AUR, true));
}

// ═══════════════════════════════════════════════════════════════════════
//  Контролы, перекрытые таблицей стилей вкладки
// ═══════════════════════════════════════════════════════════════════════

/// Тело правила таблицы стилей по его селектору.
fn qss_rule<'a>(qss: &'a str, selector: &str) -> &'a str {
    let head = format!("\n{selector} {{");
    let start = qss
        .find(&head)
        .unwrap_or_else(|| panic!("в таблице стилей нет правила {selector}"))
        + head.len();
    let end = start
        + qss[start..]
            .find('}')
            .unwrap_or_else(|| panic!("правило {selector} не закрыто"));
    &qss[start..end]
}

/// Значение свойства внутри тела правила либо `None`, если его там нет.
fn qss_value_opt<'a>(body: &'a str, prop: &str) -> Option<&'a str> {
    let head = format!("{prop}:");
    let start = body.find(&head)? + head.len();
    let end = start
        + body[start..]
            .find(';')
            .unwrap_or_else(|| panic!("свойство {prop} не закрыто"));
    Some(body[start..end].trim())
}

/// То же, но свойство обязано быть.
fn qss_value<'a>(body: &'a str, prop: &str) -> &'a str {
    qss_value_opt(body, prop).unwrap_or_else(|| panic!("в правиле нет свойства {prop}"))
}

/// Сторож согласия таблицы стилей и голых красок КОНТРОЛОВ вкладки.
///
/// ⚠️ ЗАЧЕМ. Общие поверхности `ui_base` отвечают, чем Qt красит контрол
/// САМА по роли палитры. У этой вкладки роль не участвует: контролы красит
/// её собственная таблица стилей. Пока обе стороны берут решение отсюда,
/// QML не может снова выбрать «похожий на глаз» токен.
#[test]
fn widget_qss_and_qml_control_colors_agree() {
    let _lang = i18n_pin::ru();
    let c = ap::colors();
    let qss = ap::page_qss(&c);
    let token = |name: &str| c.get(name).cloned().unwrap_or_default();

    for (kind, selector, hover_selector) in [
        ("combo", "#SortCombo", ""),
        (
            "nav",
            "#RefreshBtn, #PageBtn, #BackBtn",
            "#RefreshBtn:hover, #PageBtn:hover, #BackBtn:hover",
        ),
        (
            "secondary",
            "#DetailsBtn, #DownloadOnlyBtn, #CommentsMoreBtn",
            "#DetailsBtn:hover, #DownloadOnlyBtn:hover, #CommentsMoreBtn:hover",
        ),
    ] {
        let s = ap::control_colors(kind);
        let body = qss_rule(&qss, selector);
        assert_eq!(qss_value(body, "background"), token(s.bg), "{kind}: заливка");
        assert_eq!(
            qss_value(body, "border"),
            format!("1px solid {}", token(s.border)),
            "{kind}: рамка"
        );
        assert_eq!(
            qss_value(body, "border-radius"),
            format!("{}px", s.radius),
            "{kind}: скругление"
        );
        assert_eq!(
            qss_value(body, "padding"),
            format!("{}px {}px", s.pad_v, s.pad_h),
            "{kind}: поля"
        );
        assert_eq!(qss_value(body, "color"), token(s.fg), "{kind}: надпись");

        if hover_selector.is_empty() {
            // Своей подсветки нет — краски под курсором обязаны совпадать
            // с покойными, иначе QML нарисует то, чего у оригинала нет.
            assert_eq!((s.hover, s.border), (s.bg, s.hover_border), "{kind}: курсор");
            continue;
        }
        let hover = qss_rule(&qss, hover_selector);
        // Правило под курсором перекрывает не всё: у второстепенной кнопки
        // меняется одна рамка. Чего в нём нет — остаётся от покойного вида,
        // и голые краски обязаны говорить то же самое.
        assert_eq!(
            qss_value_opt(hover, "background").unwrap_or(&token(s.bg)),
            token(s.hover),
            "{kind}: заливка под курсором"
        );
        assert_eq!(
            qss_value(hover, "border"),
            format!("1px solid {}", token(s.hover_border)),
            "{kind}: рамка под курсором"
        );
    }

    // Имена токенов обязаны существовать в палитре ЛЮБОГО скина: иначе на
    // другом скине контрол остался бы бесцветным.
    for id in ui_base::available_skins() {
        let cols = ui_base::skin_colors(id.0);
        for kind in ["combo", "nav", "secondary"] {
            let s = ap::control_colors(kind);
            for name in [s.bg, s.hover, s.border, s.hover_border, s.fg] {
                assert!(cols.contains_key(name), "скин {}: нет токена {name}", id.0);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Исходный язык
// ═══════════════════════════════════════════════════════════════════════

/// По-английски подписи вкладки — исходные строки каталога. Сверки выше
/// держат русский вид; эта — что без каталога человек видит английский, а
/// не русские остатки и не пустоту.
#[test]
fn english_texts() {
    let _lang = i18n_pin::en();
    assert_eq!(ap::page_title(), "Appearance");
    assert_eq!(ap::sort_label(ap::SORTS[0].0), "🆕 Newest");
    assert_eq!(ap::pagination(0, 318, 18).label, "Page 1 / 16");
    assert_eq!(ap::scan_status("active", 7).unwrap(), "Active themes: 7");
    assert_eq!(ap::catalog_status(153, 10), "Found: 153 · showing 10");
    assert_eq!(ap::catalog_status(0, 0), "Nothing found");
    assert_eq!(ap::aur_query_too_short_msg(), "AUR search needs at least 2 characters");
    assert_eq!(ap::local_active_badge(), "✅ Active");
    assert_eq!(ap::card_local_button("active").text, "✅ Active");
    assert_eq!(ap::card_install_button(false, false, true).text, "Install");
    assert_eq!(ap::card_install_button(true, false, true).text, "🎨 Apply");
    assert_eq!(ap::card_install_button(true, false, false).text, "✅ Installed");
    assert_eq!(ap::install_button(true, true, true).text, "✅ Applied");
    assert_eq!(ap::aur_install_button(true, true).text, "✅ Installed");
    assert_eq!(ap::favorite_button(true).text, "⭐ Favorite");
    assert_eq!(
        ap::detail_meta_line("A", "B", "2026-01-01T00:00", "5.0"),
        "A • B • updated 2026-01-01 v5.0"
    );
    assert_eq!(ap::comments_header_text(None, 10, true), "💬 Comments (10+)");
    assert_eq!(
        ap::action_busy_status(ap::ACTION_AUR, "aerothemeplasma"),
        "🔨 Building “aerothemeplasma” from AUR…"
    );
    assert_eq!(ap::aur_meta_line("1.2.3-1", true), "🛠️ AUR · 1.2.3-1  ⚠️ out of date");
    assert_eq!(ap::description_html("", "", false, false, "#66c0f4"), "<i>No description</i>");
    assert_eq!(
        ap::source_chips().iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["KDE Store", "🛠️ AUR", "All"]
    );
    assert!(ap::lock_chips()[0].tooltip.contains("Only the lock screen part is applied"));
    assert_eq!(ap::scan_busy_status("system"), "🖥️ Looking for system themes…");
    let ru = i18n_pin::ru_catalog();
    // Каждая исходная подпись выше переведена — пустого места по-русски нет.
    for msgid in ["Appearance", "✅ Active", "Nothing found", "Page %1 / %2"] {
        assert!(ru.get(None, msgid).is_some_and(|t| !t.is_empty()), "{msgid}");
    }
    // Разные русские формы одного английского «✅ Installed» разведены
    // контекстом: тема «Установлено», пакет AUR «Установлен» (без контекста —
    // рабочий стол на вкладке столов, тоже «Установлен»).
    assert_eq!(ru.get(Some("theme"), "✅ Installed"), Some("✅ Установлено"));
    assert_eq!(ru.get(Some("AUR package"), "✅ Installed"), Some("✅ Установлен"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Экран входа: какой экран у раздела при PLM / SDDM / SteamOS (28.09.2026)
// ═══════════════════════════════════════════════════════════════════════

fn plm_view() -> CatalogView {
    CatalogView::new(categories::plasma_login_category(), "kde")
}

#[test]
fn login_section_screen_by_login_manager() {
    let _lang = i18n_pin::en();
    let home = Path::new("/home/u");

    // SDDM: полка магазина, как была.
    let sddm = view("sddm");
    assert_eq!(sddm.screen(home), ap::SCREEN_GRID);
    assert!(sddm.note(home).is_none());
    let c = sddm.chrome();
    assert!(c.search && c.sort_combo && c.pagination, "у SDDM-полки шапка магазина");

    // Plasma Login Manager: своя панель, ни поиска, ни сортировки, ни сети.
    let plm = plm_view();
    assert_eq!(plm.screen(home), ap::SCREEN_PLASMA_LOGIN);
    assert!(plm.note(home).is_none());
    let c = plm.chrome();
    assert!(c.caveat.as_deref().unwrap_or_default().contains("Shared by all desktops"), "{c:?}");
    assert_eq!(ap::Chrome { caveat: None, ..c }, ap::Chrome::default());
    assert!(plm.is_local(), "в магазин за разделом PLM не ходят");

    // Прогрев: раздел PLM магазина не имеет.
    let sidebar = categories::sidebar_for(home, true);
    let targets = ap::prefetch_targets(&plm, &sidebar);
    assert!(targets.iter().all(|(id, _)| id != "101" && id != "905"), "{targets:?}");
}

/// Экран входа и заставка загрузки — как kcm sddm и kcm plymouth: своя
/// сетка установленных тем с выделенной текущей, магазин под ней; скан —
/// раздела целиком (системный каталог тем). У Plasma Login Manager тем нет —
/// сетки нет; на SteamOS раздел объясняет отказ, а не показывает сетку.
#[test]
fn login_and_boot_sections_have_installed_grid() {
    let _lang = i18n_pin::en();
    let home = Path::new("/home/u");
    for key in ["sddm", "plymouth"] {
        let v = view(key);
        assert!(ap::has_installed_grid(key), "{key}");
        assert!(v.installed_view() && v.marks_current(), "{key}");
        assert_eq!(ap::local_scan_plan(&v.local_scan_which()), ("section".to_string(), key.to_string()), "{key}");
        assert_eq!(v.screen(home), ap::SCREEN_GRID, "{key}");
        let mut steam = view(key);
        steam.steamos = true;
        assert_eq!(steam.screen(home), ap::SCREEN_NOTE, "{key}: на SteamOS — объяснение");
    }
    let plm = plm_view();
    assert!(!plm.installed_view() && !ap::has_installed_grid(categories::PLASMA_LOGIN_KEY));
}

#[test]
fn steamos_sections_explain_before_install() {
    let _lang = i18n_pin::en();
    let home = Path::new("/home/u");
    for key in ["sddm", "plymouth"] {
        let mut v = view(key);
        assert_eq!(v.screen(home), ap::SCREEN_GRID, "{key}: не SteamOS");
        v.steamos = true;
        assert!(v.steamos_blocked(), "{key}");
        assert_eq!(v.screen(home), ap::SCREEN_NOTE, "{key}");
        assert_eq!(v.chrome(), ap::Chrome::default(), "{key}: шапка магазина не нужна");
        let note = v.note(home).unwrap();
        let reason = rexon_core::categories::steamos_block(key).unwrap().reason();
        assert!(note.contains(&reason), "{key}: {note}");
    }
    // PLM на SteamOS — тоже экран входа: причина важнее панели.
    let mut plm = plm_view();
    plm.steamos = true;
    assert_eq!(plm.screen(home), ap::SCREEN_NOTE);
    // Прочие разделы на SteamOS не задеты.
    let mut global = view("global");
    global.steamos = true;
    assert!(!global.steamos_blocked());
    assert_eq!(global.screen(home), ap::SCREEN_GRID);

    // Прогрев не греет закрытые на SteamOS витрины.
    let sidebar = categories::sidebar(home);
    let plain = ap::prefetch_targets(&view("global"), &sidebar);
    assert!(plain.iter().any(|(id, _)| id == "101") && plain.iter().any(|(id, _)| id == "108"));
    let steam = ap::prefetch_targets(&global, &sidebar);
    assert!(steam.iter().all(|(id, _)| id != "101" && id != "108"), "{steam:?}");
    let done = std::collections::BTreeSet::new();
    let front = ap::prefetch_neighbours(&global, &sidebar, &done);
    assert!(front.iter().all(|(id, _)| id != "101" && id != "108"), "{front:?}");
}

#[test]
fn login_settings_commands_prefer_system_settings() {
    let have = |list: &'static [&'static str]| move |b: &str| list.contains(&b);
    let v = |a: &str| vec![a.to_string(), "kcm_plasmalogin".to_string()];
    assert_eq!(
        ap::login_settings_commands(&have(&["systemsettings", "kcmshell6"])),
        vec![v("systemsettings"), v("kcmshell6")]
    );
    assert_eq!(ap::login_settings_commands(&have(&["kcmshell6"])), vec![v("kcmshell6")]);
    assert!(ap::login_settings_commands(&have(&[])).is_empty());
}

#[test]
fn plasma_login_panel_texts_en_and_ru() {
    {
        let _lang = i18n_pin::en();
        let p = ap::plasma_login_panel(true);
        assert_eq!(p.title, "The login screen is drawn by Plasma Login Manager");
        assert_eq!(
            p.body,
            "It comes with KDE Plasma 6.6 and later and doesn't support third-party themes: \
             the login screen always looks like Breeze. You can change its background and \
             clock and bring your desktop's colors, cursor and fonts over to it — that's done \
             in System Settings."
        );
        assert_eq!(p.open_label, "Open Login Screen Settings");
        assert_eq!(
            p.store_hint,
            "Looking for login screen themes from the store? They only work with SDDM."
        );
        assert_eq!(
            p.open_failed,
            "Couldn't open the login screen settings. Open “System Settings → Login Screen”."
        );
        // Открыть нечем — кнопки нет, текст сам говорит, куда идти.
        let bare = ap::plasma_login_panel(false);
        assert_eq!(bare.body, format!("{}\nOpen “System Settings → Login Screen”.", p.body));
    }
    let _lang = i18n_pin::ru();
    let p = ap::plasma_login_panel(true);
    assert_eq!(p.title, "Экран входа рисует Plasma Login Manager");
    assert_eq!(
        p.body,
        "Он входит в KDE Plasma 6.6 и новее и не поддерживает сторонние темы: вид экрана \
         входа всегда Breeze. Поменять можно фон, часы и перенести на экран входа цвета, \
         курсор и шрифты вашего рабочего стола — это делается в настройках системы."
    );
    assert_eq!(p.open_label, "Открыть настройки экрана входа");
    assert_eq!(p.store_hint, "Хотите темы экрана входа из магазина? Они работают только со SDDM.");
    assert_eq!(
        p.open_failed,
        "Не удалось открыть настройки экрана входа. Откройте «Параметры системы → Экран входа»."
    );
}

/// Разделы с установленным и магазином (KDE, окружения, Conky, заставка,
/// полные темы блокировки) показывают сверху установленное, а под ним на
/// той же странице — магазин раздела, сразу и без кнопки «Загрузить
/// новые…» (решение владельца 18.10.2026: кнопка сбивала с толку — в
/// «Виджетах Conky» была видна одна карточка, а 1032 виджета прятались).
#[test]
fn sections_show_installed_and_their_store_on_one_page() {
    let _lang = i18n_pin::ru();
    for key in ["global", "plasma", "colors", "aurorae", "icons", "cursor", "gtk", "sounds", "splash", "conky"] {
        let mut v = view(key);
        assert!(v.has_installed_view() && v.installed_view(), "{key}");
        assert!(!v.is_local(), "{key}: магазин грузится сам");
        assert!(v.marks_current(), "{key}: текущая выделяется");
        let c = v.chrome();
        assert!(c.search && c.sort_combo && c.pagination, "{key}");
        // Ключ запроса несёт и скан, и страницу магазина.
        let k = v.request_key();
        assert!(k.starts_with(&format!("combined:{key}:")), "{key}: {k}");
        v.page = 1;
        assert_ne!(v.request_key(), k, "{key}: другая страница магазина — другой запрос");
        v.page = 0;
        // Поиск — и по установленному, и по магазину.
        v.query = "bree".into();
        assert!(v.request_key().ends_with(":bree"), "{key}");
    }
    // Экран блокировки: установленное — только у полных тем.
    let mut v = view("lockscreen");
    assert!(!v.has_installed_view());
    v.lock_mode = "themes".into();
    assert!(v.installed_view());
    assert!(v.request_key().starts_with("combined:lockscreen:"));
    // Обзоры диска — по-прежнему только диск; в «Активных» включённых
    // много — рамкой там ничего не выделяется.
    for key in ["active", "downloaded"] {
        assert!(view(key).is_local(), "{key}");
    }
    assert!(!view("wallpapers").installed_view());
    assert!(!view("active").marks_current());
    assert_eq!(ap::installed_status(3), "Установлено: 3");
    // Строка состояния — обе части; без установленного — только магазин.
    assert_eq!(ap::combined_status(3, 1032, 20), "Установлено: 3 · Найдено: 1032 · показано 20");
    assert_eq!(ap::combined_status(0, 1032, 20), ap::catalog_status(1032, 20));
    assert!(ap::has_installed_grid("icons") && ap::has_installed_grid("splash"));
    assert!(!ap::has_installed_grid("lockscreen") && !ap::has_installed_grid("downloaded"));
}

#[test]
fn sounds_section_says_when_system_sounds_are_off() {
    let _lang = i18n_pin::ru();
    // MATE: `gsettings get org.mate.sound event-sounds` → false.
    let off = ap::event_sounds_banner("sounds", "false\n");
    assert!(off.visible);
    assert!(off.text.contains("выключены"), "{}", off.text);
    assert_eq!(off.button, "Включить");
    // Включены, прочитать не вышло, другой раздел — полосы нет.
    assert!(!ap::event_sounds_banner("sounds", "true\n").visible);
    assert!(!ap::event_sounds_banner("sounds", "").visible, "не знаем — не пугаем");
    assert!(!ap::event_sounds_banner("icons", "false").visible);
}

#[test]
fn conky_section_warns_before_start_and_on_unreliable_wayland() {
    let _lang = i18n_pin::ru();
    use rexon_core::de_apply::{conky_package, conky_section_note, conky_wayland_note};
    // Нет программы — сказано заранее, и что Rexon поставит её сам там, где может.
    let mut v = CatalogView::new(cat("conky"), "mate");
    v.conky_installed = false;
    v.conky_installable = true;
    let c = v.chrome().caveat.unwrap();
    assert!(c.contains("Conky") && c.contains("установит её"), "{c}");
    v.conky_installable = false;
    assert!(v.chrome().caveat.unwrap().contains("conky"), "SteamOS: только предупреждение");
    v.conky_installed = true;
    assert!(v.chrome().caveat.is_none(), "MATE на X11 с Conky — оговорок нет");
    // Wayland: GNOME — ненадёжно; KDE и тайловые рисуют сами.
    assert!(conky_wayland_note("gnome", true).is_some());
    assert!(conky_wayland_note("gnome", false).is_none());
    assert!(conky_wayland_note("kde", true).is_none());
    assert!(conky_wayland_note("sway", true).is_none());
    assert!(conky_section_note(true, true, "cinnamon", true).is_some());
    for family in ["arch", "debian", "fedora", "suse"] {
        assert_eq!(conky_package(family), Some("conky"), "{family}");
    }
    assert_eq!(conky_package("nix"), None);
}

#[test]
fn starting_a_conky_widget_installs_conky_first_without_question() {
    let _lang = i18n_pin::ru();
    use rexon_core::de_apply::{conky_install_failed, conky_install_package, is_conky_item};
    // Conky нет — ставится сам, без вопроса «Да/Нет» (защита — окно пароля).
    assert_eq!(conky_install_package(false, Some("conky")).as_deref(), Some("conky"));
    // Conky уже стоит — ставить нечего; поставить нечем (SteamOS) — тоже.
    assert!(conky_install_package(true, Some("conky")).is_none());
    assert!(conky_install_package(false, None).is_none());
    // Пароль не подтверждён — виджет не запущен, совет повторить.
    let text = conky_install_failed(&rexon_core::appearance_installer::password_not_confirmed());
    assert_eq!(text, "Виджет не запущен: ему нужна программа Conky. Нажмите «▶️ Запустить» ещё раз и введите пароль, чтобы её установить.");
    // Ошибка менеджера пакетов — с причиной.
    let text = conky_install_failed("x");
    assert!(text.contains("x") && text.contains("не запущен"), "{text}");
    // Чей это виджет: карточка магазина 124, прежний номер 919, местная плитка.
    let store = ThemeItem { id: "1".into(), category_id: 124, ..Default::default() };
    let old = ThemeItem { id: "2".into(), category_id: 919, ..Default::default() };
    let local = ThemeItem { id: "local:conky:clock".into(), ..Default::default() };
    let other = ThemeItem { id: "3".into(), category_id: 132, ..Default::default() };
    assert!(is_conky_item(&store) && is_conky_item(&old) && is_conky_item(&local));
    assert!(!is_conky_item(&other));
}

/// Conky Manager Rexon не предлагает (решение владельца 24.10: всё, что он
/// умеет, есть в самом разделе «Виджеты Conky»): ни карточки, ни установки
/// с Conky; товары магазина «Conky Manager GTK/Qt» в сетку не идут.
/// Уже поставленную программу Rexon не трогает.
#[test]
fn conky_manager_is_not_offered() {
    use rexon_core::de_apply::{conky_package, is_conky_program};
    use rexon_core::models::ThemeItem;
    for f in ["arch", "debian", "fedora", "suse"] {
        assert_eq!(conky_package(f), Some("conky"), "{f}");
    }
    let program = ThemeItem {
        id: "1".into(),
        category_id: 124,
        download_links: vec![serde_json::json!({"name": "conky-manager-q-1.0-2-x86-64.pkg.tar.zst"})],
        ..Default::default()
    };
    assert!(is_conky_program(&program));
}
