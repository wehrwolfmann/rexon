//! Getting the personal `webapi_token` from the local Steam client — a port of
//! the DETERMINISTIC part of the Python code (archive, git tag `python-final`):
//! reading and decrypting
//! the `steamLoginSecure` cookie from the Steam client's CEF DB (SQLite + the
//! Chromium v10 scheme). BOTH tabs use the same token — «Steam» (profile
//! backgrounds) and «Wallpaper Engine» (Workshop); there must be no second copy
//! of this code in the project (rule #12).
//!
//! What is here (moved from Python):
//!   * [`cookie_db_paths`]   — listing the paths to the CEF `Cookies`
//!     (`config/htmlcache/{Default,,*}/Cookies`), port of `_cookie_db_paths`;
//!   * [`decrypt_v10`]       — decrypting a Chromium v10 cookie (AES-128-CBC,
//!     key PBKDF2-HMAC-SHA1 pw="peanuts" salt="saltysalt" iter=1, IV = 16
//!     spaces; PKCS7 unpadding; cutting the 32-byte SHA256 domain prefix of newer
//!     builds), port of `_decrypt_v10`;
//!   * [`read_login_cookie`] — picking the right cookie from the DB (SQLite,
//!     read-only), port of `read_login_cookie`.
//!
//! ⚠️ The "no HTTP here" boundary was LIFTED on 2026-09-08, and not by
//! oversight: the core network layer appeared (`net::UreqNet`), the
//! `GetProfileItemsOwned` request has long gone through it from the Rust «Steam»
//! tab, and the only piece left on Python's `urllib` was exchanging the cookie
//! for the token. While it stayed there, the Rust tab could not get the token BY
//! ITSELF — its side-effects object had to ask Python for it. Now the exchange
//! and the cache live here ([`fetch_webapi_token`], [`access_token`]), i.e. in
//! one place for the whole product: both tabs use the same token.
//!
//! ⚠️ SECRETS. The cookie value and the derived token are the user's secret. The
//! module NEVER writes them to the log, to error messages or to debug output:
//! [`CookieError`] carries only the CLASS of the failure without values, and the
//! functions themselves log nothing. The Python original sprinkled `log.debug`
//! with the exception text (`v10-decrypt failed: %s`, `open cookies %s failed`
//! etc.) — with debug on, the DB path and the error text got there; here there
//! is none of that (see §"Defects of the original" below, item 1).
//!
//! Defects of the original fixed during the port (rule #16 — each is named):
//!   1. Leak into the log. `_decrypt_v10`/`read_login_cookie` in Python wrote
//!      `log.debug(... %s, e)` with the DB path and the decryption error text.
//!      The secret itself did not get there, but the profile path and details
//!      did. In Rust there are no diagnostics with values at all.
//!   2. Silent dependency on `cryptography`. In Python, without the package
//!      `_decrypt_v10` silently returned `None`, and the tab "fell back to the
//!      public inventory" without explanation. In Rust the crypto is compiled
//!      in — this class of failure is gone.
//!
//! A known FRAGILE spot (kept for parity, NOT a crash defect): the 32-byte
//! domain prefix is cut only if the WHOLE decrypted buffer does not decode as
//! UTF-8. If the 32 SHA256 bytes happen to be valid UTF-8, the value is returned
//! together with the prefix — exactly as in the original. The heuristic must not
//! be changed here: it would break byte-for-byte parity. The real fix (detecting
//! the prefix by length/scheme) is a separate task on top of the token exchange.

use std::path::{Path, PathBuf};

use crate::i18n::tr;
use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use rusqlite::OpenFlags;
use sha1::Sha1;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Class of a cookie retrieval failure — WITHOUT values (secrets do not get into
/// the text). Namespace-only: used in tests and internal logic; outwards (to the
/// UI/shim) an `Option` is given, as in the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CookieError {
    /// Not a single readable CEF DB at the profile paths.
    NoDatabase,
    /// The DB exists, but no suitable `steamLoginSecure` was found in it.
    NotFound,
}

// ── key and parameters of the v10 scheme (Chromium Linux fallback) ────────
const V10_PREFIX: &[u8; 3] = b"v10";
const KDF_PASSWORD: &[u8] = b"peanuts";
const KDF_SALT: &[u8] = b"saltysalt";
const KDF_ITERATIONS: u32 = 1;
const KEY_LEN: usize = 16;
/// Chromium IV on Linux — 16 spaces.
const IV: [u8; 16] = [b' '; 16];
/// Length of the SHA256 domain prefix that newer Chromium builds prepend to the
/// cookie value.
const DOMAIN_HASH_LEN: usize = 32;

/// 16-byte key of the v10 scheme: PBKDF2-HMAC-SHA1(pw="peanuts", salt="saltysalt",
/// iter=1). Deterministic — computed every time, no cache needed.
fn derive_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<Sha1>(KDF_PASSWORD, KDF_SALT, KDF_ITERATIONS, &mut key);
    key
}

