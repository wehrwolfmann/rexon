//! Разбор фонов профиля Steam из ПУБЛИЧНОГО инвентаря — порт РАЗБОРА из
//! `rexon/qt_steam/window.py` (детерминированные функции; сам файл — Qt/сеть).
//!
//! Дополняет [`crate::steam_profile`]: тот покрывает caller-scoped путь
//! `IPlayerService/GetProfileItemsOwned` (владеемые фоны с ссылками на анимацию,
//! из `rexon/steam_profile_items.py::_fetch_owned`). ЗДЕСЬ — второй, публичный
//! путь: без авторизации, только статичная картинка:
//!   * `parse_inventory_backgrounds` — порт разбора ответа
//!     `steamcommunity.com/inventory/<id64>/753/6` (`_InventoryWorker.run`);
//!   * `parse_equipped_background` — порт разбора надетого фона из кэша Steam
//!     `GetEquippedProfileItemsForUser<id64>` (`_equipped_profile_background`);
//!   * `finalize_items` — порт `_finalize_items` (пометка надетого, сортировка,
//!     вставка надетого фона, даже если инвентарь его не содержит);
//!   * `community_url_variants` — порт `_community_url_variants` (перебор
//!     двух CDN-хостов community-ассетов).
//!
//! СЮДА НЕ переносится (недетерминированно, зависит от локального Steam/сети —
//! остаётся на Python): сам HTTP-запрос инвентаря (`urllib`), скачивание
//! ассетов (`_download_community`), поиск SteamID64 и чтение `localconfig.vdf`
//! (`_steam_id64`, файловая часть `_equipped_profile_background` — regex-выемка
//! и un-escape VDF-строки). Python отдаёт сюда уже `json.load`-нутое сырьё.
//!
//! Паритет: разбор фиксированного ответа обязан вернуть те же записи, строки и
//! порядок, что Python (golden `steam_profile_items_parity.rs`). Живой сетевой
//! вызов и чтение ФС golden-тестами не покрываются.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

use crate::i18n::tr;

/// Канонический CDN community-ассетов (`_COMMUNITY_IMG_BASE`).
pub const COMMUNITY_IMG_BASE: &str =
    "https://shared.fastly.steamstatic.com/community_assets/images/";
/// Запасной CDN-хост community-ассетов (`_COMMUNITY_IMG_BASE_LEGACY`).
pub const COMMUNITY_IMG_BASE_LEGACY: &str =
    "https://cdn.fastly.steamstatic.com/steamcommunity/public/images/";
/// Economy-CDN — фолбэк, когда у предмета нет action-ссылки (`_ECONOMY_IMG_BASE`).
pub const ECONOMY_IMG_BASE: &str =
    "https://community.fastly.steamstatic.com/economy/image/";
/// Имя по умолчанию для надетого фона без названия (оригинал — «Надетый
/// фон»; на языке интерфейса).
pub fn default_equipped_name() -> String {
    // TRANSLATORS: name shown for the Steam profile background the user has
    // equipped on their profile, when Steam gives it no name.
    tr("Equipped Background")
}
/// Имя по умолчанию для предмета инвентаря без названия (`d.get("name") or "?"`).
pub const DEFAULT_INVENTORY_NAME: &str = "?";
/// Префикс подписи-источника, вырезаемый из `type` (`"Фон профиля из "`).
///
/// ⚠️ Это НЕ подпись интерфейса, а кусок ответа Steam — переводу не
/// подлежит. И он устарел: на 28.09.2026 русский Steam пишет «Фон профиля —
/// Half-Life 2», «Необычный Фон профиля — …», английский — «Half-Life 2
/// Profile Background», и вырезка не срабатывала ни на одном из 205 фонов
/// живого инвентаря. Игра теперь берётся из метки `Game` ([`game_of`]),
/// префикс — только запасной путь для ответа без этой метки.
pub const GAME_PREFIX: &str = "Фон профиля из ";

/// Игра, к которой относится фон: метка инвентаря `Game` на языке ответа.
///
/// ⚠️ Починка (правило #16): оригинал вырезал игру из строки `type`
/// префиксом [`GAME_PREFIX`], а Steam давно пишет `type` иначе (и на каждом
/// языке по-своему), — под карточкой стояло «Фон профиля — Half-Life 2»
/// вместо «Half-Life 2». Метка `Game` от языка по форме не зависит.
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

