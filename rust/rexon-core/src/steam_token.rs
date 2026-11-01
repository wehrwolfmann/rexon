//! Добыча личного `webapi_token` из локального клиента Steam — порт
//! ДЕТЕРМИНИРОВАННОЙ части Python-кода (архив, метка git `python-final`):
//! чтение и расшифровка
//! cookie `steamLoginSecure` из CEF-БД клиента Steam (SQLite + Chromium-схема
//! v10). Тем же токеном пользуются ОБЕ вкладки — «Steam» (фоны профиля) и
//! «Wallpaper Engine» (Мастерская); второй копии этого кода в проекте быть не
//! должно (правило #12).
//!
//! Что здесь (перенесено с Python):
//!   * [`cookie_db_paths`]   — перечисление путей к CEF `Cookies`
//!     (`config/htmlcache/{Default,,*}/Cookies`), порт `_cookie_db_paths`;
//!   * [`decrypt_v10`]       — расшифровка Chromium-cookie v10 (AES-128-CBC,
//!     ключ PBKDF2-HMAC-SHA1 pw="peanuts" salt="saltysalt" iter=1, IV=16
//!     пробелов; PKCS7-распад; срез 32-байтного SHA256-префикса домена у новых
//!     сборок), порт `_decrypt_v10`;
//!   * [`read_login_cookie`] — выбор нужной cookie из БД (SQLite, только
//!     чтение), порт `read_login_cookie`.
//!
//! ⚠️ Граница «сюда не переносится HTTP» СНЯТА 2026-09-08, и не по недосмотру:
//! сетевой слой ядра появился (`net::UreqNet`), запрос `GetProfileItemsOwned`
//! давно ходит через него из вкладки «Steam» на Rust, и единственным куском,
//! оставшимся на `urllib` Python, был обмен cookie на токен. Пока он там,
//! вкладка на Rust не могла добыть токен САМА — её объект побочных действий
//! обязан был просить его у Python. Теперь обмен и кэш живут здесь
//! ([`fetch_webapi_token`], [`access_token`]), то есть в одной точке на весь
//! продукт: тем же токеном пользуются обе вкладки.
//!
//! ⚠️ СЕКРЕТЫ. Значение cookie и производный токен — секрет пользователя. Модуль
//! НИКОГДА не пишет их в журнал, в сообщения об ошибках и в отладочный вывод:
//! [`CookieError`] несёт только КЛАСС сбоя без значений, а сами функции ничего
//! не логируют. Оригинал на Python сыпал `log.debug` с текстом исключения
//! (`v10-decrypt failed: %s`, `open cookies %s failed` и т.п.) — при включённом
//! debug туда попадали путь к БД и текст ошибки; здесь этого нет (см. §"Дефекты
//! оригинала" ниже, п.1).
//!
//! Дефекты оригинала, поправленные при порте (правило #16 — каждый назван):
//!   1. Утечка в журнал. `_decrypt_v10`/`read_login_cookie` на Python писали
//!      `log.debug(... %s, e)` с путём БД и текстом ошибки расшифровки. Секрет
//!      сам по себе туда не попадал, но путь профиля и детали — да. В Rust
//!      диагностики со значениями нет вовсе.
//!   2. Тихая зависимость от `cryptography`. На Python при отсутствии пакета
//!      `_decrypt_v10` молча возвращал `None`, и вкладка «откатывалась на
//!      публичный инвентарь» без объяснения. В Rust крипта вкомпилирована —
//!      этот класс отказа исчез.
//!
//! Известное ХРУПКОЕ место (сохранено ради паритета, НЕ дефект-краш): срез
//! 32-байтного префикса домена делается только если ВЕСЬ расшифрованный буфер
//! не декодируется как UTF-8. Если 32 байта SHA256 случайно окажутся валидным
//! UTF-8, вернётся значение вместе с префиксом — ровно как в оригинале. Менять
//! эвристику здесь нельзя: это сломало бы побайтовый паритет. Настоящая починка
//! (детектить префикс по длине/схеме) — отдельная задача поверх обмена токена.

use std::path::{Path, PathBuf};

