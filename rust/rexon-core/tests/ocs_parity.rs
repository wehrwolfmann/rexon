//! Golden-сверка OCS-парсера с оригиналом `rexon/appearance/ocs_client.py`.
//!
//! Эталоны сняты с самого Python (см. генератор): `ocs_list.xml` — РЕАЛЬНЫЙ
//! ответ `/content/data` (curl к api.opendesktop.org, categories=104),
//! `ocs_comments.xml` — реальный ответ `/comments/data/1/...`, `ocs_cases.json` —
//! выходы оракула (`_parse_list`/`_flatten_comments`/`_safe_url`/`_link_score`/
//! `_pick_best_link`). Rust обязан вернуть те же модели/строки/порядок.
//!
//! ⚠️ Живой сетевой вызов и SQLite-кеш НЕ тестируются (недетерминированы и в
//! ядро не портированы) — проверяется только РАЗБОР зафиксированных ответов.

use rexon_core::ocs::{
    comments_url, full_preview_url, item_url, link_score, link_score_for, list_key,
    parse_comments, parse_list, pick_best_link, pick_best_link_for, preview_cache_name, safe_url,
    search_url, sort_mode, store_type_is_kpackage, API_BASE, SORT_MODES,
};
use serde_json::Value;

fn cases() -> Value {
    serde_json::from_str(include_str!("golden/ocs_cases.json")).unwrap()
}

/// Эталоны второй волны портов (`full_preview_url`, ключи кеша, URL запросов,
/// имена файлов кеша превью) — сняты с ЖИВОГО `ocs_client.py` на РЕАЛЬНЫХ
/// previewpic-ссылках из `ocs_list.xml`.
fn extra() -> Value {
    serde_json::from_str(include_str!("golden/appearance_extra_cases.json")).unwrap()
}

/// Сторож срока кеша деталей: `DETAIL_TTL_SECS` ядра — сутки, как в
/// эталоне (`detail_ttl_secs` в `ocs_cases.json`; значение снято с прежнего
/// `DETAIL_TTL = 24 * 3600`). Срок читает и кеш каталога
/// (`appearance_cache`), поэтому его смена — изменение поведения, а не
/// косметика: правка числа требует осознанной правки эталона.
///
/// До 02.10.2026 сверка читала замороженный исходник Python
/// (`golden/frozen_ocs_client.py`); Python теперь только архив, и ожидание
/// стало собственным эталоном Rust.
#[test]
fn detail_ttl_matches_the_golden() {
    let want = cases()["detail_ttl_secs"].as_u64().expect("detail_ttl_secs в ocs_cases.json");
    assert_eq!(rexon_core::ocs::DETAIL_TTL_SECS, want);
}

