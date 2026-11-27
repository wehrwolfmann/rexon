//! Golden check of the `screenshots.rs` port against the original `rexon/screenshots.py`.
//!
//! The reference `golden/screenshots_cases.json` was captured from Python itself:
//! `_generate_svg`, the `take_screenshot` argv choice, file names and path
//! candidates, `_save_screenshot_data`, `_filename_for_de`, `SCREENSHOT_URLS`
//! and derivatives (`OFFICIAL_SCREENSHOT_URLS`, `get_online_screenshot_url(s)`)
//! were run on a battery of inputs (all DEs from `DE_COLORS` + unknown ones for
//! the fallback). Only the deterministic (pure) parts are checked — FS /
//! subprocess / network effects are not part of the port.

mod i18n_pin;

use rexon_core::screenshots as sc;
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("golden/screenshots_cases.json")).unwrap()
}

/// The screenshots directory from the reference (the real `~/.config/rexon/screenshots`).
fn dir(fx: &Value) -> String {
    fx["screenshot_dir"].as_str().unwrap().to_string()
}

#[test]
fn screenshot_dir_composition() {
    let fx = fixture();
    let d = dir(&fx);
    // Restore HOME from the reference path and check the build-up.
    let suffix = format!("/{}", sc::SCREENSHOT_SUBDIR);
    let home = d.strip_suffix(&suffix).unwrap();
    assert_eq!(sc::screenshot_dir(home), d);
}

