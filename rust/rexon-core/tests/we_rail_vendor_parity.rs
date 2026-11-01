//! Сторож подписей рейла вкладки «Wallpaper Engine»: эталон — ВЕНДОРНЫЙ
//! перевод waywallen, а не наша же копия списка.
//!
//! ЗАЧЕМ. Страницы рейла — чужие (`vendor: true`), их рисует waywallen.
//! Мерило для чужой вкладки одно: паритет с оригиналом. А сверка
//! `we_window_parity.rs` сравнивает наш список с ВЫПИСАННОЙ РЯДОМ копией
//! того же списка — то есть нас с нами же. Ровно этим мерилом приёмка
//! вкладок три недели зеленела на пустой панели свойств обоев: пропажа
//! была одинаковой в обоих снимках.
//!
//! Здесь эталон лежит СНАРУЖИ нашего кода — в вендорном
//! `ui/i18n/waywallen_ru.ts`, контекст `Window`, откуда сам waywallen
//! берёт подписи своего рейла. Разойдись наши названия с чужими при
//! следующем обновлении вендора — краснеет этот тест, а не глаз владельца.
//!
//! ЧЕГО ЗДЕСЬ НЕТ. Пункт «Разработчик» — наш собственный, эталона у него
//! нет и быть не может; проверяются только пункты с вендорной страницей.

use rexon_core::we_window as w;

mod i18n_pin;

/// Вендорный перевод: единственный источник эталонных подписей.
const VENDOR_TS: &str = include_str!(
    "../../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/i18n/waywallen_ru.ts"
);

/// Английский оригинал подписи для каждого вендорного пункта рейла.
///
/// Связь «наш идентификатор ↔ чужая строка» — единственное, что мы тут
/// пишем руками: сами русские слова берутся из перевода.
const RAIL_SOURCES: &[(&str, &str)] = &[
    ("wallpapers_qml", "Wallpapers"),
    ("discover_qml", "Discover"),
    ("displays_qml", "Displays"),
    ("status_qml", "Status"),
    ("plugins_qml", "Plugins"),
    ("settings_qml", "Settings"),
    ("about_qml", "About"),
];

/// Сколько переведённых сообщений заведомо есть в контексте `Window`.
///
/// Страховка САМОГО РАЗБОРА: сломайся поиск — и словарь вышел бы пустым,
/// а «расхождений не найдено» стало бы правдой ни о чём.
const KNOWN_WINDOW_MESSAGES: usize = 10;

/// Вырезать содержимое первого тега `<tag>…</tag>` начиная с позиции.
fn tag_body<'a>(hay: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = hay.find(&open)?;
    let body_start = start + hay[start..].find('>')? + 1;
    let end = hay[body_start..].find(&close)? + body_start;
    Some(&hay[body_start..end])
}

/// Развернуть те немногие сущности XML, что встречаются в подписях.
fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Словарь «английский оригинал → русский перевод» контекста `Window`.
fn window_translations() -> Vec<(String, String)> {
    let ctx = VENDOR_TS
        .split("<context>")
        .find(|c| c.contains("<name>Window</name>"))
        .expect("в вендорном переводе нет контекста Window");

    let mut out = Vec::new();
    for msg in ctx.split("<message>").skip(1) {
        let (Some(src), Some(tr)) = (tag_body(msg, "source"), tag_body(msg, "translation")) else {
            continue;
        };
        // Незаконченный перевод эталоном не считается.
        if tr.trim().is_empty() {
            continue;
        }
        out.push((unescape_xml(src.trim()), unescape_xml(tr.trim())));
    }
    out
}

#[test]
fn vendor_rail_labels_are_the_reference() {
    // Эталон — русский перевод waywallen, значит и наши подписи берём
    // по-русски: из `po/ru.po`, как их увидит русский покупатель.
    let _lang = i18n_pin::ru();
    let dict = window_translations();
    assert!(
        dict.len() >= KNOWN_WINDOW_MESSAGES,
        "разбор перевода дал всего {} сообщений — сломан сам разбор, \
         а не эталон",
        dict.len()
    );

    let specs = w::qml_page_specs();
    for (id, source) in RAIL_SOURCES {
        let item = specs
            .iter()
            .find(|i| i.id == *id)
            .unwrap_or_else(|| panic!("в рейле нет пункта {id}"));
        assert!(item.vendor, "пункт {id} перестал быть вендорным");

        let expected = dict
            .iter()
            .find(|(s, _)| s == source)
            .map(|(_, t)| t.as_str())
            .unwrap_or_else(|| {
                panic!("в контексте Window нет перевода строки {source:?}")
            });

        assert_eq!(
            item.label, expected,
            "подпись пункта {id} разошлась с вендорным переводом \
             ({source:?}): у нас {:?}, у waywallen {expected:?}",
            item.label
        );
    }
}

/// По-английски подпись пункта — ровно исходная строка waywallen: наш
/// `msgid` и есть вендорный `source`, иначе английский покупатель увидел
/// бы в рейле одно слово, а в заголовке самой страницы — другое.
#[test]
fn english_rail_labels_are_the_vendor_source_strings() {
    let _lang = i18n_pin::en();
    let specs = w::qml_page_specs();
    for (id, source) in RAIL_SOURCES {
        let item = specs
            .iter()
            .find(|i| i.id == *id)
            .unwrap_or_else(|| panic!("в рейле нет пункта {id}"));
        assert_eq!(item.label, *source, "английская подпись пункта {id}");
    }
}