/// Один фон профиля из публичного инвентаря/надетый — порт словаря-предмета
/// (`_InventoryWorker`/`_equipped_profile_background`).
///
/// `animated` у предметов публичного инвентаря всегда `false` (публичный
/// эндпоинт не отдаёт ссылок на анимацию — их даёт только owned-путь в
/// [`crate::steam_profile`]); у надетого фона `animated=true`, если есть
/// `movie_webm`/`movie_mp4`. `game` — короткая подпись-источник (у надетого "").
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

/// Строковое поле объекта: "" если ключа нет/`null`/не строка.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// url + его вариант на запасном CDN-хосте — порт `_community_url_variants`.
///
/// Steam-гниль: один и тот же путь может существовать только на одном из двух
/// хостов, поэтому качаем, перебирая оба. Возвращает `[url]`, если префикс не
/// совпал ни с одним из известных CDN-баз.
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

/// id надетого фона — порт `str(bg.get("communityitemid", "equipped"))`.
///
/// ⚠️ Починен баг оригинала: `dict.get(key, default)` в Python подставляет
/// default ТОЛЬКО при отсутствии ключа, поэтому явный `null` давал строку
/// "None", пустая строка — "", ноль — "0" (мусорные id, ломающие сверку с
/// `eq_id` в `finalize_items`). Здесь null/пустая строка/0/отсутствие →
/// фолбэк "equipped"; непустая строка или ненулевое число → их текст.
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

/// Надетый в профиле фон из распарсенного кэша `GetEquippedProfileItemsForUser`
/// — порт JSON-части `_equipped_profile_background` (без чтения `localconfig.vdf`).
///
/// `data` — уже `json.loads`-нутый payload (Python извлекает его regex-ом из
/// VDF и un-escape-ит). Нет `profile_background`/нет ни `movie_*`, ни
/// `image_*` → `None` (UI откатится на публичный инвентарь).
pub fn parse_equipped_background(data: &Value) -> Option<ProfileBackgroundItem> {
    // bg = data.get("profile_background") or {} — объект или пусто.
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
    // rel = movie or image; нет ссылки → None.
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

/// Фоны профиля из ответа публичного инвентаря `…/753/6` — порт цикла
/// `_InventoryWorker.run` (без сетевого запроса).
///
/// `eq_id` — id надетого фона (для предварительной пометки `equipped`; итог всё
/// равно пересчитывает [`finalize_items`]). Берутся только предметы с тегом
/// `item_class`=`item_class_3`; картинка — из первой `actions[].link`,
/// начинающейся с `http`, иначе economy-CDN по `icon_url_large`/`icon_url`; без
/// картинки предмет пропускается. Дедуп по `cid` с семантикой Python-словаря:
/// первое появление держит позицию, значение — последнее (last-value).
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
        // Фон профиля = tag item_class / item_class_3.
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

        // img = первая actions[].link, начинающаяся с http.
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
        // Фолбэк: economy-CDN по icon_url_large or icon_url.
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

        // cid = str(classid or market_hash_name or img). В 753/6 classid и
        // market_hash_name — строки, поэтому трактуем как строки.
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
        // game = метка Game, иначе (type or "").replace("Фон профиля из ", "").
        let game = game_of(d);

        let item = ProfileBackgroundItem {
            id: cid.clone(),
            name,
            game,
            // Публичный инвентарь отдаёт только картинку — preview == url.
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

/// Пометить надетый фон, отсортировать (надетый — вверх) и вставить надетый,
/// даже если инвентарь его не содержит — порт `_finalize_items`.
///
/// Признак `equipped` пересчитывается с нуля по `equipped.id` (единый хвост для
/// сетевого пути и дискового кэша). Сортировка стабильна, ключ —
/// `(не надет, game.lower, name.lower)`.
pub fn finalize_items(
    mut items: Vec<ProfileBackgroundItem>,
    equipped: Option<ProfileBackgroundItem>,
) -> Vec<ProfileBackgroundItem> {
    let eq_id: Option<String> = equipped.as_ref().map(|e| e.id.clone());

    for it in &mut items {
        it.equipped = eq_id.as_deref() == Some(it.id.as_str());
    }
    // Python sorted() стабилен; Rust sort_by — тоже.
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
