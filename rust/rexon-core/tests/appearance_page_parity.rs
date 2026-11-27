//! Acceptance of the moved numbers of the «Оформление» tab.
//!
//! The values are written out VERBATIM (once — from the Python widget tab;
//! since 02.10.2026 Python in Rexon is only an archive, and these entries are Rust's own
//! golden reference): the suite catches not "the code compiles" but a shift in what
//! the tab shows.

use rexon_core::appearance_page as ap;
use rexon_core::ui_base;

// The tab's captions are translated; checks against the former look run on the Russian
// catalog `po/ru.po` — that is the "in Russian as it was" snapshot. The English
// source is held by `english_texts`.
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
    // A round button: the radius is exactly half the side, otherwise the corner gets "cut off".
    assert!(ap::image_overlay_qss(ap::OVERLAY_ZOOM_PT).contains("border-radius: 22px"));
    assert!(ap::gallery_counter_qss().contains("border-radius: 12px"));
}

#[test]
fn counter_width_matches_the_original() {
    let _lang = i18n_pin::ru();
    // max(48, text width + 20)
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
    // The background may not take all the slots: a live tile always has somewhere to go.
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
    // n * ROW_H + 2, clamped to the row ceiling
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
    // On the default skin the tokens equal the historical Steam palette: the look
    // of the tab after the move must stay the same.
    let c = ui_base::skin_colors("steam_dark");
    let qss = ap::page_qss(&c);
    assert!(qss.contains("background: #1b2838;"));
    assert!(qss.contains("border-left: 3px solid #66c0f4;"));
    assert!(qss.contains("#InstallBtn, #InstallApplyBtn {"));
    assert!(qss.contains("background: #5ba32b;"));
    // Dimming an inactive button on this skin gives exactly the former white.
    assert!(qss.contains("#PageBtn:disabled { color: rgba(255,255,255,0.55); }"));
}

#[test]
fn every_skin_paints_the_page_without_holes() {
    let _lang = i18n_pin::ru();
    // Not a single "empty" token: an unclosed substitution would make the rule
    // invalid, and a part of the tab would stay unpainted.
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

/// Captions that used to be built as HTML with an embedded color are painted
/// by the style sheet — and on EVERY skin with its own tokens.
///
/// While the color was embedded in the text at build time, a skin change did not touch an already
/// open detail: the window and the tab around it were repainted at once, while
/// the meta line, the counters, the install path and the shown comments
/// stayed in the colors of the previous skin.
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
        // The nickname of the item's author HIMSELF — as a separate flag, not as a separate
        // branch in Python.
        assert!(
            qss.contains(&format!(
                "#CommentAuthor[own=\"true\"] {{ color: {}; }}",
                want("author_self")
            )),
            "{id}: ник автора товара красится мимо токена author_self"
        );
    }
}

/// No color embedded in caption text is left in the tab.
///
/// The only exception is a LINK `<a>`: Qt takes the anchor color once, when
/// parsing the text, and neither a style rule nor the `Link` role in the palette can
/// shift it afterwards (verified by measurement). There is one such place — the
/// «перевод ⇄ оригинал» switch (`toggle_link`), and it is repainted by re-parsing
/// its own caption: the color is substituted from the skin at build time.
#[test]
fn no_color_is_baked_into_label_text() {
    let _lang = i18n_pin::ru();
    const SRC: &str = include_str!("../src/appearance_page.rs");
    // Substituting a color into caption text is `style='color:{…}`. Comment lines
    // do not count: there an embedded color is described in words, not built.
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
    // The same by behaviour: in the built text the color is exactly on the link.
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
    // Without a translation there is no switch — and no color in the text at all.
    assert!(!ap::comment_body_html("orig", "orig", false, false, "#123456").contains("color:"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Log of installed items: "installed" ≠ "applied"
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::models::ThemeItem;
use serde_json::{json, Map, Value};
use std::path::Path;

#[test]
fn rexon_type_map_matches_the_original() {
    let _lang = i18n_pin::ru();
    // All 17 rows of the original's map, verbatim.
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
    // Both former catch-alls are absent from the map — the kind cannot be determined from them,
    // the entry must go to the second pass (by OCS category).
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
    // Its own type — taken at once, without consulting the category table.
    assert_eq!(
        ap::log_apply_kind(&home, &rec("theme-kde-global", 0, &[]), 0),
        "global"
    );
    // The former catch-all "plugin" + category 722 (Global Themes) → the second pass.
    assert_eq!(
        ap::log_apply_kind(&home, &rec("plugin", 722, &[]), 0),
        "global"
    );
    // The entry has no category — we take it from the card.
    assert_eq!(ap::log_apply_kind(&home, &rec("plugin", 0, &[]), 722), "global");
    // A kind without the notion of "applied now" — empty.
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
    // Downloaded and enabled.
    assert!(items[0].installed && items[0].applied);
    // Downloaded, but another one is enabled — "installed" without "applied".
    assert!(items[1].installed && !items[1].applied);
    // Fonts have no notion of "applied" at all.
    assert!(items[2].installed && !items[2].applied);
    // The item is not in the log — we do not touch the status.
    assert!(!items[3].installed && !items[3].applied);
}

/// A MATE frame from the store: «Установить» installs and applies it: the tile must
/// show "applied", not "installed" (live MATE check, 17.10).
/// A desktop section with a theme (Marco, xfwm4, shells) is a kind with "applied
/// now"; add-ons and Conky are not.
#[test]
fn de_section_theme_from_store_is_stamped_applied() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    // The «Metacity Themes» item (125) leads to the «marco» section.
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

/// Whether the item's theme is applicable (by section) and under what name it is installed.
#[test]
fn store_item_apply_name_comes_from_the_install_log() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let item = |cat: i64| ThemeItem { id: "7".into(), category_id: cat, ..Default::default() };
    assert!(ap::store_item_can_apply(&home, &item(722))); // global theme
    assert!(ap::store_item_can_apply(&home, &item(125))); // Marco frame
    assert!(!ap::store_item_can_apply(&home, &item(rexon_core::categories::CONKY_SECTION_ID)));
    let mut log = Map::new();
    log.insert(
        "7".into(),
        Value::Object(rec("theme-kde-colors", 0, &["/home/u/.local/share/color-schemes/Nord.colors"])),
    );
    assert_eq!(ap::logged_theme_name(&log, "7", |_| true).as_deref(), Some("Nord"));
    // The theme files were removed by hand — nothing to apply.
    assert_eq!(ap::logged_theme_name(&log, "7", |_| false), None);
    assert_eq!(ap::logged_theme_name(&log, "8", |_| true), None);
}

/// The tile of the «Новые» and «Популярные» storefronts names the kind of thing with the same
/// caption as «Активные»: authors write "for Plasma 6" in the names of
/// icons, and without the kind it seems that the desktop filter does not work.
#[test]
fn showcase_tiles_name_the_kind_of_item() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let item = |cat: i64| ThemeItem { id: "1".into(), category_id: cat, author: "Автор".into(), ..Default::default() };
    assert_eq!(ap::store_item_kind_label(&home, &item(132)).as_deref(), Some("📁 Значки"));
    assert_eq!(ap::store_item_kind_label(&home, &item(135)).as_deref(), Some("🎨 GTK-тема"));
    assert_eq!(ap::store_item_kind_label(&home, &item(107)).as_deref(), Some("🖱️ Курсор"));
    // A kind without a scan row — the section caption.
    assert!(ap::store_item_kind_label(&home, &item(299)).is_some());
    assert_eq!(ap::store_item_kind_label(&home, &item(0)), None);
    // The author line on a storefront tile: kind · author; in a section — as it was.
    assert_eq!(ap::showcase_author(&home, &item(132), true), "📁 Значки · Автор");
    assert_eq!(ap::showcase_author(&home, &item(132), false), "Автор");
}

#[test]
fn active_map_is_pulled_at_most_once_and_only_when_needed() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let mut log = Map::new();
    // Fonts only: they have no applying → there is NO reason to query the map.
    log.insert("1".into(), Value::Object(rec("font-pack", 0, &["/x/A"])));
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(Map::new);
    ap::stamp_installed_from_log(&home, &log, &mut items, &mut active);
    assert!(!active.was_fetched(), "kreadconfig6 дёрнут впустую");

    // An empty log — not a single call either.
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(Map::new);
    ap::stamp_installed_from_log(&home, &Map::new(), &mut items, &mut active);
    assert!(!active.was_fetched());

    // Two applicable cards — the source is queried exactly once.
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

/// A private directory for the log: the test does not touch the live `~/.config/rexon`.
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

    // No file and broken JSON — an empty log: cards as they came, the map
    // of active themes is not queried.
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

    // Installation failed — the former state stays.
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, false, &mut active);
    assert!(!items[0].installed && !items[0].applied);

    // Succeeded and the theme is enabled — «Применено».
    let mut items = vec![ThemeItem { id: "1".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, true, &mut active);
    assert!(items[0].installed && items[0].applied);

    // Succeeded, but another one is enabled, and the former «Применено» is a lie: we reset it.
    let mut items = vec![ThemeItem {
        id: "2".into(),
        installed: true,
        applied: true,
        ..Default::default()
    }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, true, &mut active);
    assert!(items[0].installed && !items[0].applied);

    // The log was not written (no entry) — the theme is on disk: «Установлено».
    let mut items = vec![ThemeItem { id: "9".into(), ..Default::default() }];
    let mut active = ap::LazyActiveMap::new(active_icons);
    ap::mark_installed_from_log_file(&home, &path, &mut items, true, &mut active);
    assert!(items[0].installed && !items[0].applied);
    let _ = std::fs::remove_dir_all(dir);
}

// ═══════════════════════════════════════════════════════════════════════
//  Preview loader: queue, priority, dedup, retries
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::appearance_page::{PreviewQueue, PreviewRetry};