/// Разбор РЕАЛЬНОГО ответа-списка: все поля ThemeItem, download_links,
/// выбранный лучший линк, comment_count и totalitems совпадают с Python.
#[test]
fn parse_list_matches_python() {
    let raw = include_bytes!("golden/ocs_list.xml");
    let (got, total) = parse_list(raw);

    let want = &cases()["parsed_list"];
    assert_eq!(total, want["total"].as_i64().unwrap(), "totalitems");
    let want_items = want["items"].as_array().unwrap();
    assert_eq!(got.len(), want_items.len(), "число элементов");
    assert!(!got.is_empty(), "фикстура не должна быть пустой");

    for (oc, w) in got.iter().zip(want_items) {
        let it = &oc.item;
        assert_eq!(it.id, w["id"].as_str().unwrap(), "id");
        assert_eq!(it.name, w["name"].as_str().unwrap(), "name {}", it.id);
        assert_eq!(it.description, w["description"].as_str().unwrap(), "description {}", it.id);
        assert_eq!(it.author, w["author"].as_str().unwrap(), "author {}", it.id);
        // Поля `license` в ответе нет вовсе — лицензию дают метки (`gplv3`,
        // `cc-by-sa`…); с 04.10.2026 эталон хранит её из меток
        // (`ocs::license_from_tags`), а не пустую строку Python.
        assert_eq!(it.license, w["license"].as_str().unwrap(), "license {}", it.id);
        assert_eq!(it.version, w["version"].as_str().unwrap(), "version {}", it.id);
        assert_eq!(it.rating, w["rating"].as_i64().unwrap(), "rating {}", it.id);
        assert_eq!(it.downloads, w["downloads"].as_i64().unwrap(), "downloads {}", it.id);
        assert_eq!(it.preview_url, w["preview_url"].as_str().unwrap(), "preview_url {}", it.id);
        let want_previews: Vec<&str> =
            w["preview_urls"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(it.preview_urls, want_previews, "preview_urls {}", it.id);
        assert_eq!(it.download_url, w["download_url"].as_str().unwrap(), "download_url {}", it.id);
        assert_eq!(it.download_name, w["download_name"].as_str().unwrap(), "download_name {}", it.id);
        assert_eq!(it.download_size, w["download_size"].as_i64().unwrap(), "download_size {}", it.id);
        assert_eq!(it.category_id, w["category_id"].as_i64().unwrap(), "category_id {}", it.id);
        assert_eq!(it.created, w["created"].as_str().unwrap(), "created {}", it.id);
        assert_eq!(it.updated, w["updated"].as_str().unwrap(), "updated {}", it.id);
        // download_links (полный список dict url/name/size) — побайтово.
        assert_eq!(&it.download_links, w["download_links"].as_array().unwrap(), "download_links {}", it.id);
        // динамический атрибут comment_count.
        assert_eq!(oc.comment_count, w["comment_count"].as_i64().unwrap(), "comment_count {}", it.id);
        // производные методы модели на реальных данных.
        assert_eq!(it.rating_stars(), w["rating_stars"].as_f64().unwrap(), "rating_stars {}", it.id);
        assert_eq!(it.display_description(), w["display_description"].as_str().unwrap(), "display_desc {}", it.id);
    }
}

/// Разбор РЕАЛЬНОГО ответа комментариев: плоский список с глубиной вложенности
/// совпадает с `_flatten_comments`.
#[test]
fn parse_comments_matches_python() {
    let raw = include_bytes!("golden/ocs_comments.xml");
    let got = parse_comments(raw);
    let want = cases()["comments"].as_array().unwrap().clone();
    assert_eq!(got.len(), want.len(), "число комментариев");
    assert!(!got.is_empty(), "фикстура комментариев не пуста");
    for (c, w) in got.iter().zip(&want) {
        assert_eq!(c.author, w["author"].as_str().unwrap(), "author");
        assert_eq!(c.date, w["date"].as_str().unwrap(), "date");
        assert_eq!(c.subject, w["subject"].as_str().unwrap(), "subject");
        assert_eq!(c.text, w["text"].as_str().unwrap(), "text");
        assert_eq!(c.score, w["score"].as_str().unwrap(), "score");
        assert_eq!(c.depth, w["depth"].as_i64().unwrap(), "depth");
    }
}

/// _safe_url: percent-encode небезопасных символов (urlsplit+quote+urlunsplit),
/// уже закодированные `%XX` и fragment сохраняются.
#[test]
fn safe_url_matches_python() {
    for (url, want) in cases()["safe_url"].as_object().unwrap() {
        assert_eq!(&safe_url(url), want.as_str().unwrap(), "safe_url для {url}");
    }
}

/// _link_score / _pick_best_link: оценка пригодности линков и выбор лучшего
/// (первый при равенстве баллов), -1 для пустого списка.
///
/// С 05.10.2026 эталон расходится с Python намеренно: готовые KPackage
/// (`.plasmoid`/`.kpackage`/`.kwinscript`) — архивы, пакеты дистрибутивов и
/// установщики штрафуются, текст (README) — ниже всего. С 19.10.2026 файл
/// дополнений (`extra.7z`, `Extra 1.4.tar.xz`) уступает основному архиву. Случаи с
/// `kpackage_kind` — товар вида, который ставится KPackage-ом.
#[test]
fn link_selection_matches_python() {
    for case in cases()["link_cases"].as_array().unwrap() {
        let links = case["links"].as_array().unwrap();
        let want_scores = case["scores"].as_array().unwrap();
        let kpackage = case["kpackage_kind"].as_bool().unwrap_or(false);
        assert_eq!(links.len(), want_scores.len(), "баллы на каждый линк {case:?}");
        for (l, s) in links.iter().zip(want_scores) {
            assert_eq!(link_score_for(l, kpackage), s.as_i64().unwrap(), "score для {l:?}");
            if !kpackage {
                assert_eq!(link_score(l), s.as_i64().unwrap(), "score без вида для {l:?}");
            }
        }
        let best = pick_best_link_for(links, kpackage);
        assert_eq!(best as i64, case["best"].as_i64().unwrap(), "best для {case:?}");
        if !kpackage {
            assert_eq!(pick_best_link(links), best, "best без вида для {case:?}");
        }
    }
}

/// Какие товары магазина ставятся KPackage-ом: виджеты (зонт 705 и его
/// подкатегории — «HTML Widget Host» приходит с 706), плагины обоев 715,
/// скрипты KWin 720. Иконки и стили Plasma — нет.
#[test]
fn store_types_installed_as_kpackage() {
    for id in [705, 706, 708, 715, 720] {
        assert!(store_type_is_kpackage(id), "typeid {id} должен ставиться KPackage-ом");
    }
    for id in [132, 107, 135, 299, 0] {
        assert!(!store_type_is_kpackage(id), "typeid {id} не KPackage");
    }
}

/// full_preview_url: снятие CDN-сегмента ресайзера pling на РЕАЛЬНЫХ ссылках
/// previewpic из ответа api.opendesktop.org + краевые случаи (no-op, два
/// сегмента → только первый, кириллица/пробелы в имени файла, пустой вход).
#[test]
fn full_preview_url_matches_python() {
    let cases = extra()["full_preview"].as_array().unwrap().clone();
    assert!(cases.len() >= 30, "мало кейсов previewpic");
    let mut changed = 0;
    for c in &cases {
        let url = c["url"].as_str().unwrap();
        let want = c["full"].as_str().unwrap();
        assert_eq!(full_preview_url(url), want, "full_preview_url для {url:?}");
        if url != want {
            changed += 1;
        }
    }
    assert!(changed >= 20, "эталон почти не содержит /cache/-ссылок");
}

/// SORT_MAP.get(sort, "new"): известные режимы проходят как есть, мусор → new.
#[test]
fn sort_mode_matches_python() {
    let g = extra();
    let map = g["sort_map"].as_object().unwrap();
    assert_eq!(map.len(), SORT_MODES.len(), "число режимов сортировки");
    for (k, v) in map {
        assert!(SORT_MODES.contains(&k.as_str()), "режим {k} отсутствует в SORT_MODES");
        assert_eq!(sort_mode(k), v.as_str().unwrap(), "sort_mode для {k}");
    }
    // Ключей вне SORT_MAP в оригинале нет — дефолт проверяем по кейсам list_key.
    for c in g["list_keys"].as_array().unwrap() {
        let sort = c["sort"].as_str().unwrap();
        assert_eq!(sort_mode(sort), c["sortmode"].as_str().unwrap(), "sort_mode для {sort:?}");
    }
}

/// Ключ кеша страницы (`cat…:p…:s…:q…`): strip+lower запроса по Unicode,
/// неизвестная сортировка → new.
#[test]
fn list_key_matches_python() {
    for c in extra()["list_keys"].as_array().unwrap() {
        let cat = c["category_id"].as_i64().unwrap().to_string();
        let page = c["page"].as_i64().unwrap();
        let sort = c["sort"].as_str().unwrap();
        let search = c["search"].as_str().unwrap();
        assert_eq!(
            list_key(&cat, page, sort, search),
            c["key"].as_str().unwrap(),
            "list_key для ({cat},{page},{sort:?},{search:?})"
        );
    }
}

/// URL запросов каталога/деталки/комментариев: порядок параметров и
/// quote_plus-экранирование как у `urllib.parse.urlencode`.
#[test]
fn request_urls_match_python() {
    let g = extra();
    assert_eq!(API_BASE, g["api_base"].as_str().unwrap(), "API_BASE");
    for c in g["search_urls"].as_array().unwrap() {
        let got = search_url(
            c["category_id"].as_str().unwrap(),
            c["page"].as_i64().unwrap(),
            c["pagesize"].as_i64().unwrap(),
            c["sortmode"].as_str().unwrap(),
            c["search"].as_str().unwrap(),
        );
        assert_eq!(got, c["url"].as_str().unwrap(), "search_url для {c:?}");
    }
    for c in g["item_urls"].as_array().unwrap() {
        let cid = c["content_id"].as_str().unwrap();
        assert_eq!(item_url(cid), c["url"].as_str().unwrap(), "item_url для {cid:?}");
    }
    for c in g["comments_urls"].as_array().unwrap() {
        let got = comments_url(
            c["content_id"].as_str().unwrap(),
            c["page"].as_i64().unwrap(),
            c["pagesize"].as_i64().unwrap(),
        );
        assert_eq!(got, c["url"].as_str().unwrap(), "comments_url для {c:?}");
    }
}

/// Имя файла в кеше превью: sha1(url) + расширение из ПУТИ (без query),
/// «расширение» длиннее 5 символов и отсутствующее → `.jpg`.
#[test]
fn preview_cache_name_matches_python() {
    for c in extra()["preview_cache_names"].as_array().unwrap() {
        let url = c["url"].as_str().unwrap();
        assert_eq!(
            preview_cache_name(url),
            c["name"].as_str().unwrap(),
            "preview_cache_name для {url:?}"
        );
    }
}
