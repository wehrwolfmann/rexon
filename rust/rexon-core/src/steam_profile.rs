//! Разбор владеемых фонов Steam-профиля — порт РАЗБОРА ответа из
//! `rexon/steam_profile_items.py`.
//!
//! Портирована ДЕТЕРМИНИРОВАННАЯ часть: преобразование JSON-ответа WebAPI
//! `IPlayerService/GetProfileItemsOwned` в список фонов (`parse_owned_backgrounds`,
//! порт тела `_fetch_owned`). Добыча токена (расшифровка cookie Chromium из
//! CEF-БД, `sqlite3`, `cryptography`) и сами сетевые вызовы (`_read_steam_login_cookie`/
//! `_fetch_webapi_token`/`_fetch_owned`-запрос, `urllib`) СЮДА НЕ переносятся:
//! они недетерминированы (зависят от локального Steam) и golden-тестами не
//! покрываются — это дело GUI/сетевого слоя. Здесь только «дай JSON — получи
//! список фонов», что и сверяется с Python (golden `steam_profile_parity.rs`).
//!
//! Сверка с Python: эталон снят с самого `_fetch_owned` (монипатч `urlopen` →
//! канонический ответ схемы GetProfileItemsOwned), Rust обязан вернуть те же
//! записи, те же строки и тот же порядок.

use serde::Serialize;
use serde_json::Value;

use crate::i18n::tr;

/// Базовый CDN картинок/роликов community-предметов (`_COMMUNITY_IMG_BASE`).
pub const COMMUNITY_IMG_BASE: &str =
    "https://shared.fastly.steamstatic.com/community_assets/images/";
/// User-Agent запросов (`_UA`).
pub const USER_AGENT: &str = "Rexon/5.0";
/// Имя по умолчанию для фона без названия (на языке интерфейса).
pub fn default_name() -> String {
    // TRANSLATORS: name shown for a Steam profile background that has none.
    tr("Profile Background")
}

/// Один владеемый фон профиля — порт словаря из `_fetch_owned`.
///
/// `url` анимированных ведёт на movie_webm/mp4, статичных — на картинку;
/// `preview` всегда статичная картинка (миниатюра списка). `game` в оригинале
/// всегда "" (поле есть в словаре, сохраняем для паритета).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SteamBackground {
    pub id: String,
    pub name: String,
    pub game: String,
    pub url: String,
    pub preview: String,
    pub animated: bool,
}

/// Строковое поле объекта: "" если ключа нет/`null`/не строка.
fn s<'a>(b: &'a Value, key: &str) -> &'a str {
    b.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Порт `str(b.get("communityitemid") or "")`: числа (кроме 0) и непустые
/// строки → их текст, иначе "". В реальном ответе id — строка; целочисленный
/// вариант поддержан на случай, если сервер отдаст число.
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

/// Разбор ответа GetProfileItemsOwned в список фонов — порт тела `_fetch_owned`
/// (без сетевого запроса). Отсутствие `response`/`profile_backgrounds` →
/// пустой список; записи без ссылки (`rel`) пропускаются.
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
        // movie = movie_webm or movie_mp4 (пустая строка → следующий).
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
        // rel = movie or image; нет ссылки → пропуск.
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