#[test]
fn preview_cache_path_matches_the_original() {
    let _lang = i18n_pin::ru();
    let home = "/home/u";
    // An ordinary store preview: sha1 of the URL + the extension from the link's PATH.
    let p = ap::preview_cache_path(home, "https://images.pling.com/img/00/1.png");
    assert!(p.starts_with("/home/u/.cache/rexon/ocs_previews/"), "{p}");
    assert!(p.ends_with(".png"), "{p}");
    // No extension or an implausibly long one → .jpg.
    assert!(ap::preview_cache_path(home, "https://x/img/00/1").ends_with(".jpg"));
    assert!(ap::preview_cache_path(home, "https://x/a.verylongext").ends_with(".jpg"));
    // The query does not get into the extension.
    assert!(ap::preview_cache_path(home, "https://x/a.png?v=2").ends_with(".png"));
    // GitHub OpenGraph (AUR preview) — its own directory and always PNG.
    let a = ap::preview_cache_path(home, "https://opengraph.githubassets.com/1/u/repo");
    assert!(a.starts_with("/home/u/.cache/rexon/aur_previews/"), "{a}");
    assert!(a.ends_with(".png"), "{a}");
    // The file name is the same as the store client's: there is no second naming scheme.
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
    // First background, then live ones — the live ones must go to the front.
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
    // The background gets exactly two slots out of six — the rest wait for live tiles.
    assert_eq!(started.len(), 2);
    assert_eq!(q.low_inflight(), 2);
    assert_eq!(q.inflight(), 2);
    // A live request gets a slot immediately, despite the background queue.
    q.want("live", "hot", false, None, true);
    let more = q.pump();
    assert_eq!(urls(&more), vec!["hot"]);
    // The answer to a background request frees a background slot.
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
    // Live requests for one URL are NOT collapsed: two cards occasionally share
    // a link, and otherwise there would be nobody to send "done" to the second.
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
    // A background request is cached silently — "done" does not go up.
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
    // A background success does not surface.
    q.succeeded("c2", "u2", true, "/cache/u2.png");
    assert!(!q.has_ready());
}

#[test]
fn temporary_failures_retry_and_permanent_ones_do_not() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    // A dropped connection (no HTTP code) — two extra attempts with a growing pause.
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
    // No file — repeating is pointless from the very first answer.
    for code in [400u16, 401, 403, 404, 405, 410, 451] {
        let mut q = PreviewQueue::new();
        assert_eq!(
            q.failed("u", Some(code)),
            PreviewRetry::GiveUp { fatal: true, attempts: 1 },
            "HTTP {code}"
        );
    }
    // 5xx and a timeout are temporary.
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
    // The person went to another category — the deferred retry is no longer needed.
    q.cancel_all();
    assert!(!q.requeue(gen, "c1", "u1", false));
    assert_eq!(q.queued(), 0);
    // A retry of its own generation goes back into the queue.
    let gen = q.generation();
    assert!(q.requeue(gen, "c1", "u1", false));
    assert_eq!(q.queued(), 1);
    // The same picture is already being requested anew — we do not spawn a second request.
    assert!(!q.requeue(gen, "c1", "u1", false));
    assert_eq!(q.queued(), 1);
}

#[test]
fn deduplication_is_released_when_the_answer_arrives() {
    let _lang = i18n_pin::ru();
    let mut q = PreviewQueue::new();
    q.want("c1", "u", true, None, false);
    q.pump();
    // While the picture is in flight, a repeated background request does not produce a second query.
    q.want("c2", "u", true, None, false);
    assert_eq!(q.queued(), 0);
    q.finished("u", true);
    // The answer came — the key is free, the next request is legitimate again.
    q.want("c3", "u", true, None, false);
    assert_eq!(q.queued(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
//  Catalog state
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

/// A section with a store: in sections with installed items it goes below them on the
/// same page, there is nothing to open it with separately.
fn store(key: &str) -> CatalogView {
    view(key)
}

/// The «vsthemes» source was removed on 04.10.2026. Whoever chose it before lands in
/// KDE Store silently: a shelf, not an empty screen, and the KDE Store chip is marked.
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

/// The «Razer Axon» source (14.11.2026): a chip of «Обои HD» next to KDE Store and
/// Wallhaven — only if Axon is found; when chosen — its own library screen
/// instead of the store grid, without the store's sorting and pager (the
/// library has its own). The search line is the common header one, as for the neighbouring stores
/// (16.11.2026: its own one drifted down to the left and lost the word), and the word survives
/// a store change. Without Axon the former choice silently becomes KDE Store.
#[test]
fn razer_axon_is_a_wallpaper_source_only_when_axon_is_found() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    assert!(!ap::source_keys_in("wallpapers", false).contains(&ap::SOURCE_AXON));
    let keys = ap::source_keys_in("wallpapers", true);
    assert_eq!(keys.last(), Some(&ap::SOURCE_AXON), "последним чипом");
    assert_eq!(keys[..keys.len() - 1], ap::source_keys_for("wallpapers")[..]);
    for other in ["icons", "global", "cursor"] {
        assert!(!ap::source_keys_in(other, true).contains(&ap::SOURCE_AXON), "{other}");
    }
    let chip = ap::source_chips_in("wallpapers", true).into_iter().last().unwrap();
    assert_eq!((chip.key, chip.label.as_str()), (ap::SOURCE_AXON, "Razer Axon"));
    assert!(!chip.tooltip.is_empty());
    assert!(ap::source_chips_for("wallpapers").iter().all(|c| c.key != ap::SOURCE_AXON));

    let mut v = view("wallpapers");
    v.source = ap::SOURCE_AXON.to_string();
    // Axon not found — no source: KDE Store and its grid.
    assert!(!v.axon_mode());
    assert_eq!(v.effective_source(), "ocs");
    assert_eq!(v.screen(home), ap::SCREEN_GRID);
    v.axon = true;
    assert!(v.axon_mode());
    assert_eq!(v.effective_source(), ap::SOURCE_AXON);
    assert_eq!(v.screen(home), ap::SCREEN_AXON);
    let c = v.chrome();
    assert!(c.source_bar, "чипы «Магазин:» остаются — вернуться в KDE Store");
    assert!(c.search, "строка поиска — та же, что у KDE Store");
    assert_eq!(c.search_placeholder, "Поиск обоев");
    assert!(!c.sort_combo && !c.pagination && !c.install_file);
    assert_ne!(v.request_key(), view("wallpapers").request_key());
    // A word typed in KDE Store stays in the field in Axon too, and back.
    v.source = "ocs".to_string();
    assert_eq!(v.edit_query("sky"), ap::QueryEdit::Kept);
    v.source = ap::SOURCE_AXON.to_string();
    assert_eq!(v.query, "sky");
    assert!(v.chrome().search);
    v.source = "ocs".to_string();
    assert_eq!(v.query, "sky");
    v.source = ap::SOURCE_AXON.to_string();
    // The section's «⚙️ Настройки» is above the source: the settings screen.
    v.section_settings = true;
    assert_eq!(v.screen(home), ap::SCREEN_SETTINGS);
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
    // Local disk overviews know no network.
    for key in ["active", "downloaded", "system", "favorites"] {
        let v = view(key);
        assert!(v.is_local(), "{key}");
        assert!(!v.source_applicable(), "{key}");
        assert!(!v.aur_mode() && !v.both_mode(), "{key}");
    }
    // An ordinary network category — the switch is in place.
    let mut v = store("global");
    assert!(v.source_applicable());
    v.source = "aur".into();
    assert!(v.aur_mode() && !v.both_mode());
    v.source = "both".into();
    assert!(v.both_mode() && !v.aur_mode());
    // An umbrella category: there is an online catalog, no AUR source.
    let v = view("splash");
    assert!(v.category.ocs_cats().len() > 1, "у заставки должен быть зонт");
    assert!(!v.source_applicable());
}

#[test]
fn lock_mode_picks_the_real_store_category() {
    let _lang = i18n_pin::ru();
    let mut v = view("lockscreen");
    assert_eq!(v.query_category_id(), "299"); // lock screen wallpaper — the default
    v.lock_mode = "themes".into();
    assert_eq!(v.query_category_id(), "722"); // full themes from Global Themes
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
    v.sort = "alpha".into(); // virtual ones do not override the person's choice
    assert_eq!(v.active_sort(), "high");
    let mut v = view("new");
    v.sort = "alpha".into();
    assert_eq!(v.active_sort(), "new");
    // An ordinary category obeys the choice.
    let mut v = view("global");
    v.sort = "alpha".into();
    assert_eq!(v.active_sort(), "alpha");
}

#[test]
fn chrome_matches_the_original() {
    let _lang = i18n_pin::ru();
    // An ordinary network category.
    let c = store("global").chrome();
    assert!(c.search && c.source_bar && c.sort_combo && c.pagination);
    assert!(!c.style_bar && !c.lock_bar);
    assert_eq!(c.search_placeholder, "🔍 Поиск тем…");
    // Pure AUR: sorting is pinned to votes — we hide the combo.
    let mut v = store("global");
    v.source = "aur".into();
    let c = v.chrome();
    assert!(!c.sort_combo && c.pagination);
    assert_eq!(c.search_placeholder, "🔍 Поиск в AUR…");
    assert_eq!(v.search_placeholder(), "🔍 Поиск в AUR…");
    // «Все»: the combo stays — it controls the OCS part of the list.
    let mut v = store("global");
    v.source = "both".into();
    assert!(v.chrome().sort_combo);
    // A local overview: neither sorting nor pages.
    let c = view("downloaded").chrome();
    assert!(c.search && !c.sort_combo && !c.pagination && !c.source_bar);
    // Sub-chips only in their own sections.
    assert!(view("widgetstyle").chrome().style_bar);
    assert!(view("lockscreen").chrome().lock_bar);
    assert!(!view("global").chrome().style_bar);
}

/// Every filter of the installed grid under which there is no store: the section and
/// how to enable it.
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
    // One rule for all filters of the installed grid (live check
    // 01.11.2026: under «Kvantum» «Найдено 337» and «1 / 17» remained).
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
        // The store page is not part of the key — there is nothing to page through.
        v.page = 3;
        assert_eq!(v.request_key(), rk, "{key}");
    }
    // «Все» for every filter — the store is in place.
    let mut v = store("widgetstyle");
    v.style_sub = Some("all".into());
    assert!(!v.installed_only() && v.chrome().pagination);
    let mut v = store("colors");
    v.tone_filter = "all".into();
    assert!(!v.installed_only());
    // A filter of a foreign section has no effect: «С поддержкой цветовых схем»
    // exists only for Plasma styles, «Запущенные» — only for Conky.
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
    // Plasmoids do not work in GNOME — the section is closed entirely.
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
    // The store pages by the requested twenty even if it sent 18:
    // 318 items — 16 pages, not 318 / 18 = 18 (two empty ones at the end).
    let p = ap::pagination(0, 318, 18, ap::PAGE_SIZE);
    assert_eq!(p.label, "Страница 1 / 16");
    assert!(!p.prev_enabled && p.next_enabled);
    let p = ap::pagination(15, 318, 18, ap::PAGE_SIZE);
    assert_eq!(p.label, "Страница 16 / 16");
    assert!(p.prev_enabled && !p.next_enabled);
    // Empty — one page, nowhere to page to.
    let p = ap::pagination(0, 0, 0, ap::PAGE_SIZE);
    assert_eq!(p.label, "Страница 1 / 1");
    assert!(!p.prev_enabled && !p.next_enabled);
    // The answer is empty, but the total is large: «Вперёд» must not lead nowhere.
    assert!(!ap::pagination(0, 100, 0, ap::PAGE_SIZE).next_enabled);
}

