//! Golden check of the owned-backgrounds parser against the original
//! `rexon/steam_profile_items.py`.
//!
//! The reference was captured from Python itself (see the generator):
//! `steam_owned_raw.json` — a GetProfileItemsOwned response shape
//! (representative of the documented fields
//! communityitemid/name/item_title/image_large/image_small/movie_webm/movie_mp4;
//! a live response needs a PERSONAL webapi_token and cannot be captured),
//! `steam_owned_cases.json` — the output of `_fetch_owned` itself on that
//! response (monkeypatched `urlopen`). Rust must return the same
//! records/strings/order.
//!
//! ⚠️ The live network call and token extraction (cookie/sqlite/cryptography)
//! are NOT tested (non-deterministic and not ported to the core) — only the
//! PARSING of the recorded response is checked.

// CHECK LANGUAGE (since 28.09.2026): the default background name is the
// English `msgid`, Russian is in `po/ru.po`; the Python reference remembers
// the Russian one, and the check runs on it.
mod i18n_pin;

use rexon_core::steam_profile::parse_owned_backgrounds;
use serde_json::Value;

/// Parsing of the GetProfileItemsOwned response: id/name/url/preview/animated
/// and the order (including skipping records without a link) match Python
/// `_fetch_owned`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn parse_owned_matches_python() {
    let _lang = i18n_pin::ru();
    let raw: Value =
        serde_json::from_str(include_str!("golden/steam_owned_raw.json")).unwrap();
    let got = parse_owned_backgrounds(&raw);

    let cases: Value =
        serde_json::from_str(include_str!("golden/steam_owned_cases.json")).unwrap();
    let want = cases["parsed"].as_array().unwrap();

    assert_eq!(got.len(), want.len(), "число фонов (пропуск без ссылки)");
    assert!(!got.is_empty(), "фикстура не должна быть пустой");
    for (g, w) in got.iter().zip(want) {
        assert_eq!(g.id, w["id"].as_str().unwrap(), "id");
        assert_eq!(g.name, w["name"].as_str().unwrap(), "name {}", g.id);
        assert_eq!(g.game, w["game"].as_str().unwrap(), "game {}", g.id);
        assert_eq!(g.url, w["url"].as_str().unwrap(), "url {}", g.id);
        assert_eq!(g.preview, w["preview"].as_str().unwrap(), "preview {}", g.id);
        assert_eq!(g.animated, w["animated"].as_bool().unwrap(), "animated {}", g.id);
    }
}

/// Empty/missing response → empty list (port of `(data or {}).get(...)`).
#[test]
fn empty_response_yields_empty() {
    assert!(parse_owned_backgrounds(&serde_json::json!({})).is_empty());
    assert!(parse_owned_backgrounds(&serde_json::json!({"response": {}})).is_empty());
    assert!(
        parse_owned_backgrounds(&serde_json::json!({"response": {"profile_backgrounds": []}}))
            .is_empty()
    );
}

/// Malformed GetProfileItemsOwned responses: `response` is not an object, the
/// background list is not an array, non-object records, fields of the wrong
/// type (number instead of link/name), degenerate `communityitemid`
/// (null/0). The reference is the same fixed `_py_fetch_owned` (monkeypatched
/// `urlopen`); parsing must match and must not panic.
///
/// Cases `truncated_json`/`html_instead_of_json` never reach the core
/// (`json.load` in Python fails earlier and `_fetch_owned` returns `[]`) —
/// they have `data: null`, and here we only check that Python returned an
/// empty list.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn broken_owned_responses_match_python() {
    let _lang = i18n_pin::ru();
    let cases: Value =
        serde_json::from_str(include_str!("golden/steam_owned_cases.json")).unwrap();
    let broken = cases["broken"].as_array().expect("секция broken");
    assert!(broken.len() >= 12, "батарея кейсов не должна усохнуть");

    for c in broken {
        let name = c["name"].as_str().unwrap();
        let want = c["expected"].as_array().unwrap();
        if c["data"].is_null() {
            // Network/JSON failure: no parsing happened, Python returned empty.
            assert!(want.is_empty(), "[{name}] Python должен был отдать []");
            continue;
        }
        let got = parse_owned_backgrounds(&c["data"]);
        assert_eq!(got.len(), want.len(), "[{name}] число фонов");
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g.id, w["id"].as_str().unwrap(), "[{name}] id");
            assert_eq!(g.name, w["name"].as_str().unwrap(), "[{name}] name {}", g.id);
            assert_eq!(g.url, w["url"].as_str().unwrap(), "[{name}] url {}", g.id);
            assert_eq!(
                g.preview,
                w["preview"].as_str().unwrap(),
                "[{name}] preview {}",
                g.id
            );
            assert_eq!(
                g.animated,
                w["animated"].as_bool().unwrap(),
                "[{name}] animated {}",
                g.id
            );
        }
    }
}
