//! Сторож клиента extensions.gnome.org на СИНТЕТИЧЕСКИХ образцах.
//!
//! Образцы повторяют форму ответов API v1, снятых с сайта 09.11.2026
//! (набор и типы полей, `null` там, где сайт его отдаёт), но имена,
//! описания и адреса придуманы: чужие тексты и картинки в репозиторий не
//! кладутся. Поменяет сайт форму ответа — краснеть должно здесь, а не на
//! витрине у покупателя.

mod i18n_pin;

use std::collections::HashSet;

use rexon_core::gnome_ego::*;

/// Листинг: две записи, у второй — все поля, что сайт отдаёт `null`.
const LISTING: &str = r#"{
  "count": 620,
  "next": "https://extensions.gnome.org/api/v1/extensions/?ordering=-popularity&page=2&page_size=2&shell_version=51&status=3",
  "previous": null,
  "results": [
    {
      "id": 101,
      "uuid": "sample-dock@rexon.example",
      "name": "Sample Dock",
      "creator": {"id": 5001, "username": "sample_author", "display_name": "Sample Author"},
      "description": "A made-up dock.\nSecond line.",
      "created": "2026-02-25T17:12:27.692638",
      "updated": "2026-10-01T08:00:00.000001",
      "downloads": 1234567,
      "popularity": 413,
      "screenshot": "https://extensions.gnome.org/extension-data/screenshots/screenshot_101.webp",
      "icon": "https://extensions.gnome.org/extension-data/icons/icon_101.png",
      "rating": 4.3,
      "rated": 40,
      "url": "https://example.org/sample-dock",
      "donation_urls": ["https://example.org/donate"],
      "link": "/extension/101/sample-dock/"
    },
    {
      "id": 102,
      "uuid": "bare@rexon.example",
      "name": "Bare",
      "creator": {"id": 5002, "username": "bare.user", "display_name": ""},
      "description": "",
      "created": "2026-03-01T00:00:00",
      "updated": null,
      "downloads": 0,
      "popularity": 0,
      "screenshot": null,
      "icon": null,
      "rating": 0.0,
      "rated": 0,
      "url": "",
      "donation_urls": [],
      "link": "/extension/102/bare/"
    }
  ]
}"#;

/// Страница выпусков — от старых к новым, как отдаёт сайт.
const VERSIONS: &str = r#"{
  "count": 3,
  "next": null,
  "previous": null,
  "results": [
    {"extension": "sample-dock@rexon.example", "version": 1, "version_name": null, "status": 1,
     "shell_versions": [{"major": 48, "minor": -1, "point": -1}],
     "created": "2026-02-25T17:12:27.715200", "session_modes": [], "link": "/extension/101/sample-dock/"},
    {"extension": "sample-dock@rexon.example", "version": 2, "version_name": "1.1", "status": 3,
     "shell_versions": [{"major": 47, "minor": -1, "point": -1}, {"major": 48, "minor": -1, "point": -1},
                        {"major": 49, "minor": -1, "point": -1}, {"major": 50, "minor": -1, "point": -1}],
     "created": "2026-05-01T00:00:00", "session_modes": ["user"], "link": "/extension/101/sample-dock/"},
    {"extension": "sample-dock@rexon.example", "version": 3, "version_name": "2.0", "status": 3,
     "shell_versions": [{"major": 51, "minor": -1, "point": -1}],
     "created": "2026-10-01T00:00:00", "session_modes": ["user", "unlock-dialog"], "link": "/extension/101/sample-dock/"}
  ]
}"#;

fn shell(raw: &str) -> ShellVersion {
    parse_shell_version(raw).expect(raw)
}

#[test]
fn listing_sample_parses_into_cards() {
    // Подписи строки каталога — на английском, а не на языке машины.
    let _lang = i18n_pin::en();
    let page = parse_listing(LISTING.as_bytes()).expect("листинг разбирается");
    assert_eq!((page.total, page.has_more, page.items.len()), (620, true, 2));

    let dock = &page.items[0];
    assert_eq!(dock.uuid, "sample-dock@rexon.example");
    assert_eq!(dock.creator.display_name, "Sample Author");
    assert_eq!((dock.downloads, dock.rated), (1_234_567, 40));
    let it = theme_item(dock);
    assert_eq!(it.id, "ego:sample-dock@rexon.example");
    assert_eq!(it.category_id, 914);
    assert_eq!(it.author, "Sample Author");
    assert_eq!(it.rating, 86);
    assert_eq!(it.updated, "2026-10-01T08:00:00");
    assert_eq!(it.preview_url, dock.screenshot);
    assert!(it.download_links.is_empty());
    let row = catalog_row(&it, dock, None);
    for key in [
        "egoRating",
        "egoRated",
        "egoDownloads",
        "egoUnsupported",
        "egoPage",
        "egoDonations",
        "egoSessionModes",
        "sitePage",
        "siteStats",
    ] {
        assert!(row.get(key).is_some(), "в строке каталога нет {key}");
    }
    assert_eq!(row["egoPage"], "https://extensions.gnome.org/extension/101/sample-dock/");
    // Десятичный разделитель — по языку (`human_count`), здесь английскому.
    let dl = rexon_core::appearance_page::human_count(1_234_567);
    assert_eq!(row["siteStats"], format!("★ {} (40) · ⬇ {dl}", rexon_core::i18n::format_decimal(4.3, 1)).as_str());

    // Запись с null: значок вместо скриншота нет — и галерея пустая.
    let bare = theme_item(&page.items[1]);
    assert_eq!(bare.author, "bare.user");
    assert_eq!(bare.updated, "2026-03-01T00:00:00");
    assert!(bare.preview_url.is_empty() && bare.preview_urls.is_empty());
    assert_eq!(stats_line(&page.items[1]), "⬇ 0");
}

