//! Golden-сверка парсера владеемых фонов с оригиналом
//! `rexon/steam_profile_items.py`.
//!
//! Эталон снят с самого Python (см. генератор): `steam_owned_raw.json` — ответ
//! схемы GetProfileItemsOwned (репрезентативный по документированным полям
//! communityitemid/name/item_title/image_large/image_small/movie_webm/movie_mp4;
//! живой ответ требует ЛИЧНОГО webapi_token и снят быть не может),
//! `steam_owned_cases.json` — выход самого `_fetch_owned` на этом ответе
//! (монипатч `urlopen`). Rust обязан вернуть те же записи/строки/порядок.
//!
//! ⚠️ Живой сетевой вызов и добыча токена (cookie/sqlite/cryptography) НЕ
//! тестируются (недетерминированы и в ядро не портированы) — проверяется только
//! РАЗБОР зафиксированного ответа.

// ЯЗЫК СВЕРКИ (с 28.09.2026): имя фона по умолчанию — английский `msgid`,
// русский — в `po/ru.po`; эталон Python помнит русское, сверка идёт на нём.
mod i18n_pin;

use rexon_core::steam_profile::parse_owned_backgrounds;
use serde_json::Value;

/// Разбор ответа GetProfileItemsOwned: id/name/url/preview/animated и порядок
/// (включая пропуск записей без ссылки) совпадают с Python `_fetch_owned`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Пустой/отсутствующий ответ → пустой список (порт `(data or {}).get(...)`).
#[test]
fn empty_response_yields_empty() {
    assert!(parse_owned_backgrounds(&serde_json::json!({})).is_empty());
    assert!(parse_owned_backgrounds(&serde_json::json!({"response": {}})).is_empty());
    assert!(
        parse_owned_backgrounds(&serde_json::json!({"response": {"profile_backgrounds": []}}))
            .is_empty()
    );
}

/// Кривые ответы GetProfileItemsOwned: `response` не объект, список фонов не
/// массив, записи-не-объекты, поля не того типа (число вместо ссылки/имени),
/// вырожденный `communityitemid` (null/0). Эталон — тот же починенный
/// `_py_fetch_owned` (монипатч `urlopen`); разбор обязан совпасть и не паниковать.
///
/// Кейсы `truncated_json`/`html_instead_of_json` до ядра не доходят (`json.load`
/// в Python валится раньше и `_fetch_owned` отдаёт `[]`) — у них `data: null`,
/// и здесь проверяется только, что Python отдал пустой список.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
            // Сетевой/JSON-сбой: разбора не было, Python вернул пусто.
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