#[test]
fn every_vendor_rail_item_has_a_reference() {
    // Появится восьмая ЧУЖАЯ страница — эталон для неё нужно вписать
    // сюда же, иначе сторож молча пропустит новую подпись.
    let specs = w::qml_page_specs();
    let vendor_ids: Vec<&str> = specs
        .iter()
        .filter(|i| i.vendor)
        .map(|i| i.id.as_str())
        .collect();
    for id in &vendor_ids {
        assert!(
            RAIL_SOURCES.iter().any(|(known, _)| known == id),
            "у вендорного пункта {id} не задан английский оригинал подписи"
        );
    }
    assert_eq!(vendor_ids.len(), RAIL_SOURCES.len());
}

// ════════════════════════════════════════════════════════════════════════
// Шапка вендорной страницы: эталон — САМА вендорная страница
// ════════════════════════════════════════════════════════════════════════
//
// ЗАЧЕМ. Заголовок страницы и ВСЕ её кнопки действий живут в шапке
// (`MD.AppBar`), а шапку `MD.Page` рисует по `MD.MProp.page.showHeader` —
// по свойству ХОЗЯИНА, а не своему (`Qcm.Material/qml/control/Page.qml`,
// строка 25). У waywallen хозяев два: контейнер страниц рейла
// (`ui/qml/Window.qml`) ставит `showHeader: false`, всплывающее окно
// (`component/PagePopup.qml`) — `showHeader: true`. Поэтому страницы рейла
// либо шапки не имеют, либо включают её СЕБЕ САМИ, а страницы
// всплывающего окна на своё `showHeader` не полагаются вовсе.
//
// Наш рейл держит и те и другие. Ошибись мы признаком `header` — вендорная
// страница откроется без заголовка и без всех кнопок: у «Плагинов» это
// «Неактивные плагины», «Проверить обновления» и «Установить из .zip», то
// есть вся установка и всё обновление плагинов. Список плагинов при этом
// рисуется, и снимок выглядит рабочим — ровно так дефект и прожил от
// коммита a200237 до живого снимка 08.09.2026.
//
// ⛔ Эталон НЕ выписывается рядом списком: это снова сверяло бы нас с
// нами. Он читается из САМОЙ вендорной страницы — по правилу, которое
// waywallen соблюдает во всех своих страницах: чья шапка, тот её и
// включает.

/// Каталог вендорных страниц waywallen.
const VENDOR_PAGE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/qml/page"
);

/// Свойство верхнего уровня вендорной страницы: у waywallen корень
/// страницы отбит ровно четырьмя пробелами, а всё вложенное — глубже.
/// Отступ здесь и есть признак «своё, а не чужого органа управления»:
/// `actions:` встречается и внутри страницы (например, у своей же
/// `MD.AppBar` в `WallpaperPage.qml`), и такое к шапке отношения не имеет.
fn has_root_property(src: &str, name: &str) -> bool {
    let head = format!("    {name}:");
    src.lines().any(|l| l.starts_with(&head))
}

/// Разобрать вендорную страницу: (корень — `MD.Page`, ставит ли она себе
/// `showHeader` сама, есть ли у неё заголовок или кнопки действий).
fn read_vendor_page(qml: &str) -> (bool, bool, bool) {
    let path = format!("{VENDOR_PAGE_DIR}/{qml}");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("нет вендорной страницы {path}: {e}"));
    let is_page = src.lines().any(|l| l.trim_start().starts_with("MD.Page {"));
    let own_header = has_root_property(&src, "showHeader");
    let has_bar_content =
        has_root_property(&src, "title") || has_root_property(&src, "actions");
    (is_page, own_header, has_bar_content)
}

#[test]
fn vendor_page_decides_whether_the_host_shows_the_header() {
    let specs = w::qml_page_specs();
    // Страховка САМОГО разбора: правило доказывает что-то, только если в
    // рейле нашлись обе стороны — и страница, включающая шапку себе сама,
    // и страница, которая ждёт её от хозяина. Сломайся разбор — оба счёта
    // станут нулевыми, и сверка честно покраснеет вместо «всё сошлось».
    let mut own = 0;
    let mut hosted = 0;

    for item in specs.iter().filter(|i| i.vendor) {
        let (is_page, own_header, has_bar_content) = read_vendor_page(&item.qml);
        assert!(
            is_page,
            "корень вендорной страницы {} — не MD.Page; правило про шапку              к ней неприменимо, разбирать надо заново",
            item.qml
        );

        // Шапку ждёт от хозяина та страница, которой есть что в ней
        // показать и которая не включает её себе сама.
        let wants_host_header = has_bar_content && !own_header;
        if own_header {
            own += 1;
        }
        if wants_host_header {
            hosted += 1;
        }

        assert_eq!(
            item.header, wants_host_header,
            "пункт {} ({}): у нас header={}, а сама вендорная страница              говорит {} (свой showHeader: {own_header}, есть заголовок или              кнопки: {has_bar_content}). Ошибка сюда — это страница без              заголовка и БЕЗ ВСЕХ КНОПОК действий, при живом виде списка",
            item.id, item.qml, item.header, wants_host_header
        );
    }

    assert!(
        own >= 1 && hosted >= 1,
        "разбор вендорных страниц дал {own} своих шапок и {hosted} чужих —          сломан сам разбор, а не признак: в рейле есть и те и другие"
    );
}

#[test]
fn only_vendor_pages_ask_the_host_for_a_header() {
    // Шапка — вендорная (`MD.AppBar` их `MD.Page`). Нашей странице
    // («Разработчик», заглушка «модули не собраны») просить её не у кого:
    // контекст `MD.MProp.page` живёт в шелле вокруг ВЕНДОРНОЙ страницы.
    for item in w::build_rail(true, true).items.iter() {
        if !item.vendor {
            assert!(
                !item.header,
                "не-вендорный пункт {} просит вендорную шапку",
                item.id
            );
        }
    }
    assert!(!w::no_qml_rail_item().header);
    assert!(!w::dev_rail_item().header);
}
