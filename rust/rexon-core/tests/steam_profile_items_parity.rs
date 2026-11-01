//! Golden-сверка разбора публичного инвентаря фонов профиля Steam с оригиналом
//! `rexon/qt_steam/window.py` (детерминированные функции; сам файл — Qt/сеть).
//!
//! Эталон синтетический: живой ответ `steamcommunity.com/inventory/<id64>/753/6`
//! требует публичного инвентаря конкретного аккаунта, а надетый фон читается из
//! локального `localconfig.vdf` — воспроизводимо снять нельзя. Фикстуры покрывают
//! документированные поля (item_class_3, actions[].link, economy-CDN-фолбэк,
//! classid/market_hash_name, movie_*/image_*). Rust обязан вернуть те же записи,
//! строки и порядок, что Python.
//!
//! ⚠️ Живой сетевой вызов, скачивание ассетов и чтение ФС в ядро НЕ портированы и
//! golden-тестами не покрываются — проверяется только РАЗБОР сырья.

use rexon_core::steam_profile_items::{
    community_url_variants, finalize_items, parse_equipped_background,
    parse_inventory_backgrounds, ProfileBackgroundItem, COMMUNITY_IMG_BASE,
    COMMUNITY_IMG_BASE_LEGACY,
};
use serde_json::{json, Value};

/// Собрать ожидаемый предмет из JSON-объекта фикстуры.
fn item_from(v: &Value) -> ProfileBackgroundItem {
    ProfileBackgroundItem {
        id: v["id"].as_str().unwrap().to_string(),
        name: v["name"].as_str().unwrap().to_string(),
        game: v["game"].as_str().unwrap().to_string(),
        url: v["url"].as_str().unwrap().to_string(),
        preview: v["preview"].as_str().unwrap().to_string(),
        animated: v["animated"].as_bool().unwrap(),
        equipped: v["equipped"].as_bool().unwrap(),
    }
}

fn cases() -> Value {
    serde_json::from_str(include_str!("golden/steam_profile_items_cases.json")).unwrap()
}

/// Разбор публичного инвентаря 753/6: пропуск не-фонов (item_class_2) и записей
/// без картинки, economy-CDN-фолбэк, classid/market_hash_name/дедуп по cid
/// (last-value, первая позиция) и порядок — как Python `_InventoryWorker.run`.
#[test]
fn parse_inventory_matches_python() {
    let raw: Value =
        serde_json::from_str(include_str!("golden/steam_profile_items_inventory_raw.json"))
            .unwrap();
    let got = parse_inventory_backgrounds(&raw, None);

    let cases = cases();
    let want: Vec<ProfileBackgroundItem> = cases["inventory_parsed"]
        .as_array()
        .unwrap()
        .iter()
        .map(item_from)
        .collect();

    assert_eq!(got, want);
}

/// Разбор надетого фона: movie_webm приоритетнее movie_mp4, image_large — над
/// image_small, url/preview на community-CDN, `animated=true`, `equipped=true`.
#[test]
fn parse_equipped_matches_python() {
    let raw: Value =
        serde_json::from_str(include_str!("golden/steam_profile_items_equipped_raw.json"))
            .unwrap();
    let got = parse_equipped_background(&raw).expect("надетый фон разобран");

    let want = item_from(&cases()["equipped_parsed"]);
    assert_eq!(got, want);
}

/// `finalize_items`: надетого нет в инвентаре → он вставлен наверх; остальные
/// отсортированы по (game.lower, name.lower). Порт `_finalize_items`.
#[test]
fn finalize_inserts_absent_equipped_and_sorts() {
    let cases = cases();
    let items: Vec<ProfileBackgroundItem> = cases["inventory_parsed"]
        .as_array()
        .unwrap()
        .iter()
        .map(item_from)
        .collect();
    let equipped = item_from(&cases["equipped_parsed"]);

    let got = finalize_items(items, Some(equipped));
    let want: Vec<ProfileBackgroundItem> = cases["finalized_with_equipped_absent"]
        .as_array()
        .unwrap()
        .iter()
        .map(item_from)
        .collect();

    assert_eq!(got, want);
}

