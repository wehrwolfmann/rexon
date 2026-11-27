//! Check of the `qt_steam.rs` port against the original — the widget tab
//! `rexon/qt_steam/window.py`.
//!
//! The golden here is not a snapshot but the original's strings and numbers themselves, written into
//! the tests: the tab's texts are typed right into the widget construction, and while both
//! versions live side by side, only this check keeps them the same.
//!
//! Sanctioned deviations from the original are named explicitly:
//!   1. the tab's cache and config paths respect `$XDG_CACHE_HOME` and
//!      `$XDG_CONFIG_HOME` (in the original — the literals `~/.cache`/`~/.config`);
//!   2. the dead pair `_THUMB_W, _THUMB_H = 96, 54` was not carried over;
//!   3. the download got the temporary file name as a separate answer
//!      ([`qt_steam::part_path`]) — the original assembled it in place and did not clean it up
//!      on failure;
//!   4. `bg_card` returns the FULL title, and truncation by width is done by the view: the
//!      core has no font metrics. The truncation widths are constants.

mod i18n_pin;

use rexon_core::i18n;
use rexon_core::qt_steam as qs;
use rexon_core::session_splash as ss;
use rexon_core::steam_profile_items::ProfileBackgroundItem;
use rexon_core::ui_base;
use serde_json::{json, Value};

// ── Window and header ───────────────────────────────────────────────────

#[test]
fn window_texts_match_python() {
    assert_eq!(qs::WINDOW_TITLE, "Rexon — Steam");
    assert_eq!(qs::WINDOW_MIN_SIZE, (900, 720));
    assert_eq!(qs::HEADER_TITLE, "🎮 Steam");
    assert_eq!(qs::HEADER_TITLE_POINT_SIZE, 20);
    assert_eq!(qs::GROUP_TITLE_POINT_SIZE, 13);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn header_found_text_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qs::header_found_text(Some("/home/u/.steam/steam")),
        ("Найден: ".to_string(), "/home/u/.steam/steam".to_string())
    );
    assert_eq!(
        qs::header_found_text(None),
        (
            "Steam не найден — установите Steam, чтобы использовать заставки и фоны"
                .to_string(),
            String::new()
        )
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn segments_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qs::segments(),
        vec![
            ("splash", "🎬 Видеозаставка".to_string()),
            ("bg", "🖼️ Steam-фоны".to_string()),
            ("showcase", "🖼️ Витрины профиля".to_string()),
            ("screensaver", "🌙 Заставка".to_string())
        ]
    );
    assert_eq!(qs::segment_index(qs::SEG_SPLASH), 0);
    assert_eq!(qs::segment_index(qs::SEG_BG), 1);
    assert_eq!(qs::segment_index(qs::SEG_SHOWCASE), 2);
    assert_eq!(qs::segment_index(qs::SEG_SCREENSAVER), 3);
    assert!(qs::is_segment(qs::SEG_SCREENSAVER));
    assert!(qs::is_segment(qs::SEG_SHOWCASE) && !qs::is_segment("что-то ещё"));
    // An unknown key behaves as in the original: `0 if key == "splash" else 1`.
    assert_eq!(qs::segment_index("что-то ещё"), 1);
}

// ── Grid sizes ──────────────────────────────────────────────────────────

#[test]
fn grid_sizes_match_python() {
    assert_eq!((qs::SV_THUMB_W, qs::SV_THUMB_H), (176, 110));
    assert_eq!(qs::SV_CARD_W, 200);
    assert_eq!((qs::SV_MIN_COLS, qs::SV_MAX_COLS), (2, 8));
    assert_eq!((qs::SV_HERO_W, qs::SV_HERO_H), (400, 250));
    assert_eq!(qs::SV_TITLE_ELIDE_W, 154);
    assert_eq!(qs::SV_HOVER_DELAY_MS, 220);
    assert_eq!(qs::SV_TEST_PAUSE_S, 25.0);
    assert_eq!(qs::SV_FRAME_MIN_INTERVAL, 1.0 / 15.0);
    assert_eq!(qs::SV_GROUP_FAV, "\u{0}favs");

    assert_eq!((qs::BG_THUMB_W, qs::BG_THUMB_H), (140, 79));
    assert_eq!(qs::BG_CARD_W, 180);
    assert_eq!(qs::BG_ROWS, 5);
    assert_eq!((qs::BG_MIN_COLS, qs::BG_MAX_COLS), (2, 16));
}

// ── Splash screen storefront ────────────────────────────────────────────

fn movie(path: &str, title: &str, game: &str, origin: &str, appid: i64) -> Value {
    json!({
        "path": path,
        "title": title,
        "name": title,
        "game": game,
        "origin": origin,
        "origin_label": ss::origin_label(origin),
        "appid": appid,
    })
}

fn meta_with_event(appid: i64, event: &str, source: &str) -> qs::MetaMap {
    let mut m = qs::MetaMap::new();
    m.insert(
        appid,
        qs::SplashMeta {
            name: "Видеозаставка".into(),
            source: source.into(),
            event: event.into(),
            labels: vec!["Обычный".into(), "Days Gone".into()],
        },
    );
    m
}

/// `_sv_badge_text`: an event beats a game, a game beats the origin.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn badge_text_priority_matches_python() {
    let _lang = i18n_pin::ru();
    let it = movie("/a.webm", "A", "Days Gone", ss::ORIGIN_SHOP, 1259420);
    let empty = qs::MetaMap::new();
    assert_eq!(qs::badge_text(&it, &empty), "Days Gone");
    let with_event = meta_with_event(1259420, "Осенняя распродажа", "");
    assert_eq!(qs::badge_text(&it, &with_event), "Осенняя распродажа");

    let builtin = movie("/b.webm", "B", "", ss::ORIGIN_BUILTIN, 0);
    assert_eq!(qs::badge_text(&builtin, &empty), "Встроенная в Steam");
}

/// `_sv_source_text`: the «Video splash from X» line and its fallback variants.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn source_text_matches_python() {
    let _lang = i18n_pin::ru();
    let empty = qs::MetaMap::new();
    let shop = movie("/a.webm", "A", "Days Gone", ss::ORIGIN_SHOP, 7);
    assert_eq!(qs::source_text(&shop, &empty), "Видеозаставка из Days Gone");

    let meta = meta_with_event(7, "", "Видеозаставка из Days Gone (предмет)");
    assert_eq!(
        qs::source_text(&shop, &meta),
        "Видеозаставка из Days Gone (предмет)"
    );

    let builtin = movie("/b.webm", "B", "", ss::ORIGIN_BUILTIN, 0);
    assert_eq!(qs::source_text(&builtin, &empty), "Встроенная заставка Steam");
    let custom = movie("/c.webm", "C", "", ss::ORIGIN_CUSTOM, 0);
    assert_eq!(qs::source_text(&custom, &empty), "Своё видео с диска");
    let shop_no_game = movie("/d.webm", "D", "", ss::ORIGIN_SHOP, 0);
    assert_eq!(
        qs::source_text(&shop_no_game, &empty),
        "Видеозаставка из Points Shop"
    );
}