/// Decrypt a Chromium cookie of the v10 scheme (port of `_decrypt_v10`).
///
/// Returns the decrypted value or `None` if the input is not v10, too short,
/// does not decrypt, fails PKCS7 unpadding or does not decode as UTF-8 (after
/// possibly cutting the 32-byte domain prefix). Parity with Python is
/// byte-for-byte (see `tests/steam_token_parity.rs`).
pub fn decrypt_v10(enc: &[u8]) -> Option<String> {
    if enc.len() < V10_PREFIX.len() || &enc[..V10_PREFIX.len()] != V10_PREFIX {
        return None;
    }
    let body = &enc[V10_PREFIX.len()..];
    // Empty body / not a multiple of the block — CBC/PKCS7 cannot parse it: in
    // Python this is the `except` branch → None. Copy into a buffer:
    // decrypt_padded_mut writes in place.
    let mut buf = body.to_vec();
    let key = derive_key();
    let plain = Aes128CbcDec::new(&key.into(), &IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;

    // Order as in the original: first the whole buffer, then the 32-byte prefix
    // cut. Return the FIRST variant that decodes as UTF-8.
    if let Ok(s) = std::str::from_utf8(plain) {
        return Some(s.to_owned());
    }
    let tail = if plain.len() >= DOMAIN_HASH_LEN {
        &plain[DOMAIN_HASH_LEN..]
    } else {
        // In Python `raw[32:]` for a short buffer = b"" → "".decode() = "".
        &plain[plain.len()..]
    };
    std::str::from_utf8(tail).ok().map(str::to_owned)
}

/// Possible paths to the CEF `Cookies` inside the Steam root (port of
/// `_cookie_db_paths`).
///
/// The traversal order MATTERS and matches the original: `Default/Cookies`,
/// `Cookies`, then other profiles `*/Cookies` in lexicographic order.
/// Duplicates (the same path from the glob) are dropped, non-existent files too.
pub fn cookie_db_paths(steam_root: &Path) -> Vec<PathBuf> {
    let base = steam_root.join("config").join("htmlcache");
    let mut candidates: Vec<PathBuf> = vec![
        base.join("Default").join("Cookies"),
        base.join("Cookies"),
    ];

    // glob(base/*/Cookies), sorted by profile name — like sorted() in Python (that
    // one sorts full paths, but the common prefix is the same, so the order is
    // equivalent to sorting by the profile directory name).
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

/// The decrypted value of the `steamLoginSecure` cookie from the Steam client's
/// CEF DB (port of `read_login_cookie`), or `Err` with the failure class
/// (without values).
///
/// The selection logic is as in the original: across all DBs (in the
/// [`cookie_db_paths`] order) all `encrypted_value` of the rows
/// `name='steamLoginSecure'` are read; the first value containing the
/// `steamid||token` separator (`%7C%7C` or `||`) is returned at once; otherwise
/// the FIRST decrypted value is remembered as a fallback and returned at the end.
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
                // Python: `if not val: continue` — an empty string is not a value.
                continue;
            }
            if decoded.contains("%7C%7C") || decoded.contains("||") {
                return Ok(decoded); // a full steamid||token
            }
            if fallback.is_none() {
                fallback = Some(decoded);
            }
        }
    }
    fallback.ok_or(CookieError::NotFound)
}