/// Битый/пустой ответ инвентаря → пустой список (порт `(data or {}).get(...)`).
#[test]
fn inventory_empty_or_broken_yields_empty() {
    assert!(parse_inventory_backgrounds(&json!({}), None).is_empty());
    assert!(parse_inventory_backgrounds(&json!({"descriptions": []}), None).is_empty());
    // descriptions не массив → пусто.
    assert!(parse_inventory_backgrounds(&json!({"descriptions": "oops"}), None).is_empty());
    // Только не-фон и фон-без-картинки → пусто.
    let only_junk = json!({"descriptions": [
        {"tags": [{"category": "item_class", "internal_name": "item_class_2"}]},
        {"tags": [{"category": "item_class", "internal_name": "item_class_3"}]}
    ]});
    assert!(parse_inventory_backgrounds(&only_junk, None).is_empty());
}

/// Надетый фон без ссылок (ни movie_*, ни image_*) и отсутствие
/// `profile_background` → `None` (UI откатится на публичный инвентарь).
#[test]
fn equipped_without_link_is_none() {
    assert!(parse_equipped_background(&json!({})).is_none());
    assert!(parse_equipped_background(&json!({"profile_background": {}})).is_none());
    assert!(
        parse_equipped_background(&json!({"profile_background": {"name": "Пустой"}})).is_none()
    );
}

/// Починен баг оригинала: `str(bg.get("communityitemid", "equipped"))` при
/// явном `null`/пустой строке/0 давал "None"/""/"0"; здесь → фолбэк "equipped".
#[test]
fn equipped_id_fixed_on_degenerate_communityitemid() {
    let mk = |cid: Value| {
        json!({"profile_background": {
            "communityitemid": cid,
            "name": "Фон",
            "image_large": "images/x.jpg"
        }})
    };
    for cid in [json!(null), json!(""), json!(0)] {
        let got = parse_equipped_background(&mk(cid)).unwrap();
        assert_eq!(got.id, "equipped", "вырожденный communityitemid → equipped");
    }
    // Отсутствие ключа тоже → "equipped".
    let missing = json!({"profile_background": {"name": "Фон", "image_large": "images/x.jpg"}});
    assert_eq!(parse_equipped_background(&missing).unwrap().id, "equipped");
    // Нормальные значения сохраняются.
    let num = json!({"profile_background": {"communityitemid": 12345, "image_large": "a.jpg"}});
    assert_eq!(parse_equipped_background(&num).unwrap().id, "12345");
    let string = json!({"profile_background": {"communityitemid": "abc", "image_large": "a.jpg"}});
    assert_eq!(parse_equipped_background(&string).unwrap().id, "abc");
}

/// `finalize_items`: надетый уже в инвентаре → он помечен `equipped`, поднят
/// наверх и НЕ вставлен повторно; признак пересчитан с нуля по `equipped.id`.
#[test]
fn finalize_marks_present_equipped_without_duplication() {
    let a = ProfileBackgroundItem {
        id: "aaa".into(),
        name: "Alpha".into(),
        game: "Zeta".into(),
        url: "u_a".into(),
        preview: "u_a".into(),
        animated: false,
        // Заведомо неверный исходный флаг — finalize обязан его пересчитать.
        equipped: true,
    };
    let b = ProfileBackgroundItem {
        id: "bbb".into(),
        name: "Beta".into(),
        game: "Alpha".into(),
        url: "u_b".into(),
        preview: "u_b".into(),
        animated: false,
        equipped: false,
    };
    let equipped = ProfileBackgroundItem {
        id: "bbb".into(),
        name: "Beta (equipped copy)".into(),
        game: "Ignored".into(),
        url: "u_b".into(),
        preview: "u_b".into(),
        animated: false,
        equipped: true,
    };

    let got = finalize_items(vec![a, b], Some(equipped));
    assert_eq!(got.len(), 2, "надетый уже в списке — без дубля");
    // bbb помечен equipped и поднят наверх.
    assert_eq!(got[0].id, "bbb");
    assert!(got[0].equipped);
    // Это ИСХОДНЫЙ предмет списка, а не копия equipped (имя не перезаписано).
    assert_eq!(got[0].name, "Beta");
    // aaa: флаг сброшен в false (пересчёт с нуля).
    assert_eq!(got[1].id, "aaa");
    assert!(!got[1].equipped);
}