/// `_sv_group_keys` + `_sv_rebuild_groups`: order of appearance, «All» first,
/// «★ Favorites» last, the row hides when there is no choice.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn group_chips_match_python() {
    let _lang = i18n_pin::ru();
    let items = vec![
        movie("/a.webm", "A", "Игра 1", ss::ORIGIN_SHOP, 1),
        movie("/b.webm", "B", "Игра 2", ss::ORIGIN_SHOP, 2),
        movie("/c.webm", "C", "Игра 1", ss::ORIGIN_SHOP, 3),
    ];
    let meta = qs::MetaMap::new();
    assert_eq!(qs::group_keys(&items, &meta), vec!["Игра 1", "Игра 2"]);

    let chips = qs::group_chips(&items, &[], &meta, "Игра 2");
    let keys: Vec<&str> = chips.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, vec!["", "Игра 1", "Игра 2"]);
    assert_eq!(chips[0].label, "Все");
    assert!(chips[2].checked);

    let favs = vec!["/b.webm".to_string()];
    let chips = qs::group_chips(&items, &favs, &meta, qs::SV_GROUP_FAV);
    assert_eq!(chips.last().unwrap().label, "⭐ Избранное");
    assert!(chips.last().unwrap().checked);
}

/// The selected group vanished — we silently go back to «All».
#[test]
fn vanished_group_falls_back_to_all() {
    let items = vec![movie("/a.webm", "A", "Игра 1", ss::ORIGIN_SHOP, 1)];
    let meta = qs::MetaMap::new();
    assert_eq!(qs::normalize_group_key("Игра 9", &items, &[], &meta), "");
    assert_eq!(
        qs::normalize_group_key(qs::SV_GROUP_FAV, &items, &[], &meta),
        ""
    );
    let favs = vec!["/a.webm".to_string()];
    assert_eq!(
        qs::normalize_group_key(qs::SV_GROUP_FAV, &items, &favs, &meta),
        qs::SV_GROUP_FAV
    );
}

/// `_sv_matches`: search by title, game, source and event.
#[test]
fn search_covers_all_python_fields() {
    let items = vec![
        movie("/a.webm", "Special Delivery", "Half-Life", ss::ORIGIN_SHOP, 1),
        movie("/b.webm", "Другое", "", ss::ORIGIN_BUILTIN, 0),
    ];
    let meta = meta_with_event(1, "Летняя распродажа", "Видеозаставка из Half-Life");
    let favs: Vec<String> = Vec::new();

    for needle in ["special", "half-life", "летняя", "видеозаставка из half"] {
        let shown = qs::sv_shown(&items, needle, "", &favs, &meta);
        assert_eq!(shown.len(), 1, "поиск «{needle}» должен найти ровно один ролик");
    }
    // An empty query — the whole list.
    assert_eq!(qs::sv_shown(&items, "", "", &favs, &meta).len(), 2);
    // The group filter cuts the same way as in the original.
    let shown = qs::sv_shown(&items, "", "Летняя распродажа", &favs, &meta);
    assert_eq!(shown.len(), 1);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn count_text_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::sv_count_text(0, 0, false), qs::sv_empty_text());
    assert_eq!(qs::sv_count_text(3, 10, true), "Показано 3 из 10");
    assert_eq!(qs::sv_count_text(10, 10, false), "Доступно заставок: 10");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn hero_matches_python() {
    let _lang = i18n_pin::ru();
    let cur = movie("/a.webm", "A", "Игра", ss::ORIGIN_SHOP, 1);
    let other = movie("/b.webm", "B", "", ss::ORIGIN_BUILTIN, 0);
    let meta = meta_with_event(1, "Распродажа", "Видеозаставка из Игра");

    let h = qs::hero(Some(&cur), Some(&cur), &[], &meta);
    assert!(h.present);
    assert_eq!(h.title, "A");
    assert_eq!(h.source, "🎮 Видеозаставка из Игра");
    assert_eq!(h.labels, "Метки: Обычный, Days Gone");
    assert_eq!(h.event, "Событие: Распродажа");
    assert_eq!(h.star_text, "☆ В избранное");
    assert_eq!(h.status, "");

    // Hovering over another card — an honest «preview» label.
    let h = qs::hero(Some(&other), Some(&cur), &[], &meta);
    assert_eq!(h.status, qs::sv_hero_preview_hint());
    assert_eq!(h.labels, "");
    assert_eq!(h.event, "");

    let h = qs::hero(None, None, &[], &meta);
    assert!(!h.present);
    assert_eq!(h.title, "Заставка не выбрана");

    let favs = vec!["/a.webm".to_string()];
    let h = qs::hero(Some(&cur), Some(&cur), &favs, &meta);
    assert_eq!(h.star_text, "⭐ В избранном");
    assert!(h.star_on);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn state_chip_matches_python() {
    let _lang = i18n_pin::ru();
    let cur = movie("/a.webm", "Days Gone", "", ss::ORIGIN_SHOP, 1);

    assert_eq!(
        qs::state_chip(None, true, false, 0, 0),
        ("⚪ Заставка не выбрана".to_string(), qs::CHIP_OFF)
    );
    assert_eq!(
        qs::state_chip(Some(&cur), false, false, 0, 5),
        (
            "⚪ Выключено — при входе ничего не проигрывается".to_string(),
            qs::CHIP_OFF
        )
    );
    assert_eq!(
        qs::state_chip(Some(&cur), true, false, 0, 5),
        (
            "✅ Готово: при следующем входе покажется «Days Gone»".to_string(),
            qs::CHIP_OK
        )
    );
    // Fewer than two favorites — all found ones rotate.
    assert_eq!(
        qs::state_chip(Some(&cur), true, true, 1, 5).0,
        "✅ Готово: при следующем входе — случайный ролик из всех (5 шт.), \
сейчас в очереди «Days Gone»"
    );
    assert_eq!(
        qs::state_chip(Some(&cur), true, true, 3, 5).0,
        "✅ Готово: при следующем входе — случайный ролик из избранного (3 шт.), \
сейчас в очереди «Days Gone»"
    );
}

// ── Grid of profile backgrounds ─────────────────────────────────────────

fn bg(id: &str, name: &str, game: &str, animated: bool, equipped: bool) -> ProfileBackgroundItem {
    ProfileBackgroundItem {
        id: id.into(),
        name: name.into(),
        game: game.into(),
        url: format!("https://cdn/{id}.jpg"),
        preview: String::new(),
        animated,
        equipped,
    }
}

#[test]
fn bg_search_matches_python() {
    let items = vec![
        bg("1", "Ночь", "Portal 2", false, false),
        bg("2", "День", "Half-Life", false, false),
    ];
    assert_eq!(qs::bg_filtered(&items, "portal").len(), 1);
    assert_eq!(qs::bg_filtered(&items, "ночь").len(), 1);
    assert_eq!(qs::bg_filtered(&items, "").len(), 2);
    assert_eq!(qs::bg_filtered(&items, "нет такого").len(), 0);
}

/// An animated background with a clip address — as Steam returns it.
fn bg_movie(id: &str, name: &str, game: &str) -> ProfileBackgroundItem {
    ProfileBackgroundItem {
        url: format!("https://cdn/{id}.webm"),
        preview: format!("https://cdn/{id}.jpg"),
        ..bg(id, name, game, true, false)
    }
}

/// The "animated" flag is one for the card, the mark, the filter and
/// «Apply»: a clip from Steam or an address that will be applied as a video wallpaper.
#[test]
fn bg_animated_is_one_decision() {
    let still = bg("1", "Ночь", "Portal 2", false, false);
    let movie = bg_movie("2", "Дождь", "Portal 2");
    // The public inventory does not set the flag, but the address leads to a clip.
    let by_url = ProfileBackgroundItem {
        url: "https://cdn/3.webm?t=1".into(),
        ..bg("3", "Снег", "Half-Life", false, false)
    };
    assert!(!qs::bg_is_animated(&still));
    assert!(qs::bg_is_animated(&movie));
    assert!(qs::bg_is_animated(&by_url));
    // «Apply» will decide the same from the downloaded file.
    for it in [&still, &movie, &by_url] {
        let file = qs::bg_cache_file_name(&it.id, &it.url);
        assert_eq!(qs::bg_is_animated(it), qs::apply_format(&file) != "static", "{file}");
    }
    // The card carries the flag and the mark; a static one has no mark.
    assert!(qs::bg_card(&movie).animated);
    assert_eq!(qs::bg_card(&movie).mark, qs::BG_ANIMATED_MARK);
    assert!(qs::bg_card(&by_url).animated);
    assert_eq!(qs::bg_card(&still).mark, "");
    assert!(!qs::bg_card(&still).animated);
}

/// The kind filter: only the needed cards remain, the counters are correct, search and
/// filter work together.
#[test]
fn bg_kind_filter_and_counters() {
    let items = vec![
        bg("1", "Ночь", "Portal 2", false, false),
        bg_movie("2", "Дождь", "Portal 2"),
        bg("3", "День", "Half-Life", false, false),
        bg_movie("4", "Снег", "Half-Life"),
        bg("5", "Закат", "Half-Life", false, false),
    ];
    let ids = |needle: &str, kind: &str| -> Vec<String> {
        qs::bg_filtered_by(&items, needle, kind).iter().map(|it| it.id.clone()).collect()
    };
    assert_eq!(ids("", qs::BG_KIND_ALL), ["1", "2", "3", "4", "5"]);
    assert_eq!(ids("", qs::BG_KIND_STATIC), ["1", "3", "5"]);
    assert_eq!(ids("", qs::BG_KIND_ANIMATED), ["2", "4"]);
    // Search and filter together.
    assert_eq!(ids("half-life", qs::BG_KIND_ANIMATED), ["4"]);
    assert_eq!(ids("half-life", qs::BG_KIND_STATIC), ["3", "5"]);
    assert_eq!(ids("portal", qs::BG_KIND_ALL), ["1", "2"]);
    assert!(ids("нет такого", qs::BG_KIND_ANIMATED).is_empty());
    // An unknown key — «all», not an empty grid.
    assert_eq!(ids("", "что-то новое").len(), 5);
    assert_eq!(qs::normalize_bg_kind("что-то новое"), qs::BG_KIND_ALL);

    // Counters on the buttons: without search — over the whole inventory…
    let chips = qs::bg_kind_chips(&items, "", qs::BG_KIND_ANIMATED);
    let keys: Vec<&str> = chips.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, [qs::BG_KIND_ALL, qs::BG_KIND_STATIC, qs::BG_KIND_ANIMATED]);
    assert!(chips[0].label.ends_with("(5)"), "{}", chips[0].label);
    assert!(chips[1].label.ends_with("(3)"), "{}", chips[1].label);
    assert!(chips[2].label.ends_with("(2)"), "{}", chips[2].label);
    assert_eq!(chips.iter().map(|c| c.checked).collect::<Vec<_>>(), [false, false, true]);
    // …and with search — over what was found: the number on the button equals the number of cards
    // the person will see after pressing it.
    let chips = qs::bg_kind_chips(&items, "half-life", qs::BG_KIND_ALL);
    assert!(chips[0].label.ends_with("(3)"), "{}", chips[0].label);
    assert!(chips[1].label.ends_with("(2)"), "{}", chips[1].label);
    assert!(chips[2].label.ends_with("(1)"), "{}", chips[2].label);
    for chip in &chips {
        let shown = qs::bg_filtered_by(&items, "half-life", &chip.key).len();
        assert!(chip.label.ends_with(&format!("({shown})")), "{}", chip.label);
    }
    // No backgrounds at all — no switch.
    assert!(qs::bg_kind_chips(&[], "", "").is_empty());
    // No animated ones — the button stays and honestly shows zero.
    let only_still = vec![bg("1", "Ночь", "Portal 2", false, false)];
    assert!(qs::bg_kind_chips(&only_still, "", "")[2].label.ends_with("(0)"));
}

