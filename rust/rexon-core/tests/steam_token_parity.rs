//! Побайтовая сверка добычи cookie `steamLoginSecure` с оригиналом
//! Python-кода (`_decrypt_v10` + `read_login_cookie`; архив, метка git `python-final`).
//!
//! Эталоны в `golden/steam_token_cases.json` сняты с ЖИВОГО Python-оракула:
//! известные значения зашифрованы тем же ключом схемы v10 (AES-128-CBC,
//! PBKDF2-HMAC-SHA1 pw="peanuts") и ПРОГНАНЫ через настоящий `_decrypt_v10` —
//! поле `expect` есть его вывод. sqlite-сценарии описывают фикстуру (какие БД
//! с какими строками), а `expect` — вывод `read_login_cookie` оригинала на ней;
//! этот тест собирает те же БД через rusqlite и сверяет результат Rust.
//!
//! ⚠️ Секреты: все токены в эталоне — СИНТЕТИЧЕСКИЕ (сконструированы для теста),
//! не реальные значения пользователя.

use std::path::Path;

use rexon_core::steam_token::{cookie_db_paths, decrypt_v10, read_login_cookie, CookieError};
use serde_json::Value;

const CASES: &str = include_str!("golden/steam_token_cases.json");

fn cases() -> Value {
    serde_json::from_str(CASES).expect("эталон разобран")
}

// ── маленький декодер base64 (стандартный алфавит, с паддингом) ────────
fn b64_decode(s: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Ожидаемое значение эталона: JSON-строка → Some, null → None.
fn expect_str(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => panic!("неожиданный тип expect: {other:?}"),
    }
}

#[test]
fn decrypt_v10_byte_parity() {
    let data = cases();
    let vectors = data["vectors"].as_array().expect("массив векторов");
    assert!(!vectors.is_empty(), "векторы не пусты");
    for vec in vectors {
        let name = vec["name"].as_str().unwrap();
        let enc = b64_decode(vec["enc_b64"].as_str().unwrap());
        let expect = expect_str(&vec["expect"]);
        let got = decrypt_v10(&enc);
        assert_eq!(got, expect, "decrypt_v10 разошёлся с Python на векторе '{name}'");
    }
}

/// Построить фикстуру CEF-БД из описания сценария и сверить `read_login_cookie`.
#[test]
fn read_login_cookie_selection_parity() {
    let data = cases();
    let scenarios = data["sqlite_scenarios"].as_array().expect("массив сценариев");
    assert!(!scenarios.is_empty(), "сценарии не пусты");
    for scen in scenarios {
        let name = scen["name"].as_str().unwrap();
        let tmp = tempdir(name);
        let root = tmp.path();

        if let Some(dbs) = scen["dbs"].as_object() {
            for (rel, rows) in dbs {
                let db_path = root.join(rel);
                std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
                build_cookie_db(&db_path, rows.as_array().unwrap());
            }
        }

        let expect = expect_str(&scen["expect"]);
        let got = read_login_cookie(root).ok();
        assert_eq!(got, expect, "read_login_cookie разошёлся с Python в сценарии '{name}'");
    }
}

/// Собрать SQLite-БД `cookies(name, encrypted_value)` из строк эталона.
fn build_cookie_db(path: &Path, rows: &[Value]) {
    let conn = rusqlite::Connection::open(path).expect("создать БД");
    conn.execute("CREATE TABLE cookies (name TEXT, encrypted_value BLOB)", [])
        .expect("создать таблицу");
    for row in rows {
        let nm = row["name"].as_str().unwrap();
        let enc: Option<Vec<u8>> = match &row["enc_b64"] {
            Value::Null => None,
            Value::String(s) => Some(b64_decode(s)),
            _ => panic!("enc_b64: строка или null"),
        };
        conn.execute(
            "INSERT INTO cookies(name, encrypted_value) VALUES (?1, ?2)",
            rusqlite::params![nm, enc],
        )
        .expect("вставить строку");
    }
}

// ── прочие свойства, не требующие Python-оракула ──────────────────────

#[test]
fn cookie_db_paths_order_and_dedup() {
    let tmp = tempdir("paths");
    let base = tmp.path().join("config").join("htmlcache");
    // Default, htmlcache/Cookies и два профиля из glob — проверяем порядок.
    for rel in ["Default", "Zebra", "Alpha"] {
        std::fs::create_dir_all(base.join(rel)).unwrap();
        std::fs::write(base.join(rel).join("Cookies"), b"x").unwrap();
    }
    std::fs::write(base.join("Cookies"), b"x").unwrap();

    let paths = cookie_db_paths(tmp.path());
    let names: Vec<String> = paths
        .iter()
        .map(|p| {
            let parent = p.parent().unwrap().file_name().unwrap();
            parent.to_string_lossy().into_owned()
        })
        .collect();
    // Default, затем htmlcache/Cookies (родитель = htmlcache), затем glob по
    // алфавиту: Alpha, Default(дубль отброшен), Zebra.
    assert_eq!(names, vec!["Default", "htmlcache", "Alpha", "Zebra"]);
}

#[test]
fn missing_root_is_no_database() {
    let tmp = tempdir("empty");
    assert_eq!(read_login_cookie(tmp.path()), Err(CookieError::NoDatabase));
}

#[test]
fn errors_carry_no_values() {
    // Сообщение об ошибке не несёт значений (секрет-гигиена): у enum вообще нет
    // полей, а Debug печатает только имя варианта.
    assert_eq!(format!("{:?}", CookieError::NoDatabase), "NoDatabase");
    assert_eq!(format!("{:?}", CookieError::NotFound), "NotFound");
}

// ── минимальный tempdir без внешних крейтов ───────────────────────────
struct TempDir {
    path: std::path::PathBuf,
}
impl TempDir {
    fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn tempdir(tag: &str) -> TempDir {
    let mut path = std::env::temp_dir();
    let uniq = format!(
        "rexon-steam-token-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    path.push(uniq);
    std::fs::create_dir_all(&path).expect("создать temp");
    TempDir { path }
}