/// `finalize_items` без надетого (`None`): никто не помечен, ничего не вставлено,
/// сортировка по (game.lower, name.lower).
#[test]
fn finalize_without_equipped() {
    let items = vec![
        ProfileBackgroundItem {
            id: "1".into(),
            name: "b".into(),
            game: "g2".into(),
            url: String::new(),
            preview: String::new(),
            animated: false,
            equipped: true,
        },
        ProfileBackgroundItem {
            id: "2".into(),
            name: "a".into(),
            game: "g1".into(),
            url: String::new(),
            preview: String::new(),
            animated: false,
            equipped: false,
        },
    ];
    let got = finalize_items(items, None);
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|i| !i.equipped), "без equipped никто не помечен");
    assert_eq!(got[0].id, "2", "g1 < g2");
    assert_eq!(got[1].id, "1");
}

/// Предварительная пометка `equipped` по `eq_id` в `parse_inventory_backgrounds`.
#[test]
fn inventory_marks_eq_id() {
    let raw = json!({"descriptions": [
        {
            "classid": "777",
            "name": "Фон",
            "type": "Фон профиля из Game",
            "tags": [{"category": "item_class", "internal_name": "item_class_3"}],
            "actions": [{"link": "https://x/y.png"}]
        }
    ]});
    let got = parse_inventory_backgrounds(&raw, Some("777"));
    assert_eq!(got.len(), 1);
    assert!(got[0].equipped);
    assert_eq!(got[0].game, "Game", "префикс 'Фон профиля из ' вырезан");

    let got_no = parse_inventory_backgrounds(&raw, Some("other"));
    assert!(!got_no[0].equipped);
}

/// Игра берётся из метки `Game`, а не из строки `type`: Steam пишет `type`
/// по-разному на каждом языке («Фон профиля — Half-Life 2», «Half-Life 2
/// Uncommon Profile Background»), и вырезка прежнего префикса на живом
/// инвентаре 28.09.2026 не срабатывала ни разу (205 из 205).
#[test]
fn inventory_game_comes_from_the_game_tag() {
    for ty in ["Необычный Фон профиля — Half-Life 2", "Half-Life 2 Uncommon Profile Background"] {
        let raw = json!({"descriptions": [{
            "classid": "1",
            "name": "Citadel Core",
            "type": ty,
            "tags": [
                {"category": "Game", "internal_name": "app_220", "localized_tag_name": "Half-Life 2"},
                {"category": "item_class", "internal_name": "item_class_3"}
            ],
            "actions": [{"link": "https://x/y.png"}]
        }]});
        let got = parse_inventory_backgrounds(&raw, None);
        assert_eq!(got[0].game, "Half-Life 2", "{ty}");
    }
}

/// `community_url_variants`: путь на канон-CDN → [канон, legacy]; на legacy →
/// [legacy, канон]; неизвестный префикс → [url]. Порт `_community_url_variants`.
#[test]
fn url_variants_switch_cdn_hosts() {
    let canon = format!("{COMMUNITY_IMG_BASE}items/abc.jpg");
    assert_eq!(
        community_url_variants(&canon),
        vec![
            canon.clone(),
            format!("{COMMUNITY_IMG_BASE_LEGACY}items/abc.jpg")
        ]
    );

    let legacy = format!("{COMMUNITY_IMG_BASE_LEGACY}items/abc.jpg");
    assert_eq!(
        community_url_variants(&legacy),
        vec![
            legacy.clone(),
            format!("{COMMUNITY_IMG_BASE}items/abc.jpg")
        ]
    );

    let other = "https://cdn.example/z.png".to_string();
    assert_eq!(community_url_variants(&other), vec![other]);
}