/// The section label takes the kind filter into account the same way as search.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn bg_section_text_counts_the_kind_filter() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::bg_section_text_filtered("", 205, 205, false), "Найдено фонов: 205");
    assert_eq!(qs::bg_section_text_filtered("", 205, 13, true), "Показано 13 из 205");
    assert_eq!(qs::bg_section_text_filtered("", 205, 0, true), "Показано 0 из 205");
    assert_eq!(qs::bg_section_text_filtered("", 0, 0, true), "Найдено фонов: 0");
    assert_eq!(qs::bg_section_text_filtered("Steam не найден.", 0, 0, true), "Steam не найден.");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn bg_card_matches_python() {
    let _lang = i18n_pin::ru();
    let equipped = bg("1", "Ночь", "Portal 2", true, true);
    let c = qs::bg_card(&equipped);
    assert_eq!(c.icon, "📼");
    assert_eq!(c.sub, "🎮 надет в профиле");
    assert_eq!(c.thumb_tooltip, qs::bg_animated_tooltip());
    // preview is empty — we fetch the background itself, like `it.get("preview") or it.get("url")`.
    assert_eq!(c.preview_url, "https://cdn/1.jpg");

    let plain = bg("2", "День", "Half-Life", false, false);
    let c = qs::bg_card(&plain);
    assert_eq!(c.icon, "🖼️");
    assert_eq!(c.sub, "Half-Life");
    assert_eq!(c.thumb_tooltip, "");

    let bare = bg("3", "", "", false, false);
    assert_eq!(qs::bg_card(&bare).name, "?");
    assert_eq!(qs::bg_card(&bare).sub, "");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn bg_status_texts_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::bg_status_text(0, 0, ""), "Найдено фонов: 0");
    assert_eq!(qs::bg_status_text(202, 12, "порт"), "Показано 12 из 202");
    assert_eq!(qs::bg_status_text(202, 202, ""), "Найдено фонов: 202");

    // The reason beats the count, and it is written INTO THE SAME label — as in
    // the original (`_start_inventory_load` → `_bg_status`). Previously both
    // reasons went off as a pop-up message over the neighbouring
    // «Video splashes» section and disappeared together with it.
    assert_eq!(qs::bg_section_text(&qs::bg_no_steam(), 0, 0, ""), "Steam не найден.");
    assert_eq!(
        qs::bg_section_text(&qs::bg_no_account(), 0, 0, ""),
        "Не удалось определить аккаунт Steam (нет userdata)."
    );
    // The same places: while the inventory is loading, the section label is «loading», not
    // a count of «0», and not a toast over the neighbouring section.
    assert_eq!(
        qs::bg_section_text(&qs::bg_loading_text(), 0, 0, ""),
        "⏳ Загружаю инвентарь Steam…"
    );
    assert_eq!(
        qs::bg_section_text(&qs::bg_refreshing_text(202), 202, 202, ""),
        "🔄 Обновляю… (из кэша: 202)"
    );
    assert_eq!(
        qs::bg_section_text(&qs::bg_nothing_found(), 0, 0, ""),
        qs::bg_nothing_found()
    );
    assert!(qs::bg_section_text(&qs::bg_error_text("403"), 0, 0, "")
        .starts_with("⚠️ Не удалось загрузить инвентарь"));
    // No reason — the label is as before, with the count.
    assert_eq!(qs::bg_section_text("", 0, 0, ""), "Найдено фонов: 0");
    assert_eq!(qs::bg_section_text("", 202, 12, "порт"), "Показано 12 из 202");
    assert_eq!(qs::bg_section_text("", 202, 202, ""), "Найдено фонов: 202");
    assert_eq!(qs::bg_refreshing_text(202), "🔄 Обновляю… (из кэша: 202)");
    assert_eq!(qs::bg_applying_text("Ночь"), "⏳ Скачиваю и применяю: Ночь…");
    assert_eq!(qs::bg_applied_text("Ночь"), "✅ Применено как обои: Ночь");
    assert_eq!(qs::bg_apply_error_text(""), "❌ Не удалось применить");
    assert_eq!(qs::bg_apply_error_text("нет сети"), "❌ нет сети");
}