use crate::i18n::tr;
use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use rusqlite::OpenFlags;
use sha1::Sha1;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Класс сбоя добычи cookie — БЕЗ значений (секреты не попадают в текст).
/// Namespace-only: используется в тестах и внутренней логике, наружу
/// (в UI/шим) отдаётся `Option`, как в оригинале.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CookieError {
    /// Ни одной читаемой CEF-БД по путям профиля.
    NoDatabase,
    /// БД есть, но подходящего `steamLoginSecure` в ней не нашлось.
    NotFound,
}

// ── ключ и параметры схемы v10 (Linux-фолбэк Chromium) ────────────────
const V10_PREFIX: &[u8; 3] = b"v10";
const KDF_PASSWORD: &[u8] = b"peanuts";
const KDF_SALT: &[u8] = b"saltysalt";
const KDF_ITERATIONS: u32 = 1;
const KEY_LEN: usize = 16;
/// IV Chromium под Linux — 16 пробелов.
const IV: [u8; 16] = [b' '; 16];
/// Длина SHA256-префикса домена, который новые сборки Chromium дописывают
/// перед значением cookie.
const DOMAIN_HASH_LEN: usize = 32;

/// 16-байтный ключ схемы v10: PBKDF2-HMAC-SHA1(pw="peanuts", salt="saltysalt",
/// iter=1). Детерминирован — вычисляется каждый раз, кэш не нужен.
fn derive_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<Sha1>(KDF_PASSWORD, KDF_SALT, KDF_ITERATIONS, &mut key);
    key
}

/// Расшифровать Chromium-cookie схемы v10 (порт `_decrypt_v10`).
///
/// Возвращает расшифрованное значение либо `None`, если вход не v10, слишком
/// короткий, не расшифровывается, не проходит PKCS7-распад или не декодируется
/// в UTF-8 (после возможного среза 32-байтного префикса домена). Паритет с
/// Python — побайтовый (см. `tests/steam_token_parity.rs`).
pub fn decrypt_v10(enc: &[u8]) -> Option<String> {
    if enc.len() < V10_PREFIX.len() || &enc[..V10_PREFIX.len()] != V10_PREFIX {
        return None;
    }
    let body = &enc[V10_PREFIX.len()..];
    // Пустое тело / не кратное блоку — CBC/PKCS7 не разберёт: у Python это
    // ветка `except` → None. Копируем в буфер: decrypt_padded_mut пишет на месте.
    let mut buf = body.to_vec();
    let key = derive_key();
    let plain = Aes128CbcDec::new(&key.into(), &IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;

    // Порядок как в оригинале: сперва весь буфер, затем срез 32 байт префикса.
    // Возвращаем ПЕРВЫЙ вариант, который декодируется в UTF-8.
    if let Ok(s) = std::str::from_utf8(plain) {
        return Some(s.to_owned());
    }
    let tail = if plain.len() >= DOMAIN_HASH_LEN {
        &plain[DOMAIN_HASH_LEN..]
    } else {
        // В Python `raw[32:]` для короткого буфера = b"" → "".decode() = "".
        &plain[plain.len()..]
    };
    std::str::from_utf8(tail).ok().map(str::to_owned)
}

/// Возможные пути к CEF `Cookies` внутри корня Steam (порт `_cookie_db_paths`).
///
/// Порядок обхода ВАЖЕН и совпадает с оригиналом: `Default/Cookies`,
/// `Cookies`, затем прочие профили `*/Cookies` в лексикографическом порядке.
/// Дубли (тот же путь из glob) отбрасываются, несуществующие файлы — тоже.
pub fn cookie_db_paths(steam_root: &Path) -> Vec<PathBuf> {
    let base = steam_root.join("config").join("htmlcache");
    let mut candidates: Vec<PathBuf> = vec![
        base.join("Default").join("Cookies"),
        base.join("Cookies"),
    ];

    // glob(base/*/Cookies), отсортированный по имени профиля — как sorted() в
    // Python (тот сортирует полные пути, но общий префикс одинаков, значит
    // порядок эквивалентен сортировке по имени каталога профиля).
    let mut globbed: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                globbed.push(path.join("Cookies"));
            }
        }
    }
    globbed.sort();
    candidates.extend(globbed);

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for path in candidates {
        if seen.contains(&path) {
            continue;
        }
        seen.insert(path.clone());
        if path.is_file() {
            out.push(path);
        }
    }
    out
}