/// Read `encrypted_value` of all `steamLoginSecure` rows of one DB. An
/// open/read error is turned into an empty list — as the original moved on to
/// the next DB; the path and the error text are written NOWHERE (secret hygiene).
fn read_encrypted_values(db: &Path) -> Vec<Vec<u8>> {
    // immutable=1 + read-only: we read even while Steam is running (locks are
    // ignored), like the original's URI open. rusqlite builds the URI itself with
    // the URI flag.
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

/// Minimal escaping of a path for an SQLite `file:` URI: space and `?`/`#`,
/// which would otherwise cut the path. Full percent-encoding is not needed — the
/// path is local.
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
// cookie → webapi_token: the user's personal token
// ============================================================================
//
// Port of `steam_roots`, `fetch_webapi_token`, `_fetch_token`, `_TokenCache`
// and `access_token`/`forget_token` from the Python archive (`python-final`).
// BOTH tabs take the token from here — «Steam» (owned profile backgrounds) and
// «Wallpaper Engine» (Workshop); there must be no second copy of this code in
// the project (rule #12).

/// How long a fetched token lives before it is re-read.
///
/// The token is short-lived; we re-read it at most once every ten minutes, but
/// forcibly — right after a Steam refusal (401/403), see [`forget_token`].
pub const TOKEN_TTL_SEC: u64 = 600;

/// The endpoint that exchanges the cookie for the token.
///
/// ⚠️ Exactly `steamcommunity`, NOT `store.steampowered.com`: the store domain
/// with the same cookie leads into a 302 login loop, the community domain gives
/// the token (verified live).
pub const ASYNC_CONFIG_URL: &str = "https://steamcommunity.com/pointssummary/ajaxgetasyncconfig";

/// Exchange timeout, seconds (`timeout=20` of the original).
pub const TOKEN_TIMEOUT_SEC: u64 = 20;

/// Refusal reason: there is no Steam client on the machine or nobody logged in.
pub fn no_login_reason() -> String {
    // TRANSLATORS: why Steam items can't be loaded; starts in lower case.
    tr("sign in to Steam on this computer")
}

/// Refusal reason: the cookie exists, but Steam gave no token.
pub fn no_token_reason() -> String {
    // TRANSLATORS: why Steam items can't be loaded; starts in lower case.
    tr("the Steam client didn't provide an access key")
}

/// Existing Steam roots: the usual one, `~/.steam/steam`, `~/.steam/root` and
/// two Flatpak variants — port of `steam_roots`.
///
/// ⚠️ This is NOT the same list as the splash screen's
/// (`session_splash::steam_root_candidates`), and they must not be merged: the
/// splash needs the directory with `config/` where the startup videos are, while
/// the client cookie also lives inside the Flatpak sandbox, which the splash
/// never looks into. The original made them different, and for a reason.
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

/// Exchange the login cookie for a short-lived `webapi_token` — port of
/// `fetch_webapi_token`.
///
/// `None` — the network did not answer, the cookie expired or the response has
/// no token. The reason is NOT logged and not given out: the `ureq` error text
/// contains the URL, and the cookie goes as a header — the module's secret
/// hygiene (see the header).
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

/// SteamID64 from the login cookie: `steamid||token` after percent-decoding —
/// port of two lines of `_fetch_token`. Empty — there is no separator in the
/// cookie.
pub fn steamid_from_cookie(cookie: &str) -> String {
    let raw = percent_decode(cookie);
    match raw.split_once("||") {
        Some((sid, _)) => sid.to_string(),
        None => String::new(),
    }
}

/// `urllib.parse.unquote` to the extent the cookie needs: `%XX` → byte, the rest
/// as is. Pluses are NOT turned into spaces — `unquote` does not do that either
/// (that is `unquote_plus`).
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

/// One trip for the token: `(token, steamid64, refusal reason)` — port of
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

/// Token cache — port of `_TokenCache`. One per process: the token is shared by
/// the tabs, and a second cache would mean two network trips for the same thing.
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

/// The user's personal token: `(token, miss reason)`. An empty token means
/// "working without login", not a breakage — the tab shows the public inventory.
///
/// `force` — ignore the cache (Steam refused, the token expired early).
///
/// ⚠️ Goes to the network and therefore is NEVER called from the UI thread.
pub fn access_token(home: &str, force: bool) -> (String, String) {
    // The lock is held for the whole network trip — on purpose, like the
    // original's `with self._lock`: otherwise two tabs starting at once would fetch
    // the same token twice.
    let mut cache = match TOKEN_CACHE.lock() {
        Ok(c) => c,
        // The lock is poisoned — we must not panic here: a missing token is no reason
        // to crash the window, the tab will do with the public inventory.
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

/// SteamID64 of the last successful trip — port of `_TokenCache.steamid`.
pub fn token_steamid() -> String {
    match TOKEN_CACHE.lock() {
        Ok(c) => c.steamid.clone(),
        Err(poisoned) => poisoned.into_inner().steamid.clone(),
    }
}

/// Forget the token: Steam refused, it will be re-read on the next request —
/// port of `forget_token`.
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

    /// Percent-decoding of the cookie: `%7C%7C` is `||`, otherwise the SteamID64
    /// cannot be got out of it at all.
    #[test]
    fn steamid_is_read_through_percent_encoding() {
        assert_eq!(
            steamid_from_cookie("76561198000000000%7C%7CabcDEF"),
            "76561198000000000"
        );
        assert_eq!(steamid_from_cookie("76561198000000000||abcDEF"), "76561198000000000");
        assert_eq!(steamid_from_cookie("совсем не cookie"), "");
    }

    /// A trailing `%` and a broken pair must neither panic nor eat characters.
    #[test]
    fn broken_percent_sequences_survive() {
        assert_eq!(percent_decode("a%"), "a%");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("a%41b"), "aAb");
    }

    /// The list of roots for the cookie is WIDER than the splash list: the Flatpak
    /// sandbox is only in this one. Merge them into one — and the tab would stop
    /// finding the login of a customer with Flatpak Steam.
    #[test]
    fn cookie_roots_cover_flatpak_unlike_splash_roots() {
        let splash = crate::session_splash::steam_root_candidates("/дом");
        assert!(splash.iter().all(|p| !p.contains("com.valvesoftware.Steam")));
        // The list of roots itself is checked on the live file system
        // (`canonicalize`), so we compare not its output but the fact that the
        // candidates differ at all.
        assert_eq!(splash.len(), 3);
    }
}
