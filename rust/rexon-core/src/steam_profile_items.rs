//! Parsing of Steam profile backgrounds from the PUBLIC inventory — a port of
//! the PARSING from `rexon/qt_steam/window.py` (deterministic functions; the file
//! itself is Qt/network).
//!
//! Complements [`crate::steam_profile`]: that one covers the caller-scoped path
//! `IPlayerService/GetProfileItemsOwned` (owned backgrounds with links to the
//! animation, from `rexon/steam_profile_items.py::_fetch_owned`). HERE is the
//! second, public path: no authorization, static image only:
//!   * `parse_inventory_backgrounds` — port of parsing the response of
//!     `steamcommunity.com/inventory/<id64>/753/6` (`_InventoryWorker.run`);
//!   * `parse_equipped_background` — port of parsing the equipped background
//!     from the Steam cache `GetEquippedProfileItemsForUser<id64>`
//!     (`_equipped_profile_background`);
//!   * `finalize_items` — port of `_finalize_items` (marking the equipped one,
//!     sorting, inserting the equipped background even if the inventory lacks it);
//!   * `community_url_variants` — port of `_community_url_variants` (trying
//!     both CDN hosts of community assets).
//!
//! NOT moved here (non-deterministic, depends on the local Steam/network — stays
//! in Python): the inventory HTTP request itself (`urllib`), downloading assets
//! (`_download_community`), finding the SteamID64 and reading `localconfig.vdf`
//! (`_steam_id64`, the file part of `_equipped_profile_background` — regex
//! extraction and un-escaping of the VDF string). Python passes in raw data that
//! is already `json.load`-ed.
//!
//! Parity: parsing a fixed response must return the same records, strings and
//! order as Python (golden `steam_profile_items_parity.rs`). The live network
//! call and file system reads are not covered by golden tests.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

use crate::i18n::tr;

/// Canonical CDN of community assets (`_COMMUNITY_IMG_BASE`).
pub const COMMUNITY_IMG_BASE: &str =
    "https://shared.fastly.steamstatic.com/community_assets/images/";
/// Fallback CDN host of community assets (`_COMMUNITY_IMG_BASE_LEGACY`).
pub const COMMUNITY_IMG_BASE_LEGACY: &str =
    "https://cdn.fastly.steamstatic.com/steamcommunity/public/images/";
/// Economy CDN — fallback when an item has no action link (`_ECONOMY_IMG_BASE`).
pub const ECONOMY_IMG_BASE: &str =
    "https://community.fastly.steamstatic.com/economy/image/";
/// Default name for an equipped background without a title (the original says
/// "Equipped background"; in the UI language).
pub fn default_equipped_name() -> String {
    // TRANSLATORS: name shown for the Steam profile background the user has
    // equipped on their profile, when Steam gives it no name.
    tr("Equipped Background")
}
/// Default name for an inventory item without a title (`d.get("name") or "?"`).
pub const DEFAULT_INVENTORY_NAME: &str = "?";
/// Source caption prefix cut out of `type` (Russian "Profile background from ").
///
/// ⚠️ This is NOT a UI caption but a piece of the Steam response — it must not be
/// translated. And it is outdated: as of 28.09.2026 the Russian Steam writes
/// "Profile background — Half-Life 2", "Unusual Profile background — …" (in
/// Russian), the English one "Half-Life 2 Profile Background", and the cut did
/// not fire on any of the 205 backgrounds of the live inventory. The game is now
/// taken from the `Game` tag ([`game_of`]); the prefix is only a fallback for a
/// response without that tag.
pub const GAME_PREFIX: &str = "Фон профиля из ";

/// The game the background belongs to: the inventory tag `Game` in the response
/// language.
///
/// ⚠️ Fix (rule #16): the original cut the game out of the `type` string with
/// the [`GAME_PREFIX`] prefix, but Steam has long written `type` differently (and
/// differently in each language) — the card showed "Profile background —
/// Half-Life 2" instead of "Half-Life 2". The form of the `Game` tag does not
/// depend on the language.
fn game_of(d: &Value) -> String {
    if let Some(tags) = d.get("tags").and_then(Value::as_array) {
        for t in tags {
            if s(t, "category").eq_ignore_ascii_case("game") {
                let g = s(t, "localized_tag_name");
                if !g.is_empty() {
                    return g.to_string();
                }
            }
        }
    }
    s(d, "type").replace(GAME_PREFIX, "")
}