/// The original's `error[:160]` — by CHARACTERS, not bytes: otherwise a Russian error
/// would be cut in the middle of a letter.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn bg_error_text_clips_by_chars() {
    let _lang = i18n_pin::ru();
    let long = "я".repeat(300);
    let out = qs::bg_error_text(&long);
    assert!(out.ends_with(&"я".repeat(160)));
    assert!(!out.ends_with(&"я".repeat(161)));
}

/// Pagination — the same engine as the product's other grids.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pager_agrees_with_grid_engine() {
    let _lang = i18n_pin::ru();
    for cols in 2..=8u32 {
        for shown in [0usize, 1, 24, 25, 202] {
            let per_page = ui_base::grid_page_size(cols, qs::BG_ROWS);
            let p = qs::bg_pager(shown, 0, cols);
            assert_eq!(p.pages, ui_base::grid_page_count(shown as u32, per_page));
            assert_eq!(p.visible, shown > per_page as usize);
            assert_eq!(p.text, format!("Страница 1 / {}", p.pages));
        }
    }
}

/// Columns of the background grid: a 1920 window (a grid of ~1828 points) — nine, as
/// the grid drew them; a page is nine full rows of five.
#[test]
fn bg_columns_fill_the_grid_width() {
    // 9 × 180 + 8 × 10 = 1700 ≤ 1828 < 10 × 180 + 9 × 10 = 1890.
    assert_eq!(qs::bg_columns(1828), 9);
    assert_eq!(qs::bg_columns(1890), 10);
    assert_eq!(qs::bg_columns(1889), 9);
    assert_eq!(qs::bg_columns(0), qs::BG_MIN_COLS);
    assert_eq!(qs::bg_columns(100_000), qs::BG_MAX_COLS);
    let p = qs::bg_pager(202, 0, qs::bg_columns(1828));
    assert_eq!(p.end - p.start, 45);
}

/// Changing the number of columns must not lose the place: the first element of the previous
/// page stays on the screen.
#[test]
fn page_after_resize_keeps_first_item() {
    // 5 columns, page 3 → the first element is 75; with 3 columns (15 per
    // page) it lies on page 5.
    assert_eq!(qs::bg_page_after_resize(3, 5, 3), 5);
    assert_eq!(qs::bg_page_after_resize(0, 5, 8), 0);
}

// ── Inventory ───────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn inventory_urls_match_python() {
    // Russian interface — Steam's Russian answer, as in the original.
    {
        let _lang = i18n_pin::ru();
        assert_eq!(
            qs::inventory_page_url("765", ""),
            "https://steamcommunity.com/inventory/765/753/6?l=russian&count=2000"
        );
        assert_eq!(
            qs::inventory_page_url("765", "123"),
            "https://steamcommunity.com/inventory/765/753/6?l=russian&count=2000&start_assetid=123"
        );
    }
    // English interface — English titles and inventory tags.
    {
        let _lang = i18n_pin::en();
        assert_eq!(
            qs::inventory_page_url("765", ""),
            "https://steamcommunity.com/inventory/765/753/6?l=english&count=2000"
        );
    }
    assert_eq!(qs::INVENTORY_MAX_PAGES, 20);
    assert_eq!(qs::INVENTORY_TIMEOUT_S, 25);
    assert_eq!(qs::USER_AGENT, "Rexon/5.0");
}

