//! Golden-сверка порта `screenshots.rs` с оригиналом `rexon/screenshots.py`.
//!
//! Эталон `golden/screenshots_cases.json` снят с самого Python:
//! `_generate_svg`, выбор argv `take_screenshot`, имена файлов и кандидаты
//! путей, `_save_screenshot_data`, `_filename_for_de`, `SCREENSHOT_URLS` и
//! производные (`OFFICIAL_SCREENSHOT_URLS`, `get_online_screenshot_url(s)`)
//! прогнаны на батарее входов (все DE из `DE_COLORS` + неизвестные для
//! fallback). Проверяются только детерминированные (чистые) части — эффекты
//! ФС/подпроцессов/сети в порт не входят.

mod i18n_pin;

use rexon_core::screenshots as sc;
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("golden/screenshots_cases.json")).unwrap()
}

/// Каталог скриншотов из эталона (реальный `~/.config/rexon/screenshots`).
fn dir(fx: &Value) -> String {
    fx["screenshot_dir"].as_str().unwrap().to_string()
}

#[test]
fn screenshot_dir_composition() {
    let fx = fixture();
    let d = dir(&fx);
    // Восстанавливаем HOME из эталонного пути и проверяем сборку.
    let suffix = format!("/{}", sc::SCREENSHOT_SUBDIR);
    let home = d.strip_suffix(&suffix).unwrap();
    assert_eq!(sc::screenshot_dir(home), d);
}