#[test]
fn prefetch_and_the_forward_button_agree_on_the_page_count() {
    let _lang = i18n_pin::ru();
    // The «Вперёд» button and the warm-up must count pages the same way.
    for page in 0..15 {
        assert_eq!(
            ap::has_page_to_prefetch(page, 318, ap::PAGE_SIZE),
            ap::pagination(page, 318, 18, ap::PAGE_SIZE).next_enabled,
            "страница {page}"
        );
    }
    assert!(!ap::has_page_to_prefetch(15, 318, ap::PAGE_SIZE));
    assert!(!ap::has_page_to_prefetch(0, 0, ap::PAGE_SIZE));
}

/// A fake store: server pages of a given length, items numbered
/// in stream order. It counts which pages it was asked for.
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
        ap::stream_page(page, ap::PAGE_SIZE, ap::PAGE_SIZE, |k| self.fetch(k))
    }
}

#[test]
fn store_pages_are_topped_up_to_twenty_without_repeats() {
    // A live walk of Global Themes 03.10.2026: 18, 20 × 14, 18 with totalitems 318.
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
    // The stream was read entirely, in sequence, without repeats or gaps.
    assert_eq!(all, (0..visible).collect::<Vec<_>>());
    // 316 visible — 15 full pages and the last of 16.
    assert_eq!(page, 15);
    assert_eq!(store.page(15).items.len(), 16);
}

#[test]
fn store_page_needs_only_the_pages_it_covers() {
    // Holes 18/20/20/18: the first Rexon page takes 2 more items from the second
    // server page (and one more on top — to find out whether there is a next one).
    let mut store = FakeStore::new(&[18, 20, 20, 18], 80);
    let p = store.page(0);
    assert_eq!(p.items, (0..20).collect::<Vec<_>>());
    assert!(p.has_more);
    assert_eq!(store.asked, vec![0, 1]);
    // The second starts exactly where the first ended.
    let p = store.page(1);
    assert_eq!(p.items, (20..40).collect::<Vec<_>>());
    assert!(p.has_more);
    // The last: the stream has 76 items — 16 on the fourth page, then empty.
    let p = store.page(3);
    assert_eq!(p.items, (60..76).collect::<Vec<_>>());
    assert!(!p.has_more);
    // The page count by the actual end of the stream: 4, not 80 / 20 = 4 + an empty one.
    let pg = ap::stream_pagination(3, p.total, p.has_more, ap::PAGE_SIZE);
    assert!(!pg.next_enabled && pg.prev_enabled);
}

/// A grid page is whole rows, no fewer than twenty: with nine columns
/// a page of twenty ended with a row of two tiles and seven empty ones.
#[test]
fn grid_page_is_whole_rows_of_at_least_twenty() {
    for cols in 1..=40u32 {
        let size = ap::grid_page_size(cols);
        assert_eq!(size % cols, 0, "{cols} столбцов: {size} — не целые ряды");
        assert!(size >= ap::PAGE_SIZE, "{cols} столбцов: {size} меньше двадцати");
        assert!(size < ap::PAGE_SIZE + cols, "{cols} столбцов: {size} — лишний ряд");
    }
    assert_eq!(ap::grid_page_size(4), 20);
    assert_eq!(ap::grid_page_size(5), 20);
    assert_eq!(ap::grid_page_size(6), 24);
    assert_eq!(ap::grid_page_size(9), 27);
    assert_eq!(ap::grid_page_size(0), 20);
}

/// Window width change: the first item of the former page stays on screen.
#[test]
fn page_after_resize_keeps_the_first_item_visible() {
    for (page, old, new) in [(0i64, 20u32, 27u32), (3, 20, 27), (5, 27, 24), (7, 24, 20), (2, 21, 30)] {
        let first = page * i64::from(old);
        let moved = ap::page_after_resize(page, old, new);
        let start = moved * i64::from(new);
        assert!(start <= first && first < start + i64::from(new), "{page} {old}→{new}");
    }
}

/// A Rexon page is larger than a server page (27 with nine columns): it is assembled from
/// server pages of twenty in sequence, without repeats or gaps, and
/// the last one is shorter only at the very end of the stream.
#[test]
fn whole_row_pages_are_cut_from_fixed_server_pages() {
    let mut lens = vec![18];
    lens.extend([20; 14]);
    lens.push(18);
    let total = 318;
    let size = ap::grid_page_size(9);
    let mut store = FakeStore::new(&lens, total);
    let mut seen = Vec::new();
    let mut page = 0;
    loop {
        let p = ap::stream_page(page, size, ap::PAGE_SIZE, |k| store.fetch(k));
        if p.has_more {
            assert_eq!(p.items.len(), size as usize, "страница {page}");
        }
        seen.extend(p.items);
        if !p.has_more {
            break;
        }
        page += 1;
    }
    let stream: usize = lens.iter().sum();
    assert_eq!(seen, (0..stream).collect::<Vec<_>>());
}

#[test]
fn store_page_with_a_fully_hidden_server_page_keeps_going() {
    // A whole server page is hidden (0 came with a live totalitems) —
    // that is not the end of the stream: the end is counted by totalitems.
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
    let pg = ap::stream_pagination(0, 0, false, ap::PAGE_SIZE);
    assert_eq!(pg.label, "Страница 1 / 1");
    assert!(!pg.prev_enabled && !pg.next_enabled);
}

