//! Golden check of parsing the public Steam profile background inventory
//! against the original `rexon/qt_steam/window.py` (deterministic functions;
//! the file itself is Qt/network).
//!
//! The reference is synthetic: a live `steamcommunity.com/inventory/<id64>/753/6`
//! response needs the public inventory of a specific account, and the equipped
//! background is read from the local `localconfig.vdf` — it cannot be captured
//! reproducibly. The fixtures cover the documented fields (item_class_3,
//! actions[].link, economy CDN fallback, classid/market_hash_name,
//! movie_*/image_*). Rust must return the same records, strings and order as
//! Python.
//!
//! ⚠️ The live network call, asset download and FS reads are NOT ported to the
//! core and are not covered by golden tests — only PARSING of the raw data is
//! checked.

use rexon_core::steam_profile_items::{
    community_url_variants, finalize_items, parse_equipped_background,
    parse_inventory_backgrounds, ProfileBackgroundItem, COMMUNITY_IMG_BASE,
    COMMUNITY_IMG_BASE_LEGACY,
};
use serde_json::{json, Value};

/// Build the expected item from a fixture JSON object.
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

/// Parsing the public inventory 753/6: skipping non-backgrounds (item_class_2)
/// and records without an image, economy CDN fallback,
/// classid/market_hash_name/dedup by cid (last value, first position) and the
/// order — as in Python `_InventoryWorker.run`.
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

/// Parsing the equipped background: movie_webm beats movie_mp4, image_large
/// beats image_small, url/preview on the community CDN, `animated=true`,
/// `equipped=true`.
#[test]
fn parse_equipped_matches_python() {
    let raw: Value =
        serde_json::from_str(include_str!("golden/steam_profile_items_equipped_raw.json"))
            .unwrap();
    let got = parse_equipped_background(&raw).expect("надетый фон разобран");

    let want = item_from(&cases()["equipped_parsed"]);
    assert_eq!(got, want);
}

/// `finalize_items`: the equipped one is not in the inventory → it is inserted
/// on top; the rest are sorted by (game.lower, name.lower). Port of `_finalize_items`.
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

/// Broken/empty inventory response → empty list (port of `(data or {}).get(...)`).
#[test]
fn inventory_empty_or_broken_yields_empty() {
    assert!(parse_inventory_backgrounds(&json!({}), None).is_empty());
    assert!(parse_inventory_backgrounds(&json!({"descriptions": []}), None).is_empty());
    // descriptions is not an array → empty.
    assert!(parse_inventory_backgrounds(&json!({"descriptions": "oops"}), None).is_empty());
    // Only a non-background and a background without an image → empty.
    let only_junk = json!({"descriptions": [
        {"tags": [{"category": "item_class", "internal_name": "item_class_2"}]},
        {"tags": [{"category": "item_class", "internal_name": "item_class_3"}]}
    ]});
    assert!(parse_inventory_backgrounds(&only_junk, None).is_empty());
}

/// An equipped background without links (neither movie_* nor image_*) and a
/// missing `profile_background` → `None` (the UI falls back to the public inventory).
#[test]
fn equipped_without_link_is_none() {
    assert!(parse_equipped_background(&json!({})).is_none());
    assert!(parse_equipped_background(&json!({"profile_background": {}})).is_none());
    assert!(
        parse_equipped_background(&json!({"profile_background": {"name": "Пустой"}})).is_none()
    );
}

/// Fixed bug of the original: `str(bg.get("communityitemid", "equipped"))` with
/// an explicit `null`/empty string/0 gave "None"/""/"0"; here → fallback "equipped".
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
    // A missing key also → "equipped".
    let missing = json!({"profile_background": {"name": "Фон", "image_large": "images/x.jpg"}});
    assert_eq!(parse_equipped_background(&missing).unwrap().id, "equipped");
    // Normal values are kept.
    let num = json!({"profile_background": {"communityitemid": 12345, "image_large": "a.jpg"}});
    assert_eq!(parse_equipped_background(&num).unwrap().id, "12345");
    let string = json!({"profile_background": {"communityitemid": "abc", "image_large": "a.jpg"}});
    assert_eq!(parse_equipped_background(&string).unwrap().id, "abc");
}

/// `finalize_items`: the equipped one is already in the inventory → it is
/// marked `equipped`, moved to the top and NOT inserted again; the flag is
/// recomputed from scratch by `equipped.id`.
#[test]
fn finalize_marks_present_equipped_without_duplication() {
    let a = ProfileBackgroundItem {
        id: "aaa".into(),
        name: "Alpha".into(),
        game: "Zeta".into(),
        url: "u_a".into(),
        preview: "u_a".into(),
        animated: false,
        // A deliberately wrong initial flag — finalize must recompute it.
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
    // bbb is marked equipped and moved to the top.
    assert_eq!(got[0].id, "bbb");
    assert!(got[0].equipped);
    // This is the ORIGINAL list item, not a copy of equipped (name not overwritten).
    assert_eq!(got[0].name, "Beta");
    // aaa: the flag is reset to false (recomputed from scratch).
    assert_eq!(got[1].id, "aaa");
    assert!(!got[1].equipped);
}

/// `finalize_items` without an equipped one (`None`): nobody is marked, nothing
/// inserted, sorting by (game.lower, name.lower).
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

/// Preliminary `equipped` marking by `eq_id` in `parse_inventory_backgrounds`.
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

/// The game is taken from the `Game` tag, not from the `type` string: Steam
/// writes `type` differently in each language (the Russian «Profile Background
/// — Half-Life 2», «Half-Life 2 Uncommon Profile Background»), and cutting the
/// old prefix on the live inventory on 28.09.2026 never worked (205 of 205).
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

/// `community_url_variants`: a path on the canonical CDN → [canonical, legacy];
/// on legacy → [legacy, canonical]; unknown prefix → [url]. Port of
/// `_community_url_variants`.
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