#[test]
fn de_tables_match_python() {
    let fx = fixture();
    // DE_COLORS (order + values).
    let colors: Vec<Value> = sc::DE_COLORS
        .iter()
        .map(|(_, a, b)| json!([a, b]))
        .collect();
    let keys: Vec<&str> = sc::DE_COLORS.iter().map(|(k, _, _)| *k).collect();
    let want_colors = fx["de_colors"].as_object().unwrap();
    // key order
    let want_keys: Vec<&str> = want_colors.keys().map(String::as_str).collect();
    assert_eq!(keys, want_keys, "DE_COLORS порядок ключей");
    for (i, k) in keys.iter().enumerate() {
        assert_eq!(colors[i], want_colors[*k], "DE_COLORS[{k}]");
    }
    // DE_DISPLAY_NAMES.
    let want_names = fx["de_display_names"].as_object().unwrap();
    let disp_keys: Vec<&str> = sc::DE_DISPLAY_NAMES.iter().map(|(k, _)| *k).collect();
    let want_disp_keys: Vec<&str> = want_names.keys().map(String::as_str).collect();
    assert_eq!(disp_keys, want_disp_keys, "DE_DISPLAY_NAMES порядок");
    for (k, v) in sc::DE_DISPLAY_NAMES {
        assert_eq!(json!(v), want_names[*k], "DE_DISPLAY_NAMES[{k}]");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn generate_svg_byte_for_byte() {
    let _lang = i18n_pin::ru();
    let fx = fixture();
    let svgs = fx["svgs"].as_object().unwrap();
    // Cover ALL DEs from the reference (known + unknown fallback + empty).
    for (de, want) in svgs {
        assert_eq!(json!(sc::generate_svg(de)), *want, "generate_svg({de:?})");
    }
    // Explicit check that all DEs from DE_COLORS are present.
    for (k, _, _) in sc::DE_COLORS {
        assert!(svgs.contains_key(*k), "нет golden-SVG для {k}");
    }
}

#[test]
fn file_names_match_python() {
    let fx = fixture();
    let names = fx["names"].as_object().unwrap();
    for (de, want) in names {
        assert_eq!(json!(sc::auto_name(de)), want["auto"], "auto_name({de})");
        assert_eq!(json!(sc::custom_name(de)), want["custom"], "custom_name({de})");
        assert_eq!(
            json!(sc::placeholder_name(de)),
            want["placeholder"],
            "placeholder_name({de})"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn screenshot_tools_argv_match_python() {
    let _lang = i18n_pin::ru();
    let fx = fixture();
    let d = dir(&fx);
    let want = fx["tool_argv"].as_object().unwrap();
    for (de, argv) in want {
        assert_eq!(
            serde_json::to_value(sc::screenshot_tools(&d, de)).unwrap(),
            *argv,
            "screenshot_tools({de})"
        );
    }
    // select_screenshot_tool: priority order grim → … → import
    // (Wayland tools first: on sway/Hyprland/niri grim is the only working one,
    // X11 utilities capture a black frame there).
    // Nothing available → None.
    assert_eq!(sc::select_screenshot_tool(&d, "kde", |_| false), None);
    // Only maim (5th) is available — it is chosen.
    let sel = sc::select_screenshot_tool(&d, "kde", |c| c == "maim").unwrap();
    assert_eq!(sel[0], "maim");
    assert_eq!(sel, want["kde"][4].as_array().unwrap().iter()
        .map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>());
    // scrot and spectacle are available → the first in order is taken (spectacle).
    let sel2 = sc::select_screenshot_tool(&d, "kde", |c| c == "scrot" || c == "spectacle").unwrap();
    assert_eq!(sel2[0], "spectacle");
    // On a bare Wayland compositor only grim is available — it is chosen.
    let sel3 = sc::select_screenshot_tool(&d, "kde", |c| c == "grim").unwrap();
    assert_eq!(sel3[0], "grim");
    // The message about missing tools.
    assert_eq!(json!(sc::no_tools_msg()), fx["no_tools_msg"]);
}

#[test]
fn path_candidates_match_python() {
    let fx = fixture();
    let d = dir(&fx);
    for (de, want) in fx["path_candidates"].as_object().unwrap() {
        assert_eq!(
            serde_json::to_value(sc::screenshot_path_candidates(&d, de)).unwrap(),
            *want,
            "screenshot_path_candidates({de})"
        );
    }
    for (de, want) in fx["extra_candidates"].as_object().unwrap() {
        assert_eq!(
            serde_json::to_value(sc::extra_candidates(&d, de)).unwrap(),
            *want,
            "extra_candidates({de})"
        );
    }
    // delete_screenshot uses the same candidate list.
    for (de, want) in fx["delete_candidates"].as_object().unwrap() {
        assert_eq!(
            serde_json::to_value(sc::screenshot_path_candidates(&d, de)).unwrap(),
            *want,
            "delete_candidates({de})"
        );
    }
    assert_eq!(json!(sc::MIN_SIZE), json!(100));
}

#[test]
fn save_ext_and_dest_match_python() {
    let fx = fixture();
    let d = dir(&fx);
    for (ct_key, want) in fx["save_cases"].as_object().unwrap() {
        // The "None" key in the reference = Python None.
        let ct: Option<&str> = if ct_key == "None" { None } else { Some(ct_key.as_str()) };
        assert_eq!(json!(sc::save_ext(ct)), want["ext"], "save_ext({ct_key})");
        assert_eq!(
            json!(sc::save_dest(&d, "kde", ct)),
            want["dest"],
            "save_dest({ct_key})"
        );
    }
}

#[test]
fn filename_for_de_match_python() {
    let fx = fixture();
    for (de, want) in fx["filename_for_de"].as_object().unwrap() {
        assert_eq!(
            json!(sc::filename_for_de(de)),
            *want,
            "filename_for_de({de})"
        );
    }
}

#[test]
fn screenshot_urls_and_derivatives_match_python() {
    let fx = fixture();
    // SCREENSHOT_URLS in full.
    let want_urls = fx["screenshot_urls"].as_object().unwrap();
    let keys: Vec<&str> = sc::SCREENSHOT_URLS.iter().map(|(k, _)| *k).collect();
    let want_keys: Vec<&str> = want_urls.keys().map(String::as_str).collect();
    assert_eq!(keys, want_keys, "SCREENSHOT_URLS порядок ключей");
    for (k, v) in sc::SCREENSHOT_URLS {
        assert_eq!(json!(v), want_urls[*k], "SCREENSHOT_URLS[{k}]");
    }
    // OFFICIAL_SCREENSHOT_URLS (a dict in Python).
    let official: serde_json::Map<String, Value> = sc::official_screenshot_urls()
        .into_iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    assert_eq!(Value::Object(official), fx["official_screenshot_urls"]);
    // get_online_screenshot_url / _urls.
    for (de, want) in fx["online_url"].as_object().unwrap() {
        assert_eq!(json!(sc::online_url(de)), *want, "online_url({de})");
    }
    for (de, want) in fx["online_urls"].as_object().unwrap() {
        assert_eq!(json!(sc::online_urls(de)), *want, "online_urls({de})");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Closing parity holes: parsing the Wikimedia API response, accepting the
//  downloaded response and the auto-download plan. References were captured
//  from the live screenshots.py on the Python path before its removal; the
//  Wikimedia responses are LIVE, captured with one request
//  (`golden/screenshots_wikimedia_*_raw.json`).
// ═══════════════════════════════════════════════════════════════════════════

/// The imageinfo request URL matches the original's f-string, including
/// percent-encoding of the file name (spaces, `!`, `#`, Cyrillic).
#[test]
fn wikimedia_api_url_matches_python() {
    for (name, want) in fixture()["wikimedia_api_url"].as_object().unwrap() {
        assert_eq!(
            &sc::wikimedia_api_url(name),
            want.as_str().unwrap(),
            "api_url для {name}"
        );
    }
}

/// Parsing a LIVE imageinfo response gives the same CDN URL as Python: the
/// `thumburl` thumbnail beats the original `url`.
#[test]
fn wikimedia_live_response_matches_python() {
    let live: Value =
        serde_json::from_str(include_str!("golden/screenshots_wikimedia_api_raw.json")).unwrap();
    let got = sc::parse_wikimedia_imageinfo(&live);
    let want = fixture()["imageinfo"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "live_ok")
        .expect("кейс live_ok")["expected"]
        .as_str()
        .map(str::to_string);
    assert_eq!(got, want);
    let url = got.expect("живой ответ содержит ссылку");
    assert!(url.starts_with("https://upload.wikimedia.org/"), "{url}");
    assert!(url.contains("KDE_neon_6_Breeze_Desktop_English"), "{url}");
}

/// Malformed imageinfo inputs — including a LIVE response about a missing file
/// (`missing: true`), an empty `imageinfo` (the original crashed with
/// IndexError), the formatversion=1 schema (`pages` is an object, the original
/// crashed with KeyError), a non-object instead of a response. Not a single
/// divergence from the fixed Python, not a single panic.
#[test]
fn wikimedia_broken_responses_match_python() {
    let cases = fixture();
    let cases = cases["imageinfo"].as_array().unwrap();
    assert!(cases.len() >= 12, "батарея кейсов не должна усохнуть");
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let got = sc::parse_wikimedia_imageinfo(&c["data"]);
        let want = c["expected"].as_str().map(str::to_string);
        assert_eq!(got, want, "imageinfo [{name}]");
    }
}

/// HTML instead of JSON: the core receives already parsed JSON, so we check
/// the second line — a JSON non-object (and that real HTML does not turn into JSON).
#[test]
fn wikimedia_html_instead_of_json() {
    let html = include_str!("golden/screenshots_wikimedia_html_raw.html");
    assert!(html.starts_with("<!DOCTYPE html>"), "фикстура — живой HTML");
    assert!(
        serde_json::from_str::<Value>(html).is_err(),
        "HTML не должен разбираться как JSON — на Python это RuntimeError в _http_json"
    );
    for raw in ["[]", "\"<html>\"", "null", "0"] {
        let v: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(sc::parse_wikimedia_imageinfo(&v), None, "не-объект {raw}");
    }
}

/// Accepting the downloaded response: `Content-Type` and minimal size — the
/// same decisions and THE SAME reason TEXTS as in Python (the user sees them).
/// The battery has a live case «an HTML page instead of an image».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn validate_image_response_matches_python() {
    let _lang = i18n_pin::ru();
    for c in fixture()["validate_image"].as_array().unwrap() {
        let ct = c["content_type"].as_str().unwrap();
        let len = c["len"].as_u64().unwrap() as usize;
        let got = sc::validate_image_response(Some(ct), len);
        match c["error"].as_str() {
            Some(msg) => assert_eq!(got, Err(msg.to_string()), "ct={ct} len={len}"),
            None => assert_eq!(got, Ok(()), "ct={ct} len={len}"),
        }
    }
    // None is treated as an empty string (Python: `content_type or ""`).
    assert_eq!(
        sc::validate_image_response(None, 4096),
        Err("Не изображение: ".to_string())
    );
}

/// Pause before a retry on HTTP 429.
#[test]
fn retry_wait_matches_python() {
    for (attempt, want) in fixture()["retry_wait"].as_object().unwrap() {
        let a: u32 = attempt.parse().unwrap();
        assert_eq!(sc::retry_wait_secs(a), want.as_u64().unwrap(), "attempt {a}");
    }
}

/// Auto-download file tag: the first URL has no suffix, then `_1`, `_2`…
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn auto_download_tag_matches_python() {
    let _lang = i18n_pin::ru();
    for (key, want) in fixture()["auto_download_tags"].as_object().unwrap() {
        let idx: usize = key.split(':').nth(1).unwrap().parse().unwrap();
        assert_eq!(
            sc::auto_download_tag("kde", idx),
            want.as_str().unwrap(),
            "tag kde:{idx}"
        );
    }
}

/// The auto-download plan matches what the live `auto_download_all` actually
/// queued on a prepared directory (some DEs fully covered, some partially, one
/// has a file below 100 bytes that does not count).
///
/// The task order in the original is defined by the thread pool
/// (non-deterministic), so it is compared as a set; separately we check that
/// the core plan follows the `SCREENSHOT_URLS` order.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn auto_download_plan_matches_python() {
    let _lang = i18n_pin::ru();
    let fx = fixture();
    let p = &fx["auto_download_plan"];
    let counts = p["existing_counts"].as_object().unwrap();
    let exists = p["tag_exists"].as_object().unwrap();

    let plan = sc::auto_download_plan(
        |de| counts.get(de).and_then(Value::as_u64).unwrap_or(0) as usize,
        |tag| exists.get(tag).and_then(Value::as_bool).unwrap_or(false),
    );

    let mut got: Vec<(String, String)> = plan
        .tasks
        .iter()
        .map(|t| (t.tag.clone(), t.url.clone()))
        .collect();
    let mut want: Vec<(String, String)> = p["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t[0].as_str().unwrap().to_string(),
                t[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    got.sort();
    want.sort();
    assert_eq!(got, want, "набор задач автоскачивания");
    assert!(!want.is_empty(), "план не должен быть пустым");

    // «Already present (N pcs.)» — the same DEs and the same numbers.
    let mut got_done: Vec<String> = plan
        .already_complete
        .iter()
        .map(|(de, n)| format!("{de}|уже есть ({n} шт.)"))
        .collect();
    let mut want_done: Vec<String> = p["already"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| format!("{}|{}", d[0].as_str().unwrap(), d[1].as_str().unwrap()))
        .collect();
    got_done.sort();
    want_done.sort();
    assert_eq!(got_done, want_done, "DE, у которых всё скачано");

    // The core plan order is the SCREENSHOT_URLS order (stable output).
    let de_order: Vec<&str> = sc::SCREENSHOT_URLS.iter().map(|(k, _)| *k).collect();
    let mut last = 0usize;
    for t in &plan.tasks {
        let pos = de_order.iter().position(|d| *d == t.de).unwrap();
        assert!(pos >= last, "план вышел из порядка SCREENSHOT_URLS: {}", t.de);
        last = pos;
    }
}

/// Tag path candidates — 3 extensions in the same order as the screenshot lookup.
#[test]
fn tag_candidates_cover_all_exts() {
    assert_eq!(
        sc::tag_candidates("/d", "kde_1"),
        vec!["/d/kde_1.png", "/d/kde_1.jpg", "/d/kde_1.webp"]
    );
    assert_eq!(sc::tag_candidates("/d", "kde").len(), sc::PATH_EXTS.len());
}

// ============================================================================
// Downloading a screenshot in full (`download_screenshot_from_url`)
// ============================================================================
//
// These five scenarios used to live in the bridge roundtrip test, where the
// network was replaced by monkeypatching `urllib.request.urlopen` — because
// Python itself did the HTTP. The download flow moved into the core (shared
// retries, `net.rs`), so the check lives here too: the same set of cases, the
// same LIVE Wikimedia responses from `golden/`.

use rexon_core::net::{LogLevel, NetError, NetIo};

/// A prepared response for a URL: content type and body, or a refusal code.
enum Answer {
    Body(&'static str, Vec<u8>),
    Status(u16),
}

/// A scripted network: answers by URL, counts requests, does not wait out pauses.
struct ScriptedNet {
    urls: Vec<String>,
    open: Option<(Vec<u8>, usize)>,
    ctype: String,
    slept: Vec<u64>,
    logs: Vec<(LogLevel, String)>,
}

fn png() -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend(std::iter::repeat_n(b'z', 900));
    v
}

fn html() -> Vec<u8> {
    b"<!DOCTYPE html><html><body>oops</body></html>".repeat(20)
}

/// Exactly the same decision tree as the replaced `urlopen` in the former
/// roundtrip test: the direct address by scenario name, the Commons API by
/// file name, the CDN address from the live response.
fn answer(url: &str) -> Answer {
    if let Some(de) = url.strip_prefix("direct://") {
        return match de {
            "kde" => Answer::Body("image/png", png()),
            "mate" => Answer::Body("image/png", b"tiny".to_vec()),
            "budgie" => Answer::Status(403),
            _ => Answer::Body("text/html; charset=UTF-8", html()),
        };
    }
    if url.contains("api.php") {
        let body: &[u8] = if url.contains("Fedora_Workstation") {
            include_bytes!("golden/screenshots_wikimedia_api_raw.json")
        } else if url.contains("Xfce") {
            include_bytes!("golden/screenshots_wikimedia_missing_raw.json")
        } else {
            br#"["not", "an", "object"]"#
        };
        return Answer::Body("application/json", body.to_vec());
    }
    if url.starts_with("https://upload.wikimedia.org/") {
        return Answer::Body("image/png", png());
    }
    panic!("неожиданный URL в сверке: {url}");
}

impl ScriptedNet {
    fn new() -> Self {
        ScriptedNet {
            urls: Vec::new(),
            open: None,
            ctype: String::new(),
            slept: Vec::new(),
            logs: Vec::new(),
        }
    }
}

impl NetIo for ScriptedNet {
    fn get(&mut self, url: &str, _: &str, _: u64) -> Result<Vec<u8>, NetError> {
        self.urls.push(url.to_string());
        match answer(url) {
            Answer::Body(_, b) => Ok(b),
            Answer::Status(code) => Err(NetError::Status { code, reason: String::new() }),
        }
    }

    fn open(&mut self, url: &str, _: &str, _: u64) -> Result<i64, NetError> {
        self.urls.push(url.to_string());
        match answer(url) {
            Answer::Body(ct, b) => {
                let n = b.len() as i64;
                self.ctype = ct.to_string();
                self.open = Some((b, 0));
                Ok(n)
            }
            Answer::Status(code) => Err(NetError::Status { code, reason: String::new() }),
        }
    }

    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        let Some((buf, pos)) = self.open.as_mut() else {
            return Ok(Vec::new());
        };
        let end = (*pos + n).min(buf.len());
        let out = buf[*pos..end].to_vec();
        *pos = end;
        Ok(out)
    }

    fn close(&mut self) {
        self.open = None;
        self.ctype.clear();
    }

    fn makedirs(&mut self, _: &str) {}
    fn create(&mut self, _: &str) -> Result<(), NetError> {
        unreachable!("снимок пишет на диск Python, а не ядро")
    }
    fn write(&mut self, _: &[u8]) -> Result<(), NetError> {
        unreachable!()
    }
    fn close_file(&mut self) {}
    fn exists(&mut self, _: &str) -> bool {
        false
    }
    fn remove(&mut self, _: &str) {}
    fn progress(&mut self, _: i64, _: i64) {}
    fn log(&mut self, level: LogLevel, message: &str) {
        self.logs.push((level, message.to_string()));
    }
    fn sleep(&mut self, ms: u64) {
        self.slept.push(ms);
    }
    fn content_type(&mut self) -> String {
        self.ctype.clone()
    }
}

/// The scenario outcome in the same form the former roundtrip recorded it:
/// either `Ok(Content-Type)` or the refusal reason text.
fn run(de: &str) -> (Result<String, String>, ScriptedNet) {
    let mut io = ScriptedNet::new();
    let got = sc::download_screenshot(&mut io, de, &format!("direct://{de}"));
    (got.map(|(_, ct)| ct), io)
}

/// The direct address returns the image right away — the API is not touched at all.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn download_direct_image() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("kde");
    assert_eq!(got, Ok("image/png".to_string()));
    assert_eq!(io.urls, vec!["direct://kde"], "лишних запросов быть не должно");
    assert!(io.slept.is_empty(), "успех с первой попытки не ждёт");
}

/// The direct address returns an HTML page → we resolve the real address via
/// the Commons API (LIVE response) → the image arrives from the CDN.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn download_falls_back_to_commons_api() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("gnome");
    assert_eq!(got, Ok("image/png".to_string()));
    assert_eq!(io.urls.len(), 3, "прямой, API, CDN");
    assert!(io.urls[1].contains("api.php"));
    assert!(io.urls[2].starts_with("https://upload.wikimedia.org/"));
    // «Not an image» is not a network failure: there is nothing to retry.
    assert!(io.slept.is_empty(), "HTML вместо картинки повторять незачем");
}

/// The API answers «no such file on Commons» — the buyer gets the reason of
/// the FIRST failure, not details about the API.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn download_reports_first_failure_when_commons_has_no_file() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("xfce");
    assert_eq!(
        got,
        Err("Не удалось загрузить скриншот: Не изображение: text/html; charset=UTF-8"
            .to_string())
    );
    assert_eq!(io.urls.len(), 2, "прямой и API, CDN-адреса нет");
}

/// The image arrived, but is too small — the same text as in the original.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn download_rejects_too_small_image() {
    let _lang = i18n_pin::ru();
    let (got, _) = run("mate");
    assert_eq!(got, Err("Не удалось загрузить скриншот: Файл слишком мал".to_string()));
}

/// 403 on the direct address is not retried (access denied — the second time
/// will be the same), and the API response turned out not to be an object →
/// refusal with the reason of the first failure.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn download_forbidden_is_not_retried() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("budgie");
    assert_eq!(got, Err("Не удалось загрузить скриншот: HTTP 403".to_string()));
    assert_eq!(io.urls.len(), 2);
    assert!(io.slept.is_empty(), "403 повторять бессмысленно");
}