#[test]
fn store_failure_mid_stream_does_not_kill_the_forward_button() {
    // Page zero arrived, the first — the network went down: we do not disable «Вперёд»
    // by the 18 visible ones, we judge by totalitems, as before the top-up.
    let p = ap::stream_page(0, ap::PAGE_SIZE, ap::PAGE_SIZE, |k| {
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
    // While the stream goes on — pages by totalitems.
    let p = ap::stream_pagination(0, 318, true, ap::PAGE_SIZE);
    assert_eq!(p.label, "Страница 1 / 16");
    assert!(!p.prev_enabled && p.next_enabled);
    // The stream ended earlier than the store's count — we do not promise an empty last page.
    let p = ap::stream_pagination(14, 318, false, ap::PAGE_SIZE);
    assert_eq!(p.label, "Страница 15 / 15");
    assert!(p.prev_enabled && !p.next_enabled);
    // The store undercounted (new items shifted the stream) — no fewer pages than
    // the one we are on, plus the next.
    assert_eq!(ap::stream_pagination(16, 318, true, ap::PAGE_SIZE).label, "Страница 17 / 18");
}

#[test]
fn sort_change_is_a_new_stream_from_page_zero() {
    // The stream depends on the sorting: a sorting change (the page is reset
    // to 0) reads its own server pages, not the tail of the former one.
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
    // RPC returns the whole list at once — we cut pages of 20 in place.
    assert_eq!(ap::aur_page_range(50, 0), 0..20);
    assert_eq!(ap::aur_page_range(50, 1), 20..40);
    assert_eq!(ap::aur_page_range(50, 2), 40..50);
    assert_eq!(ap::aur_page_range(50, 9), 50..50);
    assert_eq!(ap::aur_page_range(0, 0), 0..0);
    // A fresh list is shorter — we pull the page back to the last existing one.
    assert_eq!(ap::aur_clamp_page(50, 7), 2);
    assert_eq!(ap::aur_clamp_page(0, 7), 0);
    assert_eq!(ap::aur_clamp_page(21, 0), 0);
    // Paging past the end and below zero is forbidden.
    assert!(!ap::aur_can_change_page(50, 0, -1));
    assert!(ap::aur_can_change_page(50, 0, 1));
    assert!(!ap::aur_can_change_page(50, 2, 1));
    assert!(ap::aur_can_change_page(50, 2, -1));
}

#[test]
fn aur_search_needs_at_least_two_characters() {
    let _lang = i18n_pin::ru();
    assert!(!ap::aur_query_too_short(""));       // empty — this is a selection, not a search
    assert!(ap::aur_query_too_short("a"));
    assert!(ap::aur_query_too_short("я"));       // we count characters, not bytes
    assert!(!ap::aur_query_too_short("ab"));
}

#[test]
fn aur_cache_key_separates_search_from_the_section() {
    let _lang = i18n_pin::ru();
    let mut v = view("global");
    assert_eq!(v.aur_cache_key(), "cat:global");
    v.query = "nord".into();
    assert_eq!(v.aur_cache_key(), "search:global:nord");
    // Search results are selected for the section — another section has its own key.
    assert_ne!(view("icons").aur_cache_key(), "search:global:nord");
    let mut icons = view("icons");
    icons.query = "nord".into();
    assert_ne!(icons.aur_cache_key(), v.aur_cache_key());
}

/// The mark of an AUR package where it comes mixed with single items («Все»):
/// a set, a system package, a password. In «AUR» and in KDE Store it is absent.
#[test]
fn aur_tiles_are_marked_only_among_store_goods() {
    let _lang = i18n_pin::ru();
    let mut v = view("wallpapers");
    v.source = ap::SOURCE_BOTH.to_string();
    assert_eq!(ap::aur_mix_badge(&v, "wallpapers"), "📦 Набор · системный пакет · пароль");
    let mut icons = view("icons");
    icons.source = ap::SOURCE_BOTH.to_string();
    assert_eq!(ap::aur_mix_badge(&icons, "icons"), "📦 Системный пакет · пароль");
    assert!(ap::aur_mix_badge_tooltip().contains("пароль"));
    v.source = ap::SOURCE_AUR.to_string();
    assert_eq!(ap::aur_mix_badge(&v, "wallpapers"), "");
    v.source = "ocs".to_string();
    assert_eq!(ap::aur_mix_badge(&v, "wallpapers"), "");
    assert!(ap::is_font_file("/c/x.TTF") && ap::is_font_file("/c/x.otf") && !ap::is_font_file("/c/x.png"));
    assert!(ap::looks_like_font(&[0, 1, 0, 0, 0, 9]) && ap::looks_like_font(b"OTTO..") && !ap::looks_like_font(b"\x89PNG"));
}

#[test]
fn typed_word_reaches_the_query() {
    let _lang = i18n_pin::ru();
    // Live check 06.09.2026: the typed word got lost on the way, the query
    // stayed empty, and «Активные» showed all themes in a row.
    let mut v = view("active");
    assert_eq!(v.edit_query("  "), ap::QueryEdit::Idle);
    assert_eq!(v.query, "");
    assert_eq!(v.edit_query("bree"), ap::QueryEdit::Kept);
    assert_eq!(v.query, "bree");                 // ← this entry never existed
    assert_eq!(v.edit_query(" bree "), ap::QueryEdit::Idle);
    v.page = 3;
    assert_eq!(v.edit_query(""), ap::QueryEdit::Cleared);
    assert_eq!(v.query, "");
    assert_eq!(v.page, 0);                       // search reset — from the first
    assert_eq!(v.edit_query(""), ap::QueryEdit::Idle);
}

/// 19.10.2026: search in sections fired only on Enter, and a word forgotten
/// in the field («MishMty») left one tile in the section — it seemed
/// that the store was gone. Now the search runs as you type: a disk overview —
/// at once, the store and the "installed + store" section — after a pause; the status
/// line says a search is active.
#[test]
fn search_runs_while_typing_and_says_so() {
    let _lang = i18n_pin::ru();
    // A disk overview knows no network — filtering at once.
    let local = view("active");
    assert_eq!(local.search_delay_ms(), 0);
    // The store (and the installed items above it) — not queried per character.
    let store = view("global");
    assert_eq!(store.search_delay_ms(), ap::SEARCH_DEBOUNCE_MS);
    assert_eq!(ap::SEARCH_DEBOUNCE_MS, 300);

    assert_eq!(ap::search_status("MishMty", 1), "Поиск: «MishMty» — найдено 1");
    assert_eq!(ap::search_status("MishMty", 0), "Поиск: «MishMty» — ничего не найдено");
    assert_eq!(ap::search_clear_tooltip(), "Очистить поиск");

    // The page: Enter applies at once, typing — after the core's pause, the field has
    // a clear button.
    let qml = include_str!("../../rexon-gui/qml/AppearancePage.qml");
    assert!(qml.contains("page.searchDelayMs"), "пауза поиска — из ядра");
    assert!(qml.contains("page.searchClearTooltip"), "у поля поиска кнопка «Очистить»");
}

#[test]
fn request_key_carries_the_query_in_every_section() {
    let _lang = i18n_pin::ru();
    // The answer is checked by the key, and a duplicate is discarded by it too. Without the query
    // in the key, a disk overview search looked to dedup like the same
    // request, and the answer of an UNfiltered scan was accepted into the grid.
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
    assert!(ap::local_matches("что угодно", "   "));   // an empty query does not cut
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

/// A theme from disk is found both by the shown (translated) name and by the
/// original one: the untranslated name and the directory from the card number.
/// Live check r1434 (16.10.2026): in «GTK-темы» «Adwaita» and
/// «HighContrast» gave «Ничего не найдено», while «Высоко» found it.
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
    // A store card: the number is a number, there is no directory in it.
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
    assert!(ap::scan_status("splash", 3).is_none()); // the caption is taken from the section

    assert_eq!(ap::scan_local_kind("active", ""), "active");
    assert_eq!(ap::scan_local_kind("downloaded", ""), "downloaded");
    assert_eq!(ap::scan_local_kind("system", ""), "downloaded");
    // Other scans recognize the active item by the author caption.
    assert_eq!(ap::scan_local_kind("splash", "🎨 Заставка · ✅ Активна"), "active");
    assert_eq!(ap::scan_local_kind("splash", "🎨 Заставка"), "downloaded");
}

#[test]
fn a_stale_scan_answer_is_dropped() {
    let _lang = i18n_pin::ru();
    assert!(ap::scan_answer_stale("system", "downloaded"));
    assert!(!ap::scan_answer_stale("system", "system"));
    // The «Стиль приложений» scan carries a sub-choice and is captured with its own counter.
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
    // The former "one style" sub-choice no longer narrows the grid to one card.
    v.style_sub = Some("qt:Breeze".into());
    assert_eq!(v.local_scan_which(), "widgetstyle:all");
}

#[test]
fn local_scan_plan_reads_back_what_local_scan_which_wrote() {
    let _lang = i18n_pin::ru();
    // The key is built by `local_scan_which` and parsed by `local_scan_plan`:
    // the format is one, and ONE owner must know it. Previously the parsing lived in
    // the Python tab executor as a second copy.
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

    // KDE sections with an installed grid — a section scan, as for desktops.
    for key in ["global", "plasma", "colors", "aurorae", "icons", "cursor", "gtk", "sounds"] {
        assert_eq!(ap::local_scan_plan(key), ("section".to_string(), key.to_string()));
    }

    // A section without a disk scan — nothing to show, and that is NOT «активные».
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
        // Plasma styles do not apply in MATE.
        assert!(!ids.contains(&"104"), "{key}: {q}");
        // Icons, cursors, GTK themes and wallpapers — take effect; global themes
        // (01.11.2026) — in parts: colors, style, icons, cursors, fonts.
        for want in ["722", "132", "107", "135", "299"] {
            assert!(ids.contains(&want), "{key}: нет {want} в {q}");
        }
        for id in &ids {
            let c = categories::category_by_id(Path::new("/home/u"), id.parse().unwrap()).unwrap();
            assert!(categories::category_available(&c.key, "mate"), "{key}: {id} не для MATE");
        }
    }
    // The warm-up asks the same as the live query, and does not warm KDE sections.
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
    // No duplicates: «Популярные» and «Global» give the same pair.
    let mut seen = std::collections::BTreeSet::new();
    for k in &targets {
        assert!(seen.insert(k.clone()), "дубль в очереди прогрева: {k:?}");
    }
    // Not a single local overview and not a single section without a store.
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
    // The open section itself does not get into the neighbours.
    assert!(!front.iter().any(|(c, _)| *c == first_net.id.to_string()));
    // Queue reordering: neighbours in front, the rest after them and without duplicates.
    let queue = vec![("999".to_string(), "new".to_string()), front[0].clone()];
    let out = ap::prefetch_reorder(&front, &queue);
    assert_eq!(&out[..front.len()], &front[..]);
    assert_eq!(out.iter().filter(|k| **k == front[0]).count(), 1);
    assert!(out.contains(&("999".to_string(), "new".to_string())));
}

#[test]
fn the_running_desktop_wins_but_a_manual_choice_survives() {
    let _lang = i18n_pin::ru();
    // First launch: we know nothing — we take the running one and remember it.
    assert_eq!(
        ap::initial_de("gnome", "", ""),
        ap::DeChoice {
            de_id: "gnome".into(),
            remember: Some(("gnome".into(), "gnome".into()))
        }
    );
    // The person is in the same session where they chose by hand — their choice holds.
    assert_eq!(
        ap::initial_de("kde", "gnome", "kde"),
        ap::DeChoice { de_id: "gnome".into(), remember: None }
    );
    // Changed the desktop — the choice was inherited, not made: the catalog follows
    // the actual desktop.
    assert_eq!(
        ap::initial_de("gnome", "kde", "kde"),
        ap::DeChoice {
            de_id: "gnome".into(),
            remember: Some(("gnome".into(), "gnome".into()))
        }
    );
    // The desktop is not detected, but there was a choice — we keep the choice.
    assert_eq!(
        ap::initial_de("", "gnome", "kde"),
        ap::DeChoice { de_id: "gnome".into(), remember: None }
    );
    // Nothing is known at all — the widest set of sections.
    assert_eq!(
        ap::initial_de("", "", ""),
        ap::DeChoice { de_id: "kde".into(), remember: None }
    );
}

/// 19.10.2026: in a MATE session «Текущее окружение» was set to «KDE Plasma» —
/// a choice made by hand at night — and nothing on screen said that this was
/// not the desktop detected from the session. A manual choice different from the session
/// is labelled as manual, and the first item is «Определять автоматически».
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

    // The session's desktop is chosen — nothing to label, nothing to reset either.
    let auto = ap::de_picker_choices("mate", "mate");
    assert!(auto.iter().all(|(id, _)| id != ap::DE_AUTO_ID));
    assert_eq!(name(&auto, "kde"), "KDE Plasma");
    assert_eq!(auto, rexon_core::categories::appearance_de_choices("mate"));

    // The session is not recognized — nothing to compare with.
    assert!(ap::de_picker_choices("kde", "").iter().all(|(id, _)| id != ap::DE_AUTO_ID));

    // A recognized but unsupported session desktop — by its name, not by a raw id.
    let ukui = ap::de_picker_choices("kde", "ukui");
    assert_eq!(ukui[0], (ap::DE_AUTO_ID.to_string(), "Определять автоматически (UKUI)".to_string()));

    // What to do on choosing an item: (the catalog desktop, what to remember as the choice).
    // «Автоматически» — the session desktop and an empty choice: `initial_de` then
    // follows the session by itself.
    assert_eq!(ap::resolve_de_pick(ap::DE_AUTO_ID, "mate"), ("mate".to_string(), String::new()));
    assert_eq!(ap::resolve_de_pick("kde", "mate"), ("kde".to_string(), "kde".to_string()));
    // The page recognizes the "automatically" item by the same name.
    let qml = include_str!("../../rexon-gui/qml/AppearancePage.qml");
    assert!(qml.contains(&format!("picked === \"{}\"", ap::DE_AUTO_ID)));
    assert_eq!(
        ap::initial_de("mate", "", "mate"),
        ap::DeChoice { de_id: "mate".into(), remember: None }
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Details panel
// ═══════════════════════════════════════════════════════════════════════

use rexon_core::appearance_page::GalleryFrame;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn gallery_drops_the_face_of_the_product_like_the_site_does() {
    let _lang = i18n_pin::ru();
    let full = |u: &str| rexon_core::ocs::full_preview_url(u);
    let urls: Vec<String> = ["a.png", "b.png", "c.png"].iter().map(|s| s.to_string()).collect();
    // The first slot is the "face of the item", it does not get into the carousel.
    assert_eq!(
        ap::gallery_urls(&urls, "face.png"),
        vec![full("b.png"), full("c.png")]
    );
    // One picture — we show it.
    assert_eq!(ap::gallery_urls(&["only.png".into()], ""), vec![full("only.png")]);
    // No list at all — we take the tile preview.
    assert_eq!(ap::gallery_urls(&[], "p.png"), vec![full("p.png")]);
    assert!(ap::gallery_urls(&[], "").is_empty());
}

#[test]
fn tile_falls_back_to_the_next_picture_when_the_first_is_dead() {
    // «Sirin Cursor» (2370501), the store's answer of 03.10.2026: previewpic1 is
    // a GIF with hieroglyphs, the server answers 404; the rest are downscaled copies.
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
    // First the tile preview, then the rest by number, without duplicates,
    // as the last reserve — the originals of the downscaled copies.
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
    // The item has one picture — no reserve.
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
    // 0 1 2 3: the broken 1 and 2 are skipped when paging in both directions.
    assert_eq!(ap::gallery_step_skipping(0, 4, 1, &failed), 3);
    assert_eq!(ap::gallery_step_skipping(3, 4, -1, &failed), 0);
    assert_eq!(ap::gallery_step_skipping(3, 4, 1, &failed), 0);
    // The counter counts only live ones: «2/2» on the third frame of four.
    assert_eq!(ap::gallery_counter_skipping(3, 4, &failed).as_deref(), Some("2/2"));
    assert_eq!(ap::gallery_counter_skipping(0, 4, &failed).as_deref(), Some("1/2"));
    // Only one live one is left — nowhere to page to, no counter.
    let all_but_0: BTreeSet<i64> = [1, 2, 3].into_iter().collect();
    assert_eq!(ap::gallery_step_skipping(0, 4, 1, &all_but_0), 0);
    assert_eq!(ap::gallery_counter_skipping(0, 4, &all_but_0), None);
    // Without broken ones — ordinary looped paging.
    assert_eq!(ap::gallery_step_skipping(3, 4, 1, &BTreeSet::new()), 0);
}

#[test]
fn gallery_arrows_wrap_around() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::gallery_step(0, 5, 1), 1);
    assert_eq!(ap::gallery_step(4, 5, 1), 0);
    assert_eq!(ap::gallery_step(0, 5, -1), 4);
    // One frame — nothing to page through.
    assert_eq!(ap::gallery_step(0, 1, 1), 0);
    assert_eq!(ap::gallery_step(0, 0, -1), 0);
}