/// One profile background from the public inventory/equipped — a port of the
/// item dict (`_InventoryWorker`/`_equipped_profile_background`).
///
/// `animated` of public inventory items is always `false` (the public endpoint
/// gives no links to the animation — only the owned path in
/// [`crate::steam_profile`] does); for the equipped background `animated=true`
/// if there is `movie_webm`/`movie_mp4`. `game` is a short source caption ("" for
/// the equipped one).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileBackgroundItem {
    pub id: String,
    pub name: String,
    pub game: String,
    pub url: String,
    pub preview: String,
    pub animated: bool,
    pub equipped: bool,
}

/// String field of an object: "" if the key is missing/`null`/not a string.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// url + its variant on the fallback CDN host — port of `_community_url_variants`.
///
/// Steam rot: the same path may exist on only one of the two hosts, so we
/// download trying both. Returns `[url]` if the prefix matched none of the known
/// CDN bases.
pub fn community_url_variants(url: &str) -> Vec<String> {
    for (a, b) in [
        (COMMUNITY_IMG_BASE, COMMUNITY_IMG_BASE_LEGACY),
        (COMMUNITY_IMG_BASE_LEGACY, COMMUNITY_IMG_BASE),
    ] {
        if let Some(suffix) = url.strip_prefix(a) {
            return vec![url.to_string(), format!("{b}{suffix}")];
        }
    }
    vec![url.to_string()]
}

/// id of the equipped background — port of
/// `str(bg.get("communityitemid", "equipped"))`.
///
/// ⚠️ A bug of the original is fixed: `dict.get(key, default)` in Python uses the
/// default ONLY when the key is missing, so an explicit `null` gave the string
/// "None", an empty string "", zero "0" (junk ids that break matching against
/// `eq_id` in `finalize_items`). Here null/empty string/0/missing → fallback
/// "equipped"; a non-empty string or non-zero number → their text.
fn equipped_id(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                if i != 0 {
                    return i.to_string();
                }
            } else if let Some(u) = n.as_u64() {
                if u != 0 {
                    return u.to_string();
                }
            }
            "equipped".to_string()
        }
        _ => "equipped".to_string(),
    }
}

/// The background equipped in the profile from the parsed
/// `GetEquippedProfileItemsForUser` cache — port of the JSON part of
/// `_equipped_profile_background` (without reading `localconfig.vdf`).
///
/// `data` is the payload already `json.loads`-ed (Python extracts it from the
/// VDF with a regex and un-escapes it). No `profile_background`/neither
/// `movie_*` nor `image_*` → `None` (the UI falls back to the public inventory).
pub fn parse_equipped_background(data: &Value) -> Option<ProfileBackgroundItem> {
    // bg = data.get("profile_background") or {} — an object or empty.
    let null = Value::Null;
    let bg = match data.get("profile_background") {
        Some(v) if v.is_object() => v,
        _ => &null,
    };

    // movie = movie_webm or movie_mp4; image = image_large or image_small.
    let movie = {
        let w = s(bg, "movie_webm");
        if !w.is_empty() {
            w
        } else {
            s(bg, "movie_mp4")
        }
    };
    let image = {
        let l = s(bg, "image_large");
        if !l.is_empty() {
            l
        } else {
            s(bg, "image_small")
        }
    };
    // rel = movie or image; no link → None.
    let rel = if !movie.is_empty() { movie } else { image };
    if rel.is_empty() {
        return None;
    }

    let name = {
        let n = s(bg, "name");
        if !n.is_empty() {
            n.to_string()
        } else {
            let t = s(bg, "item_title");
            if !t.is_empty() {
                t.to_string()
            } else {
                default_equipped_name()
            }
        }
    };
    let url = format!("{COMMUNITY_IMG_BASE}{}", rel.trim_start_matches('/'));
    let preview = if !image.is_empty() {
        format!("{COMMUNITY_IMG_BASE}{}", image.trim_start_matches('/'))
    } else {
        String::new()
    };

    Some(ProfileBackgroundItem {
        id: equipped_id(bg.get("communityitemid")),
        name,
        game: String::new(),
        url,
        preview,
        animated: !movie.is_empty(),
        equipped: true,
    })
}