// ============================================================================
// Interface language
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Screenshot labels and the unrecognised desktop in English — without
/// Russian letters; after an English preposition the unrecognised desktop is
/// «your desktop».
#[test]
fn english_screenshots_have_no_russian() {
    let _lang = i18n_pin::en();
    let texts = [
        sc::PlaceholderState::Loading.caption(),
        sc::PlaceholderState::NoInternet.caption(),
        sc::PlaceholderState::NoScreenshot.caption(),
        sc::no_tools_msg(),
        sc::rate_limit_msg(),
        sc::unknown_de_name(),
        sc::display_name("unknown"),
        sc::display_name(""),
    ];
    for t in &texts {
        assert!(!has_cyrillic(t), "{t}");
    }
    assert_eq!(sc::PlaceholderState::Loading.caption(), "Loading screenshot…");
    assert_eq!(sc::display_name(""), "YOUR DESKTOP");
    assert!(sc::is_unknown_de_display("YOUR DESKTOP"));
    let unknown = sc::display_name("unknown");
    assert_eq!(
        sc::place_de_name("Restored 2 apps for “%1”", "%1", &unknown),
        "Restored 2 apps for your desktop"
    );
    assert_eq!(
        sc::place_de_name("Wallpapers on %1 need a background program.", "%1", &unknown),
        "Wallpapers on your desktop need a background program."
    );
    assert_eq!(
        sc::place_de_name("Current desktop: <b>%1</b>", "%1", &unknown),
        "Current desktop: <b>YOUR DESKTOP</b>"
    );
    assert_eq!(sc::place_de_name("Apps for %1", "%1", "KDE Plasma"), "Apps for KDE Plasma");
}

/// The same labels in Russian — word for word as before moving to the catalog
/// (snapshot `ca42e74^`), the cases of the unrecognised desktop are the former ones.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn russian_screenshot_texts_as_before() {
    let _lang = i18n_pin::ru();
    assert_eq!(sc::PlaceholderState::Loading.caption(), "Скриншот загружается…");
    assert_eq!(
        sc::PlaceholderState::NoInternet.caption(),
        "Скриншот появится, когда будет интернет"
    );
    assert_eq!(
        sc::PlaceholderState::NoScreenshot.caption(),
        "Скриншота этого рабочего стола пока нет"
    );
    assert_eq!(sc::rate_limit_msg(), "Превышен лимит запросов (429)");
    let unknown = sc::display_name("unknown");
    assert_eq!(unknown, "ВАШ РАБОЧИЙ СТОЛ");
    assert_eq!(
        sc::place_de_name("Для окружения «{0}»", "{0}", &unknown),
        "Для вашего рабочего стола"
    );
    assert_eq!(
        sc::place_de_name("Для обоев на {de} нужна программа фона.", "{de}", &unknown),
        "Для обоев на вашем рабочем столе нужна программа фона."
    );
}