#[test]
fn gallery_counter_only_grows() {
    let _lang = i18n_pin::ru();
    // Full details bring more links than the list: «2/5» must not
    // collapse back into «2/1».
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
    // The frame is there.
    assert_eq!(
        ap::gallery_frame(0, &loaded, &failed, false, 5),
        GalleryFrame::Image("/c/0.png".into())
    );
    // No frame, the walk is still going on — we wait.
    assert_eq!(ap::gallery_frame(1, &loaded, &failed, false, 5), GalleryFrame::Loading);
    // The server refused exactly this frame.
    assert_eq!(ap::gallery_frame(2, &loaded, &failed, false, 5), GalleryFrame::Failed);
    // The walk is finished — the "hole" will not be filled any more.
    assert_eq!(ap::gallery_frame(1, &loaded, &failed, true, 5), GalleryFrame::Failed);
    // A single picture — the panel stays as it was.
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &failed, true, 1),
        GalleryFrame::Unchanged
    );
    // No pictures at all and the walk is finished — «нет превью», not an eternal
    // wait.
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &BTreeSet::new(), true, 0),
        GalleryFrame::Unavailable
    );
    // The walk is still going on — we wait for now.
    assert_eq!(
        ap::gallery_frame(0, &BTreeMap::new(), &BTreeSet::new(), false, 0),
        GalleryFrame::Unchanged
    );
    // The only frame did not arrive.
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
    // The current frame is not among the ready ones — we move to the earliest ready one.
    assert_eq!(ap::adopt_gallery_index(&loaded, 0, false), Some(2));
    // The person has already paged by hand — we do not override their frame.
    assert_eq!(ap::adopt_gallery_index(&loaded, 0, true), None);
    // The current frame is ready — nowhere to move.
    assert_eq!(ap::adopt_gallery_index(&loaded, 2, false), None);
    assert_eq!(ap::adopt_gallery_index(&BTreeMap::new(), 0, false), None);
}

#[test]
fn install_button_has_three_states() {
    let _lang = i18n_pin::ru();
    // As in KDE: «Установить» only installs, the next press applies.
    let b = ap::install_button(false, false, true);
    assert_eq!(b.text, "Установить");
    assert_eq!(b.state, "false");
    assert!(b.tooltip.is_empty());
    let b = ap::install_button(true, false, true);
    assert_eq!(b.text, "🎨 Применить");
    assert_eq!(b.state, "installed");
    assert!(!b.tooltip.is_empty());
    // A kind without applying (fonts, widgets) — only «Установлено».
    let b = ap::install_button(true, false, false);
    assert_eq!(b.text, "✅ Установлено");
    assert_eq!(b.state, "installed");
    let b = ap::install_button(true, true, true);
    assert_eq!(b.text, "✅ Применено");
    assert_eq!(b.state, "true");
    // "Applied" wins: the theme may have been applied without being marked installed.
    assert_eq!(ap::install_button(false, true, true).state, "true");
    assert_eq!(ap::status_prefix(true), "✅ ");
    assert_eq!(ap::status_prefix(false), "❌ ");
}

#[test]
fn local_theme_name_comes_from_the_synthetic_id() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::local_theme_name("local:colors:Breeze Dark", "не то"), "Breeze Dark");
    // A theme name can contain a colon — it must survive.
    assert_eq!(ap::local_theme_name("local:gtk:A:B", "x"), "A:B");
    // Not a local card — the ordinary name.
    assert_eq!(ap::local_theme_name("123456", "Sweet KDE"), "Sweet KDE");
    // The number is corrupted — also the name.
    assert_eq!(ap::local_theme_name("local:gtk", "Sweet"), "Sweet");
    assert!(ap::stale_global_msg("Nord").contains("«Nord»"));
}

#[test]
fn detail_size_and_copy_link_labels() {
    let _lang = i18n_pin::ru();
    // The store's `downloadsize1` is in KiB: live 13 and 1419 (Discover: 13 KiB, 1.4 MiB).
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
    // The size is unknown — the line does not change.
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
    webp[20] = 0; // no animation flag — a still WebP
    assert!(!ap::is_animated_image_header(&webp));
}

#[test]
fn detail_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        ap::detail_meta_line("EliverLara", "GPL-3.0", "2026-07-29 03:00:00", "5.0"),
        "EliverLara • GPL-3.0 • обновлено 2026-07-29 v5.0"
    );
    // The store also returns the version with a letter — a second "v" must not be added.
    assert_eq!(
        ap::detail_meta_line("A", "B", "2026-01-01", "v5.0"),
        "A • B • обновлено 2026-01-01 v5.0"
    );
    // Empty fields — a dash, there is simply no version.
    assert_eq!(ap::detail_meta_line("", "", "", ""), "— • — • обновлено — ");
    assert_eq!(ap::detail_stars_label(Some(4.35)), "⭐ 4,3"); // the separator depends on the language (i18n::format_decimal)
    assert_eq!(ap::detail_stars_label(Some(5.0)), "⭐ 5,0"); // not «⭐ 5»
    assert_eq!(ap::detail_stars_label(Some(0.0)), "⭐ 0,0");
    // No votes — neither a star nor «2,5» (the OCS starting score).
    assert_eq!(ap::detail_stars_label(None), "Оценок пока нет");
    assert!(ap::detail_downloads_label(1234).contains("скачиваний: 1,2K"));
    assert_eq!(
        ap::detail_path_label("Глобальные темы", "~/.local/share/plasma"),
        "Категория: Глобальные темы\nПуть установки: ~/.local/share/plasma"
    );
}

#[test]
fn description_and_comment_toggles_match_the_original() {
    let _lang = i18n_pin::ru();
    // No translation — no switch either.
    assert_eq!(ap::description_html("ориг", "ориг", false, false, "#66c0f4"), "ориг");
    assert_eq!(ap::description_html("", "", false, false, "#66c0f4"), "<i>Без описания</i>");
    // There is a translation — we show it and offer the original.
    let h = ap::description_html("ориг", "пер", true, false, "#66c0f4");
    assert!(h.starts_with("пер<br><a href='rexon:toggle'"), "{h}");
    assert!(h.contains("color:#66c0f4"), "{h}");
    assert!(h.contains("📄 показать оригинал"), "{h}");
    assert!(h.contains("font-size:9.5pt"), "{h}");
    // Switched — we show the original and offer the translation.
    let h = ap::description_html("ориг", "пер", true, true, "#66c0f4");
    assert!(h.starts_with("ориг<br>"), "{h}");
    assert!(h.contains("🌐 показать перевод"), "{h}");
    // A comment: the same technique, its own captions and font size.
    let h = ap::comment_body_html("o", "t", true, false, "#66c0f4");
    assert!(h.starts_with("t&nbsp;&nbsp;<a href='rexon:toggle'"), "{h}");
    assert!(h.contains("📄 оригинал") && h.contains("font-size:9pt"), "{h}");
    assert_eq!(ap::comment_body_html("o", "t", false, false, "#66c0f4"), "t");
}

/// The second argument of `description_html` is the VISIBLE description
/// (`ThemeItem::display_description`), not the translation field.
///
/// The difference is visible only when there is NO translation: the panel must show
/// the store's English text, not «Без описания». The Rust tab had
/// `description_translated` there, and until a live translation appeared descriptions were
/// not shown at all; Python had in this place
/// `_desc_trans = translation if was_translated else original`.
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
    // No translation model — the store text is in place, no switch.
    assert_eq!(shown(&item, false), "The Papirus-Mace theme.");
    // The translation arrived — we show it and offer the original.
    item.description_translated = "Тема Papirus-Mace.".into();
    assert!(
        shown(&item, false).starts_with("Тема Papirus-Mace.<br><a href='rexon:toggle'"),
        "{}",
        shown(&item, false)
    );
    // «показать оригинал» was pressed — the English text came back.
    assert!(shown(&item, true).starts_with("The Papirus-Mace theme.<br>"));
    // No description at all — a placeholder, and still without a switch.
    let empty = ThemeItem::default();
    assert_eq!(shown(&empty, false), ap::no_description_html());
}