#[test]
fn detail_sample_is_a_listing_record() {
    // Карточка сайта — та же запись, что в листинге, без обёртки.
    let page: serde_json::Value = serde_json::from_str(LISTING).unwrap();
    let body = serde_json::to_vec(&page["results"][0]).unwrap();
    let e = parse_detail(&body).expect("карточка разбирается");
    assert_eq!(e.id, 101);
    assert!(parse_detail(br#"{"detail":"No Extension matches the given query."}"#).is_err());
}

#[test]
fn versions_sample_gives_release_for_you() {
    // Ожидание «Supports GNOME 47–51» — английская подпись: язык закреплён,
    // иначе при `LANG=ru_RU` и каталоге в /usr/share/locale сверка получала
    // «Поддерживает GNOME 47–51».
    let _lang = i18n_pin::en();
    let vs = parse_versions(VERSIONS.as_bytes()).expect("выпуски разбираются");
    assert!(!vs.has_more);
    let rs = vs.items;
    let cur = shell("51.1");
    let chosen = release_for(&rs, &cur).expect("под 51 есть выпуск");
    assert_eq!((chosen.version, chosen.label()), (3, "2.0".to_string()));
    assert_eq!(release_for(&rs, &shell("48")).map(|r| r.version), Some(2), "отклонённый 1 не берётся");
    assert_eq!(release_for(&rs, &shell("46")), None);
    assert_eq!(supported_summary(&rs), "47–51");

    let page = parse_listing(LISTING.as_bytes()).unwrap();
    let dock = &page.items[0];
    let mut row = catalog_row(&theme_item(dock), dock, None);
    with_release(&mut row, &rs, Some(&cur));
    assert_eq!(row["egoReleaseForYou"], 3);
    assert_eq!(row["egoSessionModes"], serde_json::json!(["user", "unlock-dialog"]));
    assert_eq!(row["egoSupports"], "Supports GNOME 47–51");

    let rows = release_rows(&rs, Some(&cur), Some(2));
    assert_eq!(rows.iter().map(|r| r["version"].as_i64().unwrap()).collect::<Vec<_>>(), vec![3, 2]);
    assert_eq!(rows[0]["forYou"], true);
    assert_eq!(rows[1]["installed"], true);
}

#[test]
fn urls_follow_the_published_schema() {
    let s = site_shell_version(&shell("51.1"));
    assert_eq!(s, "51");
    let u = listing_url("", "popular", 0, PAGE_SIZE, Some(&s));
    assert!(u.starts_with("https://extensions.gnome.org/api/v1/extensions/?status=3&ordering=-popularity&page=1&page_size="));
    assert!(u.ends_with("&shell_version=51"));
    let u = listing_url("blur", "down", 1, PAGE_SIZE, None);
    assert!(u.starts_with("https://extensions.gnome.org/api/v1/extensions/search/blur/?ordering=-downloads&page=2"));
    // Сортировки — только из списка схемы для маршрута поиска.
    let allowed = ["-created", "-downloads", "-popularity", "-updated"];
    for (_, key) in SORTS {
        assert!(allowed.contains(&api_ordering(key)), "{key}");
    }

    let names: Vec<String> = (0..51).map(|i| format!("e{i}@rexon.example")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let batches = compat_batch_urls(&refs, "51");
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[1].matches("&uuid=").count(), 1);
    let compatible: HashSet<String> = parse_listing(LISTING.as_bytes()).unwrap().items.into_iter().map(|e| e.uuid).collect();
    let bad = unsupported_uuids(&["sample-dock@rexon.example", "other@rexon.example"], &compatible);
    assert_eq!(bad, ["other@rexon.example".to_string()].into());
}