/// Profile backgrounds from the public inventory response `…/753/6` — port of
/// the `_InventoryWorker.run` loop (without the network request).
///
/// `eq_id` is the id of the equipped background (for the preliminary `equipped`
/// mark; [`finalize_items`] recomputes the result anyway). Only items with the
/// tag `item_class`=`item_class_3` are taken; the image comes from the first
/// `actions[].link` starting with `http`, otherwise the economy CDN by
/// `icon_url_large`/`icon_url`; an item without an image is skipped. Dedup by
/// `cid` with Python dict semantics: the first occurrence keeps the position, the
/// value is the last one (last-value).
pub fn parse_inventory_backgrounds(
    data: &Value,
    eq_id: Option<&str>,
) -> Vec<ProfileBackgroundItem> {
    let Some(descriptions) = data.get("descriptions").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut order: Vec<String> = Vec::new();
    let mut by_cid: HashMap<String, ProfileBackgroundItem> = HashMap::new();

    for d in descriptions {
        // Profile background = tag item_class / item_class_3.
        let is_bg = d
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter().any(|t| {
                    s(t, "category") == "item_class"
                        && s(t, "internal_name") == "item_class_3"
                })
            })
            .unwrap_or(false);
        if !is_bg {
            continue;
        }

        // img = the first actions[].link starting with http.
        let mut img = String::new();
        if let Some(actions) = d.get("actions").and_then(Value::as_array) {
            for a in actions {
                let link = s(a, "link");
                if link.starts_with("http") {
                    img = link.to_string();
                    break;
                }
            }
        }
        // Fallback: economy CDN by icon_url_large or icon_url.
        if img.is_empty() {
            let icon = {
                let l = s(d, "icon_url_large");
                if !l.is_empty() {
                    l
                } else {
                    s(d, "icon_url")
                }
            };
            if !icon.is_empty() {
                img = format!("{ECONOMY_IMG_BASE}{icon}");
            }
        }
        if img.is_empty() {
            continue;
        }

        // cid = str(classid or market_hash_name or img). In 753/6 classid and
        // market_hash_name are strings, so we treat them as strings.
        let cid = {
            let c = s(d, "classid");
            if !c.is_empty() {
                c.to_string()
            } else {
                let m = s(d, "market_hash_name");
                if !m.is_empty() {
                    m.to_string()
                } else {
                    img.clone()
                }
            }
        };

        let name = {
            let n = s(d, "name");
            if !n.is_empty() {
                n.to_string()
            } else {
                DEFAULT_INVENTORY_NAME.to_string()
            }
        };
        // game = the Game tag, otherwise (type or "").replace(GAME_PREFIX, "").
        let game = game_of(d);

        let item = ProfileBackgroundItem {
            id: cid.clone(),
            name,
            game,
            // The public inventory gives only the image — preview == url.
            url: img.clone(),
            preview: img,
            animated: false,
            equipped: eq_id == Some(cid.as_str()),
        };
        if by_cid.insert(cid.clone(), item).is_none() {
            order.push(cid);
        }
    }

    order
        .into_iter()
        .map(|cid| by_cid.remove(&cid).expect("cid в order есть в by_cid"))
        .collect()
}

/// Mark the equipped background, sort (equipped on top) and insert the equipped
/// one even if the inventory lacks it — port of `_finalize_items`.
///
/// The `equipped` flag is recomputed from scratch by `equipped.id` (a single tail
/// for the network path and the disk cache). The sort is stable, the key is
/// `(not equipped, game.lower, name.lower)`.
pub fn finalize_items(
    mut items: Vec<ProfileBackgroundItem>,
    equipped: Option<ProfileBackgroundItem>,
) -> Vec<ProfileBackgroundItem> {
    let eq_id: Option<String> = equipped.as_ref().map(|e| e.id.clone());

    for it in &mut items {
        it.equipped = eq_id.as_deref() == Some(it.id.as_str());
    }
    // Python sorted() is stable; Rust sort_by is too.
    items.sort_by(|a, b| {
        (!a.equipped, a.game.to_lowercase(), a.name.to_lowercase()).cmp(&(
            !b.equipped,
            b.game.to_lowercase(),
            b.name.to_lowercase(),
        ))
    });

    if let Some(eq) = equipped {
        let present = items
            .iter()
            .any(|i| eq_id.as_deref() == Some(i.id.as_str()));
        if !present {
            items.insert(0, eq);
        }
    }
    items
}