#[test]
fn comment_text_is_stripped_and_escaped_like_the_original() {
    let _lang = i18n_pin::ru();
    // The golden value was captured from Python itself (html.unescape → strip tags → html.escape).
    for (raw, want) in [
        ("<b>жирный</b> текст", " жирный  текст"),
        ("a &amp; b", "a &amp; b"),
        // An already escaped tag becomes real after unescape and
        // is stripped — otherwise markup from a comment would get into the caption.
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
        // The original's casefold equates «ß» with «ss».
        ("Straße", "strasse"),
    ] {
        assert_eq!(ap::norm_author(raw), want, "вход {raw:?}");
    }
    assert!(ap::comment_is_product_author("EliverLara", "eliverlara"));
    assert!(ap::comment_is_product_author("  John Doe ", "john  doe"));
    // An empty nickname is never considered the item's author.
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
    // Depth with a ceiling: deeper than the fifth level the indent would eat the whole width.
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
    // The exact number came with the item details.
    assert_eq!(ap::comments_header_text(Some(42), 10, true), "💬 Комментарии (42)");
    // From the cache the number is lost — an honest "shown N(+)".
    assert_eq!(ap::comments_header_text(None, 10, true), "💬 Комментарии (10+)");
    assert_eq!(ap::comments_header_text(None, 3, false), "💬 Комментарии (3)");
    assert_eq!(ap::comments_header_text(Some(0), 3, false), "💬 Комментарии (3)");
    // "There is more" is counted by ROOT ones: replies do not fill the page.
    let roots: Vec<i64> = vec![0; 10];
    assert!(ap::comments_has_more(&roots));
    let mixed: Vec<i64> = vec![0, 1, 1, 0, 2, 0, 0, 0, 0, 0, 1, 1];
    assert!(!ap::comments_has_more(&mixed), "корневых меньше страницы");
    assert!(!ap::comments_has_more(&[]));
}

// ═══════════════════════════════════════════════════════════════════════
//  Lifecycle of catalog requests
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
    // The person came back to the same category while the request is still in flight.
    let g2 = r.bump();
    r.schedule(g2, "cat=722&p=0");
    assert_eq!(r.fire(), FireOutcome::Deduplicated { key: "cat=722&p=0".into() });
    // The in-flight answer is accepted as fresh: its generation was updated.
    assert!(!r.answer("cat=722&p=0"));
}

#[test]
fn an_answer_from_a_category_already_left_is_dropped() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    let g = r.bump();
    r.schedule(g, "a");
    assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    r.bump(); // went to another category
    assert!(r.answer("a"), "ответ прежней категории обязан быть выброшен");
    // An answer nobody is waiting for (a cache hit) is not affected by this.
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
    // And even one submitted by hand with a stale generation does not go away.
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
    // The person's live request for a page also counts as a warm-up.
    let g = r.bump();
    r.schedule(g, "p2");
    assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    assert!(!r.claim_page_prefetch("p2"));
    // Resetting the list cache by hand allows warming again.
    r.forget_page_prefetches();
    assert!(r.claim_page_prefetch("p1"));
}

#[test]
fn the_warm_up_yields_the_channel_to_a_live_request() {
    let _lang = i18n_pin::ru();
    let mut r = CatalogRequests::new();
    r.enqueue_prefetch(&[("722".into(), "new".into()), ("104".into(), "new".into())]);
    assert_eq!(r.prefetch_queue_len(), 2);
    // While the open category is still loading, we do not start the background.
    let g = r.bump();
    r.schedule(g, "live");
    assert_eq!(r.next_prefetch(), None, "отложенный запрос уже занимает канал");
    assert!(matches!(r.fire(), FireOutcome::Send { .. }));
    assert_eq!(r.next_prefetch(), None, "запрос в полёте");
    // The answer came — the queue catches up.
    r.answer("live");
    let first = r.next_prefetch().expect("очередь должна тронуться");
    // One at a time: while this warm-up is going, we do not hand out the next.
    assert_eq!(r.next_prefetch(), None);
    r.prefetch_finished(first.clone());
    let second = r.next_prefetch().expect("следующая цель");
    assert_ne!(first, second);
    r.prefetch_finished(second);
    assert_eq!(r.next_prefetch(), None);
    // What is already warm does not come back into the queue.
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
    // No duplicate is left: reordered, not added.
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
    // The details panel has no suggestions — there is also no grid there for them to lead to.
    assert_eq!(ap::suggest_action("nord", false, &v), SuggestAction::Hide);
    // Sections without a network filter the already loaded list in place.
    assert_eq!(ap::suggest_action("nord", true, &view("downloaded")), SuggestAction::Local);
    let mut v = store("global");
    v.source = "aur".into();
    assert_eq!(ap::suggest_action("nord", true, &v), SuggestAction::Local);
}

#[test]
fn a_stale_suggestion_answer_is_dropped() {
    let _lang = i18n_pin::ru();
    assert!(ap::suggest_answer_fresh(5, 5, true, "nord"));
    // The person typed on.
    assert!(!ap::suggest_answer_fresh(4, 5, true, "nord"));
    // Left the search line.
    assert!(!ap::suggest_answer_fresh(5, 5, false, "nord"));
    // Erased what was typed.
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
//  Catalog tiles
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn card_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::card_name(""), "—");
    assert_eq!(ap::card_name("Sweet KDE"), "Sweet KDE");
    assert_eq!(ap::card_author(""), "—");
    // Stat line slots: rating · downloads · date (the day without time).
    let nb = "\u{a0}";
    assert_eq!(
        ap::card_stats_line(Some(4.4), 123456, "2026-07-29 03:00:00"),
        format!("{nb}4,4{nb}{nb}⬇️ 123,5K{nb}{nb}📅 2026-07-29")
    );
    // No date — the slot is simply absent, we do not draw "—" instead.
    assert_eq!(
        ap::card_stats_line(Some(5.0), 0, ""),
        format!("{nb}5,0{nb}{nb}⬇️ 0")
    );
    // An item without votes (OCS gives it score 50): instead of «★ 2,5» —
    // a caption, and no star on the tile.
    assert_eq!(
        ap::card_stats_line(None, 12, "2026-11-16"),
        format!("Оценок пока нет{nb}{nb}⬇️ 12{nb}{nb}📅 2026-11-16")
    );
    assert_eq!(ap::card_star_glyph(None), "");
    assert_eq!(ap::card_star_glyph(Some(4.4)), "⭐");
    let unrated = rexon_core::models::ThemeItem {
        rating: rexon_core::ocs::UNRATED_SCORE,
        ..Default::default()
    };
    assert_eq!(unrated.rated_stars(), None);
    let rated = rexon_core::models::ThemeItem { rating: 58, ..Default::default() };
    assert_eq!(rated.rated_stars(), Some(2.9));
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
    // Installed but not enabled — the same button is now «Применить».
    let b = ap::card_install_button(true, false, true);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("🎨 Применить", "true", true));
    let b = ap::card_install_button(true, false, false);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("✅ Установлено", "true", true));
    let b = ap::card_install_button(true, true, true);
    assert_eq!((b.text.as_str(), b.state, b.enabled), ("✅ Применено", "applied", true));
    // What a press does: not installed — only install; installed and applicable —
    // apply what is installed (without downloading again); installed, but nothing to apply —
    // install again.
    assert_eq!(ap::store_card_action(false, true), ap::StoreCardAction::Install);
    assert_eq!(ap::store_card_action(true, true), ap::StoreCardAction::ApplyInstalled);
    assert_eq!(ap::store_card_action(true, false), ap::StoreCardAction::Install);
    // A local tile: an enabled theme shows a badge and cannot be pressed.
    let b = ap::card_local_button("active");
    assert_eq!((b.text.as_str(), b.enabled), ("✅ Активна", false));
    let b = ap::card_local_button("downloaded");
    assert_eq!((b.text.as_str(), b.enabled), ("🎨 Применить", true));
    assert_eq!(ap::card_local_button("system").text, "🎨 Применить");
    // Favourites.
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
    // Version width: the remainder from the caption, but not below the readable minimum.
    assert_eq!(ap::aur_version_budget(270, 100), 170);
    assert_eq!(ap::aur_version_budget(270, 500), 40);
    // The stat line has the same slots as the store tile.
    assert_eq!(
        ap::aur_stats_line(1234, 3.456, "2026-07-29"),
        format!("{nb}1,2K{nb}{nb}📈 3,5{nb}{nb}📅 2026-07-29")
    );
    // Zero popularity and no date — the slots are not drawn.
    assert_eq!(ap::aur_stats_line(7, 0.0, ""), format!("{nb}7"));
    assert_eq!(
        ap::aur_stats_tooltip(1234, 3.456, "2026-07-29"),
        "👍 голосов: 1234 · 📈 популярность AUR 3,46 · обновлён 2026-07-29"
    );
    assert!(ap::aur_stats_tooltip(0, 0.0, "").contains("обновлён —"));
}

#[test]
fn aur_button_explains_why_it_cannot_install() {
    let _lang = i18n_pin::ru();
    let b = ap::aur_install_button(false, true);
    assert!(b.enabled && b.text == "Установить");
    assert!(b.tooltip.contains("Сборка из исходников"));
    // No helper — we say why instead of silently leaving a dead button.
    let b = ap::aur_install_button(false, false);
    assert!(!b.enabled);
    assert_eq!(
        b.tooltip,
        "Не найден сборщик пакетов AUR (paru или yay) — установка недоступна"
    );
    // For the buyer — without jargon (rule #13): «AUR-хелпер» is replaced with the words
    // the decoration rebuild already uses for the same refusal.
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
    // Already installed.
    let b = ap::aur_install_button(true, true);
    assert!(!b.enabled && b.text == "✅ Установлен");
    // The build is in progress.
    assert_eq!(ap::aur_busy_button(true).text, "⏳ Сборка…");
    assert!(!ap::aur_busy_button(true).enabled);
    assert!(ap::aur_busy_button(false).enabled);
}

