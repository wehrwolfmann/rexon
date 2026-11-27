//! Parsing of owned Steam profile backgrounds — a port of the RESPONSE PARSING
//! from `rexon/steam_profile_items.py`.
//!
//! Ported is the DETERMINISTIC part: turning the WebAPI
//! `IPlayerService/GetProfileItemsOwned` JSON response into a list of backgrounds
//! (`parse_owned_backgrounds`, a port of the `_fetch_owned` body). Getting the
//! token (decrypting the Chromium cookie from the CEF DB, `sqlite3`,
//! `cryptography`) and the network calls themselves (`_read_steam_login_cookie`/
//! `_fetch_webapi_token`/the `_fetch_owned` request, `urllib`) are NOT moved here:
//! they are non-deterministic (depend on the local Steam) and not covered by
//! golden tests — that is the job of the GUI/network layer. Here it is only
//! "give JSON — get a list of backgrounds", which is checked against Python
//! (golden `steam_profile_parity.rs`).
//!
//! Parity with Python: the reference was taken from `_fetch_owned` itself
//! (monkeypatched `urlopen` → a canonical GetProfileItemsOwned response); Rust
//! must return the same records, the same strings and the same order.

use serde::Serialize;
use serde_json::Value;

use crate::i18n::tr;

/// Base CDN for images/videos of community items (`_COMMUNITY_IMG_BASE`).
pub const COMMUNITY_IMG_BASE: &str =
    "https://shared.fastly.steamstatic.com/community_assets/images/";
/// User-Agent of the requests (`_UA`).
pub const USER_AGENT: &str = "Rexon/5.0";
/// Default name for a background without a title (in the UI language).
pub fn default_name() -> String {
    // TRANSLATORS: name shown for a Steam profile background that has none.
    tr("Profile Background")
}

/// One owned profile background — a port of the dict from `_fetch_owned`.
///
/// `url` of animated ones points to movie_webm/mp4, of static ones to the image;
/// `preview` is always a static image (the list thumbnail). `game` in the
/// original is always "" (the field is in the dict, kept for parity).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SteamBackground {
    pub id: String,
    pub name: String,
    pub game: String,
    pub url: String,
    pub preview: String,
    pub animated: bool,
}

/// String field of an object: "" if the key is missing/`null`/not a string.
fn s<'a>(b: &'a Value, key: &str) -> &'a str {
    b.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Port of `str(b.get("communityitemid") or "")`: numbers (except 0) and
/// non-empty strings → their text, otherwise "". In a real response the id is a
/// string; the integer variant is supported in case the server sends a number.
fn community_id(v: Option<&Value>) -> String {
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
            String::new()
        }
        _ => String::new(),
    }
}

/// Parse a GetProfileItemsOwned response into a list of backgrounds — a port of
/// the `_fetch_owned` body (without the network request). Missing
/// `response`/`profile_backgrounds` → empty list; records without a link (`rel`)
/// are skipped.
pub fn parse_owned_backgrounds(data: &Value) -> Vec<SteamBackground> {
    let resp = data.get("response");
    let list = resp
        .and_then(|r| r.get("profile_backgrounds"))
        .and_then(Value::as_array);
    let Some(list) = list else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for b in list {
        // movie = movie_webm or movie_mp4 (empty string → the next one).
        let movie = {
            let w = s(b, "movie_webm");
            if !w.is_empty() {
                w
            } else {
                s(b, "movie_mp4")
            }
        };
        // image = image_large or image_small.
        let image = {
            let l = s(b, "image_large");
            if !l.is_empty() {
                l
            } else {
                s(b, "image_small")
            }
        };
        // rel = movie or image; no link → skip.
        let rel = if !movie.is_empty() { movie } else { image };
        if rel.is_empty() {
            continue;
        }

        let cid = community_id(b.get("communityitemid"));
        let name_val = s(b, "name");
        // id = cid or (name or rel).
        let id = if !cid.is_empty() {
            cid
        } else if !name_val.is_empty() {
            name_val.to_string()
        } else {
            rel.to_string()
        };
        // name = name or item_title or default_name().
        let name = if !name_val.is_empty() {
            name_val.to_string()
        } else {
            let t = s(b, "item_title");
            if !t.is_empty() {
                t.to_string()
            } else {
                default_name()
            }
        };

        let url = format!("{COMMUNITY_IMG_BASE}{}", rel.trim_start_matches('/'));
        let preview = if !image.is_empty() {
            format!("{COMMUNITY_IMG_BASE}{}", image.trim_start_matches('/'))
        } else {
            String::new()
        };

        out.push(SteamBackground {
            id,
            name,
            game: String::new(),
            url,
            preview,
            animated: !movie.is_empty(),
        });
    }
    out
}