/// The original's fix No. 4 (2026-07-28) must live here too: pages keep coming
/// while Steam says `more_items`, not "the first 2000 items".
#[test]
fn inventory_paging_follows_more_items() {
    let page = json!({"more_items": 1, "last_assetid": "42"});
    assert_eq!(qs::inventory_next_start(&page).as_deref(), Some("42"));
    assert_eq!(qs::inventory_next_start(&json!({"more_items": 1})), None);
    assert_eq!(
        qs::inventory_next_start(&json!({"more_items": 1, "last_assetid": ""})),
        None
    );
    assert_eq!(qs::inventory_next_start(&json!({"assets": []})), None);

    let merged = qs::merge_inventory_pages(&[
        json!({"assets": [1], "descriptions": ["a"]}),
        json!({"assets": [2, 3], "descriptions": ["b"]}),
    ]);
    assert_eq!(merged["assets"], json!([1, 2, 3]));
    assert_eq!(merged["descriptions"], json!(["a", "b"]));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn item_labels_match_python() {
    let _lang = i18n_pin::ru();
    let desc = json!({
        "tags": [
            {"category": "droprate", "localized_tag_name": "Обычный"},
            {"category": "Game", "localized_tag_name": "Days Gone"},
            {"category": "item_class", "localized_tag_name": "Видеозаставка"},
        ],
        "tradable": 0,
        "marketable": 0,
    });
    assert_eq!(
        qs::item_labels(&desc),
        vec![
            "Обычный",
            "Days Gone",
            "Видеозаставка",
            "Нельзя обменять",
            "Нельзя продать"
        ]
    );
    // Repeats are collapsed, the order of first appearance is kept.
    let dup = json!({
        "tags": [
            {"category": "droprate", "localized_tag_name": "X"},
            {"category": "game", "localized_tag_name": "X"},
        ],
        "tradable": 1,
        "marketable": 1,
    });
    assert_eq!(qs::item_labels(&dup), vec!["X"]);
}

#[test]
fn splash_item_meta_matches_python() {
    let descs = vec![
        json!({
            "tags": [
                {"internal_name": "item_class_17", "category": "item_class",
                 "localized_tag_name": "Видеозаставка"},
                {"internal_name": "s1", "category": "event",
                 "localized_tag_name": "Летняя распродажа"},
            ],
            "market_fee_app": 1259420,
            "name": "Days Gone",
            "type": "Видеозаставка из Days Gone",
            "tradable": 1,
            "marketable": 1,
        }),
        // Not a splash — skipped.
        json!({"tags": [{"internal_name": "item_class_3"}], "market_fee_app": 7}),
        // A splash without market_fee_app — skipped.
        json!({"tags": [{"internal_name": "item_class_17"}], "market_fee_app": 0}),
    ];
    let meta = qs::splash_item_meta(&descs);
    assert_eq!(meta.len(), 1);
    let m = &meta[&1259420];
    assert_eq!(m.name, "Days Gone");
    assert_eq!(m.source, "Видеозаставка из Days Gone");
    assert_eq!(m.event, "Летняя распродажа");
    assert_eq!(m.labels, vec!["Видеозаставка"]);

    // The tag cache survives the "written → read" round.
    let json_text = qs::meta_cache_json(&meta);
    let back = qs::parse_meta_cache(&serde_json::from_str::<Value>(&json_text).unwrap());
    assert_eq!(back, meta);
}

#[test]
fn steamid64_matches_python() {
    assert_eq!(qs::STEAMID64_BASE, 76561197960265728);
    assert_eq!(
        qs::steamid64_from_accountid("123").as_deref(),
        Some("76561197960265851")
    );
    assert_eq!(qs::steamid64_from_accountid("0"), None);
    assert_eq!(qs::steamid64_from_accountid("абв"), None);
}

/// The original's regex must not catch similar keys and must
/// take the value up to the quote before the newline.
#[test]
fn equipped_extraction_is_precise() {
    let vdf = concat!(
        "\t\"NotGetEquippedProfileItemsForUser1\"\t\"мимо\"\n",
        "\t\"GetEquippedProfileItemsForUser76561198000000000\"\t",
        "\"{\\\"response\\\":{\\\"profile_background\\\":{\\\"image_large\\\":",
        "\\\"images/a.jpg\\\",\\\"movie_webm\\\":\\\"images/a.webm\\\"}}}\"\n",
        "\t\"Хвост\"\t\"1\"\n"
    );
    let raw = qs::extract_equipped_json(vdf).expect("значение обязано найтись");
    let parsed: Value = serde_json::from_str(&raw).expect("после un-escape — валидный JSON");
    let bgv = &parsed["response"]["profile_background"];
    assert_eq!(bgv["image_large"], json!("images/a.jpg"));
    assert_eq!(bgv["movie_webm"], json!("images/a.webm"));

    // No key at all.
    assert_eq!(qs::extract_equipped_json("\"Другое\"\t\"1\"\n"), None);
    // A key without digits is not that key.
    assert_eq!(
        qs::extract_equipped_json("\"GetEquippedProfileItemsForUser\"\t\"{}\"\n"),
        None
    );
}

// ── Caches and configs ──────────────────────────────────────────────────

/// Deviation No. 1: XDG is respected. The default matches the original.
#[test]
fn cache_paths_respect_xdg() {
    assert_eq!(
        qs::inventory_cache_path(None, "/home/u", "765"),
        "/home/u/.cache/rexon/steam_profile_inventory_765.json"
    );
    assert_eq!(
        qs::inventory_cache_path(Some("/run/cache"), "/home/u", "765"),
        "/run/cache/rexon/steam_profile_inventory_765.json"
    );
    assert_eq!(
        qs::splash_meta_cache_path(None, "/home/u", "765"),
        "/home/u/.cache/rexon/steam_splash_items_765.json"
    );
    assert_eq!(
        qs::bg_cache_dir(None, "/home/u"),
        "/home/u/.cache/rexon/steam_profile_bg"
    );
    assert_eq!(
        qs::thumb_cache_dir(None, "/home/u"),
        "/home/u/.cache/rexon/steam_profile_thumb"
    );
    assert_eq!(
        qs::applied_config_path(None, "/home/u"),
        "/home/u/.config/rexon/steam_profile_bg.json"
    );
    assert_eq!(
        qs::applied_config_path(Some("/run/cfg"), "/home/u"),
        "/run/cfg/rexon/steam_profile_bg.json"
    );
}

/// The cache file name: an id with "/" is defused, the extension is taken from the address.
#[test]
fn cache_file_names_match_python() {
    assert_eq!(qs::bg_cache_file_name("123", "https://h/a/b.png"), "123.png");
    assert_eq!(qs::bg_cache_file_name("123", "https://h/a/b"), "123.jpg");
    assert_eq!(
        qs::thumb_cache_file_name("Ночь/тьма", "https://h/a/b.JPG"),
        "Ночь_тьма.jpg"
    );
    assert_eq!(qs::part_path("/c/1.jpg"), "/c/1.jpg.part");
}

#[test]
fn inventory_cache_roundtrip() {
    let items = vec![
        bg("1", "Ночь", "Portal 2", false, true),
        bg("2", "День", "", true, false),
    ];
    let text = qs::inventory_cache_json("765", &items, 1700000000);
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["sid"], json!("765"));
    assert_eq!(data["ts"], json!(1700000000));

    assert_eq!(qs::parse_inventory_cache(&data, "765"), Some(items));
    // Another account's cache is not taken.
    assert_eq!(qs::parse_inventory_cache(&data, "999"), None);
    // An empty list — as if there were no cache.
    let empty = serde_json::from_str::<Value>(
        &qs::inventory_cache_json("765", &[], 0),
    )
    .unwrap();
    assert_eq!(qs::parse_inventory_cache(&empty, "765"), None);
}

#[test]
fn applied_config_roundtrip() {
    let text = qs::applied_config_json("1", "Ночь");
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(qs::parse_applied_config(&data), "1");
    assert_eq!(data["name"], json!("Ночь"));
    assert_eq!(qs::parse_applied_config(&json!({})), "");
}

// ── Styles ──────────────────────────────────────────────────────────────

/// The original's QSS must match line for line.
#[test]
fn qss_matches_python() {
    assert_eq!(
        qs::group_qss("steamBgGroup", true),
        "QFrame#steamBgGroup { background-color: rgba(255,255,255,0.04); \
border: 1px solid rgba(255,255,255,0.12); border-radius: 6px; } \
QFrame#steamBgGroup QLabel { border: none; background: transparent; }"
    );
    assert_eq!(
        qs::primary_btn_qss(),
        "QPushButton { background-color: rgba(59,130,246,0.85); color: white; \
border: none; border-radius: 4px; padding: 6px 14px; font-weight: 600; } \
QPushButton:hover { background-color: rgba(59,130,246,1.0); } \
QPushButton:disabled { background-color: rgba(120,120,120,0.35); }"
    );
    assert_eq!(
        qs::applied_btn_qss(),
        "QPushButton { background-color: rgba(34,197,94,0.90); color: white; \
border: none; border-radius: 4px; padding: 6px 14px; font-weight: 600; } \
QPushButton:hover { background-color: rgba(34,197,94,1.0); }"
    );
    assert_eq!(
        qs::card_qss("steamBgCard", true, false),
        "QFrame#steamBgCard { background-color: rgba(59,130,246,0.18); \
border: 1px solid rgba(59,130,246,0.85); border-radius: 6px; } \
QFrame#steamBgCard:hover { border-color: rgba(59,130,246,1.0); } \
QFrame#steamBgCard QLabel { border: none; background: transparent; }"
    );
    assert_eq!(
        qs::badge_qss(qs::BADGE_ACCENT, true),
        "QLabel { background-color: rgba(59,130,246,0.85); color: white; \
border-radius: 7px; padding: 1px 7px; font-size: 11px; }"
    );
    assert_eq!(
        qs::chip_qss(qs::CHIP_OK, true),
        "QLabel { background-color: rgba(34,197,94,0.16); \
border: 1px solid rgba(34,197,94,0.65); color: rgba(34,197,94,1.0); \
border-radius: 12px; padding: 5px 12px; font-weight: 600; }"
    );
}