/// Расшифрованное значение cookie `steamLoginSecure` из CEF-БД клиента Steam
/// (порт `read_login_cookie`), либо `Err` с классом сбоя (без значений).
///
/// Логика выбора — как в оригинале: по всем БД (в порядке [`cookie_db_paths`])
/// читаются все `encrypted_value` строк `name='steamLoginSecure'`; первое
/// значение, содержащее разделитель `steamid||token` (`%7C%7C` или `||`),
/// возвращается сразу; иначе запоминается ПЕРВОЕ расшифрованное значение как
/// фолбэк и возвращается в конце.
pub fn read_login_cookie(steam_root: &Path) -> Result<String, CookieError> {
    let paths = cookie_db_paths(steam_root);
    if paths.is_empty() {
        return Err(CookieError::NoDatabase);
    }
    let mut fallback: Option<String> = None;
    for db in &paths {
        for value in read_encrypted_values(db) {
            let Some(decoded) = decrypt_v10(&value) else { continue };
            if decoded.is_empty() {
                // Python: `if not val: continue` — пустая строка не значение.
                continue;
            }
            if decoded.contains("%7C%7C") || decoded.contains("||") {
                return Ok(decoded); // полноценный steamid||token
            }
            if fallback.is_none() {
                fallback = Some(decoded);
            }
        }
    }
    fallback.ok_or(CookieError::NotFound)
}

/// Прочитать `encrypted_value` всех строк `steamLoginSecure` одной БД.
/// Ошибку открытия/чтения гасим в пустой список — как оригинал переходил к
/// следующей БД; путь и текст ошибки НИКУДА не пишем (секрет-гигиена).
fn read_encrypted_values(db: &Path) -> Vec<Vec<u8>> {
    // immutable=1 + read-only: читаем даже при запущенном Steam (локи игнорим),
    // как URI-открытие оригинала. rusqlite сам собирает URI при флаге URI.
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let Some(uri) = db.to_str() else { return Vec::new() };
    let uri = format!("file:{}?immutable=1&mode=ro", uri_escape(uri));
    let Ok(conn) = rusqlite::Connection::open_with_flags(uri, flags) else {
        return Vec::new();
    };
    let sql = "SELECT encrypted_value FROM cookies \
               WHERE name='steamLoginSecure' AND encrypted_value IS NOT NULL";
    let Ok(mut stmt) = conn.prepare(sql) else { return Vec::new() };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// Минимальное экранирование пути для `file:`-URI SQLite: пробел и `?`/`#`,
/// которые иначе оборвут путь. Полный percent-encode не нужен — путь локальный.
fn uri_escape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            _ => out.push(ch),
        }
    }
    out
}


// ============================================================================
// cookie → webapi_token: личный токен пользователя
// ============================================================================
//
// Порт `steam_roots`, `fetch_webapi_token`, `_fetch_token`, `_TokenCache` и
// `access_token`/`forget_token` из Python-архива (`python-final`). Отсюда токен
// берут ОБЕ вкладки — «Steam» (владеемые фоны профиля) и «Wallpaper Engine»
// (Мастерская); второй копии этого кода в проекте быть не должно (правило #12).

/// Сколько живёт добытый токен, прежде чем его перечитают.
///
/// Токен короткоживущий; перечитываем не чаще раза в десять минут, но
/// принудительно — сразу после отказа Steam (401/403), см. [`forget_token`].
pub const TOKEN_TTL_SEC: u64 = 600;

/// Точка обмена cookie на токен.
///
/// ⚠️ Именно `steamcommunity`, а НЕ `store.steampowered.com`: магазинный домен
/// с той же cookie уводит в 302-петлю входа, community-домен отдаёт токен
/// (проверено живьём).
pub const ASYNC_CONFIG_URL: &str = "https://steamcommunity.com/pointssummary/ajaxgetasyncconfig";