// ═══════════════════════════════════════════════════════════════════════
//  The tab header and overlay controls
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chrome_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::page_title(), "Оформление");
    assert_eq!(ap::sidebar_row_text("🎨", "Глобальные темы"), "  🎨  Глобальные темы");
    assert_eq!(ap::section_title("🎨", "Глобальные темы"), "🎨  Глобальные темы");
    // Three sources, the keys match the ones the catalog state distinguishes.
    let keys: Vec<&str> = ap::source_chips().iter().map(|c| c.key).collect();
    assert_eq!(keys, vec!["ocs", "aur", "both"]);
    assert!(ap::source_chips().iter().all(|c| !c.label.is_empty() && !c.tooltip.is_empty()));
    // Two lock screen modes, and both are explained — the section is hybrid.
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
    // The Kvantum engine — by its themes; the qtNct proxies are not a look.
    assert_eq!(ap::shown_qt_styles(&styles), vec!["Breeze", "Fusion", "Windows"]);
    let keys: Vec<&str> = ap::style_filter_chips().iter().map(|c| c.key).collect();
    assert_eq!(keys, ["all", "qt", "kvantum"]);
    assert_eq!(ap::style_filter_chips()[0].label, "Все");
    // The installed grid, the filter above it; the store — Kvantum themes (123) and AUR.
    let v = view("widgetstyle");
    assert!(v.installed_view() && v.marks_current());
    let c = v.chrome();
    assert!(c.style_bar && c.pagination);
    assert_eq!(v.query_category_id(), "123");
    assert!(v.source_applicable(), "темы Kvantum и стили Qt есть в AUR");
    assert!(rexon_core::aur::has_category("widgetstyle"));
    // Namesakes of style families with a different purpose do not get on the shelf (AUR snapshot).
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
    // The magnifier — the top right corner, with a margin.
    let p = ap::overlay_zoom_pos(600, 44);
    assert_eq!((p.x, p.y), (600 - 44 - 10, 10));
    // Arrows — on the sides, vertically centred.
    let p = ap::overlay_prev_pos(340, 44);
    assert_eq!((p.x, p.y), (10, (340 - 44) / 2));
    let p = ap::overlay_next_pos(600, 340, 44, 44);
    assert_eq!((p.x, p.y), (600 - 44 - 10, (340 - 44) / 2));
    // The counter — bottom centre.
    let p = ap::overlay_counter_pos(600, 340, 48, 24);
    assert_eq!((p.x, p.y), ((600 - 48) / 2, 340 - 24 - 10));
    // The picture is smaller than the control — we do not slide past the left/top edge.
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
    // The current frame among the downloaded ones — we page in full-screen view too.
    let (paths, idx) = ap::zoom_frames("/c/1.png", &loaded, exists).unwrap();
    assert_eq!(paths, vec!["/c/0.png", "/c/1.png"]);
    assert_eq!(idx, 1);
    // A single preview of a local theme: we do not put others' pictures into the list.
    let (paths, idx) = ap::zoom_frames("/local/preview.png", &loaded, exists).unwrap();
    assert_eq!(paths, vec!["/local/preview.png"]);
    assert_eq!(idx, 0);
    // Nothing to show.
    assert!(ap::zoom_frames("", &loaded, exists).is_none());
    assert!(ap::zoom_frames("/c/gone.png", &loaded, exists).is_none());
}

/// Guard of agreement between the style sheet and the bare colors.
///
/// QSS does not work in QML, so the tile button has two forms of one decision:
/// a string for QtWidgets (`page_qss`) and bare colors for QML
/// (`card_button_colors`). They must not drift apart — the grid and the detail
/// would tell different things about the item.
#[test]
fn widget_qss_and_qml_button_colors_agree() {
    let _lang = i18n_pin::ru();
    let qss = ap::page_qss(&ap::colors());

    // «✓ Применено» — filled teal.
    let (bg, fg, border) = ap::card_button_colors("applied");
    assert_eq!((bg, fg, border), ("#2f9e94", "#ffffff", "#2f9e94"));
    assert!(qss.contains("background: #2f9e94;"));
    assert!(qss.contains("border: 1px solid #2f9e94;"));

    // «✓ Установлено» — outlined teal: no fill at all.
    let (bg, fg, border) = ap::card_button_colors("true");
    assert_eq!((bg, fg, border), ("", "#7fe3da", "#4fd1c5"));
    assert!(qss.contains("background: transparent;"));
    assert!(qss.contains("border: 1px solid #4fd1c5;"));
    assert!(qss.contains("color: #7fe3da;"));

    // «Установить» — the color of the active skin, not its own hard-coded one: the token name
    // must exist in the palette, otherwise the button would stay colorless.
    let (bg, fg, border) = ap::card_button_colors("false");
    assert_eq!((fg, border), ("#ffffff", ""));
    assert!(ap::colors().contains_key(bg), "нет токена {bg} в палитре");
}

/// The smallest tab window size is the same as for the neighbouring tabs.
#[test]
fn window_minimum_is_the_product_wide_one() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::WINDOW_MIN_SIZE, (900, 720));
    assert_eq!(ap::PAGE_TITLE_POINT_SIZE, 15);
}

/// The details panel captions are written out from the original VERBATIM.
#[test]
fn detail_button_labels_match_the_original() {
    let _lang = i18n_pin::ru();
    // ⚠️ Two spaces after the arrow — that is how it is in the original.
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

/// Guard of agreement between the style sheet and the bare colors of the main details button.
#[test]
fn widget_qss_and_qml_detail_button_colors_agree() {
    let _lang = i18n_pin::ru();
    let qss = ap::page_qss(&ap::colors());

    let (bg, fg, border) = ap::detail_button_colors("true");
    assert_eq!((bg, fg, border), ("#3a5a3f", "#cfe8d2", ""));
    assert!(qss.contains("background: #3a5a3f;"));
    assert!(qss.contains("color: #cfe8d2;"));

    // «✓ Установлено» on the details — the same outlined look as on the tile.
    assert_eq!(ap::detail_button_colors("installed"), ap::card_button_colors("true"));

    let (bg, fg, border) = ap::detail_button_colors("false");
    assert_eq!((fg, border), ("#ffffff", ""));
    assert!(ap::colors().contains_key(bg), "нет токена {bg} в палитре");
}

// ═══════════════════════════════════════════════════════════════════════
//  Installation progress
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

/// Only a download from the store reports the completed fraction: an AUR helper
/// build and applying a theme have none, and a bar at zero would lie about progress.
#[test]
fn only_the_store_download_shows_a_progress_bar() {
    let _lang = i18n_pin::ru();
    assert!(ap::action_shows_progress(ap::ACTION_INSTALL));
    assert!(!ap::action_shows_progress(ap::ACTION_AUR));
    assert!(!ap::action_shows_progress(ap::ACTION_APPLY));
}

/// The progress bar must stay a bar: the installer occasionally reports both
/// more than a hundred and less than zero.
#[test]
fn install_percent_stays_inside_the_bar() {
    let _lang = i18n_pin::ru();
    assert_eq!(ap::install_percent(-5), 0);
    assert_eq!(ap::install_percent(0), 0);
    assert_eq!(ap::install_percent(37), 37);
    assert_eq!(ap::install_percent(100), 100);
    assert_eq!(ap::install_percent(140), 100);
}

/// A finished result must not have a second icon: the installer sends
/// messages with its own, and «✅ ✅ Установлено» was a defect of the original.
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
    // Leading whitespace before the icon does not spoil the message.
    assert_eq!(ap::action_result_status(true, "  ✅ Готово"), "  ✅ Готово");
}

/// Only a successful theme apply rebuilds the grid: the «✅ Активна» badge
/// must move. Installation and failure do not touch it.
#[test]
fn only_a_successful_apply_rebuilds_the_grid() {
    let _lang = i18n_pin::ru();
    assert!(ap::reload_after_action(ap::ACTION_APPLY, true));
    assert!(!ap::reload_after_action(ap::ACTION_APPLY, false));
    assert!(!ap::reload_after_action(ap::ACTION_INSTALL, true));
    assert!(!ap::reload_after_action(ap::ACTION_AUR, true));
}

// ═══════════════════════════════════════════════════════════════════════
//  Controls overridden by the tab's style sheet
// ═══════════════════════════════════════════════════════════════════════

/// The body of a style sheet rule by its selector.
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

/// The value of a property inside the rule body, or `None` if it is not there.
fn qss_value_opt<'a>(body: &'a str, prop: &str) -> Option<&'a str> {
    let head = format!("{prop}:");
    let start = body.find(&head)? + head.len();
    let end = start
        + body[start..]
            .find(';')
            .unwrap_or_else(|| panic!("свойство {prop} не закрыто"));
    Some(body[start..end].trim())
}

/// The same, but the property must be present.
fn qss_value<'a>(body: &'a str, prop: &str) -> &'a str {
    qss_value_opt(body, prop).unwrap_or_else(|| panic!("в правиле нет свойства {prop}"))
}

/// Guard of agreement between the style sheet and the bare paints of the tab's CONTROLS.
///
/// ⚠️ WHY. The common `ui_base` surfaces answer with what Qt paints a control
/// ITSELF by palette role. In this tab the role takes no part: the controls are painted by
/// its own style sheet. As long as both sides take the decision from here,
/// QML cannot again pick a token that "looks similar".
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
            // No highlight of its own — the paints under the cursor must match
            // the resting ones, otherwise QML would draw what the original does not have.
            assert_eq!((s.hover, s.border), (s.bg, s.hover_border), "{kind}: курсор");
            continue;
        }
        let hover = qss_rule(&qss, hover_selector);
        // The hover rule does not override everything: for a secondary button
        // only the border changes. Whatever is not in it stays from the resting look,
        // and the bare paints must say the same.
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

    // Token names must exist in the palette of ANY skin: otherwise on
    // another skin the control would stay colorless.
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
//  Source language
// ═══════════════════════════════════════════════════════════════════════