/// A sign that "the decision leaked into the layout" is a colour typed into the .qml by hand.
/// So every QSS colour has a sister returning the bare colour, and they
/// must show ONE AND THE SAME colour (the same guard as for «Comparison»).
#[test]
fn widget_qss_and_qml_colors_agree() {
    for (rgb, name) in [
        (qs::ACCENT_RGB, "акцент"),
        (qs::APPLIED_RGB, "применено"),
        (qs::GOLD_RGB, "избранное"),
    ] {
        let h = qs::hex(rgb);
        let expected = format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2);
        assert_eq!(h, expected, "цвет «{name}» разошёлся");
        // The same triple must be in the QSS too.
        let qss = qs::primary_btn_qss();
        if rgb == qs::ACCENT_RGB {
            assert!(qss.contains(&format!("rgba({},{},{},", rgb.0, rgb.1, rgb.2)));
        }
    }
    // The star: on — golden, off — the core's neutral grey.
    assert_eq!(qs::star_color(true), "rgba(234,179,8,1.0)");
    assert_eq!(qs::star_color(false), ui_base::neutral_rgba(0.45));
    // The «off» chip takes the core's secondary text, not its own grey.
    let (_, _, fg) = qs::chip_colors(qs::CHIP_OFF, true);
    assert_eq!(fg, ui_base::secondary_text_color(true));
    // The neutral badge takes the core's main text.
    let (_, fg) = qs::badge_colors(qs::BADGE_NEUTRAL, false);
    assert_eq!(fg, ui_base::primary_text_color(false));
}

/// The «choose a splash first» refusal is ONE string for the whole product.
///
/// There were three copies (measured on 07.09.2026): here, in `session_splash` and in the
/// Python host of the QML tab. The storefront and autostart refuse for one and the same
/// reason, and these words must not diverge.
#[test]
fn choose_first_refusal_has_a_single_source() {
    assert_eq!(qs::SV_TEST_NO_ITEM, ss::MSG_CHOOSE_FIRST);
}

/// A custom clip from the config is appended to the storefront exactly when it is
/// selected, lies on disk and did not get into the Steam scan.
#[test]
fn a_custom_movie_is_added_only_when_the_scan_missed_it() {
    let scan = vec![
        "/steam/movies/a.webm".to_string(),
        "/steam/movies/b.webm".to_string(),
    ];
    // The ordinary case: the file was chosen by a person from outside.
    assert!(qs::needs_custom_movie("/home/w/своё.mp4", &scan, true));
    // The clip is already in the storefront — there must be no second card.
    assert!(!qs::needs_custom_movie("/steam/movies/a.webm", &scan, true));
    // No clip selected at all.
    assert!(!qs::needs_custom_movie("", &scan, true));
    // The config outlived the file's removal — we do not draw a card into nowhere.
    assert!(!qs::needs_custom_movie("/home/w/снесённое.mp4", &scan, false));
    // An empty scan — the storefront consists of one custom clip.
    assert!(qs::needs_custom_movie("/home/w/своё.mp4", &[], true));
}

/// The page must lie in a scroll area, and the banner outside it.
///
/// Without the outer scroll bar the tab lost everything that did not fit in the window: a long path
/// to Steam in the header does not shrink and makes the page wider than the window, and at
/// the main window's own minimum it lacks height too — the right edge of
/// the section with the «Custom file…» button and the bottom line were simply cut off. The
/// original has the scroll bar (`rexon/qt_steam/window.py:797`), and it is not
/// decoration but the only way to reach them.
///
/// ⛔ The status banner must not get inside the scroll area: it is an overlay ON TOP of
/// the page and, having moved inside, would scroll together with the content.
#[test]
fn steam_page_scrolls_and_toast_stays_outside() {
    const QML: &str = include_str!("../../rexon-gui/qml/SteamPage.qml");

    assert!(
        QML.contains("\n    ScrollView {\n        id: pageScroll"),
        "страница «Steam» вынута из области прокрутки: длинный путь к Steam \
снова уведёт правый край секции за край окна"
    );
    // An indent of four spaces = the banner lies right in the page root. Inside
    // the scroll area it would have a deeper indent.
    assert!(
        QML.contains("\n    RexonToast {"),
        "плашка состояния уехала внутрь области прокрутки — она накладка \
поверх страницы, а не её содержимое"
    );
}

// ============================================================================
// Translation: Russian as before, English without a single Russian letter
// ============================================================================

/// A snapshot of the tab's Russian texts before moving the strings into the catalog.
fn ru_before() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/qt_steam_ru_before.json");
    let text = std::fs::read_to_string(path).expect("снимок qt_steam_ru_before.json читается");
    serde_json::from_str(&text).expect("снимок не разбирается как JSON")
}

/// The tab's constant labels by their former names (snapshot keys).
fn constant_texts() -> Vec<(&'static str, String)> {
    vec![
        ("SPLASH_GROUP_TITLE", qs::splash_group_title()),
        ("SPLASH_PITCH", qs::splash_pitch()),
        ("BG_GROUP_TITLE", qs::bg_group_title()),
        ("BG_GROUP_DESC", qs::bg_group_desc()),
        ("SV_SEARCH_PLACEHOLDER", qs::sv_search_placeholder()),
        ("SV_PICK_TEXT", qs::sv_pick_text()),
        ("SV_PICK_TOOLTIP", qs::sv_pick_tooltip()),
        ("SV_PICK_DIALOG_TITLE", qs::sv_pick_dialog_title()),
        ("SV_PICK_FILTER", qs::sv_pick_filter()),
        ("SV_AUTOSTART_TEXT", qs::sv_autostart_text()),
        ("SV_AUTOSTART_TOOLTIP", qs::sv_autostart_tooltip()),
        ("SV_ROTATE_TEXT", qs::sv_rotate_text()),
        ("SV_ROTATE_TOOLTIP", qs::sv_rotate_tooltip()),
        ("SV_SOUND_TEXT", qs::sv_sound_text()),
        ("SV_TEST_TEXT", qs::sv_test_text()),
        ("SV_TEST_TOOLTIP", qs::sv_test_tooltip()),
        ("SV_TEST_PLAYING", qs::sv_test_playing()),
        ("SV_NO_REASON", qs::sv_no_reason()),
        ("SV_AUTOSTART_ON_FAILED", qs::sv_autostart_on_failed()),
        ("SV_AUTOSTART_OFF_FAILED", qs::sv_autostart_off_failed()),
        ("SV_AUTOSTART_TRY_AGAIN", qs::sv_autostart_try_again()),
        ("SV_CAUSE_NO_ACCESS", qs::sv_cause_no_access()),
        ("SV_CAUSE_NO_SPACE", qs::sv_cause_no_space()),
        ("SV_CAUSE_READ_ONLY", qs::sv_cause_read_only()),
        ("SV_STAR_ON", qs::sv_star_on()),
        ("SV_STAR_OFF", qs::sv_star_off()),
        ("SV_STAR_TOOLTIP", qs::sv_star_tooltip()),
        ("SV_CARD_STAR_TOOLTIP", qs::sv_card_star_tooltip()),
        ("SV_ALL_CHIP", qs::sv_all_chip()),
        ("SV_FAV_CHIP", qs::sv_fav_chip()),
        ("SV_EMPTY_TEXT", qs::sv_empty_text()),
        ("SV_HERO_EMPTY_TITLE", qs::sv_hero_empty_title()),
        ("SV_HERO_PREVIEW_HINT", qs::sv_hero_preview_hint()),
        ("SV_ROTATE_NEED_TWO", qs::sv_rotate_need_two()),
        ("BG_SEARCH_PLACEHOLDER", qs::bg_search_placeholder()),
        ("BG_AUDIO_TEXT", qs::bg_audio_text()),
        ("BG_LOADING_TEXT", qs::bg_loading_text()),
        ("BG_APPLY_TEXT", qs::bg_apply_text()),
        ("BG_APPLIED_TEXT", qs::bg_applied_label()),
        ("BG_PREV_TEXT", qs::bg_prev_text()),
        ("BG_NEXT_TEXT", qs::bg_next_text()),
        ("BG_EQUIPPED_SUB", qs::bg_equipped_sub()),
        ("BG_ANIMATED_TOOLTIP", qs::bg_animated_tooltip()),
        ("BG_NO_STEAM", qs::bg_no_steam()),
        ("BG_NO_ACCOUNT", qs::bg_no_account()),
        ("BG_NOTHING_FOUND", qs::bg_nothing_found()),
        ("BG_BUSY", qs::bg_busy()),
        ("BG_NO_ENGINE", qs::bg_no_engine()),
        ("BG_APPLY_FAILED", qs::bg_apply_failed()),
        ("WINDOW_TITLE", qs::WINDOW_TITLE.to_string()),
        ("HEADER_TITLE", qs::HEADER_TITLE.to_string()),
    ]
}