#[test]
fn de_tables_match_python() {
    let fx = fixture();
    // DE_COLORS (порядок + значения).
    let colors: Vec<Value> = sc::DE_COLORS
        .iter()
        .map(|(_, a, b)| json!([a, b]))
        .collect();
    let keys: Vec<&str> = sc::DE_COLORS.iter().map(|(k, _, _)| *k).collect();
    let want_colors = fx["de_colors"].as_object().unwrap();
    // порядок ключей
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn generate_svg_byte_for_byte() {
    let _lang = i18n_pin::ru();
    let fx = fixture();
    let svgs = fx["svgs"].as_object().unwrap();
    // Покрываем ВСЕ DE из эталона (известные + неизвестные fallback + пустой).
    for (de, want) in svgs {
        assert_eq!(json!(sc::generate_svg(de)), *want, "generate_svg({de:?})");
    }
    // Явная проверка, что все DE из DE_COLORS присутствуют.
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
    // select_screenshot_tool: порядок приоритета grim → … → import
    // (Wayland-инструменты первыми: на sway/Hyprland/niri grim
    // единственный рабочий, X11-утилиты там снимают чёрный кадр).
    // Ничего недоступно → None.
    assert_eq!(sc::select_screenshot_tool(&d, "kde", |_| false), None);
    // Доступен только maim (5-й) — он и выбирается.
    let sel = sc::select_screenshot_tool(&d, "kde", |c| c == "maim").unwrap();
    assert_eq!(sel[0], "maim");
    assert_eq!(sel, want["kde"][4].as_array().unwrap().iter()
        .map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>());
    // Доступны scrot и spectacle → берётся первый по порядку (spectacle).
    let sel2 = sc::select_screenshot_tool(&d, "kde", |c| c == "scrot" || c == "spectacle").unwrap();
    assert_eq!(sel2[0], "spectacle");
    // На голом Wayland-композиторе доступен только grim — он и выбирается.
    let sel3 = sc::select_screenshot_tool(&d, "kde", |c| c == "grim").unwrap();
    assert_eq!(sel3[0], "grim");
    // Сообщение об отсутствии инструментов.
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
    // delete_screenshot использует тот же список кандидатов.
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
        // Ключ "None" в эталоне = Python None.
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
    // SCREENSHOT_URLS целиком.
    let want_urls = fx["screenshot_urls"].as_object().unwrap();
    let keys: Vec<&str> = sc::SCREENSHOT_URLS.iter().map(|(k, _)| *k).collect();
    let want_keys: Vec<&str> = want_urls.keys().map(String::as_str).collect();
    assert_eq!(keys, want_keys, "SCREENSHOT_URLS порядок ключей");
    for (k, v) in sc::SCREENSHOT_URLS {
        assert_eq!(json!(v), want_urls[*k], "SCREENSHOT_URLS[{k}]");
    }
    // OFFICIAL_SCREENSHOT_URLS (dict в Python).
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
//  Закрытие дыр паритета: разбор ответа Wikimedia API, приёмка скачанного
//  ответа и план автоскачивания. Эталоны сняты с живого screenshots.py при
//  Python-путь до его удаления; ответы Wikimedia — ЖИВЫЕ, снятые одним запросом
//  (`golden/screenshots_wikimedia_*_raw.json`).
// ═══════════════════════════════════════════════════════════════════════════

/// URL запроса imageinfo совпадает с f-string оригинала, включая percent-encode
/// имени файла (пробелы, `!`, `#`, кириллица).
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

/// Разбор ЖИВОГО ответа imageinfo даёт тот же CDN-URL, что Python: миниатюра
/// `thumburl` приоритетнее оригинала `url`.
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

/// Кривые входы разбора imageinfo — включая ЖИВОЙ ответ про отсутствующий файл
/// (`missing: true`), пустой `imageinfo` (оригинал ронял IndexError), схему
/// formatversion=1 (`pages` — объект, оригинал ронял KeyError), не-объект
/// вместо ответа. Ни одного расхождения с починенным Python, ни одной паники.
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

/// HTML вместо JSON: до ядра доходит уже разобранный JSON, поэтому проверяем
/// вторую линию — JSON-не-объект (и что настоящий HTML в JSON не превращается).
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

/// Приёмка скачанного ответа: `Content-Type` и минимальный размер — те же
/// решения и ТЕ ЖЕ ТЕКСТЫ причин, что у Python (они видны пользователю).
/// В батарее есть живой случай «HTML-страница вместо картинки».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
    // None трактуется как пустая строка (Python: `content_type or ""`).
    assert_eq!(
        sc::validate_image_response(None, 4096),
        Err("Не изображение: ".to_string())
    );
}

/// Пауза перед повтором при HTTP 429.
#[test]
fn retry_wait_matches_python() {
    for (attempt, want) in fixture()["retry_wait"].as_object().unwrap() {
        let a: u32 = attempt.parse().unwrap();
        assert_eq!(sc::retry_wait_secs(a), want.as_u64().unwrap(), "attempt {a}");
    }
}

/// Тег файла автоскачивания: первый URL — без суффикса, дальше `_1`, `_2`…
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// План автоскачивания совпадает с тем, что реально поставил в очередь живой
/// `auto_download_all` на подготовленном каталоге (часть DE закрыта полностью,
/// часть частично, у одного файл меньше 100 байт и не считается).
///
/// Порядок задач в оригинале определяет пул потоков (недетерминирован), поэтому
/// сверяется как множество; отдельно проверяется, что план ядра идёт в порядке
/// `SCREENSHOT_URLS`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

    // «Уже есть (N шт.)» — те же DE и те же числа.
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

    // Порядок плана ядра — порядок SCREENSHOT_URLS (стабильный вывод).
    let de_order: Vec<&str> = sc::SCREENSHOT_URLS.iter().map(|(k, _)| *k).collect();
    let mut last = 0usize;
    for t in &plan.tasks {
        let pos = de_order.iter().position(|d| *d == t.de).unwrap();
        assert!(pos >= last, "план вышел из порядка SCREENSHOT_URLS: {}", t.de);
        last = pos;
    }
}

/// Кандидаты путей тега — 3 расширения в том же порядке, что у поиска снимка.
#[test]
fn tag_candidates_cover_all_exts() {
    assert_eq!(
        sc::tag_candidates("/d", "kde_1"),
        vec!["/d/kde_1.png", "/d/kde_1.jpg", "/d/kde_1.webp"]
    );
    assert_eq!(sc::tag_candidates("/d", "kde").len(), sc::PATH_EXTS.len());
}

