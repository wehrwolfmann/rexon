//! Guard of the rail labels of the «Wallpaper Engine» tab: the reference is
//! the VENDOR waywallen translation, not our own copy of the list.
//!
//! WHY. The rail pages are foreign (`vendor: true`), waywallen draws them.
//! There is one yardstick for a foreign tab: parity with the original. And the
//! `we_window_parity.rs` check compares our list with a copy of the same list
//! WRITTEN OUT NEXT TO IT — i.e. us with ourselves. With exactly this
//! yardstick the tab acceptance stayed green for three weeks on an empty
//! wallpaper properties panel: the loss was identical in both snapshots.
//!
//! Here the reference lies OUTSIDE our code — in the vendor
//! `ui/i18n/waywallen_ru.ts`, context `Window`, where waywallen itself takes
//! its rail labels from. Should our names diverge from theirs on the next
//! vendor update, this test goes red, not the owner's eye.
//!
//! WHAT IS NOT HERE. The «Developer» item is our own, it has no reference and
//! cannot have one; only items with a vendor page are checked.

use rexon_core::we_window as w;

mod i18n_pin;

/// The vendor translation: the only source of reference labels.
const VENDOR_TS: &str = include_str!(
    "../../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/i18n/waywallen_ru.ts"
);

/// The English original label for each vendor rail item.
///
/// The «our id ↔ foreign string» link is the only thing we write by hand
/// here: the Russian words themselves come from the translation.
const RAIL_SOURCES: &[(&str, &str)] = &[
    ("wallpapers_qml", "Wallpapers"),
    ("discover_qml", "Discover"),
    ("displays_qml", "Displays"),
    ("status_qml", "Status"),
    ("plugins_qml", "Plugins"),
    ("settings_qml", "Settings"),
    ("about_qml", "About"),
];

/// How many translated messages are known to exist in the `Window` context.
///
/// Insurance of the PARSING ITSELF: should the search break, the dictionary
/// would come out empty, and «no divergences found» would be a truth about nothing.
const KNOWN_WINDOW_MESSAGES: usize = 10;

/// Cut out the content of the first `<tag>…</tag>` starting at a position.
fn tag_body<'a>(hay: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = hay.find(&open)?;
    let body_start = start + hay[start..].find('>')? + 1;
    let end = hay[body_start..].find(&close)? + body_start;
    Some(&hay[body_start..end])
}

/// Expand the few XML entities that occur in labels.
fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// The «English original → Russian translation» dictionary of the `Window` context.
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
        // An unfinished translation does not count as a reference.
        if tr.trim().is_empty() {
            continue;
        }
        out.push((unescape_xml(src.trim()), unescape_xml(tr.trim())));
    }
    out
}

#[test]
fn vendor_rail_labels_are_the_reference() {
    // The reference is the Russian waywallen translation, so we take our labels
    // in Russian too: from `po/ru.po`, as a Russian buyer will see them.
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

/// In English the item label is exactly the original waywallen string: our
/// `msgid` is the vendor `source`, otherwise an English buyer would see one
/// word in the rail and another in the page's own title.
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
    // Should an eighth FOREIGN page appear, its reference must be added here
    // too, otherwise the guard will silently skip the new label.
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
// Vendor page header: the reference is the vendor page ITSELF
// ════════════════════════════════════════════════════════════════════════
//
// WHY. The page title and ALL its action buttons live in the header
// (`MD.AppBar`), and `MD.Page` draws the header by `MD.MProp.page.showHeader`
// — a property of the HOST, not its own (`Qcm.Material/qml/control/Page.qml`,
// line 25). waywallen has two hosts: the rail page container
// (`ui/qml/Window.qml`) sets `showHeader: false`, the popup window
// (`component/PagePopup.qml`) — `showHeader: true`. So rail pages either have
// no header or turn it on FOR THEMSELVES, and popup pages do not rely on
// their own `showHeader` at all.
//
// Our rail holds both kinds. Should we get the `header` flag wrong, the
// vendor page opens without a title and without all its buttons: for
// «Plugins» these are «Inactive plugins», «Check for updates» and «Install
// from .zip», i.e. all plugin installation and updating. The plugin list is
// still drawn, and the snapshot looks working — exactly how the defect lived
// from commit a200237 until the live snapshot of 08.09.2026.
//
// ⛔ The reference is NOT written out as a list nearby: that would again
// compare us with ourselves. It is read from the vendor page ITSELF — by the
// rule waywallen follows in all its pages: whoever owns the header turns it on.

/// The waywallen vendor page directory.
const VENDOR_PAGE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/qml/page"
);

/// A top-level property of a vendor page: in waywallen the page root is
/// indented by exactly four spaces, and everything nested deeper. The indent
/// here is the sign of «own, not of a foreign control»: `actions:` also occurs
/// inside the page (e.g. in its own `MD.AppBar` in `WallpaperPage.qml`), and
/// that has nothing to do with the header.
fn has_root_property(src: &str, name: &str) -> bool {
    let head = format!("    {name}:");
    src.lines().any(|l| l.starts_with(&head))
}

/// Parse a vendor page: (root is `MD.Page`, whether it sets `showHeader`
/// itself, whether it has a title or action buttons).
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
    // Insurance of the parsing itself: the rule proves something only if both
    // sides were found in the rail — a page turning the header on for itself,
    // and a page waiting for it from the host. Should the parsing break, both
    // counts would become zero, and the check honestly goes red instead of «all matched».
    let mut own = 0;
    let mut hosted = 0;

    for item in specs.iter().filter(|i| i.vendor) {
        let (is_page, own_header, has_bar_content) = read_vendor_page(&item.qml);
        assert!(
            is_page,
            "корень вендорной страницы {} — не MD.Page; правило про шапку              к ней неприменимо, разбирать надо заново",
            item.qml
        );

        // A page waits for the header from the host if it has something to show in
        // it and does not turn it on for itself.
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
    // The header is vendor (`MD.AppBar` of their `MD.Page`). Our page
    // («Developer», the «modules not built» stub) has no one to ask for it: the
    // `MD.MProp.page` context lives in the shell around a VENDOR page.
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