/// The tab's assembled phrases on the same inputs as in the snapshot.
fn phrase_texts() -> Vec<(&'static str, Value)> {
    let it = json!({"path":"/a","title":"T","game":"G","origin":"shop","appid":1});
    let b = json!({"path":"/b","title":"T","origin":"builtin","appid":0});
    let cu = json!({"path":"/c","title":"T","origin":"custom","appid":0});
    let sh = json!({"path":"/d","title":"T","origin":"shop","appid":0});
    let m = qs::MetaMap::new();
    let mut mm = qs::MetaMap::new();
    mm.insert(
        1,
        qs::SplashMeta {
            name: "n".into(),
            source: "S".into(),
            event: "E".into(),
            labels: vec!["A".into(), "B".into()],
        },
    );
    let h = qs::hero(Some(&it), None, &[], &mm);
    let v = |x: String| Value::from(x);
    vec![
        ("header_found", v(qs::header_found_text(Some("/p")).0)),
        ("header_missing", v(qs::header_found_text(None).0)),
        ("refusal_on_access", json!(qs::autostart_refusal_text(true, "x (os error 13)"))),
        ("refusal_off_other", json!(qs::autostart_refusal_text(false, "x"))),
        ("source_game", v(qs::source_text(&it, &m))),
        ("source_builtin", v(qs::source_text(&b, &m))),
        ("source_custom", v(qs::source_text(&cu, &m))),
        ("source_shop", v(qs::source_text(&sh, &m))),
        ("count_filtered", v(qs::sv_count_text(3, 10, true))),
        ("count_all", v(qs::sv_count_text(10, 10, false))),
        ("hero_labels", v(h.labels)),
        ("hero_event", v(h.event)),
        ("chip_none", v(qs::state_chip(None, true, false, 0, 0).0)),
        ("chip_off", v(qs::state_chip(Some(&it), false, false, 0, 0).0)),
        ("chip_one", v(qs::state_chip(Some(&it), true, false, 0, 0).0)),
        ("chip_all", v(qs::state_chip(Some(&it), true, true, 0, 5).0)),
        ("chip_favs", v(qs::state_chip(Some(&it), true, true, 3, 5).0)),
        ("bg_error", v(qs::bg_error_text("X"))),
        ("bg_status_zero", v(qs::bg_status_text(0, 0, ""))),
        ("bg_status_filtered", v(qs::bg_status_text(202, 12, "q"))),
        ("bg_status_all", v(qs::bg_status_text(202, 202, ""))),
        ("bg_refreshing", v(qs::bg_refreshing_text(5))),
        ("bg_applying", v(qs::bg_applying_text("N"))),
        ("bg_applied", v(qs::bg_applied_text("N"))),
        ("bg_apply_error", v(qs::bg_apply_error_text(""))),
        ("pager", v(qs::bg_pager(100, 1, 5).text)),
        ("labels", json!(qs::item_labels(&json!({"tags": []})))),
    ]
}

fn other_texts() -> Vec<(&'static str, String)> {
    vec![
        ("DEFAULT_NAME", rexon_core::steam_profile::default_name()),
        ("DEFAULT_EQUIPPED_NAME", rexon_core::steam_profile_items::default_equipped_name()),
        ("NO_LOGIN_REASON", rexon_core::steam_token::no_login_reason()),
        ("NO_TOKEN_REASON", rexon_core::steam_token::no_token_reason()),
        ("gui_no_url", i18n::tr("no download address")),
    ]
}