// ============================================================================
// Скачивание снимка целиком (`download_screenshot_from_url`)
// ============================================================================
//
// Раньше эти пять сценариев жили в roundtrip-тесте моста, где сеть подменялась
// монипатчем `urllib.request.urlopen` — потому что сам HTTP делал Python. Ход
// скачивания переехал в ядро (общие повторы, `net.rs`), значит и сверка ходит
// здесь: тот же набор случаев, те же ЖИВЫЕ ответы Wikimedia из `golden/`.

use rexon_core::net::{LogLevel, NetError, NetIo};

/// Заготовленный ответ на URL: тип содержимого и тело, либо код отказа.
enum Answer {
    Body(&'static str, Vec<u8>),
    Status(u16),
}

/// Сеть по сценарию: отвечает по URL, считает запросы, паузы не выжидает.
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

/// Ровно то же дерево решений, что у подменённого `urlopen` в прежнем
/// roundtrip-тесте: прямой адрес по имени сценария, API Commons по имени
/// файла, CDN-адрес из живого ответа.
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

/// Итог сценария в том же виде, в каком его записывал прежний roundtrip:
/// либо `Ok(Content-Type)`, либо текст причины отказа.
fn run(de: &str) -> (Result<String, String>, ScriptedNet) {
    let mut io = ScriptedNet::new();
    let got = sc::download_screenshot(&mut io, de, &format!("direct://{de}"));
    (got.map(|(_, ct)| ct), io)
}

/// Прямой адрес сразу отдаёт картинку — API не трогаем вовсе.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn download_direct_image() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("kde");
    assert_eq!(got, Ok("image/png".to_string()));
    assert_eq!(io.urls, vec!["direct://kde"], "лишних запросов быть не должно");
    assert!(io.slept.is_empty(), "успех с первой попытки не ждёт");
}

/// Прямой адрес отдаёт HTML-страницу → разрешаем настоящий адрес через API
/// Commons (ЖИВОЙ ответ) → картинка приезжает с CDN.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn download_falls_back_to_commons_api() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("gnome");
    assert_eq!(got, Ok("image/png".to_string()));
    assert_eq!(io.urls.len(), 3, "прямой, API, CDN");
    assert!(io.urls[1].contains("api.php"));
    assert!(io.urls[2].starts_with("https://upload.wikimedia.org/"));
    // «Не изображение» — не сетевой сбой: повторять его нечего.
    assert!(io.slept.is_empty(), "HTML вместо картинки повторять незачем");
}

/// API отвечает «файла на Commons нет» — до покупателя доезжает причина
/// ПЕРВОЙ неудачи, а не подробности про API.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Картинка пришла, но слишком мелкая — тот же текст, что у оригинала.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn download_rejects_too_small_image() {
    let _lang = i18n_pin::ru();
    let (got, _) = run("mate");
    assert_eq!(got, Err("Не удалось загрузить скриншот: Файл слишком мал".to_string()));
}

/// 403 на прямом адресе не повторяется (доступ закрыт — второй раз будет то
/// же), а ответ API оказался не объектом → отказ с причиной первой неудачи.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn download_forbidden_is_not_retried() {
    let _lang = i18n_pin::ru();
    let (got, io) = run("budgie");
    assert_eq!(got, Err("Не удалось загрузить скриншот: HTTP 403".to_string()));
    assert_eq!(io.urls.len(), 2);
    assert!(io.slept.is_empty(), "403 повторять бессмысленно");
}

// ============================================================================
// Язык интерфейса
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Подписи снимков и неопознанный стол по-английски — без русских букв;
/// после английского предлога неопознанный стол — «your desktop».
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

/// Те же подписи по-русски — дословно как до перевода на каталог (снимок
/// `ca42e74^`), падежи неопознанного стола — прежние.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