/// In English the tab's captions are the catalog's source strings. The checks above
/// hold the Russian look; this one — that without a catalog the person sees English, not
/// Russian leftovers and not emptiness.
#[test]
fn english_texts() {
    let _lang = i18n_pin::en();
    assert_eq!(ap::page_title(), "Appearance");
    assert_eq!(ap::sort_label(ap::SORTS[0].0), "🆕 Newest");
    assert_eq!(ap::pagination(0, 318, 18, ap::PAGE_SIZE).label, "Page 1 / 16");
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
    // Every source caption above is translated — there is no empty space in Russian.
    for msgid in ["Appearance", "✅ Active", "Nothing found", "Page %1 / %2"] {
        assert!(ru.get(None, msgid).is_some_and(|t| !t.is_empty()), "{msgid}");
    }
    // The different Russian forms of one English "✅ Installed" are separated by
    // context: a theme «Установлено», an AUR package «Установлен» (without context —
    // a desktop in the desktops tab, also «Установлен»).
    assert_eq!(ru.get(Some("theme"), "✅ Installed"), Some("✅ Установлено"));
    assert_eq!(ru.get(Some("AUR package"), "✅ Installed"), Some("✅ Установлен"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Login screen: which screen the section has with PLM / SDDM / SteamOS (28.09.2026)
// ═══════════════════════════════════════════════════════════════════════

fn plm_view() -> CatalogView {
    CatalogView::new(categories::plasma_login_category(), "kde")
}

#[test]
fn login_section_screen_by_login_manager() {
    let _lang = i18n_pin::en();
    let home = Path::new("/home/u");

    // SDDM: the store shelf, as it was.
    let sddm = view("sddm");
    assert_eq!(sddm.screen(home), ap::SCREEN_GRID);
    assert!(sddm.note(home).is_none());
    let c = sddm.chrome();
    assert!(c.search && c.sort_combo && c.pagination, "у SDDM-полки шапка магазина");

    // Plasma Login Manager: its own panel, no search, no sorting, no network.
    let plm = plm_view();
    assert_eq!(plm.screen(home), ap::SCREEN_PLASMA_LOGIN);
    assert!(plm.note(home).is_none());
    let c = plm.chrome();
    assert!(c.caveat.as_deref().unwrap_or_default().contains("Shared by all desktops"), "{c:?}");
    assert_eq!(ap::Chrome { caveat: None, ..c }, ap::Chrome::default());
    assert!(plm.is_local(), "в магазин за разделом PLM не ходят");

    // Warm-up: the PLM section has no store.
    let sidebar = categories::sidebar_for(home, true);
    let targets = ap::prefetch_targets(&plm, &sidebar);
    assert!(targets.iter().all(|(id, _)| id != "101" && id != "905"), "{targets:?}");
}

/// The login screen and the boot splash — like kcm sddm and kcm plymouth: their own
/// grid of installed themes with the current one highlighted, the store below it; the scan is of
/// the whole section (the system themes directory). Plasma Login Manager has no themes —
/// no grid; on SteamOS the section explains the refusal instead of showing a grid.
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
    // PLM on SteamOS is also a login screen: the reason matters more than the panel.
    let mut plm = plm_view();
    plm.steamos = true;
    assert_eq!(plm.screen(home), ap::SCREEN_NOTE);
    // Other sections on SteamOS are not affected.
    let mut global = view("global");
    global.steamos = true;
    assert!(!global.steamos_blocked());
    assert_eq!(global.screen(home), ap::SCREEN_GRID);

    // The warm-up does not warm the storefronts closed on SteamOS.
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
        // Nothing to open with — no button, the text itself says where to go.
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

/// Sections with installed items and a store (KDE, desktops, Conky, splash,
/// full lock screen themes) show the installed items on top, and below them on
/// the same page — the section's store, right away and without the «Загрузить
/// новые…» button (owner's decision 18.10.2026: the button was confusing — in
/// «Виджеты Conky» one card was visible, while 1032 widgets were hidden).
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
        // The query key carries both the scan and the store page.
        let k = v.request_key();
        assert!(k.starts_with(&format!("combined:{key}:")), "{key}: {k}");
        v.page = 1;
        assert_ne!(v.request_key(), k, "{key}: другая страница магазина — другой запрос");
        v.page = 0;
        // Search — over both the installed items and the store.
        v.query = "bree".into();
        assert!(v.request_key().ends_with(":bree"), "{key}");
    }
    // Lock screen: installed items — only for full themes.
    let mut v = view("lockscreen");
    assert!(!v.has_installed_view());
    v.lock_mode = "themes".into();
    assert!(v.installed_view());
    assert!(v.request_key().starts_with("combined:lockscreen:"));
    // Disk overviews — still only the disk; in «Активные» there are many
    // enabled ones — nothing is highlighted with a frame there.
    for key in ["active", "downloaded"] {
        assert!(view(key).is_local(), "{key}");
    }
    assert!(!view("wallpapers").installed_view());
    assert!(!view("active").marks_current());
    assert_eq!(ap::installed_status(3), "Установлено: 3");
    // The status line — both parts; without installed items — only the store.
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
    // Enabled, could not be read, another section — no banner.
    assert!(!ap::event_sounds_banner("sounds", "true\n").visible);
    assert!(!ap::event_sounds_banner("sounds", "").visible, "не знаем — не пугаем");
    assert!(!ap::event_sounds_banner("icons", "false").visible);
}

#[test]
fn conky_section_warns_before_start_and_on_unreliable_wayland() {
    let _lang = i18n_pin::ru();
    use rexon_core::de_apply::{conky_package, conky_section_note, conky_wayland_note};
    // No program — said in advance, and that Rexon will install it itself where it can.
    let mut v = CatalogView::new(cat("conky"), "mate");
    v.conky_installed = false;
    v.conky_installable = true;
    let c = v.chrome().caveat.unwrap();
    assert!(c.contains("Conky") && c.contains("установит её"), "{c}");
    v.conky_installable = false;
    assert!(v.chrome().caveat.unwrap().contains("conky"), "SteamOS: только предупреждение");
    v.conky_installed = true;
    assert!(v.chrome().caveat.is_none(), "MATE на X11 с Conky — оговорок нет");
    // Wayland: GNOME — unreliable; KDE and tiling ones draw by themselves.
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
    // No Conky — it is installed by itself, without a «Да/Нет» question (the password window is the safeguard).
    assert_eq!(conky_install_package(false, Some("conky")).as_deref(), Some("conky"));
    // Conky is already installed — nothing to install; nothing to install with (SteamOS) — the same.
    assert!(conky_install_package(true, Some("conky")).is_none());
    assert!(conky_install_package(false, None).is_none());
    // The password was not confirmed — the widget is not launched, advice to repeat.
    let text = conky_install_failed(&rexon_core::appearance_installer::password_not_confirmed());
    assert_eq!(text, "Виджет не запущен: ему нужна программа Conky. Нажмите «▶️ Запустить» ещё раз и введите пароль, чтобы её установить.");
    // A package manager error — with the reason.
    let text = conky_install_failed("x");
    assert!(text.contains("x") && text.contains("не запущен"), "{text}");
    // Whose widget this is: store card 124, former number 919, a local tile.
    let store = ThemeItem { id: "1".into(), category_id: 124, ..Default::default() };
    let old = ThemeItem { id: "2".into(), category_id: 919, ..Default::default() };
    let local = ThemeItem { id: "local:conky:clock".into(), ..Default::default() };
    let other = ThemeItem { id: "3".into(), category_id: 132, ..Default::default() };
    assert!(is_conky_item(&store) && is_conky_item(&old) && is_conky_item(&local));
    assert!(!is_conky_item(&other));
}

/// Rexon does not offer Conky Manager (owner's decision 24.10: everything it
/// can do is in the «Виджеты Conky» section itself): neither a card nor installation
/// together with Conky; the store items «Conky Manager GTK/Qt» do not go into the grid.
/// Rexon does not touch an already installed program.
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

/// An installed Conky widget in the section catalog is launched right from the tile
/// and from the details panel (live check 15.11.2026: after «Установить»
/// the tile froze on «✅ Установлено», while «▶️ Запустить» was only in
/// «Загруженные»). The button and the number are the same as those of the installed tile.
#[test]
fn installed_conky_widget_starts_from_the_store_tile() {
    use rexon_core::de_apply::Button;
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let conky = ThemeItem { id: "2203005".into(), category_id: rexon_core::categories::CONKY_SECTION_ID, ..Default::default() };
    let icons = ThemeItem { id: "1".into(), category_id: 132, ..Default::default() };
    assert_eq!(ap::store_item_switch(home, &conky), Some(Button::Run));
    assert_eq!(ap::store_item_switch(home, &icons), None, "тема — «Применить», не запуск");

    let start = ap::card_store_button(true, false, false, Some(Button::Run));
    assert_eq!((start.text.as_str(), start.state), ("▶️ Запустить", "false"));
    let stop = ap::card_store_button(true, true, false, Some(Button::Run));
    assert_eq!((stop.text.as_str(), stop.state), ("⏹️ Остановить", "true"));
    // The same words as on the installed tile.
    assert_eq!(start, ap::card_local_button_for("downloaded", "local:conky:skyrim"));
    assert_eq!(stop, ap::card_local_button_for("active", "local:conky:skyrim"));
    // Not installed yet — «Установить»; a theme — the former three states.
    assert_eq!(ap::card_store_button(false, false, false, Some(Button::Run)), ap::card_install_button(false, false, false));
    assert_eq!(ap::card_store_button(true, false, true, None), ap::card_install_button(true, false, true));
    let d = ap::store_detail_button(true, false, false, Some(Button::Run));
    assert_eq!((d.text.as_str(), d.state), ("▶️ Запустить", "installed"));
    assert_eq!(ap::store_detail_button(true, true, false, Some(Button::Run)).text, "⏹️ Остановить");
    // Pressing an installed one — launching the same way as «Применить».
    assert_eq!(ap::store_card_action(true, ap::store_item_switch(home, &conky).is_some()), ap::StoreCardAction::ApplyInstalled);

    // The number of the installed tile: «Запускать при входе» and the settings go by it.
    let id = ap::store_local_id(home, &conky, "skyrim").unwrap();
    assert_eq!(id, "local:conky:skyrim");
    assert!(ap::detail_autostart_visible(&id));
    assert_eq!(ap::store_local_id(home, &icons, "Papirus"), None);
}

/// "Running" for an installed widget — by the map of active ones (any number
/// running there, separated by line breaks), as for the installed tile.
#[test]
fn stamp_marks_running_conky_widget() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/home/u");
    let mut log = Map::new();
    let cid = rexon_core::categories::CONKY_SECTION_ID;
    log.insert("7".into(), Value::Object(rec("", cid, &["/home/u/.config/conky/skyrim"])));
    let stamp = |running: &str| {
        let mut items = vec![ThemeItem { id: "7".into(), category_id: cid, ..Default::default() }];
        let running = running.to_string();
        let mut active = ap::LazyActiveMap::new(move || {
            let mut m = Map::new();
            m.insert("conky".into(), json!(running.clone()));
            m
        });
        ap::stamp_installed_from_log(home, &log, &mut items, &mut active);
        (items[0].installed, items[0].applied)
    };
    assert_eq!(stamp("Jhalak\nskyrim"), (true, true));
    assert_eq!(stamp("Jhalak"), (true, false));
    assert_eq!(stamp(""), (true, false));
}