/// Таймаут обмена, секунды (`timeout=20` оригинала).
pub const TOKEN_TIMEOUT_SEC: u64 = 20;

/// Причина отказа: клиента Steam на машине нет либо в него не входили.
pub fn no_login_reason() -> String {
    // TRANSLATORS: why Steam items can't be loaded; starts in lower case.
    tr("sign in to Steam on this computer")
}

/// Причина отказа: cookie есть, а токена Steam не дал.
pub fn no_token_reason() -> String {
    // TRANSLATORS: why Steam items can't be loaded; starts in lower case.
    tr("the Steam client didn't provide an access key")
}

/// Существующие корни Steam: обычный, `~/.steam/steam`, `~/.steam/root` и два
/// Flatpak-варианта — порт `steam_roots`.
///
/// ⚠️ Это НЕ тот список, что у заставки (`session_splash::steam_root_candidates`),
/// и сводить их в один нельзя: заставке нужен каталог с `config/`, где лежат
/// стартовые ролики, а cookie клиента живёт в том числе внутри Flatpak-песочницы,
/// куда заставка не заглядывает вовсе. Разными их сделал оригинал, и по делу.
pub fn steam_roots(home: &str) -> Vec<PathBuf> {
    let candidates = [
        format!("{home}/.local/share/Steam"),
        format!("{home}/.steam/steam"),
        format!("{home}/.steam/root"),
        format!("{home}/.var/app/com.valvesoftware.Steam/data/Steam"),
        format!("{home}/.var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ];
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for cand in candidates {
        let Ok(real) = std::fs::canonicalize(&cand) else { continue };
        if !real.is_dir() || !seen.insert(real.clone()) {
            continue;
        }
        out.push(real);
    }
    out
}

/// Обменять cookie входа на короткоживущий `webapi_token` — порт
/// `fetch_webapi_token`.
///
/// `None` — сеть не ответила, cookie протухла либо ответ без токена. Причину
/// НЕ логируем и наружу не отдаём: в текст ошибки `ureq` попадает URL, а
/// cookie уходит заголовком — секрет-гигиена модуля (см. шапку).
pub fn fetch_webapi_token(login_cookie: &str) -> Option<String> {
    let mut net = crate::net::UreqNet::new()
        .with_headers(vec![(
            "Cookie".to_string(),
            format!("steamLoginSecure={login_cookie}"),
        )]);
    let body = crate::net::NetIo::get(
        &mut net,
        ASYNC_CONFIG_URL,
        crate::qt_steam::USER_AGENT,
        TOKEN_TIMEOUT_SEC,
    )
    .ok()?;
    let data: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let token = data.get("data")?.get("webapi_token")?.as_str()?;
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// SteamID64 из cookie входа: `steamid||token` после percent-раскодировки —
/// порт двух строк `_fetch_token`. Пусто — разделителя в cookie нет.
pub fn steamid_from_cookie(cookie: &str) -> String {
    let raw = percent_decode(cookie);
    match raw.split_once("||") {
        Some((sid, _)) => sid.to_string(),
        None => String::new(),
    }
}

/// `urllib.parse.unquote` в объёме, который нужен cookie: `%XX` → байт,
/// остальное как есть. Плюсы в пробелы НЕ превращаются — `unquote` этого тоже
/// не делает (это `unquote_plus`).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Один заход за токеном: `(токен, steamid64, причина отказа)` — порт
/// `_fetch_token`.
fn fetch_token(home: &str) -> (String, String, String) {
    let mut cookie = String::new();
    for root in steam_roots(home) {
        if let Ok(value) = read_login_cookie(&root) {
            cookie = value;
            break;
        }
    }
    if cookie.is_empty() {
        return (String::new(), String::new(), no_login_reason());
    }
    let steamid = steamid_from_cookie(&cookie);
    match fetch_webapi_token(&cookie) {
        Some(token) => (token, steamid, String::new()),
        None => (String::new(), steamid, no_token_reason()),
    }
}

/// Кэш токена — порт `_TokenCache`. Один на процесс: токен общий для вкладок,
/// и второй кэш означал бы два похода в сеть за одним и тем же.
struct TokenCache {
    token: String,
    steamid: String,
    error: String,
    stamp: Option<std::time::Instant>,
}

static TOKEN_CACHE: std::sync::Mutex<TokenCache> = std::sync::Mutex::new(TokenCache {
    token: String::new(),
    steamid: String::new(),
    error: String::new(),
    stamp: None,
});

/// Личный токен пользователя: `(токен, причина-осечки)`. Пустой токен значит
/// «работаем без входа», а не поломку — вкладка покажет публичный инвентарь.
///
/// `force` — не смотреть на кэш (Steam отказал, токен протух раньше срока).
///
/// ⚠️ Ходит в сеть и потому НИКОГДА не зовётся из потока интерфейса.
pub fn access_token(home: &str, force: bool) -> (String, String) {
    // Замок держится на всё время похода в сеть — намеренно, как `with
    // self._lock` оригинала: иначе две вкладки, стартующие разом, сходили бы
    // за одним токеном дважды.
    let mut cache = match TOKEN_CACHE.lock() {
        Ok(c) => c,
        // Замок отравлен — паниковать здесь нельзя: отсутствие токена не
        // повод ронять окно, вкладка обойдётся публичным инвентарём.
        Err(poisoned) => poisoned.into_inner(),
    };
    let fresh = cache
        .stamp
        .map(|t| t.elapsed().as_secs() < TOKEN_TTL_SEC)
        .unwrap_or(false);
    if !cache.token.is_empty() && fresh && !force {
        return (cache.token.clone(), String::new());
    }
    let (token, steamid, error) = fetch_token(home);
    cache.stamp = Some(std::time::Instant::now());
    if token.is_empty() {
        cache.token = String::new();
        cache.error = error;
    } else {
        cache.token = token;
        cache.steamid = steamid;
        cache.error = String::new();
    }
    (cache.token.clone(), cache.error.clone())
}

/// SteamID64 последнего удачного захода — порт `_TokenCache.steamid`.
pub fn token_steamid() -> String {
    match TOKEN_CACHE.lock() {
        Ok(c) => c.steamid.clone(),
        Err(poisoned) => poisoned.into_inner().steamid.clone(),
    }
}

/// Забыть токен: Steam отказал, при следующем запросе перечитаем — порт
/// `forget_token`.
pub fn forget_token() {
    let mut cache = match TOKEN_CACHE.lock() {
        Ok(c) => c,
        Err(poisoned) => poisoned.into_inner(),
    };
    cache.token = String::new();
    cache.stamp = None;
}

#[cfg(test)]
mod token_tests {
    use super::*;

    /// Percent-раскодировка cookie: `%7C%7C` — это `||`, иначе SteamID64 из
    /// неё не достать вовсе.
    #[test]
    fn steamid_is_read_through_percent_encoding() {
        assert_eq!(
            steamid_from_cookie("76561198000000000%7C%7CabcDEF"),
            "76561198000000000"
        );
        assert_eq!(steamid_from_cookie("76561198000000000||abcDEF"), "76561198000000000");
        assert_eq!(steamid_from_cookie("совсем не cookie"), "");
    }

    /// Хвостовой `%` и битая пара не должны ни падать, ни съедать символы.
    #[test]
    fn broken_percent_sequences_survive() {
        assert_eq!(percent_decode("a%"), "a%");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("a%41b"), "aAb");
    }

    /// Список корней для cookie ШИРЕ, чем список заставки: Flatpak-песочница
    /// входит только сюда. Сведи их в один — и вкладка перестала бы находить
    /// вход у покупателя с Flatpak-Steam.
    #[test]
    fn cookie_roots_cover_flatpak_unlike_splash_roots() {
        let splash = crate::session_splash::steam_root_candidates("/дом");
        assert!(splash.iter().all(|p| !p.contains("com.valvesoftware.Steam")));
        // Сам список корней проверяется на живой ФС (`canonicalize`), поэтому
        // сверяем не его вывод, а то, что кандидаты вообще разные.
        assert_eq!(splash.len(), 3);
    }
}