/// ⭐ For the Russian interface NOTHING changed: every label, every
/// assembled phrase of the «Steam» tab and the neighbouring modules is word for
/// word the same as stood in the code before the move (the snapshot was taken from HEAD on 28.09).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    let before = ru_before();

    let consts = before["constants"].as_object().unwrap();
    let got = constant_texts();
    // Segments are a separate snapshot line, the rest — by name.
    assert_eq!(consts.len(), got.len() + qs::SEGMENTS.len(), "состав подписей разошёлся со снимком");
    for (key, text) in &got {
        assert_eq!(Some(text.as_str()), consts[*key].as_str(), "подпись {key}");
    }
    for (key, label) in qs::segments() {
        assert_eq!(Some(label.as_str()), consts[&format!("SEGMENTS.{key}")].as_str(), "сегмент {key}");
    }

    let phrases = before["phrases"].as_object().unwrap();
    let got = phrase_texts();
    assert_eq!(phrases.len(), got.len());
    for (key, text) in got {
        assert_eq!(text, phrases[key], "фраза {key}");
    }

    let other = before["other"].as_object().unwrap();
    for (key, text) in other_texts() {
        assert_eq!(Some(text.as_str()), other[key].as_str(), "{key}");
    }
}

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// The tab's English interface: not a single Russian letter in labels and
/// assembled phrases — a forgotten string shows up here, not to the buyer.
///
/// The exceptions are named: the «choose a splash first» answer and the clip's origin
/// label live in `session_splash` (not in this pass), so
/// the inputs here do not touch them.
#[test]
fn english_tab_has_no_russian() {
    let _lang = i18n_pin::en();
    let mut all: Vec<String> = Vec::new();
    all.extend(constant_texts().into_iter().map(|(_, t)| t));
    all.extend(qs::segments().into_iter().map(|(_, t)| t));
    for (_, v) in phrase_texts() {
        match v {
            Value::String(s) => all.push(s),
            Value::Array(a) => all.extend(a.into_iter().filter_map(|x| x.as_str().map(String::from))),
            Value::Null => {}
            other => panic!("неожиданная фраза: {other}"),
        }
    }
    all.extend(other_texts().into_iter().map(|(_, t)| t));
    let card = qs::bg_card(&bg("1", "Night", "Portal 2", true, true));
    all.extend([card.sub, card.thumb_tooltip]);
    all.push(qs::bg_animated_badge());
    all.extend(qs::bg_kind_chips(&[bg("1", "Night", "Portal 2", true, true)], "", "").into_iter().map(|c| c.label));
    let it = movie("/a.webm", "A", "Days Gone", ss::ORIGIN_SHOP, 1);
    let h = qs::hero(Some(&it), None, &[], &qs::MetaMap::new());
    all.extend([h.star_text, h.status, h.source]);
    all.push(qs::hero(None, None, &[], &qs::MetaMap::new()).title);
    for chip in qs::group_chips(&[it.clone()], &["/a.webm".to_string()], &qs::MetaMap::new(), "") {
        all.push(chip.label);
    }
    all.extend(qs::rotation_refusal(true, 0, 1));
    assert!(all.len() > 80, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// English phrases assembled from parts read as phrases.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    let it = movie("/a.webm", "Days Gone", "Days Gone", ss::ORIGIN_SHOP, 1);
    assert_eq!(
        qs::state_chip(Some(&it), true, true, 3, 5).0,
        "✅ Ready: the next login plays a random favorite (3 in the pool), “Days Gone” is up next"
    );
    assert_eq!(qs::source_text(&it, &qs::MetaMap::new()), "Startup movie from Days Gone");
    let builtin = movie("/b.webm", "B", "", ss::ORIGIN_BUILTIN, 0);
    assert_eq!(qs::badge_text(&builtin, &qs::MetaMap::new()), "Built into Steam");
    assert_eq!(qs::sv_count_text(3, 10, true), "Showing 3 of 10");
    assert_eq!(qs::bg_status_text(202, 202, ""), "Backgrounds found: 202");
    assert_eq!(qs::bg_refreshing_text(5), "🔄 Updating… (5 from cache)");
    assert_eq!(qs::bg_pager(100, 1, 5).text, "Page 2 / 4");
    assert_eq!(
        qs::autostart_refusal_text(true, "Permission denied (os error 13)").as_deref(),
        Some("❌ Couldn't turn on the startup movie at login: no permission to write to the settings folder.")
    );
    assert_eq!(
        qs::item_labels(&json!({"tags": []})),
        vec!["Not Tradable", "Not Marketable"]
    );
}

/// The language of Steam's answers follows the interface language; an unknown one means English.
#[test]
fn steam_language_follows_the_interface() {
    for (ui, steam) in [
        ("en", "english"),
        ("ru", "russian"),
        ("ru_RU.UTF-8", "russian"),
        ("de", "german"),
        ("pt_BR", "brazilian"),
        ("pt", "portuguese"),
        ("zh_TW", "tchinese"),
        ("zh_CN", "schinese"),
        ("es_MX", "latam"),
        ("es", "spanish"),
        ("ko", "koreana"),
        ("eo", "english"),
        ("", "english"),
    ] {
        assert_eq!(qs::steam_api_language(ui), steam, "{ui}");
    }
}

/// Owned backgrounds: the order "key → request → on refusal a fresh key and one
/// retry" and the verdict on why there are no animated ones.
#[test]
fn owned_backgrounds_retry_once_and_name_the_gap() {
    use qs::{AnimatedGap, OwnedError};
    use std::cell::RefCell;

    // No key — we do not go to the network at all, the reason reaches the person.
    let asked = RefCell::new(Vec::<String>::new());
    let (items, gap) = qs::owned_backgrounds_with::<u32>(
        |_force| (String::new(), "войдите в Steam".to_string()),
        |key| {
            asked.borrow_mut().push(key.to_string());
            Ok(vec![1])
        },
    );
    assert!(items.is_empty());
    assert_eq!(gap, AnimatedGap::NoKey("войдите в Steam".to_string()));
    assert!(asked.borrow().is_empty(), "без ключа запроса быть не должно");

    // There is a key, Steam answered — no gap.
    let (items, gap) =
        qs::owned_backgrounds_with(|_| ("k1".to_string(), String::new()), |_| Ok(vec![7u32, 8]));
    assert_eq!(items, [7, 8]);
    assert_eq!(gap, AnimatedGap::None);

    // The key went stale: refusal → a fresh key (force) → the retry succeeded.
    let forces = RefCell::new(Vec::<bool>::new());
    let asked = RefCell::new(Vec::<String>::new());
    let (items, gap) = qs::owned_backgrounds_with(
        |force| {
            forces.borrow_mut().push(force);
            (if force { "новый" } else { "старый" }.to_string(), String::new())
        },
        |key| {
            asked.borrow_mut().push(key.to_string());
            if key == "старый" { Err(OwnedError::Refused) } else { Ok(vec![5u32]) }
        },
    );
    assert_eq!(items, [5]);
    assert_eq!(gap, AnimatedGap::None);
    assert_eq!(*forces.borrow(), [false, true]);
    assert_eq!(*asked.borrow(), ["старый", "новый"]);

    // A refusal even with a fresh key — exactly one retry, not a loop.
    let count = RefCell::new(0u32);
    let (items, gap) = qs::owned_backgrounds_with::<u32>(
        |_| ("k".to_string(), String::new()),
        |_| {
            *count.borrow_mut() += 1;
            Err(OwnedError::Refused)
        },
    );
    assert!(items.is_empty());
    assert_eq!(gap, AnimatedGap::Refused);
    assert_eq!(*count.borrow(), 2);

    // After a refusal no fresh key was given — this is "no key", not "refused".
    let (_, gap) = qs::owned_backgrounds_with::<u32>(
        |force| if force { (String::new(), "ключа нет".to_string()) } else { ("k".to_string(), String::new()) },
        |_| Err(OwnedError::Refused),
    );
    assert_eq!(gap, AnimatedGap::NoKey("ключа нет".to_string()));

    // The network did not answer — no retry (a fresh key will not help here).
    let count = RefCell::new(0u32);
    let (_, gap) = qs::owned_backgrounds_with::<u32>(
        |_| ("k".to_string(), String::new()),
        |_| {
            *count.borrow_mut() += 1;
            Err(OwnedError::NoAnswer)
        },
    );
    assert_eq!(gap, AnimatedGap::NoAnswer);
    assert_eq!(*count.borrow(), 1);

    // «Key not accepted» — only 401 and 403.
    assert_eq!(qs::owned_error_of_status(Some(401)), OwnedError::Refused);
    assert_eq!(qs::owned_error_of_status(Some(403)), OwnedError::Refused);
    assert_eq!(qs::owned_error_of_status(Some(500)), OwnedError::NoAnswer);
    assert_eq!(qs::owned_error_of_status(None), OwnedError::NoAnswer);
}

/// The «why there are no animated ones» line: silent when there is no gap, and always
/// says what to do when there is one.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn animated_gap_text_says_what_to_do() {
    use qs::AnimatedGap;
    let _lang = i18n_pin::ru();
    assert_eq!(qs::bg_animated_gap_text(&AnimatedGap::None), "");
    let retry = qs::bg_retry_text();
    assert_eq!(retry, "Повторить");
    let no_key = qs::bg_animated_gap_text(&AnimatedGap::NoKey(
        rexon_core::steam_token::no_login_reason(),
    ));
    assert_eq!(
        no_key,
        "⚠️ Анимированные фоны не показаны: войдите в Steam на этом компьютере. \
Запустите Steam, войдите в учётную запись и нажмите «Повторить»."
    );
    for gap in [AnimatedGap::Refused, AnimatedGap::NoAnswer] {
        let text = qs::bg_animated_gap_text(&gap);
        assert!(text.starts_with("⚠️ Анимированные фоны не показаны: "), "{text}");
        assert!(text.contains(&format!("«{retry}»")), "{text}");
    }
}
